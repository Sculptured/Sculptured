//! A team's mandate: the rules a change its agents propose (an order, a reschedule, a record in another
//! system) must meet, and whether Sculptured approves a change that meets them without asking a person.
//! Sculptured evaluates the rules itself, so their results are verified, not agent-reported.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct Mandate {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<ChangeRule>>", optional)]
    pub rules: Vec<ChangeRule>,
    /// A change every applicable rule passes is approved by Sculptured; otherwise a person decides.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(as = "Option<bool>", optional)]
    pub approve_within: bool,
}

impl Mandate {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty() && !self.approve_within
    }
}

/// One rule: for changes of `action` (`*`: any), the payload's `field` must pass `test`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct ChangeRule {
    pub id: String,
    /// The action it applies to, as the agent names it (`create_purchase_order`), or `*` for every change.
    #[serde(default = "any_action")]
    pub action: String,
    /// Where in the change's payload: a dotted path, such as `amount` or `lines.0.item`.
    pub field: String,
    pub test: RuleTest,
}

fn any_action() -> String {
    "*".into()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[ts(export)]
pub enum RuleTest {
    /// A number (or a number written as text) no greater than `value`.
    AtMost { value: f64 },
    /// A number no less than `value`.
    AtLeast { value: f64 },
    /// One of `values`, compared as text, ignoring case and surrounding spaces.
    OneOf { values: Vec<String> },
    /// Present and not empty.
    Present,
}

// Limits are JSON numbers, which are never NaN, so every value equals itself.
impl Eq for RuleTest {}
impl Eq for ChangeRule {}
impl Eq for Mandate {}

/// What one rule found, in plain words, with the value it read.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct RuleResult {
    pub rule_id: String,
    pub field: String,
    pub passed: bool,
    #[ts(type = "unknown")]
    pub actual: Option<Value>,
    /// "amount is 4200, at most 5000".
    pub words: String,
}

impl ChangeRule {
    #[must_use]
    pub fn applies_to(&self, action: &str) -> bool {
        let a = self.action.trim();
        a == "*" || a.eq_ignore_ascii_case(action.trim())
    }

    #[must_use]
    pub fn check(&self, payload: &Value) -> RuleResult {
        check_field(&self.id, &self.field, &self.test, payload)
    }
}

/// Rule `id`: `payload`'s `field` against `test`. A missing or empty field fails.
#[must_use]
pub fn check_field(id: &str, field: &str, test: &RuleTest, payload: &Value) -> RuleResult {
    let actual = lookup(payload, field).filter(|v| !is_empty(v)).cloned();
    let (passed, words) = match &actual {
        None => (false, format!("{field} is missing")),
        Some(v) => test.judge(field, v),
    };
    RuleResult {
        rule_id: id.to_string(),
        field: field.to_string(),
        passed,
        actual,
        words,
    }
}

impl RuleTest {
    fn judge(&self, field: &str, v: &Value) -> (bool, String) {
        let shown = shown(v);
        match self {
            Self::AtMost { value } => match number(v) {
                Some(n) => (
                    n <= *value,
                    format!("{field} is {shown}, at most {}", num(*value)),
                ),
                None => (false, format!("{field} is {shown}, not a number")),
            },
            Self::AtLeast { value } => match number(v) {
                Some(n) => (
                    n >= *value,
                    format!("{field} is {shown}, at least {}", num(*value)),
                ),
                None => (false, format!("{field} is {shown}, not a number")),
            },
            Self::OneOf { values } => {
                let text = text(v);
                let ok = values
                    .iter()
                    .any(|x| x.trim().eq_ignore_ascii_case(text.trim()));
                (
                    ok,
                    format!(
                        "{field} is {shown}, {} {}",
                        if ok { "one of" } else { "not one of" },
                        values.join(", ")
                    ),
                )
            }
            Self::Present => (true, format!("{field} is {shown}")),
        }
    }
}

/// The rules for `action`, each checked against `payload`, in the mandate's order.
#[must_use]
pub fn evaluate(m: &Mandate, action: &str, payload: &Value) -> Vec<RuleResult> {
    m.rules
        .iter()
        .filter(|r| r.applies_to(action))
        .map(|r| r.check(payload))
        .collect()
}

