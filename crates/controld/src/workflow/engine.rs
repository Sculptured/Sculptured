//! Pure transitions; tool identity and live-team checks happen at the boundary.
use model::{
    ASSEMBLY, DispatchTask, StageKind, StepBy, TaskStatus, Workflow, WorkflowRun, WorkflowStage,
    WorkflowStep, WorkflowTask,
};

pub fn task(input: DispatchTask, stage: &str) -> WorkflowTask {
    WorkflowTask {
        input,
        stage: stage.into(),
        status: TaskStatus::Queued,
        assignment: None,
        assigned_agent: None,
        history: vec![],
        problem: None,
        waiting: None,
        copy: None,
        reviews: vec![],
        escalated: false,
    }
}

pub fn new_run(
    rig: &str,
    start_seq: i64,
    batch_id: String,
    summary: String,
    config: Workflow,
    tasks: Vec<DispatchTask>,
) -> Result<WorkflowRun, String> {
    if tasks.is_empty() || tasks.len() > config.max_tasks as usize {
        return Err(format!("Dispatch 1–{} tasks.", config.max_tasks));
    }
    let mut ids = std::collections::HashSet::new();
    for t in &tasks {
        if !model::valid_segment(&t.id)
            || t.id == ASSEMBLY
            || !ids.insert(&t.id)
            || t.title.trim().is_empty()
            || t.title.len() > 200
            || t.context.trim().is_empty()
            || t.context.len() > 12000
        {
            return Err(
                "Tasks need unique simple IDs, a title (1–200 bytes), and context (1–12000 bytes)."
                    .into(),
            );
        }
    }
    super::topics::validate(&tasks)?;
    let tasks = tasks.into_iter().map(|t| task(t, &config.entry)).collect();
    let assembly = task(
        DispatchTask {
            id: ASSEMBLY.into(),
            title: "Assemble accepted results".into(),
            context: summary.clone(),
            mode: Default::default(),
            depends_on: vec![],
        },
        ASSEMBLY,
    );
    Ok(WorkflowRun {
        rig: rig.into(),
        start_seq,
        batch_id,
        summary,
        config,
        tasks,
        assembly,
        guard: None,
    })
}

pub fn stage<'a>(w: &'a Workflow, id: &str) -> Option<&'a WorkflowStage> {
    w.stages.iter().find(|s| s.id == id)
}

/// The stage's escalation, when this item has already been through its `after_rounds` rounds here.
pub fn escalation_due<'a>(w: &'a Workflow, t: &WorkflowTask) -> Option<&'a model::Escalation> {
    stage(w, &t.stage)?.escalate.as_ref().filter(|e| {
        t.history.iter().filter(|h| h.stage == t.stage).count() >= e.after_rounds as usize
    })
}

/// The agent working on (or due to work on) an item; None at check and human stages.
pub fn agent_for<'a>(w: &'a Workflow, task: &'a WorkflowTask) -> Option<&'a str> {
    if let Some(agent) = &task.assigned_agent {
        return Some(agent);
    }
    if task.stage == ASSEMBLY {
        Some(&w.collector)
    } else {
        stage(w, &task.stage)
            .filter(|s| s.kind == StageKind::Agent && s.reviewers.is_empty())
            .map(|s| s.agent.as_str())
    }
}

