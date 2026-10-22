//! Hosted beta analytics from this user's Sculptured: the shape of what they set up and what ran,
//! never names, jobs, messages, folders or commands. Each report is signed with this instance's
//! gateway secret and sent to the gateway (`/internal/analytics`), which allowlists it again.
//! Local mode has no `Analytics`, so nothing is sent.
use crate::state::AppState;
use model::{AgentKind, ConnectionKind, EventBody, Subject, TeamDraft};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct Analytics {
    tx: tokio::sync::mpsc::Sender<(&'static str, Value)>,
    saved: Mutex<HashMap<String, Instant>>,
}

/// `team.saved` is reported at most this often per team (drafts save on every edit).
const SAVED_EVERY: Duration = Duration::from_secs(600);

impl Analytics {
    /// Starts the sender: reports go to `<gateway>/internal/analytics` signed as `user`.
    #[must_use]
    pub fn start(gateway: String, user: String, secret: Arc<Vec<u8>>) -> Arc<Analytics> {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<(&'static str, Value)>(100);
        // In the full build reqwest has rustls with no provider chosen; building a client then panics.
        // The same provider as the runner's link and the pairing client (installing twice is harmless).
        let _ = rustls::crypto::ring::default_provider().install_default();
        tokio::spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("http client");
            let url = format!("{}/internal/analytics", gateway.trim_end_matches('/'));
            while let Some((name, props)) = rx.recv().await {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let sig = signing::sign(&secret, "POST", "/internal/analytics", &user, ts);
                let sent = client
                    .post(&url)
                    .header(signing::USER_HEADER, &user)
                    .header(signing::TS_HEADER, ts.to_string())
                    .header(signing::SIG_HEADER, sig)
                    .json(&json!({ "name": name, "props": props }))
                    .send()
                    .await;
                if let Err(e) = sent {
                    tracing::debug!(name, error = %e, "analytics report not sent");
                }
            }
        });
        Arc::new(Analytics {
            tx,
            saved: Mutex::new(HashMap::new()),
        })
    }

    /// Queues a report; dropped (never blocking) when the queue is full.
    pub fn emit(&self, name: &'static str, props: Value) {
        let _ = self.tx.try_send((name, props));
    }

    /// `team.saved`, at most once per team per ten minutes.
    pub fn saved(&self, team: &str, props: Value) {
        let mut last = self.saved.lock().expect("saved mutex must not be poisoned");
        if last.get(team).is_some_and(|t| t.elapsed() < SAVED_EVERY) {
            return;
        }
        last.insert(team.to_string(), Instant::now());
        drop(last);
        self.emit("team.saved", props);
    }
}

/// Reports `name` if this Sculptured has analytics (hosted), else nothing.
pub fn emit(st: &AppState, name: &'static str, props: Value) {
    if let Some(a) = st.analytics() {
        a.emit(name, props);
    }
}

/// A template id as reported (anything not built in is "other").
#[must_use]
pub fn template(id: &str) -> &'static str {
    const KNOWN: &[&str] = &[
        "solo",
        "builder-reviewer",
        "planner-builders",
        "blank",
        "textbook",
        "support-triage",
        "essay-committee",
        "design-review",
    ];
    KNOWN.iter().find(|k| **k == id).copied().unwrap_or("other")
}

/// The shape of a team: counts and yes/no only.
#[must_use]
pub fn shape(d: &TeamDraft, own_copies: bool) -> Value {
    let kind = |k: AgentKind| d.agents.iter().filter(|a| a.kind == k).count();
    let arrows = |k: ConnectionKind| d.connections.iter().filter(|c| c.kind == k).count();
    json!({
        "agents": d.agents.len(),
        "claude": kind(AgentKind::Claude),
        "codex": kind(AgentKind::Codex),
        "practice": kind(AgentKind::Practice),
        "groups": d.groups.len(),
        "arrows_hands": arrows(ConnectionKind::HandsWorkTo),
        "arrows_reports": arrows(ConnectionKind::ReportsTo),
        "arrows_watches": arrows(ConnectionKind::Watches),
        "token_limits": d.agents.iter().any(|a| a.advanced.token_limit.is_some()),
        "checks": d.agents.iter().any(|a| a.advanced.check.as_deref().is_some_and(|c| !c.trim().is_empty())),
        "copies": own_copies,
        "workflow": d.workflow.is_some(),
    })
}

/// What a start reports: how many agents, which runtimes, how many distinct models, and the shape.
#[must_use]
pub fn started(d: &TeamDraft, own_copies: bool) -> Value {
    let runtimes: BTreeSet<&str> = d
        .agents
        .iter()
        .map(|a| match a.kind {
            AgentKind::Claude => "claude-code",
            AgentKind::Codex => "codex",
            AgentKind::Practice => "stub",
        })
        .collect();
    let models: BTreeSet<String> = d
        .agents
        .iter()
        .filter_map(|a| d.effective_model(a))
        .collect();
    json!({ "agents": d.agents.len(), "runtimes": runtimes, "models": models.len(), "shape": shape(d, own_copies) })
}

fn own_copies(st: &AppState, team: &str) -> bool {
    st.store
        .lock()
        .expect("store mutex must not be poisoned")
        .team_settings(team)
        .map(|s| s.own_copies)
        .unwrap_or(false)
}

/// `team.saved` for a team just saved (throttled).
pub fn saved(st: &AppState, d: &TeamDraft) {
    if let Some(a) = st.analytics() {
        a.saved(&d.id, json!({ "shape": shape(d, own_copies(st, &d.id)) }));
    }
}

/// `team.started` for a team just started.
pub fn team_started(st: &AppState, d: &TeamDraft) {
    if st.analytics().is_some() {
        emit(st, "team.started", started(d, own_copies(st, &d.id)));
    }
}

/// A finished run's numbers: duration, tokens, agents, spawned agents, failed agents.
fn run_summary(st: &AppState, team: &str) -> Option<Value> {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let seq = store.latest_run_start(team).ok()??;
    let (run, end) = crate::collaboration::history::bounds(&store, team, seq).ok()?;
    let detail = crate::collaboration::history::load_detail(&store, team, seq).ok()?;
    let tokens = store.run_tokens(team, seq, end).ok()?;
    let stopped = detail.stopped_ms.unwrap_or(run.started_ms);
    Some(json!({
        "duration_s": ((stopped - run.started_ms).max(0) / 1000),
        "tokens_in": tokens.input,
        "tokens_out": tokens.output,
        "agents": detail.agents.len(),
        "spawned": detail.agents.iter().filter(|a| a.parent.is_some()).count(),
        "errors": detail.agents.iter().filter(|a| a.lifecycle == Some(model::SeatLifecycle::Failed)).count(),
    }))
}

/// Turns recorded events into reports: each check's result, and a summary when a run stops.
pub fn watch(st: &AppState) {
    if st.analytics().is_none() {
        return;
    }
    let st = st.clone();
    let mut rx = st.hub.subscribe();
    tokio::spawn(async move {
        loop {
            let item = match rx.recv().await {
                Ok(i) => i,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => return,
            };
            match (&item.event.subject, &item.event.body) {
                (_, EventBody::CheckRan { passed, .. }) => {
                    emit(&st, "check.ran", json!({ "passed": passed }))
                }
                (
                    Subject::Rig(rig),
                    EventBody::RigDesiredChanged {
                        desired: model::Desired::Stopped,
                    },
                ) => {
                    if let Some(s) = run_summary(&st, rig) {
                        emit(&st, "run.summary", s);
                    }
                }
                _ => {}
            }
        }
    });
}