/// `a.b.0.c` in a JSON value; a number segment also indexes a list.
#[must_use]
pub fn lookup<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .filter(|s| !s.is_empty())
        .try_fold(v, |cur, seg| match cur {
            Value::Object(o) => o.get(seg),
            Value::Array(a) => seg.parse::<usize>().ok().and_then(|i| a.get(i)),
            _ => None,
        })
}

fn is_empty(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => s.trim().is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        _ => false,
    }
}

/// A number, or text that is one ("4,200.00" and "$4200" too: thousands separators and a currency sign are dropped).
fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s
            .trim()
            .trim_start_matches(['$', '€', '£'])
            .replace(',', "")
            .parse()
            .ok(),
        _ => None,
    }
}

fn text(v: &Value) -> String {
    v.as_str().map_or_else(|| v.to_string(), str::to_string)
}

fn shown(v: &Value) -> String {
    match v {
        Value::String(s) => format!("\"{s}\""),
        other => other.to_string(),
    }
}

fn num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rule(id: &str, action: &str, field: &str, test: RuleTest) -> ChangeRule {
        ChangeRule {
            id: id.into(),
            action: action.into(),
            field: field.into(),
            test,
        }
    }

    fn mandate() -> Mandate {
        Mandate {
            approve_within: true,
            rules: vec![
                rule(
                    "limit",
                    "create_purchase_order",
                    "amount",
                    RuleTest::AtMost { value: 5000.0 },
                ),
                rule(
                    "vendor",
                    "create_purchase_order",
                    "supplier",
                    RuleTest::OneOf {
                        values: vec!["Acme Hydraulics".into(), "Bolt & Co".into()],
                    },
                ),
                rule("part", "*", "lines.0.item", RuleTest::Present),
            ],
        }
    }

    #[test]
    fn a_purchase_order_within_the_limit_from_an_approved_supplier_passes_every_rule() {
        let po = json!({ "amount": "4,200.00", "supplier": " acme hydraulics ", "lines": [{ "item": "SP-118" }] });
        let r = evaluate(&mandate(), "create_purchase_order", &po);
        assert!(r.iter().all(|x| x.passed), "{r:?}");
        assert_eq!(r[0].words, "amount is \"4,200.00\", at most 5000");
        assert_eq!(
            r[1].words,
            "supplier is \" acme hydraulics \", one of Acme Hydraulics, Bolt & Co"
        );
    }

    #[test]
    fn over_the_limit_an_unknown_supplier_or_a_missing_part_fails_and_says_why() {
        let po = json!({ "amount": 7300, "supplier": "Cheap Parts Ltd", "lines": [] });
        let r = evaluate(&mandate(), "create_purchase_order", &po);
        assert_eq!(
            r.iter().map(|x| x.passed).collect::<Vec<_>>(),
            [false, false, false]
        );
        assert_eq!(r[0].words, "amount is 7300, at most 5000");
        assert_eq!(
            r[1].words,
            "supplier is \"Cheap Parts Ltd\", not one of Acme Hydraulics, Bolt & Co"
        );
        assert_eq!(r[2].words, "lines.0.item is missing");
        assert_eq!(
            evaluate(&mandate(), "reschedule", &po).len(),
            1,
            "only the rules for every change apply"
        );
    }

    #[test]
    fn text_that_is_not_a_number_fails_a_limit() {
        let r = rule("x", "*", "amount", RuleTest::AtMost { value: 10.0 })
            .check(&json!({ "amount": "about nine" }));
        assert!(!r.passed);
        assert_eq!(r.words, "amount is \"about nine\", not a number");
    }

    #[test]
    fn an_empty_mandate_is_left_out_of_a_team_file() {
        assert!(Mandate::default().is_empty());
        let one: Mandate = serde_json::from_value(json!({ "rules": [{ "id": "r", "field": "amount", "test": { "kind": "at-most", "value": 5 } }] })).unwrap();
        assert_eq!(one.rules[0].action, "*", "no action: every change");
    }
}
