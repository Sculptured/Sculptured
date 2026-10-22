//! The Codex parser against the scrubbed M0 recordings (Codex 0.160.0) and the M2 refresh
//! recordings (Codex 0.160.1, `codex-m2-*`).
use drivers::DriverEvent;
use drivers::codex::{CodexParser, EXPIRED_ANSWER, Parsed, TOOL_UNFINISHED};
use model::{Answer, LimitKind, QuestionSource, TurnStatus};
use serde_json::{Value, json};
use std::path::PathBuf;

const CODEX_RECORDINGS: [&str; 24] = [
    "codex-approval",
    "codex-basic",
    "codex-fork",
    "codex-interrupt",
    "codex-resume-1",
    "codex-resume-2",
    "codex-steer",
    "codex-tool",
    "codex-user-input",
    "codex-user-input-noflag",
    "codex-m2-approval",
    "codex-m2-basic",
    "codex-m2-interrupt",
    "codex-m2-limits",
    "codex-m2-mcp-cli",
    "codex-m2-mcp-config-dotted",
    "codex-m2-mcp-config-nested",
    "codex-m2-resume",
    "codex-m2-seat-cli",
    "codex-m2-seat-config-dotted",
    "codex-m2-seat-shim",
    "codex-m2-seat-shim-resume",
    "codex-m2-tool",
    "codex-m2-user-input",
];