pub fn complete(
    run: &mut WorkflowRun,
    agent: &str,
    assignment: &str,
    outcome: &str,
    result: &str,
) -> Result<(), String> {
    if result.trim().is_empty() || result.len() > 12000 {
        return Err("Provide a result of 1–12000 bytes, with artifact paths and evidence.".into());
    }
    if let Some(i) = run.tasks.iter().position(|t| {
        t.reviews
            .iter()
            .any(|r| r.assignment.as_deref() == Some(assignment))
    }) {
        return vote(run, i, agent, assignment, outcome, result);
    }
    if let Some(step) = run
        .tasks
        .iter_mut()
        .flat_map(|t| t.history.iter_mut())
        .find(|h| {
            h.panel
                .iter()
                .any(|r| r.assignment.as_deref() == Some(assignment))
        })
    {
        return late_vote(step, agent, assignment, outcome);
    }
    let task = run
        .tasks
        .iter_mut()
        .chain(std::iter::once(&mut run.assembly))
        .find(|t| {
            t.assignment.as_deref() == Some(assignment)
                || t.history.iter().any(|h| h.assignment == assignment)
        })
        .ok_or("Unknown or stale assignment.")?;
    if let Some(h) = task.history.iter().find(|h| h.assignment == assignment) {
        return if h.agent == agent && h.outcome == outcome && h.result == result {
            Ok(())
        } else {
            Err("This assignment already has a different result.".into())
        };
    }
    if task.status != TaskStatus::Assigned || agent_for(&run.config, task) != Some(agent) {
        return Err("This assignment is not assigned to you.".into());
    }
    let mut step = WorkflowStep {
        assignment: assignment.into(),
        stage: task.stage.clone(),
        agent: agent.into(),
        outcome: outcome.into(),
        result: result.into(),
        at_ms: model::now_ms(),
        ..Default::default()
    };
    if task.escalated {
        let rounds = stage(&run.config, &task.stage)
            .and_then(|s| s.escalate.as_ref())
            .map_or(0, |e| e.after_rounds);
        step.evidence = Some(format!(
            "This item was escalated after {rounds} rounds here, to {agent}."
        ));
    }
    advance(&run.config, task, step)
}

/// One reviewer's vote on a panel item; the stage completes once its join rule is met.
fn vote(
    run: &mut WorkflowRun,
    index: usize,
    agent: &str,
    assignment: &str,
    outcome: &str,
    result: &str,
) -> Result<(), String> {
    let (w, t) = (&run.config, &mut run.tasks[index]);
    let s = stage(w, &t.stage).ok_or("Stage not found.")?;
    if outcome != "blocked" && !s.routes.contains_key(outcome) {
        return Err("Choose a configured outcome, or blocked.".into());
    }
    let r = t
        .reviews
        .iter_mut()
        .find(|r| r.assignment.as_deref() == Some(assignment))
        .ok_or("Unknown or stale assignment.")?;
    if r.agent != agent {
        return Err("This assignment is not assigned to you.".into());
    }
    if let Some(given) = &r.outcome {
        return if given == outcome && r.result.as_deref() == Some(result) {
            Ok(())
        } else {
            Err("You have already voted on this item.".into())
        };
    }
    r.outcome = Some(outcome.into());
    r.result = Some(result.into());
    // A blocked item keeps the vote for its retry, but nothing moves it until someone looks.
    if t.status != TaskStatus::Assigned {
        return Ok(());
    }
    close_panel(w, t)
}

/// Completes a panel item once its join rule is met; otherwise leaves it waiting for more votes.
pub fn close_panel(w: &Workflow, t: &mut WorkflowTask) -> Result<(), String> {
    let s = stage(w, &t.stage).ok_or("Stage not found.")?;
    let Some(decided) = panel_outcome(s, &t.reviews) else {
        return Ok(());
    };
    let votes = t
        .reviews
        .iter()
        .filter_map(|r| Some((r.agent.clone(), r.outcome.clone()?)))
        .collect();
    let result = t
        .reviews
        .iter()
        .filter_map(|r| Some(format!("— {}: {}", r.agent, r.result.as_ref()?)))
        .collect::<Vec<_>>()
        .join("\n");
    let result = match split_note(s, &t.reviews) {
        Some(note) => format!("{note}\n{result}"),
        None => result,
    };
    let step = WorkflowStep {
        assignment: uuid::Uuid::new_v4().to_string(),
        stage: t.stage.clone(),
        agent: "panel".into(),
        outcome: decided,
        result,
        at_ms: model::now_ms(),
        votes,
        panel: std::mem::take(&mut t.reviews),
        ..Default::default()
    };
    advance(w, t, step)
}

