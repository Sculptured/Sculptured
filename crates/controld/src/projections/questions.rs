//! Questions: open → answered | expired (the agent decides) | withdrawn. Keeps each seat's open count.
use crate::views::{DecisionView, QuestionView};
use model::{
    Answer, DecisionSource, EventBody, EventRecord, Provenance, QuestionPrompt, QuestionSource,
    QuestionState, RiskFlag, SeatLifecycle, Subject, TurnStatus,
};
use rusqlite::{Connection, OptionalExtension, Params, Row, Transaction, params};
use serde::Serialize;

pub(crate) const PROCESS_ENDED: &str = "the agent's process ended before anyone answered";
pub(crate) const TURN_INTERRUPTED: &str = "the agent's turn was interrupted before anyone answered";
pub(crate) const TURN_FAILED: &str = "the agent's turn failed before anyone answered";
pub(crate) const SETTLED_BY_PROGRAM: &str =
    "the agent's program settled this question itself before Sculptured answered";
pub(crate) const ANSWER_LOST_PROCESS: &str =
    "the agent's process ended before the answer reached it";
pub(crate) const ANSWER_LOST_TURN: &str = "the agent's turn ended before the answer reached it";

/// Why an answer that did not reach the agent cannot be taken any more: the program settled the
/// question itself (Codex can, while the answer was on its way), the process is gone, or the asking
/// turn is over. None while the agent can still take an answer.
fn answer_gone(
    settled: bool,
    live: bool,
    turn: Option<String>,
    current: Option<String>,
) -> Option<&'static str> {
    if settled {
        Some(SETTLED_BY_PROGRAM)
    } else if !live {
        Some(ANSWER_LOST_PROCESS)
    } else if turn.is_some() && turn != current {
        Some(ANSWER_LOST_TURN)
    } else {
        None
    }
}

