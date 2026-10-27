//! `request_input` and `approve`: a Needs-you question with a waiter, and the wait for its answer.
use super::{Outcome, SeatWait, ToolError};
use crate::state::AppState;
use model::{
    Answer, EventBody, Provenance, QuestionOption, QuestionPrompt, QuestionSource, QuestionState,
    SeatActivity, Subject,
};
use serde_json::{Value, json};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::oneshot;

/// Why a question raised through a seat tool is withdrawn when its call stopped waiting unanswered. A
/// computer dropping off is not such a reason: the question stays open and the call waits for it.
const STOPPED_WAITING: &str =
    "the agent's call stopped waiting for an answer; the agent was told the question failed";
/// Why an answer is recorded as failed when Sculptured could no longer pass the call's reply on.
const ANSWER_LOST: &str = "Sculptured could not pass the answer on to the agent's call";

pub(super) async fn request_input(
    st: &AppState,
    seat: &str,
    args: &Value,
) -> Result<Outcome, ToolError> {
    let text = crate::seat_tools::string_arg(args, "question")?.to_string();
    let options = match args.get("options") {
        None | Some(Value::Null) => vec![],
        Some(Value::Array(a)) => a.iter().map(option_of).collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err(crate::seat_tools::invalid("options must be a list")),
    };
    let header = args
        .get("header")
        .and_then(Value::as_str)
        .map(str::to_string);
    let urgency = match args.get("urgency") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value::<model::Urgency>(v.clone())
                .map_err(|_| crate::seat_tools::invalid("urgency must be low, normal or high"))?,
        ),
    };
    let prompt = QuestionPrompt {
        urgency,
        header,
        text,
        options,
        allow_other: true,
        tool: None,
        tool_input: None,
    };
    let (id, answer) = ask(st, seat, QuestionSource::RequestInput, prompt).await?;
    let (text, timed_out) = match &answer {
        Answer::Expired => (expired_text(st, seat, &id), true),
        Answer::Text { text } => (text.clone(), false),
        Answer::Allow => ("allow".to_string(), false),
        Answer::Deny { reason } => (
            format!(
                "deny{}",
                reason
                    .as_ref()
                    .map(|r| format!(": {r}"))
                    .unwrap_or_default()
            ),
            false,
        ),
    };
    Ok((
        json!({ "text": text, "question_id": id, "timed_out": timed_out }),
        Some((id, answer)),
    ))
}

/// D8's instruction for an agent question nobody answered, naming the question to decide on.
fn expired_text(st: &AppState, seat: &str, id: &str) -> String {
    let instruction = model::timeout_instruction(waited_ms(st, seat, id));
    format!("{instruction} When you have decided, call record_decision with question_id \"{id}\".")
}

/// How long the question waited before it expired, from its own deadline; the setting when unknown.
fn waited_ms(st: &AppState, seat: &str, id: &str) -> u64 {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let row = crate::projections::questions::question(store.connection(), id)
        .ok()
        .flatten();
    match row.and_then(|q| q.deadline_ms.map(|d| d - q.opened_at_ms)) {
        Some(ms) if ms > 0 => ms as u64,
        _ => store
            .question_timeout_ms(crate::seat_ids::rig_of(seat).unwrap_or_default())
            .unwrap_or(600_000),
    }
}

fn option_of(v: &Value) -> Result<QuestionOption, ToolError> {
    let bad = || {
        crate::seat_tools::invalid("each option must be a non-empty string or {label, description}")
    };
    match v {
        Value::String(s) if !s.trim().is_empty() => Ok(QuestionOption {
            label: s.trim().to_string(),
            description: None,
        }),
        Value::Object(o) => match o.get("label").and_then(Value::as_str).map(str::trim) {
            Some(l) if !l.is_empty() => Ok(QuestionOption {
                label: l.to_string(),
                description: o
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            }),
            _ => Err(bad()),
        },
        _ => Err(bad()),
    }
}

/// Claude's permission-prompt tool: `{tool_name, input, tool_use_id}` in, Claude's behaviour JSON out.
/// D8: a prompt nobody answers in time is allowed (`questions::expire` records that decision).
pub(super) async fn approve(st: &AppState, seat: &str, args: &Value) -> Result<Outcome, ToolError> {
    let tool_name = crate::seat_tools::string_arg(args, "tool_name")?.to_string();
    let input = args
        .get("input")
        .cloned()
        .filter(Value::is_object)
        .ok_or_else(|| crate::seat_tools::invalid("input must be an object"))?;
    let text = format!("Allow {tool_name} to run?");
    let prompt = QuestionPrompt {
        urgency: None,
        header: None,
        text,
        options: vec![],
        allow_other: false,
        tool: Some(tool_name),
        tool_input: Some(input.clone()),
    };
    let (id, answer) = ask(st, seat, QuestionSource::NativePermission, prompt).await?;
    let behaviour = match &answer {
        Answer::Allow | Answer::Expired => json!({ "behavior": "allow", "updatedInput": input }),
        Answer::Deny { reason } => {
            let message = reason
                .clone()
                .unwrap_or_else(|| "The person running this team did not allow this.".into());
            json!({ "behavior": "deny", "message": message })
        }
        Answer::Text { text } => json!({ "behavior": "deny", "message": text }),
    };
    Ok((json!({ "text": behaviour.to_string() }), Some((id, answer))))
}