/// When reviewers voted differently: who voted what (and from which angle), since a split shows risks
/// one reviewer saw and another did not. Derived from the votes alone (an idea from Advocate).
pub fn split_note(s: &WorkflowStage, reviews: &[model::Review]) -> Option<String> {
    let mut by: std::collections::BTreeMap<&str, Vec<String>> = std::collections::BTreeMap::new();
    for r in reviews {
        let who = s.personas.get(&r.agent).map_or(r.agent.clone(), |p| {
            format!("{} ({})", r.agent, p.label().to_lowercase())
        });
        by.entry(r.outcome.as_deref()?).or_default().push(who);
    }
    (by.len() > 1).then(|| {
        format!(
            "Reviewers disagree: {}. Read why before acting on the outcome.",
            by.iter()
                .map(|(v, who)| format!("{} voted {v}", who.join(", ")))
                .collect::<Vec<_>>()
                .join("; ")
        )
    })
}

/// A reviewer answering after its panel already decided: recorded on that step, never acted on.
fn late_vote(
    step: &mut WorkflowStep,
    agent: &str,
    assignment: &str,
    outcome: &str,
) -> Result<(), String> {
    let r = step
        .panel
        .iter()
        .find(|r| r.assignment.as_deref() == Some(assignment))
        .ok_or("Unknown or stale assignment.")?;
    if r.agent != agent {
        return Err("This assignment is not assigned to you.".into());
    }
    if r.outcome.is_some() {
        return Err("You have already voted on this item.".into());
    }
    let key = format!("{agent} (late)");
    match step.votes.get(&key) {
        Some(given) if given != outcome => Err("You have already voted on this item.".into()),
        _ => {
            step.votes.insert(key, outcome.into());
            Ok(())
        }
    }
}

/// The panel's outcome once its join rule is met: `any` takes the first approval; otherwise, once every
/// reviewer has voted, the worst vote (by `priority`, else blocked, then the first non-approval).
pub fn panel_outcome(s: &WorkflowStage, reviews: &[model::Review]) -> Option<String> {
    let votes: Vec<&str> = reviews
        .iter()
        .filter_map(|r| r.outcome.as_deref())
        .collect();
    if s.join == model::Join::Any && votes.contains(&"approved") {
        return Some("approved".into());
    }
    if votes.len() < reviews.len() {
        return None;
    }
    let worst = if s.priority.is_empty() {
        if votes.contains(&"blocked") {
            "blocked"
        } else {
            votes
                .iter()
                .copied()
                .find(|v| *v != "approved")
                .unwrap_or("approved")
        }
    } else {
        votes
            .iter()
            .copied()
            .min_by_key(|v| s.priority.iter().position(|p| p == v).unwrap_or(usize::MAX))
            .unwrap_or("approved")
    };
    Some(worst.to_string())
}

/// Sculptured's own result for a running assignment (a check stage).
pub fn settle(
    run: &mut WorkflowRun,
    assignment: &str,
    outcome: &str,
    result: &str,
    by: StepBy,
    evidence: Option<String>,
) -> Result<(), String> {
    let task = run
        .tasks
        .iter_mut()
        .find(|t| t.assignment.as_deref() == Some(assignment) && t.status == TaskStatus::Assigned)
        .ok_or("Unknown or stale assignment.")?;
    let step = WorkflowStep {
        assignment: assignment.into(),
        stage: task.stage.clone(),
        agent: String::new(),
        outcome: outcome.into(),
        result: result.into(),
        at_ms: model::now_ms(),
        by,
        evidence,
        ..Default::default()
    };
    advance(&run.config, task, step)
}

/// Sculptured blocks an item it couldn't go on with (its copy couldn't be made, say), with the reason.
pub fn block(run: &mut WorkflowRun, task: &str, why: &str) -> Result<(), String> {
    let t = run
        .tasks
        .iter_mut()
        .find(|t| t.input.id == task)
        .ok_or("Item not found.")?;
    let step = WorkflowStep {
        assignment: uuid::Uuid::new_v4().to_string(),
        stage: t.stage.clone(),
        outcome: "blocked".into(),
        result: why.into(),
        at_ms: model::now_ms(),
        by: StepBy::Sculptured,
        ..Default::default()
    };
    advance(&run.config, t, step)
}

