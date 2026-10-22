//! Reversible sidebar visibility. Trash retains drafts, files and deployment history.
use crate::error::ApiError;
use crate::extract::{Json, Path};
use crate::state::AppState;
use crate::store::Store;
use axum::Router;
use axum::extract::State;
use axum::response::Response;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub enum ProjectVisibilityStatus {
    Visible,
    Hidden,
    Trashed,
}
#[derive(Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct ProjectVisibility {
    pub id: String,
    pub status: ProjectVisibilityStatus,
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/project-visibility", axum::routing::get(list))
        .route(
            "/api/project-visibility/{id}",
            axum::routing::put(update).delete(delete_forever),
        )
}
pub(crate) fn ensure_available(store: &Store, id: &str) -> Result<(), ApiError> {
    let status: Option<String> = store
        .connection()
        .query_row(
            "SELECT status FROM project_visibility WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    if status.as_deref() == Some("trashed") {
        return Err(ApiError::conflict(
            "project_trashed",
            "Restore this project from Trash first",
        ));
    }
    Ok(())
}
async fn list(State(st): State<AppState>) -> Response {
    crate::error::respond((|| -> Result<Vec<ProjectVisibility>, ApiError> {
        let store = st.store.lock().expect("store mutex must not be poisoned");
        let mut q = store
            .connection()
            .prepare("SELECT id,status FROM project_visibility ORDER BY id")?;
        let rows = q
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .map(|(id, status)| ProjectVisibility {
                id,
                status: if status == "trashed" {
                    ProjectVisibilityStatus::Trashed
                } else {
                    ProjectVisibilityStatus::Hidden
                },
            })
            .collect())
    })())
}
#[derive(Deserialize)]
struct Update {
    status: ProjectVisibilityStatus,
}
async fn update(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<Update>,
) -> Response {
    crate::error::respond((|| -> Result<ProjectVisibility, ApiError> {
        // A start/stop and a visibility mutation cannot interleave. Never stop agents implicitly.
        let _claim = st.try_begin_boot(&id).ok_or_else(|| {
            ApiError::conflict("team_starting", "Wait for project changes to finish")
        })?;
        let store = st.store.lock().expect("store mutex must not be poisoned");
        if store.draft(&id)?.is_none() {
            return Err(ApiError::not_found("Project not found"));
        }
        if matches!(req.status, ProjectVisibilityStatus::Trashed)
            && store
                .rig_snapshot(&id)?
                .is_some_and(|s| s.seats.iter().any(|a| crate::store::is_live(a.lifecycle)))
        {
            return Err(ApiError::conflict(
                "project_running",
                "Stop the project before moving it to Trash",
            ));
        }
        match req.status {
            ProjectVisibilityStatus::Visible => {
                store
                    .connection()
                    .execute("DELETE FROM project_visibility WHERE id=?1", [&id])?;
            }
            _ => {
                store.connection().execute("INSERT INTO project_visibility(id,status) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET status=excluded.status",rusqlite::params![id,if matches!(req.status,ProjectVisibilityStatus::Hidden){"hidden"}else{"trashed"}])?;
            }
        }
        Ok(ProjectVisibility {
            id,
            status: req.status,
        })
    })())
}

/// Every table that holds a team's own records: (table, condition on the team id `?1` and its seats'
/// suffix `?2`). The project's folder and agents' git copies are files, and are left alone.
const TEAM_ROWS: &[(&str, &str)] = &[
    (
        "events",
        "subject='rig:'||?1 OR (substr(subject,1,5)='seat:' AND substr(subject,-length(?2))=?2)",
    ),
    ("messages", "substr(seat,-length(?2))=?2"),
    ("agent_controls", "substr(seat,-length(?2))=?2"),
    ("agent_budget_baselines", "substr(seat,-length(?2))=?2"),
    ("budget_queue", "substr(seat,-length(?2))=?2"),
    ("questions", "rig=?1"),
    ("actions", "rig=?1"),
    ("agent_children", "rig=?1"),
    ("seats", "rig=?1"),
    ("rigs", "name=?1"),
    ("workflow_runs", "rig=?1"),
    ("deployment_specs", "rig=?1"),
    ("deployment_drafts", "rig=?1"),
    ("run_budgets", "rig=?1"),
    ("run_reports", "rig=?1"),
    ("team_output_budgets", "rig=?1"),
    (
        "imported_events",
        "run IN (SELECT id FROM imported_runs WHERE rig=?1)",
    ),
    ("imported_runs", "rig=?1"),
    ("team_origins", "team=?1"),
    ("schedule_runs", "team=?1"),
    ("team_decisions", "team=?1"),
    ("project_maps", "team=?1"),
    ("archived_projects", "id=?1"),
    ("project_visibility", "id=?1"),
    ("drafts", "id=?1"),
];

impl Store {
    /// Removes every record of team `id`, in one transaction.
    pub fn delete_project(&self, id: &str) -> rusqlite::Result<()> {
        let suffix = format!("@{id}");
        let tx = self.connection().unchecked_transaction()?;
        for (table, cond) in TEAM_ROWS {
            let sql = format!("DELETE FROM {table} WHERE {cond}");
            if cond.contains("?2") {
                tx.execute(&sql, rusqlite::params![id, suffix])?
            } else {
                tx.execute(&sql, [id])?
            };
        }
        tx.commit()
    }
}

/// Deletes a project in the Trash forever: its design, runs, history and reports. Nothing running.
async fn delete_forever(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    crate::error::respond((|| -> Result<serde_json::Value, ApiError> {
        let _claim = st.try_begin_boot(&id).ok_or_else(|| {
            ApiError::conflict("team_starting", "Wait for project changes to finish")
        })?;
        let store = st.store.lock().expect("store mutex must not be poisoned");
        let status: Option<String> = store
            .connection()
            .query_row(
                "SELECT status FROM project_visibility WHERE id=?1",
                [&id],
                |r| r.get(0),
            )
            .optional()?;
        if status.as_deref() != Some("trashed") {
            return Err(ApiError::conflict(
                "not_in_trash",
                "Move the project to Trash first; only projects in the Trash can be deleted forever",
            ));
        }
        if store
            .rig_snapshot(&id)?
            .is_some_and(|s| s.seats.iter().any(|a| crate::store::is_live(a.lifecycle)))
        {
            return Err(ApiError::conflict(
                "project_running",
                "Stop the project before deleting it",
            ));
        }
        store.delete_project(&id)?;
        Ok(serde_json::json!({ "deleted": id }))
    })())
}
