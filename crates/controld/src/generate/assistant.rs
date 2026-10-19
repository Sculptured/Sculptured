//! The design assistant: the person's own Claude Code or Codex changes a team's design, as MADL,
//! and the change comes back as a proposal. Nothing is saved here: the person inspects the
//! proposal on the board and approves it (the web app saves it as the design) or rejects it.
//!
//! The CLI sees the design without board positions, tools, workflows or the decision log, and
//! without the person's budgets and runner settings; those are put back afterwards (positions by
//! id), so cards stay where they were and new ones land beside their group.
use super::{call, Options};
use crate::error::{respond, ApiError};
use crate::extract::{Json, Path};
use crate::portable::madl::{from_madl, to_madl, Imported};
use crate::state::AppState;
use axum::extract::State;
use axum::response::Response;
use madl::model as m;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Turn {
    /// "person" or "assistant".
    pub role: String,
    pub text: String,
}

#[derive(Deserialize)]
pub(super) struct AssistantReq {
    options: Options,
    /// The conversation so far; the last turn is the person's request.
    messages: Vec<Turn>,
    /// The proposal being iterated on (MADL YAML from an earlier answer); none starts from the saved design.
    #[serde(default)]
    proposal: Option<String>,
}

#[derive(Serialize)]
pub(super) struct Proposal {
    /// What the assistant says it changed.
    summary: String,
    /// The proposed design as MADL, to send back when iterating.
    madl: String,
    /// The proposed design as a team draft (the saved team's id and revision).
    draft: model::TeamDraft,
    /// What the assistant wrote that Sculptured could not keep.
    dropped: Vec<rigspec::Diagnostic>,
    issues: Vec<model::DraftIssue>,
}

pub(super) async fn assistant(State(st): State<AppState>, Path(team): Path<String>, Json(req): Json<AssistantReq>) -> Response {
    respond(propose(&st, &team, req).await)
}

const ABOUT_MADL: &str = concat!("You change the design of a team of AI agents in Sculptured. The design is a MADL (Multi-Agent Descriptor Language) document in YAML. \
Sculptured runs: agents (`runtime: claude-code` or `codex`); groups (`groups:` on an agent lists the one group it is in); connections of kind `handoff` (the agent's finished work is sent to the other), `report` and `watch`; \
one input (the brief the starting agents receive); each agent's `instructions` (its job: the first message it receives, written as a direct, specific instruction); `model` (`provider/model`); \
`checks` (a shell command run after each turn); token budgets (`settings.restrictions.budget.tokens`); \
`data_access` (`reads`, `writes`, `side_effects`, `rationale`: what the agent reads and writes, relative to its folder; writes outside it are refused) and \
`authority` (`domains`: the decisions it owns; `scope`: a group id or `system` when its decisions reach across that group or the whole team). \
Keep the id of everything that still exists; give new things short, new ids (lowercase letters, digits and hyphens). \
Do not add channels, communication rules, people (`runtime: human`), steps, tools or workflows: the person sets those up on the board.\n\n", super::design_rules!());

/// The CLI takes at most 32 KB of prompt.
const PROMPT_LIMIT: usize = 31 * 1024;

async fn propose(st: &AppState, team: &str, req: AssistantReq) -> Result<Proposal, ApiError> {
    let saved = crate::portable::team::export_team(st, team, true)?;
    let current = match &req.proposal {
        Some(yaml) => parse_valid(yaml).map_err(|why| ApiError::bad_request("proposal_invalid", format!("The proposal to continue from is not valid MADL: {why}")))?,
        None => to_madl(&saved),
    };
    let prompt = prompt_for(&current, &req.messages)?;
    let (summary, mut doc) = call(st, &req.options, prompt, schema(), |v| answer(v)).await?;
    restore(&current, &mut doc);
    let Imported { mut file, dropped } = from_madl(&madl::expand(&doc));
    file.team.id = saved.team.id.clone();
    file.team.revision = saved.team.revision;
    let issues = crate::teams::issues_for(st, &file.team);
    let madl = serde_yaml_ng::to_string(&doc).map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Proposal { summary, madl, draft: file.team, dropped, issues })
}

fn prompt_for(current: &m::Document, messages: &[Turn]) -> Result<String, ApiError> {
    let last_is_person = messages.last().is_some_and(|t| t.role == "person" && !t.text.trim().is_empty());
    if !last_is_person {
        return Err(ApiError::bad_request("no_request", "Say what to change"));
    }
    let shown = serde_yaml_ng::to_string(&for_prompt(current)).map_err(|e| ApiError::internal(e.to_string()))?;
    let talk: String = messages.iter().map(|t| format!("{}: {}\n", if t.role == "person" { "Person" } else { "You" }, t.text.trim())).collect();
    let prompt = format!(
        "{ABOUT_MADL}\n\nThe current design:\n```yaml\n{shown}```\n\nThe conversation so far:\n{talk}\nMake the change the person asked for in their last message, and nothing they did not ask for. \
Answer with `madl`: the complete revised document (not a diff), and `summary`: one to three sentences saying what you changed."
    );
    if prompt.len() > PROMPT_LIMIT {
        return Err(ApiError::bad_request("design_too_large", format!("This design is too large for the assistant ({} KB; at most {} KB). Shorten some jobs, or ask about part of it.", prompt.len() / 1024, PROMPT_LIMIT / 1024)));
    }
    Ok(prompt)
}

