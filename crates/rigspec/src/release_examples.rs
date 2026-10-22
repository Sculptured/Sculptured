//! Small original exercises illustrating the architectures in CS2680 Agent Design II (slides 9,13,16).
use model::{
    AgentAdvanced, AgentKind, ConnectionKind, DraftAgent, DraftConnection, ModelDefaults,
    PermissionPosture, TeamDraft,
};
pub fn template(id: &str, team: &str, name: &str, practice: bool) -> Option<TeamDraft> {
    let kind = if practice {
        AgentKind::Practice
    } else {
        AgentKind::Codex
    };
    let a = |id: &str, name: &str, job: &str, x, y| DraftAgent {
        id: id.into(),
        group_id: None,
        name: name.into(),
        kind,
        job: job.into(),
        folder: None,
        x,
        y,
        advanced: AgentAdvanced { data_access: None, authority: None, skills: vec![], integrations: vec![],
            permission: PermissionPosture::Restricted,
            ..AgentAdvanced::default()
        },
        connection: None,
    };
    let edge = |from: &str, to: &str| DraftConnection {
        id: format!("{from}-{to}"),
        from: from.into(),
        to: to.into(),
        kind: ConnectionKind::HandsWorkTo,
    };
    let (agents, connections) = match id {
        "support-triage" => (
            vec![a(
                "triage",
                "Support coordinator",
                "Demonstrate dynamic teamwork on this fictional ticket: 'I upgraded yesterday, was charged twice, and exports fail with error E42.' No real accounts, network requests or file changes. First use team_status. Decide whether specialists are needed. Use the managed spawn_agent tool to create at most two specialists: Billing analyst (examine fictional duplicate charges: two pending authorizations, one settled payment) and Log analyst (examine fictional log: E42 export denied because workspace role is viewer). Give each only its relevant facts, a clear question, output format (finding, evidence, confidence, next step), and instructions to send_agent_message back to your exact ID. Continue coordinating; do not conclude until both results arrive. Ask a specialist one clarification if needed. Combine a user-facing reply; distinguish fictional evidence from advice, and never claim a refund was made. Tell children to summarize and finish. Success: two justified findings, bounded specialists, evidence visible in the live graph. Avoid native or shell-spawned agents.",
                40,
                40,
            )],
            vec![],
        ),
        "essay-committee" => (
            vec![
                a(
                    "clarity",
                    "Clarity reviewer",
                    "Independently evaluate this fictional mini-essay: 'Cities should add trees because shade reduces heat. Trees cost money, so choose streets with little shade first.' Score clarity 1–5, quote one phrase as evidence, and give one actionable revision. Do not inspect the other review. Hand your rubric and evidence to the Editor. No files or network needed.",
                    40,
                    40,
                ),
                a(
                    "skeptic",
                    "Evidence reviewer",
                    "Independently evaluate this fictional mini-essay: 'Cities should add trees because shade reduces heat. Trees cost money, so choose streets with little shade first.' Score strength of evidence 1–5, identify one unsupported claim, and suggest one concrete measurement. Do not inspect the other review. Hand your rubric and evidence to the Editor. No files or network needed.",
                    40,
                    300,
                ),
                a(
                    "editor",
                    "Editor",
                    "Wait for both independent reviews. Compare their evidence, explain one agreement and one difference, then write an improved paragraph and a compact rubric. You are synthesizing opinions, not verifying empirical claims. If a review is incomplete, ask its author for the missing evidence through send_agent_message. Completion requires both reviews and an explicit limitation.",
                    340,
                    170,
                ),
            ],
            vec![edge("clarity", "editor"), edge("skeptic", "editor")],
        ),
        "design-review" => (
            vec![
                a(
                    "designer",
                    "Designer",
                    "Design a tiny habit tracker without writing code. Output a five-line specification covering add habit, mark today, undo, empty state and keyboard access. Hand it to Reviewer. If Reviewer sends feedback through send_agent_message, revise once and return the revised specification. No more than two revisions. Clearly label the final draft; do not claim implementation or testing.",
                    40,
                    40,
                ),
                a(
                    "reviewer",
                    "Reviewer",
                    "Review Designer's five-line habit tracker specification. Check that add, mark, undo, empty state and keyboard access are each unambiguous. If any item is missing, use team_status to find Designer, then send_agent_message with exact revision requests. Inspect the revised reply. Stop after two review rounds and report unresolved issues. Output a checklist with evidence from the specification. This is design review, not a verified implementation.",
                    340,
                    40,
                ),
            ],
            vec![edge("designer", "reviewer")],
        ),
        _ => return None,
    };
    Some(TeamDraft { claude_setup: Default::default(), protection: Default::default(), safety: vec![], components: vec![], budget: Default::default(), input: None, schedules: vec![], decision_log: false, limits: Default::default(),
        id: team.into(),
        name: name.into(),
        revision: 0,
        folder: None,
        workflow: None,
        models: ModelDefaults::default(),
        groups: vec![],
        agents,
        connections,
    })
}
