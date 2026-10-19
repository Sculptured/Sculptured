//! Operator-authored architecture, delivered with work to every team member.
use crate::state::AppState;
use model::{ConnectionKind, DraftAgent, TeamDraft, Workflow};

mod minimal;

/// `text` with what the agent needs around it; the team briefing goes with a conversation's first
/// message only, since the agent keeps it for the whole conversation.
pub(crate) fn with_context(st: &AppState, seat: &str, text: String) -> String {
    let Some(id) = crate::seat_ids::rig_of(seat) else { return text };
    let text = {
        let store=st.store.lock().unwrap();
        match store.output_limit(id).ok().flatten() {
            Some(limit) => format!("Per-agent budget: {limit} output tokens for this project run, including your managed descendants each with their own {limit}-token budget. This agent has reported {} output tokens. Finish and report well before the limit. Runner stops you and descendants when reported usage reaches the limit; delayed CLI reports can overshoot.\n\n{text}",store.budget_used(seat).ok().flatten().map(|v|v.to_string()).unwrap_or_else(||"unknown".into())),
            None => text,
        }
    };
    let text = if crate::copies::enabled(st, id) {
        format!("You work in your own git copy of the project, on branch {}. Commit your work there. Teammates cannot see your files until the operator merges your branch, so send them what they need (file contents, paths and commits) in messages.\n\n{text}", crate::copies::branch_of(id, seat))
    } else { text };
    let briefed = briefed(&st.store.lock().unwrap(), seat).unwrap_or(false);
    let team = st.store.lock().unwrap().draft(id).ok().flatten();
    let lessons = team.as_ref().filter(|_| briefed).and_then(|d| crate::knowledge::new_lessons(st, id, seat, d));
    let text = match lessons { Some(note) => format!("{note}\n\n{text}"), None => text };
    let draft = team.filter(|_| !briefed);
    let log = draft.as_ref().and_then(|d| decision_log_note(&st.store.lock().unwrap(), id, d.decision_log));
    let text = match log { Some(note) => format!("{note}\n\n{text}"), None => text };
    let known = draft.as_ref().map(|d| match minimal::is_minimal(d, seat) {
        true => crate::knowledge::lesson_lines(st, id),
        false => crate::knowledge::briefing_lines(st, id, seat, d),
    }).unwrap_or_default();
    match draft.and_then(|d| briefing_with(&d, seat, &text)) {
        Some(context) => format!("{context}\n\n{}--- Current message ---\n{text}", known.iter().map(|l| format!("{l}\n\n")).collect::<String>()),
        None if briefed => text,
        None => ancestor_briefing(st, id, seat, text),
    }
}

/// A managed child's first message: its nearest configured ancestor's briefing, marked as context only.
fn ancestor_briefing(st: &AppState, id: &str, seat: &str, text: String) -> String {
    let store = st.store.lock().unwrap();
    let draft = store.draft(id).ok().flatten();
    let mut ancestor = seat.to_string();
    for _ in 0..4 {
        let Some(parent) = store.parent(&ancestor).ok().flatten() else { break };
        // A minimal agent's helpers are briefed minimally too.
        if let Some(context) = draft.as_ref().and_then(|d| minimal::child(d, &parent, seat)) {
            return format!("{context}\n\n--- Current message ---\n{text}");
        }
        if let Some(context) = draft.as_ref().and_then(|d| briefing(d, &parent)) {
            return format!("{context}\n\nYour active role is managed child agent {seat}; the ancestor role above is architecture context only. Use team_status for current teammates.\n\n--- Current message ---\n{text}");
        }
        ancestor = parent;
    }
    text
}

/// One line telling agents the team keeps a decision log, when it is on and has entries.
pub(crate) fn decision_log_note(store: &crate::store::Store, team: &str, on: bool) -> Option<String> {
    let n = store.decision_count(team).ok().filter(|n| on && *n > 0)?;
    Some(format!("The team keeps a decision log ({n} {}); read it with read_decisions before deciding something it may cover.", if n == 1 { "entry" } else { "entries" }))
}

/// Every agent's usage counts against the person's own plan; tell agents what is expensive.
const EFFICIENCY: &str = "Work efficiently: everything you read stays in your context and is re-read at every later step, and every agent you spawn is a full agent with usage of its own. Read a PDF with read_pdf first: it gives each page's text and names pages with no text, so you look at a page as an image only when it has no text and you need what is on it. Read only the parts of large files you need. Spawn a helper only for large, independent work; never spawn helpers to split up reading.";

pub(crate) fn briefing(d: &TeamDraft, seat: &str) -> Option<String> { briefing_with(d, seat, "") }

