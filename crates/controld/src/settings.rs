//! Settings: global (D7 posture, D8 timeout) and per team. Task 4 adds the HTTP routes.
use crate::state::AppState;
use crate::store::Store;
use crate::views::{Settings, TeamSettings};
use axum::Router;
use rusqlite::{Connection, OptionalExtension, params};

const GLOBAL: &str = "global";
const OVERRIDE: &str = "override:question_timeout_ms";
/// The computer last chosen under Run on: a new team starts there.
const LAST_HOST: &str = "last_team_host";

fn team_key(team: &str) -> String {
    format!("team:{team}")
}

fn read<T: serde::de::DeserializeOwned + Default>(conn: &Connection, key: &str) -> rusqlite::Result<T> {
    let json: Option<String> = conn
        .query_row("SELECT json FROM settings WHERE k = ?1", [key], |r| r.get(0))
        .optional()?;
    Ok(json.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default())
}

fn write<T: serde::Serialize>(conn: &Connection, key: &str, v: &T) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (k, json) VALUES (?1, ?2) ON CONFLICT (k) DO UPDATE SET json = excluded.json",
        params![key, serde_json::to_string(v).expect("settings serialize")],
    )?;
    Ok(())
}

pub(crate) fn settings_in(conn: &Connection) -> rusqlite::Result<Settings> {
    let mut raw: serde_json::Value = read(conn, GLOBAL)?;
    // An older save's Community address (`community_url`, which the desktop app could set on its
    // own) only pre-fills the Turn on page: Community stays off until the user turns it on.
    if let Some(o) = raw.as_object_mut()
        && let Some(old) = o.remove("community_url")
    {
        o.entry("suggested_origin").or_insert(old);
    }
    let mut s: Settings = serde_json::from_value(raw).unwrap_or_default();
    s.suggested_origin = s.suggested_origin.as_deref().map(migrate_origin);
    Ok(s)
}

/// The old Community address `https://community.X…` becomes the app's `https://app.X…`, which now
/// serves Community; any other (self-hosted) address is kept.
pub fn migrate_origin(url: &str) -> String {
    match url.strip_prefix("https://community.") {
        Some(rest) => format!("https://app.{rest}"),
        None => url.to_string(),
    }
}

/// A Sculptured server address: `https://host[:port]`, or `http://127.0.0.1|localhost:port` for
/// testing on this computer. No user, path or query. Returned without a trailing slash.
pub fn valid_origin(raw: &str) -> Result<String, crate::ApiError> {
    let bad = || crate::ApiError::bad_request("community_origin_invalid", "Use an https:// address");
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() || trimmed.len() > 300 {
        return Err(bad());
    }
    let u = reqwest::Url::parse(trimmed).map_err(|_| bad())?;
    let plain = u.username().is_empty() && u.password().is_none() && u.query().is_none() && u.fragment().is_none() && u.path() == "/";
    let host = u.host_str().ok_or_else(bad)?;
    let ok = match u.scheme() {
        "https" => true,
        "http" => matches!(host, "127.0.0.1" | "localhost") && u.port().is_some(),
        _ => false,
    };
    if !(plain && ok) {
        return Err(bad());
    }
    Ok(u.as_str().trim_end_matches('/').to_string())
}

pub(crate) fn team_settings_in(conn: &Connection, team: &str) -> rusqlite::Result<TeamSettings> {
    let saved: Option<String> = conn.query_row("SELECT json FROM settings WHERE k = ?1", [team_key(team)], |r| r.get(0)).optional()?;
    match saved {
        Some(_) => read(conn, &team_key(team)),
        // A team whose settings were never saved runs on the computer chosen last for any team.
        None => Ok(TeamSettings { host: read::<Option<String>>(conn, LAST_HOST)?, ..Default::default() }),
    }
}

/// A team's question timeout: the test override, else the team's minutes, else the global minutes.
pub fn question_timeout_ms_in(conn: &Connection, rig: &str) -> rusqlite::Result<u64> {
    if let Some(ms) = read::<Option<u64>>(conn, OVERRIDE)? {
        return Ok(ms);
    }
    let minutes = team_settings_in(conn, rig)?
        .question_timeout_min
        .unwrap_or(settings_in(conn)?.question_timeout_min);
    Ok(u64::from(minutes.max(1)) * 60_000)
}

impl Store {
    pub fn settings(&self) -> rusqlite::Result<Settings> {
        settings_in(self.connection())
    }
    pub fn save_settings(&mut self, s: &Settings) -> rusqlite::Result<()> {
        write(self.connection(), GLOBAL, s)
    }
    pub fn team_settings(&self, team: &str) -> rusqlite::Result<TeamSettings> {
        team_settings_in(self.connection(), team)
    }
    pub fn save_team_settings(&mut self, team: &str, s: &TeamSettings) -> rusqlite::Result<()> {
        if let Some(h) = s.host.as_deref().filter(|h| !h.is_empty()) {
            write(self.connection(), LAST_HOST, &Some(h))?;
        }
        write(self.connection(), &team_key(team), s)
    }
    /// `sculptured serve --question-timeout-secs` (tests and e2e only). None removes it.
    pub fn set_question_timeout_override(&mut self, ms: Option<u64>) -> rusqlite::Result<()> {
        match ms {
            Some(ms) => write(self.connection(), OVERRIDE, &Some(ms)),
            None => self
                .connection()
                .execute("DELETE FROM settings WHERE k = ?1", [OVERRIDE])
                .map(|_| ()),
        }
    }
    pub fn question_timeout_ms(&self, rig: &str) -> rusqlite::Result<u64> {
        question_timeout_ms_in(self.connection(), rig)
    }
}

