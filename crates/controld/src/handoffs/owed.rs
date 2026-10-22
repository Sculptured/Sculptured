//! What a restarted agent is owed in the current start, and sending it once the agent runs.
use super::team::TeamNames;
use super::work::SeatWork;
use super::{CONTINUE_PREFIX, OWED_WAIT};
use crate::error::ApiError;
use crate::state::AppState;
use crate::store::{HandedOverRecord, ResendText, Store};
use model::SeatLifecycle;
use tokio::sync::broadcast::error::RecvError;

/// How the agent that is owed work came back: a fresh process, or its previous session resumed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwedMode {
    Fresh,
    /// Its history holds the unfinished work already: a short continue message is enough.
    Resumed,
}

/// The message for unfinished work: the continue prefix and the work's text (fresh), or the prefix alone (resumed).
#[must_use]
pub fn continue_text(mode: OwedMode, original: &str) -> String {
    match mode {
        OwedMode::Fresh => format!("{CONTINUE_PREFIX}\n\n{original}"),
        OwedMode::Resumed => CONTINUE_PREFIX.to_string(),
    }
}

/// What a restarted agent is owed in the current start.
enum Owed {
    /// Re-send this handoff (the original's id), asking it to continue when it was read.
    Handoff { original: String, continuing: bool },
    /// Send this text as its Job (actor team-start); `continuing` when it read it and did not finish.
    Job { text: String, continuing: bool },
}

/// Which unfinished work a restarted agent is asked to continue.
#[derive(PartialEq)]
enum Continued {
    Nothing,
    Job,
    /// The handoff, by its original's id.
    Handoff(String),
}

fn owed(
    store: &Store,
    rig: &str,
    seat: &str,
) -> Result<Option<(i64, Vec<Owed>, TeamNames)>, ApiError> {
    let start = crate::handoffs::operator::running_start(store, rig)?;
    let Some(snap) = store.rig_snapshot(rig)? else {
        return Ok(None);
    };
    let draft = store.draft(rig)?;
    let names = TeamNames::new(&snap.rig, draft.as_ref());
    let head = !crate::handoffs::team::upstreams_of(&snap.rig)
        .iter()
        .any(|(b, _)| b == seat);
    let job = draft
        .as_ref()
        .and_then(|d| crate::handoffs::team::job_of(d, seat));
    let records = store.handed_over(seat, start)?;
    let w = crate::handoffs::work::seat_work(store, seat, start, &records, head && job.is_none())?;
    let (mut out, continued) = continue_unfinished(store, &w, &records)?;
    // Then everything else it never read (the old process is gone): its Job, unless that is
    // what it continues, and every other handoff of this start.
    if head
        && continued != Continued::Job
        && let Some(text) = job
        && !w.jobs.iter().any(|j| w.read.contains(j))
    {
        out.push(Owed::Job {
            text,
            continuing: false,
        });
    }
    for (r, copies) in crate::handoffs::plan::groups(&records) {
        if continued == Continued::Handoff(r.message_id.clone()) {
            continue;
        }
        if !copies.iter().any(|c| w.read.contains(c)) {
            out.push(Owed::Handoff {
                original: r.message_id.clone(),
                continuing: false,
            });
        }
    }
    Ok(Some((start, out, names)))
}

/// Work it read and did not finish is sent again, asking it to continue.
fn continue_unfinished(
    store: &Store,
    w: &SeatWork,
    records: &[HandedOverRecord],
) -> rusqlite::Result<(Vec<Owed>, Continued)> {
    let Some((message, _)) = &w.unfinished else {
        return Ok((Vec::new(), Continued::Nothing));
    };
    Ok(match records.iter().find(|r| &r.message_id == message) {
        Some(r) => {
            let original = r.resend_of.clone().unwrap_or_else(|| r.message_id.clone());
            (
                vec![Owed::Handoff {
                    original: original.clone(),
                    continuing: true,
                }],
                Continued::Handoff(original),
            )
        }
        None => {
            let text = store.message_text(message)?.unwrap_or_default();
            (
                vec![Owed::Job {
                    text,
                    continuing: true,
                }],
                Continued::Job,
            )
        }
    })
}