fn schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["summary", "madl"],
        "properties": { "summary": { "type": "string" }, "madl": { "type": "string" } },
    })
}

/// A usable answer is a valid MADL document; the reasons it is not go back to the CLI once.
fn answer(v: &Value) -> Result<(String, m::Document), String> {
    let summary = v.get("summary").and_then(Value::as_str).unwrap_or("").trim().to_string();
    let text = v.get("madl").and_then(Value::as_str).ok_or("there was no `madl`")?;
    let doc = parse_valid(text)?;
    if doc.agents.is_empty() {
        return Err("the design has no agents".into());
    }
    Ok((summary, doc))
}

fn parse_valid(text: &str) -> Result<m::Document, String> {
    let nowhere = std::env::temp_dir().join("sculptured-assistant-has-no-folder");
    let doc = madl::from_str(text, &nowhere, &madl::Options::default()).map_err(|e| e.0.to_string())?;
    let report = madl::validate(&doc);
    if report.is_ok() {
        return Ok(doc);
    }
    Err(report.errors().map(ToString::to_string).collect::<Vec<_>>().join("; "))
}

/// The design as the CLI sees it: no positions, tools, workflows, decision log or runner settings.
fn for_prompt(doc: &m::Document) -> m::Document {
    let mut d = doc.clone();
    d.schema = None;
    d.layout = None;
    d.tools.clear();
    d.workflows.clear();
    d.channels.clear();
    let workflow_inputs: Vec<String> = doc.workflows.iter().filter_map(|w| w.input.clone()).collect();
    d.inputs.retain(|i| !workflow_inputs.contains(&i.id));
    d.agents.iter_mut().for_each(|a| a.tools.clear());
    if let Some(s) = d.defaults.as_mut() {
        if let Some(p) = s.preferences.as_mut() {
            (p.context, p.config) = (None, None);
        }
        if let Some(r) = s.restrictions.as_mut() {
            (r.tools, r.approvals) = (None, None);
        }
    }
    d
}

/// Budgets are the person's: the run budget and the existing agents' limits stay as saved,
/// whatever the proposal says (a new agent keeps what the proposal gives it).
fn keep_budgets(before: &m::Document, doc: &mut m::Document) {
    doc.system.budget = before.system.budget.clone();
    for a in &mut doc.agents {
        let Some(was) = before.agents.iter().find(|b| b.id == a.id) else { continue };
        let saved = was.settings.as_ref().and_then(|s| s.restrictions.as_ref()).and_then(|r| r.budget.clone());
        let settings = a.settings.get_or_insert_with(Default::default);
        let restrictions = settings.restrictions.get_or_insert_with(Default::default);
        restrictions.budget = saved;
    }
}

/// The person's tools, workflows, decision log and runner settings, as saved; an agent keeps the
/// tools it had.
fn keep_setup(before: &m::Document, doc: &mut m::Document) {
    doc.schema.clone_from(&before.schema);
    (doc.tools, doc.workflows, doc.channels) = (before.tools.clone(), before.workflows.clone(), before.channels.clone());
    let workflow_inputs = before.inputs.iter().filter(|i| before.workflows.iter().any(|w| w.input.as_ref() == Some(&i.id)));
    doc.inputs.extend(workflow_inputs.cloned());
    for a in &mut doc.agents {
        a.tools = before.agents.iter().find(|b| b.id == a.id).map(|b| b.tools.clone()).unwrap_or_default();
    }
    let saved = before.defaults.clone().unwrap_or_default();
    let s = doc.defaults.get_or_insert_with(Default::default);
    let p = s.preferences.get_or_insert_with(Default::default);
    let was = saved.preferences.unwrap_or_default();
    (p.context, p.config) = (was.context, was.config);
    let r = s.restrictions.get_or_insert_with(Default::default);
    let was = saved.restrictions.unwrap_or_default();
    (r.tools, r.approvals) = (was.tools, was.approvals);
}

/// Put back what [`for_prompt`] left out, by id, and place what is new.
fn restore(before: &m::Document, doc: &mut m::Document) {
    doc.extensions = before.extensions.clone();
    for g in &mut doc.groups {
        g.extensions = before.groups.iter().find(|b| b.id == g.id).map(|b| b.extensions.clone()).unwrap_or_default();
    }
    for a in &mut doc.agents {
        a.extensions = before.agents.iter().find(|b| b.id == a.id).map(|b| b.extensions.clone()).unwrap_or_default();
    }
    keep_budgets(before, doc);
    keep_setup(before, doc);
    doc.layout = Some(super::assistant_layout::place(before.layout.clone().unwrap_or_default(), doc));
}
