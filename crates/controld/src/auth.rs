//! Authentication seam. Sculptured assumes a single-user machine until a login
//! system exists, so today every request is allowed. A login system plugs in here
//! and nowhere else. `/host` is exempt: hostd authenticates with its bearer token.
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

pub const HOST_PATH: &str = "/host";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum AuthPolicy {
    /// Today's behaviour: every local request is allowed.
    #[default]
    AllowAll,
    /// Refuses everything the seam covers. Tests use it to prove the seam's coverage.
    DenyAll,
    /// Hosted beta: requests must be signed by the gateway (see `hosted`), with no exemption.
    Gateway { secret: std::sync::Arc<Vec<u8>> },
}

pub async fn auth(State(st): State<AppState>, req: Request, next: Next) -> Response {
    match &st.auth {
        AuthPolicy::AllowAll => next.run(req).await,
        AuthPolicy::DenyAll if req.uri().path() == HOST_PATH => next.run(req).await,
        AuthPolicy::DenyAll => ApiError::new(
            StatusCode::UNAUTHORIZED,
            "login_required",
            "sign in to use Sculptured",
        )
        .into_response(),
        AuthPolicy::Gateway { secret } => {
            let pq = req
                .uri()
                .path_and_query()
                .map(|p| p.as_str())
                .unwrap_or("/")
                .to_string();
            if let Err(e) = crate::hosted::verify(secret, req.method().as_str(), &pq, req.headers())
            {
                return e.into_response();
            }
            if crate::hosted::local_only(req.method().as_str(), req.uri().path()) {
                return crate::hosted::needs_computer().into_response();
            }
            next.run(req).await
        }
    }
}