/// Key in `AppState::handoffs_in_flight` while `seat` is being restarted and its owed
/// messages are not all sent yet (message ids are UUIDs, so they never collide).
pub(super) fn restart_key(seat: &str) -> String {
    format!("restart:{seat}")
}

/// Marks an agent as being restarted, from before its relaunch until `deliver_owed` has
/// sent what it is owed (or given up), so the planner does not take the moment between
/// "running again" and "owed messages queued" for an agent with nothing left to do.
pub struct Restarting {
    st: AppState,
    key: String,
}

impl Restarting {
    #[must_use]
    pub fn new(st: &AppState, seat: &str) -> Self {
        let key = restart_key(seat);
        st.handoffs_in_flight
            .lock()
            .expect("handoffs_in_flight mutex must not be poisoned")
            .insert(key.clone());
        Restarting {
            st: st.clone(),
            key,
        }
    }
}

impl Drop for Restarting {
    fn drop(&mut self) {
        self.st
            .handoffs_in_flight
            .lock()
            .expect("handoffs_in_flight mutex must not be poisoned")
            .remove(&self.key);
    }
}

/// After "Restart this agent": once it runs, sends what it is owed in the current start.
/// Work it read and did not finish is sent again first, with `CONTINUE_PREFIX` (and, for a
/// fresh process, the work's text: `continue_text`); then its Job (a first agent that never
/// read it) and every other handoff it never read. The old process is gone, so a handoff
/// delivered to it but never read is owed again. A first handoff not yet due is left to the
/// dispatcher. `_restarting` is released when done.
pub async fn deliver_owed(st: AppState, seat: String, restarting: Restarting, mode: OwedMode) {
    send_owed(&st, &seat, mode).await;
    // What it was owed is sent (or given up): plan again without the restart mark, so an
    // agent that was owed nothing and has nothing left to do shows as such now.
    drop(restarting);
    if let Some(rig) = crate::seat_ids::rig_of(&seat) {
        crate::handoffs::process_rig(&st, rig, false).await;
    }
}

/// Waits (up to `OWED_WAIT`) until the agent runs; false when it never does or is gone.
async fn wait_until_running(st: &AppState, seat: &str) -> bool {
    let mut rx = st.hub.subscribe();
    let deadline = tokio::time::Instant::now() + OWED_WAIT;
    loop {
        match st
            .store
            .lock()
            .expect("store mutex must not be poisoned")
            .seat(seat)
            .ok()
            .flatten()
            .map(|v| v.lifecycle)
        {
            Some(SeatLifecycle::Running) => return true,
            None => return false,
            Some(_) => {}
        }
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Ok(_)) | Ok(Err(RecvError::Lagged(_))) => {}
            _ => return false,
        }
    }
}

async fn send_owed(st: &AppState, seat: &str, mode: OwedMode) {
    if !wait_until_running(st, seat).await {
        return;
    }
    let Some(rig) = crate::seat_ids::rig_of(seat).map(str::to_string) else {
        return;
    };
    let plan = owed(
        &st.store.lock().expect("store mutex must not be poisoned"),
        &rig,
        seat,
    );
    let Ok(Some((start, owed, names))) = plan else {
        return;
    };
    for o in owed {
        match o {
            Owed::Job { text, continuing } => {
                let text = if continuing {
                    continue_text(mode, &text)
                } else {
                    text
                };
                crate::handoffs::operator::send_job(st, seat, text, "owed job delivery").await;
            }
            Owed::Handoff {
                original,
                continuing,
            } => {
                let text = match (continuing, mode) {
                    (false, _) => ResendText::Same,
                    (true, OwedMode::Fresh) => ResendText::Prefixed(CONTINUE_PREFIX),
                    (true, OwedMode::Resumed) => ResendText::Only(CONTINUE_PREFIX),
                };
                crate::handoffs::operator::resend_one(
                    st, seat, start, &original, text, true, &names,
                )
                .await;
            }
        }
    }
}
