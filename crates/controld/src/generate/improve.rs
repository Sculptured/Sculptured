//! Improving a team rebuilt from an imported Claude Code or Codex session, with the user's own CLI.
//! The CLI reads a summary of the run (each agent, what it was asked, which tools it used and how often,
//! what it said last) and the rebuilt team; never tool inputs, tool output or file contents. The answer
//! is offered, not applied: the person sees it as a diff and applies it from the Design board.
use super::{ABOUT, Kinds, Offered, Options, sketch};
use crate::error::ApiError;
use crate::extract::{Json, Path};
use crate::portable::session::SessionLine;
use crate::state::AppState;
use axum::extract::State;
use axum::response::Response;
use model::{EventBody, Subject, TeamDraft};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};

#[derive(Deserialize)]
pub(super) struct ImproveReq {
    options: Options,
    /// The imported run (negative, as History numbers imported runs).
    start_seq: i64,
}

pub(super) async fn improve(
    State(st): State<AppState>,
    Path(team): Path<String>,
    Json(req): Json<ImproveReq>,
) -> Response {
    crate::error::respond(improve_team(&st, &team, &req).await)
}

async fn improve_team(st: &AppState, team: &str, req: &ImproveReq) -> Result<Offered, ApiError> {
    if req.start_seq >= 0 {
        return Err(ApiError::bad_request(
            "not_imported",
            "Improve with AI works on a run imported from a Claude Code or Codex session",
        ));
    }
    let summary = run_summary(
        &st.store.lock().expect("store mutex must not be poisoned"),
        team,
        -req.start_seq,
    )?;
    let current = crate::portable::team::export_team(st, team, true)?;
    let rebuilt = team_brief(&current.team);
    let prompt = format!(
        "{ABOUT}\n\nThis team was rebuilt from a past Claude Code or Codex session. Propose a better, reusable version: general roles and jobs instead of one-off prompts, the handoffs that actually happened, and checks where they help. Keep agent kinds unless there is a clear reason.\n\nThe session:\n{summary}\n\nThe team as rebuilt:\n{rebuilt}\n\nGive 1 team, labelled \"team\", with a 2-3 sentence rationale. Use short, distinct agent names. Groups are optional; an agent's group is a group name or null."
    );
    let a = crate::generate::call(
        st,
        &req.options,
        prompt,
        sketch::teams_schema(1),
        crate::generate::alternatives_parser(1),
    )
    .await?;
    let x = a
        .alternatives
        .into_iter()
        .next()
        .expect("the parser keeps one team");
    let folder = current.team.agents.first().and_then(|a| a.folder.clone());
    let mut o = crate::generate::offered(st, x, Kinds::from_option(None), folder.as_deref());
    keep_models(&mut o.draft, &current.team);
    Ok(o)
}

/// An agent keeps the model its namesake used, else the model of the first agent of its kind.
fn keep_models(proposed: &mut TeamDraft, current: &TeamDraft) {
    for a in &mut proposed.agents {
        let same = current
            .agents
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(&a.name));
        let kind = current.agents.iter().find(|c| c.kind == a.kind);
        if let Some(m) = same.or(kind).and_then(|c| c.advanced.model.clone()) {
            a.advanced.model = Some(m);
        }
    }
}

/// What one agent did in the run, without content beyond its own short words.
#[derive(Default)]
struct Facts {
    task: Option<String>,
    tools: BTreeMap<String, usize>,
    last: Option<String>,
}

fn cut(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if t.len() < s.len() {
        format!("{t}…")
    } else {
        t
    }
}

impl Facts {
    fn take(&mut self, b: &EventBody) {
        match b {
            EventBody::MessageQueued { text, .. } if self.task.is_none() => {
                self.task = Some(cut(text, 300))
            }
            EventBody::ToolCalled { name, .. } => *self.tools.entry(name.clone()).or_default() += 1,
            EventBody::AgentText { text, .. } => self.last = Some(cut(text, 200)),
            _ => {}
        }
    }
}

fn run_summary(store: &crate::store::Store, team: &str, id: i64) -> Result<String, ApiError> {
    let detail = crate::portable::session::imported_detail(store, team, id)?;
    let mut facts: HashMap<String, Facts> = HashMap::new();
    for raw in store.imported_lines(id)? {
        let Ok(line) = serde_json::from_str::<SessionLine>(&raw) else {
            continue;
        };
        if let Subject::Seat(seat) = &line.event.subject {
            facts
                .entry(seat.clone())
                .or_default()
                .take(&line.event.body);
        }
    }
    let name = |id: &str| {
        detail
            .agents
            .iter()
            .find(|a| a.id == id)
            .and_then(|a| a.name.clone())
            .unwrap_or_else(|| id.to_string())
    };
    let lines = detail.agents.iter().map(|a| {
        let f = facts.remove(&a.id).unwrap_or_default();
        let by = a
            .parent
            .as_deref()
            .map_or("the main agent".to_string(), |p| {
                format!("started by {}", name(p))
            });
        let tools = f
            .tools
            .iter()
            .map(|(t, n)| format!("{t} ×{n}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "- {} ({by}). Asked: {:?}. Tools: {}. Last said: {:?}.",
            name(&a.id),
            f.task.unwrap_or_default(),
            if tools.is_empty() { "none" } else { &tools },
            f.last.unwrap_or_default()
        )
    });
    Ok(fit(lines.collect(), SUMMARY_BUDGET))
}

/// The CLI takes at most 32 KB of prompt: room for the instructions, the summary and the team.
const SUMMARY_BUDGET: usize = 12 * 1024;
const TEAM_BUDGET: usize = 14 * 1024;

/// As many lines as fit in `budget` bytes, then how many were left out.
fn fit(lines: Vec<String>, budget: usize) -> String {
    let mut out = String::new();
    let indexed_lines = lines.iter().enumerate();
    for (i, l) in indexed_lines {
        if out.len() + l.len() + 1 > budget {
            out.push_str(&format!("…and {} more agents", lines.len() - i));
            break;
        }
        out.push_str(l);
        out.push('\n');
    }
    out
}

/// The rebuilt team, short: each agent's name, kind, model and the start of its job, and the arrows.
fn team_brief(t: &TeamDraft) -> String {
    let name = |id: &str| {
        t.agents
            .iter()
            .find(|a| a.id == id)
            .map_or(id.to_string(), |a| a.name.clone())
    };
    let agents = t.agents.iter().map(|a| {
        let model = a.advanced.model.as_deref().unwrap_or("default");
        format!(
            "- {} ({:?}, {model}): {:?}",
            a.name,
            a.kind,
            cut(&a.job, 400)
        )
    });
    let arrows = t
        .connections
        .iter()
        .map(|c| format!("- {} {:?} {}", name(&c.from), c.kind, name(&c.to)));
    format!(
        "Agents:\n{}Arrows:\n{}",
        fit(agents.collect(), TEAM_BUDGET - 2048),
        fit(arrows.collect(), 2048)
    )
}
