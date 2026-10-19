//! Generating a team with the user's own Claude Code or Codex: optional clarifying questions, then
//! 1 or 3 team sketches, each turned into a laid-out team draft with its issues, which the person
//! trims and creates (`/api/generate/check`, `/api/generate/create`).
//! The CLI runs on the user's computer (`LocalOp::Generate`), with no tools.
mod accept;
mod assistant;
mod assistant_layout;
mod improve;
pub mod sketch;

use crate::error::{respond, ApiError};
use crate::extract::Json;
use crate::ops::{self, Target};
use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::post;
use axum::Router;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sketch::{Kinds, Sketch};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/generate/questions", post(questions))
        .route("/api/generate/teams", post(teams))
        .route("/api/generate/check", post(accept::check_draft))
        .route("/api/generate/create", post(accept::create_draft))
        .route("/api/teams/{id}/improve-from-run", post(improve::improve))
        .route("/api/teams/{id}/assistant", post(assistant::assistant))
}

#[derive(Deserialize, Serialize, Clone, Default, Debug)]
pub struct Options {
    pub cli: String,
    #[serde(default)]
    pub model: Option<String>,
    /// "2-3" | "4-6" | "7-10"
    #[serde(default)]
    pub size: Option<String>,
    /// "claude" | "codex" | "practice" | "mixed"
    #[serde(default)]
    pub kinds: Option<String>,
    /// "chain" | "lead" | "reviewers"
    #[serde(default)]
    pub shape: Option<String>,
    #[serde(default)]
    pub checks: bool,
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub team: Option<String>,
}

impl Options {
    fn target(&self) -> Target<'_> {
        match (&self.team, &self.host) {
            (Some(t), _) => Target::Team(t),
            (None, Some(h)) => Target::Host(h),
            _ => Target::Default,
        }
    }

    /// The options in plain sentences, for the prompt.
    fn sentences(&self) -> String {
        let mut s = Vec::new();
        match self.size.as_deref() {
            Some("2-3") => s.push("Use 2 or 3 agents.".to_string()),
            Some("4-6") => s.push("Use about 4 to 6 agents.".to_string()),
            Some("7-10") => s.push("Use about 7 to 10 agents, in groups where that helps.".to_string()),
            _ => {}
        }
        match self.kinds.as_deref() {
            Some("claude") => s.push("Every agent is a Claude Code agent (kind \"claude\").".into()),
            Some("codex") => s.push("Every agent is a Codex agent (kind \"codex\").".into()),
            Some("mixed") => s.push("Mix Claude Code (\"claude\") and Codex (\"codex\") agents where each fits best.".into()),
            Some("practice") => s.push("Use kind \"claude\" for every agent.".into()),
            _ => {}
        }
        match self.shape.as_deref() {
            Some("chain") => s.push("Shape: a chain, each agent handing its work to the next.".into()),
            Some("lead") => s.push("Shape: one lead agent that hands work to helpers.".into()),
            Some("reviewers") => s.push("Shape: builders whose work goes to reviewers.".into()),
            _ => {}
        }
        s.push(if self.checks {
            "Where a shell command can verify an agent's work (tests, a build, a linter), set it as that agent's check; otherwise null.".into()
        } else {
            "Set every check to null.".into()
        });
        s.join(" ")
    }
}

/// Rules of thumb from multi-agent research (Kim et al. 2025, McEntire 2026, Anthropic, Cognition, Yang's
/// CS2680): heuristics the design assistants follow, not laws.
macro_rules! design_rules {
    () => {
        "Rules of thumb (heuristics from multi-agent research, not laws): \
Use one agent unless the work splits into parts that are truly independent, need different permissions, or would not fit in one agent's context; every extra agent adds planning, messages and merging. \
Split along the work's real dependencies, not along job titles. \
When the parts barely depend on each other, give every agent the same goal and the context it needs; when they depend heavily on each other, make one agent own the decisions that cross between them rather than adding more messages. \
Settle shared interfaces and file formats first, before the parts are built. \
Judge work with checks and tests, not with another agent's approval: agents with the same context share the same blind spots, so a reviewer should try to refute the work, and tests are best written without seeing the implementation. \
Prefer a few clear connections to everyone talking to everyone. \
Give each agent a clear area of files it writes, so no two agents write the same files."
    };
}
pub(super) use design_rules;

pub(super) const ABOUT: &str = concat!("You design teams of AI agents for Sculptured. Agents are Claude Code or Codex sessions. \
An arrow \"hands-work-to\" means the first agent's finished work is sent to the second; \"reports-to\" and \"watches\" record intent only. \
Each agent's job is the first message it receives: write it as a direct, specific instruction.\n\n", design_rules!());

