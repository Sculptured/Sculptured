//! Deterministic driver with no model: used by demos, integration tests and e2e. Practice scripts (the
//! message text): `hang`, `crash`, `fail`, `limit`, `tool:<cmd>`, `ask:`, `ask-only:`, `ask-silent:`,
//! `perm:`, `request:`, `report:`, `<tool>:<json>` for the collaboration tools, `propose_change` and `write_status`;
//! anything else is echoed as `stub received: <text>`.
use crate::{
    Driver, DriverError, DriverEvent, LaunchSpec, OutboundMessage, SeatToolsLaunch, Session,
};
use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};
use host_proto::seat_tools::{SOCKET_ENV, TOKEN_ENV};
use model::{
    Answer, Caps, PermissionPosture, QuestionOption, QuestionPrompt, QuestionSource, TurnStatus,
};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_stream::wrappers::UnboundedReceiverStream;

/// Deterministic driver with no model: used by demos, integration tests and e2e.
pub struct StubDriver;

#[async_trait]
impl Driver for StubDriver {
    fn name(&self) -> &'static str {
        "stub"
    }
    fn capabilities(&self) -> Caps {
        Caps {
            interrupt: true,
            resume: true,
            usage: true,
            fork: true,
            questions: true,
            ..Caps::default()
        }
    }
    async fn launch(&self, spec: LaunchSpec) -> Result<Box<dyn Session>, DriverError> {
        crate::check_cwd(&spec)?;
        Ok(Box::new(StubSession::start(
            format!("stub-{}", spec.seat),
            &spec,
        )))
    }
    async fn resume(
        &self,
        spec: LaunchSpec,
        native_id: &str,
    ) -> Result<Box<dyn Session>, DriverError> {
        crate::check_cwd(&spec)?;
        Ok(Box::new(StubSession::start(native_id.to_string(), &spec)))
    }
    async fn fork(
        &self,
        spec: LaunchSpec,
        _native_id: &str,
    ) -> Result<Box<dyn Session>, DriverError> {
        crate::check_cwd(&spec)?;
        let id = format!(
            "stub-fork-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        );
        Ok(Box::new(StubSession::start(id, &spec)))
    }
}

/// What a pending question will do with its answer.
enum AskKind {
    /// `ask:` / `ask-only:` / `ask-silent:`: on expiry, decide the first option (or say nothing when silent).
    Question {
        fallback: Option<String>,
        silent: bool,
    },
    /// `perm:` in restricted posture: allow (or expiry, D8) runs the command, deny does not.
    Permission { command: String },
}

/// The question the stub is waiting on; its turn stays open until it is answered.
struct Ask {
    id: String,
    turn: String,
    kind: AskKind,
}

/// What the turn says once its seat-tool call returns.
enum AfterTool {
    Collaboration,
    Request { options: Vec<String> },
    Report { summary: String },
}

/// The session's state, shared with a turn that waits on a seat-tool call, so that turn can end by
/// itself (and drain the queue) when the call returns.
struct Core {
    tx: Option<UnboundedSender<DriverEvent>>,
    turns: u32,
    /// The turn that is still open (`hang`, a question, a seat-tool call).
    hanging: Option<String>,
    queued: VecDeque<OutboundMessage>,
    /// The question this session is waiting on (`ask:`, `ask-only:`, `ask-silent:`, restricted `perm:`).
    asking: Option<Ask>,
    /// The agent's permission posture (`perm:` asks only when restricted).
    posture: PermissionPosture,
    /// The seat-tools launch, when hostd offers them (`request:` and `report:` call through it).
    seat_tools: Option<SeatToolsLaunch>,
}

type Shared = Arc<Mutex<Core>>;

impl Core {
    fn emit(&self, e: DriverEvent) -> Result<(), DriverError> {
        self.tx
            .as_ref()
            .ok_or(DriverError::Closed)?
            .send(e)
            .map_err(|_| DriverError::Closed)
    }

    fn exit(&mut self, code: i32, tail: &str) {
        let _ = self.emit(DriverEvent::Exited {
            exit_code: Some(code),
            signal: None,
            stderr_tail: tail.into(),
        });
        self.tx = None;
    }

    fn run_tool(&self, turn: &str, cmd: &str) -> Result<(), DriverError> {
        let call_id = format!("{turn}-call");
        self.emit(DriverEvent::ToolCall {
            turn_id: Some(turn.to_string()),
            call_id: call_id.clone(),
            name: "Bash".into(),
            input: json!({ "command": cmd }),
            parent_call_id: None,
        })?;
        self.emit(DriverEvent::ToolResult {
            turn_id: Some(turn.to_string()),
            call_id,
            output: format!("ran {cmd}"),
            is_error: false,
        })
    }