/// The briefing that goes with `message`; a minimal one leaves out the job when the message carries it.
pub(crate) fn briefing_with(d: &TeamDraft, seat: &str, message: &str) -> Option<String> {
    let me = d.agent_for_seat(seat)?;
    // Practice messages are executable fixture commands, not model prompts.
    if me.kind == model::AgentKind::Practice { return None; }
    if minimal::is_minimal(d, seat) { return Some(minimal::briefing(d, me, message)); }
    let mut lines = vec![format!("Team architecture (operator configuration): {}\nYour role: {} [{}]{}", d.name, me.name, me.id, in_group(d, me))];
    lines.extend(group_notes(d, me));
    if let Some(w) = &d.workflow {
        lines.push(workflow_line(w));
    }
    lines.push("Work only on your assigned job. A leader plans and coordinates the configured team; it does not replace the other agents or implement their jobs unless explicitly assigned. Sculptured automatically delivers completed replies along 'hands work to' connections. Use spawn_agent to create helpers; all child agents must be managed through this tool so their lifecycle and communication appear on the board. Do not create untracked agents through native CLI delegation, shell processes or external tools. Do not replace configured teammates. Use team_status and send_agent_message to ask teammates for help, request features, report results and coordinate shared interfaces. Include exact artifact revisions and task context. 'Reports to' and 'watches' are descriptive only, not automatic routing. Report missing instructions or blocked dependencies rather than guessing. A reported result is not independently verified.".into());
    lines.push(EFFICIENCY.into());
    lines.extend(crate::skills::briefing_lines(d, me));
    lines.push("Use downstream jobs to decide your handoff: when another agent owns implementation, provide its plan and inputs instead of doing that agent's work. Respect project-wide constraints stated in the jobs (such as output folders or no publishing) throughout the team. Ask the operator about conflicting instructions.".into());
    lines.extend(team_lines(d, me));
    lines.extend(contract_lines(d, me));
    Some(lines.join("\n\n"))
}

/// How the dynamic workflow works, for every agent of a team with one.
fn workflow_line(w: &Workflow) -> String {
    let who = match &w.dispatch {
        Some(_) => "Sculptured makes the batch by running the workflow's dispatch command once per project start; no agent calls dispatch_work.".to_string(),
        None => format!("Only agent {} calls dispatch_work(batch_id, summary, tasks:[{{id,title,context,mode,depends_on}}]) once per project start. It chooses N within 1–{}.", w.dispatcher, w.max_tasks),
    };
    format!("This project uses a dynamic workflow, not ordinary turn-end handoffs. {who} Each task runs stage {} first; stages branch by complete_work outcomes and can loop for at most {} steps. Assembly agent {} waits for every task to complete. Workers receive assignment IDs and must call complete_work(assignment,outcome,result); plain completion replies do not advance the workflow. workflow_status reads the batch. Reserved outcome blocked stops an item. Each topic chooses mode independent with empty depends_on for unrelated work, or dependent with prerequisite topic IDs. Independent topics can run simultaneously on available configured workers; dependent topics wait for accepted results from all prerequisites. Topic dependencies must be acyclic. Reading-order prerequisites alone do not require build dependencies. Use separate module paths; coordinate shared file changes. Use spawn_agent for tracked, bounded helpers when useful. Configured stages: {}", w.entry, w.max_steps, w.collector, serde_json::to_string(&w.stages).unwrap())
}

/// Who decides what in the team (MADL `authority`), and `me`'s own data contract (`data_access`).
fn contract_lines(d: &TeamDraft, me: &DraftAgent) -> Vec<String> {
    let mut out = Vec::new();
    let owners: Vec<String> = d.agents.iter()
        .flat_map(|a| a.advanced.authority.iter().flat_map(|x| x.domains.iter()).map(move |dom| format!("{}: {} [{}]", dom.trim(), a.name, a.id)))
        .collect();
    if !owners.is_empty() {
        out.push(format!("Who decides what: {}. Within another agent's domain, ask it (send_agent_message) before you decide.", owners.join("; ")));
    }
    // A decision belongs to the agent whose scope contains its consequences (McEntire, "The Swarm Needs a General").
    let scoped: Vec<String> = d.agents.iter().filter_map(|a| {
        let scope = a.advanced.authority.as_ref()?.scope.as_deref()?;
        let reach = if scope == "system" { "the whole team".to_string() } else { format!("the {} group", d.groups.iter().find(|g| g.id == scope).map_or(scope, |g| g.label.trim())) };
        Some(format!("{} [{}] owns decisions whose consequences reach across {reach}", a.name, a.id))
    }).collect();
    if !scoped.is_empty() {
        out.push(format!("{}. Settle what stays inside your own work directly with the agents involved; when a decision's consequences cross into another group's work, take it to that owner instead of agreeing it between yourselves.", scoped.join("; ")));
    }
    out.extend(own_contract(me));
    out
}