/// A recorded answer that did not reach the agent: the question opens again while the agent can still
/// take an answer (its process runs, the asking turn is current, and its program has not settled the
/// question itself), else it is withdrawn with the reason. A reopened question gets a fresh deadline:
/// the team's full question timeout, counted from the failure, so the operator has the whole window again.
fn reopen(tx: &Transaction, seat: &str, id: &str, at_ms: i64) -> rusqlite::Result<()> {
    let found: Option<(Option<String>, bool, String, Option<String>)> = tx
        .query_row(
            "SELECT q.turn_id, q.settled, s.lifecycle, s.current_turn FROM questions q JOIN seats s ON s.id = q.seat
             WHERE q.id = ?1 AND q.state = 'answered'",
            [id],
            |r| Ok((r.get(0)?, r.get::<_, i64>(1)? != 0, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((turn, settled, lifecycle, current)) = found else {
        return Ok(());
    };
    let live =
        crate::store::enum_parse::<SeatLifecycle>(&lifecycle).is_ok_and(crate::store::is_live);
    if let Some(reason) = answer_gone(settled, live, turn, current) {
        tx.execute(
            "UPDATE questions SET state = 'withdrawn', withdrawn_reason = ?2, answer_json = NULL, answered_at_ms = NULL,
                 unconfirmed_reason = NULL WHERE id = ?1",
            params![id, reason],
        )?;
        return Ok(());
    }
    let deadline = at_ms
        + crate::settings::question_timeout_ms_in(tx, crate::seat_ids::rig_or_empty(seat))? as i64;
    tx.execute(
        "UPDATE questions SET state = 'open', answer_json = NULL, answered_at_ms = NULL, unconfirmed_reason = NULL,
             deadline_ms = ?2 WHERE id = ?1",
        params![id, deadline],
    )?;
    tx.execute(
        "UPDATE seats SET open_questions = open_questions + 1 WHERE id = ?1",
        [seat],
    )?;
    Ok(())
}

fn json_text<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).expect("serializes")
}

/// Moves an open question to `state` and lowers its seat's open count. False when it was not open.
fn close(tx: &Transaction, seat: &str, id: &str, state: QuestionState) -> rusqlite::Result<bool> {
    let n = tx.execute(
        "UPDATE questions SET state = ?2 WHERE id = ?1 AND state = 'open'",
        params![id, crate::store::enum_text(&state)],
    )?;
    if n == 1 {
        tx.execute(
            "UPDATE seats SET open_questions = MAX(open_questions - 1, 0) WHERE id = ?1",
            [seat],
        )?;
    }
    Ok(n == 1)
}

fn withdraw_open(
    tx: &Transaction,
    seat: &str,
    turn: Option<&str>,
    reason: &str,
) -> rusqlite::Result<()> {
    let ids: Vec<String> = {
        let mut q = tx.prepare("SELECT id FROM questions WHERE seat = ?1 AND state = 'open' AND (?2 IS NULL OR turn_id = ?2)")?;
        q.query_map(params![seat, turn], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    for id in ids {
        withdraw(tx, seat, &id, reason)?;
    }
    Ok(())
}

pub(crate) fn project(tx: &Transaction, ev: &EventRecord) -> rusqlite::Result<()> {
    let Subject::Seat(seat) = &ev.subject else {
        return Ok(());
    };
    match &ev.body {
        EventBody::QuestionRaised { .. } => raise(tx, seat, ev),
        EventBody::QuestionAnswered { .. }
        | EventBody::QuestionExpired { .. }
        | EventBody::QuestionDecided { .. }
        | EventBody::QuestionWithdrawn { .. }
        | EventBody::QuestionAcknowledged { .. } => settle(tx, seat, ev),
        EventBody::QuestionResolved { .. }
        | EventBody::QuestionAnswerFailed { .. }
        | EventBody::QuestionAnswerUnconfirmed { .. }
        | EventBody::QuestionAnswerConfirmed { .. } => delivery(tx, seat, ev),
        body => withdraw_on_end(tx, seat, body),
    }
}

fn raise(tx: &Transaction, seat: &str, ev: &EventRecord) -> rusqlite::Result<()> {
    let EventBody::QuestionRaised {
        question_id,
        source,
        prompt,
        turn_id,
    } = &ev.body
    else {
        return Ok(());
    };
    let deadline = ev.at_ms
        + crate::settings::question_timeout_ms_in(tx, crate::seat_ids::rig_or_empty(seat))? as i64;
    let n = tx.execute(
        "INSERT OR IGNORE INTO questions (id, seat, rig, source, prompt_json, state, opened_at_ms, opened_seq, deadline_ms, turn_id)
         VALUES (?1, ?2, ?3, ?4, ?5, 'open', ?6, ?7, ?8, ?9)",
        params![question_id, seat, crate::seat_ids::rig_or_empty(seat), crate::store::enum_text(source), json_text(prompt), ev.at_ms, ev.seq, deadline, turn_id],
    )?;
    if n == 1 {
        tx.execute(
            "UPDATE seats SET open_questions = open_questions + 1 WHERE id = ?1",
            [seat],
        )?;
    }
    Ok(())
}

/// How a question ends or is marked: answered (in time or late), expired, decided, withdrawn, seen.
fn settle(tx: &Transaction, seat: &str, ev: &EventRecord) -> rusqlite::Result<()> {
    match &ev.body {
        EventBody::QuestionAnswered {
            question_id,
            answer,
            late: false,
        } => {
            if close(tx, seat, question_id, QuestionState::Answered)? {
                tx.execute(
                    "UPDATE questions SET answer_json = ?2, answered_at_ms = ?3 WHERE id = ?1",
                    params![question_id, json_text(answer), ev.at_ms],
                )?;
            }
        }
        EventBody::QuestionAnswered {
            question_id,
            answer,
            late: true,
        } => {
            // Acting on an expired question is also seeing it: it leaves Needs you.
            tx.execute(
                "UPDATE questions SET late_answer_json = ?2, answered_at_ms = ?3, acknowledged = 1 WHERE id = ?1 AND state = 'expired'",
                params![question_id, json_text(answer), ev.at_ms],
            )?;
        }
        EventBody::QuestionExpired { question_id } => {
            if close(tx, seat, question_id, QuestionState::Expired)? {
                tx.execute(
                    "UPDATE questions SET expired_seq = ?2 WHERE id = ?1",
                    params![question_id, ev.seq],
                )?;
            }
        }
        EventBody::QuestionDecided {
            question_id,
            decision,
            rationale,
            source,
        } => {
            decide(tx, ev, question_id, decision, rationale.as_deref(), *source)?;
        }
        EventBody::QuestionWithdrawn {
            question_id,
            reason,
        } => withdraw(tx, seat, question_id, reason)?,
        EventBody::QuestionAcknowledged { question_id } => {
            tx.execute(
                "UPDATE questions SET acknowledged = 1 WHERE id = ?1",
                [question_id],
            )?;
        }
        _ => {}
    }
    Ok(())
}

/// The agent's decision. A structured one (record_decision) replaces an unstructured one;
/// otherwise the first one stands.
fn decide(
    tx: &Transaction,
    ev: &EventRecord,
    question_id: &str,
    decision: &str,
    rationale: Option<&str>,
    source: DecisionSource,
) -> rusqlite::Result<()> {
    let only_first = if source == DecisionSource::RecordDecision {
        ""
    } else {
        " AND decision_source IS NULL"
    };
    tx.execute(
        &format!(
            "UPDATE questions SET decision_text = ?2, decision_rationale = ?3, decision_source = ?4,
                 decision_provenance = ?5, decision_at_ms = ?6 WHERE id = ?1{only_first}"
        ),
        params![
            question_id,
            decision,
            rationale,
            crate::store::enum_text(&source),
            crate::store::enum_text(&ev.provenance),
            ev.at_ms
        ],
    )?;
    Ok(())
}

/// Closes an open question as withdrawn, with why.
fn withdraw(tx: &Transaction, seat: &str, question_id: &str, reason: &str) -> rusqlite::Result<()> {
    if close(tx, seat, question_id, QuestionState::Withdrawn)? {
        tx.execute(
            "UPDATE questions SET withdrawn_reason = ?2 WHERE id = ?1",
            params![question_id, reason],
        )?;
    }
    Ok(())
}

/// What the program reported about an answer on its way: settled, failed, in doubt or confirmed.
fn delivery(tx: &Transaction, seat: &str, ev: &EventRecord) -> rusqlite::Result<()> {
    match &ev.body {
        EventBody::QuestionResolved { question_id } => {
            if !close(tx, seat, question_id, QuestionState::Withdrawn)? {
                // An answered (or expired) question: the program took what Sculptured sent, which confirms
                // a delivery in doubt. `settled` keeps a later failed delivery from reopening it.
                tx.execute(
                    "UPDATE questions SET settled = 1, unconfirmed_reason = NULL WHERE id = ?1 AND state IN ('answered', 'expired')",
                    [question_id],
                )?;
                return Ok(());
            }
            tx.execute(
                "UPDATE questions SET withdrawn_reason = ?2 WHERE id = ?1",
                params![question_id, SETTLED_BY_PROGRAM],
            )?;
        }
        EventBody::QuestionAnswerFailed { question_id, .. } => {
            reopen(tx, seat, question_id, ev.at_ms)?
        }
        EventBody::QuestionAnswerUnconfirmed {
            question_id,
            reason,
        } => {
            tx.execute(
                "UPDATE questions SET unconfirmed_reason = ?2 WHERE id = ?1 AND state IN ('answered', 'expired')",
                params![question_id, reason],
            )?;
        }
        EventBody::QuestionAnswerConfirmed { question_id } => {
            tx.execute(
                "UPDATE questions SET unconfirmed_reason = NULL WHERE id = ?1",
                [question_id],
            )?;
        }
        _ => {}
    }
    Ok(())
}

/// Open questions the agent can no longer answer are withdrawn: its process ended, or the asking
/// turn was interrupted or failed.
fn withdraw_on_end(tx: &Transaction, seat: &str, body: &EventBody) -> rusqlite::Result<()> {
    match body {
        EventBody::TenureEnded { .. } => withdraw_open(tx, seat, None, PROCESS_ENDED),
        EventBody::TurnEnded {
            turn_id,
            status: TurnStatus::Interrupted,
        } => withdraw_open(tx, seat, Some(turn_id), TURN_INTERRUPTED),
        // Answers and expiries are recorded before they are delivered, so a question still open here
        // was never answered by Sculptured.
        EventBody::TurnEnded {
            turn_id,
            status: TurnStatus::Failed,
        } => withdraw_open(tx, seat, Some(turn_id), TURN_FAILED),
        // A process lost without a TenureEnded (hostd restarted, the machine slept): `restore::on_hello`
        // records only `attention` for it. `is_live` keeps `stale`, so a link blip withdraws nothing;
        // `stopped`, `failed` and `attention` do.
        EventBody::SeatLifecycleChanged { lifecycle, .. } if !crate::store::is_live(*lifecycle) => {
            withdraw_open(tx, seat, None, PROCESS_ENDED)
        }
        _ => Ok(()),
    }
}

/// One stored question.
#[derive(Clone, Debug)]
pub(crate) struct QuestionRow {
    pub id: String,
    pub seat: String,
    pub rig: String,
    pub source: QuestionSource,
    pub prompt: QuestionPrompt,
    pub state: QuestionState,
    pub opened_at_ms: i64,
    pub deadline_ms: Option<i64>,
    /// The turn that raised it. A contract field (Interfaces); nothing in Task 3 reads it yet.
    #[allow(dead_code)]
    pub turn_id: Option<String>,
    pub answer: Option<Answer>,
    pub answered_at_ms: Option<i64>,
    pub late_answer: Option<Answer>,
    pub expired_seq: Option<i64>,
    pub decision: Option<DecisionView>,
    pub withdrawn_reason: Option<String>,
    /// Why the recorded answer (or expiry) may not have reached the agent; None once confirmed.
    pub unconfirmed: Option<String>,
    pub acknowledged: bool,
}

const COLS: &str = "id, seat, rig, source, prompt_json, state, opened_at_ms, deadline_ms, turn_id, answer_json, answered_at_ms, \
    late_answer_json, expired_seq, decision_text, decision_rationale, decision_source, decision_provenance, decision_at_ms, \
    withdrawn_reason, acknowledged, unconfirmed_reason";

fn row(r: &Row) -> rusqlite::Result<QuestionRow> {
    let decision = match r.get::<_, Option<String>>(15)? {
        Some(source) => Some(DecisionView {
            text: r
                .get::<_, Option<String>>(13)?
                .unwrap_or_else(|| "unknown".into()),
            rationale: r.get(14)?,
            source: crate::store::enum_parse(&source)?,
            provenance: r
                .get::<_, Option<String>>(16)?
                .map(|p| crate::store::enum_parse::<Provenance>(&p))
                .transpose()?
                .unwrap_or(Provenance::Heuristic),
            at_ms: r.get::<_, Option<i64>>(17)?.unwrap_or(0),
        }),
        None => None,
    };
    Ok(QuestionRow {
        id: r.get(0)?,
        seat: r.get(1)?,
        rig: r.get(2)?,
        source: crate::store::enum_parse(&r.get::<_, String>(3)?)?,
        prompt: crate::store::json_parse(&r.get::<_, String>(4)?)?,
        state: crate::store::enum_parse(&r.get::<_, String>(5)?)?,
        opened_at_ms: r.get(6)?,
        deadline_ms: r.get(7)?,
        turn_id: r.get(8)?,
        answer: r
            .get::<_, Option<String>>(9)?
            .map(|s| crate::store::json_parse(&s))
            .transpose()?,
        answered_at_ms: r.get(10)?,
        late_answer: r
            .get::<_, Option<String>>(11)?
            .map(|s| crate::store::json_parse(&s))
            .transpose()?,
        expired_seq: r.get(12)?,
        decision,
        withdrawn_reason: r.get(18)?,
        acknowledged: r.get::<_, i64>(19)? != 0,
        unconfirmed: r.get(20)?,
    })
}

pub(crate) fn questions_where<P: Params>(
    conn: &Connection,
    clause: &str,
    args: P,
) -> rusqlite::Result<Vec<QuestionRow>> {
    let mut q = conn.prepare(&format!("SELECT {COLS} FROM questions WHERE {clause}"))?;
    q.query_map(args, row)?.collect()
}

pub(crate) fn question(conn: &Connection, id: &str) -> rusqlite::Result<Option<QuestionRow>> {
    conn.query_row(
        &format!("SELECT {COLS} FROM questions WHERE id = ?1"),
        [id],
        row,
    )
    .optional()
}

/// Expired agent questions of `seat` whose decision is not recorded yet, with the expiry's seq.
pub(crate) fn undecided_expired(
    conn: &Connection,
    seat: &str,
) -> rusqlite::Result<Vec<(String, i64)>> {
    let mut q = conn.prepare(
        "SELECT id, expired_seq FROM questions WHERE seat = ?1 AND state = 'expired' AND decision_source IS NULL
             AND source NOT IN ('native-permission', 'approval') AND expired_seq IS NOT NULL",
    )?;
    q.query_map([seat], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect()
}

impl QuestionRow {
    pub(crate) fn into_view(self, consequence: Vec<RiskFlag>) -> QuestionView {
        QuestionView {
            id: self.id,
            seat: self.seat,
            rig: self.rig,
            source: self.source,
            prompt: self.prompt,
            state: self.state,
            opened_at_ms: self.opened_at_ms,
            deadline_ms: self.deadline_ms,
            answer: self.answer,
            answered_at_ms: self.answered_at_ms,
            late_answer: self.late_answer,
            decision: self.decision,
            withdrawn_reason: self.withdrawn_reason,
            acknowledged: self.acknowledged,
            consequence,
            expired_seq: self.expired_seq,
            unconfirmed: self.unconfirmed,
        }
    }
}