/// Raises a question with a waiter and waits for the answer, the expiry or a withdrawal. While it
/// waits the agent is `waiting-input` (spec §4.2), as for a native question; afterwards it is
/// `working` again once no question of it is open, unless it stopped meanwhile.
pub(super) async fn ask(
    st: &AppState,
    seat: &str,
    source: QuestionSource,
    prompt: QuestionPrompt,
) -> Result<(String, Answer), ToolError> {
    let id = format!("q-{}", uuid::Uuid::new_v4().simple());
    let (tx, rx) = oneshot::channel();
    // The waiter exists before the question does, so an answer or expiry always finds it.
    st.seat_waits
        .lock()
        .expect("seat_waits mutex must not be poisoned")
        .insert(id.clone(), SeatWait { answer: tx });
    let body = EventBody::QuestionRaised {
        question_id: id.clone(),
        source,
        prompt: prompt.capped(),
        turn_id: crate::seat_tools::current_turn(st, seat),
    };
    if let Err(e) = crate::seat_tools::record(st, seat, Provenance::AgentReported, body) {
        st.seat_waits
            .lock()
            .expect("seat_waits mutex must not be poisoned")
            .remove(&id);
        return Err(e);
    }
    set_activity(st, seat, SeatActivity::WaitingInput);
    let out = wait_for_answer(st, seat, &id, rx).await;
    let back_to_work = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .seat(seat)
        .ok()
        .flatten()
        .is_some_and(|v| {
            v.activity == SeatActivity::WaitingInput
                && v.open_questions == 0
                && crate::store::is_live(v.lifecycle)
        });
    if back_to_work {
        set_activity(st, seat, SeatActivity::Working);
    }
    out.map(|a| (id, a))
}

/// Records the agent's activity as Sculptured saw it (system:controld, observed).
fn set_activity(st: &AppState, seat: &str, activity: SeatActivity) {
    crate::seat_tools::record_system(
        st,
        seat,
        Provenance::ProtocolObserved,
        EventBody::SeatActivityChanged { activity },
    );
}

/// Waits for the answer (or expiry) or the question's withdrawal (its turn or process ended), whichever
/// comes first. A dropped link is not an end: the computer's runner keeps the call and sends it again.
async fn wait_for_answer(
    st: &AppState,
    seat: &str,
    id: &str,
    mut rx: oneshot::Receiver<Answer>,
) -> Result<Answer, ToolError> {
    // Subscribed before the first check, so a withdrawal recorded in between is not missed.
    let mut events = st.hub.subscribe();
    if let Some(e) = withdrawn(st, id) {
        return Err(e);
    }
    loop {
        tokio::select! {
            biased;
            // The waiter was dropped without an answer: nothing can answer this call any more.
            a = &mut rx => match a {
                Ok(a) => return Ok(a),
                Err(_) => break,
            },
            item = events.recv() => {
                let concerns_agent = match item {
                    Ok(item) => matches!(&item.event.subject, Subject::Seat(s) if s == seat),
                    Err(RecvError::Lagged(_)) => true,
                    Err(RecvError::Closed) => return Err(ToolError::Failed("Sculptured is shutting down".into())),
                };
                if concerns_agent && let Some(e) = withdrawn(st, id) {
                    return Err(e);
                }
            }
        }
    }
    drop(rx);
    lost_on_the_way(st, seat, id, None).await;
    Err(ToolError::Failed(STOPPED_WAITING.into()))
}

/// The question's withdrawal, if the store shows one (an interrupted turn, an ended process, a restart).
fn withdrawn(st: &AppState, id: &str) -> Option<ToolError> {
    let q = crate::projections::questions::question(
        st.store
            .lock()
            .expect("store mutex must not be poisoned")
            .connection(),
        id,
    )
    .ok()
    .flatten()?;
    if q.state != QuestionState::Withdrawn {
        return None;
    }
    st.seat_waits
        .lock()
        .expect("seat_waits mutex must not be poisoned")
        .remove(id);
    let why = q.withdrawn_reason.unwrap_or_else(|| "unknown".into());
    Some(ToolError::Failed(format!(
        "the question was withdrawn: {why}"
    )))
}

/// The call can no longer be answered (its waiter is gone, or its reply could not be passed on). Under
/// the question's claim (so an answer or expiry in flight finishes first), an answer handed over in the
/// meantime is recorded as failed, and a question still open is withdrawn rather than left for someone
/// to answer into a call nobody waits on.
pub(super) async fn lost_on_the_way(st: &AppState, seat: &str, id: &str, answer: Option<Answer>) {
    let _held = crate::questions::tool_claim(id).await;
    st.seat_waits
        .lock()
        .expect("seat_waits mutex must not be poisoned")
        .remove(id);
    if answer.is_some_and(|a| a != Answer::Expired) {
        let body = EventBody::QuestionAnswerFailed {
            question_id: id.to_string(),
            reason: ANSWER_LOST.into(),
        };
        crate::seat_tools::record_system(st, seat, Provenance::ProtocolObserved, body);
    }
    let open = crate::projections::questions::question(
        st.store
            .lock()
            .expect("store mutex must not be poisoned")
            .connection(),
        id,
    )
    .ok()
    .flatten()
    .is_some_and(|q| q.state == QuestionState::Open);
    if open {
        let body = EventBody::QuestionWithdrawn {
            question_id: id.to_string(),
            reason: STOPPED_WAITING.into(),
        };
        crate::seat_tools::record_system(st, seat, Provenance::Heuristic, body);
    }
}

#[cfg(test)]
mod tests;
