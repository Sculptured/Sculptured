//! Driving one external run: while the run's team works, each usage report its agents' CLIs make is
//! passed to Arena; the run is finished when the operator asks, when the team stops (Stop team,
//! Emergency stop), or when every agent has finished its turns and nothing waits on it for the settle
//! time (the board's "every agent has finished" rule). Finishing records each agent's end with its
//! totals, asks Arena to freeze and score the world, then stops the team.
use super::client;
use super::plan::{Totals, usage_report};
use crate::arena::Arena;
use crate::state::AppState;
use model::{EventRecord, SeatActivity, SeatLifecycle};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// How often the run's new events are read.
const POLL: Duration = Duration::from_millis(500);

/// Where a driven run stands. `Finished` carries Arena's own outcome (evidence).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum DriveState {
    Driving,
    Finishing {
        why: String,
    },
    Finished {
        why: String,
        status: Option<String>,
        error: Option<String>,
    },
    Failed {
        why: String,
    },
}

/// One run Sculptured drives: the Arena run and branch, the run's team and the board it came from.
pub struct Drive {
    pub run: String,
    pub branch: String,
    pub team: String,
    pub board: String,
    pub task: String,
    /// Board agent id by the run team's seat.
    pub agents: Vec<(String, String)>,
    pub state: Mutex<DriveState>,
    finish: watch::Sender<bool>,
}

impl Drive {
    #[must_use]
    pub fn new(
        started: client::Started,
        team: String,
        board: String,
        task: String,
        agents: Vec<(String, String)>,
    ) -> Arc<Drive> {
        let (finish, _) = watch::channel(false);
        let state = Mutex::new(DriveState::Driving);
        Arc::new(Drive {
            run: started.run,
            branch: started.branch,
            team,
            board,
            task,
            agents,
            state,
            finish,
        })
    }

    /// Asks the loop to finish the run now.
    pub fn ask_finish(&self) {
        self.finish.send_replace(true);
    }

    pub fn state(&self) -> DriveState {
        self.state
            .lock()
            .expect("drive state mutex must not be poisoned")
            .clone()
    }

    fn set(&self, s: DriveState) {
        *self
            .state
            .lock()
            .expect("drive state mutex must not be poisoned") = s;
    }
}

/// The runs driven on one arenad, by branch.
#[derive(Default)]
pub struct Drives(Mutex<HashMap<String, Arc<Drive>>>);

impl Drives {
    pub fn insert(&self, d: Arc<Drive>) {
        self.0
            .lock()
            .expect("drives mutex must not be poisoned")
            .insert(d.branch.clone(), d);
    }
    pub fn get(&self, branch: &str) -> Option<Arc<Drive>> {
        self.0
            .lock()
            .expect("drives mutex must not be poisoned")
            .get(branch)
            .cloned()
    }
    /// The runs started from board `board`, in no set order.
    pub fn of_board(&self, board: &str) -> Vec<Arc<Drive>> {
        self.0
            .lock()
            .expect("drives mutex must not be poisoned")
            .values()
            .filter(|d| d.board == board)
            .cloned()
            .collect()
    }
}

/// What the loop has seen so far: the next event to read, each seat's totals, since when all were quiet.
struct Seen {
    cursor: i64,
    totals: HashMap<String, Totals>,
    quiet_since: Option<Instant>,
    started: Instant,
}

/// Reads the run team's new events; passes each usage report to Arena.
async fn read_new(st: &AppState, arena: &Arena, d: &Drive, seen: &mut Seen) {
    let rows = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .run_events(&d.team, seen.cursor, i64::MAX, 500)
        .unwrap_or_default();
    for (seq, json) in rows {
        seen.cursor = seq + 1;
        let Ok(e) = serde_json::from_str::<EventRecord>(&json) else {
            continue;
        };
        let model::Subject::Seat(seat) = &e.subject else {
            continue;
        };
        let Some((_, agent)) = d.agents.iter().find(|(s, _)| s == seat) else {
            continue;
        };
        seen.totals.entry(seat.clone()).or_default().add(&e.body);
        if let Some(report) = usage_report(seq, &e.body)
            && let Err(err) = client::usage(arena, &d.branch, agent, &report).await
        {
            tracing::warn!(branch = %d.branch, agent, error = %err.message, "Arena did not take a usage report");
        }
    }
}