fn recording(name: &str) -> Vec<Value> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/recordings")
        .join(format!("{name}.jsonl"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing {}: run the M0 spike first", path.display()));
    text.lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// `warning` notifications are kept as evidence, but every recording carries one about the
/// probe user's own hooks config (scrubbed paths): user noise, not protocol (spike doc, M2 refresh).
fn is_warning(e: &DriverEvent) -> bool {
    matches!(e, DriverEvent::Unrecognized { raw } if raw["method"] == "warning")
}

/// `replay_all` without the hooks warnings and plan-usage reports (account-wide, not part of a
/// turn), for tests of the protocol flow.
fn replay(name: &str) -> (Vec<DriverEvent>, Vec<Value>, CodexParser) {
    let (mut events, replies, p) = replay_all(name);
    events.retain(|e| {
        !is_warning(e) && !matches!(e, DriverEvent::PlanUsage(_) | DriverEvent::Context { .. })
    });
    (events, replies, p)
}

/// Replays a recording: each `in` `turn/start` registers message `m1`, `m2`, …; `out` lines
/// are parsed (stderr and meta lines are skipped). Returns the events, the replies the
/// parser asked for, and the parser.
fn replay_all(name: &str) -> (Vec<DriverEvent>, Vec<Value>, CodexParser) {
    let mut p = CodexParser::default();
    let (mut events, mut replies, mut n) = (Vec::new(), Vec::new(), 0);
    for rec in recording(name) {
        let line = &rec["line"];
        match rec["dir"].as_str() {
            Some("in") if line["method"] == "turn/start" => {
                n += 1;
                p.on_turn_start_sent(line["id"].as_u64().unwrap(), format!("m{n}"));
            }
            Some("out") if line.is_object() => {
                let Parsed {
                    events: e,
                    replies: r,
                    ..
                } = p.parse_line(&line.to_string());
                events.extend(e);
                replies.extend(r);
            }
            _ => {}
        }
    }
    (events, replies, p)
}

fn kinds(events: &[DriverEvent]) -> Vec<&'static str> {
    events
        .iter()
        .map(|e| match e {
            DriverEvent::TurnStarted { .. } => "turn-started",
            DriverEvent::MessageConsumed { .. } => "consumed",
            DriverEvent::Text { .. } => "text",
            DriverEvent::ToolCall { .. } => "tool-call",
            DriverEvent::ToolResult { .. } => "tool-result",
            DriverEvent::Usage { .. } => "usage",
            DriverEvent::TurnEnded { .. } => "turn-ended",
            DriverEvent::QuestionRaised { .. } => "question",
            DriverEvent::QuestionResolved { .. } => "resolved",
            DriverEvent::Unrecognized { .. } => "unrecognized",
            _ => "other",
        })
        .collect()
}

fn usage_total(events: &[DriverEvent]) -> u64 {
    events
        .iter()
        .map(|e| match e {
            DriverEvent::Usage {
                input_tokens,
                output_tokens,
                cache_read_tokens,
                ..
            } => input_tokens + output_tokens + cache_read_tokens,
            _ => 0,
        })
        .sum()
}

#[test]
fn no_recording_produces_unrecognized_events_or_replies() {
    for name in CODEX_RECORDINGS {
        let (events, replies, _) = replay_all(name);
        // `warning` notifications are runtime text, kept as evidence; nothing else is unrecognized.
        let warnings = recording(name)
            .iter()
            .filter(|r| r["dir"] == "out" && r["line"]["method"] == "warning")
            .count();
        let unrecognized: Vec<&DriverEvent> = events
            .iter()
            .filter(|e| matches!(e, DriverEvent::Unrecognized { .. }))
            .collect();
        assert!(
            unrecognized.iter().all(|e| is_warning(e)),
            "{name}: {unrecognized:?}"
        );
        assert_eq!(
            unrecognized.len(),
            warnings,
            "{name}: every warning is kept"
        );
        assert!(replies.is_empty(), "{name}: {replies:?}");
    }
}

/// The user's hooks warning (codex-m2-basic) is runtime text: kept verbatim as evidence.
#[test]
fn warnings_are_kept_as_evidence() {
    let (events, _, _) = replay_all("codex-m2-basic");
    let kept = events.iter().find_map(|e| match e {
        DriverEvent::Unrecognized { raw } if raw["method"] == "warning" => {
            raw["params"]["message"].as_str()
        }
        _ => None,
    });
    assert!(
        kept.is_some_and(|m| m.starts_with("loading hooks from both")),
        "{events:?}"
    );
}

#[test]
fn basic_is_one_turn_with_its_reply_and_usage() {
    let (events, _, p) = replay("codex-basic");
    assert_eq!(
        kinds(&events),
        ["turn-started", "consumed", "text", "usage", "turn-ended"]
    );
    let DriverEvent::TurnStarted { turn_id } = &events[0] else {
        unreachable!()
    };
    assert_eq!(
        events[1],
        DriverEvent::MessageConsumed {
            message_id: "m1".into(),
            turn_id: turn_id.clone()
        }
    );
    assert_eq!(
        events[2],
        DriverEvent::Text {
            turn_id: Some(turn_id.clone()),
            text: "ready".into()
        }
    );
    assert_eq!(
        events[3],
        // The recorded thread runs gpt-6-astra, so its tokens are priced at OpenAI's list prices.
        DriverEvent::Usage {
            turn_id: Some(turn_id.clone()),
            input_tokens: 19967,
            output_tokens: 5,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            in_turn: false,
            cost: model::pricing::openai_cost("gpt-6-astra", 19967, 0, 5)
        }
    );
    assert_eq!(
        events[4],
        DriverEvent::TurnEnded {
            turn_id: turn_id.clone(),
            status: TurnStatus::Completed
        }
    );
    assert!(!p.busy());
}

#[test]
fn a_command_is_a_tool_call_and_its_result() {
    let (events, _, _) = replay("codex-tool");
    assert_eq!(
        kinds(&events),
        [
            "turn-started",
            "consumed",
            "text",
            "tool-call",
            "tool-result",
            "usage",
            "text",
            "usage",
            "turn-ended"
        ]
    );
    let DriverEvent::ToolCall {
        name,
        input,
        call_id,
        ..
    } = &events[3]
    else {
        unreachable!()
    };
    assert_eq!(name, "commandExecution");
    assert_eq!(
        input["command"], "/bin/bash -lc 'ls -1'",
        "the whole command, verbatim, not commandActions[0]"
    );
    let DriverEvent::ToolResult {
        call_id: result_of,
        output,
        is_error,
        ..
    } = &events[4]
    else {
        unreachable!()
    };
    assert_eq!((result_of, *is_error), (call_id, false));
    assert!(output.contains("README.txt"), "{output}");
    assert_eq!(
        usage_total(&events),
        36018,
        "two model calls, counted once each"
    );
}

#[test]
fn an_interrupted_turn_closes_its_open_command_and_the_next_turn_runs() {
    let (events, _, _) = replay("codex-interrupt");
    assert_eq!(
        kinds(&events),
        [
            "turn-started",
            "consumed",
            "text",
            "tool-call",
            "usage",
            "tool-result",
            "turn-ended",
            "turn-started",
            "consumed",
            "text",
            "usage",
            "turn-ended"
        ]
    );
    assert!(
        matches!(&events[5], DriverEvent::ToolResult { is_error: true, output, .. } if output == TOOL_UNFINISHED)
    );
    assert!(matches!(
        &events[6],
        DriverEvent::TurnEnded {
            status: TurnStatus::Interrupted,
            ..
        }
    ));
    assert!(
        matches!(&events[8], DriverEvent::MessageConsumed { message_id, .. } if message_id == "m2")
    );
    assert!(matches!(
        &events[11],
        DriverEvent::TurnEnded {
            status: TurnStatus::Completed,
            ..
        }
    ));
}

/// A recording replayed with the operator answering every question it raises.
struct Answered {
    parser: CodexParser,
    events: Vec<DriverEvent>,
    /// The parser's reply to each answer, in order.
    replies: Vec<Option<Value>>,
    /// The answers the probe sent, as recorded.
    recorded: Vec<Value>,
    /// Lines the parser itself asked to write back.
    parser_replies: Vec<Value>,
}

/// Replays `name`, answering the `n`th question raised (from 0) with `answer(n)`.
fn replay_answering(name: &str, answer: impl Fn(usize) -> Answer) -> Answered {
    let mut r = Answered {
        parser: CodexParser::default(),
        events: vec![],
        replies: vec![],
        recorded: vec![],
        parser_replies: vec![],
    };
    for rec in recording(name) {
        let line = &rec["line"];
        match rec["dir"].as_str() {
            Some("in") if line["method"] == "turn/start" => r
                .parser
                .on_turn_start_sent(line["id"].as_u64().unwrap(), "m1".into()),
            Some("in") if line.get("result").is_some() => r.recorded.push(line.clone()),
            Some("out") if line.is_object() => {
                let parsed = r.parser.parse_line(&line.to_string());
                r.parser_replies.extend(parsed.replies);
                for e in &parsed.events {
                    if let DriverEvent::QuestionRaised { question_id, .. } = e {
                        let reply = r.parser.answer_reply(question_id, &answer(r.replies.len()));
                        r.replies.push(reply);
                    }
                }
                r.events.extend(parsed.events);
            }
            _ => {}
        }
    }
    r
}

#[test]
fn a_native_question_is_raised_answered_and_resolved() {
    for name in ["codex-user-input", "codex-m2-user-input"] {
        native_question(name);
    }
}

fn native_question(name: &str) {
    let r = replay_answering(name, |_| Answer::Text {
        text: "green".into(),
    });
    let (answered, recorded_answer, events) = (
        r.replies.last().cloned().flatten(),
        r.recorded.last().cloned(),
        r.events,
    );
    let raised = events.iter().find_map(|e| match e {
        DriverEvent::QuestionRaised {
            question_id,
            source,
            prompt,
            turn_id,
        } => Some((
            question_id.clone(),
            *source,
            prompt.clone(),
            turn_id.clone(),
        )),
        _ => None,
    });
    let (id, source, prompt, turn_id) = raised.expect("a question");
    assert_eq!(source, QuestionSource::NativeQuestion);
    assert!(id.starts_with("codex-") && id.ends_with("-0"), "{id}");
    assert_eq!(prompt.text, "Which color do you prefer?");
    assert_eq!(prompt.header.as_deref(), Some("Color"));
    assert_eq!(
        prompt
            .options
            .iter()
            .map(|o| o.label.as_str())
            .collect::<Vec<_>>(),
        ["Blue", "Green", "Red"]
    );
    assert!(prompt.allow_other);
    assert!(turn_id.is_some());
    assert_eq!(
        answered, recorded_answer,
        "{name}: the reply is exactly what the probe sent"
    );
    assert!(events.contains(&DriverEvent::QuestionResolved { question_id: id }));
    assert!(matches!(
        events.last(),
        Some(DriverEvent::TurnEnded {
            status: TurnStatus::Completed,
            ..
        })
    ));
}

#[test]
fn a_file_change_approval_is_a_permission_question() {
    let r = replay_answering("codex-approval", |_| Answer::Allow);
    let (reply, recorded, events) = (
        r.replies.last().cloned().flatten(),
        r.recorded.last().cloned(),
        r.events,
    );
    let prompt = events
        .iter()
        .find_map(|e| match e {
            DriverEvent::QuestionRaised {
                source: QuestionSource::NativePermission,
                prompt,
                ..
            } => Some(prompt.clone()),
            _ => None,
        })
        .expect("a permission question");
    assert_eq!(prompt.tool.as_deref(), Some("fileChange"));
    let input = prompt.tool_input.unwrap();
    assert!(
        input["changes"][0]["path"]
            .as_str()
            .unwrap()
            .ends_with("codex-probe.txt")
    );
    assert_eq!(input["changes"][0]["kind"], "add");
    assert!(
        prompt.text.starts_with("Allow Codex to change "),
        "{}",
        prompt.text
    );
    assert_eq!(reply, recorded, "accept, exactly as M0 sent it");
    let results: Vec<bool> = events
        .iter()
        .filter_map(|e| match e {
            DriverEvent::ToolResult { is_error, .. } => Some(*is_error),
            _ => None,
        })
        .collect();
    assert_eq!(results, [false], "the approved change completed");
}

#[test]
fn usage_after_resume_starts_from_a_baseline() {
    let (events, _, _) = replay("codex-resume-2");
    let usages: Vec<&DriverEvent> = events
        .iter()
        .filter(|e| matches!(e, DriverEvent::Usage { .. }))
        .collect();
    assert_eq!(
        usages.len(),
        1,
        "the replayed total before the turn is a baseline, not usage"
    );
    let DriverEvent::Usage {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        ..
    } = usages[0]
    else {
        unreachable!()
    };
    assert_eq!(
        (
            *input_tokens,
            *output_tokens,
            *cache_read_tokens,
            *cache_write_tokens
        ),
        (36275 - 17901 - (30720 - 17664), 14 - 6, 30720 - 17664, 0),
        "only the new turn's tokens, cached ones apart"
    );
    let (fork, _, _) = replay("codex-fork");
    assert_eq!(
        usage_total(&fork),
        17906,
        "the fork's replay of the parent's total is not counted again"
    );

    // 0.160.1, resume after an interrupt: the replay at @232 names the previous process's last turn.
    let (events, _, _) = replay("codex-m2-resume");
    let usages: Vec<&DriverEvent> = events
        .iter()
        .filter(|e| matches!(e, DriverEvent::Usage { .. }))
        .collect();
    assert_eq!(usages.len(), 1, "{events:?}");
    let DriverEvent::Usage {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        ..
    } = usages[0]
    else {
        unreachable!()
    };
    assert_eq!(
        (
            *input_tokens,
            *output_tokens,
            *cache_read_tokens,
            *cache_write_tokens
        ),
        (68669 - 41606 - (49408 - 25984), 68 - 60, 49408 - 25984, 0)
    );
}

/// Every question raised, each a permission question with a turn.
fn permission_questions(events: &[DriverEvent]) -> Vec<(String, model::QuestionPrompt)> {
    events
        .iter()
        .filter_map(|e| match e {
            DriverEvent::QuestionRaised {
                question_id,
                source: QuestionSource::NativePermission,
                prompt,
                turn_id,
            } => {
                assert!(turn_id.is_some());
                Some((question_id.clone(), prompt.clone()))
            }
            DriverEvent::QuestionRaised { source, .. } => panic!("unexpected source {source:?}"),
            _ => None,
        })
        .collect()
}

/// Each tool result's (output, is_error).
fn tool_results(events: &[DriverEvent]) -> Vec<(String, bool)> {
    events
        .iter()
        .filter_map(|e| match e {
            DriverEvent::ToolResult {
                output, is_error, ..
            } => Some((output.clone(), *is_error)),
            _ => None,
        })
        .collect()
}

/// Restricted posture under 0.160.1 asks through `item/commandExecution/requestApproval` (no `reason`
/// key). The first command was declined and completed `declined` with `aggregatedOutput: null`; the
/// second was accepted and completed with exit 0 and `aggregatedOutput: null` (codex-m2-approval).
#[test]
fn command_approvals_are_permission_questions_declined_then_accepted() {
    // The probe declined the first command and accepted the second.
    let r = replay_answering("codex-m2-approval", |n| {
        if n == 0 {
            Answer::Deny { reason: None }
        } else {
            Answer::Allow
        }
    });
    assert!(r.parser_replies.is_empty(), "{:?}", r.parser_replies);
    let (p, events, recorded) = (r.parser, r.events, r.recorded);
    let replies: Vec<Value> = r.replies.into_iter().map(|x| x.expect("pending")).collect();
    assert_eq!(
        replies, recorded,
        "decline then accept, exactly as the probe sent them"
    );
    let raised = permission_questions(&events);
    assert_eq!(raised.len(), 2);
    let (first, prompt) = &raised[0];
    assert!(first.ends_with("-0"), "{first}");
    assert_eq!(
        prompt.text,
        "Allow Codex to run `/bin/bash -lc 'touch approval-a.txt'`?"
    );
    assert_eq!(prompt.tool.as_deref(), Some("commandExecution"));
    assert_eq!(
        prompt.tool_input,
        Some(
            json!({ "command": "/bin/bash -lc 'touch approval-a.txt'", "cwd": "/tmp/codex-probe" })
        ),
        "the whole command; no reason key: 0.160.1 sends none"
    );
    assert_eq!(
        raised[1].1.text,
        "Allow Codex to run `/bin/bash -lc 'printf hi > approval-b.txt'`?"
    );
    for (id, _) in &raised {
        assert!(
            events.contains(&DriverEvent::QuestionResolved {
                question_id: id.clone()
            }),
            "{id}"
        );
    }
    let results = tool_results(&events);
    assert_eq!(
        results,
        [("declined".to_string(), true), (String::new(), false)],
        "null output: the status word, else empty"
    );
    assert!(matches!(
        events.last(),
        Some(DriverEvent::TurnEnded {
            status: TurnStatus::Completed,
            ..
        })
    ));
    assert!(!p.busy());
}

/// 0.160.1 answers a `turn/start` sent while a turn runs with the active turn's id: no error, no new
/// turn, no `turn/started` (codex-m2-interrupt@6654-6656). The message joined (steered) that turn; the
/// parser must not keep it pending, or `busy()` would stay true for ever and the driver's queue stall.
#[test]
fn a_turn_start_during_a_running_turn_steers_it() {
    let (events, _, p) = replay("codex-m2-interrupt");
    assert_eq!(
        kinds(&events),
        [
            "turn-started",
            "consumed",
            "text",
            "tool-call",
            "usage",
            "consumed",
            "tool-result",
            "turn-ended",
            "turn-started",
            "consumed",
            "text",
            "usage",
            "turn-ended"
        ]
    );
    let DriverEvent::TurnStarted { turn_id: first } = &events[0] else {
        unreachable!()
    };
    assert_eq!(
        events[1],
        DriverEvent::MessageConsumed {
            message_id: "m1".into(),
            turn_id: first.clone()
        }
    );
    assert_eq!(
        events[5],
        DriverEvent::MessageConsumed {
            message_id: "m2".into(),
            turn_id: first.clone()
        },
        "steered into turn 1"
    );
    assert!(
        matches!(&events[6], DriverEvent::ToolResult { is_error: true, output, .. } if output == TOOL_UNFINISHED)
    );
    assert_eq!(
        events[7],
        DriverEvent::TurnEnded {
            turn_id: first.clone(),
            status: TurnStatus::Interrupted
        }
    );
    let DriverEvent::TurnStarted { turn_id: second } = &events[8] else {
        unreachable!()
    };
    assert_ne!(first, second);
    assert_eq!(
        events[9],
        DriverEvent::MessageConsumed {
            message_id: "m3".into(),
            turn_id: second.clone()
        }
    );
    assert!(!p.busy(), "nothing is left pending");
    assert_eq!(p.current_turn(), None);
}

/// The seat tools' server (`rig`) through thread `config` (codex-m2-seat-config-dotted@7399-7401).
#[test]
fn a_seat_tool_call_is_an_mcp_tool_call() {
    for name in ["codex-m2-seat-config-dotted", "codex-m2-seat-cli"] {
        let (events, _, _) = replay(name);
        let call = events.iter().find_map(|e| match e {
            DriverEvent::ToolCall {
                name,
                input,
                call_id,
                ..
            } if name.starts_with("mcp__") => Some((name.clone(), input.clone(), call_id.clone())),
            _ => None,
        });
        let (tool, input, call_id) = call.unwrap_or_else(|| panic!("{name}: {events:?}"));
        assert_eq!(tool, "mcp__rig__ping");
        assert_eq!(input, json!({ "message": "probe" }));
        let result = events.iter().find_map(|e| match e {
            DriverEvent::ToolResult {
                call_id: c,
                output,
                is_error,
                ..
            } if *c == call_id => Some((output.clone(), *is_error)),
            _ => None,
        });
        assert_eq!(result, Some(("pong: probe".to_string(), false)), "{name}");
        assert!(
            matches!(
                events.last(),
                Some(DriverEvent::TurnEnded {
                    status: TurnStatus::Completed,
                    ..
                })
            ),
            "{name}"
        );
    }
}

/// 0.160.1 basic and tool have the same shapes as M0's: one turn each, and the tool's command.
#[test]
fn refresh_basic_and_tool_match_m0() {
    let (events, _, _) = replay("codex-m2-basic");
    assert_eq!(
        kinds(&events),
        ["turn-started", "consumed", "text", "usage", "turn-ended"]
    );
    let (events, _, _) = replay("codex-m2-tool");
    let k = kinds(&events);
    assert_eq!(
        k.iter().filter(|k| **k == "tool-call").count(),
        k.iter().filter(|k| **k == "tool-result").count()
    );
    assert!(k.contains(&"tool-call"));
    assert!(matches!(
        events.last(),
        Some(DriverEvent::TurnEnded {
            status: TurnStatus::Completed,
            ..
        })
    ));
}

#[test]
fn steering_lines_are_ignored() {
    let (events, _, _) = replay("codex-steer");
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, DriverEvent::MessageConsumed { .. }))
            .count(),
        1
    );
    assert!(matches!(
        events.last(),
        Some(DriverEvent::TurnEnded {
            status: TurnStatus::Completed,
            ..
        })
    ));
}

