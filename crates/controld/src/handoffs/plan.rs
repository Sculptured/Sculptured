//! The planner: from the store alone, what each agent of a start is due (handoffs to send)
//! and why it waits or is stuck.
use super::Action;
use super::start_plan::{StartPlan, StartRef};
use super::starts::Ended;
use super::team::Team;
use super::work::SeatWork;
use crate::store::{HandedOverRecord, RigSource, Store};
use model::{HandoffStuck, SeatLifecycle};
use std::collections::HashSet;

pub(super) fn plan(
    store: &Store,
    rig: &str,
    with_previous: bool,
    in_flight: &HashSet<String>,
) -> rusqlite::Result<Vec<Action>> {
    if !matches!(store.rig_source(rig)?, Some(RigSource::Team { .. })) {
        return Ok(Vec::new());
    }
    let Some(snap) = store.rig_snapshot(rig)? else {
        return Ok(Vec::new());
    };
    let draft = store.draft(rig)?;
    if draft.as_ref().is_some_and(|d| d.workflow.is_some()) {
        return Ok(Vec::new());
    }
    let team = Team::load(store, &snap.rig, &snap.seats, draft)?;
    let (current, previous) = crate::handoffs::starts::starts(store, rig)?;
    let mut actions = Vec::new();
    if with_previous && let Some((seq, end)) = previous {
        StartPlan::new(
            store,
            &team,
            StartRef {
                seq,
                end,
                current: false,
            },
            in_flight,
        )?
        .run(&mut actions)?;
    }
    match current {
        Some((seq, end)) => StartPlan::new(
            store,
            &team,
            StartRef {
                seq,
                end,
                current: true,
            },
            in_flight,
        )?
        .run(&mut actions)?,
        // No start this dispatcher plans (none yet, or only starts from before handoffs
        // existed): nobody waits and nothing is stuck.
        None => {
            for s in &snap.seats {
                if s.waiting_for.is_some() || s.handoff_stuck.is_some() {
                    actions.push(Action::Status {
                        seat: s.id.clone(),
                        waiting_for: Vec::new(),
                        stuck: None,
                    });
                }
            }
        }
    }
    Ok(actions)
}

/// What can happen to a handoff for a seat right now.
pub(super) enum Fate {
    Send,
    Wait,
    Fail(String),
}

pub(super) fn fate(team: &Team, seat: &str, start_seq: i64, ended: Option<Ended>) -> Fate {
    let name = team.name(seat);
    match ended {
        Some(Ended::Stopped) => {
            return Fate::Fail(format!(
                "not sent: the team was stopped before {name} got this handoff"
            ));
        }
        Some(Ended::StartedAgain) => {
            return Fate::Fail(format!(
                "not sent: the team was started again before {name} got this handoff"
            ));
        }
        None => {}
    }
    let Some(view) = team.view(seat) else {
        return Fate::Fail(format!("not sent: {name} is no longer part of the team"));
    };
    match team.lifecycle_in(seat, start_seq) {
        Some(SeatLifecycle::Running) => Fate::Send,
        Some(SeatLifecycle::Planned | SeatLifecycle::Launching | SeatLifecycle::Stale) | None => {
            Fate::Wait
        }
        Some(l) => {
            let why = view
                .lifecycle_reason
                .as_deref()
                .map(|r| team.names.plain(r))
                .unwrap_or_else(|| lifecycle_words(l).to_string());
            Fate::Fail(format!("not sent: {name} is not running ({why})"))
        }
    }
}

fn lifecycle_words(l: SeatLifecycle) -> &'static str {
    match l {
        SeatLifecycle::Attention => "needs attention",
        SeatLifecycle::Failed => "failed",
        SeatLifecycle::Stopped => "stopped",
        SeatLifecycle::Stopping => "stopping",
        _ => "not running",
    }
}

/// Copies of each handoff message (the message and its resends), oldest first.
pub(super) fn groups(records: &[HandedOverRecord]) -> Vec<(&HandedOverRecord, Vec<String>)> {
    records
        .iter()
        .filter(|r| r.resend_of.is_none())
        .map(|r| {
            let copies = records
                .iter()
                .filter(|c| {
                    c.message_id == r.message_id
                        || c.resend_of.as_deref() == Some(r.message_id.as_str())
                })
                .map(|c| c.message_id.clone())
                .collect();
            (r, copies)
        })
        .collect()
}

/// How a handoff (all its copies) fared: delivered or read (by the host's own events),
/// still being sent, queued and not being sent (lost between recording and sending, e.g.
/// across a controld restart), or every copy not sent.
#[derive(PartialEq)]
pub(super) enum Delivery {
    Reached,
    Pending,
    NotDelivered,
    NotSent,
}

pub(super) fn delivery(
    store: &Store,
    w: &SeatWork,
    copies: &[String],
    in_flight: &HashSet<String>,
) -> rusqlite::Result<Delivery> {
    if copies.iter().any(|c| w.delivered.contains(c)) {
        return Ok(Delivery::Reached);
    }
    let mut queued = Vec::new();
    for c in copies {
        match store.message_state(c)? {
            Some(model::MessageState::Delivered | model::MessageState::Consumed) => {
                return Ok(Delivery::Reached);
            }
            Some(model::MessageState::Queued) => queued.push(c),
            _ => {}
        }
    }
    // A copy the run budget holds for a free slot is on its way: the gate sends it when one frees.
    let held = |c: &str| {
        store
            .connection()
            .query_row(
                "SELECT 1 FROM budget_queue WHERE message_id = ?1",
                [c],
                |_| Ok(()),
            )
            .is_ok()
    };
    Ok(
        if queued.iter().any(|c| in_flight.contains(*c) || held(c)) {
            Delivery::Pending
        } else if !queued.is_empty() {
            Delivery::NotDelivered
        } else {
            Delivery::NotSent
        },
    )
}

pub(super) fn stuck(reason: String, can_resend: bool) -> HandoffStuck {
    HandoffStuck { reason, can_resend }
}

/// True when a first agent's Job was not sent in this start although the agent runs: it
/// has Job messages, none was read, and each was recorded as failed in this start (a
/// previous start's Job recorded "started again" after this start began does not count).
/// Not while it is being restarted: what it is owed is then on its way.
pub(super) fn job_unsent(
    store: &Store,
    team: &Team,
    seat: &str,
    start_seq: i64,
    w: &SeatWork,
    in_flight: &HashSet<String>,
) -> rusqlite::Result<bool> {
    if team.job(seat).is_none()
        || w.jobs.is_empty()
        || team.lifecycle_in(seat, start_seq) != Some(SeatLifecycle::Running)
        || in_flight.contains(&crate::handoffs::restart_key(seat))
        || w.jobs.iter().any(|j| w.read.contains(j))
    {
        return Ok(false);
    }
    for j in &w.jobs {
        if store.message_state(j)? != Some(model::MessageState::Failed)
            || store
                .message_reason(j)?
                .is_some_and(|r| r.contains(crate::reconciler::STARTED_AGAIN))
        {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn push_status(
    team: &Team,
    seat: &str,
    waiting_for: Vec<String>,
    stuck: Option<HandoffStuck>,
    actions: &mut Vec<Action>,
) {
    let now = team
        .view(seat)
        .map(|v| {
            (
                v.waiting_for.clone().unwrap_or_default(),
                v.handoff_stuck.clone(),
            )
        })
        .unwrap_or_default();
    if now != (waiting_for.clone(), stuck.clone()) {
        actions.push(Action::Status {
            seat: seat.to_string(),
            waiting_for,
            stuck,
        });
    }
}
