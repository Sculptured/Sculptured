//! Questions and the Needs-you inbox (spec §5.3, §6.5, D8). The first of "answer" and "timeout" wins
//! (a process-wide claim per question); an answer that loses becomes a late, overriding instruction.
mod answers;
mod inbox;
mod watch;

use crate::api::{fingerprint, idempotent};
use crate::error::{respond, to_value, ApiError};
use crate::extract::{Json, Path};
use crate::projections::questions::{question, questions_where, QuestionRow};
use crate::state::AppState;
use crate::views::QuestionView;
use axum::extract::State;
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use model::{Actor, Answer, EventBody, Provenance, QuestionSource, Subject};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

pub use answers::{acknowledge, answer, expire, send_again};
pub use inbox::inbox;
pub use watch::run;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/inbox", get(get_inbox))
        .route("/api/seats/{id}/questions", get(get_seat_questions))
        .route("/api/teams/{id}/questions", get(get_team_questions))
        .route("/api/questions/{id}/answer", post(post_answer))
        .route("/api/questions/{id}/acknowledge", post(post_acknowledge))
        .route("/api/questions/{id}/send-again", post(post_send_again))
}

/// Questions being resolved right now (answer or timeout). Question ids are unique, so one set serves every AppState.
static RESOLVING: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Mutex::default);

struct Claim(String);
impl Drop for Claim {
    fn drop(&mut self) {
        RESOLVING.lock().unwrap().remove(&self.0);
    }
}
fn claim(id: &str) -> Option<Claim> {
    RESOLVING.lock().unwrap().insert(id.to_string()).then(|| Claim(id.to_string()))
}

/// The question's claim for a seat tool (Task 6) that must not race an answer or expiry in flight.
/// None when someone still holds it after the usual wait.
pub(crate) async fn tool_claim(id: &str) -> Option<impl Sized> {
    wait_for_claim(id).await
}

const DECISION_CHARS: usize = 500;
const RESTARTED: &str = "Sculptured restarted while the agent waited for an answer; if the agent still waits, its question is asked again";

fn human() -> Actor {
    Actor::Human("local".into())
}
fn timeout_actor() -> Actor {
    Actor::System("timeout".into())
}

fn load(st: &AppState, id: &str) -> Result<QuestionRow, ApiError> {
    let store = st.store.lock().unwrap();
    question(store.connection(), id)?.ok_or_else(|| ApiError::not_found(format!("no question {id:?}")))
}

fn record(st: &AppState, seat: &str, actor: Actor, provenance: Provenance, body: EventBody) -> Result<(), ApiError> {
    st.local_event(actor, Subject::Seat(seat.to_string()), provenance, body)?;
    Ok(())
}

/// The view, with the interpreter's reading of a permission prompt (≈ heuristic).
pub(crate) fn view(st: &AppState, q: QuestionRow) -> QuestionView {
    let consequence = match (q.source, &q.prompt.tool, &q.prompt.tool_input) {
        (QuestionSource::NativePermission, Some(tool), Some(input)) => {
            let cwd = st.store.lock().unwrap().seat(&q.seat).ok().flatten().and_then(|s| s.cwd);
            crate::interpret::consequence(tool, input, cwd.as_deref().map(std::path::Path::new))
        }
        _ => Vec::new(),
    };
    q.into_view(consequence)
}

/// How long an answer waits for another answer, or the timeout, to finish with the question. Longer
/// than the 30 s a host has to reply, so a slow delivery that fails still leaves time to answer.
const CLAIM_WAIT: Duration = Duration::from_secs(45);

fn claim_wait() -> Duration {
    #[cfg(feature = "test-seams")]
    if let Some(d) = crate::seams::override_ms(&crate::seams::CLAIM_WAIT_MS) {
        return d;
    }
    CLAIM_WAIT
}

/// Waits for the question's claim; None when someone else still holds it after `CLAIM_WAIT`.
async fn wait_for_claim(id: &str) -> Option<Claim> {
    let deadline = tokio::time::Instant::now() + claim_wait();
    loop {
        if let Some(held) = claim(id) {
            return Some(held);
        }
        if tokio::time::Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Records an event about a question's agent, logging (not returning) a failure: `what` says what failed.
fn record_or_log(st: &AppState, seat: &str, (actor, provenance): (Actor, Provenance), body: EventBody, question: &str, what: &str) {
    if let Err(e) = record(st, seat, actor, provenance, body) {
        tracing::error!(%question, error = %e.message, "{what}");
    }
}

async fn get_inbox(State(st): State<AppState>) -> Response {
    respond(inbox(&st))
}

async fn get_seat_questions(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    let rows = questions_where(st.store.lock().unwrap().connection(), "seat = ?1 ORDER BY opened_at_ms DESC, opened_seq DESC LIMIT 200", [&id]);
    respond(rows.map_err(ApiError::from).map(|rows| rows.into_iter().map(|q| view(&st, q)).collect::<Vec<_>>()))
}

/// Every question the team's agents asked, newest first: the project's Questions page.
async fn get_team_questions(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    let rows = questions_where(st.store.lock().unwrap().connection(), "rig = ?1 ORDER BY opened_at_ms DESC, opened_seq DESC LIMIT 500", [&id]);
    respond(rows.map_err(ApiError::from).map(|rows| rows.into_iter().map(|q| view(&st, q)).collect::<Vec<_>>()))
}

#[derive(Deserialize, Serialize)]
struct AnswerReq {
    #[serde(default)]
    request_id: String,
    answer: Answer,
}

#[derive(Deserialize, Serialize)]
struct AckReq {
    #[serde(default)]
    request_id: String,
}

async fn post_answer(State(st): State<AppState>, Path(id): Path<String>, Json(req): Json<AnswerReq>) -> Response {
    let fp = fingerprint(&format!("answer:{id}"), &req);
    let request_id = req.request_id.clone();
    let st2 = st.clone();
    idempotent(&st, &request_id, fp, move || async move { to_value(answer(&st2, &id, req.answer).await?) }).await
}

async fn post_acknowledge(State(st): State<AppState>, Path(id): Path<String>, Json(req): Json<AckReq>) -> Response {
    let fp = fingerprint(&format!("acknowledge:{id}"), &req);
    let request_id = req.request_id.clone();
    let st2 = st.clone();
    idempotent(&st, &request_id, fp, move || async move { to_value(acknowledge(&st2, &id)?) }).await
}

async fn post_send_again(State(st): State<AppState>, Path(id): Path<String>, Json(req): Json<AckReq>) -> Response {
    let fp = fingerprint(&format!("send-again:{id}"), &req);
    let request_id = req.request_id.clone();
    let st2 = st.clone();
    idempotent(&st, &request_id, fp, move || async move { to_value(send_again(&st2, &id).await?) }).await
}
