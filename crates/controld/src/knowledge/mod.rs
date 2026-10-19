//! Knowledge: what a team's agents know about their project and each other, with nobody maintaining
//! it. A project map generated on the agents' computer at every start (`localops::map`), who has been
//! working where, lessons agents recorded, and a search of what the team already did. Agents get the
//! short version in their briefing and look up the rest; people see it in the Knowledge view.
mod tools;
pub use tools::{record_lesson, search_history};

use crate::error::{respond, ApiError};
use crate::extract::Path;
use crate::state::AppState;
use axum::extract::State;
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use localops::map::ProjectMap;
use crate::store::DecisionEntry;
use model::TeamDraft;
use rusqlite::params;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

/// How long a start waits for the maps before the agents' first messages go without one.
const MAP_WAIT: Duration = Duration::from_secs(20);
/// Areas in a briefing's outline, at most (the full map has the rest).
const OUTLINE_AREAS: usize = 25;
/// Lessons in a briefing, newest first.
const BRIEFING_LESSONS: usize = 8;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/teams/{id}/knowledge", get(view))
        .route("/api/teams/{id}/decisions/{entry}/confirm", post(confirm))
}

/// The folders the team's agents work in, as this computer (or the agents' computer) names them.
fn folders(st: &AppState, d: &TeamDraft) -> BTreeSet<String> {
    let home = st.home.as_deref().cloned().unwrap_or_default();
    d.agents.iter().filter_map(|a| d.effective_folder(a)).map(|f| crate::launch::cwd_for(st, &home, f)).collect()
}

/// Maps every folder the team works in, on the agents' computer, and keeps them. Never fails a start:
/// a folder that cannot be mapped (an older runner, a missing folder) is skipped.
pub async fn refresh(st: &AppState, team: &str, d: &TeamDraft) {
    let home = st.home.as_deref().cloned().unwrap_or_default();
    for folder in folders(st, d) {
        // The shared values of the agents working in this folder, checked in its files.
        let shared = d.agents.iter()
            .filter(|a| d.effective_folder(a).map(|f| crate::launch::cwd_for(st, &home, f)).as_deref() == Some(folder.as_str()))
            .flat_map(|a| a.advanced.data_access.iter().flat_map(|c| c.shared.clone()))
            .collect();
        let op = localops::LocalOp::ProjectMap { path: folder.clone(), shared };
        match tokio::time::timeout(MAP_WAIT, crate::ops::run(st, crate::ops::Target::Team(team), op)).await {
            Ok(Ok(map)) => {
                let saved = st.store.lock().unwrap().connection().execute(
                    "INSERT OR REPLACE INTO project_maps (team, folder, json, at_ms) VALUES (?1, ?2, ?3, ?4)",
                    params![team, folder, map.to_string(), model::now_ms()],
                );
                if let Err(e) = saved {
                    tracing::warn!(%team, error = %e, "project map not kept");
                }
            }
            Ok(Err(e)) => tracing::info!(%team, %folder, error = %e.message, "no project map for this folder"),
            Err(_) => tracing::info!(%team, %folder, "the project map took too long; agents start without it"),
        }
    }
}

/// The team's kept maps: (folder, map, when).
fn maps(st: &AppState, team: &str) -> Vec<(String, ProjectMap, i64)> {
    let store = st.store.lock().unwrap();
    let Ok(mut q) = store.connection().prepare("SELECT folder, json, at_ms FROM project_maps WHERE team = ?1 ORDER BY folder") else { return vec![] };
    let rows = q.query_map([team], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)));
    rows.map(|it| it.flatten().filter_map(|(f, j, at)| serde_json::from_str(&j).ok().map(|m| (f, m, at))).collect()).unwrap_or_default()
}

/// Who has changed files where: folder area → agent names, from the agents' recorded file writes.
fn workers(st: &AppState, team: &str, d: &TeamDraft, folder: &str) -> BTreeMap<String, BTreeSet<String>> {
    let store = st.store.lock().unwrap();
    let sql = "SELECT substr(e.subject, 6), json_extract(e.json, '$.body.input.file_path') FROM events e JOIN seats s ON e.subject = 'seat:' || s.id
               WHERE s.rig = ?1 AND json_extract(e.json, '$.body.type') = 'tool-called'
               AND json_extract(e.json, '$.body.name') IN ('Write', 'Edit', 'MultiEdit', 'NotebookEdit') ORDER BY e.seq DESC LIMIT 5000";
    let Ok(mut q) = store.connection().prepare(sql) else { return BTreeMap::new() };
    let rows = q.query_map([team], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)));
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (seat, file) in rows.map(|it| it.flatten().collect::<Vec<_>>()).unwrap_or_default() {
        let Some(rel) = file.as_deref().and_then(|f| f.strip_prefix(folder)).map(|r| r.trim_start_matches('/')) else { continue };
        let area: Vec<&str> = rel.split('/').collect();
        let area = area[..area.len().saturating_sub(1)].iter().take(2).copied().collect::<Vec<_>>().join("/");
        let name = d.agent_for_seat(&seat).map_or(seat.clone(), |a| a.name.clone());
        out.entry(area).or_default().insert(name);
    }
    out
}

