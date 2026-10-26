//! The OpenCode parser against recordings of OpenCode 1.19.4 over ACP, made against a local stand-in
//! for OpenRouter's chat API (`crates/fixtures/recordings/opencode-*`).
use drivers::DriverEvent;
use drivers::opencode::{AcpParser, Parsed};
use model::{Answer, CostSource, LimitKind, QuestionSource, TurnStatus};
use serde_json::{Value, json};
use std::path::PathBuf;

fn recording(name: &str) -> Vec<Value> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/recordings")
        .join(format!("{name}.jsonl"));
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing {}", path.display()));
    text.lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// Replays a recording from its first prompt on: each `in` `session/prompt` is message `m1`, `m2`, …;
/// `out` lines are parsed, and where the recorded client answered a permission request the parser is
/// answered Allow. Returns the events, the replies the parser wrote (its answers included), the client's
/// own replies as recorded, and the parser.
fn replay(name: &str, fresh: bool) -> (Vec<DriverEvent>, Vec<Value>, Vec<Value>, AcpParser) {
    let mut p = AcpParser::new(fresh);
    let (mut events, mut replies, mut recorded, mut n, mut prompting) =
        (vec![], vec![], vec![], 0, false);
    for rec in recording(name) {
        let line = &rec["line"];
        match rec["dir"].as_str() {
            Some("in") if line["method"] == "session/prompt" => {
                n += 1;
                prompting = true;
                p.on_prompt_sent(line["id"].as_u64().unwrap(), format!("turn-m{n}"));
            }
            Some("in") if line.get("result").is_some() => {
                recorded.push(line.clone());
                let id = format!("perm-{}", line["id"]);
                replies.extend(p.answer_reply(&id, &Answer::Allow));
            }
            Some("out") if prompting => {
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
    (events, replies, recorded, p)
}

fn texts(events: &[DriverEvent]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|e| match e {
            DriverEvent::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn ended(events: &[DriverEvent]) -> TurnStatus {
    events
        .iter()
        .find_map(|e| match e {
            DriverEvent::TurnEnded { status, .. } => Some(*status),
            _ => None,
        })
        .expect("the turn ended")
}

fn usage(events: &[DriverEvent]) -> Option<&DriverEvent> {
    events
        .iter()
        .find(|e| matches!(e, DriverEvent::Usage { .. }))
}

#[test]
fn chunks_of_one_message_become_one_text_and_the_turn_reports_usage_and_cost() {
    let (events, ..) = replay("opencode-basic", true);
    assert_eq!(texts(&events), ["Hello from the model."]);
    let Some(DriverEvent::Usage {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cost,
        in_turn,
        turn_id,
        ..
    }) = usage(&events)
    else {
        panic!("{events:?}")
    };
    assert_eq!(
        (*input_tokens, *output_tokens, *cache_read_tokens),
        (100, 12, 1200),
        "the last call's tokens"
    );
    let cost = cost.expect("a fresh session's cost is known");
    assert!(cost.usd > 0.0);
    assert_eq!(cost.source, CostSource::Opencode);
    assert!(!in_turn);
    assert_eq!(turn_id.as_deref(), Some("turn-m1"));
    assert!(events.iter().any(|e| matches!(
        e,
        DriverEvent::Context {
            used_tokens: 1300,
            window_tokens: Some(163_840),
            ..
        }
    )));
    assert_eq!(ended(&events), TurnStatus::Completed);
}

#[test]
fn a_tool_call_is_announced_with_its_real_input_and_its_result_follows() {
    let (events, ..) = replay("opencode-tool", true);
    let call = events.iter().find_map(|e| match e {
        DriverEvent::ToolCall {
            name,
            input,
            call_id,
            ..
        } => Some((name.clone(), input.clone(), call_id.clone())),
        _ => None,
    });
    let (name, input, id) = call.expect("a tool call");
    assert_eq!(name, "bash");
    assert_eq!(
        input["command"], "echo hello-from-tool",
        "not the first update's bare {{cwd}}"
    );
    assert!(events.contains(&DriverEvent::ToolResult {
        turn_id: Some("turn-m1".into()),
        call_id: id,
        output: "hello-from-tool\n".into(),
        is_error: false,
    }));
    assert_eq!(
        texts(&events),
        ["Let me check.", "All done."],
        "one text per model call"
    );
    assert_eq!(ended(&events), TurnStatus::Completed);
}

#[test]
fn thoughts_are_kept_apart_from_the_answer() {
    let (events, ..) = replay("opencode-think", true);
    let thought = events.iter().find_map(|e| match e {
        DriverEvent::Thinking { text, .. } => Some(text.as_str()),
        _ => None,
    });
    assert_eq!(thought, Some("First I consider the question."));
    assert_eq!(texts(&events), ["Hello there."]);
}

#[test]
fn a_permission_request_becomes_an_operator_question_answered_with_its_allow_option() {
    let (events, replies, recorded, mut p) = replay("opencode-permission", true);
    assert_eq!(
        replies, recorded,
        "Allow is answered with the option the recorded client chose"
    );
    let (id, prompt) = events
        .iter()
        .find_map(|e| match e {
            DriverEvent::QuestionRaised {
                question_id,
                source: QuestionSource::NativePermission,
                prompt,
                ..
            } => Some((question_id.clone(), prompt.clone())),
            _ => None,
        })
        .expect("a permission question");
    assert_eq!(prompt.text, "Allow OpenCode to run `echo hello-from-tool`?");
    assert_eq!(prompt.tool.as_deref(), Some("bash"));
    assert_eq!(
        prompt.tool_input.unwrap()["command"],
        "echo hello-from-tool"
    );
    assert_eq!(p.answer_reply(&id, &Answer::Allow), None, "answered once");
    assert_eq!(ended(&events), TurnStatus::Completed);
}

#[test]
fn a_denied_or_expired_permission_picks_reject_or_allow() {
    let asked = recording("opencode-permission")
        .into_iter()
        .find(|r| r["line"]["method"] == "session/request_permission")
        .unwrap();
    for (answer, option) in [
        (Answer::Deny { reason: None }, "reject"),
        (Answer::Expired, "once"),
    ] {
        let mut p = AcpParser::new(true);
        p.on_prompt_sent(4, "turn-m1".into());
        p.parse_line(&asked["line"].to_string());
        let reply = p.answer_reply("perm-0", &answer).unwrap();
        assert_eq!(
            reply["result"]["outcome"],
            json!({ "outcome": "selected", "optionId": option })
        );
    }
}

#[test]
fn a_team_tool_shows_as_rig_and_its_result_is_kept() {
    let (events, ..) = replay("opencode-mcp", true);
    assert!(events.iter().any(|e| matches!(e, DriverEvent::ToolCall { name, input, .. } if name == "rig_ping" && input["message"] == "hi")));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, DriverEvent::ToolResult { output, .. } if output == "pong: hi"))
    );
}

#[test]
fn running_out_of_credit_is_a_usage_limit_and_the_turn_fails() {
    let (events, ..) = replay("opencode-error", true);
    let limit = events.iter().find_map(|e| match e {
        DriverEvent::LimitReached { kind, text, .. } => Some((*kind, text.clone())),
        _ => None,
    });
    let (kind, text) = limit.expect("a limit");
    assert_eq!(kind, LimitKind::Usage);
    assert!(text.contains("Insufficient credits"), "{text}");
    assert_eq!(ended(&events), TurnStatus::Failed);
    assert!(usage(&events).is_none(), "nothing was used");
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, DriverEvent::Context { .. })),
        "a 0 is no context figure"
    );
}

#[test]
fn a_cancelled_prompt_is_interrupted_without_a_zero_usage() {
    let (events, ..) = replay("opencode-interrupt", true);
    assert_eq!(ended(&events), TurnStatus::Interrupted);
    assert!(usage(&events).is_none());
}

#[test]
fn a_resumed_sessions_first_turn_has_tokens_but_no_cost() {
    let (events, ..) = replay("opencode-resume-2", false);
    let Some(DriverEvent::Usage {
        cost,
        output_tokens,
        ..
    }) = usage(&events)
    else {
        panic!("{events:?}")
    };
    assert_eq!(*output_tokens, 12);
    assert_eq!(
        *cost, None,
        "the earlier sessions' total is not reported, so this turn's share is unknown"
    );
    let (fresh, ..) = replay("opencode-resume-1", true);
    assert!(matches!(
        usage(&fresh),
        Some(DriverEvent::Usage { cost: Some(_), .. })
    ));
}
