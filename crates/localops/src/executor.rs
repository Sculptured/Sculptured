//! Executes local operations while preserving their wire response shapes.
use crate::{
    Context, LocalError, LocalOp, analyze, checks, clis, command, copies, folders, generate, git,
    map, plan,
};
use std::path::Path;

fn json<T: serde::Serialize>(v: T) -> serde_json::Value {
    serde_json::to_value(v).expect("serializes")
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, LocalError> + Send + 'static,
) -> Result<T, LocalError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| LocalError::failed("internal", format!("task failed: {e}")))?
}

/// Does one request and returns its answer as JSON (the same shapes the HTTP API returns).
pub async fn execute(cx: &Context, op: LocalOp) -> Result<serde_json::Value, LocalError> {
    let cx = cx.clone();
    match op {
        LocalOp::Describe => Ok(serde_json::json!({})),
        LocalOp::ListFolder { path, files } => {
            blocking(move || {
                Ok(json(folders::listing(
                    &folders::Home::of(&cx.home)?,
                    path.as_deref(),
                    files,
                )?))
            })
            .await
        }
        LocalOp::CreateFolder { parent, name } => {
            blocking(move || {
                Ok(json(folders::create(
                    &folders::Home::of(&cx.home)?,
                    &parent,
                    &name,
                )?))
            })
            .await
        }
        LocalOp::GitStatus { repo } => Ok(json(git::snapshot(Path::new(&repo)).await?)),
        LocalOp::GitDiff { repo, path, staged } => {
            Ok(serde_json::json!({ "text": git::diff(Path::new(&repo), &path, staged).await? }))
        }
        LocalOp::GitAct { repo, action } => Ok(json(git::act(Path::new(&repo), &action).await?)),
        LocalOp::CopyPrepare {
            team,
            seat,
            parent,
            cwd,
        } => {
            blocking(move || {
                Ok(json(
                    copies::prepare(
                        &cx.copies_root,
                        &team,
                        &seat,
                        parent.as_deref(),
                        Path::new(&cwd),
                    )?
                    .display()
                    .to_string(),
                ))
            })
            .await
        }
        LocalOp::CopiesRead {
            project,
            team,
            seats,
            enabled,
        } => {
            blocking(move || {
                Ok(json(copies::read(
                    project.as_deref().map(Path::new),
                    &cx.copies_root,
                    &team,
                    &seats,
                    enabled,
                )))
            })
            .await
        }
        LocalOp::CopyMerge {
            project,
            team,
            seat,
        } => {
            blocking(move || {
                copies::merge(Path::new(&project), &cx.copies_root, &team, &seat)
                    .map(|_| serde_json::json!({}))
            })
            .await
        }
        LocalOp::CopyRemove {
            project,
            team,
            seat,
        } => {
            blocking(move || {
                copies::remove(Path::new(&project), &cx.copies_root, &team, &seat)
                    .map(|_| serde_json::json!({}))
            })
            .await
        }
        LocalOp::ItemCopyPrepare {
            project,
            team,
            task,
        } => {
            blocking(move || {
                Ok(json(
                    copies::prepare(
                        &cx.copies_root,
                        &team,
                        &copies::item_seat(&task),
                        None,
                        Path::new(&project),
                    )?
                    .display()
                    .to_string(),
                ))
            })
            .await
        }
        LocalOp::ItemMerge {
            project,
            team,
            task,
            into,
        } => {
            blocking(move || {
                Ok(
                    match copies::merge_item(
                        Path::new(&project),
                        &cx.copies_root,
                        &team,
                        &task,
                        into.as_deref(),
                    )? {
                        copies::MergeOutcome::Merged { summary } => {
                            serde_json::json!({ "merged": true, "summary": summary })
                        }
                        copies::MergeOutcome::Conflict(why) => {
                            serde_json::json!({ "merged": false, "summary": why })
                        }
                    },
                )
            })
            .await
        }
        LocalOp::RunCheck {
            cwd,
            item,
            command,
            timeout_s,
        } => {
            let cwd = match item {
                Some(i) => copies::item_dir(&cx.copies_root, &i.team, &i.task)
                    .display()
                    .to_string(),
                None => cwd,
            };
            if !Path::new(&cwd).is_dir() {
                return Err(LocalError::conflict(
                    "no_folder",
                    format!(
                        "Could not run the check: the folder {cwd} does not exist on this computer."
                    ),
                ));
            }
            Ok(json(
                checks::execute_within(
                    &command,
                    &cwd,
                    std::time::Duration::from_secs(timeout_s.into()),
                )
                .await,
            ))
        }
        LocalOp::RunDispatch {
            cwd,
            command,
            timeout_s,
        } => {
            if !Path::new(&cwd).is_dir() {
                return Err(LocalError::conflict(
                    "no_folder",
                    format!("The folder {cwd} does not exist on this computer."),
                ));
            }
            Ok(json(
                command::capture(
                    &command,
                    &cwd,
                    std::time::Duration::from_secs(timeout_s.into()),
                )
                .await,
            ))
        }
        LocalOp::Clis => {
            let (a, b) = tokio::join!(
                clis::status("claude", &cx.claude),
                clis::status("codex", &cx.codex)
            );
            Ok(json(vec![a, b]))
        }
        LocalOp::CliLogin { name } => {
            let program = match name.as_str() {
                "claude" => cx.claude.clone(),
                "codex" => cx.codex.clone(),
                _ => {
                    return Err(LocalError::not_found(
                        "not_found",
                        format!("no CLI {name:?}"),
                    ));
                }
            };
            Ok(json(clis::login(&name, &program).await?))
        }
        LocalOp::CliLoginCode { name, code } => clis::login_code(&name, &code)
            .await
            .map(|()| serde_json::json!({})),
        LocalOp::PlanUsage => blocking(move || Ok(json(plan::codex_files(&cx.codex_home)))).await,
        LocalOp::Generate {
            cli,
            model,
            prompt,
            schema,
        } => generate::run(&cx, &cli, model.as_deref(), &prompt, &schema).await,
        LocalOp::AnalyzeRun { sessions, export } => {
            blocking(move || analyze::run(&cx, &sessions, &export)).await
        }
        LocalOp::ProjectMap { path, shared } => {
            blocking(move || Ok(json(map::build_checked(Path::new(&path), &shared)?))).await
        }
        LocalOp::KeepLesson { path, text } => {
            blocking(move || {
                Ok(serde_json::json!({ "kept": map::keep_lesson(Path::new(&path), &text)? }))
            })
            .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn one_executor_serves_every_op_with_plain_json() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join("proj")).unwrap();
        let fake = home.path().join("claude");
        std::fs::write(&fake, "#!/bin/sh\ncase \"$1\" in --version) echo 9.9;; auth) echo '{\"loggedIn\":true,\"email\":\"a@x\"}';; esac\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        // A test thread forking while the script was open for writing makes exec fail with "text file busy" for a moment.
        for _ in 0..100 {
            match std::process::Command::new(&fake).arg("--version").output() {
                Err(e) if e.raw_os_error() == Some(26) => {
                    std::thread::sleep(std::time::Duration::from_millis(20))
                }
                _ => break,
            }
        }
        let cx = Context {
            home: home.path().into(),
            copies_root: home.path().join("copies"),
            claude: fake.clone(),
            codex: home.path().join("no-codex"),
            codex_home: home.path().join(".codex"),
        };
        let l = execute(
            &cx,
            LocalOp::ListFolder {
                path: None,
                files: false,
            },
        )
        .await
        .unwrap();
        assert_eq!(l["entries"][0]["name"], "proj");
        let s = execute(
            &cx,
            LocalOp::GitStatus {
                repo: home.path().join("proj").display().to_string(),
            },
        )
        .await
        .unwrap();
        assert_eq!(s["repository"], false);
        let clis = execute(&cx, LocalOp::Clis).await.unwrap();
        assert_eq!(
            (
                clis[0]["name"].as_str(),
                clis[0]["installed"].as_bool(),
                clis[0]["signed_in"].as_bool(),
                clis[0]["account"].as_str()
            ),
            (Some("claude"), Some(true), Some(true), Some("a@x"))
        );
        assert_eq!(clis[1]["installed"], false);
        assert!(
            execute(&cx, LocalOp::PlanUsage).await.unwrap().is_null(),
            "no codex session files: null"
        );
        let wire: LocalOp = serde_json::from_str(r#"{"op":"git-status","repo":"/x"}"#).unwrap();
        assert_eq!(wire, LocalOp::GitStatus { repo: "/x".into() });
        let e = execute(
            &cx,
            LocalOp::ListFolder {
                path: Some("/".into()),
                files: false,
            },
        )
        .await
        .unwrap_err();
        assert_eq!(e.status, 403);
    }
}
