//! What each agent did in a start: its finished turns, the work it read, and the messages it got.
use super::{HANDOFF_ACTOR, TEAM_START_ACTOR};
use crate::store::{HandedOverRecord, Store};
use model::{
    Actor, EventBody, EventRecord, HandoffSource, ResultCheck, RuleResult, Subject, TurnStatus,
    Wave,
};
use std::collections::{HashMap, HashSet};

/// One finished turn and its last reply.
#[derive(Clone, Debug)]
pub(super) struct Turn {
    pub(super) seq: i64,
    /// Seq of its `TurnStarted`.
    pub(super) started: i64,
    pub(super) turn_id: String,
    pub(super) text: Option<String>,
    /// It finished the agent's work: its result is passed along.
    pub(super) work: bool,
    /// The wave of the work it finished, if that work was part of one.
    pub(super) wave: Option<Wave>,
}

/// One thing an upstream agent has to pass along: a work turn, or a turn the operator
/// asked it to hand over again.
#[derive(Clone, Debug)]
pub(super) struct Item {
    pub(super) turn: Turn,
    pub(super) requested_seq: Option<i64>,
    pub(super) wave: Option<Wave>,
}

impl Item {
    pub(super) fn key(&self) -> i64 {
        self.requested_seq.unwrap_or(self.turn.seq)
    }
    pub(super) fn source(&self, seat: &str) -> HandoffSource {
        HandoffSource {
            seat: seat.to_string(),
            turn_id: self.turn.turn_id.clone(),
            turn_seq: self.turn.seq,
            requested_seq: self.requested_seq,
        }
    }
    /// The wave this item moves in: its own, else a new one starting at `seat`.
    pub(super) fn wave_of(&self, seat: &str) -> Wave {
        self.wave.clone().unwrap_or_else(|| Wave {
            id: self.key(),
            origin: seat.to_string(),
        })
    }
}

/// Why work whose result failed the agent's result checks did not finish.
pub(super) const CHECKS_FAILED: &str = "its result did not pass its checks";

/// A completed turn on the agent's work, against its result checks.
#[derive(Clone, Debug)]
pub(super) struct Checked {
    pub(super) turn_id: String,
    pub(super) passed: bool,
    pub(super) results: Vec<RuleResult>,
    /// Failed checks on the same work so far, this one included.
    pub(super) failures: u32,
}

/// What one agent did in a start.
#[derive(Clone, Debug, Default)]
pub(super) struct SeatWork {
    pub(super) turns: Vec<Turn>,
    /// "Hand over again" requests: `(request seq, turn seq)`.
    pub(super) requests: Vec<(i64, i64)>,
    /// Work it read and did not finish: `(message id, why)`.
    pub(super) unfinished: Option<(String, &'static str)>,
    /// It read work whose turn has not ended yet.
    pub(super) reading: bool,
    /// A turn of its current process has started and not ended.
    pub(super) in_turn: bool,
    /// Every message queued to it, oldest first.
    pub(super) queued: Vec<String>,
    /// Its Job messages (the Job, or a continuation of it), oldest first.
    pub(super) jobs: Vec<String>,
    /// Messages the host reported delivered to it or read by it.
    pub(super) delivered: HashSet<String>,
    /// Messages the host reported read by it.
    pub(super) read: HashSet<String>,
    /// Messages delivered to an earlier process of it that never read them.
    pub(super) lost: HashSet<String>,
    /// Messages other agents sent it themselves: `(seq, message id, sender seat)`, oldest first.
    pub(super) from_agents: Vec<(i64, String, String)>,
    /// Its completed turns on its work against its result checks, oldest first (none without checks).
    pub(super) checked: Vec<Checked>,
    /// Turns whose check is recorded (`ResultChecked`).
    pub(super) checks_recorded: HashSet<String>,
}

/// Work read and not finished yet: the message, its wave, and why it did not finish (None while
/// its turn may still finish it).
struct Reading {
    message: String,
    wave: Option<Wave>,
    why: Option<&'static str>,
    /// Its result failed the agent's checks this many times.
    failures: u32,
}

/// One pass over a seat's events of a start, building its `SeatWork`.
struct Scan<'a> {
    out: SeatWork,
    /// Each handoff message's wave, from its record.
    waves: HashMap<&'a str, &'a Option<Wave>>,
    head_without_job: bool,
    /// Messages that are work, with their wave.
    work_messages: HashMap<String, Option<Wave>>,
    first_human_seen: bool,
    /// The latest work read and not yet finished.
    reading: Option<Reading>,
    /// The running turn, the seq it started at, and its last reply so far.
    current: Option<(String, i64, Option<String>)>,
    /// The agent's result checks (empty: every completed turn on its work finishes it).
    checks: Vec<ResultCheck>,
}

