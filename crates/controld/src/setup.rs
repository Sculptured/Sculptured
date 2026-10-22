//! Setup progress: which steps of the setup flow (Welcome, Computer, Models, More computers, First
//! project) the person finished or skipped. Kept on the server, so Setup resumes on any device.
use crate::error::{ApiError, respond};
use crate::extract::Json;
use crate::state::AppState;
use axum::Router;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum SetupStep {
    Welcome,
    Computer,
    Models,
    /// More computers and clusters (optional).
    Computers,
    /// The first project tour (optional).
    Tour,
}

impl SetupStep {
    const ALL: [SetupStep; 5] = [
        SetupStep::Welcome,
        SetupStep::Computer,
        SetupStep::Models,
        SetupStep::Computers,
        SetupStep::Tour,
    ];

    pub fn id(self) -> &'static str {
        match self {
            SetupStep::Welcome => "welcome",
            SetupStep::Computer => "computer",
            SetupStep::Models => "models",
            SetupStep::Computers => "computers",
            SetupStep::Tour => "tour",
        }
    }

    pub fn parse(id: &str) -> Option<SetupStep> {
        SetupStep::ALL.into_iter().find(|s| s.id() == id)
    }
}

/// Each step at most once, and never both done and skipped.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SetupProgress {
    #[serde(default)]
    pub done: Vec<SetupStep>,
    #[serde(default)]
    pub skipped: Vec<SetupStep>,
}

/// `GET/PUT /api/setup`.
pub fn routes() -> Router<AppState> {
    Router::new().route("/api/setup", axum::routing::get(get_setup).put(put_setup))
}

async fn get_setup(State(st): State<AppState>) -> Response {
    respond(
        st.store
            .lock()
            .unwrap()
            .setup_progress()
            .map_err(ApiError::from),
    )
}

#[derive(Deserialize)]
struct SaveSetup {
    #[serde(default)]
    done: Vec<String>,
    #[serde(default)]
    skipped: Vec<String>,
}

/// The steps named, each once, in the order given; Err names one Sculptured doesn't know.
fn steps(ids: &[String], except: &[SetupStep]) -> Result<Vec<SetupStep>, String> {
    let mut out = Vec::new();
    for id in ids {
        let step =
            SetupStep::parse(id.trim()).ok_or_else(|| format!("There is no setup step {id:?}"))?;
        if !out.contains(&step) && !except.contains(&step) {
            out.push(step);
        }
    }
    Ok(out)
}

/// Replaces the saved progress.
async fn put_setup(State(st): State<AppState>, Json(req): Json<SaveSetup>) -> Response {
    let p = match steps(&req.done, &[]).and_then(|done| {
        Ok(SetupProgress {
            skipped: steps(&req.skipped, &done)?,
            done,
        })
    }) {
        Ok(p) => p,
        Err(m) => return ApiError::bad_request("setup_step_invalid", m).into_response(),
    };
    let saved = st.store.lock().unwrap().save_setup_progress(&p);
    respond(saved.map(|()| p).map_err(ApiError::from))
}
