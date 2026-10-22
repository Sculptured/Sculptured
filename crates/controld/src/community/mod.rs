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

mod connection;
pub use connection::{
    ACCOUNT_TTL, CommunityLink, CommunityLinkAccount, CommunityLinkView, CommunityPairStarted,
    CommunityPairState, CommunityPairStatus, DEFAULT_ORIGIN, PROXY_TIMEOUT, REVOKE_TIMEOUT, routes,
};
pub(crate) use connection::{device_unknown, saved_origin};
