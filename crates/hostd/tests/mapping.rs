use drivers::DriverEvent;

use hostd::mapping::{MAX_INPUT_LEAF, MAX_TEXT, Mapped, PumpCtx};

use model::{
    Actor, Caps, ContextBasis, EventBody, LimitKind, Provenance, QuestionPrompt, QuestionSource,
    SeatActivity, SeatLifecycle, TenureCause, ThinkingForm,
};
use serde_json::json;

fn ctx(ready: bool) -> PumpCtx {
    PumpCtx {
        tenure: 1,
        driver: "stub".into(),
        caps: Caps::default(),
        ready,
        cause: TenureCause::Fresh,
    }
}
fn bodies(v: Vec<Mapped>) -> Vec<EventBody> {
    v.into_iter().map(|m| m.body).collect()
}

#[test]
fn truncates_a_200kb_tool_result() {
    let mut c = ctx(true);
    let out = hostd::mapping::map_event(
        &mut c,
        "dev.a@r",
        DriverEvent::ToolResult {
            turn_id: None,
            call_id: "c".into(),
            output: "x".repeat(200_000),
            is_error: false,
        },
        false,
    );
    match &out[0].body {
        EventBody::ToolResulted { output, .. } => {
            assert!(output.len() < MAX_TEXT + 64, "len {}", output.len());
            assert!(
                output.ends_with("[truncated 134464 bytes]"),
                "{}",
                &output[output.len() - 40..]
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(out[0].actor, Actor::Seat("dev.a@r".into()));
    assert_eq!(out[0].provenance, Provenance::ProtocolObserved);
}

#[test]
fn truncate_respects_char_boundaries() {
    let s = "é".repeat(40_000); // 80,000 bytes, 2 bytes per char
    let t = hostd::mapping::truncate(s, 65_537);
    assert!(t.starts_with('é'));
    assert!(t.contains("[truncated "));
    assert_eq!(hostd::mapping::truncate("short".into(), 10), "short");
}

#[test]
fn session_ready_starts_tenure_running_idle() {
    let mut c = ctx(false);
    let out = bodies(hostd::mapping::map_event(
        &mut c,
        "dev.a@r",
        DriverEvent::SessionReady {
            native_session_id: "n1".into(),
        },
        false,
    ));
    assert!(c.ready);
    assert_eq!(
        out,
        vec![
            EventBody::TenureStarted {
                tenure: 1,
                cause: TenureCause::Fresh,
                driver: "stub".into(),
                native_session_id: Some("n1".into()),
                caps: Caps::default(),
            },
            EventBody::SeatLifecycleChanged {
                lifecycle: SeatLifecycle::Running,
                reason: None
            },
            EventBody::SeatActivityChanged {
                activity: SeatActivity::Idle
            },
        ]
    );
}

#[test]
fn exit_lifecycle_depends_on_stop_and_readiness() {
    let exit = || DriverEvent::Exited {
        exit_code: Some(3),
        signal: None,
        stderr_tail: "warn\nboom\n\n".into(),
    };
    let lifecycle_of = |out: Vec<EventBody>| {
        out.into_iter().find_map(|b| match b {
            EventBody::SeatLifecycleChanged { lifecycle, reason } => Some((lifecycle, reason)),
            _ => None,
        })
    };

    let stopped = lifecycle_of(bodies(hostd::mapping::map_event(
        &mut ctx(true),
        "s",
        exit(),
        true,
    )))
    .unwrap();
    assert_eq!(stopped, (SeatLifecycle::Stopped, None));

    let early = lifecycle_of(bodies(hostd::mapping::map_event(
        &mut ctx(false),
        "s",
        exit(),
        false,
    )))
    .unwrap();
    assert_eq!(early.0, SeatLifecycle::Attention);
    assert_eq!(early.1.unwrap(), "exited before ready (code 3): boom");

    let out = bodies(hostd::mapping::map_event(
        &mut ctx(true),
        "s",
        exit(),
        false,
    ));
    assert!(matches!(
        &out[0],
        EventBody::TenureEnded {
            tenure: 1,
            exit_code: Some(3),
            ..
        }
    ));
    assert_eq!(
        out[2],
        EventBody::SeatActivityChanged {
            activity: SeatActivity::Unknown
        }
    );
    let late = lifecycle_of(out).unwrap();
    assert_eq!(late.1.unwrap(), "exited unexpectedly (code 3): boom");
}

#[test]
fn huge_unrecognized_payload_is_truncated_to_a_string() {
    let raw = json!({ "type": "mystery", "blob": "y".repeat(100_000) });
    let out = bodies(hostd::mapping::map_event(
        &mut ctx(true),
        "s",
        DriverEvent::Unrecognized { raw },
        false,
    ));
    match &out[0] {
        EventBody::Unrecognized { raw } => {
            let s = raw
                .as_str()
                .expect("oversized raw is stored as a truncated string");
            assert!(s.contains("[truncated "));
        }
        other => panic!("{other:?}"),
    }
}

/// I3: a Write call with a 200 KB `content` is capped, and its structure survives.
#[test]
fn caps_a_200kb_tool_input_and_keeps_its_structure() {
    let mut c = ctx(true);
    let input = json!({ "file_path": "/w/big.txt", "content": "y".repeat(200_000), "options": { "mode": "overwrite", "lines": [1, 2] } });
    let out = hostd::mapping::map_event(
        &mut c,
        "dev.a@r",
        DriverEvent::ToolCall {
            turn_id: None,
            call_id: "c".into(),
            name: "Write".into(),
            input,
            parent_call_id: None,
        },
        false,
    );
    match &out[0].body {
        EventBody::ToolCalled { input, .. } => {
            let size = serde_json::to_string(input).unwrap().len();
            assert!(size <= MAX_TEXT, "capped input is {size} bytes");
            assert_eq!(input["file_path"], "/w/big.txt");
            assert_eq!(
                input["options"],
                json!({ "mode": "overwrite", "lines": [1, 2] })
            );
            let content = input["content"]
                .as_str()
                .expect("content is still a string");
            assert!(
                content.len() < MAX_INPUT_LEAF + 64,
                "content is {} bytes",
                content.len()
            );
            assert!(content.starts_with("yyyy"));
            assert!(
                content.ends_with(&format!("[truncated {} bytes]", 200_000 - MAX_INPUT_LEAF)),
                "{}",
                &content[content.len() - 40..]
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn small_tool_input_is_unchanged() {
    let input = json!({ "command": "ls -la", "description": "x".repeat(10_000) });
    assert_eq!(
        hostd::mapping::cap_input(input.clone()),
        input,
        "inputs under MAX_TEXT keep long leaves"
    );
}

#[test]
fn exit_reasons_name_a_signal_or_say_unknown() {
    let reason = |exit_code, signal| {
        let e = DriverEvent::Exited {
            exit_code,
            signal,
            stderr_tail: "boom".into(),
        };
        bodies(hostd::mapping::map_event(&mut ctx(true), "s", e, false))
            .into_iter()
            .find_map(|b| match b {
                EventBody::SeatLifecycleChanged { reason, .. } => reason,
                _ => None,
            })
    };
    assert_eq!(
        reason(None, Some(9)).unwrap(),
        "exited unexpectedly (signal 9): boom"
    );
    assert_eq!(
        reason(None, None).unwrap(),
        "exited unexpectedly (code unknown): boom"
    );
}

#[test]
fn questions_map_to_events_and_activity() {
    let mut c = ctx(true);
    let prompt = QuestionPrompt {
        urgency: None,
        header: None,
        text: "x".repeat(9000),
        options: vec![],
        allow_other: true,
        tool: None,
        tool_input: None,
    };
    let out = hostd::mapping::map_event(
        &mut c,
        "dev.a@r",
        DriverEvent::QuestionRaised {
            question_id: "q1".into(),
            source: QuestionSource::NativeQuestion,
            prompt,
            turn_id: Some("t".into()),
        },
        false,
    );
    match &out[0].body {
        EventBody::QuestionRaised {
            question_id,
            prompt,
            ..
        } => {
            assert_eq!(question_id, "q1");
            assert!(prompt.text.len() <= QuestionPrompt::MAX_TEXT + 4, "capped");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(out[0].actor, Actor::Seat("dev.a@r".into()));
    assert_eq!(
        out[1].body,
        EventBody::SeatActivityChanged {
            activity: SeatActivity::WaitingInput
        }
    );
    // The settled question's id reaches controld, which withdraws it if Sculptured never answered it.
    let out = hostd::mapping::map_event(
        &mut c,
        "dev.a@r",
        DriverEvent::QuestionResolved {
            question_id: "q1".into(),
        },
        false,
    );
    assert_eq!(out[0].actor, Actor::Seat("dev.a@r".into()));
    assert_eq!(out[0].provenance, Provenance::ProtocolObserved);
    assert_eq!(
        bodies(out),
        vec![
            EventBody::QuestionResolved {
                question_id: "q1".into()
            },
            EventBody::SeatActivityChanged {
                activity: SeatActivity::Working
            }
        ]
    );
}

#[test]
fn tenure_started_carries_the_launch_cause() {
    let mut c = ctx(false);
    c.cause = TenureCause::Handover;
    let out = hostd::mapping::map_event(
        &mut c,
        "dev.a@r",
        DriverEvent::SessionReady {
            native_session_id: "n".into(),
        },
        false,
    );
    assert!(matches!(
        &out[0].body,
        EventBody::TenureStarted {
            cause: TenureCause::Handover,
            ..
        }
    ));
}

#[test]
fn a_limit_notice_is_recorded_verbatim() {
    let mut c = ctx(true);
    let text = "You've hit your session limit · resets 4:30am (America/New_York)".to_string();
    let ev = DriverEvent::LimitReached {
        turn_id: Some("t".into()),
        kind: LimitKind::Usage,
        resets: Some("4:30am".into()),
        text: text.clone(),
    };
    let out = hostd::mapping::map_event(&mut c, "dev.a@r", ev, false);
    assert_eq!(out[0].actor, Actor::Seat("dev.a@r".into()));
    assert_eq!(
        bodies(out),
        vec![EventBody::LimitReached {
            turn_id: Some("t".into()),
            kind: LimitKind::Usage,
            resets: Some("4:30am".into()),
            text
        }]
    );
}

/// A message the driver gave up on (Codex refused its `turn/start` too often) fails with the
/// runtime's words; the agent never read it, so it is the runner's record, not the agent's.
#[test]
fn a_message_the_driver_gave_up_on_fails() {
    let mut c = ctx(true);
    let reason =
        "Codex did not start a turn for this message (3 tries): a turn is already in progress"
            .to_string();
    let out = hostd::mapping::map_event(
        &mut c,
        "dev.a@r",
        DriverEvent::MessageFailed {
            message_id: "m1".into(),
            reason: reason.clone(),
        },
        false,
    );
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].actor, Actor::System("hostd".into()));
    assert_eq!(out[0].provenance, Provenance::ProtocolObserved);
    assert_eq!(
        bodies(out),
        vec![EventBody::MessageFailed {
            message_id: "m1".into(),
            reason
        }]
    );
}

/// C0: the model a session asked for and the one the CLI reports is the runner's protocol record.
#[test]
fn the_observed_model_is_a_protocol_record() {
    let mut c = ctx(true);
    let ev = DriverEvent::ModelObserved {
        requested: Some("gpt-6-astra".into()),
        observed: Some("gpt-6-sol".into()),
        provider: Some("openai".into()),
        effort: Some("high".into()),
        rerouted_from: Some("gpt-6-astra".into()),
        reason: Some("high demand".into()),
    };
    let out = hostd::mapping::map_event(&mut c, "dev.a@r", ev, false);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].actor, Actor::System("hostd".into()));
    assert_eq!(out[0].provenance, Provenance::ProtocolObserved);
    assert_eq!(
        bodies(out),
        vec![EventBody::ModelObserved {
            requested: Some("gpt-6-astra".into()),
            observed: Some("gpt-6-sol".into()),
            provider: Some("openai".into()),
            effort: Some("high".into()),
            rerouted_from: Some("gpt-6-astra".into()),
            reason: Some("high demand".into()),
        }]
    );
}

fn thinking(text: String) -> DriverEvent {
    DriverEvent::Thinking {
        turn_id: Some("t".into()),
        item_id: Some("rs_1".into()),
        parent_call_id: Some("toolu_9".into()),
        form: ThinkingForm::Summary,
        text,
        estimated_tokens: Some(50),
    }
}

/// C0: thinking is the agent's own words (agent-reported, never protocol-observed), kept verbatim.
#[test]
fn thinking_is_agent_reported_and_verbatim() {
    let mut c = ctx(true);
    let text = "  First, *read* the failing test.\n".to_string();
    let out = hostd::mapping::map_event(&mut c, "dev.a@r", thinking(text.clone()), false);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].actor, Actor::Seat("dev.a@r".into()));
    assert_eq!(out[0].provenance, Provenance::AgentReported);
    let expected = EventBody::ThinkingRecorded {
        turn_id: Some("t".into()),
        item_id: Some("rs_1".into()),
        parent_call_id: Some("toolu_9".into()),
        form: ThinkingForm::Summary,
        text,
        estimated_tokens: Some(50),
    };
    assert_eq!(bodies(out), vec![expected]);
}

#[test]
fn long_thinking_is_capped() {
    let mut c = ctx(true);
    match &hostd::mapping::map_event(&mut c, "dev.a@r", thinking("z".repeat(200_000)), false)[0]
        .body
    {
        EventBody::ThinkingRecorded { text, .. } => {
            assert!(text.len() < MAX_TEXT + 64, "len {}", text.len());
            assert!(text.ends_with("[truncated 134464 bytes]"));
        }
        other => panic!("{other:?}"),
    }
}

/// C0: how full the context was is the agent's protocol record, with its basis kept.
#[test]
fn context_is_a_protocol_record_of_the_agent() {
    let mut c = ctx(true);
    let ev = DriverEvent::Context {
        turn_id: Some("t".into()),
        used_tokens: 18_013,
        window_tokens: Some(258_400),
        basis: ContextBasis::Reported,
    };
    let out = hostd::mapping::map_event(&mut c, "dev.a@r", ev, false);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].actor, Actor::Seat("dev.a@r".into()));
    assert_eq!(out[0].provenance, Provenance::ProtocolObserved);
    assert_eq!(
        bodies(out),
        vec![EventBody::ContextReported {
            turn_id: Some("t".into()),
            used_tokens: 18_013,
            window_tokens: Some(258_400),
            basis: ContextBasis::Reported,
        }]
    );
}

#[test]
fn a_protection_refusal_is_recorded_as_such_and_other_errors_are_not() {
    let result = |output: &str| DriverEvent::ToolResult {
        turn_id: None,
        call_id: "c1".into(),
        output: output.into(),
        is_error: true,
    };
    let out = bodies(hostd::mapping::map_event(
        &mut ctx(true),
        "dev.a@r",
        result("PreToolUse:Read hook error: Sculptured protection: big.csv is 293 KB…"),
        false,
    ));
    assert!(
        matches!(&out[1], EventBody::ProtectionApplied { call_id, reason } if call_id == "c1" && reason.starts_with("Sculptured protection: big.csv")),
        "{out:?}"
    );
    let plain = bodies(hostd::mapping::map_event(
        &mut ctx(true),
        "dev.a@r",
        result("No such file"),
        false,
    ));
    assert_eq!(
        plain.len(),
        1,
        "an ordinary error is only the tool's result"
    );
}