#[test]
fn unknown_server_requests_are_refused_not_ignored() {
    let mut p = CodexParser::default();
    p.set_thread("thr-1");
    for (id, method) in [
        (json!(7), "item/permissions/requestApproval"),
        (json!("s-1"), "mcpServer/elicitation/request"),
    ] {
        let parsed = p.parse_line(
            &json!({ "id": id, "method": method, "params": { "threadId": "thr-1" } }).to_string(),
        );
        assert_eq!(parsed.replies.len(), 1, "{method}");
        let r = &parsed.replies[0];
        assert_eq!((&r["id"], r["error"]["code"].as_i64()), (&id, Some(-32601)));
        assert_eq!(
            r["error"]["message"],
            format!("Sculptured does not handle {method}")
        );
        assert!(
            matches!(&parsed.events[..], [DriverEvent::Unrecognized { .. }]),
            "{:?}",
            parsed.events
        );
    }
    let parsed = p.parse_line(r#"{"method":"fake/unknownThing","params":{}}"#);
    assert!(parsed.replies.is_empty());
    assert!(matches!(
        &parsed.events[..],
        [DriverEvent::Unrecognized { .. }]
    ));
    let parsed = p.parse_line("not json at all");
    assert!(
        matches!(&parsed.events[..], [DriverEvent::Unrecognized { raw }] if raw == "not json at all")
    );
}

#[test]
fn a_failed_turn_keeps_its_error_and_ends_failed() {
    let mut p = CodexParser::default();
    p.set_thread("thr-1");
    p.on_turn_start_sent(3, "m1".into());
    p.parse_line(r#"{"id":3,"result":{"turn":{"id":"t1","status":"inProgress"}}}"#);
    let parsed = p.parse_line(
        r#"{"method":"turn/completed","params":{"threadId":"thr-1","turn":{"id":"t1","status":"failed","error":{"message":"model unavailable"}}}}"#,
    );
    assert!(parsed.turn_ended);
    assert!(
        matches!(&parsed.events[0], DriverEvent::Unrecognized { raw } if raw["turnError"]["message"] == "model unavailable")
    );
    assert_eq!(
        parsed.events[1],
        DriverEvent::TurnEnded {
            turn_id: "t1".into(),
            status: TurnStatus::Failed
        }
    );
}

#[test]
fn a_usage_limit_ends_the_turn_with_a_limit_notice() {
    let mut p = CodexParser::default();
    p.set_thread("thr-1");
    p.on_turn_start_sent(3, "m1".into());
    p.parse_line(r#"{"id":3,"result":{"turn":{"id":"t1","status":"inProgress"}}}"#);
    // The shape is the 0.160.0 schema's `TurnError` (M0 recorded no limit); the message is illustrative.
    let parsed = p.parse_line(
        r#"{"method":"turn/completed","params":{"threadId":"thr-1","turn":{"id":"t1","status":"failed","error":{"message":"You've hit your usage limit. Try again at 4:30 AM.","codexErrorInfo":"usageLimitExceeded"}}}}"#,
    );
    assert!(
        matches!(&parsed.events[0], DriverEvent::Unrecognized { raw } if raw["turnError"]["codexErrorInfo"] == "usageLimitExceeded")
    );
    assert_eq!(
        parsed.events[1],
        DriverEvent::LimitReached {
            turn_id: Some("t1".into()),
            kind: LimitKind::Usage,
            resets: Some("4:30 AM".into()),
            text: "You've hit your usage limit. Try again at 4:30 AM.".into(),
        }
    );
    assert_eq!(
        parsed.events[2],
        DriverEvent::TurnEnded {
            turn_id: "t1".into(),
            status: TurnStatus::Failed
        }
    );
}

fn failed_turn(error: Value) -> Parsed {
    let mut p = CodexParser::default();
    p.set_thread("thr-1");
    p.on_turn_start_sent(3, "m1".into());
    p.parse_line(r#"{"id":3,"result":{"turn":{"id":"t1","status":"inProgress"}}}"#);
    let line = json!({ "method": "turn/completed", "params": { "threadId": "thr-1", "turn": { "id": "t1", "status": "failed", "error": error } } });
    p.parse_line(&line.to_string())
}

/// `sessionBudgetExceeded` (schema only; unobserved) is an exhausted allowance too: a retry would hit
/// it again, so it is a usage limit for Needs you, never retried.
#[test]
fn a_session_budget_ends_the_turn_with_a_usage_limit() {
    let parsed = failed_turn(
        json!({ "message": "Session budget exceeded.", "codexErrorInfo": "sessionBudgetExceeded" }),
    );
    assert!(matches!(
        &parsed.events[0],
        DriverEvent::Unrecognized { .. }
    ));
    assert_eq!(
        parsed.events[1],
        DriverEvent::LimitReached {
            turn_id: Some("t1".into()),
            kind: LimitKind::Usage,
            resets: None,
            text: "Session budget exceeded.".into()
        }
    );
    assert_eq!(
        parsed.events[2],
        DriverEvent::TurnEnded {
            turn_id: "t1".into(),
            status: TurnStatus::Failed
        }
    );

    let parsed =
        failed_turn(json!({ "message": "Slow down.", "codexErrorInfo": "rateLimitExceeded" }));
    assert!(matches!(
        &parsed.events[1],
        DriverEvent::LimitReached {
            kind: LimitKind::Rate,
            resets: None,
            ..
        }
    ));
}

/// The object form of `CodexErrorInfo` (`{httpConnectionFailed: {…}}` and the like) is no limit.
#[test]
fn an_object_error_info_is_a_plain_failure() {
    let parsed = failed_turn(
        json!({ "message": "connection failed", "codexErrorInfo": { "httpConnectionFailed": { "httpStatusCode": 502 } } }),
    );
    assert_eq!(parsed.events.len(), 2, "{:?}", parsed.events);
    assert!(
        matches!(&parsed.events[0], DriverEvent::Unrecognized { raw } if raw["turnError"]["codexErrorInfo"]["httpConnectionFailed"]["httpStatusCode"] == 502)
    );
    assert_eq!(
        parsed.events[1],
        DriverEvent::TurnEnded {
            turn_id: "t1".into(),
            status: TurnStatus::Failed
        }
    );
}

/// An `error` notification with `willRetry: true` is Codex retrying by itself: evidence only.
#[test]
fn a_retrying_error_notification_is_evidence_not_a_limit() {
    let mut p = CodexParser::default();
    p.set_thread("thr-1");
    p.on_turn_start_sent(3, "m1".into());
    p.parse_line(r#"{"id":3,"result":{"turn":{"id":"t1","status":"inProgress"}}}"#);
    let parsed = p.parse_line(
        r#"{"method":"error","params":{"threadId":"thr-1","turnId":"t1","willRetry":true,"error":{"message":"Rate limited, retrying","codexErrorInfo":"rateLimitExceeded"}}}"#,
    );
    assert!(!parsed.turn_ended);
    assert!(
        matches!(&parsed.events[..], [DriverEvent::Unrecognized { .. }]),
        "{:?}",
        parsed.events
    );
    assert!(p.busy());
}

/// A failed `rig` server start is kept as evidence; other servers' statuses are not.
#[test]
fn a_failed_seat_tools_server_is_kept_as_evidence() {
    let mut p = CodexParser::default();
    let rig = p.parse_line(r#"{"method":"mcpServer/startupStatus/updated","params":{"name":"rig","status":"failed","error":"spawn failed","threadId":null}}"#);
    assert!(
        matches!(&rig.events[..], [DriverEvent::Unrecognized { raw }] if raw["name"] == "rig" && raw["error"] == "spawn failed")
    );
    let other = p.parse_line(r#"{"method":"mcpServer/startupStatus/updated","params":{"name":"node_repl","status":"failed","threadId":null}}"#);
    assert!(other.events.is_empty());
    let ready = p.parse_line(r#"{"method":"mcpServer/startupStatus/updated","params":{"name":"rig","status":"ready","threadId":null}}"#);
    assert!(ready.events.is_empty());
}

#[test]
fn a_refused_turn_start_frees_the_parser_and_names_its_message() {
    let mut p = CodexParser::default();
    p.set_thread("thr-1");
    p.on_turn_start_sent(4, "m9".into());
    assert!(p.busy());
    let parsed = p.parse_line(
        r#"{"id":4,"error":{"code":-32600,"message":"a turn is already in progress"}}"#,
    );
    assert_eq!(parsed.refused_start.as_deref(), Some("m9"));
    assert!(!p.busy());
}

#[test]
fn expired_answers_follow_d8() {
    let mut p = CodexParser::default();
    p.set_thread("thr-1");
    let parsed = p.parse_line(QUESTION);
    let DriverEvent::QuestionRaised { question_id: q, .. } = &parsed.events[0] else {
        panic!("{:?}", parsed.events)
    };
    assert!(q.starts_with("codex-thr-1-") && q.ends_with("-0"), "{q}");
    assert_eq!(
        p.answer_reply(q, &Answer::Expired),
        Some(
            json!({ "id": 0, "jsonrpc": "2.0", "result": { "answers": { "k": { "answers": [EXPIRED_ANSWER] } } } })
        )
    );
    assert_eq!(
        p.answer_reply(q, &Answer::Expired),
        None,
        "a question is answered once"
    );
    let a = r#"{"id":1,"method":"item/commandExecution/requestApproval","params":{"command":"rm -rf build","cwd":"/w","threadId":"thr-1","turnId":"t1","itemId":"i"}}"#;
    let parsed = p.parse_line(a);
    let DriverEvent::QuestionRaised {
        question_id: a,
        prompt,
        ..
    } = &parsed.events[0]
    else {
        panic!("{:?}", parsed.events)
    };
    assert_eq!(prompt.text, "Allow Codex to run `rm -rf build`?");
    assert_eq!(
        p.answer_reply(a, &Answer::Expired),
        Some(json!({ "id": 1, "jsonrpc": "2.0", "result": { "decision": "accept" } }))
    );
}

const QUESTION: &str = r#"{"id":0,"method":"item/tool/requestUserInput","params":{"questions":[{"id":"k","question":"Which?","options":[],"isOther":true}],"threadId":"thr-1","turnId":"t1"}}"#;

/// Server request ids restart at 0 in every app-server process and a resumed session keeps its
/// thread id: two processes on one thread must not give their first prompts the same id.
#[test]
fn question_ids_differ_between_processes() {
    let ids: Vec<String> = (0..2)
        .map(|_| {
            let mut p = CodexParser::default();
            p.set_thread("thr-1");
            match &p.parse_line(QUESTION).events[0] {
                DriverEvent::QuestionRaised { question_id, .. } => question_id.clone(),
                e => panic!("{e:?}"),
            }
        })
        .collect();
    assert_ne!(ids[0], ids[1]);
}

/// Without `--enable default_mode_request_user_input` Codex asks in plain text and then waits in
/// `sleep` items; the turn never ends (M0: 480 s). Each wait is a visible tool call, not silence.
#[test]
fn noflag_waits_show_as_sleep_calls() {
    let (events, _, _) = replay("codex-user-input-noflag");
    let sleeps: Vec<&DriverEvent> = events
        .iter()
        .filter(|e| matches!(e, DriverEvent::ToolCall { name, .. } if name == "sleep"))
        .collect();
    assert!(!sleeps.is_empty(), "{events:?}");
    assert!(
        matches!(sleeps[0], DriverEvent::ToolCall { input, .. } if input["durationMs"] == 60000)
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, DriverEvent::TurnEnded { .. })),
        "the recorded turn never ended"
    );
}

// ---- Review fix round 1 ----

fn running_turn() -> CodexParser {
    let mut p = CodexParser::default();
    p.set_thread("thr-1");
    p.on_turn_start_sent(3, "m1".into());
    p.parse_line(r#"{"id":3,"result":{"turn":{"id":"t1","status":"inProgress"}}}"#);
    p
}

fn raised(parsed: &Parsed) -> (String, model::QuestionPrompt) {
    match &parsed.events[..] {
        [
            DriverEvent::QuestionRaised {
                question_id,
                prompt,
                ..
            },
        ] => (question_id.clone(), prompt.clone()),
        e => panic!("{e:?}"),
    }
}

/// `commandActions` is best-effort display parsing and can name only part of a compound command. The
/// operator must see, and approve, the whole command: an approval that read only `cat notes.txt` would
/// let the `rm` through unseen.
#[test]
fn a_compound_command_is_shown_whole() {
    let mut p = running_turn();
    let command = "/bin/bash -lc 'cat notes.txt && rm -rf ~/work'";
    let actions = json!([{ "command": "cat notes.txt", "name": "notes.txt", "path": "notes.txt", "type": "read" }, { "command": "rm -rf ~/work", "type": "unknown" }]);
    let started = json!({ "method": "item/started", "params": { "threadId": "thr-1", "turnId": "t1",
        "item": { "type": "commandExecution", "id": "exec-1", "command": command, "commandActions": actions, "cwd": "/w", "status": "inProgress" } } });
    let parsed = p.parse_line(&started.to_string());
    assert!(
        matches!(&parsed.events[..], [DriverEvent::ToolCall { input, .. }] if input["command"] == command),
        "{:?}",
        parsed.events
    );

    let ask = json!({ "id": 4, "method": "item/commandExecution/requestApproval", "params": { "threadId": "thr-1", "turnId": "t1",
        "itemId": "exec-1", "kind": "command", "command": command, "commandActions": actions, "cwd": "/w" } });
    let (_, prompt) = raised(&p.parse_line(&ask.to_string()));
    assert_eq!(prompt.text, format!("Allow Codex to run `{command}`?"));
    assert_eq!(
        prompt.tool_input,
        Some(json!({ "command": command, "cwd": "/w" }))
    );

    // Without `command`, every action is shown, never only the first.
    let ask = json!({ "id": 5, "method": "item/commandExecution/requestApproval", "params": { "threadId": "thr-1", "turnId": "t1",
        "itemId": "exec-2", "commandActions": actions, "cwd": "/w" } });
    let (_, prompt) = raised(&p.parse_line(&ask.to_string()));
    assert_eq!(
        prompt.text,
        "Allow Codex to run `cat notes.txt\nrm -rf ~/work`?"
    );
}

/// Schema `kind: writeStdin` and `networkApprovalContext` get their own words (neither was recorded).
#[test]
fn stdin_and_network_approvals_say_what_they_allow() {
    let mut p = running_turn();
    let stdin = r#"{"id":6,"method":"item/commandExecution/requestApproval","params":{"threadId":"thr-1","turnId":"t1","itemId":"exec-3","kind":"writeStdin","command":"python3 repl.py","cwd":"/w"}}"#;
    let (_, prompt) = raised(&p.parse_line(stdin));
    assert_eq!(
        prompt.text,
        "Allow Codex to send input to the running command `python3 repl.py`?"
    );
    assert_eq!(prompt.tool_input.unwrap()["kind"], "writeStdin");

    let net = r#"{"id":7,"method":"item/commandExecution/requestApproval","params":{"threadId":"thr-1","turnId":"t1","itemId":"exec-4","kind":"command","command":"curl https://example.org","cwd":"/w","networkApprovalContext":{"host":"example.org","protocol":"https"},"reason":"network access"}}"#;
    let (_, prompt) = raised(&p.parse_line(net));
    assert_eq!(
        prompt.text,
        "Allow Codex network access to example.org (https) for `curl https://example.org`?"
    );
    let input = prompt.tool_input.unwrap();
    assert_eq!(
        input["network"],
        json!({ "host": "example.org", "protocol": "https" })
    );
    assert_eq!(input["reason"], "network access");
    assert!(
        input.get("kind").is_none(),
        "a plain command kind is not repeated"
    );
}

/// A fileChange approval keeps its `reason` (the M0 field; null in the recording).
#[test]
fn a_file_change_approval_keeps_its_reason() {
    let mut p = running_turn();
    p.parse_line(
        r#"{"method":"item/started","params":{"threadId":"thr-1","turnId":"t1","item":{"type":"fileChange","id":"fc-1","status":"inProgress","changes":[{"path":"/w/a.txt","kind":{"type":"add"},"diff":"hi\n"}]}}}"#,
    );
    let ask = r#"{"id":8,"method":"item/fileChange/requestApproval","params":{"threadId":"thr-1","turnId":"t1","itemId":"fc-1","grantRoot":null,"reason":"write outside the workspace"}}"#;
    let (_, prompt) = raised(&p.parse_line(ask));
    assert_eq!(prompt.text, "Allow Codex to change /w/a.txt?");
    assert_eq!(
        prompt.tool_input,
        Some(
            json!({ "changes": [{ "path": "/w/a.txt", "kind": "add" }], "reason": "write outside the workspace" })
        )
    );
}

/// Codex resolved the request by itself (or the turn ended) before anyone answered: a late answer
/// writes nothing, since nobody waits for it.
#[test]
fn an_answer_after_codex_resolved_the_request_writes_nothing() {
    let mut p = running_turn();
    let (q, _) = raised(&p.parse_line(QUESTION));
    let resolved = p.parse_line(
        r#"{"method":"serverRequest/resolved","params":{"threadId":"thr-1","requestId":0}}"#,
    );
    assert_eq!(
        resolved.events,
        [DriverEvent::QuestionResolved {
            question_id: q.clone()
        }]
    );
    assert_eq!(
        p.answer_reply(
            &q,
            &Answer::Text {
                text: "late".into()
            }
        ),
        None
    );
}

#[test]
fn an_unknown_or_missing_turn_status_is_a_failure() {
    for turn in [
        json!({ "id": "t1", "status": "somethingNew" }),
        json!({ "id": "t1" }),
    ] {
        let mut p = running_turn();
        let parsed = p.parse_line(
            &json!({ "method": "turn/completed", "params": { "threadId": "thr-1", "turn": turn } })
                .to_string(),
        );
        assert_eq!(
            parsed.events,
            [DriverEvent::TurnEnded {
                turn_id: "t1".into(),
                status: TurnStatus::Failed
            }]
        );
        assert!(parsed.turn_ended && !p.busy());
    }
}

/// `responseTooManyFailedAttempts` with HTTP 429 is Codex giving up on a rate limit.
#[test]
fn too_many_failed_attempts_on_429_is_a_rate_limit() {
    let parsed = failed_turn(
        json!({ "message": "exceeded retry limit, last status: 429 Too Many Requests",
        "codexErrorInfo": { "responseTooManyFailedAttempts": { "httpStatusCode": 429 } } }),
    );
    assert!(
        matches!(
            &parsed.events[1],
            DriverEvent::LimitReached {
                kind: LimitKind::Rate,
                ..
            }
        ),
        "{:?}",
        parsed.events
    );
    let parsed = failed_turn(
        json!({ "message": "exceeded retry limit, last status: 500", "codexErrorInfo": { "responseTooManyFailedAttempts": { "httpStatusCode": 500 } } }),
    );
    assert_eq!(
        parsed.events.len(),
        2,
        "a 500 is no limit: {:?}",
        parsed.events
    );
}

/// An unrecognised `codexErrorInfo` falls back to the message words, as for Claude.
#[test]
fn an_unclassified_error_falls_back_to_the_message_words() {
    let parsed = failed_turn(
        json!({ "message": "You've hit your usage limit. Try again at 4:30 AM.", "codexErrorInfo": "other" }),
    );
    assert_eq!(
        parsed.events[1],
        DriverEvent::LimitReached {
            turn_id: Some("t1".into()),
            kind: LimitKind::Usage,
            resets: Some("4:30 AM".into()),
            text: "You've hit your usage limit. Try again at 4:30 AM.".into()
        }
    );
    let parsed = failed_turn(
        json!({ "message": "Rate limit reached for requests", "codexErrorInfo": null }),
    );
    assert!(
        matches!(
            &parsed.events[1],
            DriverEvent::LimitReached {
                kind: LimitKind::Rate,
                ..
            }
        ),
        "{:?}",
        parsed.events
    );
    let parsed = failed_turn(
        json!({ "message": "context window exceeded", "codexErrorInfo": "contextWindowExceeded" }),
    );
    assert_eq!(
        parsed.events.len(),
        2,
        "no limit words, no limit: {:?}",
        parsed.events
    );
}

/// A `turn/start` answered with a turn that already ended steers nothing: no `MessageConsumed` against
/// a finished turn; the message is handed back (`refused_start`) and the parser is free.
#[test]
fn a_turn_start_answered_with_an_ended_turn_is_handed_back() {
    let mut p = running_turn();
    p.parse_line(r#"{"method":"turn/completed","params":{"threadId":"thr-1","turn":{"id":"t1","status":"completed"}}}"#);
    p.on_turn_start_sent(4, "m2".into());
    let parsed = p.parse_line(r#"{"id":4,"result":{"turn":{"id":"t1","status":"completed"}}}"#);
    assert!(
        !parsed
            .events
            .iter()
            .any(|e| matches!(e, DriverEvent::MessageConsumed { .. })),
        "{:?}",
        parsed.events
    );
    assert_eq!(parsed.refused_start.as_deref(), Some("m2"));
    assert!(matches!(
        &parsed.events[..],
        [DriverEvent::Unrecognized { .. }]
    ));
    assert!(!p.busy());
}

/// An item type nobody maps leaves evidence when it starts, so one that never completes is not silent.
#[test]
fn an_unknown_item_is_kept_when_it_starts() {
    let mut p = running_turn();
    let parsed = p.parse_line(r#"{"method":"item/started","params":{"threadId":"thr-1","turnId":"t1","item":{"type":"imageGeneration","id":"ig-1"}}}"#);
    assert!(
        matches!(&parsed.events[..], [DriverEvent::Unrecognized { raw }] if raw["type"] == "imageGeneration"),
        "{:?}",
        parsed.events
    );
    for known in ["agentMessage", "userMessage", "reasoning"] {
        let line = json!({ "method": "item/started", "params": { "threadId": "thr-1", "turnId": "t1", "item": { "type": known, "id": "x" } } });
        assert!(p.parse_line(&line.to_string()).events.is_empty(), "{known}");
    }
}

// ---- Task 13 carry-forwards ----

fn ended_turn(status: &str, error: Value) -> Parsed {
    let mut p = running_turn();
    let line = json!({ "method": "turn/completed", "params": { "threadId": "thr-1", "turn": { "id": "t1", "status": status, "error": error } } });
    p.parse_line(&line.to_string())
}

fn limit(parsed: &Parsed) -> Option<LimitKind> {
    parsed.events.iter().find_map(|e| match e {
        DriverEvent::LimitReached { kind, .. } => Some(*kind),
        _ => None,
    })
}

/// The message-words fallback only reads a turn that failed with an error Codex did not classify:
/// `codexErrorInfo` null, `"other"` or an object form. A classified string (`serverOverloaded`, …)
/// is never re-read as a limit, and a turn that did not fail is never a limit.
#[test]
fn the_limit_words_fallback_is_only_for_unclassified_failures() {
    let words = "You've hit your usage limit. Try again at 4:30 AM.";
    for status in ["interrupted", "completed"] {
        let parsed = ended_turn(status, json!({ "message": words, "codexErrorInfo": null }));
        assert_eq!(limit(&parsed), None, "{status}: {:?}", parsed.events);
        assert!(parsed.events.iter().any(|e| matches!(e, DriverEvent::Unrecognized { raw } if raw["turnError"]["message"] == words)), "the error is still evidence");
    }
    for info in [
        "serverOverloaded",
        "internalServerError",
        "contextWindowExceeded",
        "badRequest",
    ] {
        let parsed = ended_turn(
            "failed",
            json!({ "message": words, "codexErrorInfo": info }),
        );
        assert_eq!(limit(&parsed), None, "{info}: {:?}", parsed.events);
    }
    let fallback = [
        json!(null),
        json!("other"),
        json!({ "responseStreamDisconnected": { "httpStatusCode": null } }),
    ];
    for info in fallback {
        let parsed = ended_turn(
            "failed",
            json!({ "message": words, "codexErrorInfo": info }),
        );
        assert_eq!(
            limit(&parsed),
            Some(LimitKind::Usage),
            "{info}: {:?}",
            parsed.events
        );
    }
    // The classified strings still count on a failed turn, and only there.
    assert_eq!(
        limit(&ended_turn(
            "failed",
            json!({ "message": "x", "codexErrorInfo": "usageLimitExceeded" })
        )),
        Some(LimitKind::Usage)
    );
    assert_eq!(
        limit(&ended_turn(
            "interrupted",
            json!({ "message": "x", "codexErrorInfo": "usageLimitExceeded" })
        )),
        None
    );
}

/// An unknown item that starts and completes leaves one piece of evidence, not two.
#[test]
fn an_unknown_item_that_completes_is_kept_once() {
    let mut p = running_turn();
    let started = p.parse_line(r#"{"method":"item/started","params":{"threadId":"thr-1","turnId":"t1","item":{"type":"imageGeneration","id":"ig-1"}}}"#);
    let completed =
        p.parse_line(r#"{"method":"item/completed","params":{"threadId":"thr-1","turnId":"t1","item":{"type":"imageGeneration","id":"ig-1","status":"completed"}}}"#);
    assert_eq!(started.events.len(), 1, "{:?}", started.events);
    assert!(
        completed.events.is_empty(),
        "already kept when it started: {:?}",
        completed.events
    );
    // One that was never seen starting is still kept when it completes.
    let late = p.parse_line(r#"{"method":"item/completed","params":{"threadId":"thr-1","turnId":"t1","item":{"type":"imageGeneration","id":"ig-2"}}}"#);
    assert!(
        matches!(&late.events[..], [DriverEvent::Unrecognized { raw }] if raw["id"] == "ig-2"),
        "{:?}",
        late.events
    );
    // A later item with a reused id after the turn ended is new evidence again.
    p.parse_line(r#"{"method":"turn/completed","params":{"threadId":"thr-1","turn":{"id":"t1","status":"completed"}}}"#);
    p.on_turn_start_sent(5, "m2".into());
    p.parse_line(r#"{"id":5,"result":{"turn":{"id":"t2","status":"inProgress"}}}"#);
    let again = p.parse_line(r#"{"method":"item/completed","params":{"threadId":"thr-1","turnId":"t2","item":{"type":"imageGeneration","id":"ig-1"}}}"#);
    assert_eq!(again.events.len(), 1, "{:?}", again.events);
}

/// Task 13 Step 12, the one real probe: the real `sculptured seat-tools` shim through the thread
/// `config` (dotted keys), on `thread/start` and again on `thread/resume`. Codex called the tools,
/// the test socket's answers came back as the tool results, and both turns completed.
#[test]
fn the_real_seat_tools_shim_is_called_through_thread_config() {
    for (name, tool, input, output, reply) in [
        (
            "codex-m2-seat-shim",
            "mcp__rig__request_input",
            json!({ "options": ["Blue", "Green"], "question": "Which colour?" }),
            "Blue",
            "Blue",
        ),
        (
            "codex-m2-seat-shim-resume",
            "mcp__rig__report",
            json!({ "summary": "probe" }),
            "Recorded your report.",
            "ready",
        ),
    ] {
        let (events, replies, _) = replay(name);
        assert!(
            replies.is_empty(),
            "{name}: no server request needed an answer: {replies:?}"
        );
        let call_id = events
            .iter()
            .find_map(|e| match e {
                DriverEvent::ToolCall {
                    name: n,
                    input: i,
                    call_id,
                    ..
                } if n == tool && *i == input => Some(call_id.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{name}: {events:?}"));
        assert!(
            events.contains(&DriverEvent::ToolResult {
                turn_id: events.iter().find_map(|e| match e {
                    DriverEvent::TurnStarted { turn_id } => Some(turn_id.clone()),
                    _ => None,
                }),
                call_id,
                output: output.into(),
                is_error: false
            }),
            "{name}: {events:?}"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, DriverEvent::Text { text, .. } if text == reply)),
            "{name}"
        );
        assert!(
            matches!(
                events.last(),
                Some(DriverEvent::TurnEnded {
                    status: TurnStatus::Completed,
                    ..
                })
            ),
            "{name}"
        );
    }
}

/// `account/rateLimits/updated` is the plan's usage, account-wide: the weekly window and its percent.
#[test]
fn rate_limit_updates_report_plan_usage() {
    let (events, _, _) = replay_all("codex-basic");
    let usage = events.iter().find_map(|e| match e {
        DriverEvent::PlanUsage(u) => Some(u),
        _ => None,
    });
    let w = &usage
        .expect("the recording reports its rate limits")
        .windows[0];
    assert_eq!((w.name.as_str(), w.used_percent), ("seven_day", Some(31.0)));
}

#[test]
fn codex_usage_arrives_during_the_turn() {
    // Codex reports `thread/tokenUsage/updated` after each model call, so a long turn is counted as it runs.
    let (events, _, _) = replay("codex-m2-approval");
    let k = kinds(&events);
    let ended = k.iter().position(|&x| x == "turn-ended").unwrap();
    let during: Vec<_> = k[..ended]
        .iter()
        .enumerate()
        .filter(|&(_, &x)| x == "usage")
        .map(|(i, _)| i)
        .collect();
    assert!(during.len() >= 2, "{k:?}");
    assert!(
        k[during[0]..ended].iter().any(|&x| x != "usage"),
        "work happens between the reports: {k:?}"
    );
}

#[test]
fn each_model_call_reports_the_context_it_read_and_the_window() {
    let (events, _, _) = replay_all("codex-basic");
    let ctx: Vec<(u64, Option<u64>)> = events
        .iter()
        .filter_map(|e| match e {
            DriverEvent::Context {
                used_tokens,
                window_tokens,
                ..
            } => Some((*used_tokens, *window_tokens)),
            _ => None,
        })
        .collect();
    assert!(
        !ctx.is_empty()
            && ctx
                .iter()
                .all(|(used, window)| *used > 0 && window.is_some()),
        "{ctx:?}"
    );
}
