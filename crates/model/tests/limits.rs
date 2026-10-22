use model::LimitKind;
use serde_json::{json, to_value};

#[test]
fn the_reset_time_is_read_from_the_clis_words() {
    assert_eq!(
        model::limit_resets("You've hit your session limit · resets 4:30am (America/New_York)")
            .as_deref(),
        Some("4:30am")
    );
    assert_eq!(
        model::limit_resets("You've hit your usage limit. Try again at 4:30 AM.").as_deref(),
        Some("4:30 AM")
    );
    assert_eq!(
        model::limit_resets("You've hit your usage limit."),
        None,
        "no time named: unknown, never guessed"
    );
    assert_eq!(
        model::limit_resets(&format!("resets {}", "x".repeat(200))),
        None,
        "too long to be a time"
    );
    assert_eq!(to_value(LimitKind::Usage).unwrap(), json!("usage"));
    assert_eq!(to_value(LimitKind::Rate).unwrap(), json!("rate"));
}
