//! `sculptured.session/v1`: a run as JSON lines — a header, then each event in order with its origin.
use crate::collaboration::history;
use crate::error::ApiError;
use crate::extract::Path;
use crate::state::AppState;
use axum::extract::State;
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::response::{IntoResponse, Response};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const SESSION_FORMAT: &str = "sculptured.session/v1";

/// How a line came to be: recorded by Sculptured, converted from another tool's file, or inferred by AI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Origin {
    Observed,
    Converted,
    AiReconstructed,
}

/// Where the run happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Source {
    #[serde(rename = "sculptured")]
    Sculptured,
    #[serde(rename = "imported:claude-code")]
    ClaudeCode,
    #[serde(rename = "imported:codex")]
    Codex,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SessionAgent {
    pub id: String,
    pub name: Option<String>,
    /// The agent that spawned this one, if any.
    pub parent: Option<String>,
    pub runtime: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TokenTotals {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[schemars(title = "SessionHeader")]
pub struct SessionHeader {
    /// Always "sculptured.session/v1".
    #[schemars(regex(pattern = r"^sculptured\.session/v1$"))]
    pub format: String,
    pub source: Source,
    /// The team as of the run's start, as a MADL document; null when it was not recorded.
    pub team: Option<madl::model::Document>,
    pub team_id: String,
    pub started_ms: i64,
    pub ended_ms: Option<i64>,
    pub agents: Vec<SessionAgent>,
    pub tokens: TokenTotals,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionLine {
    pub origin: Origin,
    pub event: model::EventRecord,
}

/// Rows read per store lock while exporting, so a long run never holds the store for long.
const EXPORT_CHUNK: i64 = 2000;

/// The run of `team` that started at `seq`, as a session file: the header, then every event in
/// order, each exactly as recorded (an event this build can't read is still written unchanged).
/// The store is locked briefly for the header, then once per chunk of events.
pub fn export_run(st: &AppState, team: &str, seq: i64) -> Result<String, ApiError> {
    export_events(st, team, seq, None).map(|(text, _)| text)
}

/// The run as `sculptured.session/v1` with only the events whose type is in `keep` (all when None),
/// and when the run ended (None while it goes).
pub(crate) fn export_events(st: &AppState, team: &str, seq: i64, keep: Option<&[&str]>) -> Result<(String, Option<i64>), ApiError> {
    let header = {
        let store = st.store.lock().unwrap();
        let (run, end) = history::bounds(&store, team, seq)?;
        let detail = history::load_detail(&store, team, seq)?;
        let snapshot = store.run_team_snapshot(team, seq)?;
        let next_start = store.event_at_ms(end)?;
        let tokens = store.run_tokens(team, seq, end)?;
        let header = SessionHeader {
            format: SESSION_FORMAT.into(),
            source: Source::Sculptured,
            team: snapshot.and_then(|j| serde_json::from_str::<super::team::PortableTeam>(&j).ok()).map(|t| super::madl::to_madl(&t)),
            team_id: team.to_string(),
            started_ms: run.started_ms,
            ended_ms: detail.stopped_ms.or(next_start),
            agents: detail.agents.into_iter().map(|a| SessionAgent { id: a.id, name: a.name, parent: a.parent, runtime: a.runtime }).collect(),
            tokens,
        };
        (header, end)
    };
    let (header, end) = header;
    let ended_ms = header.ended_ms;
    let mut out = serde_json::to_string(&header).expect("header serializes");
    out.push('\n');
    let mut from = seq;
    loop {
        let rows = st.store.lock().unwrap().run_events(team, from, end, EXPORT_CHUNK)?;
        let Some((last, _)) = rows.last() else { break };
        from = last + 1;
        for (_, raw) in rows.iter().filter(|(_, raw)| keep.is_none_or(|k| k.contains(&event_type(raw)))) {
            out.push_str(r#"{"origin":"observed","event":"#);
            out.push_str(raw);
            out.push_str("}\n");
        }
        if (rows.len() as i64) < EXPORT_CHUNK {
            break;
        }
    }
    Ok((out, ended_ms))
}

/// An event's `body.type`, from its stored JSON.
fn event_type(raw: &str) -> &str {
    const AT: &str = r#""body":{"type":""#;
    let Some(i) = raw.find(AT) else { return "" };
    let rest = &raw[i + AT.len()..];
    rest.split('"').next().unwrap_or("")
}

pub(crate) fn download(name: &str, text: String) -> Response {
    let disposition = format!("attachment; filename=\"{name}.sculptured-session.jsonl\"");
    ([(CONTENT_TYPE, "application/x-ndjson".to_string()), (CONTENT_DISPOSITION, disposition)], text).into_response()
}

pub(super) async fn get_session(State(st): State<AppState>, Path((team, seq)): Path<(String, i64)>) -> Response {
    let safe: String = team.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    let text = if seq < 0 { export_imported(&st.store.lock().unwrap(), &team, -seq) } else { export_run(&st, &team, seq) };
    match text {
        Ok(text) => download(&format!("{safe}-run-{seq}"), text),
        Err(e) => e.into_response(),
    }
}

// ---- Imported runs: kept apart from the live `events` log, so nothing that drives agents sees them.
// In the History API an imported run's `start_seq` is minus its `imported_runs.id`.

fn session_invalid(diagnostics: Vec<rigspec::Diagnostic>) -> ApiError {
    let n = diagnostics.len();
    ApiError::unprocessable("session_file_invalid", format!("This run file can't be read ({n} {})", if n == 1 { "problem" } else { "problems" }), diagnostics)
}

pub(crate) fn at_line(n: usize, message: impl Into<String>) -> rigspec::Diagnostic {
    rigspec::Diagnostic { path: format!("line {n}"), severity: rigspec::Severity::Error, message: message.into() }
}

/// Parses a session file; up to 20 problems, each by line number.
pub fn parse_session(text: &str) -> Result<(SessionHeader, Vec<(String, SessionLine)>), ApiError> {
    let mut lines = text.lines().enumerate().map(|(i, l)| (i + 1, l)).filter(|(_, l)| !l.trim().is_empty());
    let Some((n, first)) = lines.next() else { return Err(session_invalid(vec![at_line(1, "The file is empty")])) };
    let raw: serde_json::Value = serde_json::from_str(first).map_err(|e| session_invalid(vec![at_line(n, format!("Not JSON: {e}"))]))?;
    if raw.get("format").and_then(|f| f.as_str()) != Some(SESSION_FORMAT) {
        return Err(session_invalid(vec![at_line(n, format!("The first line must be a \"{SESSION_FORMAT}\" header"))]));
    }
    let header: SessionHeader = serde_json::from_value(raw).map_err(|e| session_invalid(vec![at_line(n, e.to_string())]))?;
    let mut out = Vec::new();
    let mut problems = Vec::new();
    for (n, l) in lines {
        match serde_json::from_str::<SessionLine>(l) {
            Ok(line) => out.push((l.to_string(), line)),
            Err(e) if problems.len() < 20 => problems.push(at_line(n, e.to_string())),
            Err(_) => {}
        }
    }
    if !problems.is_empty() {
        return Err(session_invalid(problems));
    }
    Ok((header, out))
}

/// Adds the file's run as a read-only past run of a new team (built from the header's team when it
/// can be). Never writes the live `events` log.
/// Returns the new team, the run, and the team's issues: its warnings, or why it was left out.
pub fn import_run(st: &AppState, text: &str) -> Result<(model::TeamDraft, history::DeploymentRun, Vec<model::DraftIssue>), ApiError> {
    let (mut header, lines) = parse_session(text)?;
    let mut warnings = Vec::new();
    let read = |doc: &madl::model::Document| super::madl::read_document(doc).and_then(|i| super::team::check(st, i.file));
    let team = header.team.as_ref().and_then(|t| match read(t) {
        Ok((file, issues)) => {
            warnings = issues;
            Some(file)
        }
        Err(e) => {
            warnings = left_out(e);
            None
        }
    });
    let file = team.unwrap_or_else(|| super::team::PortableTeam {
            team: model::TeamDraft { claude_setup: Default::default(), protection: Default::default(), safety: vec![], components: vec![], budget: Default::default(), input: None, schedules: vec![], decision_log: false, limits: Default::default(),
                workflow: None,
                folder: None,
                id: String::new(),
                name: format!("{} (imported run)", header.team_id),
                groups: vec![],
                agents: vec![],
                connections: vec![],
                models: Default::default(),
                revision: 0,
            },
            settings: Default::default(),
        });
    let draft = super::team::create(st, file)?;
    crate::analytics::emit(st, "team.created", serde_json::json!({ "template": "run-import" }));
    if header.source != Source::Sculptured {
        // Only that it happened, which tool, and how many agents: never content.
        crate::analytics::emit(st, "run.imported", serde_json::json!({ "source": header.source, "agents": header.agents.len() }));
    }
    header.team_id = draft.id.clone();
    let rows: Vec<(i64, String, &str)> = lines.iter().map(|(raw, l)| (l.event.seq, l.event.subject.key(), raw.as_str())).collect();
    let header_json = serde_json::to_string(&header).expect("header serializes");
    let run = st.store.lock().unwrap().insert_imported_run(&draft.id, &header_json, &rows)?;
    Ok((draft, imported_run(run, &header), warnings))
}

/// Why the run's team was left out, as warnings on the (empty) team that holds the run.
fn left_out(e: ApiError) -> Vec<model::DraftIssue> {
    let team = |code: &str, message: String| model::DraftIssue {
        element: model::ElementRef { kind: model::ElementKind::Team, id: String::new() },
        severity: model::IssueSeverity::Warning,
        code: code.into(),
        message,
        detail: None,
        spec_path: None,
    };
    let mut out = vec![team("run-team-left-out", format!("The run's team could not be imported, so this team is empty: {}", e.message))];
    out.extend(e.issues.unwrap_or_default());
    out.extend(e.diagnostics.unwrap_or_default().into_iter().map(|d| team("run-team-invalid", format!("{}: {}", d.path, d.message))));
    out
}

fn imported_run(id: i64, h: &SessionHeader) -> history::DeploymentRun {
    let source = serde_json::to_value(h.source).ok().and_then(|v| v.as_str().map(str::to_string));
    history::DeploymentRun { start_seq: -id, started_ms: h.started_ms, initial_agents: h.agents.len() as u32, source }
}

fn header_of(store: &crate::store::Store, rig: &str, id: i64) -> Result<SessionHeader, ApiError> {
    let text = store.imported_run_header(rig, id)?.ok_or_else(|| ApiError::not_found("Deployment run not found"))?;
    serde_json::from_str(&text).map_err(|_| ApiError::bad_request("invalid_history", "Saved run is unreadable"))
}

pub(crate) fn imported_runs(store: &crate::store::Store, rig: &str) -> Result<Vec<history::DeploymentRun>, ApiError> {
    let mut out = Vec::new();
    for (id, text) in store.imported_run_headers(rig)? {
        if let Ok(h) = serde_json::from_str::<SessionHeader>(&text) {
            out.push(imported_run(id, &h));
        }
    }
    Ok(out)
}

pub(crate) fn imported_detail(store: &crate::store::Store, rig: &str, id: i64) -> Result<history::DeploymentHistory, ApiError> {
    let h = header_of(store, rig, id)?;
    let mut agents = std::collections::BTreeMap::new();
    for a in &h.agents {
        let mut d = history::blank(a.id.clone());
        (d.name, d.parent, d.runtime) = (a.name.clone(), a.parent.clone(), a.runtime.clone());
        agents.insert(a.id.clone(), d);
    }
    for raw in store.imported_lines(id)? {
        if let Ok(line) = serde_json::from_str::<SessionLine>(&raw) {
            history::fold(&mut agents, line.event);
        }
    }
    let mut agents: Vec<_> = agents.into_values().collect();
    agents.sort_by_key(|a| (a.launched_ms.unwrap_or(i64::MAX), a.id.clone()));
    Ok(history::DeploymentHistory { run: imported_run(id, &h), agents, has_snapshot: h.team.is_some(), stopped_ms: h.ended_ms, summary_truncated: false })
}

pub(crate) fn imported_events(store: &crate::store::Store, rig: &str, id: i64, before: Option<i64>, seat: Option<&str>) -> Result<history::DeploymentEvents, ApiError> {
    header_of(store, rig, id)?;
    let rows = store.imported_lines_page(id, before.unwrap_or(i64::MAX), seat, 201)?;
    let has_more = rows.len() > 200;
    let events = rows.into_iter().take(200).filter_map(|raw| serde_json::from_str::<SessionLine>(&raw).ok().map(|l| l.event)).collect();
    Ok(history::DeploymentEvents { events, has_more })
}

/// An imported run's file: its stored header and lines, unchanged.
pub(crate) fn export_imported(store: &crate::store::Store, rig: &str, id: i64) -> Result<String, ApiError> {
    let h = header_of(store, rig, id)?;
    let mut out = serde_json::to_string(&h).expect("header serializes");
    out.push('\n');
    for raw in store.imported_lines(id)? {
        out.push_str(&raw);
        out.push('\n');
    }
    Ok(out)
}

#[derive(Serialize)]
struct ImportedRun {
    team: crate::views::TeamRecord,
    run: history::DeploymentRun,
    warnings: Vec<model::DraftIssue>,
}

pub(super) async fn post_import(State(st): State<AppState>, body: String) -> Response {
    let result = import_run(&st, &body).and_then(|(draft, run, warnings)| Ok(ImportedRun { team: crate::teams::record(&st, draft)?, run, warnings }));
    crate::error::respond(result)
}
