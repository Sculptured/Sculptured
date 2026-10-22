//! Budget and limit checks: soft at or below hard, known plans and windows, nothing zero.
use model::{DraftIssue, ElementKind, IssueSeverity, TeamDraft};

fn error(kind: ElementKind, id: &str, message: String) -> DraftIssue {
    let mut i = super::check::issue(kind, id, IssueSeverity::Error, "budget-invalid", message);
    i.detail = None;
    i
}

pub fn budget_issues(d: &TeamDraft) -> Vec<DraftIssue> {
    let mut out: Vec<DraftIssue> = d
        .budget
        .problems()
        .into_iter()
        .map(|m| error(ElementKind::Team, &d.id, m))
        .collect();
    for a in &d.agents {
        let adv = &a.advanced;
        let tokens = adv.token_limit.and_then(|t| {
            model::SoftHard {
                soft: t.soft,
                hard: t.tokens,
            }
            .problem("Token limit")
        });
        let turns = adv.turn_limit.and_then(|t| t.problem("Turn limit"));
        let minutes = adv.minute_limit.and_then(|m| m.problem("Time limit"));
        out.extend(
            [tokens, turns, minutes]
                .into_iter()
                .flatten()
                .map(|m| error(ElementKind::Agent, &a.id, m)),
        );
    }
    out
}
