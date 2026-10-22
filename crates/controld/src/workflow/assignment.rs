//! The message an agent gets with each workflow assignment. Every prior step says who settled it, so an
//! agent's report is never presented as verification.
use model::{ASSEMBLY, StepBy, TeamDraft, WorkflowRun, WorkflowStep, WorkflowTask};
use serde_json::json;

/// How much of a step's evidence an assignment repeats (the end, where a check's verdict is).
const EVIDENCE_IN_MESSAGE: usize = 1500;

pub fn text(
    d: &TeamDraft,
    run: &WorkflowRun,
    task: &WorkflowTask,
    id: &str,
    agent: &str,
) -> String {
    let job = d.agent(agent).map(|a| a.job.as_str()).unwrap_or_default();
    let (folder, panel) = (folder_note(task), panel_note(run, task, agent));
    let panel = match super::engine::escalation_due(&run.config, task).filter(|e| e.agent == agent)
    {
        Some(e) => format!(
            "{panel}\nEscalated: this item has been through {} rounds at this stage; you are taking over.",
            e.after_rounds
        ),
        None => panel,
    };
    let history = format!(
        "{}\nPrerequisite topic results: {}",
        history_json(task),
        super::topics::reports(run, task)
    );
    [
        format!("Dynamic workflow — {}", d.name),
        format!("Batch: {}", run.batch_id),
        format!("Assignment: {id}"),
        format!("Stage: {}", task.stage),
        format!("Task: {} — {}", task.input.id, task.input.title),
        format!("Your role: {job}"),
        format!("Shared brief: {}", run.summary),
        format!("Task context: {}{folder}{panel}", task.input.context),
        format!("Prior steps, newest first (\"by\" says who settled each: agent-reported steps are not independent verification): {history}"),
        format!("Accepted items for assembly: {}", accepted_json(run, task)),
        String::new(),
        format!("Work only on this assignment; shared sessions also handle other tasks, so use its task ID and context. Finish by calling complete_work with assignment={id:?}, outcome (one of {}), and result containing artifact paths, source references, checks actually run and remaining limitations. Plain replies do not advance the workflow. Review findings must name the artifact revision. Do not spawn replacement agents or call dispatch_work. Use request_input if blocked; never claim visual checks you did not run.", outcomes(run, task)),
    ]
    .join("\n")
}

/// The outcomes the agent may report: the stage's routes, and always `blocked`.
fn outcomes(run: &WorkflowRun, task: &WorkflowTask) -> String {
    if task.stage == ASSEMBLY {
        return "done, blocked".into();
    }
    let routes = crate::workflow::engine::stage(&run.config, &task.stage)
        .map(|s| s.routes.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    routes
        .into_iter()
        .chain(["blocked".to_string()])
        .collect::<Vec<_>>()
        .join(", ")
}

fn by(step: &WorkflowStep) -> &'static str {
    match step.by {
        StepBy::Agent => "agent-reported",
        StepBy::Check => "verified: Sculptured ran the check",
        StepBy::Person => "a person's decision",
        StepBy::Sculptured => "Sculptured",
    }
}

/// The last six steps, newest first, each labelled by who settled it.
fn history_json(task: &WorkflowTask) -> String {
    let steps: Vec<_> = task.history.iter().rev().take(6).map(|h| {
        let evidence = h.evidence.as_deref().map(|e| if e.len() > EVIDENCE_IN_MESSAGE { format!("…{}", model::cap::tail(e, EVIDENCE_IN_MESSAGE)) } else { e.to_string() });
        json!({"stage": h.stage, "agent": h.agent, "outcome": h.outcome, "by": by(h), "result": h.result, "evidence": evidence})
    }).collect();
    serde_json::Value::from(steps).to_string()
}

/// For the assembly: every item's accepted report.
fn accepted_json(run: &WorkflowRun, task: &WorkflowTask) -> String {
    if task.stage != ASSEMBLY {
        return String::new();
    }
    let items: Vec<_> = run.tasks.iter().map(|t| json!({"id": t.input.id, "title": t.input.title, "accepted": t.history.last(), "review_steps": t.history.len()})).collect();
    serde_json::Value::from(items).to_string()
}

fn folder_note(task: &WorkflowTask) -> String {
    task.copy
        .as_ref()
        .map(|c| format!("\nItem folder: {c} — work only there; it is this item's own branch."))
        .unwrap_or_default()
}

fn panel_note(run: &WorkflowRun, task: &WorkflowTask, agent: &str) -> String {
    crate::workflow::engine::stage(&run.config, &task.stage)
        .filter(|s| !s.reviewers.is_empty())
        .map(|s| {
            format!(
                "\nYou are one of {} reviewers of this item; give your own vote.{}",
                s.reviewers.len(),
                persona_note(s, agent)
            )
        })
        .unwrap_or_default()
}

/// The angle this reviewer was given, if any; the other reviewers cover the rest.
fn persona_note(s: &model::WorkflowStage, agent: &str) -> String {
    s.personas
        .get(agent)
        .map(|p| {
            format!(
                " Review as the {}: {} Leave other angles to the other reviewers.",
                p.label().to_lowercase(),
                p.brief()
            )
        })
        .unwrap_or_default()
}
