//! Sends the runs of teams with export on, every few seconds: the events since the last send become
//! spans, posted as OTLP/HTTP JSON. A failed send keeps its spans for the next try and shows the error
//! in the team's setting. What was sent is remembered, so a restart rebuilds the run without resending.
use super::settings::{Content, TraceExport};
use super::spans::{Builder, Ctx, Span};
use crate::state::AppState;
use model::EventRecord;
use std::collections::HashMap;
use std::sync::{LazyLock, OnceLock};
use std::time::Duration;

const EVERY: Duration = Duration::from_secs(5);
const CHUNK: i64 = 500;
const BATCH: usize = 500;
/// Spans kept for a collector that is down; older ones go first, and the setting says so.
const MAX_PENDING: usize = 20_000;
/// How long after a stop the run waits for its agents' sessions to end before its own span goes.
const GRACE_MS: i64 = 60_000;

struct Run {
    start: i64,
    builder: Builder,
    /// The next event to read.
    from: i64,
    /// Events up to here were sent before a restart: they rebuild the run and send nothing.
    replay_through: i64,
    last_ms: i64,
    pending: Vec<Span>,
    dropped: usize,
    finished: bool,
    done: bool,
    /// Sends failed in a row, and when the next may be tried: a collector that is down is tried less
    /// and less often (up to every 5 minutes), so it never slows the other teams' sends.
    failures: u32,
    next_try_ms: i64,
}

/// One tick at a time (the loop and a test may both call it). Keyed by the server's store as well as
/// the team, so servers sharing a process (tests) never mix their runs.
static RUNS: LazyLock<tokio::sync::Mutex<HashMap<(usize, String), Run>>> =
    LazyLock::new(Default::default);

pub async fn run(st: AppState) {
    let mut every = tokio::time::interval(EVERY);
    loop {
        every.tick().await;
        tick(&st).await;
    }
}

/// Sends what is new for every team with export on.
pub async fn tick(st: &AppState) {
    let mut all = RUNS.lock().await;
    let Ok(teams) = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .trace_export_teams()
    else {
        return;
    };
    let server = std::sync::Arc::as_ptr(&st.store) as usize;
    for team in teams {
        let mut runs: HashMap<String, Run> = all
            .remove(&(server, team.clone()))
            .map(|r| (team.clone(), r))
            .into_iter()
            .collect();
        let result = team_tick(st, &mut runs, &team).await;
        all.extend(runs.into_iter().map(|(t, r)| ((server, t), r)));
        if let Err(e) = result {
            tracing::debug!(%team, error = %e, "traces: cannot read the run");
        }
    }
}

async fn team_tick(
    st: &AppState,
    runs: &mut HashMap<String, Run>,
    team: &str,
) -> rusqlite::Result<()> {
    let (cfg, start, draft) = {
        let s = st.store.lock().expect("store mutex must not be poisoned");
        (
            s.trace_export(team)?,
            crate::handoffs::current_start(&s, team)?,
            s.draft(team)?,
        )
    };
    let (Some((seq, stop)), Some(draft)) = (start, draft) else {
        return Ok(());
    };
    if runs.get(team).is_none_or(|r| r.start != seq) {
        // The run before (stopped or started again since the last tick) goes whole: its last events,
        // its open spans closed, and its own span, ahead of the new run's.
        let carried = match runs.remove(team) {
            Some(mut old) if !old.done => {
                read(st, team, &mut old, seq)?;
                let end = old.last_ms;
                old.pending.extend(old.builder.close_open(end));
                if !old.finished {
                    old.pending.push(old.builder.finish_run(end));
                }
                old.pending
            }
            _ => vec![],
        };
        let ctx = Ctx {
            team: team.to_string(),
            team_name: draft.name.clone(),
            start_seq: seq,
            names: draft
                .agents
                .iter()
                .map(|a| (a.id.clone(), a.name.clone()))
                .collect(),
            // Texts are always kept; whether they go is decided at each send (`content` can change).
            full: true,
        };
        let sent_before = cfg.sent_run == Some(seq);
        runs.insert(
            team.to_string(),
            Run {
                start: seq,
                builder: Builder::new(ctx),
                from: seq,
                replay_through: if sent_before {
                    cfg.sent_through.unwrap_or(0)
                } else {
                    0
                },
                last_ms: 0,
                pending: carried,
                dropped: 0,
                finished: sent_before && cfg.run_sent,
                done: false,
                failures: 0,
                next_try_ms: 0,
            },
        );
    }
    let run = runs.get_mut(team).expect("inserted above");
    if run.done && run.pending.is_empty() {
        return Ok(());
    }
    let read_from = run.from;
    if !run.finished {
        read(st, team, run, i64::MAX)?;
    }
    if !run.finished
        && let Some(stop) = stop
    {
        let end_ms = st
            .store
            .lock()
            .expect("store mutex must not be poisoned")
            .event_at_ms(stop)?
            .unwrap_or_else(model::now_ms);
        if run.builder.open_spans() == 0 || model::now_ms() - end_ms > GRACE_MS {
            let end = run.last_ms.max(end_ms);
            run.pending.extend(run.builder.close_open(end));
            run.pending.push(run.builder.finish_run(end));
            run.finished = true;
        }
    }
    if run.pending.len() > MAX_PENDING {
        let over = run.pending.len() - MAX_PENDING;
        run.pending.drain(..over);
        run.dropped += over;
    }
    if run.pending.is_empty() && run.from == read_from && !run.finished {
        return Ok(());
    }
    if model::now_ms() < run.next_try_ms {
        return Ok(());
    }
    let hosted = matches!(st.auth, crate::auth::AuthPolicy::Gateway { .. });
    let mut sent = false;
    while !run.pending.is_empty() {
        let n = run.pending.len().min(BATCH);
        match send(&cfg, hosted, &run.pending[..n]).await {
            Ok(()) => {
                run.pending.drain(..n);
                sent = true;
                run.failures = 0;
            }
            Err(e) => {
                run.failures += 1;
                run.next_try_ms = model::now_ms() + (5_000i64 << run.failures.min(6)).min(300_000);
                note(st, team, |t| t.last_error = Some(e));
                return Ok(());
            }
        }
    }
    let (through, finished, dropped) =
        (run.from - 1, run.finished, std::mem::take(&mut run.dropped));
    run.done = finished;
    note(st, team, |t| {
        t.sent_run = Some(seq);
        t.sent_through = Some(through);
        t.run_sent = finished;
        t.last_error = (dropped > 0).then(|| {
            format!("{dropped} spans were dropped while the collector could not be reached")
        });
        if sent {
            t.last_sent_ms = Some(model::now_ms());
        }
    });
    Ok(())
}