impl Scan<'_> {
    fn event(&mut self, e: EventRecord) {
        match e.body {
            EventBody::MessageQueued { message_id, .. } => self.queued(e.seq, &e.actor, message_id),
            EventBody::MessageDelivered { message_id } => {
                self.out.delivered.insert(message_id);
            }
            EventBody::MessageConsumed { message_id, .. } => self.consumed(message_id),
            EventBody::TenureEnded { .. } => {
                self.mark_unfinished("the agent stopped");
                self.current = None;
            }
            EventBody::TenureStarted { .. } => self.new_process(),
            EventBody::TurnStarted { turn_id } => self.current = Some((turn_id, e.seq, None)),
            EventBody::AgentText { turn_id, text } => {
                if let Some((id, _, last)) = self.current.as_mut()
                    && turn_id.as_ref().is_none_or(|t| t == id)
                {
                    *last = Some(text);
                }
            }
            EventBody::TurnEnded { turn_id, status } => self.turn_ended(e.seq, turn_id, status),
            EventBody::HandoverRequested { turn_seq, .. } => {
                self.out.requests.push((e.seq, turn_seq))
            }
            EventBody::ResultChecked { turn_id, .. } => {
                self.out.checks_recorded.insert(turn_id);
            }
            _ => {}
        }
    }

    /// Its work messages are its Jobs, its handoffs (including an agent's own message recorded
    /// as one) and, for a first agent with no Job, the first message you sent it.
    fn queued(&mut self, seq: i64, actor: &Actor, message_id: String) {
        self.out.queued.push(message_id.clone());
        match actor {
            Actor::Seat(sender) => {
                self.out
                    .from_agents
                    .push((seq, message_id.clone(), sender.clone()));
                if let Some(wave) = self.waves.get(message_id.as_str()) {
                    self.work_messages.insert(message_id, (*wave).clone());
                }
            }
            Actor::System(id) if id == TEAM_START_ACTOR => {
                self.out.jobs.push(message_id.clone());
                self.work_messages.insert(message_id, None);
            }
            Actor::System(id) if id == HANDOFF_ACTOR => {
                let wave = self
                    .waves
                    .get(message_id.as_str())
                    .and_then(|w| (*w).clone());
                self.work_messages.insert(message_id, wave);
            }
            Actor::Human(_) if self.head_without_job && !self.first_human_seen => {
                self.first_human_seen = true;
                self.work_messages.insert(message_id, None);
            }
            _ => {}
        }
    }

    fn consumed(&mut self, message_id: String) {
        self.out.delivered.insert(message_id.clone());
        self.out.read.insert(message_id.clone());
        if let Some(wave) = self.work_messages.get(&message_id) {
            self.reading = Some(Reading {
                message: message_id,
                wave: wave.clone(),
                why: None,
                failures: 0,
            });
        }
    }

    fn mark_unfinished(&mut self, why: &'static str) {
        if let Some(r) = self.reading.as_mut() {
            r.why = Some(why);
        }
    }

    /// A new process. The old one may have died without a TenureEnded (it was lost
    /// with its host): work it read is unfinished, its turn is over, and a message
    /// delivered to it and never read will never be read by it.
    fn new_process(&mut self) {
        if let Some(r) = self.reading.as_mut()
            && r.why.is_none()
        {
            r.why = Some("the agent stopped");
        }
        self.current = None;
        let lost: Vec<String> = self
            .out
            .delivered
            .iter()
            .filter(|m| !self.out.read.contains(*m))
            .cloned()
            .collect();
        self.out.lost.extend(lost);
    }

    fn turn_ended(&mut self, seq: i64, turn_id: String, status: TurnStatus) {
        let (started, text) = match self.current.take() {
            Some((id, started, last)) if id == turn_id => (started, last),
            _ => (seq, None),
        };
        match status {
            TurnStatus::Completed => {
                if !self.passes_checks(&turn_id, text.as_deref()) {
                    self.out.turns.push(Turn {
                        seq,
                        started,
                        turn_id,
                        text,
                        work: false,
                        wave: None,
                    });
                    return;
                }
                let finished = self.reading.take();
                let work = finished.is_some();
                self.out.turns.push(Turn {
                    seq,
                    started,
                    turn_id,
                    text,
                    work,
                    wave: finished.and_then(|r| r.wave),
                });
            }
            TurnStatus::Interrupted => self.mark_unfinished("interrupted"),
            TurnStatus::Failed => self.mark_unfinished("failed"),
        }
    }

    /// Whether a completed turn on the agent's work passes its result checks (true without work or
    /// checks). A failure leaves the work unfinished, so a later turn can finish it.
    fn passes_checks(&mut self, turn_id: &str, text: Option<&str>) -> bool {
        let Some(r) = self.reading.as_mut().filter(|_| !self.checks.is_empty()) else {
            return true;
        };
        let results = model::result_check::check_result(&self.checks, text.unwrap_or(""));
        let passed = results.iter().all(|x| x.passed);
        if !passed {
            r.failures += 1;
            r.why = Some(CHECKS_FAILED);
        }
        self.out.checked.push(Checked {
            turn_id: turn_id.to_string(),
            passed,
            results,
            failures: r.failures,
        });
        passed
    }

    fn finish(mut self) -> SeatWork {
        self.out.in_turn = self.current.is_some();
        self.out.reading = self.reading.as_ref().is_some_and(|r| r.why.is_none());
        self.out.unfinished = self.reading.and_then(|r| r.why.map(|w| (r.message, w)));
        self.out
    }
}

