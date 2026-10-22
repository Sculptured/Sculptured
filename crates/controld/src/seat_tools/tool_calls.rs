use super::{
    Actor, Answer, AppState, CompiledSeat, EventBody, PermissionPosture, Provenance, Runtime,
    SeatToolsParams, Subject, ToHost, UnboundedSender, Value, ask, oneshot, records,
};

/// A `request_input` or `approve` call waiting for its answer (`AppState::seat_waits`).
pub struct SeatWait {
    pub(super) answer: oneshot::Sender<Answer>,
}

impl SeatWait {
    /// Hands the answer to the waiting call. Err (with the answer) when the call stopped waiting.
    /// `questions::deliver` then records the answer as not sent. A computer that is offline is no
    /// reason: the reply waits for it (the runner sends the call again when it reconnects).
    pub fn send(self, answer: Answer) -> Result<(), Answer> {
        self.answer.send(answer)
    }
}

/// Claude, Codex and Practice agents get the tools; `approve` is Claude's permission-prompt tool, so
/// only a restricted Claude agent gets it (Codex asks natively, Task 13; Practice uses `perm:`).
#[must_use]
pub fn launch_params(
    st: &AppState,
    seat: &CompiledSeat,
    posture: PermissionPosture,
) -> Option<SeatToolsParams> {
    let workflow = crate::seat_ids::rig_of(&seat.id)
        .and_then(|rig| {
            st.store
                .lock()
                .expect("store mutex must not be poisoned")
                .draft(rig)
                .ok()
                .flatten()
        })
        .is_none_or(|d| d.workflow.is_some());
    matches!(
        seat.runtime,
        Runtime::ClaudeCode | Runtime::Codex | Runtime::Stub
    )
    .then(|| SeatToolsParams {
        approve: posture == PermissionPosture::Restricted && seat.runtime == Runtime::ClaudeCode,
        workflow,
    })
}

/// Defence in depth behind hostd's own check: `approve` only for an agent that would be launched with it.
pub(super) fn may_approve(st: &AppState, seat: &str) -> bool {
    let Ok((rig, spec)) = crate::launch::compiled_seat(st, seat) else {
        return false;
    };
    let posture = crate::posture::for_seat(st, &rig.name, &spec);
    launch_params(st, &spec, posture).is_some_and(|p| p.approve)
}

pub(super) enum ToolError {
    /// The call itself was wrong: kept as `report-rejected` evidence.
    Invalid(String),
    /// A valid call that could not complete (the question was withdrawn, the store failed).
    Failed(String),
}

pub(super) fn invalid(s: impl Into<String>) -> ToolError {
    ToolError::Invalid(s.into())
}

pub(super) fn cut(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

/// The most of a rejected call's arguments kept as evidence.
pub(super) const RAW_BYTES: usize = 16 * 1024;

/// The call's arguments as evidence: whole when small, else their first `RAW_BYTES` bytes as text.
pub(super) fn raw_evidence(args: &Value) -> Value {
    let s = args.to_string();
    if s.len() <= RAW_BYTES {
        return args.clone();
    }
    Value::String(format!("{}…", model::cap::head(&s, RAW_BYTES)))
}

/// Records an event the agent's call reports (actor: the agent).
pub(super) fn record(
    st: &AppState,
    seat: &str,
    provenance: Provenance,
    body: EventBody,
) -> Result<(), ToolError> {
    st.local_event(
        Actor::Seat(seat.to_string()),
        Subject::Seat(seat.to_string()),
        provenance,
        body,
    )
    .map(|_| ())
    .map_err(|e| ToolError::Failed(format!("Sculptured could not record this: {e}")))
}

/// Records an event about a seat-tool call as Sculptured (system:controld).
pub(super) fn record_system(st: &AppState, seat: &str, provenance: Provenance, body: EventBody) {
    if let Err(e) = st.local_event(
        Actor::System("controld".into()),
        Subject::Seat(seat.to_string()),
        provenance,
        body,
    ) {
        tracing::error!(%seat, error = %e, "cannot record a seat-tool call's outcome");
    }
}

pub(super) fn current_turn(st: &AppState, seat: &str) -> Option<String> {
    st.store
        .lock()
        .expect("store mutex must not be poisoned")
        .seat(seat)
        .ok()
        .flatten()
        .and_then(|v| v.current_turn)
}

pub(super) fn string_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    match args.get(key).and_then(Value::as_str).map(str::trim) {
        Some(s) if !s.is_empty() => Ok(s),
        _ => Err(invalid(format!("{key} must be a non-empty string"))),
    }
}

