//! Community on a self-run install (C5). Off until the user turns it on: then controld keeps a
//! community-scope device key (`creds`), reports the link (`link`) and forwards an allow-listed
//! `/api/community/*` to the Sculptured server with that key. The browser never sees the key.
pub mod creds;
mod link;

use crate::state::AppState;
use axum::Router;
use axum::extract::State;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use ts_rs::TS;

pub const DEFAULT_ORIGIN: &str = "https://app.usesculptured.com";
pub const PROXY_TIMEOUT: Duration = Duration::from_secs(15);
pub const REVOKE_TIMEOUT: Duration = Duration::from_secs(5);
pub const ACCOUNT_TTL: Duration = Duration::from_secs(60);

/// `GET /api/community-link`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct CommunityLinkView {
    pub enabled: bool,
    pub origin: Option<String>,
    pub signed_in: bool,
    pub account: Option<CommunityLinkAccount>,
}

/// Who you are signed in to Community as.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct CommunityLinkAccount {
    pub name: String,
    pub handle: Option<String>,
}

impl CommunityLinkView {
    pub fn off() -> Self {
        Self { enabled: false, origin: None, signed_in: false, account: None }
    }
}

/// The link to one Sculptured server: where its key lives, the HTTP client (no redirects) and a
/// short-lived copy of the signed-in account.
pub struct CommunityLink {
    data_dir: PathBuf,
    default_origin: String,
    pub(crate) http: reqwest::Client,
    pub(crate) timeout: Duration,
    /// (bearer it was read with, when, the account).
    account: Mutex<Option<(String, Instant, CommunityLinkAccount)>>,
}

impl CommunityLink {
    pub fn new(data_dir: PathBuf, default_origin: String) -> Self {
        // The same provider as analytics, the runner's link and pairing (installing twice is harmless).
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().expect("http client");
        Self { data_dir, default_origin, http, timeout: PROXY_TIMEOUT, account: Mutex::new(None) }
    }

    /// Tests only shorten it.
    pub fn with_timeout(mut self, d: Duration) -> Self {
        self.timeout = d;
        self
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub(crate) fn default_origin(&self) -> &str {
        &self.default_origin
    }

    /// The saved key, when it is for `origin`.
    pub(crate) fn creds_for(&self, origin: &str) -> Option<creds::Credentials> {
        creds::load(&self.data_dir).filter(|c| c.origin == origin)
    }

    /// The server refused the key (C5): forget it; the origin stays, so Community stays on.
    pub(crate) fn sign_out(&self) {
        if let Err(e) = creds::remove(&self.data_dir) {
            tracing::warn!(error = %e, "could not remove community.json");
        }
        self.forget_account();
    }

    pub(crate) fn forget_account(&self) {
        *self.account.lock().unwrap() = None;
    }

    fn cached(&self, bearer: &str) -> Option<CommunityLinkAccount> {
        let cache = self.account.lock().unwrap();
        cache.as_ref().filter(|(b, at, _)| b == bearer && at.elapsed() < ACCOUNT_TTL).map(|(_, _, a)| a.clone())
    }

    /// `GET {origin}/api/community/me` with the key: Ok(account), Ok(None) when the server could not
    /// be asked, Err(()) when it refused the key (`401 device_unknown`).
    async fn account(&self, c: &creds::Credentials) -> Result<Option<CommunityLinkAccount>, ()> {
        let bearer = c.bearer();
        if let Some(a) = self.cached(&bearer) {
            return Ok(Some(a));
        }
        let sent = self.http.get(format!("{}/api/community/me", c.origin)).bearer_auth(&bearer).timeout(self.timeout).send().await;
        let Ok(res) = sent else { return Ok(None) };
        let status = res.status().as_u16();
        let body = res.bytes().await.unwrap_or_default();
        if device_unknown(status, &body) {
            return Err(());
        }
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
        let account = (status == 200).then(|| v["user"]["name"].as_str().map(|name| CommunityLinkAccount { name: name.into(), handle: v["user"]["handle"].as_str().map(String::from) })).flatten();
        if let Some(a) = &account {
            *self.account.lock().unwrap() = Some((bearer, Instant::now(), a.clone()));
        }
        Ok(account)
    }

    /// The link for the saved setting: off, on and signed out, or signed in (with the account when
    /// the server answered). Signed out and off make no request.
    pub(crate) async fn view(&self, origin: Option<String>) -> CommunityLinkView {
        let Some(origin) = origin else { return CommunityLinkView::off() };
        let signed_out = CommunityLinkView { enabled: true, origin: Some(origin.clone()), signed_in: false, account: None };
        let Some(c) = self.creds_for(&origin) else { return signed_out };
        match self.account(&c).await {
            Err(()) => {
                self.sign_out();
                signed_out
            }
            Ok(account) => CommunityLinkView { signed_in: true, account, ..signed_out },
        }
    }
}

/// Whether an upstream answer is the gateway's refusal of the key (C4): `401 {"error":{"code":"device_unknown"}}`.
pub(crate) fn device_unknown(status: u16, body: &[u8]) -> bool {
    status == 401 && serde_json::from_slice::<serde_json::Value>(body).is_ok_and(|v| v["error"]["code"] == "device_unknown")
}

/// The saved Sculptured server, None when Community is off.
pub(crate) fn saved_origin(st: &AppState) -> Result<Option<String>, crate::ApiError> {
    Ok(st.store.lock().unwrap().settings()?.sculptured_origin)
}

/// `/api/community-link*` and the `/api/community/*` proxy.
pub fn routes() -> Router<AppState> {
    link::routes().route("/api/community/{*path}", axum::routing::any(forward))
}

/// `/api/community/*`: refused while Community is off (`409 community_off`).
async fn forward(State(st): State<AppState>) -> Result<axum::response::Response, crate::ApiError> {
    let on = st.community.is_some() && saved_origin(&st)?.is_some();
    if !on {
        return Err(crate::ApiError::conflict("community_off", "Community is off on this computer"));
    }
    Err(crate::ApiError::not_found("no API route"))
}
