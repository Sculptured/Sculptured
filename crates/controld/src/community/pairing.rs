//! `/api/community-link/pair` (C5): sign in to Community with a community-scope device key. controld
//! polls the gateway itself and saves the key once the person approves the code.
use super::{CommunityLink, CommunityPairStarted, CommunityPairState, CommunityPairStatus, creds, saved_origin};
use crate::device_pairing::{self, Poll, StartError};
use crate::extract::Json;
use crate::state::AppState;
use crate::ApiError;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/api/community-link/pair", post(pair).get(status))
}

/// The gateway's own refusals of a pairing that the page has words for (C4); anything else is a bad answer.
const PASSED_ON: [&str; 4] = ["bad_scope", "too_many_attempts", "community_unavailable", "rate_limited"];

fn start_error(e: StartError) -> ApiError {
    match e {
        StartError::Network(_) => ApiError::bad_gateway("community_unreachable", "Could not reach the Sculptured server"),
        StartError::Refused { status, code } => match PASSED_ON.iter().find(|c| **c == code) {
            Some(c) => ApiError::new(StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY), c, "The Sculptured server refused this sign-in"),
            None => ApiError::bad_gateway("community_bad_answer", "The Sculptured server answered in a way this app does not accept"),
        },
    }
}

fn computer_name() -> String {
    let name = sysinfo::System::host_name().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "This computer".into());
    name.chars().take(80).collect()
}

/// Starts a pairing (replacing any earlier one) and answers the code to show.
async fn pair(State(st): State<AppState>) -> Result<Json<CommunityPairStarted>, ApiError> {
    let (Some(link), Some(origin)) = (st.community.clone(), saved_origin(&st)?) else {
        return Err(ApiError::conflict("community_off", "Community is off on this computer"));
    };
    let started = device_pairing::start(&link.http, &origin, &computer_name(), std::env::consts::OS, Some("community")).await.map_err(start_error)?;
    let generation = link.begin_pairing();
    let answer = CommunityPairStarted { user_code: started.user_code.clone(), verify_url: started.verify_url.clone(), expires_in: started.expires_in };
    tokio::spawn(poll(link, origin, started, generation));
    Ok(Json(answer))
}

/// Polls every `interval` seconds until approved, expired (also at `expires_in`) or turned down.
/// It stops as soon as another pairing starts or Community is turned off.
async fn poll(link: Arc<CommunityLink>, origin: String, s: device_pairing::Started, generation: u64) {
    let deadline = Instant::now() + Duration::from_secs(s.expires_in);
    let every = Duration::from_secs(s.interval.clamp(1, 30));
    loop {
        tokio::time::sleep(every).await;
        if !link.is_current(generation) {
            return;
        }
        if Instant::now() > deadline {
            link.finish_pairing(generation, CommunityPairState::Expired);
            return;
        }
        let outcome = match device_pairing::poll_once(&link.http, &origin, &s.device_code).await {
            Ok(Poll::Pending) | Err(_) => continue,
            Ok(Poll::Expired) => CommunityPairState::Expired,
            Ok(Poll::Denied) => CommunityPairState::Denied,
            Ok(Poll::Approved { device_id, key }) => approved(&link, generation, creds::Credentials { origin: origin.clone(), device_id, key }).await,
        };
        link.finish_pairing(generation, outcome);
        return;
    }
}

/// Saves the approved key, unless this pairing was replaced or Community turned off meanwhile: then
/// the key is revoked on the server, never left there unused.
async fn approved(link: &CommunityLink, generation: u64, c: creds::Credentials) -> CommunityPairState {
    if !link.is_current(generation) {
        link.revoke(&c).await;
        return CommunityPairState::Expired;
    }
    match creds::save(link.data_dir(), &c) {
        Ok(()) => {
            link.forget_account();
            CommunityPairState::Approved
        }
        Err(e) => {
            tracing::warn!(error = %e, "could not save community.json");
            CommunityPairState::Expired
        }
    }
}

/// The pairing's state; without one, `approved` when signed in, else `expired`.
async fn status(State(st): State<AppState>) -> Result<Json<CommunityPairStatus>, ApiError> {
    let Some(link) = st.community.clone() else { return Ok(Json(CommunityPairStatus { status: CommunityPairState::Expired })) };
    let signed_in = saved_origin(&st)?.is_some_and(|o| link.creds_for(&o).is_some());
    let status = link.pairing_state().unwrap_or(if signed_in { CommunityPairState::Approved } else { CommunityPairState::Expired });
    Ok(Json(CommunityPairStatus { status }))
}