/// What goes into an agent's briefing: its folder's map outline (with who worked where), the team's
/// lessons, and how to look further.
pub(crate) fn briefing_lines(st: &AppState, team: &str, seat: &str, d: &TeamDraft) -> Vec<String> {
    let mut out = Vec::new();
    let home = st.home.as_deref().cloned().unwrap_or_default();
    let folder = d.agent_for_seat(seat).and_then(|a| d.effective_folder(a)).map(|f| crate::launch::cwd_for(st, &home, f));
    let map = folder.and_then(|f| maps(st, team).into_iter().find(|(m, ..)| *m == f));
    if let Some((f, map, _)) = &map {
        out.push(outline(map, &workers(st, team, d, f)));
        if !map.shared.is_empty() {
            out.push(shared_lines(map));
        }
    }
    let entries = st.store.lock().unwrap().decisions(team, None, 50, false).unwrap_or_default();
    out.extend(recorded(&entries));
    // Kept with the project by earlier teams; this team's own entries are listed above already.
    let kept: Vec<&String> = map.as_ref().map(|(_, m, _)| m.kept.iter().filter(|k| !entries.iter().any(|e| k.ends_with(e.text.trim()))).collect()).unwrap_or_default();
    if !kept.is_empty() {
        out.push(format!("Kept with this project from earlier work (confirmed by a person; [constraint] ones are rules):\n- {}", kept.iter().map(|k| k.as_str()).collect::<Vec<_>>().join("\n- ")));
    }
    out.push("Before building something, check whether it already exists: the project map, then search_history (what the team's agents, including finished ones, wrote, ran and decided). When something cost you a wrong turn that others could avoid, record it with record_lesson.".into());
    out
}

/// Only the constraints and lessons the team's agents recorded (a minimal briefing's share).
pub(crate) fn lesson_lines(st: &AppState, team: &str) -> Vec<String> {
    recorded(&st.store.lock().unwrap().decisions(team, None, 50, false).unwrap_or_default())
}

/// The newest recorded constraints, then lessons, each as one briefing paragraph.
fn recorded(entries: &[DecisionEntry]) -> Vec<String> {
    let line = |e: &DecisionEntry| format!("{}{} ({})", if e.confirmed_ms.is_some() { "[confirmed] " } else { "" }, e.text.trim(), e.author);
    let of = |source: &str| entries.iter().filter(|e| e.source == source).take(BRIEFING_LESSONS).map(line).collect::<Vec<_>>();
    let mut out = Vec::new();
    let constraints = of("constraint");
    if !constraints.is_empty() {
        out.push(format!("Constraints this team's agents found (follow them):\n- {}", constraints.join("\n- ")));
    }
    let lessons = of("lesson");
    if !lessons.is_empty() {
        out.push(format!("Lessons this team's agents recorded, newest first:\n- {}", lessons.join("\n- ")));
    }
    out
}

/// For an agent already briefed: the lessons its teammates recorded since its last message, so a
/// lesson reaches agents that are already working, quietly, with the next message they get anyway.
pub(crate) fn new_lessons(st: &AppState, team: &str, seat: &str, d: &TeamDraft) -> Option<String> {
    let me = d.agent_for_seat(seat).map(|a| a.name.clone());
    let store = st.store.lock().unwrap();
    let since = store.latest_of_types(&model::Subject::Seat(seat.to_string()), &["message-queued"], 1).ok()?.first()?.at_ms;
    let fresh: Vec<String> = store.decisions(team, None, 50, false).ok()?.into_iter()
        .filter(|e| (e.source == "lesson" || e.source == "constraint") && e.at_ms > since && me.as_ref() != Some(&e.author))
        .take(BRIEFING_LESSONS)
        .map(|e| format!("{}{} ({})", if e.source == "constraint" { "[constraint] " } else { "" }, e.text.trim(), e.author))
        .collect();
    (!fresh.is_empty()).then(|| format!("New lesson{} from your team since your last message:\n- {}", if fresh.len() == 1 { "" } else { "s" }, fresh.join("\n- ")))
}