    fn finish(&self, turn: String, reply: Option<String>) -> Result<(), DriverError> {
        if let Some(text) = reply {
            self.emit(DriverEvent::Text {
                turn_id: Some(turn.clone()),
                text,
            })?;
        }
        self.emit(DriverEvent::Usage {
            turn_id: Some(turn.clone()),
            input_tokens: 10,
            output_tokens: 5,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost: None,
            in_turn: false,
        })?;
        self.emit(DriverEvent::TurnEnded {
            turn_id: turn,
            status: TurnStatus::Completed,
        })
    }

    /// Raises a question and holds the turn open until `answer`, `interrupt` or `stop`.
    fn ask(
        &mut self,
        turn: String,
        source: QuestionSource,
        prompt: QuestionPrompt,
        kind: AskKind,
    ) -> Result<(), DriverError> {
        // Question ids live in one table for every agent and session, so they must be unique everywhere.
        let id = format!("q-{}", uuid::Uuid::new_v4().simple());
        self.emit(DriverEvent::QuestionRaised {
            question_id: id.clone(),
            source,
            prompt,
            turn_id: Some(turn.clone()),
        })?;
        self.asking = Some(Ask {
            id,
            turn: turn.clone(),
            kind,
        });
        self.hanging = Some(turn);
        Ok(())
    }
}

