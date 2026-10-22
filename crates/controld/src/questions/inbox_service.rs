use super::{
    Actor, Answer, ApiError, AppState, Deserialize, Duration, EventBody, HashSet, Json, LazyLock,
    Mutex, Path, Provenance, QuestionRow, QuestionSource, QuestionView, Response, Router,
    Serialize, State, Subject,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/inbox", crate::questions::get(get_inbox))
        .route(
            "/api/seats/{id}/questions",
            crate::questions::get(get_seat_questions),
        )
        .route(
            "/api/teams/{id}/questions",
            crate::questions::get(get_team_questions),
        )
        .route(
            "/api/questions/{id}/answer",
            crate::questions::post(post_answer),
        )
        .route(
            "/api/questions/{id}/acknowledge",
            crate::questions::post(post_acknowledge),
        )
        .route(
            "/api/questions/{id}/send-again",
            crate::questions::post(post_send_again),
        )
}

/// Questions being resolved right now (answer or timeout). Question ids are unique, so one set serves every AppState.
pub(super) static RESOLVING: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Mutex::default);

pub(super) struct Claim(pub(super) String);
impl Drop for Claim {
    fn drop(&mut self) {
        RESOLVING
            .lock()
            .expect("RESOLVING mutex must not be poisoned")
            .remove(&self.0);
    }
}
pub(super) fn claim(id: &str) -> Option<Claim> {
    RESOLVING
        .lock()
        .expect("RESOLVING mutex must not be poisoned")
        .insert(id.to_string())
        .then(|| Claim(id.to_string()))
}

/// The question's claim for a seat tool (Task 6) that must not race an answer or expiry in flight.
/// None when someone still holds it after the usual wait.
pub(crate) async fn tool_claim(id: &str) -> Option<impl Sized> {
    wait_for_claim(id).await
}

pub(super) const DECISION_CHARS: usize = 500;
pub(super) const RESTARTED: &str = "Sculptured restarted while the agent waited for an answer; if the agent still waits, its question is asked again";

pub(super) fn human() -> Actor {
    Actor::Human("local".into())
}
pub(super) fn timeout_actor() -> Actor {
    Actor::System("timeout".into())
}

pub(super) fn load(st: &AppState, id: &str) -> Result<QuestionRow, ApiError> {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    crate::questions::question(store.connection(), id)?
        .ok_or_else(|| ApiError::not_found(format!("no question {id:?}")))
}

pub(super) fn record(
    st: &AppState,
    seat: &str,
    actor: Actor,
    provenance: Provenance,
    body: EventBody,
) -> Result<(), ApiError> {
    st.local_event(actor, Subject::Seat(seat.to_string()), provenance, body)?;
    Ok(())
}

/// The view, with the interpreter's reading of a permission prompt (≈ heuristic).
pub(crate) fn view(st: &AppState, q: QuestionRow) -> QuestionView {
    let consequence = match (q.source, &q.prompt.tool, &q.prompt.tool_input) {
        (QuestionSource::NativePermission, Some(tool), Some(input)) => {
            let cwd = st
                .store
                .lock()
                .expect("store mutex must not be poisoned")
                .seat(&q.seat)
                .ok()
                .flatten()
                .and_then(|s| s.cwd);
            crate::interpret::consequence(tool, input, cwd.as_deref().map(std::path::Path::new))
        }
        _ => Vec::new(),
    };
    q.into_view(consequence)
}

/// How long an answer waits for another answer, or the timeout, to finish with the question. Longer
/// than the 30 s a host has to reply, so a slow delivery that fails still leaves time to answer.
pub(super) const CLAIM_WAIT: Duration = Duration::from_secs(45);

pub(super) fn claim_wait() -> Duration {
    #[cfg(feature = "test-seams")]
    if let Some(d) = crate::seams::override_ms(&crate::seams::CLAIM_WAIT_MS) {
        return d;
    }
    CLAIM_WAIT
}

/// Waits for the question's claim; None when someone else still holds it after `CLAIM_WAIT`.
pub(super) async fn wait_for_claim(id: &str) -> Option<Claim> {
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
pub(super) fn record_or_log(
    st: &AppState,
    seat: &str,
    (actor, provenance): (Actor, Provenance),
    body: EventBody,
    question: &str,
    what: &str,
) {
    if let Err(e) = record(st, seat, actor, provenance, body) {
        tracing::error!(%question, error = %e.message, "{what}");
    }
}

pub(super) async fn get_inbox(State(st): State<AppState>) -> Response {
    crate::questions::respond(crate::questions::inbox(&st))
}

pub(super) async fn get_seat_questions(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let rows = crate::questions::questions_where(
        st.store
            .lock()
            .expect("store mutex must not be poisoned")
            .connection(),
        "seat = ?1 ORDER BY opened_at_ms DESC, opened_seq DESC LIMIT 200",
        [&id],
    );
    crate::questions::respond(
        rows.map_err(ApiError::from)
            .map(|rows| rows.into_iter().map(|q| view(&st, q)).collect::<Vec<_>>()),
    )
}

/// Every question the team's agents asked, newest first: the project's Questions page.
pub(super) async fn get_team_questions(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let rows = crate::questions::questions_where(
        st.store
            .lock()
            .expect("store mutex must not be poisoned")
            .connection(),
        "rig = ?1 ORDER BY opened_at_ms DESC, opened_seq DESC LIMIT 500",
        [&id],
    );
    crate::questions::respond(
        rows.map_err(ApiError::from)
            .map(|rows| rows.into_iter().map(|q| view(&st, q)).collect::<Vec<_>>()),
    )
}

#[derive(Deserialize, Serialize)]
pub(super) struct AnswerReq {
    #[serde(default)]
    pub(super) request_id: String,
    pub(super) answer: Answer,
}

#[derive(Deserialize, Serialize)]
pub(super) struct AckReq {
    #[serde(default)]
    pub(super) request_id: String,
}

pub(super) async fn post_answer(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<AnswerReq>,
) -> Response {
    let fp = crate::questions::fingerprint(&format!("answer:{id}"), &req);
    let request_id = req.request_id.clone();
    let st2 = st.clone();
    crate::questions::idempotent(&st, &request_id, fp, move || async move {
        crate::questions::to_value(crate::questions::answer(&st2, &id, req.answer).await?)
    })
    .await
}

pub(super) async fn post_acknowledge(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<AckReq>,
) -> Response {
    let fp = crate::questions::fingerprint(&format!("acknowledge:{id}"), &req);
    let request_id = req.request_id.clone();
    let st2 = st.clone();
    crate::questions::idempotent(&st, &request_id, fp, move || async move {
        crate::questions::to_value(crate::questions::acknowledge(&st2, &id)?)
    })
    .await
}

pub(super) async fn post_send_again(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<AckReq>,
) -> Response {
    let fp = crate::questions::fingerprint(&format!("send-again:{id}"), &req);
    let request_id = req.request_id.clone();
    let st2 = st.clone();
    crate::questions::idempotent(&st, &request_id, fp, move || async move {
        crate::questions::to_value(crate::questions::send_again(&st2, &id).await?)
    })
    .await
}
