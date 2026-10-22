//! Hosted beta: reading the person's model connections from the gateway, which keeps their keys
//! sealed. Requests are signed with this instance's gateway secret, as analytics reports are
//! (`crate::analytics`). What comes back holds a key: it goes into one agent's launch environment
//! and nowhere else, never into an event, the store, a log or an error. Local mode has no `Gateway`.
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;

const CONNECTION_ENV: &str = "/internal/connection-env";

pub struct Gateway {
    url: String,
    user: String,
    secret: Arc<Vec<u8>>,
    client: reqwest::Client,
}

/// A model connection as the gateway hands it over for one launch.
#[derive(Deserialize)]
pub struct ConnectionEnv {
    pub provider: String,
    /// The model the connection names (OpenRouter), if any.
    #[serde(default)]
    pub model: Option<String>,
    /// Holds the key. Only for the agent's process environment.
    pub env: Vec<(String, String)>,
}

/// Names only: the values hold a key.
impl std::fmt::Debug for ConnectionEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionEnv")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field(
                "env",
                &self.env.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl Gateway {
    /// Requests go to `<gateway>/internal/...` signed as `user`.
    pub fn new(gateway: String, user: String, secret: Arc<Vec<u8>>) -> Arc<Gateway> {
        // In the full build reqwest has rustls with no provider chosen; building a client then panics.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("http client");
        Arc::new(Gateway {
            url: gateway.trim_end_matches('/').to_string(),
            user,
            secret,
            client,
        })
    }

    /// The connection's settings for an agent's launch. Err says why in plain words, never with
    /// anything the gateway sent.
    pub async fn connection_env(&self, connection_id: &str) -> Result<ConnectionEnv, String> {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let sig = signing::sign(&self.secret, "POST", CONNECTION_ENV, &self.user, ts);
        let resp = self
            .client
            .post(format!("{}{CONNECTION_ENV}", self.url))
            .header(signing::USER_HEADER, &self.user)
            .header(signing::TS_HEADER, ts.to_string())
            .header(signing::SIG_HEADER, sig)
            .json(&serde_json::json!({ "connection_id": connection_id }))
            .send()
            .await
            .map_err(|_| "Sculptured's server couldn't be reached".to_string())?;
        match resp.status().as_u16() {
            200..=299 => {}
            404 => return Err("it no longer exists".into()),
            401 | 403 => return Err("Sculptured's server refused to hand it over".into()),
            code => return Err(format!("Sculptured's server answered {code}")),
        }
        let got: ConnectionEnv = resp
            .json()
            .await
            .map_err(|_| "Sculptured's server sent an answer that couldn't be read".to_string())?;
        if !host_proto::model_env_allowed(&got.env) {
            return Err("it asks for settings Sculptured doesn't give agents".into());
        }
        Ok(got)
    }
}
