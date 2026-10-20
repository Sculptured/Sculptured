//! Community on a self-run install (C5). Off until the user turns it on: then controld keeps a
//! community-scope device key (`creds`), reports the link (`link`) and forwards an allow-listed
//! `/api/community/*` to the Sculptured server with that key. The browser never sees the key.
pub mod allow;
pub mod creds;
mod link;
mod pairing;
pub mod proxy;

use crate::state::AppState;
use axum::Router;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
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
    /// While off: a server address from an older save, to pre-fill the Turn on page (absent otherwise,
    /// so the C5 shape is unchanged).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub suggested_origin: Option<String>,
}

/// Who you are signed in to Community as.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct CommunityLinkAccount {
    pub name: String,
    pub handle: Option<String>,
}

/// `POST /api/community-link/pair`: the code to show and where to approve it.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(export)]
pub struct CommunityPairStarted {
    pub user_code: String,
    pub verify_url: String,
    #[ts(type = "number")]
    pub expires_in: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum CommunityPairState {
    Pending,
    Approved,
    Expired,
    Denied,
}

/// `GET /api/community-link/pair`.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(export)]
pub struct CommunityPairStatus {
    pub status: CommunityPairState,
}

impl CommunityLinkView {
    pub fn off() -> Self {
        Self { enabled: false, origin: None, signed_in: false, account: None, suggested_origin: None }
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
    /// The latest pairing: its generation and state. A newer pairing or turning off bumps
    /// `generation`, so an older poll stops and never saves its key.
    pairing: Mutex<Option<(u64, CommunityPairState)>>,
    generation: AtomicU64,
}

impl CommunityLink {
    pub fn new(data_dir: PathBuf, default_origin: String) -> Self {
        // The same provider as analytics, the runner's link and pairing (installing twice is harmless).
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().expect("http client");
        Self { data_dir, default_origin, http, timeout: PROXY_TIMEOUT, account: Mutex::new(None), pairing: Mutex::new(None), generation: AtomicU64::new(0) }
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

    /// A new pairing, pending; earlier ones stop.
    pub(crate) fn begin_pairing(&self) -> u64 {
        let g = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *self.pairing.lock().unwrap() = Some((g, CommunityPairState::Pending));
        g
    }

    pub(crate) fn is_current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == generation
    }

    pub(crate) fn finish_pairing(&self, generation: u64, state: CommunityPairState) {
        let mut p = self.pairing.lock().unwrap();
        if p.is_some_and(|(g, _)| g == generation) {
            *p = Some((generation, state));
        }
    }

    pub(crate) fn pairing_state(&self) -> Option<CommunityPairState> {
        self.pairing.lock().unwrap().map(|(_, s)| s)
    }

    /// Stops any pairing in flight and forgets its state.
    pub(crate) fn cancel_pairing(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        *self.pairing.lock().unwrap() = None;
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
        let signed_out = CommunityLinkView { enabled: true, origin: Some(origin.clone()), signed_in: false, account: None, suggested_origin: None };
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
    link::routes().merge(pairing::routes()).route("/api/community/{*path}", axum::routing::any(proxy::forward))
}