/// The contracts' shared values as found at this start: what agents can rely on, and what not.
fn shared_lines(map: &ProjectMap) -> String {
    let lines: Vec<String> = map.shared.iter().map(|c| match (c.status.as_str(), c.line) {
        ("found", Some(l)) => format!("- {} = {} ({} line {l})", c.name, c.value, c.file),
        ("missing", _) => format!("- NOT FOUND: {} = {} should be in {}, which does not contain it. Tell the agent whose contract lists it before relying on either value.", c.name, c.value, c.file),
        _ => format!("- NOT FOUND: {} = {} should be in {}, which does not exist.", c.name, c.value, c.file),
    }).collect();
    format!("Values shared between agents, checked in their files at this start (use exactly these):\n{}", lines.join("\n"))
}

fn outline(map: &ProjectMap, who: &BTreeMap<String, BTreeSet<String>>) -> String {
    let mut areas: Vec<_> = map.areas.iter().collect();
    areas.sort_by(|a, b| who.contains_key(&b.path).cmp(&who.contains_key(&a.path)).then(b.files.cmp(&a.files)));
    let lines: Vec<String> = areas.iter().take(OUTLINE_AREAS).map(|a| {
        let kinds: Vec<&str> = a.kinds.iter().take(2).map(|(k, _)| k.as_str()).collect();
        let name = if a.path.is_empty() { "(top level)" } else { a.path.as_str() };
        let about = a.about.as_ref().map(|s| format!(": {s}")).unwrap_or_default();
        let by = who.get(&a.path).map(|w| format!(" · worked on by {}", w.iter().cloned().collect::<Vec<_>>().join(", "))).unwrap_or_default();
        format!("- {name} ({} files, {}){about}{by}", a.files, kinds.join(", "))
    }).collect();
    let more = map.areas.len().saturating_sub(OUTLINE_AREAS);
    format!(
        "Project map, generated at this start from your folder ({} files). The full map, with every document's sections and their line ranges, is in {}: read it before building something that may exist already.\n{}{}",
        map.files, map.file, lines.join("\n"), if more > 0 { format!("\n- and {more} more areas in the full map") } else { String::new() }
    )
}

async fn view(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    let d = st.store.lock().unwrap().draft(&id).ok().flatten();
    let maps: Vec<Value> = maps(&st, &id).into_iter().map(|(folder, map, at)| {
        let who = d.as_ref().map(|d| workers(&st, &id, d, &folder)).unwrap_or_default();
        json!({ "folder": folder, "at_ms": at, "map": map, "workers": who })
    }).collect();
    let entries = st.store.lock().unwrap().decisions(&id, None, 200, false);
    respond(entries.map(|e| json!({ "maps": maps, "entries": e })).map_err(ApiError::from))
}

/// A confirmed lesson or constraint is also kept with the project, in each folder the team works in,
/// so later teams there are told it. A runner that cannot keep it leaves it with this team only.
fn keep_in_project(st: &AppState, team: &str, entry: &str) {
    let store = st.store.lock().unwrap();
    let found = store.decisions(team, None, 500, true).unwrap_or_default().into_iter().find(|e| e.id == entry);
    let Some(e) = found.filter(|e| e.source == "lesson" || e.source == "constraint") else { return };
    let Some(d) = store.draft(team).ok().flatten() else { return };
    drop(store);
    let text = format!("{}{}", if e.source == "constraint" { "[constraint] " } else { "" }, e.text.trim());
    let (st, team, folders) = (st.clone(), team.to_string(), folders(st, &d));
    tokio::spawn(async move {
        for path in folders {
            let op = localops::LocalOp::KeepLesson { path: path.clone(), text: text.clone() };
            if let Err(e) = crate::ops::run(&st, crate::ops::Target::Team(&team), op).await {
                tracing::info!(%team, %path, error = %e.message, "lesson not kept with the project");
            }
        }
    });
}

async fn confirm(State(st): State<AppState>, Path((id, entry)): Path<(String, String)>) -> Response {
    let done = st.store.lock().unwrap().confirm_decision(&id, &entry);
    if matches!(done, Ok(true)) {
        keep_in_project(&st, &id, &entry);
    }
    respond(match done {
        Ok(true) => Ok(json!({ "confirmed": entry })),
        Ok(false) => Err(ApiError::not_found("No such entry")),
        Err(e) => Err(ApiError::from(e)),
    })
}
