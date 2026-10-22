//! Prepare the bundled textbook shell in an empty, operator-selected project folder.
use super::textbook_files::FILES;
use crate::error::ApiError;
use crate::extract::{Json, Path};
use crate::folders;
use crate::state::AppState;
use axum::extract::State;
use axum::response::Response;
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;
use std::path::Path as FsPath;

#[derive(Deserialize)]
pub(super) struct Prepare {
    revision: u64,
}

pub(super) async fn prepare(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<Prepare>,
) -> Response {
    crate::error::respond(prepare_project(&st, &id, req.revision))
}

fn prepare_project(st: &AppState, id: &str, revision: u64) -> Result<Value, ApiError> {
    // Share Start's claim: a start and folder setup must not interleave.
    let _claim = st
        .try_begin_boot(id)
        .ok_or_else(|| ApiError::conflict("team_running", "The project is busy."))?;
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let draft = store
        .draft(id)?
        .ok_or_else(|| ApiError::not_found("Project not found"))?;
    if draft.revision != revision {
        return Err(ApiError::conflict(
            "draft_conflict",
            "Reload the saved project first.",
        ));
    }
    if store
        .rig_snapshot(id)?
        .is_some_and(|s| s.seats.iter().any(|a| crate::store::is_live(a.lifecycle)))
    {
        return Err(ApiError::conflict(
            "team_running",
            "Stop the team before preparing its folder.",
        ));
    }
    if draft.workflow.is_none() {
        return Err(ApiError::bad_request(
            "textbook_workflow",
            "Choose a workflow project.",
        ));
    }
    let folder = draft.folder.as_ref().ok_or_else(|| {
        ApiError::bad_request("textbook_folder", "Choose a project folder first.")
    })?;
    let home = folders::home(st)?;
    let target = folders::resolve_inside(&home, FsPath::new(folder))?;
    if target == home.real {
        return Err(ApiError::bad_request(
            "textbook_folder",
            "Choose a dedicated empty folder.",
        ));
    }
    install(&target)?;
    Ok(
        json!({ "folder": target, "files": FILES.len(), "commands": ["npm ci", "npm test", "npm run build", "npm run dev"] }),
    )
}

fn install(target: &FsPath) -> Result<(), ApiError> {
    let io = |e: std::io::Error| {
        ApiError::internal(format!("Textbook folder could not be prepared: {e}"))
    };
    if fs::read_dir(target).map_err(io)?.next().is_some() {
        return Err(ApiError::conflict(
            "textbook_not_empty",
            "Choose an empty folder; existing files are never replaced.",
        ));
    }
    let parent = target
        .parent()
        .ok_or_else(|| ApiError::bad_request("textbook_folder", "Choose a dedicated folder."))?;
    let staging = parent.join(format!(".textbook-prepare-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&staging).map_err(io)?;
    let result = (|| -> std::io::Result<()> {
        for (name, contents) in FILES {
            let file = staging.join(name);
            if let Some(parent) = file.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(file, contents)?;
        }
        // Atomic on the same filesystem. Rename refuses a destination that became nonempty.
        fs::rename(&staging, target)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result.map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    #[test]
    fn prepares_blank_source_starter_without_replacing_existing_work() {
        let home = tempfile::tempdir().unwrap();
        let folder = home.path().join("book");
        fs::create_dir(&folder).unwrap();
        let mut store = Store::open_in_memory().unwrap();
        let mut d = rigspec::textbook::template("book", "Book", true);
        d.folder = Some(folder.to_string_lossy().into());
        store.insert_draft(&d).unwrap();
        let st = AppState::new(store, "test".into(), vec![]).with_home(home.path());
        assert!(prepare_project(&st, "book", 1).is_err());
        let prepared = prepare_project(&st, "book", 0).unwrap();
        assert_eq!(prepared["files"], FILES.len());
        assert!(folder.join("FRAMEWORK.md").exists());
        assert!(folder.join("src/contract.ts").exists());
        let registry = fs::read_to_string(folder.join("src/registry.ts")).unwrap();
        assert!(registry.contains("[]"));
        fs::write(folder.join("src/registry.ts"), "operator work").unwrap();
        assert_eq!(
            prepare_project(&st, "book", 0).unwrap_err().code,
            "textbook_not_empty"
        );
        assert_eq!(
            fs::read_to_string(folder.join("src/registry.ts")).unwrap(),
            "operator work"
        );
    }

    #[test]
    fn refuses_folder_links_outside_home() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), home.path().join("escape")).unwrap();
        let mut store = Store::open_in_memory().unwrap();
        let mut d = rigspec::textbook::template("book", "Book", true);
        d.folder = Some(home.path().join("escape").to_string_lossy().into());
        store.insert_draft(&d).unwrap();
        let st = AppState::new(store, "test".into(), vec![]).with_home(home.path());
        assert_eq!(
            prepare_project(&st, "book", 0).unwrap_err().code,
            "outside_home"
        );
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }
}