/// What a call returns, and the question it asked and the answer it got (`request_input`, `approve`).
pub(super) type Outcome = (Value, Option<(String, Answer)>);

/// Handles one call and sends its reply on `reply`, which passes it to the host's link of the moment
/// (`hosts::session::on_call`): a link that dropped meanwhile is no reason to lose it.
pub async fn handle_call(
    st: AppState,
    reply: UnboundedSender<ToHost>,
    call_id: String,
    seat: String,
    tool: String,
    args: Value,
) {
    let (ok, result, error, asked) = match dispatch(&st, &seat, &tool, &args).await {
        Ok((result, asked)) => (true, result, None, asked),
        Err(ToolError::Invalid(reason)) => {
            let body = EventBody::ReportRejected {
                tool: cut(&tool, 200),
                reason: reason.clone(),
                raw: raw_evidence(&args),
            };
            if let Err(ToolError::Failed(e)) =
                record(&st, &seat, Provenance::ProtocolObserved, body)
            {
                tracing::error!(%seat, error = %e, "cannot record a rejected tool call");
            }
            (false, Value::Null, Some(reason), None)
        }
        Err(ToolError::Failed(reason)) => (false, Value::Null, Some(reason), None),
    };
    // The reply could not be passed on (controld is shutting down): the agent never gets the answer.
    // A computer that dropped off is not this case; its reply waits for it (`HostRegistry::finish_call`).
    if reply
        .send(ToHost::SeatToolReply {
            call_id,
            ok,
            result,
            error,
        })
        .is_err()
        && let Some((id, answer)) = asked
    {
        ask::lost_on_the_way(&st, &seat, &id, Some(answer)).await;
    }
}

pub(super) async fn dispatch(
    st: &AppState,
    seat: &str,
    tool: &str,
    args: &Value,
) -> Result<Outcome, ToolError> {
    match tool {
        "spawn_agent" | "send_agent_message" | "team_status" => {
            crate::collaboration::tools::call(st, seat, tool, args)
                .await
                .map(|v| (v, None))
                .map_err(invalid)
        }
        "request_input" => ask::request_input(st, seat, args).await,
        "approve" if !may_approve(st, seat) => {
            Err(invalid("the approve tool is not available to this agent"))
        }
        "approve" => ask::approve(st, seat, args).await,
        "record_decision" => records::record_decision(st, seat, args).map(|v| (v, None)),
        "report" => records::report(st, seat, args).map(|v| (v, None)),
        "dispatch_work" | "complete_work" | "workflow_status" => {
            crate::workflow::call(st, seat, tool, args)
                .map(|v| (v, None))
                .map_err(invalid)
        }
        "read_decisions" => crate::decisions::read_tool(st, seat, args)
            .map(|v| (v, None))
            .map_err(invalid),
        "search_history" => crate::knowledge::search_history(st, seat, args)
            .map(|v| (v, None))
            .map_err(invalid),
        "record_lesson" => crate::knowledge::record_lesson(st, seat, args)
            .map(|v| (v, None))
            .map_err(invalid),
        "read_skill" => crate::skills::read_skill(st, seat, args)
            .map(|v| (v, None))
            .map_err(invalid),
        other => Err(invalid(format!("unknown tool {:?}", cut(other, 200)))),
    }
}
