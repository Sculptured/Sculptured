//! The Claude parser on the claude-tool recording with one line changed: a line of an unknown type
//! is kept and the turn goes on, and a very large tool result passes through intact.
use drivers::DriverEvent;
use drivers::claude::ClaudeParser;
use model::TurnStatus;
use serde_json::{Value, json};
use std::path::PathBuf;

/// The recording's lines: `in` user lines register `m1`, `m2`, …; `out` lines go through `edit`
/// (which may add lines after it) and are parsed.
fn replay_edited(edit: impl Fn(&Value) -> Vec<Value>) -> Vec<DriverEvent> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/recordings/claude-tool.jsonl");
    let text = std::fs::read_to_string(&path).unwrap();
    let mut p = ClaudeParser::default();
    let mut n = 0;
    let mut out = Vec::new();
    for entry in text
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
    {
        let line = &entry["line"];
        match entry["dir"].as_str() {
            Some("in") if line["type"] == "user" => {
                n += 1;
                p.on_sent(format!("m{n}"));
            }
            Some("out") => {
                for l in edit(line) {
                    out.extend(p.parse_line(&l.to_string()));
                }
            }
            _ => {}
        }
    }
    out
}

fn is_tool_result(line: &Value) -> bool {
    line["type"] == "user" && line["message"]["content"][0]["type"] == "tool_result"
}

fn completed(events: &[DriverEvent]) -> bool {
    matches!(
        events.last(),
        Some(DriverEvent::TurnEnded {
            status: TurnStatus::Completed,
            ..
        })
    )
}

#[test]
fn an_unknown_line_is_kept_and_the_turn_goes_on() {
    let got = replay_edited(|l| {
        if is_tool_result(l) {
            vec![l.clone(), json!({ "type": "mystery", "detail": 1 })]
        } else {
            vec![l.clone()]
        }
    });
    assert!(
        got.iter()
            .any(|e| matches!(e, DriverEvent::Unrecognized { raw } if raw["type"] == "mystery")),
        "{got:?}"
    );
    assert!(completed(&got), "{got:?}");
}

#[test]
fn a_big_tool_result_passes_through_intact() {
    let big = "x".repeat(2_000_000);
    let got = replay_edited(|l| {
        let mut l = l.clone();
        if is_tool_result(&l) {
            l["message"]["content"][0]["content"] = json!(big);
        }
        vec![l]
    });
    assert!(
        got.iter().any(
            |e| matches!(e, DriverEvent::ToolResult { output, .. } if output.len() == 2_000_000)
        ),
        "the whole result arrives"
    );
    assert!(completed(&got));
}
