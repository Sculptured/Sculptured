use super::jobs::{Jobs, Kind};
use super::{engine::agent_for, running};
use crate::{state::AppState, store::Store};
use host_proto::HostCommand;
use model::{ASSEMBLY, CopiesMode, MessageIntent, MessageState, SeatLifecycle, StageKind, StepBy, TeamDraft, Waiting, WorkflowRun, WorkflowTask, TaskStatus};
use std::collections::HashSet;

pub async fn run(st: AppState) {
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        tick.tick().await;
        let rigs = st.store.lock().unwrap().list_rigs().unwrap_or_default();
        for rig in rigs {
            if let Err(e) = process(&st, &rig.name).await { tracing::warn!(rig = %rig.name, error = %e, "workflow dispatch failed"); }
            crate::schedules::tick(&st, &rig.name).await;
        }
    }
}

pub(super) async fn process(st: &AppState, rig: &str) -> Result<(), String> {
    let (sends, work, start) = {
        let mut store = st.store.lock().unwrap();
        let current = running(&store, rig).ok();
        st.workflow_jobs.stop_stale(&mut store, rig, current.as_ref().map(|(_, start)| *start));
        let Some((d, start)) = current else { return Ok(()) };
        // No batch yet: a dispatch command makes it (a dispatcher agent calls dispatch_work).
        let Some(mut run) = store.workflow_run(rig, start).map_err(|e| e.to_string())? else { super::dispatch::start(st, &store, &d, start); return Ok(()) };
        let (sends, mut work) = schedule(&mut store, &st.workflow_jobs, &d, &mut run).map_err(|e| e.to_string())?;
        work.extend(super::guard::due(st, &run, d.folder.as_deref().unwrap_or_default()).map(Work::Guard));
        for out in &sends { st.publish(out.items.clone()); }
        (sends, work, start)
    };
    for job in work {
        match job {
            Work::Check(j) => super::stage_checks::spawn(st.clone(), rig.to_string(), start, j),
            Work::Merge(j) => super::stage_merge::spawn(st.clone(), rig.to_string(), start, j),
            Work::Copy(j) => super::stage_copies::spawn(st.clone(), rig.to_string(), start, j),
            Work::Guard(j) => super::guard::spawn(st.clone(), rig.to_string(), start, j),
        }
    }
    for Outbound { host, seat, id, text, .. } in sends {
        // One attempt only. An ambiguous acknowledgement never causes an automatic duplicate.
        if !st.store.lock().unwrap().rig_snapshot(rig).map_err(|e| e.to_string())?.is_some_and(|s| s.desired == model::Desired::Running) { return Ok(()); }
        if let Err(reason) = st.hosts.command(&host, HostCommand::Send { seat: seat.clone(), message_id: id, intent: MessageIntent::Inform, text }).await {
            tracing::warn!(%seat, %reason, "workflow delivery unconfirmed; retained for operator inspection");
        }
    }
    Ok(())
}

/// copies: per-item — an item has no folder to work in until its copy is made.
/// Once merged, an item's later stages work in the project itself.
fn needs_copy(run: &WorkflowRun, t: &WorkflowTask) -> bool { run.config.copies == CopiesMode::PerItem && t.copy.is_none() && !merged(t) }

fn merged(t: &WorkflowTask) -> bool { t.history.iter().any(|h| h.by == StepBy::Sculptured && h.outcome == "merged") }

/// The name an item's copy and branch are kept under: unique per batch, since a later batch may reuse task ids.
pub(super) fn item_key(run: &WorkflowRun, task: &str) -> String { format!("{}-{task}", run.start_seq) }

/// At most this many copies of one team's items are made at once.
const COPIES_AT_ONCE: usize = 2;

/// An assignment message recorded in the store, to send to the agent's computer.
struct Outbound { host: String, seat: String, id: String, text: String, items: Vec<crate::views::StreamItem> }

