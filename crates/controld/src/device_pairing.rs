//! The gateway's device-code pairing (C4), shared by `sculptured connect` (a computer) and
//! Community sign-in (scope `community`): start shows a code to approve in the browser, then the
//! caller polls until the key is handed over, the code expires or the person turns it down.
use serde::Deserialize;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, Deserialize)]
pub struct Started {
    pub device_code: String,
    pub user_code: String,
    pub verify_url: String,
    pub interval: u64,
    pub expires_in: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Poll {
    Pending,
    Approved { device_id: String, key: String },
    Expired,
    Denied,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartError {
    /// The server answered with an error: its status and `error.code` (empty when it had none).
    Refused { status: u16, code: String },
    /// The server could not be reached, or its answer could not be read.
    Network(String),
}

/// `POST {origin}/gw/api/device/start` with `Origin: {origin}` (the gateway checks it on
/// unauthenticated writes). The body is `{name, platform}`, plus `scope` only when given, so a
/// computer's pairing request is unchanged.
pub async fn start(http: &reqwest::Client, origin: &str, name: &str, platform: &str, scope: Option<&str>) -> Result<Started, StartError> {
    let mut body = serde_json::json!({"name": name, "platform": platform});
    if let Some(s) = scope {
        body["scope"] = s.into();
    }
    let res = http.post(format!("{origin}/gw/api/device/start")).header("origin", origin).json(&body).timeout(TIMEOUT)
        .send().await.map_err(|e| StartError::Network(format!("cannot reach {origin}: {e}")))?;
    let status = res.status().as_u16();
    if !res.status().is_success() {
        let v: serde_json::Value = res.json().await.unwrap_or_default();
        return Err(StartError::Refused { status, code: v["error"]["code"].as_str().unwrap_or_default().to_string() });
    }
    res.json().await.map_err(|e| StartError::Network(e.to_string()))
}

/// One `POST {origin}/gw/api/device/token`: 200 Approved, 428 Pending, 410 Expired, any other 4xx
/// Denied. A 5xx or a network error is `Err`, and the caller tries again.
pub async fn poll_once(http: &reqwest::Client, origin: &str, device_code: &str) -> Result<Poll, String> {
    let res = http.post(format!("{origin}/gw/api/device/token")).header("origin", origin).json(&serde_json::json!({"device_code": device_code}))
        .timeout(TIMEOUT).send().await.map_err(|e| e.to_string())?;
    match res.status().as_u16() {
        200 => {
            #[derive(Deserialize)]
            struct Got { device_id: String, device_key: String }
            let g: Got = res.json().await.map_err(|e| e.to_string())?;
            Ok(Poll::Approved { device_id: g.device_id, key: g.device_key })
        }
        428 => Ok(Poll::Pending),
        410 => Ok(Poll::Expired),
        s if (400..500).contains(&s) => Ok(Poll::Denied),
        s => Err(format!("the server answered {s}")),
    }
}
