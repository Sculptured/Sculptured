//! Hosted mode (`sculptured serve --hosted`): this Sculptured belongs to one beta user and sits behind the
//! gateway. Every request must carry the gateway's signature (`signing`). Routes that would act on this
//! server's own files are refused until they are routed to the user's computer (milestone 3).
use crate::error::ApiError;
use axum::http::{HeaderMap, StatusCode};

/// (method, path pattern) still refused in hosted mode: they would act on this server itself. Folders, git,
/// copies, CLIs and plan usage run on the team's computer instead (`ops`).
/// `*` method = any, `*` segment = one segment.
/// `POST /api/rigs` reads a spec file by path; `textbook/prepare` writes into a folder; metrics show the host.
pub const LOCAL_ONLY: &[(&str, &str)] = &[
    ("POST", "/api/rigs"),
    ("*", "/api/teams/*/textbook/prepare"),
    ("*", "/api/system/metrics"),
];

#[must_use]
pub fn local_only(method: &str, path: &str) -> bool {
    let parts: Vec<&str> = path.trim_end_matches('/').split('/').collect();
    LOCAL_ONLY.iter().any(|(m, p)| {
        let want: Vec<&str> = p.split('/').collect();
        (*m == "*" || m.eq_ignore_ascii_case(method))
            && want.len() == parts.len()
            && want.iter().zip(&parts).all(|(w, g)| *w == "*" || w == g)
    })
}

#[must_use]
pub fn needs_computer() -> ApiError {
    ApiError::conflict(
        "needs_computer",
        "This works only in Sculptured run on your own computer, not in the hosted app.",
    )
}

/// Ok(user id) when the request carries a valid, fresh gateway signature.
pub fn verify(
    secret: &[u8],
    method: &str,
    path_and_query: &str,
    headers: &HeaderMap,
) -> Result<String, ApiError> {
    let get = |h: &str| headers.get(h).and_then(|v| v.to_str().ok());
    let refused = || {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "login_required",
            "sign in to use Sculptured",
        )
    };
    let (Some(user), Some(ts), Some(sig)) = (
        get(signing::USER_HEADER),
        get(signing::TS_HEADER),
        get(signing::SIG_HEADER),
    ) else {
        return Err(refused());
    };
    let ts: i64 = ts.parse().map_err(|_| refused())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    if signing::verify(secret, method, path_and_query, user, ts, sig, now) {
        Ok(user.to_string())
    } else {
        Err(refused())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn local_only_matches_segments_exactly() {
        assert!(
            !super::local_only("GET", "/api/teams/abc/git"),
            "git runs on the team's computer now"
        );
        assert!(
            !super::local_only("POST", "/api/seats/team.a@x/check"),
            "checks run on the agent's computer now"
        );
        assert!(super::local_only("POST", "/api/rigs") && !super::local_only("GET", "/api/rigs"));
        assert!(!super::local_only("GET", "/api/teams/abc"));
        assert!(!super::local_only("GET", "/api/teams/abc/gitx"));
    }
}
