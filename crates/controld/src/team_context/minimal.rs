//! The minimal briefing (`advanced.briefing: minimal`): only the agent's role, its own job and data
//! contract, its own skills, and one line on how to finish. No team architecture, neighbours' jobs,
//! group notes, workflow description or working rules: every one of those is re-read on every call,
//! and an agent that only does its assignment does not need them. Its managed children inherit it.
use model::{BriefingLevel, DraftAgent, TeamDraft};

/// Whether `seat` is one of the team's own agents set to the minimal briefing.
pub(super) fn is_minimal(d: &TeamDraft, seat: &str) -> bool {
    d.agent_for_seat(seat)
        .is_some_and(|a| a.advanced.briefing == Some(BriefingLevel::Minimal))
}

/// `message` is what the briefing goes with: when it already carries the job, the job is not repeated.
pub(super) fn briefing(d: &TeamDraft, me: &DraftAgent, message: &str) -> String {
    let mut lines = vec![format!("Your role: {} [{}] in {}", me.name, me.id, d.name)];
    let job = me.job.trim();
    if !job.is_empty() && !message.contains(job) {
        lines.push(format!("Your job: {job}"));
    }
    lines.extend(super::own_contract(me));
    lines.extend(crate::skills::briefing_lines(d, me));
    lines.extend(crate::status_page::briefing_line(d, me));
    lines.push(finish(d, me).into());
    lines.join("\n\n")
}

/// A managed child's briefing when its parent `parent` is minimal: who it helps, and how to finish.
pub(super) fn child(d: &TeamDraft, parent: &str, seat: &str) -> Option<String> {
    let p = d.agent_for_seat(parent).filter(|_| is_minimal(d, parent))?;
    Some(format!(
        "Your role: managed child agent {seat}, a helper of {} [{}] in {}\n\nDo the task in the message below, then reply with your result.",
        p.name, p.id, d.name
    ))
}

/// One line on how this agent's work ends.
fn finish(d: &TeamDraft, me: &DraftAgent) -> &'static str {
    let Some(w) = &d.workflow else {
        return "When you are done, reply with your result.";
    };
    if w.dispatcher == me.id {
        return "Finish by calling dispatch_work once with the batch, then reply.";
    }
    let works = w.collector == me.id
        || w.stages.iter().any(|s| {
            s.agent == me.id
                || s.workers.contains(&me.id)
                || s.reviewers.contains(&me.id)
                || s.escalate.as_ref().is_some_and(|e| e.agent == me.id)
        });
    if works {
        "Finish each assignment by calling complete_work with its assignment ID, an outcome and your result; a plain reply does not advance the work."
    } else {
        "When you are done, reply with your result."
    }
}

#[cfg(test)]
mod tests {

    use model::BriefingLevel;

    fn textbook() -> model::TeamDraft {
        let mut d = rigspec::textbook::template("book", "Book", false);
        d.input = None;
        for a in &mut d.agents {
            a.advanced.briefing = Some(BriefingLevel::Minimal);
        }
        d
    }

    #[test]
    fn a_minimal_agent_gets_its_role_and_job_and_nothing_about_the_team() {
        let d = textbook();
        let me = d.agents.iter().find(|a| a.id == "visualizer").unwrap();
        let text = crate::team_context::briefing(&d, &d.seat_id(&me.id).unwrap()).unwrap();
        assert!(
            text.contains(&format!("Your role: {} [{}]", me.name, me.id)),
            "{text}"
        );
        assert!(text.contains(me.job.trim()), "its own job");
        assert!(
            text.contains("complete_work"),
            "how a workflow worker finishes"
        );
        for absent in [
            "Team architecture",
            "dynamic workflow",
            "Configured stages",
            "Assigned job:",
            "read_pdf",
            "Use spawn_agent to create helpers",
            "Your group",
        ] {
            assert!(!text.contains(absent), "{absent:?} in {text}");
        }
        let matching_agents = d.agents.iter().filter(|a| a.id != me.id);
        for other in matching_agents {
            assert!(
                !text.contains(other.job.trim()),
                "a neighbour's job: {}",
                other.id
            );
        }
        assert!(
            text.len() < me.job.len() + 600,
            "short: {} bytes",
            text.len()
        );
    }

    #[test]
    fn a_minimal_briefing_does_not_repeat_a_job_the_message_carries_and_keeps_the_data_contract() {
        let mut d = textbook();
        d.workflow = None;
        d.agents[1].advanced.data_access = Some(model::DataAccess {
            reads: vec!["notes/".into()],
            writes: vec!["out/".into()],
            side_effects: vec![],
            rationale: None,
            shared: vec![],
        });
        let me = &d.agents[1];
        let seat = d.seat_id(&me.id).unwrap();
        let text =
            crate::team_context::briefing_with(&d, &seat, &format!("Start: {}", me.job)).unwrap();
        assert!(
            !text.contains("Your job:")
                && text.contains("Your data contract: you read notes/; you write out/."),
            "{text}"
        );
        assert!(
            text.contains("When you are done, reply with your result."),
            "{text}"
        );
    }

    #[test]
    fn full_stays_the_default() {
        let mut d = textbook();
        for a in &mut d.agents {
            a.advanced.briefing = None;
        }
        let text = crate::team_context::briefing(&d, &d.seat_id("visualizer").unwrap()).unwrap();
        assert!(text.contains("Team architecture") && text.contains("dynamic workflow"));
        d.agents[0].advanced.briefing = Some(BriefingLevel::Full);
        assert!(
            crate::team_context::briefing(&d, &d.seat_id(&d.agents[0].id).unwrap())
                .unwrap()
                .contains("Team architecture")
        );
    }

    #[test]
    fn a_minimal_agents_helper_is_briefed_minimally() {
        let d = textbook();
        let parent = d.seat_id("visualizer").unwrap();
        let text = super::child(&d, &parent, "team.visualizer-helper-1@book").unwrap();
        assert!(
            text.contains("helper of Interaction builder [visualizer]")
                && !text.contains("Team architecture"),
            "{text}"
        );
    }
}