fn schedule(store: &mut Store, jobs: &Jobs, d: &TeamDraft, run: &mut WorkflowRun) -> rusqlite::Result<(Vec<Outbound>, Vec<Work>)> {
    reconcile(store, d, run)?;
    // Items reaching a human stage wait for a person (POST …/workflow/decide).
    let mut parked = false;
    let gated: Vec<bool> = run.tasks.iter().map(|t| !super::topics::waiting(run, t).is_empty()).collect();
    for (t, _) in run.tasks.iter_mut().zip(&gated).filter(|(t, gated)| t.status == TaskStatus::Queued && !**gated) {
        if super::engine::stage(&run.config, &t.stage).is_some_and(|s| s.kind == StageKind::Human) {
            t.status = TaskStatus::Waiting; t.waiting = Some(Waiting { stage: t.stage.clone(), since_ms: model::now_ms() }); parked = true;
        }
    }
    if parked { store.save_workflow(run, None)?; }
    // A failing guard holds up new work; what is under way finishes.
    if super::guard::paused(run) { return Ok((vec![], vec![])); }
    let work = start_jobs(store, jobs, d, run, &gated)?;
    open_panels(store, run, &gated)?;
    let mut busy = busy_agents(run);
    let mut sends = Vec::new();
    let ready = run.tasks.iter().all(|t| t.status == TaskStatus::Complete);
    for index in 0..run.tasks.len() + usize::from(ready) {
        let t = task_at(run, index);
        if t.status != TaskStatus::Queued || !super::topics::waiting(run, t).is_empty() || needs_copy(run, t) { continue; }
        let Some(agent) = available_worker(store, d, run, t, &busy)? else { continue };
        if let Some(send) = assign(store, d, run, index, None, &agent)? { busy.insert(agent); sends.push(send); }
    }
    // Review panels: each reviewer gets the item as soon as they are free.
    for index in 0..run.tasks.len() {
        for r in 0..run.tasks[index].reviews.len() {
            let agent = run.tasks[index].reviews[r].agent.clone();
            if run.tasks[index].reviews[r].assignment.is_some() || busy.contains(&agent) || !idle(store, d, &agent)? { continue; }
            if let Some(send) = assign(store, d, run, index, Some(r), &agent)? { busy.insert(agent); sends.push(send); }
        }
    }
    Ok((sends, work))
}

/// Work Sculptured does itself, started once the store is unlocked.
pub(super) enum Work { Check(super::stage_checks::Job), Merge(super::stage_merge::Job), Copy(super::stage_copies::Job), Guard(super::guard::Job) }

/// Items at check and merge stages: claims a job for each (merges one per team at a time). An item
/// assigned with no job in this process lost it to a restart, so it runs again.
fn start_jobs(store: &mut Store, jobs: &Jobs, d: &TeamDraft, run: &mut WorkflowRun, gated: &[bool]) -> rusqlite::Result<Vec<Work>> {
    let (mut work, mut changed) = (Vec::new(), false);
    let (rig, start, per_item) = (run.rig.clone(), run.start_seq, run.config.copies == CopiesMode::PerItem);
    let mut merging = jobs.busy(&rig, Kind::Merge);
    let mut copying = jobs.count(&rig, Kind::Copy);
    // copies: per-item — make the copies of items about to be worked on, a few at a time.
    for (index, gated) in gated.iter().enumerate() {
        let t = &run.tasks[index];
        let worked = super::engine::stage(&run.config, &t.stage).is_some_and(|s| matches!(s.kind, StageKind::Agent | StageKind::Check));
        let id = format!("copy-{}", item_key(run, &t.input.id));
        if t.status != TaskStatus::Queued || *gated || !worked || !needs_copy(run, t) || jobs.running(&id) || copying >= COPIES_AT_ONCE { continue; }
        jobs.claim(&id, &rig, start, Kind::Copy);
        copying += 1;
        work.push(Work::Copy(super::stage_copies::Job { id, project: d.folder.clone().unwrap_or_default(), task: t.input.id.clone(), key: item_key(run, &t.input.id) }));
    }
    for (t, gated) in run.tasks.iter_mut().zip(gated) {
        let Some(s) = super::engine::stage(&run.config, &t.stage).filter(|s| matches!(s.kind, StageKind::Check | StageKind::Merge)) else { continue };
        if t.status == TaskStatus::Assigned && t.assignment.as_deref().is_some_and(|id| !jobs.running(id)) { t.status = TaskStatus::Queued; changed = true; }
        if t.status != TaskStatus::Queued || *gated || (per_item && s.kind == StageKind::Check && t.copy.is_none() && !merged(t)) { continue; }
        if s.kind == StageKind::Merge && merging { continue; }
        let id = uuid::Uuid::new_v4().to_string();
        let (project, key, task) = (d.folder.clone().unwrap_or_default(), format!("{start}-{}", t.input.id), t.input.id.clone());
        work.push(if s.kind == StageKind::Merge {
            merging = true;
            jobs.claim(&id, &rig, start, Kind::Merge);
            Work::Merge(super::stage_merge::Job { assignment: id.clone(), project, task, key, into: s.into.clone() })
        } else {
            jobs.claim(&id, &rig, start, Kind::Check);
            let item = t.copy.as_ref().map(|_| localops::ItemRef { team: rig.clone(), task: key });
            Work::Check(super::stage_checks::Job { assignment: id.clone(), command: s.command.clone().unwrap_or_default(), cwd: project, item, timeout_s: s.timeout_s.unwrap_or(model::DEFAULT_CHECK_TIMEOUT_S) })
        });
        t.status = TaskStatus::Assigned; t.assignment = Some(id); t.assigned_agent = None;
        changed = true;
    }
    if changed { store.save_workflow(run, None)?; }
    Ok(work)
}

