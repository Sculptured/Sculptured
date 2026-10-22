//! A resumed Claude session reports `modelUsage` that carries across `--resume` (claude-resume-2), so the
//! parser starts from the previous session's totals instead of counting the whole session again.
use drivers::DriverEvent;
use drivers::claude::ClaudeParser;

fn result_with_usage(u: [u64; 4]) -> String {
    serde_json::json!({ "type": "result", "subtype": "success", "is_error": false, "result": "ok", "num_turns": 1,
        "modelUsage": { "claude-x": { "inputTokens": u[0], "outputTokens": u[1],
                                      "cacheReadInputTokens": u[2], "cacheCreationInputTokens": u[3] } } })
    .to_string()
}

fn usage(p: &mut ClaudeParser, line: &str) -> [u64; 4] {
    p.on_sent("m".into());
    p.parse_line(line)
        .into_iter()
        .find_map(|e| match e {
            DriverEvent::Usage {
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_write_tokens,
                ..
            } => Some([
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_write_tokens,
            ]),
            _ => None,
        })
        .expect("a usage event")
}

#[test]
fn a_usage_baseline_is_subtracted_from_the_first_turn_after_a_resume() {
    let line = result_with_usage([100, 50, 10, 5]);
    assert_eq!(usage(&mut ClaudeParser::default(), &line), [100, 50, 10, 5]);
    let mut resumed = ClaudeParser::with_usage_baseline([90, 40, 10, 0]);
    assert_eq!(usage(&mut resumed, &line), [10, 10, 0, 5]);
    // The next turn counts from the new totals, not from the baseline again.
    assert_eq!(
        usage(&mut resumed, &result_with_usage([130, 60, 10, 5])),
        [30, 10, 0, 0]
    );
}

fn assistant(id: &str, u: [u64; 4], parent: Option<&str>) -> String {
    serde_json::json!({ "type": "assistant", "parent_tool_use_id": parent,
        "message": { "id": id, "role": "assistant", "content": [{ "type": "text", "text": "…" }],
                     "usage": { "input_tokens": u[0], "output_tokens": u[1], "cache_read_input_tokens": u[2], "cache_creation_input_tokens": u[3] } } })
    .to_string()
}

/// Every usage event of `lines`, as `([input, output, cache_read, cache_write], in_turn)`.
fn usages(p: &mut ClaudeParser, lines: &[String]) -> Vec<([u64; 4], bool)> {
    p.on_sent("m".into());
    lines
        .iter()
        .flat_map(|l| p.parse_line(l))
        .filter_map(|e| match e {
            DriverEvent::Usage {
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_write_tokens,
                in_turn,
                ..
            } => Some((
                [
                    input_tokens,
                    output_tokens,
                    cache_read_tokens,
                    cache_write_tokens,
                ],
                in_turn,
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn usage_arrives_per_message_not_only_at_result() {
    let got = usages(
        &mut ClaudeParser::default(),
        &[
            assistant("msg_1", [3, 10, 1000, 50], None),
            assistant("msg_2", [2, 20, 1100, 0], None),
        ],
    );
    assert_eq!(
        got,
        [([3, 10, 1000, 50], true), ([2, 20, 1100, 0], true)],
        "each model call is counted while the turn runs"
    );
}

#[test]
fn repeated_lines_of_one_message_count_once() {
    // One message spans several lines (one per content block); its usage repeats, and output grows.
    let lines = [
        assistant("msg_1", [3, 5, 1000, 50], None),
        assistant("msg_1", [3, 12, 1000, 50], None),
        assistant("sub", [9, 9, 9, 9], Some("toolu_1")),
    ];
    assert_eq!(
        usages(&mut ClaudeParser::default(), &lines),
        [
            ([3, 5, 1000, 50], true),
            ([0, 7, 0, 0], true),
            ([9, 9, 9, 9], true)
        ],
        "growth only; a subagent's call counts too"
    );
}

#[test]
fn result_reconciles_to_model_usage_exactly() {
    let lines = [
        assistant("msg_1", [3, 10, 1000, 50], None),
        result_with_usage([10, 40, 3000, 50]),
    ];
    let got = usages(&mut ClaudeParser::default(), &lines);
    assert_eq!(got, [([3, 10, 1000, 50], true), ([7, 30, 2000, 0], false)]);
    let sum = got.iter().fold([0; 4], |a, (u, _)| {
        [a[0] + u[0], a[1] + u[1], a[2] + u[2], a[3] + u[3]]
    });
    assert_eq!(
        sum,
        [10, 40, 3000, 50],
        "the turn's total is modelUsage's, as before"
    );
}

#[test]
fn each_model_call_of_the_main_conversation_reports_its_context_once() {
    let mut p = ClaudeParser::default();
    p.on_sent("m".into());
    let contexts = |events: Vec<DriverEvent>| {
        events
            .into_iter()
            .filter_map(|e| match e {
                DriverEvent::Context { used_tokens, .. } => Some(used_tokens),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        contexts(p.parse_line(&assistant("msg_1", [5, 10, 40_000, 2_000], None))),
        [42_005]
    );
    assert_eq!(
        contexts(p.parse_line(&assistant("msg_1", [5, 30, 40_000, 2_000], None))),
        Vec::<u64>::new(),
        "the same call again: no new reading"
    );
    assert_eq!(
        contexts(p.parse_line(&assistant("msg_2", [1, 1, 9_000, 0], Some("toolu_sub")))),
        Vec::<u64>::new(),
        "a subagent's call is not this agent's context"
    );
}
