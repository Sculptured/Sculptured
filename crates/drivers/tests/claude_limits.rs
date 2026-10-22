//! A turn that ends on the provider's usage or rate limit. M0 recorded none; the line is the shape the
//! user's real run showed (an `is_error` result carrying the CLI's words), as fake-claude's `limit` prints it.
use drivers::DriverEvent;
use drivers::claude::ClaudeParser;
use model::{LimitKind, TurnStatus};

const WORDS: &str = "You've hit your session limit · resets 4:30am (America/New_York)";

fn result_line(is_error: bool, text: &str) -> String {
    serde_json::json!({ "type": "result", "subtype": "success", "is_error": is_error, "result": text, "num_turns": 1 }).to_string()
}

fn one_turn(line: &str) -> Vec<DriverEvent> {
    let mut p = ClaudeParser::default();
    p.on_sent("m1".into());
    p.parse_line(line)
}

#[test]
fn a_limit_ending_is_a_failed_turn_with_the_clis_words() {
    let out = one_turn(&result_line(true, WORDS));
    let limit = out
        .iter()
        .position(|e| matches!(e, DriverEvent::LimitReached { .. }))
        .expect("a limit notice");
    let end = out
        .iter()
        .position(|e| {
            matches!(
                e,
                DriverEvent::TurnEnded {
                    status: TurnStatus::Failed,
                    ..
                }
            )
        })
        .expect("ends failed");
    assert!(
        limit < end,
        "the notice comes just before the turn ends: {out:?}"
    );
    match &out[limit] {
        DriverEvent::LimitReached {
            turn_id,
            kind,
            resets,
            text,
        } => {
            assert!(turn_id.is_some());
            assert_eq!(
                (*kind, resets.as_deref(), text.as_str()),
                (LimitKind::Usage, Some("4:30am"), WORDS)
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_rate_limit_is_its_own_kind_and_a_missing_time_stays_unknown() {
    let out = one_turn(&result_line(true, "API Error: Rate limit reached"));
    assert!(
        out.iter().any(|e| matches!(
            e,
            DriverEvent::LimitReached {
                kind: LimitKind::Rate,
                resets: None,
                ..
            }
        )),
        "{out:?}"
    );
}

#[test]
fn a_finished_turn_that_talks_about_limits_is_not_a_limit() {
    let out = one_turn(&result_line(
        false,
        "I added a rate limit; the usage limit is now 100 a minute.",
    ));
    assert!(
        !out.iter()
            .any(|e| matches!(e, DriverEvent::LimitReached { .. })),
        "{out:?}"
    );
    assert!(out.iter().any(|e| matches!(
        e,
        DriverEvent::TurnEnded {
            status: TurnStatus::Completed,
            ..
        }
    )));
}

fn failed_turn_after_text(text: &str) -> Vec<DriverEvent> {
    let mut p = ClaudeParser::default();
    p.on_sent("m1".into());
    let assistant = serde_json::json!({ "type": "assistant", "parent_tool_use_id": null,
        "message": { "id": "msg_1", "content": [{ "type": "text", "text": text }] } });
    let mut out = p.parse_line(&assistant.to_string());
    out.extend(p.parse_line(&result_line(true, "")));
    out
}

fn limit_of(out: &[DriverEvent]) -> Option<(LimitKind, Option<String>, String)> {
    out.iter().find_map(|e| match e {
        DriverEvent::LimitReached {
            kind, resets, text, ..
        } => Some((*kind, resets.clone(), text.clone())),
        _ => None,
    })
}

/// The installed CLI (2.1.292) builds its limit words from "You've hit your ${limit}" with the limits
/// "session limit", "weekly limit", "Opus limit", "Sonnet limit", "Fable limit" and "usage credit limit",
/// and also starts them with "You've reached your" and "You're out of usage credits".
#[test]
fn every_limit_the_cli_names_is_a_usage_limit() {
    let cases: [(&str, Option<&str>); 9] = [
        (
            "You've hit your Opus limit · resets Oct 9, 4pm",
            Some("Oct 9, 4pm"),
        ),
        (
            "You've hit your Sonnet limit · resets 4:30am (America/New_York)",
            Some("4:30am"),
        ),
        (
            "You've hit your weekly limit · resets Mon 9am",
            Some("Mon 9am"),
        ),
        ("You've hit your usage credit limit", None),
        ("You've reached your Fable limit.", None),
        ("You've hit your monthly spend limit.", None),
        (
            "You're out of usage credits. /model to switch models.",
            None,
        ),
        ("Opus 4 requires usage credits.", None),
        (
            "Your plan's allowance is used up · limit resets 5pm",
            Some("5pm"),
        ),
    ];
    for (words, resets) in cases {
        assert_eq!(
            drivers::claude::limit_kind(words),
            Some(LimitKind::Usage),
            "{words}"
        );
        let out = one_turn(&result_line(true, words));
        assert_eq!(
            limit_of(&out),
            Some((
                LimitKind::Usage,
                resets.map(str::to_string),
                words.to_string()
            )),
            "{words}"
        );
    }
}

#[test]
fn limit_words_in_the_agents_text_before_a_failed_turn_count() {
    // The user's real run: the words arrived as the turn's agent text, then the turn failed.
    let out = failed_turn_after_text(WORDS);
    assert_eq!(
        limit_of(&out),
        Some((
            LimitKind::Usage,
            Some("4:30am".to_string()),
            WORDS.to_string()
        )),
        "{out:?}"
    );
    let limit = out
        .iter()
        .position(|e| matches!(e, DriverEvent::LimitReached { .. }))
        .unwrap();
    assert!(
        matches!(
            out[limit + 1],
            DriverEvent::TurnEnded {
                status: TurnStatus::Failed,
                ..
            }
        ),
        "{out:?}"
    );
}

#[test]
fn a_failed_turn_for_another_reason_is_not_a_limit() {
    assert_eq!(
        limit_of(&one_turn(&result_line(
            true,
            "API Error: 500 Internal server error"
        ))),
        None
    );
    assert_eq!(
        limit_of(&failed_turn_after_text(
            "I could not finish: the build failed."
        )),
        None
    );
}

#[test]
fn a_completed_turn_that_mentions_limits_is_never_a_limit() {
    for words in [
        WORDS,
        "You've hit your target; the session limit resets at noon.",
        "You're out of usage credits? No.",
    ] {
        let mut p = ClaudeParser::default();
        p.on_sent("m1".into());
        let assistant = serde_json::json!({ "type": "assistant", "parent_tool_use_id": null,
            "message": { "id": "msg_1", "content": [{ "type": "text", "text": words }] } });
        let mut out = p.parse_line(&assistant.to_string());
        out.extend(p.parse_line(&result_line(false, words)));
        assert_eq!(limit_of(&out), None, "{words}");
        assert!(out.iter().any(|e| matches!(
            e,
            DriverEvent::TurnEnded {
                status: TurnStatus::Completed,
                ..
            }
        )));
    }
}

#[test]
fn agent_prose_about_limits_before_another_failure_is_not_a_limit() {
    // The agent's own words are read only for the CLI's limit message at the start of its last text.
    for prose in [
        "I added a rate limit to the client.",
        "The session limit resets at noon, so I added a retry.",
        "Done. You've hit your target for this week.",
    ] {
        let mut p = ClaudeParser::default();
        p.on_sent("m1".into());
        let assistant = serde_json::json!({ "type": "assistant", "parent_tool_use_id": null,
            "message": { "id": "msg_1", "content": [{ "type": "text", "text": prose }] } });
        let mut out = p.parse_line(&assistant.to_string());
        out.extend(p.parse_line(&result_line(true, "API Error: 529 Overloaded")));
        assert_eq!(limit_of(&out), None, "{prose}");
        assert!(out.iter().any(|e| matches!(
            e,
            DriverEvent::TurnEnded {
                status: TurnStatus::Failed,
                ..
            }
        )));
    }
}
