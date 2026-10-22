//! Context ceiling (protections): an agent whose context passed its team's ceiling is asked, between
//! turns, to write its notes; when that turn ends it continues in a fresh conversation from them. A
//! smaller context makes every later model call cheaper. It never interrupts a turn or stops a run.
use crate::reconciler;
use crate::state::AppState;
use crate::store::Store;
use host_proto::LaunchMode;
use model::{
    Actor, EventBody, MessageIntent, Provenance, SeatActivity, SeatLifecycle, Subject, TenureCause,
};
use std::collections::HashMap;

const ACTOR: &str = "protection";

fn note_text(seat: &str, used: u64) -> String {
    format!(
        "Your conversation has grown to about {}k tokens of context, and every further step re-reads all of it. To keep working efficiently, save your work now and write {} in your folder: what is done (exact file paths), checks actually run, what is next, and the files you need. Keep it short, then end your turn. You will continue in a fresh conversation from that note.",
        used / 1000,
        super::note_path(seat)
    )
}

fn continue_text(seat: &str) -> String {
    format!(
        "You are continuing in a fresh conversation, because your previous one grew large. Read your note {} in your folder and continue your job from there. Update the note if you are asked to write one again.",
        super::note_path(seat)
    )
}

/// What to do now for each agent of a team with a ceiling.
enum Step {
    Ask {
        seat: String,
        used: u64,
        ceiling: u64,
        seq: i64,
    },
    Continue {
        seat: String,
    },
}

/// The latest seq of `types` on `seat`, if any.
fn latest(store: &Store, seat: &str, types: &[&str]) -> rusqlite::Result<Option<(i64, EventBody)>> {
    Ok(store
        .latest_of_types(&Subject::Seat(seat.to_string()), types, 1)?
        .into_iter()
        .next()
        .map(|e| (e.seq, e.body)))
}

fn steps(store: &Store, asked: &HashMap<String, i64>) -> rusqlite::Result<Vec<Step>> {
    let mut out = Vec::new();
    for rig in store.list_rigs()? {
        let Some(ceiling) = store
            .draft(&rig.name)?
            .and_then(|d| d.protection.context_ceiling())
        else {
            continue;
        };
        let Some(snap) = store.rig_snapshot(&rig.name)? else {
            continue;
        };
        let matching_seats = snap
            .seats
            .iter()
            .filter(|v| v.lifecycle == SeatLifecycle::Running && v.activity == SeatActivity::Idle);
        for view in matching_seats {
            if let Some(&at) = asked.get(&view.id) {
                // The note's turn has ended once a turn ended after the request.
                if latest(store, &view.id, &["turn-ended"])?.is_some_and(|(seq, _)| seq > at) {
                    out.push(Step::Continue {
                        seat: view.id.clone(),
                    });
                }
                continue;
            }
            if store.control(&view.id)?.is_some() {
                continue; // a stop or a budget wrap-up is already in charge of this agent
            }
            let tenure = latest(store, &view.id, &["tenure-started"])?.map_or(0, |(seq, _)| seq);
            if let Some((seq, EventBody::ContextReported { used_tokens, .. })) =
                latest(store, &view.id, &["context-reported"])?
                && seq > tenure
                && used_tokens > ceiling.max(2 * start_size(store, &view.id, tenure)?)
            {
                out.push(Step::Ask {
                    seat: view.id.clone(),
                    used: used_tokens,
                    ceiling,
                    seq,
                });
            }
        }
    }
    Ok(out)
}

/// The conversation's starting context (its first reading since `tenure`): instructions, tools and the
/// team briefing. The ceiling acts only past twice this, so a fresh conversation, which starts at this
/// size again, can never trip it at once and restart in a loop.
fn start_size(store: &Store, seat: &str, tenure: i64) -> rusqlite::Result<u64> {
    Ok(store
        .events_for_seat(seat, tenure, 500)?
        .into_iter()
        .find_map(|e| match e.body {
            EventBody::ContextReported { used_tokens, .. } => Some(used_tokens),
            _ => None,
        })
        .unwrap_or(0))
}

/// One round: ask agents over their ceiling for notes, and continue those whose note turn ended.
pub async fn check(st: &AppState) {
    let asked = st
        .context_asked
        .lock()
        .expect("context_asked mutex must not be poisoned")
        .clone();
    let due = match steps(
        &st.store.lock().expect("store mutex must not be poisoned"),
        &asked,
    ) {
        Ok(s) => s,
        Err(e) => return tracing::warn!(error = %e, "context ceilings not checked"),
    };
    for step in due {
        match step {
            Step::Ask {
                seat,
                used,
                ceiling,
                seq,
            } => ask(st, seat, used, ceiling, seq).await,
            Step::Continue { seat } => continue_fresh(st, seat).await,
        }
    }
}

/// Ends the agent's current conversation and starts it again with a fresh one.
async fn restart_fresh(st: &AppState, seat: &str) -> Result<(), crate::error::ApiError> {
    reconciler::stop_seat_now(st, seat).await?;
    for _ in 0..100 {
        let live = st
            .store
            .lock()
            .expect("store mutex must not be poisoned")
            .seat(seat)?
            .is_some_and(|v| {
                matches!(
                    v.lifecycle,
                    SeatLifecycle::Running | SeatLifecycle::Launching | SeatLifecycle::Stopping
                )
            });
        if !live {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    crate::restore::relaunch_and_send(st, seat, LaunchMode::Fresh, TenureCause::Fresh, None).await
}

async fn ask(st: &AppState, seat: String, used: u64, ceiling: u64, seq: i64) {
    // The note's turn is the first turn to end after this record, so remember its place: an earlier
    // turn-end (the turn that grew the context) must not count as the note being written.
    let seq = match st.local_event(
        Actor::System(ACTOR.into()),
        Subject::Seat(seat.clone()),
        Provenance::Operator,
        EventBody::ContextCeilingReached {
            used_tokens: used,
            ceiling,
        },
    ) {
        Ok(item) => item.event.seq,
        Err(e) => {
            tracing::warn!(%seat, error = %e, "context ceiling not recorded");
            st.store
                .lock()
                .expect("store mutex must not be poisoned")
                .last_seq()
                .unwrap_or(seq)
        }
    };
    match reconciler::send_message_as(
        st,
        &seat,
        note_text(&seat, used),
        MessageIntent::Instruct,
        false,
        Actor::System(ACTOR.into()),
    )
    .await
    {
        Ok(_) => {
            st.context_asked
                .lock()
                .expect("context_asked mutex must not be poisoned")
                .insert(seat, seq);
        }
        Err(e) => {
            tracing::warn!(%seat, error = %e.message, "context ceiling: note request will retry")
        }
    }
}

async fn continue_fresh(st: &AppState, seat: String) {
    st.context_asked
        .lock()
        .expect("context_asked mutex must not be poisoned")
        .remove(&seat);
    let fresh = match restart_fresh(st, &seat).await {
        Ok(()) => reconciler::send_message_as(
            st,
            &seat,
            continue_text(&seat),
            MessageIntent::Instruct,
            false,
            Actor::System(ACTOR.into()),
        )
        .await
        .map(|_| ()),
        Err(e) => Err(e),
    };
    if let Err(e) = fresh {
        tracing::warn!(%seat, error = %e.message, "context ceiling: fresh continue failed");
    }
}
