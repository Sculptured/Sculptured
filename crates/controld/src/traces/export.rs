//! Sends the runs of teams with export on, every few seconds: the events since the last send become
//! spans, posted as OTLP/HTTP JSON. A failed send keeps its spans for the next try and shows the error
//! in the team's setting. What was sent is remembered, so a restart rebuilds the run without resending.
use super::settings::{Content, TraceExport};
use super::spans::{Builder, Ctx, Span, otlp};
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
}

/// One tick at a time (the loop and a test may both call it).
static RUNS: LazyLock<tokio::sync::Mutex<HashMap<String, Run>>> = LazyLock::new(Default::default);

pub async fn run(st: AppState) {
    let mut every = tokio::time::interval(EVERY);
    loop {
        every.tick().await;
        tick(&st).await;
    }
}

/// Sends what is new for every team with export on.
pub async fn tick(st: &AppState) {
    let mut runs = RUNS.lock().await;
    let Ok(teams) = st.store.lock().unwrap().trace_export_teams() else { return };
    for team in teams {
        if let Err(e) = team_tick(st, &mut runs, &team).await {
            tracing::debug!(%team, error = %e, "traces: cannot read the run");
        }
    }
}

async fn team_tick(st: &AppState, runs: &mut HashMap<String, Run>, team: &str) -> rusqlite::Result<()> {
    let (cfg, start, draft) = {
        let s = st.store.lock().unwrap();
        (s.trace_export(team)?, crate::handoffs::current_start(&s, team)?, s.draft(team)?)
    };
    let (Some((seq, stop)), Some(draft)) = (start, draft) else { return Ok(()) };
    if runs.get(team).is_none_or(|r| r.start != seq) {
        let ctx = Ctx {
            team: team.to_string(),
            team_name: draft.name.clone(),
            start_seq: seq,
            names: draft.agents.iter().map(|a| (a.id.clone(), a.name.clone())).collect(),
            full: cfg.content == Content::Full,
        };
        let sent_before = cfg.sent_run == Some(seq);
        runs.insert(team.to_string(), Run {
            start: seq,
            builder: Builder::new(ctx),
            from: seq,
            replay_through: if sent_before { cfg.sent_through.unwrap_or(0) } else { 0 },
            last_ms: 0,
            pending: vec![],
            dropped: 0,
            finished: sent_before && cfg.run_sent,
            done: sent_before && cfg.run_sent,
        });
    }
    let run = runs.get_mut(team).expect("inserted above");
    if run.done {
        return Ok(());
    }
    let read_from = run.from;
    loop {
        let rows = st.store.lock().unwrap().run_events(team, run.from, i64::MAX, CHUNK)?;
        for (s, raw) in &rows {
            run.from = s + 1;
            let Ok(e) = serde_json::from_str::<EventRecord>(raw) else { continue };
            run.last_ms = run.last_ms.max(e.at_ms);
            let spans = run.builder.feed(&e);
            if *s > run.replay_through {
                run.pending.extend(spans);
            }
        }
        if (rows.len() as i64) < CHUNK {
            break;
        }
    }
    if !run.finished && let Some(stop) = stop {
        let end_ms = st.store.lock().unwrap().event_at_ms(stop)?.unwrap_or_else(model::now_ms);
        if run.builder.open_spans() == 0 || model::now_ms() - end_ms > GRACE_MS {
            run.pending.push(run.builder.finish_run(run.last_ms.max(end_ms)));
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
    let hosted = matches!(st.auth, crate::auth::AuthPolicy::Gateway { .. });
    let mut sent = false;
    while !run.pending.is_empty() {
        let n = run.pending.len().min(BATCH);
        match send(&cfg, hosted, &run.pending[..n]).await {
            Ok(()) => {
                run.pending.drain(..n);
                sent = true;
            }
            Err(e) => {
                note(st, team, |t| t.last_error = Some(e));
                return Ok(());
            }
        }
    }
    let (through, finished, dropped) = (run.from - 1, run.finished, std::mem::take(&mut run.dropped));
    run.done = finished;
    note(st, team, |t| {
        t.sent_run = Some(seq);
        t.sent_through = Some(through);
        t.run_sent = finished;
        t.last_error = (dropped > 0).then(|| format!("{dropped} spans were dropped while the collector could not be reached"));
        if sent {
            t.last_sent_ms = Some(model::now_ms());
        }
    });
    Ok(())
}

/// Changes the saved setting as it is now (a save made during the send stays).
fn note(st: &AppState, team: &str, change: impl FnOnce(&mut TraceExport)) {
    let mut store = st.store.lock().unwrap();
    let Ok(mut t) = store.trace_export(team) else { return };
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
        let b = reqwest::Client::builder().timeout(Duration::from_secs(15)).redirect(reqwest::redirect::Policy::none());
        if hosted { b.dns_resolver(PublicOnly) } else { b }.build().expect("http client")
    };
    if hosted { HOSTED.get_or_init(build) } else { LOCAL.get_or_init(build) }
}

/// Resolves a name and keeps only its public addresses, so a name pointing inside the server's
/// network cannot be used to reach it.
struct PublicOnly;

impl reqwest::dns::Resolve for PublicOnly {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host((host.as_str(), 0)).await?.filter(|a| super::settings::is_public(a.ip())).collect();
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
    let mut req = client(hosted).post(&cfg.endpoint).header("content-type", "application/json").body(otlp(spans).to_string());
    if let Some(key) = &cfg.key {
        req = req.header("authorization", format!("Bearer {key}"));
    }
    let host = reqwest::Url::parse(&cfg.endpoint).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
    let res = req.send().await.map_err(|e| format!("Could not reach {host}: {}", if e.is_timeout() { "it did not answer in time".to_string() } else { "the connection failed".to_string() }))?;
    match res.status() {
        s if s.is_success() => Ok(()),
        s if s == reqwest::StatusCode::UNAUTHORIZED || s == reqwest::StatusCode::FORBIDDEN => Err(format!("{host} refused the API key ({s})")),
        s => Err(format!("{host} answered {s}")),
    }
}
