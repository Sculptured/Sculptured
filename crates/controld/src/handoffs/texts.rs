//! The words of handoff messages.
use super::{RESULT_CAP, TRUNCATED};

/// Cuts `text` to at most `RESULT_CAP` bytes on a character boundary, marking the cut.
#[must_use]
pub fn cap_result(text: &str) -> String {
    if text.len() <= RESULT_CAP {
        return text.to_string();
    }
    format!("{}\n{TRUNCATED}", model::cap::head(text, RESULT_CAP))
}

fn result_block(name: &str, text: Option<&str>) -> String {
    let body = match text.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => cap_result(t),
        None => "(no reply text was captured for this turn)".to_string(),
    };
    format!("--- {name}'s result ---\n{body}")
}

/// The first handoff of a start: who handed over, the Job, then each result.
#[must_use]
pub fn first_text(
    names: &[String],
    job: Option<&str>,
    results: &[(String, Option<String>)],
) -> String {
    let mut parts = vec![format!("Handed over by {}", names.join(", "))];
    if let Some(job) = job {
        parts.push(job.to_string());
    }
    parts.extend(
        results
            .iter()
            .map(|(name, text)| result_block(name, text.as_deref())),
    );
    parts.join("\n\n")
}

/// A wave's message: more results from the upstream agents the wave reached.
#[must_use]
pub fn more_text(names: &[String], results: &[(String, Option<String>)]) -> String {
    let mut parts = vec![format!("More work from {}", names.join(", "))];
    parts.extend(
        results
            .iter()
            .map(|(name, text)| result_block(name, text.as_deref())),
    );
    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_result_is_cut_on_a_character_boundary_and_marked() {
        let long = "é".repeat(RESULT_CAP);
        let cut = cap_result(&long);
        assert!(cut.ends_with(&format!("\n{TRUNCATED}")));
        assert!(cut.len() <= RESULT_CAP + TRUNCATED.len() + 1);
        assert_eq!(cap_result("short"), "short");
    }

    #[test]
    fn texts_follow_the_handoff_contract() {
        let t = first_text(
            &["Planner".into(), "Builder 1".into()],
            Some("Review it"),
            &[
                ("Planner".into(), Some("plan".into())),
                ("Builder 1".into(), None),
            ],
        );
        assert_eq!(
            t,
            "Handed over by Planner, Builder 1\n\nReview it\n\n--- Planner's result ---\nplan\n\n--- Builder 1's result ---\n(no reply text was captured for this turn)"
        );
        assert_eq!(
            first_text(&["A".into()], None, &[("A".into(), Some("x".into()))]),
            "Handed over by A\n\n--- A's result ---\nx"
        );
        assert_eq!(
            more_text(&["A".into()], &[("A".into(), Some("y".into()))]),
            "More work from A\n\n--- A's result ---\ny"
        );
        assert_eq!(
            more_text(
                &["B".into(), "C".into()],
                &[
                    ("B".into(), Some("b".into())),
                    ("C".into(), Some("c".into()))
                ]
            ),
            "More work from B, C\n\n--- B's result ---\nb\n\n--- C's result ---\nc"
        );
    }
}