/// Feeds the run's events from `run.from` up to (not including) `end`, keeping the spans they end.
fn read(st: &AppState, team: &str, run: &mut Run, end: i64) -> rusqlite::Result<()> {
    loop {
        let rows = st
            .store
            .lock()
            .expect("store mutex must not be poisoned")
            .run_events(team, run.from, end, CHUNK)?;
        for (s, raw) in &rows {
            run.from = s + 1;
            let Ok(e) = serde_json::from_str::<EventRecord>(raw) else {
                continue;
            };
            run.last_ms = run.last_ms.max(e.at_ms);
            let spans = run.builder.feed(&e);
            if *s > run.replay_through {
                run.pending.extend(spans);
            }
        }
        if (rows.len() as i64) < CHUNK {
            return Ok(());
        }
    }
}

/// Changes the saved setting as it is now (a save made during the send stays).
fn note(st: &AppState, team: &str, change: impl FnOnce(&mut TraceExport)) {
    let mut store = st.store.lock().expect("store mutex must not be poisoned");
    let Ok(mut t) = store.trace_export(team) else {
        return;
    };
    change(&mut t);
    if let Err(e) = store.save_trace_export(team, &t) {
        tracing::warn!(%team, error = %e, "traces: cannot save the export state");
    }
}

/// The client for this server: on the hosted server, names resolve only to public addresses.
fn client(hosted: bool) -> &'static reqwest::Client {
    static LOCAL: OnceLock<reqwest::Client> = OnceLock::new();
    static HOSTED: OnceLock<reqwest::Client> = OnceLock::new();
    let build = || {
        // See analytics: reqwest's rustls needs a provider chosen before a client is built.
        let _ = rustls::crypto::ring::default_provider().install_default();
        // No redirects: the key goes only to the address the team chose.
        let b = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none());
        // On the hosted server no proxy either: it would resolve the name itself, past `PublicOnly`.
        if hosted {
            b.dns_resolver(PublicOnly).no_proxy()
        } else {
            b
        }
        .build()
        .expect("http client")
    };
    if hosted {
        HOSTED.get_or_init(build)
    } else {
        LOCAL.get_or_init(build)
    }
}

/// Resolves a name and keeps only its public addresses, so a name pointing inside the server's
/// network cannot be used to reach it.
struct PublicOnly;

impl reqwest::dns::Resolve for PublicOnly {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(|a| super::settings::is_public(a.ip()))
                .collect();
            if addrs.is_empty() {
                return Err(format!("{host} has no public address").into());
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

async fn send(cfg: &TraceExport, hosted: bool, spans: &[Span]) -> Result<(), String> {
    if !super::settings::endpoint_ok(&cfg.endpoint, hosted) {
        return Err("This address cannot be used here; choose a public https:// address".into());
    }
    let body = if cfg.content == Content::Full {
        crate::traces::spans::otlp(spans)
    } else {
        crate::traces::spans::otlp(&crate::traces::spans::without_texts(spans.to_vec()))
    };
    let mut req = client(hosted)
        .post(&cfg.endpoint)
        .header("content-type", "application/json")
        .body(body.to_string());
    if let Some(key) = &cfg.key {
        req = req.header("authorization", format!("Bearer {key}"));
    }
    let host = reqwest::Url::parse(&cfg.endpoint)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_default();
    let res = req.send().await.map_err(|e| {
        format!(
            "Could not reach {host}: {}",
            if e.is_timeout() {
                "it did not answer in time".to_string()
            } else {
                "the connection failed".to_string()
            }
        )
    })?;
    match res.status() {
        s if s.is_success() => Ok(()),
        s if s == reqwest::StatusCode::UNAUTHORIZED || s == reqwest::StatusCode::FORBIDDEN => {
            Err(format!("{host} refused the API key ({s})"))
        }
        s => Err(format!("{host} answered {s}")),
    }
}
