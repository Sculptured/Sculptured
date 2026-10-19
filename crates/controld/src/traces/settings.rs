//! A team's trace export setting: on/off, where to, the key (kept here, never sent back) and how much
//! to send. `GET/PUT /api/teams/{id}/traces`.
use crate::state::AppState;
use crate::store::Store;
use axum::Router;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

/// Laminar's OTLP/HTTP endpoint.
pub const LAMINAR: &str = "https://api.lmnr.ai/v1/traces";

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Content {
    /// Timings, tokens and costs.
    #[default]
    Metadata,
    /// Also prompts and outputs (capped).
    Full,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TraceExport {
    pub enabled: bool,
    pub endpoint: String,
    pub key: Option<String>,
    pub content: Content,
    pub last_error: Option<String>,
    pub last_sent_ms: Option<i64>,
    /// The run whose spans were sent through `sent_through` (its last sent event's seq).
    pub sent_run: Option<i64>,
    pub sent_through: Option<i64>,
    /// That run's own span went too.
    pub run_sent: bool,
}

impl Default for TraceExport {
    fn default() -> Self {
        TraceExport { enabled: false, endpoint: LAMINAR.into(), key: None, content: Content::Metadata, last_error: None, last_sent_ms: None, sent_run: None, sent_through: None, run_sent: false }
    }
}

/// What the browser sees: whether a key is saved, never the key.
#[derive(Serialize)]
pub struct TraceExportView {
    enabled: bool,
    endpoint: String,
    key_saved: bool,
    content: Content,
    last_error: Option<String>,
    last_sent_ms: Option<i64>,
}

impl From<&TraceExport> for TraceExportView {
    fn from(t: &TraceExport) -> Self {
        TraceExportView { enabled: t.enabled, endpoint: t.endpoint.clone(), key_saved: t.key.is_some(), content: t.content, last_error: t.last_error.clone(), last_sent_ms: t.last_sent_ms }
    }
}

fn key(team: &str) -> String {
    format!("traces:{team}")
}

impl Store {
    pub fn trace_export(&self, team: &str) -> rusqlite::Result<TraceExport> {
        let json: Option<String> = self.connection().query_row("SELECT json FROM settings WHERE k = ?1", [key(team)], |r| r.get(0)).optional()?;
        Ok(json.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default())
    }
    pub fn save_trace_export(&mut self, team: &str, t: &TraceExport) -> rusqlite::Result<()> {
        self.connection().execute(
            "INSERT INTO settings (k, json) VALUES (?1, ?2) ON CONFLICT (k) DO UPDATE SET json = excluded.json",
            params![key(team), serde_json::to_string(t).expect("trace setting serializes")],
        )?;
        Ok(())
    }
    /// Teams with export on.
    pub fn trace_export_teams(&self) -> rusqlite::Result<Vec<String>> {
        let mut q = self.connection().prepare("SELECT substr(k, 8) FROM settings WHERE k LIKE 'traces:%' AND json_extract(json, '$.enabled') = 1")?;
        let rows = q.query_map([], |r| r.get(0))?.collect();
        rows
    }
}

/// `https://`, or `http://` to this computer (a local collector, tests). The hosted server sends only
/// to public `https://` addresses: never to itself, its private network or a bare service name.
pub fn endpoint_ok(endpoint: &str, hosted: bool) -> bool {
    let Ok(u) = reqwest::Url::parse(endpoint) else { return false };
    if endpoint.len() > 500 || !u.username().is_empty() || u.password().is_some() {
        return false;
    }
    let Some(host) = u.host_str() else { return false };
    let ip: Option<std::net::IpAddr> = host.trim_start_matches('[').trim_end_matches(']').parse().ok();
    let local = host.eq_ignore_ascii_case("localhost") || ip.is_some_and(|ip| ip.is_loopback());
    match u.scheme() {
        "http" => !hosted && local,
        "https" if !hosted => true,
        "https" => match ip {
            Some(ip) => is_public(ip),
            None => host.contains('.') && !local && !host.ends_with(".localhost") && !host.ends_with(".internal"),
        },
        _ => false,
    }
}

/// An address on the public internet (not loopback, private, link-local, shared, multicast or reserved).
pub fn is_public(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_private() || v4.is_loopback() || v4.is_link_local() || v4.is_broadcast() || v4.is_documentation() || v4.is_unspecified() || v4.is_multicast()
                || a == 0 || a >= 240 || (a == 100 && (64..128).contains(&b)) || (a == 192 && b == 0))
        }
        std::net::IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_public(v4.into()),
            None => {
                let first = v6.segments()[0];
                !(v6.is_loopback() || v6.is_unspecified() || v6.is_multicast() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80 || first == 0x2001 && v6.segments()[1] == 0x0db8)
            }
        },
    }
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/teams/{id}/traces", axum::routing::get(get_traces).put(put_traces))
}