/// Item `index`, or the assembly item just past the end.
fn task_at(run: &WorkflowRun, index: usize) -> &WorkflowTask { run.tasks.get(index).unwrap_or(&run.assembly) }

fn task_at_mut(run: &mut WorkflowRun, index: usize) -> &mut WorkflowTask {
    if index < run.tasks.len() { &mut run.tasks[index] } else { &mut run.assembly }
}

/// Agents with work in hand: an assigned item's agent, and reviewers who haven't voted yet.
fn busy_agents(run: &WorkflowRun) -> HashSet<String> {
    let all = || run.all_tasks();
    let agents = all().filter(|t| t.status == TaskStatus::Assigned).filter_map(|t| agent_for(&run.config, t).map(str::to_string));
    let reviewers = all().flat_map(|t| &t.reviews).filter(|r| r.assignment.is_some() && r.outcome.is_none()).map(|r| r.agent.clone());
    agents.chain(reviewers).collect()
}

/// Items reaching a review panel: one review per reviewer; the item counts as assigned from now on.
fn open_panels(store: &mut Store, run: &mut WorkflowRun, gated: &[bool]) -> rusqlite::Result<()> {
    let per_item = run.config.copies == CopiesMode::PerItem;
    let mut opened = false;
    for (t, gated) in run.tasks.iter_mut().zip(gated) {
        let Some(s) = super::engine::stage(&run.config, &t.stage).filter(|s| !s.reviewers.is_empty()) else { continue };
        if t.status != TaskStatus::Queued || *gated || (per_item && t.copy.is_none() && !merged(t)) { continue; }
        // A retried panel keeps the votes it has; a new one starts with every reviewer to ask.
        if t.reviews.is_empty() { t.reviews = s.reviewers.iter().map(|agent| model::Review { agent: agent.clone(), ..Default::default() }).collect(); }
        t.status = TaskStatus::Assigned; t.assignment = None; t.assigned_agent = None;
        if let Err(e) = super::engine::close_panel(&run.config, t) { tracing::warn!(%e, "cannot close a review panel"); }
        opened = true;
    }
    if opened { store.save_workflow(run, None)?; }
    Ok(())
}

/// Sends `agent` item `index` (as reviewer `review` of its panel, when given), recorded with the message.
fn assign(store: &mut Store, d: &TeamDraft, run: &mut WorkflowRun, index: usize, review: Option<usize>, agent: &str) -> rusqlite::Result<Option<Outbound>> {
    let Some(seat) = d.seat_id(agent) else { return Ok(None) };
    let Some(view) = store.seat(&seat)? else { return Ok(None) };
    let id = uuid::Uuid::new_v4().to_string();
    let escalated = review.is_none() && super::engine::escalation_due(&run.config, task_at(run, index)).is_some_and(|e| e.agent == agent);
    let text = super::assignment::text(d, run, task_at(run, index), &id, agent);
    let (text, mut notes) = crate::team_context::held_notes_in(store, &seat, text);
    let text = match crate::team_context::decision_log_note(store, &run.rig, d.decision_log) { Some(note) => format!("{note}\n\n{text}"), None => text };
    let fresh = !crate::team_context::briefed(store, &seat).unwrap_or(false);
    let text = match crate::team_context::briefing_with(d, &seat, &text).filter(|_| fresh) { Some(brief) => format!("{brief}\n\n--- Current message ---\n{text}"), None => text };
    let t = task_at_mut(run, index);
    match review {
        Some(r) => t.reviews[r].assignment = Some(id.clone()),
        None => {
            t.escalated = escalated;
            t.status = TaskStatus::Assigned; t.assignment = Some(id.clone()); t.assigned_agent = Some(agent.to_string());
        }
    }
    let mut items = store.save_workflow(run, Some((&seat, &id, &text)))?;
    items.append(&mut notes);
    Ok(Some(Outbound { host: view.host.unwrap_or_else(|| "local".into()), seat, id, text, items }))
}

