//! One computer, several accounts. A computer's first account keeps its runner in `~/.sculptured/runner`;
//! each other account gets a runner of its own in `~/.sculptured/runners/<id>`, with its own background
//! service, so both run at once. A connect key is matched to the runner of its account.
use crate::connect::Credentials;
use std::path::{Path, PathBuf};

/// Where a connect key's account runs on this computer.
#[derive(Debug, PartialEq)]
pub enum Place {
    /// A runner already connected to the key's account: its folder and saved credentials.
    Reuse(PathBuf, Credentials),
    /// This folder: redeem the key into it (nothing is connected there, or its computer was disconnected).
    Redeem(PathBuf),
    /// Every runner here belongs to another account: a new runner, named after its new device.
    NewAccount,
}

/// The folder holding the other accounts' runners (beside `base`).
fn others_root(base: &Path) -> PathBuf {
    base.parent()
        .map_or_else(|| base.join("runners"), |p| p.join("runners"))
}

/// The other accounts' runner folders, sorted.
pub fn other_dirs(base: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(others_root(base)) else {
        return vec![];
    };
    let mut v: Vec<PathBuf> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    v.sort();
    v
}

/// The folder for a new account's runner.
pub fn new_dir(base: &Path, device_id: &str) -> PathBuf {
    others_root(base).join(device_id.chars().take(8).collect::<String>())
}

/// The service-name suffix of a runner folder: None for the first account's, else the folder's name.
pub fn service_suffix(dir: &Path) -> Option<String> {
    let parent = dir.parent()?.file_name()?;
    (parent == "runners")
        .then(|| dir.file_name()?.to_str().map(String::from))
        .flatten()
}

/// Whether `key` is for the account `c` is connected to; None when the server cannot say (an older
/// server), which keeps the saved connection as before.
pub async fn same_account(c: &Credentials, key: &str) -> Option<bool> {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .ok()?;
    let r = http
        .post(format!("{}/gw/api/device/same-account", c.origin))
        .header("origin", &c.origin)
        .bearer_auth(format!("{}.{}", c.device_id, c.key))
        .json(&serde_json::json!({ "key": key }))
        .send()
        .await
        .ok()?;
    if !r.status().is_success() {
        return None;
    }
    r.json::<serde_json::Value>().await.ok()?["same"].as_bool()
}

/// The runner a connect key for `origin` belongs in, given the first account's folder `base`.
pub async fn place_for_key(origin: &str, key: &str, base: &Path) -> Result<Place, String> {
    let mut base_taken = false;
    let matching_items = std::iter::once(base.to_path_buf()).chain(other_dirs(base));
    for dir in matching_items {
        let Some(c) = Credentials::load(&dir)?.filter(|c| c.origin == origin) else {
            continue;
        };
        if !crate::connect::still_connected(&c).await {
            continue;
        }
        match same_account(&c, key).await {
            Some(true) | None => return Ok(Place::Reuse(dir, c)),
            Some(false) => base_taken |= dir == base,
        }
    }
    Ok(if base_taken {
        Place::NewAccount
    } else {
        Place::Redeem(base.to_path_buf())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::{Json, Router};
    use serde_json::{Value, json};

    /// A gateway with two accounts: `a` (devices a-*, keys ka-*) and `b` (b-*, kb-*).
    async fn gateway(with_same_account: bool) -> String {
        let account = |s: &str| s.chars().next().unwrap_or('?');
        let mut app = Router::new().route(
            "/gw/api/device/check",
            axum::routing::get(|| async { axum::http::StatusCode::OK }),
        );
        if with_same_account {
            app = app.route(
                "/gw/api/device/same-account",
                axum::routing::post(
                    move |h: axum::http::HeaderMap, Json(b): Json<Value>| async move {
                        let bearer = h
                            .get("authorization")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("")
                            .trim_start_matches("Bearer ")
                            .to_string();
                        let key = b["key"]
                            .as_str()
                            .unwrap_or("")
                            .trim_start_matches('k')
                            .to_string();
                        Json(json!({ "same": account(&bearer) == account(&key) }))
                    },
                ),
            );
        }
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        origin
    }

    fn saved(dir: &Path, origin: &str, device: &str) {
        std::fs::create_dir_all(dir).unwrap();
        Credentials {
            origin: origin.into(),
            device_id: device.into(),
            key: "k".repeat(64),
        }
        .save(dir)
        .unwrap();
    }

    #[tokio::test]
    async fn a_key_finds_its_accounts_runner_or_asks_for_a_new_one() {
        let origin = gateway(true).await;
        let home = tempfile::tempdir().unwrap();
        let base = home.path().join("runner");
        assert_eq!(
            place_for_key(&origin, "ka-1", &base).await.unwrap(),
            Place::Redeem(base.clone()),
            "nothing connected yet"
        );
        saved(&base, &origin, "a-device");
        assert!(
            matches!(place_for_key(&origin, "ka-2", &base).await.unwrap(), Place::Reuse(d, _) if d == base),
            "the same account again"
        );
        assert_eq!(
            place_for_key(&origin, "kb-1", &base).await.unwrap(),
            Place::NewAccount,
            "another account"
        );
        let b_dir = new_dir(&base, "b-device-0000");
        saved(&b_dir, &origin, "b-device-0000");
        assert!(
            matches!(place_for_key(&origin, "kb-2", &base).await.unwrap(), Place::Reuse(d, _) if d == b_dir),
            "that account's runner, reused"
        );
        assert_eq!(
            (service_suffix(&base), service_suffix(&b_dir).as_deref()),
            (None, Some("b-device"))
        );
    }

    #[tokio::test]
    async fn an_older_server_keeps_the_saved_connection() {
        let origin = gateway(false).await;
        let home = tempfile::tempdir().unwrap();
        let base = home.path().join("runner");
        saved(&base, &origin, "a-device");
        assert!(
            matches!(place_for_key(&origin, "kb-1", &base).await.unwrap(), Place::Reuse(d, _) if d == base)
        );
    }
}