/// What `seat` did after `start_seq`: its finished turns (with each turn's last reply and
/// whether it finished the agent's work), the operator's "Hand over again" requests, and
/// work it read and did not finish. Its work messages are its Jobs, its handoffs (`records`)
/// and, for a first agent with no Job, the first message you sent it.
pub(super) fn seat_work(
    store: &Store,
    seat: &str,
    start_seq: i64,
    records: &[HandedOverRecord],
    head_without_job: bool,
) -> rusqlite::Result<SeatWork> {
    let types = [
        "message-queued",
        "message-delivered",
        "message-consumed",
        "turn-started",
        "tenure-started",
        "agent-text",
        "turn-ended",
        "tenure-ended",
        "handover-requested",
        "result-checked",
    ];
    let events = store.events_of_types(&Subject::Seat(seat.to_string()), start_seq, &types)?;
    let checks = crate::seat_ids::rig_of(seat)
        .and_then(|rig| store.draft(rig).ok().flatten())
        .and_then(|d| {
            d.agent_for_seat(seat)
                .map(|a| a.advanced.result_checks.clone())
        })
        .unwrap_or_default();
    let mut scan = Scan {
        out: SeatWork::default(),
        waves: records
            .iter()
            .map(|r| (r.message_id.as_str(), &r.wave))
            .collect(),
        head_without_job,
        work_messages: HashMap::new(),
        first_human_seen: false,
        reading: None,
        current: None,
        checks,
    };
    for e in events {
        scan.event(e);
    }
    Ok(scan.finish())
}

/// What upstream `seat` has to pass along in this start: its work turns and the turns
/// the operator asked it to hand over again (each a wave of its own), in order.
pub(super) fn work_items(w: &SeatWork, seat: &str, before: &impl Fn(i64) -> bool) -> Vec<Item> {
    let mut items: Vec<Item> = w
        .turns
        .iter()
        .filter(|t| t.work && before(t.seq))
        .map(|t| Item {
            turn: t.clone(),
            requested_seq: None,
            wave: t.wave.clone(),
        })
        .collect();
    for &(req, turn_seq) in &w.requests {
        if !before(req) {
            continue;
        }
        if let Some(t) = w.turns.iter().find(|t| t.seq == turn_seq) {
            items.push(Item {
                turn: t.clone(),
                requested_seq: Some(req),
                wave: Some(Wave {
                    id: req,
                    origin: seat.to_string(),
                }),
            });
        }
    }
    items.sort_by_key(|i| i.key());
    items
}