/// Seam (Task 4): `GET/PUT /api/settings`, `GET/PUT /api/teams/{id}/settings`.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/settings", axum::routing::get(get_settings).put(put_settings))
        .route(
            "/api/teams/{id}/settings",
            axum::routing::get(get_team_settings).put(put_team_settings),
        )
}
async fn get_team_settings(
    axum::extract::State(st): axum::extract::State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<crate::extract::Json<TeamSettings>, crate::ApiError> {
    Ok(crate::extract::Json(st.store.lock().unwrap().team_settings(&id)?))
}
#[derive(serde::Deserialize)]
struct SaveTeamSettings {
    settings: TeamSettings,
}
/// Applies at each agent's next launch.
async fn put_team_settings(
    axum::extract::State(st): axum::extract::State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    crate::extract::Json(req): crate::extract::Json<SaveTeamSettings>,
) -> Result<crate::extract::Json<TeamSettings>, crate::ApiError> {
    let s = req.settings;
    if s.question_timeout_min.is_some_and(|m| !(1..=1440).contains(&m)) {
        return Err(crate::ApiError::bad_request(
            "settings_invalid",
            "Question timeout must be between 1 and 1440 minutes",
        ));
    }
    let mut store = st.store.lock().unwrap();
    if store.draft(&id)?.is_none() {
        return Err(crate::ApiError::not_found("Project not found"));
    }
    let had_copies = store.team_settings(&id)?.own_copies;
    store.save_team_settings(&id, &s)?;
    drop(store);
    if s.own_copies && !had_copies {
        crate::analytics::emit(&st, "copies.enabled", serde_json::json!({}));
    }
    Ok(crate::extract::Json(s))
}
async fn get_settings(axum::extract::State(st): axum::extract::State<AppState>) -> Result<crate::extract::Json<Settings>, crate::ApiError> {
    Ok(crate::extract::Json(st.store.lock().unwrap().settings()?))
}
#[derive(serde::Deserialize)]
struct SaveSettings {
    settings: Settings,
}
async fn put_settings(
    axum::extract::State(st): axum::extract::State<AppState>,
    crate::extract::Json(req): crate::extract::Json<SaveSettings>,
) -> Result<crate::extract::Json<Settings>, crate::ApiError> {
    let mut s = req.settings;
    {
        // Only /api/community-link turns Community on, off or to another server.
        let stored = st.store.lock().unwrap().settings()?;
        s.sculptured_origin = stored.sculptured_origin;
        s.suggested_origin = stored.suggested_origin;
    }
    if !(1..=1440).contains(&s.question_timeout_min) {
        return Err(crate::ApiError::bad_request(
            "settings_invalid",
            "Question timeout must be between 1 and 1440 minutes",
        ));
    }
    st.store.lock().unwrap().save_settings(&s)?;
    Ok(crate::extract::Json(s))
}

#[cfg(test)]
mod tests {
    use super::{migrate_origin, valid_origin};
    use crate::store::Store;
    use crate::views::Settings;

    #[test]
    fn an_old_community_url_is_only_a_suggestion_and_community_stays_off() {
        let store = Store::open_in_memory().unwrap();
        store.connection().execute("INSERT INTO settings (k, json) VALUES ('global', ?1)",
            [r#"{"community_url":"https://community.example.com","posture":"unrestricted","question_timeout_min":10}"#]).unwrap();
        let s = store.settings().unwrap();
        assert_eq!(s.sculptured_origin, None, "Community stays off until the user turns it on");
        assert_eq!(s.suggested_origin.as_deref(), Some("https://app.example.com"));
    }

    #[test]
    fn a_self_hosted_origin_is_kept_as_is() {
        assert_eq!(migrate_origin("https://sculptured.my.org"), "https://sculptured.my.org");
        assert_eq!(migrate_origin("http://127.0.0.1:9000"), "http://127.0.0.1:9000");
    }

    #[test]
    fn saving_writes_sculptured_origin_and_never_community_url() {
        let mut store = Store::open_in_memory().unwrap();
        store.save_settings(&Settings { sculptured_origin: Some("https://app.x".into()), ..Settings::default() }).unwrap();
        let raw: String = store.connection().query_row("SELECT json FROM settings WHERE k = 'global'", [], |r| r.get(0)).unwrap();
        assert!(raw.contains("\"sculptured_origin\"") && !raw.contains("community_url"), "{raw}");
    }

    #[test]
    fn valid_origin_takes_https_and_loopback_http_only() {
        assert_eq!(valid_origin("https://app.x.com/").unwrap(), "https://app.x.com");
        assert!(valid_origin("http://127.0.0.1:14411").is_ok() && valid_origin("http://localhost:3000").is_ok());
        let long = format!("https://{}.com", "a".repeat(300));
        for bad in ["http://app.x.com", "https://u:p@x.com", "https://x.com/path", "https://x.com?q=1", "ftp://x", "", &long] {
            assert_eq!(valid_origin(bad).unwrap_err().code, "community_origin_invalid", "{bad}");
        }
    }
}