/// Whether `agent` can take work now: running, not in a turn, nothing queued for it, not under manual control.
fn idle(store: &Store, d: &TeamDraft, agent: &str) -> rusqlite::Result<bool> {
    let Some(seat) = d.seat_id(agent) else { return Ok(false) };
    if store.control(&seat)?.is_some() { return Ok(false); }
    Ok(store.seat(&seat)?.is_some_and(|s| s.lifecycle == SeatLifecycle::Running && s.current_turn.is_none()) && !store.has_pending_messages(&seat)?)
}

fn available_worker(store: &Store, d: &TeamDraft, run: &WorkflowRun, t: &WorkflowTask, busy: &HashSet<String>) -> rusqlite::Result<Option<String>> {
    // Past its rounds, an item goes only to the stage's stronger agent, and waits for it.
    let escalated = super::engine::escalation_due(&run.config, t);
    let candidates = if let Some(e) = escalated { vec![&e.agent] } else if t.stage == ASSEMBLY { vec![&run.config.collector] } else {
        super::engine::stage(&run.config, &t.stage).filter(|s| s.kind == StageKind::Agent && s.reviewers.is_empty()).map(|s| std::iter::once(&s.agent).chain(s.workers.iter()).collect()).unwrap_or_default()
    };
    for agent in candidates {
        if !busy.contains(agent) && idle(store, d, agent)? { return Ok(Some(agent.clone())); }
    }
    Ok(None)
}


fn reconcile(store: &mut Store, d: &TeamDraft, run: &mut WorkflowRun) -> rusqlite::Result<()> {
    let mut changed = false;
    // Split borrows: the loop reads run.config while changing tasks.
    for t in run.tasks.iter_mut().chain(std::iter::once(&mut run.assembly)).filter(|t| t.status == TaskStatus::Assigned) {
        // Check stages have no agent; their jobs settle themselves (stage_checks).
        if agent_for(&run.config, t).is_none() { continue; }
        let Some(id) = &t.assignment else { continue };
        let state = store.message_state(id)?;
        let seat = agent_for(&run.config, t).and_then(|a| d.seat_id(a));
        let ended = seat.as_ref().map(|s| assignment_ended(store, s, id)).transpose()?.unwrap_or(false);
        if state == Some(MessageState::Failed) || ended {
            t.status = TaskStatus::Blocked;
            t.problem = Some("Agent ended without complete_work, or delivery failed. Inspect its transcript before retrying.".into());
            changed = true;
        }
    }
    for t in run.tasks.iter_mut().filter(|t| t.status == TaskStatus::Assigned) {
        let mut silent = None;
        for r in t.reviews.iter().filter(|r| r.outcome.is_none()) {
            let (Some(id), Some(seat)) = (&r.assignment, d.seat_id(&r.agent)) else { continue };
            if store.message_state(id)? == Some(MessageState::Failed) || assignment_ended(store, &seat, id)? { silent = Some(r.agent.clone()); break; }
        }
        if let Some(agent) = silent {
            t.status = TaskStatus::Blocked;
            t.problem = Some(format!("{agent} ended without a vote, or delivery failed. Inspect its transcript before retrying."));
            changed = true;
        }
    }
    if changed { store.save_workflow(run, None)?; }
    Ok(())
}

fn assignment_ended(store: &Store, seat: &str, id: &str) -> rusqlite::Result<bool> {
    store.connection().query_row("SELECT EXISTS(SELECT 1 FROM events e JOIN messages m ON m.id=?1 WHERE e.subject=?2 AND json_extract(e.json,'$.body.type')='turn-ended' AND json_extract(e.json,'$.body.turn_id')=m.turn_id)",
        rusqlite::params![id, model::Subject::Seat(seat.into()).key()], |r| r.get(0))
}

