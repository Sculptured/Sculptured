use super::{Deserialize, Serialize};

/// Environment for an agent's process. It can hold a key, so Debug shows the names only; on the wire
/// it is the plain list of pairs.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Env(pub Vec<(String, String)>);

impl std::fmt::Debug for Env {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|(k, _)| format!("{k}=…")))
            .finish()
    }
}
impl std::ops::Deref for Env {
    type Target = Vec<(String, String)>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for Env {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl From<Vec<(String, String)>> for Env {
    fn from(v: Vec<(String, String)>) -> Self {
        Env(v)
    }
}
impl FromIterator<(String, String)> for Env {
    fn from_iter<I: IntoIterator<Item = (String, String)>>(i: I) -> Self {
        Env(i.into_iter().collect())
    }
}
impl<'a> IntoIterator for &'a Env {
    type Item = &'a (String, String);
    type IntoIter = std::slice::Iter<'a, (String, String)>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// The only environment the server may give an agent: a model connection's key, and where its
/// requests go. Anything else in `LaunchParams.env` is refused by a connected computer.
pub const MODEL_ENV_KEYS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_MODEL",
    "OPENROUTER_API_KEY",
];

/// The addresses `ANTHROPIC_BASE_URL` may name (OpenRouter's Anthropic-compatible API).
pub const MODEL_BASE_URLS: &[&str] = &["https://openrouter.ai/api"];

/// True when every entry is a model connection setting (`MODEL_ENV_KEYS`), each named once, with a
/// known `ANTHROPIC_BASE_URL`.
#[must_use]
pub fn model_env_allowed(env: &[(String, String)]) -> bool {
    env.iter().enumerate().all(|(i, (k, v))| {
        MODEL_ENV_KEYS.contains(&k.as_str())
            && !env[..i].iter().any(|(o, _)| o == k)
            && (k != "ANTHROPIC_BASE_URL" || MODEL_BASE_URLS.contains(&v.as_str()))
    })
}