/// `first|rest|…` → (first, the non-empty rest).
fn split(rest: &str) -> (String, Vec<String>) {
    let mut parts = rest.split('|').map(str::trim);
    let first = parts.next().unwrap_or_default().to_string();
    (
        first,
        parts
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

fn run_turn(shared: &Shared, core: &mut Core, msg: OutboundMessage) -> Result<(), DriverError> {
    core.turns += 1;
    let turn = format!("turn-{}", core.turns);
    core.emit(DriverEvent::TurnStarted {
        turn_id: turn.clone(),
    })?;
    core.emit(DriverEvent::MessageConsumed {
        message_id: msg.message_id,
        turn_id: turn.clone(),
    })?;
    let text = msg.text.trim().to_string();
    match text.as_str() {
        "hang" => {
            core.hanging = Some(turn);
            Ok(())
        }
        "crash" => {
            core.exit(3, "stub crash");
            Ok(())
        }
        "limit" => limit_turn(core, turn),
        "fail" => {
            core.emit(DriverEvent::Text {
                turn_id: Some(turn.clone()),
                text: "stub failed this turn".into(),
            })?;
            core.emit(DriverEvent::TurnEnded {
                turn_id: turn,
                status: TurnStatus::Failed,
            })
        }
        _ => scripted_turn(shared, core, turn, &text),
    }
}

/// The user's real Claude run ended a turn this way (Task 1 reads the CLI's words).
fn limit_turn(core: &mut Core, turn: String) -> Result<(), DriverError> {
    let words = "You've hit your session limit · resets 4:30am (America/New_York)";
    let notice = DriverEvent::LimitReached {
        turn_id: Some(turn.clone()),
        kind: model::LimitKind::Usage,
        resets: model::limit_resets(words),
        text: words.into(),
    };
    core.emit(notice)?;
    core.emit(DriverEvent::TurnEnded {
        turn_id: turn,
        status: TurnStatus::Failed,
    })
}

/// `ask-silent:` / `ask-only:` / `ask:` → (rest, silent on expiry, other answers allowed);
/// `ask-only:` takes just its options.
fn ask_script(text: &str) -> Option<(&str, bool, bool)> {
    [
        ("ask-silent:", true, true),
        ("ask-only:", false, false),
        ("ask:", false, true),
    ]
    .into_iter()
    .find_map(|(prefix, silent, other)| text.strip_prefix(prefix).map(|r| (r, silent, other)))
}

/// The prefixed scripts; anything else is echoed (after running a `tool:` or unrestricted `perm:` command).
fn scripted_turn(
    shared: &Shared,
    core: &mut Core,
    turn: String,
    text: &str,
) -> Result<(), DriverError> {
    if let Some((rest, silent, allow_other)) = ask_script(text) {
        return ask_turn(core, turn, rest, silent, allow_other);
    }
    for tool in [
        "spawn_agent",
        "send_agent_message",
        "team_status",
        "propose_change",
        "write_status",
    ] {
        if let Some(rest) = text.strip_prefix(&format!("{tool}:")) {
            let args = match serde_json::from_str(rest) {
                Ok(v) => v,
                Err(e) => return core.finish(turn, Some(format!("Invalid Practice JSON: {e}"))),
            };
            return tool_turn(shared, core, turn, tool, args, AfterTool::Collaboration);
        }
    }
    if let Some(rest) = text.strip_prefix("request:") {
        let (question, options) = split(rest);
        let args = json!({ "question": question, "options": options });
        return tool_turn(
            shared,
            core,
            turn,
            "request_input",
            args,
            AfterTool::Request { options },
        );
    }
    if let Some(rest) = text.strip_prefix("report:") {
        let (summary, findings) = split(rest);
        let verdict = if findings.is_empty() {
            "no-problems"
        } else {
            "problems-found"
        };
        let args = json!({ "summary": summary, "findings": findings, "verdict": verdict });
        return tool_turn(
            shared,
            core,
            turn,
            "report",
            args,
            AfterTool::Report { summary },
        );
    }
    if let Some(cmd) = text.strip_prefix("perm:").map(str::trim) {
        if core.posture == PermissionPosture::Restricted {
            return permission_turn(core, turn, cmd);
        }
        core.run_tool(&turn, cmd)?;
    }
    if let Some(cmd) = text.strip_prefix("tool:") {
        core.run_tool(&turn, cmd.trim())?;
    }
    core.finish(turn, Some(format!("stub received: {text}")))
}

fn ask_turn(
    core: &mut Core,
    turn: String,
    rest: &str,
    silent: bool,
    allow_other: bool,
) -> Result<(), DriverError> {
    let (question, options) = split(rest);
    core.emit(DriverEvent::Text {
        turn_id: Some(turn.clone()),
        text: format!("stub asks: {question}"),
    })?;
    let prompt = QuestionPrompt {
        urgency: None,
        header: None,
        text: question,
        options: options
            .iter()
            .map(|l| QuestionOption {
                label: l.clone(),
                description: None,
            })
            .collect(),
        allow_other,
        tool: None,
        tool_input: None,
    };
    let kind = AskKind::Question {
        fallback: options.first().cloned(),
        silent,
    };
    core.ask(turn, QuestionSource::NativeQuestion, prompt, kind)
}

/// `perm:` in restricted posture asks before running the command.
fn permission_turn(core: &mut Core, turn: String, cmd: &str) -> Result<(), DriverError> {
    let prompt = QuestionPrompt {
        urgency: None,
        header: None,
        text: format!("Allow Bash to run `{cmd}`?"),
        options: vec![],
        allow_other: false,
        tool: Some("Bash".into()),
        tool_input: Some(json!({ "command": cmd })),
    };
    core.ask(
        turn,
        QuestionSource::NativePermission,
        prompt,
        AskKind::Permission {
            command: cmd.to_string(),
        },
    )
}

fn drain_queue(shared: &Shared, core: &mut Core) -> Result<(), DriverError> {
    while core.hanging.is_none() && core.tx.is_some() {
        let Some(next) = core.queued.pop_front() else {
            break;
        };
        run_turn(shared, core, next)?;
    }
    Ok(())
}

/// The socket and token the shim would use, from the seat-tools environment.
fn tool_env(t: &SeatToolsLaunch) -> Option<(PathBuf, String)> {
    let get = |k: &str| t.env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    Some((PathBuf::from(get(SOCKET_ENV)?), get(TOKEN_ENV)?))
}

async fn seat_call(
    socket: &Path,
    token: &str,
    tool: &'static str,
    args: Value,
) -> Result<Value, String> {
    let (socket, token) = (socket.to_path_buf(), token.to_string());
    tokio::task::spawn_blocking(move || host_proto::seat_tools::call(&socket, &token, tool, args))
        .await
        .map_err(|e| e.to_string())?
}

/// What the turn says once its seat-tool call returns.
async fn tool_reply(
    socket: &Path,
    token: &str,
    tool: &'static str,
    args: Value,
    after: AfterTool,
) -> String {
    match after {
        AfterTool::Collaboration => match seat_call(socket, token, tool, args).await {
            Ok(v) => v.to_string(),
            Err(e) => format!("Practice collaboration failed: {e}"),
        },
        AfterTool::Report { summary } => match seat_call(socket, token, tool, args).await {
            Ok(_) => format!("stub reported: {summary}"),
            Err(e) => format!("stub could not report: {e}"),
        },
        AfterTool::Request { options } => match seat_call(socket, token, tool, args).await {
            Ok(v) if v["timed_out"] == json!(true) => {
                let choice = options
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "to carry on".into());
                let decision = json!({ "question_id": v["question_id"], "choice": choice, "rationale": "a Practice agent picks the first option" });
                match seat_call(socket, token, "record_decision", decision).await {
                    Ok(_) => format!("I decided: {choice}"),
                    Err(e) => format!("I decided: {choice} (recording it failed: {e})"),
                }
            }
            Ok(v) => format!(
                "stub got answer: {}",
                v["text"].as_str().unwrap_or("unknown")
            ),
            Err(e) => format!("stub's question failed: {e}"),
        },
    }
}

/// Calls a seat tool through the real socket in the background; the turn stays open until it returns.
fn tool_turn(
    shared: &Shared,
    core: &mut Core,
    turn: String,
    tool: &'static str,
    args: Value,
    after: AfterTool,
) -> Result<(), DriverError> {
    let Some((socket, token)) = core.seat_tools.as_ref().and_then(tool_env) else {
        return core.finish(
            turn,
            Some(format!(
                "stub has no Sculptured tools, so it could not call {tool}"
            )),
        );
    };
    core.hanging = Some(turn.clone());
    let shared = shared.clone();
    tokio::spawn(async move {
        let reply = tool_reply(&socket, &token, tool, args, after).await;
        let mut core = shared.lock().expect("shared mutex must not be poisoned");
        // Interrupted or stopped meanwhile: that already ended the turn.
        if core.hanging.as_deref() != Some(turn.as_str()) {
            return;
        }
        core.hanging = None;
        let _ = core.finish(turn, Some(reply));
        let _ = drain_queue(&shared, &mut core);
    });
    Ok(())
}

struct StubSession {
    core: Shared,
    rx: Option<UnboundedReceiver<DriverEvent>>,
}

impl StubSession {
    fn start(native_session_id: String, spec: &LaunchSpec) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let _ = tx.send(DriverEvent::SessionReady { native_session_id });
        let core = Core {
            tx: Some(tx),
            turns: 0,
            hanging: None,
            queued: VecDeque::new(),
            asking: None,
            posture: spec.posture,
            seat_tools: spec.seat_tools.clone(),
        };
        Self {
            core: Arc::new(Mutex::new(core)),
            rx: Some(rx),
        }
    }
}

