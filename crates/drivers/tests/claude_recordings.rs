use drivers::DriverEvent;
use drivers::claude::ClaudeParser;
use model::TurnStatus;
use serde_json::Value;
use std::path::PathBuf;

const CLAUDE_RECORDINGS: [&str; 9] = [
    "claude-basic",
    "claude-init",
    "claude-interrupt",
    "claude-permission",
    "claude-queue",
    "claude-resume-1",
    "claude-resume-2",
    "claude-subagent",
    "claude-tool",
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

#[derive(Default)]
struct Opts {
    /// Skip `on_interrupt` for `in` control_request lines.
    ignore_interrupts: bool,
    /// Call `on_interrupt` before any line, when no turn exists.
    interrupt_first: bool,
    /// Register an extra message, `queued`, right after `m1`, as if the operator
    /// had sent it while turn-1 was running.
    queue_extra: bool,
}

/// Replays a recording: `in` user lines register message ids `m1`, `m2`, ...,
/// `in` interrupt lines mark an interrupt, and `out` lines are parsed.
fn replay_with(name: &str, opts: Opts) -> Vec<DriverEvent> {
    let mut p = ClaudeParser::default();
    if opts.interrupt_first {
        p.on_interrupt();
    }
    let mut n = 0;
    let mut out = Vec::new();
    for entry in recording(name) {
        let line = &entry["line"];
        match entry["dir"].as_str() {
            Some("in") if line["type"] == "user" => {
                n += 1;
                p.on_sent(format!("m{n}"));
                if n == 1 && opts.queue_extra {
                    p.on_sent("queued".into());
                }
            }
            Some("in") if line["type"] == "control_request" => {
                if !opts.ignore_interrupts {
                    p.on_interrupt();
                }
            }
            Some("out") => out.extend(p.parse_line(&line.to_string())),
            _ => {}
        }
    }
    out
}

fn replay(name: &str) -> Vec<DriverEvent> {
    replay_with(name, Opts::default())
}

fn no_unrecognized(events: &[DriverEvent]) {
    for e in events {
        if let DriverEvent::Unrecognized { raw } = e {
            panic!("line not understood: {raw}");
        }
    }
}

fn consumed(events: &[DriverEvent]) -> Vec<(&str, &str)> {
    events
        .iter()
        .filter_map(|e| match e {
            DriverEvent::MessageConsumed {
                message_id,
                turn_id,
            } => Some((message_id.as_str(), turn_id.as_str())),
            _ => None,
        })
        .collect()
}

/// (turn, [input, output, cache read, cache write]) per turn: the in-turn reports and the turn's last
/// report summed, which is what the CLI's `modelUsage` says the turn used.
fn usage(events: &[DriverEvent]) -> Vec<(&str, [u64; 4])> {
    let mut out: Vec<(&str, [u64; 4])> = Vec::new();
    for e in events {
        let DriverEvent::Usage {
            turn_id,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
            ..
        } = e
        else {
            continue;
        };
        let (t, u) = (
            turn_id.as_deref().unwrap_or(""),
            [
                *input_tokens,
                *output_tokens,
                *cache_read_tokens,
                *cache_write_tokens,
            ],
        );
        match out.last_mut() {
            Some((last, sum)) if *last == t => {
                for i in 0..4 {
                    sum[i] += u[i];
                }
            }
            _ => out.push((t, u)),
        }
    }
    out
}

fn ends(events: &[DriverEvent]) -> Vec<(&str, TurnStatus)> {
    events
        .iter()
        .filter_map(|e| match e {
            DriverEvent::TurnEnded { turn_id, status } => Some((turn_id.as_str(), *status)),
            _ => None,
        })
        .collect()
}

#[test]
fn every_claude_recording_parses_without_unrecognized() {
    // hook_*, init, thinking_tokens, task_*, rate_limit_event, control_response and
    // thinking blocks are all deliberately ignored, so nothing is Unrecognized.
    for name in CLAUDE_RECORDINGS {
        no_unrecognized(&replay(name));
    }
}

#[test]
fn init_recording_starts_no_turn() {
    // claude-init: hook lines only, with no user message and no init.
    assert!(replay("claude-init").is_empty());
}

#[test]
fn basic_recording() {
    let ev = replay("claude-basic");
    assert_eq!(consumed(&ev), [("m1", "turn-1")]);
    assert!(ev.contains(&DriverEvent::Text {
        turn_id: Some("turn-1".into()),
        text: "ready".into()
    }));
    // result@1595 modelUsage, baseline 0.
    assert_eq!(usage(&ev), [("turn-1", [2, 4, 10225, 14338])]);
    assert_eq!(ends(&ev), [("turn-1", TurnStatus::Completed)]);
}

#[test]
fn tool_recording_pairs_calls_and_results() {
    let ev = replay("claude-tool");
    let call = ev
        .iter()
        .find_map(|e| match e {
            DriverEvent::ToolCall { call_id, name, .. } if name == "Bash" => Some(call_id.clone()),
            _ => None,
        })
        .expect("a Bash call");
    assert!(ev.iter().any(|e| matches!(e, DriverEvent::ToolResult { call_id, is_error: false, .. } if *call_id == call)));
    assert_eq!(usage(&ev), [("turn-1", [4, 242, 36471, 17281])]);
    assert_eq!(ends(&ev), [("turn-1", TurnStatus::Completed)]);
}

#[test]
fn interrupt_recording_orders_tool_result_before_turn_end() {
    let ev = replay("claude-interrupt");
    let call = ev
        .iter()
        .find_map(|e| match e {
            DriverEvent::ToolCall { call_id, .. } => Some(call_id.clone()),
            _ => None,
        })
        .expect("the interrupted Bash call (@2342)");
    let rejected = ev
        .iter()
        .position(|e| matches!(e, DriverEvent::ToolResult { turn_id: Some(t), call_id, is_error: true, .. } if t == "turn-1" && *call_id == call))
        .expect("the synthetic is_error tool_result (@2431)");
    // The marker line `[Request interrupted by user for tool use]` (@2431) yields nothing,
    // so the result's Usage and TurnEnded (@2434) follow directly.
    assert!(
        matches!(&ev[rejected + 1], DriverEvent::Usage { turn_id: Some(t), .. } if t == "turn-1"),
        "{:?}",
        &ev[rejected..]
    );
    assert_eq!(
        ev[rejected + 2],
        DriverEvent::TurnEnded {
            turn_id: "turn-1".into(),
            status: TurnStatus::Interrupted
        }
    );
    assert_eq!(consumed(&ev), [("m1", "turn-1"), ("m2", "turn-2")]);
    assert_eq!(
        ends(&ev),
        [
            ("turn-1", TurnStatus::Interrupted),
            ("turn-2", TurnStatus::Completed)
        ]
    );
    // modelUsage @2434 is 2/128/11896/12668; @3096 is 4/132/36460/17487, so turn-2 is the delta.
    assert_eq!(
        usage(&ev),
        [
            ("turn-1", [2, 128, 11896, 12668]),
            ("turn-2", [2, 4, 24564, 4819])
        ]
    );
}

#[test]
fn aborted_tools_alone_marks_the_turn_interrupted() {
    let ev = replay_with(
        "claude-interrupt",
        Opts {
            ignore_interrupts: true,
            ..Opts::default()
        },
    );
    assert_eq!(
        ends(&ev),
        [
            ("turn-1", TurnStatus::Interrupted),
            ("turn-2", TurnStatus::Completed)
        ]
    );
}

#[test]
fn interrupt_with_no_turn_is_a_no_op() {
    let ev = replay_with(
        "claude-basic",
        Opts {
            interrupt_first: true,
            ..Opts::default()
        },
    );
    assert_eq!(ends(&ev), [("turn-1", TurnStatus::Completed)]);
}

#[test]
fn queue_recording_runs_two_turns_in_order() {
    let ev = replay("claude-queue");
    // init repeats before each turn (@927, @2143) and starts nothing itself.
    assert_eq!(
        ev.iter()
            .filter(|e| matches!(e, DriverEvent::TurnStarted { .. }))
            .count(),
        2
    );
    assert_eq!(consumed(&ev), [("m1", "turn-1"), ("m2", "turn-2")]);
    let second = ev
        .iter()
        .position(|e| {
            *e == DriverEvent::TurnStarted {
                turn_id: "turn-2".into(),
            }
        })
        .expect("turn-2");
    assert!(ev[..second].contains(&DriverEvent::TurnEnded {
        turn_id: "turn-1".into(),
        status: TurnStatus::Completed
    }));
    assert_eq!(
        ev[second + 1],
        DriverEvent::MessageConsumed {
            message_id: "m2".into(),
            turn_id: "turn-2".into()
        }
    );
    assert!(ev.contains(&DriverEvent::Text {
        turn_id: Some("turn-1".into()),
        text: "one".into()
    }));
    assert!(ev.contains(&DriverEvent::Text {
        turn_id: Some("turn-2".into()),
        text: "two".into()
    }));
    // modelUsage @2136 is 2/3/11896/16808; @3522 is 4/51/40600/17170.
    assert_eq!(
        usage(&ev),
        [
            ("turn-1", [2, 3, 11896, 16808]),
            ("turn-2", [2, 48, 28704, 362])
        ]
    );
    assert_eq!(
        ends(&ev),
        [
            ("turn-1", TurnStatus::Completed),
            ("turn-2", TurnStatus::Completed)
        ]
    );
}

#[test]
fn queued_message_survives_marker_and_subagent_prompt() {
    // A message queued during turn-1 is not consumed by the interrupt marker (@2431).
    // The recording's own second message queues behind it.
    let ev = replay_with(
        "claude-interrupt",
        Opts {
            queue_extra: true,
            ..Opts::default()
        },
    );
    assert_eq!(consumed(&ev), [("m1", "turn-1"), ("queued", "turn-2")]);
    // Nor by the subagent's prompt line (@3277). That recording has no second turn.
    let ev = replay_with(
        "claude-subagent",
        Opts {
            queue_extra: true,
            ..Opts::default()
        },
    );
    assert_eq!(consumed(&ev), [("m1", "turn-1")]);
}

#[test]
fn subagent_recording_links_children_and_counts_their_usage() {
    let ev = replay("claude-subagent");
    let agent = ev
        .iter()
        .find_map(|e| match e {
            DriverEvent::ToolCall {
                call_id,
                name,
                parent_call_id: None,
                ..
            } if name == "Agent" => Some(call_id.clone()),
            _ => None,
        })
        .expect("the Agent call (@3265)");
    assert!(ev.iter().any(|e| matches!(e, DriverEvent::ToolCall { name, parent_call_id: Some(p), .. } if name == "Bash" && *p == agent)));
    assert!(ev.iter().any(|e| matches!(e, DriverEvent::ToolResult { call_id, is_error: false, .. } if *call_id == agent)));
    assert_eq!(consumed(&ev), [("m1", "turn-1")]);
    // modelUsage sum at @10782, which includes the subagent. result.usage is 4/484/36490/17984.
    assert_eq!(usage(&ev), [("turn-1", [8, 943, 56502, 40601])]);
    assert_eq!(ends(&ev), [("turn-1", TurnStatus::Completed)]);
}

#[test]
fn resume_recordings() {
    // resume-1 is a fresh process (`--session-id`), so baseline 0 is right.
    let ev = replay("claude-resume-1");
    assert_eq!(consumed(&ev), [("m1", "turn-1")]);
    assert_eq!(usage(&ev), [("turn-1", [2, 121, 11896, 12679])]);
    assert_eq!(ends(&ev), [("turn-1", TurnStatus::Completed)]);
    // resume-2 (`--resume`) recalls the code. Its modelUsage carries resume-1's totals,
    // so its per-turn usage needs M2's resume baseline and is not asserted here.
    let ev = replay("claude-resume-2");
    assert_eq!(consumed(&ev), [("m1", "turn-1")]);
    assert!(ev.contains(&DriverEvent::Text {
        turn_id: Some("turn-1".into()),
        text: "PLUM-42".into()
    }));
    assert_eq!(ends(&ev), [("turn-1", TurnStatus::Completed)]);
}