/// `me`'s own data contract (`data_access`), if it has one.
fn own_contract(me: &DraftAgent) -> Option<String> {
    let c = me.advanced.data_access.as_ref().filter(|c| !(c.reads.is_empty() && c.writes.is_empty() && c.side_effects.is_empty()))?;
    let mut parts = Vec::new();
    if !c.reads.is_empty() { parts.push(format!("you read {}", c.reads.join(", "))); }
    if !c.writes.is_empty() { parts.push(format!("you write {}", c.writes.join(", "))); }
    if !c.side_effects.is_empty() { parts.push(format!("other effects: {}", c.side_effects.join(", "))); }
    let why = c.rationale.as_deref().map(|r| format!(" Why: {}", r.trim())).unwrap_or_default();
    Some(format!("Your data contract: {}.{why} Stay within it: Sculptured refuses file reads and writes outside it where it can tell.", parts.join("; ")))
}

/// Whether the seat's current conversation already holds the briefing: a message went to it since the
/// conversation began. A resumed conversation is the earlier one continued.
pub(crate) fn briefed(store: &crate::store::Store, seat: &str) -> rusqlite::Result<bool> {
    let subject = model::Subject::Seat(seat.to_string());
    let starts = store.latest_of_types(&subject, &["tenure-started"], 50)?;
    let fresh = |e: &&model::EventRecord| !matches!(e.body, model::EventBody::TenureStarted { cause: model::TenureCause::Resume, .. });
    let Some(begin) = starts.iter().find(fresh).map(|e| e.seq) else { return Ok(false) };
    Ok(store.latest_of_types(&subject, &["message-queued"], 1)?.first().is_some_and(|m| m.seq > begin))
}

/// Agents `me` works with: its group and every agent it is connected to, either way.
fn neighbours<'a>(d: &'a TeamDraft, me: &DraftAgent) -> Vec<&'a DraftAgent> {
    let linked = |a: &DraftAgent| d.connections.iter().any(|c| (c.from == me.id && c.to == a.id) || (c.to == me.id && c.from == a.id));
    let grouped = |a: &DraftAgent| me.group_id.is_some() && a.group_id == me.group_id;
    d.agents.iter().filter(|a| a.id == me.id || linked(a) || grouped(a)).collect()
}

/// `me`'s neighbours with their jobs and connections; the rest of the team by name (team_status has more).
fn team_lines(d: &TeamDraft, me: &DraftAgent) -> Vec<String> {
    let near = neighbours(d, me);
    let mut lines: Vec<String> = near.iter().map(|a| {
        let group = group_label(d, a).map(|l| format!(" · Group: {l}")).unwrap_or_default();
        format!("Agent: {} [{}]{group} · Folder: {}\nAssigned job: {}", a.name, a.id, d.effective_folder(a).unwrap_or("not configured"), a.job.trim())
    }).collect();
    let rest: Vec<String> = d.agents.iter().filter(|a| !near.iter().any(|n| n.id == a.id)).map(|a| format!("{} [{}]{}", a.name, a.id, in_group(d, a))).collect();
    if !rest.is_empty() {
        lines.push(format!("The rest of the team (team_status gives their jobs and status): {}", rest.join(", ")));
    }
    for c in d.connections.iter().filter(|c| c.from == me.id || c.to == me.id) {
        let (Some(from), Some(to)) = (d.agent(&c.from), d.agent(&c.to)) else { continue };
        let kind = match c.kind { ConnectionKind::HandsWorkTo => "hands work to", ConnectionKind::ReportsTo => "reports to (descriptive)", ConnectionKind::Watches => "watches (descriptive)" };
        lines.push(format!("{} [{}]{} {kind} {} [{}]{}", from.name, from.id, in_group(d, from), to.name, to.id, in_group(d, to)));
    }
    lines
}

/// The name of the group an agent is in, if it is in a named group.
fn group_label<'a>(d: &'a TeamDraft, a: &DraftAgent) -> Option<&'a str> {
    let id = a.group_id.as_deref()?;
    d.groups.iter().find(|g| g.id == id).map(|g| g.label.trim()).filter(|l| !l.is_empty())
}

fn in_group(d: &TeamDraft, a: &DraftAgent) -> String {
    group_label(d, a).map(|l| format!(" (group {l})")).unwrap_or_default()
}

fn members(d: &TeamDraft, group: &str, except: &str) -> String {
    let names: Vec<&str> = d.agents.iter().filter(|a| a.group_id.as_deref() == Some(group) && a.id != except).map(|a| a.name.as_str()).collect();
    if names.is_empty() { "no other agents".into() } else { names.join(", ") }
}

