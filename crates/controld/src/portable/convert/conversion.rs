use super::{
    ApiError, Deserialize, EventBody, Provenance, Serialize, Source, TokenTotals, Value, claude,
    codex, team, write,
};

/// One uploaded file: its name (`agent-<id>.meta.json` and the like matter) and its text.
#[derive(Clone, Debug, Deserialize)]
pub struct Upload {
    pub name: String,
    pub text: String,
}

/// One step of an agent's thread.
#[derive(Clone, Debug)]
pub enum Ev {
    /// An event exactly as the agent's own line says it, with how we know it.
    Body(EventBody, Provenance),
    /// A message the agent was given: the person's prompt, or (`from`) its parent's task.
    Prompt { text: String, from: Option<String> },
}

#[derive(Clone, Debug)]
pub struct Step {
    pub at_ms: i64,
    pub ev: Ev,
}

/// One agent of the session, as read from its file(s).
#[derive(Clone, Debug, Default)]
pub struct Thread {
    /// Stable within the upload: "main", or the subagent's or thread's own id.
    pub key: String,
    pub name: Option<String>,
    /// The parent thread's key.
    pub parent: Option<String>,
    /// The live driver name: "claude-stream" or "codex-app-server".
    pub runtime: &'static str,
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    /// Every model seen, in order, repeats kept.
    pub models: Vec<String>,
    /// What its parent asked it to do (subagents).
    pub task: Option<String>,
    pub agent_type: Option<String>,
    pub steps: Vec<Step>,
}

/// What a reader found: the threads, warnings, and subagents mentioned whose files were not uploaded.
#[derive(Debug, Default)]
pub struct Read {
    pub threads: Vec<Thread>,
    pub warnings: Vec<String>,
    pub missing: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Preview {
    pub source: Source,
    pub agents: usize,
    pub subagents: usize,
    pub started_ms: i64,
    pub ended_ms: Option<i64>,
    pub tokens: TokenTotals,
    pub missing: Vec<String>,
    pub unknown_lines: usize,
}

#[derive(Debug)]
pub struct Converted {
    pub preview: Preview,
    /// The `.sculptured-session.jsonl` text, ready for `/api/runs/import`.
    pub session: String,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Claude,
    Codex,
    Meta,
}

/// The session files as one run, or why they can't be read.
pub fn convert(files: &[Upload]) -> Result<Converted, ApiError> {
    // The same file chosen twice (a folder added again) counts once.
    let files: Vec<Upload> = files
        .iter()
        .enumerate()
        .filter(|(i, f)| {
            !files[i + 1..]
                .iter()
                .any(|g| g.name == f.name && g.text == f.text)
        })
        .map(|(_, f)| f.clone())
        .collect();
    let files = files.as_slice();
    let kinds = files.iter().map(kind_of).collect::<Result<Vec<_>, _>>()?;
    let source = match (kinds.contains(&Kind::Claude), kinds.contains(&Kind::Codex)) {
        (true, true) => {
            return Err(invalid(
                "These files are from different tools; import one session at a time",
                1,
                "Claude Code and Codex files",
            ));
        }
        (true, false) => Source::ClaudeCode,
        (false, true) => Source::Codex,
        (false, false) => {
            return Err(invalid(
                "These files hold no session: choose the session's .jsonl file",
                1,
                "no .jsonl file",
            ));
        }
    };
    let read = if source == Source::Codex {
        codex::read(files)?
    } else {
        claude::read(files)?
    };
    let team = team::rebuild(&read.threads, source);
    let (session, preview) = write::write(&read.threads, source, read.missing, Some(team));
    Ok(Converted {
        preview,
        session,
        warnings: read.warnings,
    })
}

/// Which tool wrote the file, from its first lines (shape, not version numbers).
pub(super) fn kind_of(f: &Upload) -> Result<Kind, ApiError> {
    if f.name.ends_with(".json") {
        return Ok(Kind::Meta);
    }
    let mut lines = f
        .text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(50)
        .peekable();
    let first = lines
        .peek()
        .and_then(|l| serde_json::from_str::<Value>(l).ok());
    if first.is_some_and(|v| v["type"] == "session_meta" && v["payload"].is_object()) {
        return Ok(Kind::Codex);
    }
    for l in lines {
        let Ok(v) = serde_json::from_str::<Value>(l) else {
            continue;
        };
        let message = (v["type"] == "user" || v["type"] == "assistant") && v["message"].is_object();
        if message || v["sessionId"].is_string() {
            return Ok(Kind::Claude);
        }
    }
    let first: String = f
        .text
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(120)
        .collect();
    Err(invalid(
        "This isn't a Claude Code or Codex session file",
        1,
        &format!("{}: {first}", f.name),
    ))
}

pub(crate) fn invalid(message: &str, line: usize, detail: &str) -> ApiError {
    ApiError::unprocessable(
        "session_file_invalid",
        message.to_string(),
        vec![crate::portable::convert::at_line(line, detail.to_string())],
    )
}

#[derive(Deserialize)]
pub(crate) struct ConvertReq {
    pub(super) files: Vec<Upload>,
}

#[derive(Serialize)]
pub(super) struct ConvertReply {
    pub(super) preview: Preview,
    pub(super) warnings: Vec<String>,
    pub(super) session: String,
}

/// `POST /api/runs/convert`: the session files as a run file, with what was found. Nothing is stored;
/// the browser sends `session` to `/api/runs/import` once the person has seen the preview.
pub(crate) async fn post_convert(
    crate::extract::Json(req): crate::extract::Json<ConvertReq>,
) -> axum::response::Response {
    crate::error::respond(convert(&req.files).map(|c| ConvertReply {
        preview: c.preview,
        warnings: c.warnings,
        session: c.session,
    }))
}