async fn get_traces(
    axum::extract::State(st): axum::extract::State<AppState>,
    crate::extract::Path(id): crate::extract::Path<String>,
) -> Result<crate::extract::Json<TraceExportView>, crate::ApiError> {
    let store = st.store.lock().unwrap();
    if store.draft(&id)?.is_none() {
        return Err(crate::ApiError::not_found("Project not found"));
    }
    Ok(crate::extract::Json((&store.trace_export(&id)?).into()))
}

#[derive(Deserialize)]
struct SaveTraces {
    enabled: bool,
    endpoint: String,
    /// Absent keeps the saved key; "" removes it.
    key: Option<String>,
    content: Content,
}

async fn put_traces(
    axum::extract::State(st): axum::extract::State<AppState>,
    crate::extract::Path(id): crate::extract::Path<String>,
    crate::extract::Json(req): crate::extract::Json<SaveTraces>,
) -> Result<crate::extract::Json<TraceExportView>, crate::ApiError> {
    let endpoint = req.endpoint.trim().to_string();
    let hosted = matches!(st.auth, crate::auth::AuthPolicy::Gateway { .. });
    if !endpoint_ok(&endpoint, hosted) {
        let why = if hosted { "Use a public https:// address" } else { "Use an https:// address, or http:// to this computer" };
        return Err(crate::ApiError::bad_request("trace_endpoint_invalid", why));
    }
    if req.key.as_ref().is_some_and(|k| k.len() > 1000 || k.chars().any(|c| c.is_control())) {
        return Err(crate::ApiError::bad_request("trace_key_invalid", "That API key cannot be used"));
    }
    let mut store = st.store.lock().unwrap();
    if store.draft(&id)?.is_none() {
        return Err(crate::ApiError::not_found("Project not found"));
    }
    let mut t = store.trace_export(&id)?;
    if t.endpoint != endpoint {
        t.last_error = None;
    }
    t.enabled = req.enabled;
    t.endpoint = endpoint;
    t.content = req.content;
    match req.key.map(|k| k.trim().to_string()) {
        Some(k) if k.is_empty() => t.key = None,
        Some(k) => t.key = Some(k),
        None => {}
    }
    store.save_trace_export(&id, &t)?;
    Ok(crate::extract::Json((&t).into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_your_own_computer_a_local_collector_is_fine() {
        assert!(endpoint_ok(LAMINAR, false));
        assert!(endpoint_ok("http://127.0.0.1:4318/v1/traces", false));
        assert!(endpoint_ok("http://localhost:4318/v1/traces", false));
        assert!(!endpoint_ok("http://otel.example.com/v1/traces", false));
        assert!(!endpoint_ok("https://user:pw@otel.example.com/v1/traces", false));
    }

    #[test]
    fn the_hosted_server_sends_only_to_public_https_addresses() {
        assert!(endpoint_ok(LAMINAR, true));
        assert!(endpoint_ok("https://otel.example.com/v1/traces", true));
        for bad in [
            "http://127.0.0.1:4318/v1/traces", "https://localhost/v1/traces", "https://127.0.0.1/v1/traces", "https://10.0.0.5/v1/traces",
            "https://169.254.169.254/latest", "https://[::1]/v1/traces", "https://[fd00::1]/v1/traces", "https://gateway:8443/v1/traces",
            "https://100.64.0.1/v1/traces", "https://[::ffff:10.0.0.1]/v1/traces",
        ] {
            assert!(!endpoint_ok(bad, true), "{bad}");
        }
    }

    #[test]
    fn only_public_addresses_count_as_public() {
        for ok in ["1.1.1.1", "2606:4700::1111"] {
            assert!(is_public(ok.parse().unwrap()), "{ok}");
        }
        for bad in ["10.1.2.3", "172.16.0.1", "192.168.1.1", "127.0.0.1", "169.254.169.254", "100.64.0.1", "0.0.0.0", "224.0.0.1", "::1", "fe80::1", "fc00::1", "::ffff:192.168.0.1"] {
            assert!(!is_public(bad.parse().unwrap()), "{bad}");
        }
    }
}
