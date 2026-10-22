//! Pure translation from driver events to journaled event bodies.
use drivers::DriverEvent;
use model::{Actor, Caps, EventBody, Provenance, SeatActivity, SeatLifecycle, TenureCause};

// The caps live in `model::cap` so `QuestionPrompt::capped` bounds a tool input exactly like `ToolCalled`.
pub use model::cap::{MAX_INPUT_LEAF, MAX_TEXT, cap_input, truncate};

pub struct PumpCtx {
    pub tenure: u32,
    pub driver: String,
    pub caps: Caps,
    /// Set once SessionReady arrives; an exit before that is "exited before ready".
    pub ready: bool,
    /// Why this tenure started (fresh launch, resume, fork, handover); recorded on `TenureStarted`.
    pub cause: TenureCause,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mapped {
    pub actor: Actor,
    pub provenance: Provenance,
    pub body: EventBody,
}

fn last_line(s: &str) -> &str {
    s.lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
}

fn system(body: EventBody) -> Mapped {
    Mapped {
        actor: Actor::System("hostd".into()),
        provenance: Provenance::ProtocolObserved,
        body,
    }
}

fn agent(seat: &str, body: EventBody) -> Mapped {
    Mapped {
        actor: Actor::Seat(seat.to_string()),
        provenance: Provenance::ProtocolObserved,
        body,
    }
}

fn activity(activity: SeatActivity) -> Mapped {
    system(EventBody::SeatActivityChanged { activity })
}

/// One flat match over every driver event; each arm is the event's journaled form.
#[allow(clippy::too_many_lines)] // one flat match over the DriverEvent variants, one short arm each
pub fn map_event(
    ctx: &mut PumpCtx,
    seat: &str,
    ev: DriverEvent,
    stop_requested: bool,
) -> Vec<Mapped> {
    let agent = |body: EventBody| agent(seat, body);
    match ev {
        DriverEvent::SessionReady { native_session_id } => session_ready(ctx, native_session_id),
        DriverEvent::TurnStarted { turn_id } => vec![
            agent(EventBody::TurnStarted { turn_id }),
            activity(SeatActivity::Working),
        ],
        DriverEvent::MessageConsumed {
            message_id,
            turn_id,
        } => vec![agent(EventBody::MessageConsumed {
            message_id,
            turn_id,
        })],
        DriverEvent::Text { turn_id, text } => vec![agent(EventBody::AgentText {
            turn_id,
            text: truncate(text, MAX_TEXT),
        })],
        DriverEvent::ToolCall {
            turn_id,
            call_id,
            name,
            input,
            parent_call_id,
        } => {
            vec![agent(EventBody::ToolCalled {
                turn_id,
                call_id,
                name,
                input: cap_input(input),
                parent_call_id,
            })]
        }
        DriverEvent::ToolResult {
            turn_id,
            call_id,
            output,
            is_error,
        } => {
            // A protection's refusal arrives as the call's error, inside the CLI's own wording.
            let refused = is_error
                .then(|| {
                    output
                        .find(model::protection::MARK)
                        .map(|i| output[i..].to_string())
                })
                .flatten();
            let mut out = vec![agent(EventBody::ToolResulted {
                turn_id,
                call_id: call_id.clone(),
                output: truncate(output, MAX_TEXT),
                is_error,
            })];
            if let Some(reason) = refused {
                out.push(system(EventBody::ProtectionApplied {
                    call_id,
                    reason: model::cap::truncate(reason, MAX_TEXT),
                }));
            }
            out
        }
        DriverEvent::Usage {
            turn_id,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
            cost,
            in_turn,
        } => {
            vec![agent(EventBody::UsageReported {
                turn_id,
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_write_tokens,
                cost,
                in_turn,
            })]
        }
        DriverEvent::TurnEnded { turn_id, status } => vec![
            agent(EventBody::TurnEnded { turn_id, status }),
            activity(SeatActivity::Idle),
        ],
        DriverEvent::QuestionRaised {
            question_id,
            source,
            prompt,
            turn_id,
        } => vec![
            agent(EventBody::QuestionRaised {
                question_id,
                source,
                prompt: prompt.capped(),
                turn_id,
            }),
            activity(SeatActivity::WaitingInput),
        ],
        // controld withdraws the question if it is still open: the program settled it by itself.
        DriverEvent::QuestionResolved { question_id } => {
            vec![
                agent(EventBody::QuestionResolved { question_id }),
                activity(SeatActivity::Working),
            ]
        }
        DriverEvent::LimitReached {
            turn_id,
            kind,
            resets,
            text,
        } => {
            vec![agent(EventBody::LimitReached {
                turn_id,
                kind,
                resets,
                text: truncate(text, MAX_TEXT),
            })]
        }
        // The runtime never started a turn for it: the runner's record (like controld's own
        // MessageFailed), with the runtime's words.
        DriverEvent::MessageFailed { message_id, reason } => {
            vec![system(EventBody::MessageFailed {
                message_id,
                reason: truncate(reason, MAX_TEXT),
            })]
        }
        DriverEvent::ModelObserved {
            requested,
            observed,
            provider,
            effort,
            rerouted_from,
            reason,
        } => {
            vec![system(EventBody::ModelObserved {
                requested,
                observed,
                provider,
                effort,
                rerouted_from,
                reason,
            })]
        }
        // The agent's own words about its reasoning: agent-reported, never protocol-observed.
        DriverEvent::Thinking {
            turn_id,
            item_id,
            parent_call_id,
            form,
            text,
            estimated_tokens,
        } => {
            let text = model::cap::truncate(text, MAX_TEXT);
            let body = EventBody::ThinkingRecorded {
                turn_id,
                item_id,
                parent_call_id,
                form,
                text,
                estimated_tokens,
            };
            vec![Mapped {
                actor: Actor::Seat(seat.to_string()),
                provenance: Provenance::AgentReported,
                body,
            }]
        }
        DriverEvent::Context {
            turn_id,
            used_tokens,
            window_tokens,
            basis,
        } => {
            vec![agent(EventBody::ContextReported {
                turn_id,
                used_tokens,
                window_tokens,
                basis,
            })]
        }
        DriverEvent::PlanUsage(usage) => vec![system(EventBody::PlanUsageReported { usage })],
        DriverEvent::Unrecognized { raw } => vec![agent(EventBody::Unrecognized {
            raw: capped_raw(raw),
        })],
        DriverEvent::Exited {
            exit_code,
            signal,
            stderr_tail,
        } => exited(ctx, exit_code, signal, stderr_tail, stop_requested),
    }
}

fn session_ready(ctx: &mut PumpCtx, native_session_id: String) -> Vec<Mapped> {
    ctx.ready = true;
    vec![
        system(EventBody::TenureStarted {
            tenure: ctx.tenure,
            cause: ctx.cause,
            driver: ctx.driver.clone(),
            native_session_id: Some(native_session_id),
            caps: ctx.caps,
        }),
        system(EventBody::SeatLifecycleChanged {
            lifecycle: SeatLifecycle::Running,
            reason: None,
        }),
        activity(SeatActivity::Idle),
    ]
}

/// Raw evidence over `MAX_TEXT` is kept as truncated text.
fn capped_raw(raw: serde_json::Value) -> serde_json::Value {
    let text = raw.to_string();
    if text.len() > MAX_TEXT {
        serde_json::Value::String(model::cap::truncate(text, MAX_TEXT))
    } else {
        raw
    }
}

fn exited(
    ctx: &PumpCtx,
    exit_code: Option<i32>,
    signal: Option<i32>,
    stderr_tail: String,
    stop_requested: bool,
) -> Vec<Mapped> {
    let (lifecycle, reason) = if stop_requested {
        (SeatLifecycle::Stopped, None)
    } else {
        let how = if ctx.ready {
            "exited unexpectedly"
        } else {
            "exited before ready"
        };
        (
            SeatLifecycle::Attention,
            Some(format!(
                "{how} ({}): {}",
                exit_words(exit_code, signal),
                last_line(&stderr_tail)
            )),
        )
    };
    vec![
        system(EventBody::TenureEnded {
            tenure: ctx.tenure,
            exit_code,
            signal,
            stderr_tail: truncate(stderr_tail, MAX_TEXT),
        }),
        system(EventBody::SeatLifecycleChanged { lifecycle, reason }),
        activity(SeatActivity::Unknown),
    ]
}

/// How a process ended, in plain words: "code 3", "signal 9", or "code unknown".
fn exit_words(exit_code: Option<i32>, signal: Option<i32>) -> String {
    match (exit_code, signal) {
        (Some(c), _) => format!("code {c}"),
        (None, Some(s)) => format!("signal {s}"),
        (None, None) => "code unknown".to_string(),
    }
}
