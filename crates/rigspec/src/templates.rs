//! Built-in starting points for "Start a team". Positions use the 200×200 card size.
use model::{
    AgentAdvanced, AgentKind, ConnectionKind, DraftAgent, DraftConnection, ModelDefaults, TeamDraft,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const TEMPLATE_IDS: [&str; 8] = [
    "solo",
    "builder-reviewer",
    "planner-builders",
    "blank",
    "textbook",
    "support-triage",
    "essay-committee",
    "design-review",
];

const BUILDER_JOB: &str = "Build the change you are given, test it, then hand it to Reviewer with a short summary of what changed.";
const REVIEWER_JOB: &str =
    "Review what Builder hands you. Report problems plainly and say what you checked.";
const PLANNER_JOB: &str =
    "Break the goal into two independent pieces of work and hand one to each builder.";
const BUILDER_N_JOB: &str =
    "Build the piece of work the Planner hands you, test it, and report what you did.";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TemplateInfo {
    pub id: String,
    pub title: String,
    pub summary: String,
    /// Team name used when the user does not type one.
    pub default_name: String,
    /// The graph, for the gallery's drawing (non-Practice kinds).
    pub preview: TeamDraft,
}

fn agent(id: &str, name: &str, kind: AgentKind, job: &str, x: i32, y: i32) -> DraftAgent {
    DraftAgent {
        id: id.into(),
        group_id: None,
        name: name.into(),
        kind,
        job: job.into(),
        folder: None,
        x,
        y,
        advanced: AgentAdvanced::default(),
        connection: None,
    }
}

fn hands(from: &str, to: &str) -> DraftConnection {
    DraftConnection {
        id: format!("c-{from}-{to}"),
        from: from.into(),
        to: to.into(),
        kind: ConnectionKind::HandsWorkTo,
    }
}

/// A fresh draft for template `id`, or None if there is no such template.
/// `practice` swaps every agent to a Practice agent.
#[must_use]
pub fn template(id: &str, team_id: &str, name: &str, practice: bool) -> Option<TeamDraft> {
    if id == "textbook" {
        return Some(crate::textbook::template(team_id, name, practice));
    }
    if let Some(d) = crate::release_examples::template(id, team_id, name, practice) {
        return Some(d);
    }
    let k = |kind: AgentKind| if practice { AgentKind::Practice } else { kind };
    let (agents, connections) = match id {
        "solo" => (
            vec![agent("claude", "Claude", k(AgentKind::Claude), "", 40, 40)],
            vec![],
        ),
        "builder-reviewer" => (
            vec![
                agent(
                    "builder",
                    "Builder",
                    k(AgentKind::Claude),
                    BUILDER_JOB,
                    40,
                    40,
                ),
                agent(
                    "reviewer",
                    "Reviewer",
                    k(AgentKind::Codex),
                    REVIEWER_JOB,
                    300,
                    40,
                ),
            ],
            vec![hands("builder", "reviewer")],
        ),
        "planner-builders" => (
            vec![
                agent(
                    "planner",
                    "Planner",
                    k(AgentKind::Claude),
                    PLANNER_JOB,
                    40,
                    160,
                ),
                agent(
                    "builder-1",
                    "Builder 1",
                    k(AgentKind::Claude),
                    BUILDER_N_JOB,
                    300,
                    40,
                ),
                agent(
                    "builder-2",
                    "Builder 2",
                    k(AgentKind::Claude),
                    BUILDER_N_JOB,
                    300,
                    280,
                ),
            ],
            vec![hands("planner", "builder-1"), hands("planner", "builder-2")],
        ),
        "blank" => (vec![], vec![]),
        _ => return None,
    };
    Some(TeamDraft {
        claude_setup: Default::default(),
        protection: Default::default(),
        safety: vec![],
        components: vec![],
        budget: Default::default(),
        input: None,
        schedules: vec![],
        mandate: Default::default(),
        triggers: Vec::new(),
        decision_log: false,
        limits: Default::default(),
        workflow: None,
        folder: None,
        id: team_id.into(),
        name: name.into(),
        groups: vec![],
        agents,
        connections,
        models: ModelDefaults::default(),
        revision: 0,
    })
}

#[must_use]
pub fn templates() -> Vec<TemplateInfo> {
    let info = |id: &str, title: &str, summary: &str, default_name: &str| TemplateInfo {
        id: id.into(),
        title: title.into(),
        summary: summary.into(),
        default_name: default_name.into(),
        preview: template(id, "preview", default_name, false).expect("listed template exists"),
    };
    vec![
        info("solo", "Solo agent", "One Claude agent.", "Solo agent"),
        info(
            "builder-reviewer",
            "Builder → Reviewer",
            "Claude builds, then hands the work to Codex to review.",
            "Builder and Reviewer",
        ),
        info(
            "planner-builders",
            "Planner → 2 Builders",
            "One planner hands work to two builders.",
            "Planner and builders",
        ),
        info(
            "blank",
            "Blank canvas",
            "Start empty and drag agents in.",
            "New team",
        ),
        info(
            "textbook",
            "Textbook creator",
            "Input → dynamic interaction batches → mathematical and UI review loops → textbook assembly.",
            "Textbook creator",
        ),
        info(
            "support-triage",
            "Adaptive support team",
            "A coordinator creates billing and log specialists only when needed. CS2680 dynamic topology.",
            "Adaptive support team",
        ),
        info(
            "essay-committee",
            "Independent reviewers",
            "Two independent perspectives, then an editor synthesizes the evidence. CS2680 committee.",
            "Independent reviewers",
        ),
        info(
            "design-review",
            "Design and revise",
            "A designer and reviewer exchange bounded feedback. CS2680 pipeline with a retry edge.",
            "Design and revise",
        ),
    ]
}