/// What the group names mean for this agent: the group it is in, and each other group it has an
/// arrow to or from. The name is the shared purpose of that part of the team.
fn group_notes(d: &TeamDraft, me: &DraftAgent) -> Vec<String> {
    let mut out = Vec::new();
    if let (Some(label), Some(id)) = (group_label(d, me), me.group_id.as_deref()) {
        out.push(format!("Your group: {label}. Its name is the purpose you share with its other members ({}); do your job as part of it.", members(d, id, &me.id)));
    }
    let mut linked: Vec<&str> = Vec::new();
    for c in d.connections.iter().filter(|c| c.from == me.id || c.to == me.id) {
        let other = if c.from == me.id { &c.to } else { &c.from };
        let Some(g) = d.agent(other).and_then(|a| a.group_id.as_deref()) else { continue };
        if Some(g) != me.group_id.as_deref() && !linked.contains(&g) {
            linked.push(g);
        }
    }
    for g in linked {
        let Some(label) = d.groups.iter().find(|x| x.id == g).map(|x| x.label.trim()).filter(|l| !l.is_empty()) else { continue };
        out.push(format!("You are connected to the {label} group ({}). Its name says what that part of the team is for.", members(d, g, &me.id)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn each_agent_receives_its_own_role_and_the_configured_architecture() {
        let mut d = rigspec::templates::template("builder-reviewer", "project", "My project", false).unwrap();
        d.folder = Some("/home/me/project".into());
        for a in &d.agents {
            let text = briefing(&d, &d.seat_id(&a.id).unwrap()).unwrap();
            assert!(text.contains(&format!("Your role: {} [{}]", a.name, a.id)));
            assert!(text.contains("Builder [builder] hands work to Reviewer [reviewer]"));
            assert!(text.contains("Folder: /home/me/project"));
            assert!(text.contains("Use spawn_agent to create helpers"));
            assert!(text.contains("read_pdf"), "every agent is told to read PDFs as text first");
            assert!(text.contains(&a.job));
        }
        assert!(briefing(&d, "missing").is_none());
        assert!(!briefing(&d, &d.seat_id(&d.agents[0].id).unwrap()).unwrap().contains("Your group"));
    }

    #[test]
    fn agents_learn_who_decides_what_and_their_own_data_contract() {
        let mut d = rigspec::templates::template("builder-reviewer", "project", "My project", false).unwrap();
        let (builder, reviewer) = (d.agents[0].id.clone(), d.agents[1].id.clone());
        d.agents[0].advanced.authority = Some(model::Authority { domains: vec!["wing geometry".into()], scope: Some("system".into()), rationale: None });
        d.agents[0].advanced.data_access = Some(model::DataAccess { reads: vec!["manuals/".into()], writes: vec!["cad/wings/".into()], side_effects: vec!["none".into()], rationale: None, shared: vec![] });
        let to_reviewer = briefing(&d, &d.seat_id(&reviewer).unwrap()).unwrap();
        assert!(to_reviewer.contains("Who decides what: wing geometry: Builder [builder]."), "{to_reviewer}");
        assert!(to_reviewer.contains("Builder [builder] owns decisions whose consequences reach across the whole team"), "{to_reviewer}");
        assert!(!to_reviewer.contains("Your data contract"), "only the agent's own contract");
        let to_builder = briefing(&d, &d.seat_id(&builder).unwrap()).unwrap();
        assert!(to_builder.contains("Your data contract: you read manuals/; you write cad/wings/; other effects: none."), "{to_builder}");
    }

    #[test]
    fn the_group_name_reaches_its_members_and_the_agents_connected_to_it() {
        let mut d = rigspec::templates::template("builder-reviewer", "project", "My project", false).unwrap();
        d.groups.push(model::DraftGroup { id: "checks".into(), label: "Quality checks".into(), x: 0, y: 0, w: 300, h: 300, models: Default::default() });
        let (builder, reviewer) = (d.agents[0].id.clone(), d.agents[1].id.clone());
        d.agents[1].group_id = Some("checks".into());
        let to_reviewer = briefing(&d, &d.seat_id(&reviewer).unwrap()).unwrap();
        assert!(to_reviewer.contains("Your group: Quality checks. Its name is the purpose you share"), "{to_reviewer}");
        let to_builder = briefing(&d, &d.seat_id(&builder).unwrap()).unwrap();
        assert!(to_builder.contains("You are connected to the Quality checks group (Reviewer)"), "{to_builder}");
        assert!(to_builder.contains("Reviewer [reviewer] (group Quality checks)"), "{to_builder}");
        assert!(!to_builder.contains("Your group:"));
    }

    #[test]
    fn practice_agents_get_no_briefing() {
        let mut d = rigspec::templates::template("builder-reviewer", "project", "My project", false).unwrap();
        d.agents[0].kind = model::AgentKind::Practice;
        assert!(briefing(&d, &d.seat_id(&d.agents[0].id).unwrap()).is_none());
    }
}
