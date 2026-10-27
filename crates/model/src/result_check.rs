//! Result checks: rules an agent's result must pass before its work is handed on. The result carries
//! its facts as a JSON block at its end (a fenced `json` block, or the whole reply as JSON), and each
//! check tests one field of it with the same tests as a team's change rules (`mandate`). Sculptured
//! evaluates them itself, so their results are verified.
use crate::mandate::{RuleResult, RuleTest, check_field};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct ResultCheck {
    pub id: String,
    /// A dotted path into the result's JSON block, such as `part_number` or `quotes.0.price`.
    pub field: String,
    pub test: RuleTest,
}

// Limits are JSON numbers, which are never NaN.
impl Eq for ResultCheck {}

/// The result's JSON: its last fenced block that parses as a JSON object, else the whole reply if
/// it is one.
#[must_use]
pub fn result_json(text: &str) -> Option<Value> {
    let fenced = text
        .split("```")
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .map(|(_, b)| b);
    let blocks: Vec<Value> = fenced
        .filter_map(|b| {
            let body = b.strip_prefix("json").unwrap_or(b);
            serde_json::from_str::<Value>(body.trim()).ok()
        })
        .filter(Value::is_object)
        .collect();
    blocks.into_iter().last().or_else(|| {
        serde_json::from_str::<Value>(text.trim())
            .ok()
            .filter(Value::is_object)
    })
}

/// Each check on `text`, in order. Without a JSON block every check fails and says so.
#[must_use]
pub fn check_result(checks: &[ResultCheck], text: &str) -> Vec<RuleResult> {
    let Some(json) = result_json(text) else {
        return checks
            .iter()
            .map(|c| RuleResult {
                rule_id: c.id.clone(),
                field: c.field.clone(),
                passed: false,
                actual: None,
                words: format!("{}: the result has no JSON block", c.field),
            })
            .collect();
    };
    checks
        .iter()
        .map(|c| check_field(&c.id, &c.field, &c.test, &json))
        .collect()
}

/// What an agent is told its result must have, one line per check: "`amount`: at most 5000".
#[must_use]
pub fn wanted(checks: &[ResultCheck]) -> Vec<String> {
    checks
        .iter()
        .map(|c| {
            let what = match &c.test {
                RuleTest::AtMost { value } => format!("a number at most {value}"),
                RuleTest::AtLeast { value } => format!("a number at least {value}"),
                RuleTest::OneOf { values } => format!("one of: {}", values.join(", ")),
                RuleTest::Present => "filled in".to_string(),
            };
            format!("`{}`: {what}", c.field)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn checks() -> Vec<ResultCheck> {
        vec![
            ResultCheck {
                id: "part".into(),
                field: "part_number".into(),
                test: RuleTest::Present,
            },
            ResultCheck {
                id: "price".into(),
                field: "price".into(),
                test: RuleTest::AtMost { value: 5000.0 },
            },
        ]
    }

    #[test]
    fn the_last_json_block_is_the_result_and_each_check_says_what_it_found() {
        let text = "Found two quotes.\n```json\n{\"price\": 9}\n```\nThe best:\n```json\n{\"part_number\": \"SP-118\", \"price\": \"$4,200\"}\n```";
        assert_eq!(
            result_json(text),
            Some(json!({"part_number": "SP-118", "price": "$4,200"}))
        );
        let r = check_result(&checks(), text);
        assert!(r.iter().all(|r| r.passed), "{r:?}");
        assert_eq!(r[1].words, "price is \"$4,200\", at most 5000");
    }

    #[test]
    fn no_json_fails_every_check_and_a_bare_json_reply_counts() {
        let r = check_result(&checks(), "I found the part, it is SP-118.");
        assert_eq!(r.iter().filter(|r| !r.passed).count(), 2);
        assert_eq!(r[0].words, "part_number: the result has no JSON block");
        let r = check_result(&checks(), r#"{"price": 7300}"#);
        assert_eq!((r[0].passed, r[1].passed), (false, false));
        assert_eq!(r[0].words, "part_number is missing");
    }

    #[test]
    fn an_agent_is_told_what_its_result_must_have() {
        assert_eq!(
            wanted(&checks()),
            ["`part_number`: filled in", "`price`: a number at most 5000"]
        );
    }
}