/// An agent has finished its turns for now: running, idle, at least one turn ended, nothing waiting on it.
fn quiet(st: &AppState, seat: &str, totals: Option<&Totals>) -> bool {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let Ok(Some(s)) = store.seat(seat) else {
        return false;
    };
    s.lifecycle == SeatLifecycle::Running
        && s.activity == SeatActivity::Idle
        && s.current_turn.is_none()
        && s.open_questions == 0
        && s.waiting_for.as_ref().is_none_or(Vec::is_empty)
        && s.handoff_stuck.is_none()
        && !store.has_pending_messages(seat).unwrap_or(true)
        && totals.is_some_and(|t| t.turns_ended > 0)
}

fn all_stopped(st: &AppState, d: &Drive) -> bool {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    d.agents.iter().all(|(seat, _)| {
        store
            .seat(seat)
            .ok()
            .flatten()
            .is_some_and(|s| !crate::store::is_live(s.lifecycle))
    })
}

/// Why the run should finish now, if it should.
fn finish_reason(
    st: &AppState,
    d: &Drive,
    seen: &mut Seen,
    asked: bool,
    settle: Duration,
) -> Option<String> {
    if asked {
        return Some("you asked to finish the run".into());
    }
    if all_stopped(st, d) {
        return Some("the team stopped".into());
    }
    if !d
        .agents
        .iter()
        .all(|(seat, _)| quiet(st, seat, seen.totals.get(seat)))
    {
        seen.quiet_since = None;
        return None;
    }
    let since = *seen.quiet_since.get_or_insert_with(Instant::now);
    (since.elapsed() >= settle)
        .then(|| "every agent finished its turns and nothing waited on it".into())
}

/// Why an agent failed (the runtime's reason), when it did.
fn failure(st: &AppState, seat: &str) -> Option<String> {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let s = store.seat(seat).ok().flatten()?;
    (s.lifecycle == SeatLifecycle::Failed).then(|| {
        s.lifecycle_reason
            .unwrap_or_else(|| "the agent failed".into())
    })
}

/// Records each agent's end in Arena: ok or failed, with its totals.
async fn end_agents(st: &AppState, arena: &Arena, d: &Drive, seen: &Seen) {
    let seconds = seen.started.elapsed().as_secs_f64();
    for (seat, agent) in &d.agents {
        let totals = seen.totals.get(seat).cloned().unwrap_or_default();
        let end = totals.end(failure(st, seat).as_deref(), seconds);
        if let Err(e) = client::end(arena, &d.branch, agent, &end).await {
            tracing::warn!(branch = %d.branch, agent, error = %e.message, "Arena did not record an agent's end");
        }
    }
}

/// Ends each agent with its totals, finishes the run in Arena, then stops the team.
async fn finish(st: &AppState, arena: &Arena, d: &Drive, seen: &Seen, why: String) {
    d.set(DriveState::Finishing { why: why.clone() });
    end_agents(st, arena, d, seen).await;
    let outcome = client::finish(arena, &d.branch).await;
    stop_team(st, &d.team).await;
    d.set(match outcome {
        Ok(v) => DriveState::Finished {
            why,
            status: v["status"].as_str().map(str::to_string),
            error: v["error"].as_str().map(str::to_string),
        },
        Err(e) => DriveState::Failed {
            why: format!("Arena could not finish the run: {}", e.message),
        },
    });
}

/// Stops the run's team as Stop team does, unless a start or stop holds it.
async fn stop_team(st: &AppState, team: &str) {
    let Some(_claim) = st.try_begin_boot(team) else {
        return;
    };
    if let Err(e) = crate::reconciler::stop_rig(st.clone(), team.to_string()).await {
        tracing::warn!(team, error = %e.message, "the Arena run's team did not stop");
    }
}

/// Drives `d` from event `from` until it finishes.
pub async fn run(st: AppState, arena: Arc<Arena>, d: Arc<Drive>, from: i64, settle: Duration) {
    let mut asked = d.finish.subscribe();
    let mut seen = Seen {
        cursor: from,
        totals: HashMap::new(),
        quiet_since: None,
        started: Instant::now(),
    };
    loop {
        tokio::select! {
            () = tokio::time::sleep(POLL) => {}
            _ = asked.changed() => {}
        }
        read_new(&st, &arena, &d, &mut seen).await;
        let was_asked = *asked.borrow();
        if let Some(why) = finish_reason(&st, &d, &mut seen, was_asked, settle) {
            finish(&st, &arena, &d, &seen, why).await;
            return;
        }
    }
}
