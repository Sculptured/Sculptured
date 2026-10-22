//! Idempotent mutating calls: each `request_id` acts once, and a retry replays the stored receipt.
use crate::error::ApiError;
use crate::state::AppState;
use crate::views::Receipt;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use tokio::sync::OwnedMutexGuard;

pub(crate) fn fingerprint<T: Serialize>(route: &str, req: &T) -> String {
    format!(
        "{route}:{}",
        serde_json::to_string(req).expect("request serializes")
    )
}

fn replay(r: &Receipt) -> Response {
    let status = StatusCode::from_u16(r.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        r.body.clone(),
    )
        .into_response()
}

type RequestLocks = Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>;

/// Holds one request_id's lock. Dropping it, on any path (replay, error, success,
/// panic), removes the map entry unless another request is still waiting on it.
struct RequestSlot {
    locks: RequestLocks,
    request_id: String,
    held: OwnedMutexGuard<()>,
}

impl Drop for RequestSlot {
    fn drop(&mut self) {
        // Cleanup must not recover potentially inconsistent state or panic during another unwind.
        let Ok(mut locks) = self.locks.lock() else {
            return;
        };
        let mine = OwnedMutexGuard::mutex(&self.held);
        // 2 = the map's reference + this guard's. More means a duplicate is waiting.
        if locks
            .get(&self.request_id)
            .is_some_and(|l| Arc::ptr_eq(l, mine) && Arc::strong_count(l) == 2)
        {
            locks.remove(&self.request_id);
        }
    }
}

impl RequestSlot {
    /// Waits for the request id's lock (a concurrent duplicate holds it until it is done).
    async fn take(st: &AppState, request_id: &str) -> Self {
        let lock = st
            .request_locks
            .lock()
            .expect("request_locks mutex must not be poisoned")
            .entry(request_id.to_string())
            .or_default()
            .clone();
        RequestSlot {
            locks: st.request_locks.clone(),
            request_id: request_id.to_string(),
            held: lock.lock_owned().await,
        }
    }
}

/// The response for a request id already used: its receipt replayed, or a conflict when it was
/// used for a different request. None for a new request id.
fn earlier(st: &AppState, request_id: &str, fingerprint: &str) -> Option<Response> {
    let stored = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .receipt(request_id);
    match stored {
        Err(e) => Some(ApiError::from(e).into_response()),
        Ok(Some(r)) if r.fingerprint != fingerprint => Some(
            ApiError::conflict(
                "request_id_reused",
                "request_id was already used for a different request",
            )
            .into_response(),
        ),
        Ok(Some(r)) => Some(replay(&r)),
        Ok(None) => None,
    }
}

/// Executes `f` at most once per `request_id`. Concurrent duplicates wait on a
/// per-id lock and then replay the stored receipt. Errors are stored too, so a
/// retry returns the same failure instead of acting twice.
///
/// The action runs in a spawned task that also saves the receipt and holds the
/// lock. If the client disconnects and hyper drops this future, the action still
/// finishes and its receipt is still saved, so a retry replays it.
pub(crate) async fn idempotent<F, Fut>(
    st: &AppState,
    request_id: &str,
    fingerprint: String,
    f: F,
) -> Response
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<Value, ApiError>> + Send + 'static,
{
    if request_id.trim().is_empty() {
        return ApiError::bad_request(
            "request_id_required",
            "request_id is required for every mutating call",
        )
        .into_response();
    }
    let slot = RequestSlot::take(st, request_id).await;
    if let Some(done) = earlier(st, request_id, &fingerprint) {
        return done;
    }
    let action = f();
    let st2 = st.clone();
    let task = tokio::spawn(async move {
        let (status, body) = match action.await {
            Ok(v) => (StatusCode::OK, v),
            Err(e) => (e.status, e.body()),
        };
        let receipt = Receipt {
            fingerprint,
            status: status.as_u16(),
            body: body.to_string(),
        };
        let saved = st2
            .store
            .lock()
            .expect("store mutex must not be poisoned")
            .save_receipt(&slot.request_id, &receipt);
        drop(slot);
        saved.map(|()| receipt)
    });
    match task.await {
        Ok(Ok(receipt)) => replay(&receipt),
        Ok(Err(e)) => ApiError::from(e).into_response(),
        Err(e) => ApiError::internal(format!("request task failed: {e}")).into_response(),
    }
}