/// A person's decision on an item waiting at a human stage.
pub fn decide(run: &mut WorkflowRun, task: &str, outcome: &str, note: &str) -> Result<(), String> {
    if note.len() > 4096 {
        return Err("Keep the note under 4 KB.".into());
    }
    let (w, tasks) = (&run.config, &mut run.tasks);
    let t = tasks
        .iter_mut()
        .find(|t| t.input.id == task)
        .ok_or("Item not found.")?;
    let at = t
        .waiting
        .as_ref()
        .filter(|_| t.status == TaskStatus::Waiting)
        .map(|x| x.stage.clone())
        .ok_or("This item is not waiting for a decision.")?;
    if !stage(w, &at).is_some_and(|s| s.routes.contains_key(outcome)) {
        return Err("Choose one of this stage's outcomes.".into());
    }
    let result = if note.trim().is_empty() {
        format!("Decided: {outcome}")
    } else {
        note.trim().to_string()
    };
    let step = WorkflowStep {
        assignment: uuid::Uuid::new_v4().to_string(),
        stage: at,
        agent: "person".into(),
        outcome: outcome.into(),
        result,
        at_ms: model::now_ms(),
        by: StepBy::Person,
        ..Default::default()
    };
    advance(w, t, step)
}

/// Every outcome goes through here: routing, round limits and the step record.
pub fn advance(
    w: &Workflow,
    task: &mut WorkflowTask,
    mut step: WorkflowStep,
) -> Result<(), String> {
    let mut next = route(w, task, &step.outcome)?;
    // A stage over its round limit is refused; its on_limit stage is taken once, if that one isn't spent too.
    // When both are spent the item parks at the refused stage, where Retry grants it one more round.
    let spent = |id: &str| {
        stage(w, id).and_then(|s| {
            s.max_rounds
                .filter(|&max| {
                    task.history.iter().filter(|h| h.stage == s.id).count()
                        + usize::from(step.stage == s.id)
                        >= max as usize
                })
                .map(|max| format!("Round limit ({max}) reached for {}.", s.name))
        })
    };
    let mut limited: Option<(String, String)> = None;
    if let Some(id) = next.clone()
        && let Some(note) = spent(&id)
    {
        next = stage(w, &id).and_then(|s| s.on_limit.clone());
        limited = Some((note, id));
        if let Some(alt) = next.clone()
            && let Some(again) = spent(&alt)
        {
            limited = limited.map(|(n, refused)| (format!("{n} {again}"), refused));
            next = None;
        }
    }
    if let Some((note, _)) = &limited {
        step.evidence = Some(match step.evidence.take() {
            Some(e) => format!("{e}\n{note}"),
            None => note.clone(),
        });
    }
    let (outcome, result) = (step.outcome.clone(), step.result.clone());
    task.history.push(step);
    task.assignment = None;
    task.assigned_agent = None;
    task.problem = None;
    task.waiting = None;
    task.escalated = false;
    if outcome == "blocked" {
        task.status = TaskStatus::Blocked;
        task.problem = Some(result);
    } else if let (Some((note, refused)), None) = (&limited, &next) {
        // Retrying grants one more round at the refused stage.
        task.stage = refused.clone();
        task.status = TaskStatus::Blocked;
        task.problem = Some(format!("{note} Retry to allow one more round."));
    } else if let Some(next) = next {
        task.stage = next;
        task.status = TaskStatus::Queued;
        if task.history.len() >= w.max_steps as usize {
            task.status = TaskStatus::Blocked;
            task.problem = Some("Step limit reached; needs operator review.".into());
        }
    } else {
        task.status = TaskStatus::Complete;
    }
    Ok(())
}

fn route(w: &Workflow, task: &WorkflowTask, outcome: &str) -> Result<Option<String>, String> {
    if outcome == "blocked" || (task.stage == ASSEMBLY && outcome == "done") {
        return Ok(None);
    }
    stage(w, &task.stage)
        .and_then(|s| s.routes.get(outcome))
        .cloned()
        .ok_or("Choose a configured outcome, or blocked.".into())
}