fn questions_prompt(prompt: &str, o: &Options) -> String {
    format!("{ABOUT}\n\nThe user wants this team:\n{prompt}\n\n{}\n\nBefore designing it, ask 2 to 4 short questions whose answers would most change the design. Give each 2 to 4 likely answers.", o.sentences())
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Answer {
    pub question: String,
    pub answer: String,
}

fn teams_prompt(prompt: &str, o: &Options, answers: &[Answer], count: usize) -> String {
    let qa: String = answers.iter().filter(|a| !a.answer.trim().is_empty()).map(|a| format!("- {} {}\n", a.question, a.answer)).collect();
    let qa = if qa.is_empty() { String::new() } else { format!("\nThe user answered:\n{qa}") };
    let alternatives = if count == 3 {
        "Give 3 alternatives: \"lean\" (the fewest agents that can do it well), \"balanced\", and \"thorough\" (more checking and review). Give each a 2-3 sentence rationale."
    } else {
        "Give 1 team, labelled \"team\", with a 2-3 sentence rationale."
    };
    format!("{ABOUT}\n\nThe user wants this team:\n{prompt}\n{qa}\n{}\n\n{alternatives} Use short, distinct agent names. Groups are optional; an agent's group is a group name or null.", o.sentences())
}

/// One call, then one retry that says what was wrong; the raw answer travels with the final error.
async fn call<T>(st: &AppState, o: &Options, prompt: String, schema: Value, usable: impl Fn(&Value) -> Result<T, String>) -> Result<T, ApiError> {
    let mut why = String::new();
    let mut raw = String::new();
    for attempt in 0..2 {
        let text = if attempt == 0 { prompt.clone() } else { format!("{prompt}\n\nYour previous answer could not be used: {why}. Answer again, following the schema exactly.") };
        let op = localops::LocalOp::Generate { cli: o.cli.clone(), model: o.model.clone().filter(|m| !m.trim().is_empty()), prompt: text, schema: schema.clone() };
        match ops::run(st, o.target(), op).await {
            Ok(v) => match usable(&v) {
                Ok(t) => return Ok(t),
                Err(e) => {
                    why = e;
                    raw = v.to_string();
                }
            },
            Err(e) if e.code == "generate_failed" => {
                why = e.message.clone();
                raw = e.message;
            }
            Err(e) => return Err(e),
        }
    }
    let name = if o.cli == "codex" { "Codex" } else { "Claude Code" };
    let mut e = ApiError::new(StatusCode::BAD_GATEWAY, "generate_unusable", format!("{name}'s answer could not be used: {why}"));
    e.raw = Some(raw.chars().take(4096).collect());
    Err(e)
}

#[derive(Deserialize)]
struct QuestionsReq {
    options: Options,
    prompt: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
struct Question {
    question: String,
    suggestions: Vec<String>,
}

#[derive(Deserialize, Serialize)]
struct Questions {
    questions: Vec<Question>,
}

async fn questions(State(st): State<AppState>, Json(req): Json<QuestionsReq>) -> Response {
    let prompt = questions_prompt(&req.prompt, &req.options);
    let parse = |v: &Value| {
        let mut q = serde_json::from_value::<Questions>(v.clone()).map_err(|e| e.to_string())?;
        q.questions.retain(|x| !x.question.trim().is_empty());
        if q.questions.is_empty() {
            return Err("there were no questions".to_string());
        }
        q.questions.truncate(4);
        for x in &mut q.questions {
            x.suggestions.truncate(4);
        }
        Ok(q)
    };
    respond(call(&st, &req.options, prompt, sketch::questions_schema(), parse).await)
}

#[derive(Deserialize)]
struct TeamsReq {
    options: Options,
    prompt: String,
    #[serde(default)]
    answers: Vec<Answer>,
    #[serde(default = "three")]
    count: usize,
}

fn three() -> usize {
    3
}

#[derive(Deserialize)]
pub(super) struct Alternative {
    label: String,
    rationale: String,
    team: Sketch,
}

#[derive(Deserialize)]
pub(super) struct Alternatives {
    alternatives: Vec<Alternative>,
}

/// Reads the CLI's answer as `count` team sketches, each with agents.
fn alternatives_parser(count: usize) -> impl Fn(&Value) -> Result<Alternatives, String> {
    move |v: &Value| {
        let mut a: Alternatives = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
        if a.alternatives.is_empty() || a.alternatives.iter().any(|x| x.team.agents.is_empty()) {
            return Err("a team had no agents".to_string());
        }
        a.alternatives.truncate(count);
        Ok(a)
    }
}

/// One sketch as a laid-out team draft, with what was dropped and the team's issues.
fn offered(st: &AppState, x: Alternative, kinds: Kinds, folder: Option<&str>) -> Offered {
    let c = sketch::to_draft(&x.team, kinds, folder);
    let issues = crate::teams::issues_for(st, &c.team);
    Offered { label: x.label, rationale: x.rationale, draft: c.team, dropped: c.dropped, issues }
}

#[derive(Serialize)]
pub(super) struct Offered {
    label: String,
    rationale: String,
    pub(super) draft: model::TeamDraft,
    dropped: Vec<String>,
    issues: Vec<model::DraftIssue>,
}

#[derive(Serialize)]
struct Offer {
    alternatives: Vec<Offered>,
    cli: String,
    model: Option<String>,
}

async fn teams(State(st): State<AppState>, Json(req): Json<TeamsReq>) -> Response {
    let count = if req.count == 1 { 1 } else { 3 };
    let prompt = teams_prompt(&req.prompt, &req.options, &req.answers, count);
    let result = call(&st, &req.options, prompt, sketch::teams_schema(count), alternatives_parser(count)).await.map(|a| {
        let kinds = Kinds::from_option(req.options.kinds.as_deref());
        let folder = req.options.folder.as_deref().map(str::trim).filter(|f| !f.is_empty());
        let alternatives = a.alternatives.into_iter().map(|x| offered(&st, x, kinds, folder)).collect();
        Offer { alternatives, cli: req.options.cli.clone(), model: req.options.model.clone() }
    });
    respond(result)
}
