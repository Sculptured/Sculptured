use super::{AppState, Router, Schema, convert, madl, session};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/teams/{id}/madl", crate::portable::get(madl::get_madl))
        .route(
            "/api/teams/import-madl",
            crate::portable::post(madl::import_madl),
        )
        .route(
            "/api/teams/check-madl",
            crate::portable::post(madl::check_madl),
        )
        .route(
            "/api/teams/{id}/deployments/{seq}/session",
            crate::portable::get(session::get_session),
        )
        // A run file can be large: up to 64 MB on this route only.
        .route(
            "/api/runs/import",
            crate::portable::post(session::post_import)
                .layer(axum::extract::DefaultBodyLimit::max(64 << 20)),
        )
        // Claude Code / Codex session files, converted to a run file: the same 64 MB.
        .route(
            "/api/runs/convert",
            crate::portable::post(convert::post_convert)
                .layer(axum::extract::DefaultBodyLimit::max(64 << 20)),
        )
}

/// (file name under `web/public/schema`, schema) for each published format.
#[must_use]
pub fn schemas() -> Vec<(&'static str, Schema)> {
    // A session file is JSON lines: line 1 is the header, every later line one event.
    let mut generator = schemars::generate::SchemaSettings::draft2020_12().into_generator();
    let header = generator.subschema_for::<session::SessionHeader>();
    let line = generator.subschema_for::<session::SessionLine>();
    let defs = generator.take_definitions(true);
    let session: Schema = serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "sculptured.session/v1 line",
        "description": "One line of a .sculptured-session.jsonl file: the first line is a SessionHeader, each later line a SessionLine.",
        "oneOf": [header, line],
        "$defs": defs,
    })
    .try_into()
    .expect("a schema object");
    vec![("session-v1.json", session)]
}