#[async_trait]
impl Session for StubSession {
    async fn send(&mut self, msg: OutboundMessage) -> Result<(), DriverError> {
        let mut core = self.core.lock().expect("core mutex must not be poisoned");
        if core.tx.is_none() {
            return Err(DriverError::Closed);
        }
        if core.hanging.is_some() {
            core.queued.push_back(msg);
            return Ok(());
        }
        run_turn(&self.core, &mut core, msg)
    }

    async fn interrupt(&mut self) -> Result<(), DriverError> {
        let mut core = self.core.lock().expect("core mutex must not be poisoned");
        core.asking = None;
        if let Some(turn_id) = core.hanging.take() {
            core.emit(DriverEvent::TurnEnded {
                turn_id,
                status: TurnStatus::Interrupted,
            })?;
        }
        drain_queue(&self.core, &mut core)
    }

    async fn stop(&mut self, _grace: Duration) -> Result<(), DriverError> {
        let mut core = self.core.lock().expect("core mutex must not be poisoned");
        core.asking = None;
        if let Some(turn_id) = core.hanging.take() {
            let _ = core.emit(DriverEvent::TurnEnded {
                turn_id,
                status: TurnStatus::Interrupted,
            });
        }
        if core.tx.is_some() {
            core.exit(0, "");
        }
        Ok(())
    }

    async fn answer(&mut self, question_id: &str, answer: Answer) -> Result<(), DriverError> {
        let mut core = self.core.lock().expect("core mutex must not be poisoned");
        let Some(ask) = core.asking.take_if(|a| a.id == question_id) else {
            return Err(DriverError::Unsupported(
                "an answer to a question this agent is not asking",
            ));
        };
        core.hanging = None;
        core.emit(DriverEvent::QuestionResolved {
            question_id: question_id.to_string(),
        })?;
        let turn = ask.turn;
        let reply = match (ask.kind, answer) {
            (AskKind::Question { .. }, Answer::Text { text }) => {
                Some(format!("stub got answer: {text}"))
            }
            (AskKind::Question { silent: true, .. }, Answer::Expired) => None,
            (AskKind::Question { fallback, .. }, Answer::Expired) => Some(format!(
                "No answer came, so I decided: {}",
                fallback.unwrap_or_else(|| "to carry on".into())
            )),
            (AskKind::Question { .. }, Answer::Allow) => Some("stub got answer: allow".into()),
            (AskKind::Question { .. }, Answer::Deny { .. }) => Some("stub got answer: deny".into()),
            (AskKind::Permission { command }, Answer::Allow | Answer::Expired) => {
                core.run_tool(&turn, &command)?;
                Some(format!("stub ran {command}"))
            }
            (AskKind::Permission { command }, Answer::Deny { .. }) => {
                Some(format!("stub was not allowed to run {command}"))
            }
            (AskKind::Permission { .. }, Answer::Text { text }) => {
                Some(format!("stub got answer: {text}"))
            }
        };
        core.finish(turn, reply)?;
        drain_queue(&self.core, &mut core)
    }

    fn take_events(&mut self) -> Option<BoxStream<'static, DriverEvent>> {
        self.rx
            .take()
            .map(|rx| UnboundedReceiverStream::new(rx).boxed())
    }
}
