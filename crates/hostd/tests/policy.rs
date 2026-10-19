use host_proto::LaunchParams;
use hostd::policy::RunnerPolicy;

fn p(cwd: &str) -> LaunchParams { LaunchParams { cwd: cwd.into(), ..Default::default() } }

#[test]
fn only_allowed_folders_and_no_server_supplied_programs_env_or_args() {
    let root = tempfile::tempdir().unwrap();
    let proj = root.path().join("proj");
    let sibling = root.path().join("proj2");
    let outside = root.path().join("outside");
    for d in [&proj, &sibling, &outside] { std::fs::create_dir_all(d).unwrap(); }
    std::os::unix::fs::symlink(&outside, proj.join("escape")).unwrap();
    std::fs::create_dir_all(proj.join("sub")).unwrap();

    let mut pol = RunnerPolicy::default();
    assert!(pol.check(&p(proj.to_str().unwrap())).unwrap_err().contains("No folders are allowed"), "nothing allowed yet");
    pol.allow(&proj).unwrap();
    let canon = proj.canonicalize().unwrap();
    assert_eq!(pol.check(&p(proj.join("sub").to_str().unwrap())).unwrap(), canon.join("sub"));
    assert_eq!(pol.check(&p("")).unwrap(), canon, "relative/empty → the first allowed folder");
    assert_eq!(pol.check(&p("sub")).unwrap(), canon.join("sub"));
    for bad in [sibling.to_str().unwrap().to_string(), proj.join("escape").to_str().unwrap().to_string(), format!("{}/../outside", proj.display()), outside.to_str().unwrap().to_string()] {
        assert!(pol.check(&p(&bad)).unwrap_err().contains("not in a folder this computer allows"), "{bad}");
    }
    assert!(pol.check(&p("/definitely/missing")).is_err());
    let mut with_program = p(proj.to_str().unwrap()); with_program.program = Some("/bin/sh".into());
    let mut with_env = p(proj.to_str().unwrap()); with_env.env = vec![("LD_PRELOAD".into(), "x".into())];
    let mut with_args = p(proj.to_str().unwrap()); with_args.extra_args = vec!["--dangerous".into()];
    for bad in [with_program, with_env, with_args] {
        assert!(pol.check(&bad).unwrap_err().contains("refused"), "{bad:?}");
    }
}

#[test]
fn the_policy_file_is_private_and_round_trips() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("runner.json");
    assert!(RunnerPolicy::load(&file).unwrap().allowed_folders.is_empty());
    let mut pol = RunnerPolicy::default();
    pol.allow(dir.path()).unwrap();
    pol.allow(dir.path()).unwrap();
    pol.save(&file).unwrap();
    assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(RunnerPolicy::load(&file).unwrap().allowed_folders.len(), 1, "no duplicates");
}

#[tokio::test]
async fn a_restricted_supervisor_refuses_a_launch_outside_its_folders_and_says_why() {
    use drivers::stub::StubDriver;
    use host_proto::HostCommand;
    use std::sync::Arc;
    let allowed = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let mut pol = RunnerPolicy::default();
    pol.allow(allowed.path()).unwrap();
    let sup = hostd::Supervisor::new("dev-1".into(), vec![Arc::new(StubDriver)], hostd::Journal::open_in_memory(1000).unwrap()).restricted(pol);
    let err = sup.handle(HostCommand::Launch { seat: "dev.a@r".into(), driver: "stub".into(), params: p(other.path().to_str().unwrap()) }).await.unwrap_err();
    assert!(err.contains("not in a folder this computer allows"), "{err}");
    sup.handle(HostCommand::Launch { seat: "dev.b@r".into(), driver: "stub".into(), params: p(allowed.path().to_str().unwrap()) }).await.unwrap();
}

#[test]
fn the_computer_decides_how_much_agents_may_do_and_the_server_cannot_raise_it() {
    use hostd::policy::Access;
    use model::PermissionPosture::{Restricted, Unrestricted};
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("runner.json"), r#"{"allowed_folders":[]}"#).unwrap();
    let old = RunnerPolicy::load(&dir.path().join("runner.json")).unwrap();
    assert_eq!(old.access, Access::Full, "a policy without the setting keeps today's full access");
    assert_eq!(old.posture(Unrestricted), Unrestricted);
    let sandboxed = RunnerPolicy { access: Access::Sandboxed, ..Default::default() };
    assert_eq!(sandboxed.posture(Unrestricted), Restricted, "the server asked for full access; this computer says no");
    assert_eq!(sandboxed.posture(Restricted), Restricted);
    assert!(sandboxed.sandboxed() && !old.sandboxed());
}
#[test]
fn ops_stay_inside_allowed_folders_and_respect_capabilities() {
    use localops::LocalOp;
    let root = tempfile::tempdir().unwrap();
    let proj = root.path().join("proj"); let outside = root.path().join("outside");
    std::fs::create_dir_all(&proj).unwrap(); std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, proj.join("escape")).unwrap();
        let mut pol = RunnerPolicy::default(); pol.allow(&proj).unwrap();
    let p = |s: &std::path::Path| s.display().to_string();
    assert!(pol.check_op(&LocalOp::GitStatus { repo: p(&proj) }).is_ok());
    for bad in [LocalOp::GitStatus { repo: p(&outside) }, LocalOp::GitStatus { repo: p(&proj.join("escape")) }, LocalOp::ListFolder { path: Some(p(&outside)), files: false },
                LocalOp::CreateFolder { parent: p(&outside), name: "x".into() }, LocalOp::CopiesRead { project: Some(p(&outside)), team: "t".into(), seats: vec![], enabled: true }] {
        let e = pol.check_op(&bad).unwrap_err();
        assert_eq!(e.code, "not_allowed_here", "{bad:?}");
    }
    assert!(pol.check_op(&LocalOp::CopyPrepare { team: "t".into(), seat: "s".into(), parent: None, cwd: p(&proj) }).is_ok());
    let sandboxed = RunnerPolicy { access: hostd::policy::Access::Sandboxed, ..pol.clone() };
    assert!(sandboxed.caps().git && !sandboxed.caps().checks, "sandboxed: checks off by default");
    let mut no_git = pol.clone(); no_git.capabilities.git = Some(false);
    let off = no_git.check_op(&LocalOp::GitStatus { repo: p(&proj) }).unwrap_err();
    assert_eq!(off.code, "not_allowed_here");
    assert!(off.message.contains("--enable git"), "names the real flag: {}", off.message);
    // Item work: the project must be allowed; checks and copies follow their switches.
    let item = Some(localops::ItemRef { team: "t".into(), task: "bug-1".into() });
    assert!(pol.check_op(&LocalOp::ItemCopyPrepare { project: p(&proj), team: "t".into(), task: "bug-1".into() }).is_ok());
    assert_eq!(pol.check_op(&LocalOp::ItemMerge { project: p(&outside), team: "t".into(), task: "bug-1".into(), into: None }).unwrap_err().code, "not_allowed_here");
    assert_eq!(pol.check_op(&LocalOp::RunCheck { cwd: p(&outside), item: None, command: "true".into(), timeout_s: 5 }).unwrap_err().code, "not_allowed_here");
    assert!(pol.check_op(&LocalOp::RunCheck { cwd: p(&proj), item: item.clone(), command: "true".into(), timeout_s: 5 }).is_ok());
    assert!(sandboxed.check_op(&LocalOp::RunCheck { cwd: p(&proj), item, command: "true".into(), timeout_s: 5 }).unwrap_err().message.contains("--enable checks"));
    // A workflow's dispatch command is a command like a check: the same folders and the same switch.
    let dispatch = |cwd: &std::path::Path| LocalOp::RunDispatch { cwd: p(cwd), command: "cat tasks.json".into(), timeout_s: 5 };
    assert!(pol.check_op(&dispatch(&proj)).is_ok());
    assert_eq!(pol.check_op(&dispatch(&outside)).unwrap_err().code, "not_allowed_here");
    assert!(sandboxed.check_op(&dispatch(&proj)).unwrap_err().message.contains("--enable checks"));
    let mut no_copies = pol.clone(); no_copies.capabilities.copies = Some(false);
    assert!(no_copies.check_op(&LocalOp::ItemCopyPrepare { project: p(&proj), team: "t".into(), task: "x".into() }).unwrap_err().message.contains("--enable copies"));
}

#[tokio::test]
async fn copies_are_refused_when_the_repository_reaches_outside_the_allowed_folder() {
    use drivers::stub::StubDriver;
    use host_proto::{HostCommand, OwnCopy};
    use localops::LocalOp;
    use std::sync::Arc;
    let tmp = tempfile::tempdir().unwrap();
    let mono = tmp.path().join("mono");
    let pkg = mono.join("pkg-a");
    std::fs::create_dir_all(&pkg).unwrap();
    let git = |args: &[&str]| assert!(std::process::Command::new("git").current_dir(&mono).args(["-c", "user.name=T", "-c", "user.email=t@t"]).args(args).status().unwrap().success());
    git(&["init", "-q", "-b", "main"]);
    std::fs::write(mono.join("secret.txt"), "outside").unwrap();
    std::fs::write(pkg.join("a.txt"), "a").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-qm", "first"]);
    let mut pol = RunnerPolicy::default();
    pol.allow(&pkg).unwrap();
    let seat = localops::copies::CopySeat { seat: "a".into(), running: false };
    let at = pkg.display().to_string();
    for op in [LocalOp::CopyPrepare { team: "t".into(), seat: "a".into(), parent: None, cwd: at.clone() },
               LocalOp::CopyMerge { project: at.clone(), team: "t".into(), seat: seat.clone() },
               LocalOp::CopyRemove { project: at.clone(), team: "t".into(), seat: seat.clone() }] {
        let e = pol.check_op(&op).unwrap_err();
        assert_eq!(e.code, "not_allowed_here", "{op:?}");
    }
    let copies = tmp.path().join("runner/copies");
    let cx = localops::Context { home: tmp.path().into(), copies_root: copies.clone(), claude: "/no".into(), codex: "/no".into(), codex_home: tmp.path().join(".codex") };
    let sup = hostd::Supervisor::new("dev-1".into(), vec![Arc::new(StubDriver)], hostd::Journal::open_in_memory(100).unwrap())
        .restricted(pol).with_ops(hostd::ops::OpsHost { context: cx });
    let params = host_proto::LaunchParams { cwd: at, own_copy: Some(OwnCopy { team: "t".into(), parent: None }), ..Default::default() };
    let _ = sup.handle(HostCommand::Launch { seat: "team.a@t".into(), driver: "stub".into(), params }).await;
    assert!(!copies.join("t/a/secret.txt").exists(), "no copy of files outside the allowed folder");
}

#[tokio::test]
async fn a_sandboxed_computer_refuses_checks_and_says_how_to_allow_them() {
    use drivers::stub::StubDriver;
    use host_proto::HostCommand;
    use std::sync::Arc;
    let dir = tempfile::tempdir().unwrap();
    let mut pol = RunnerPolicy { access: hostd::policy::Access::Sandboxed, ..Default::default() };
    pol.allow(dir.path()).unwrap();
    let sup = hostd::Supervisor::new("dev-1".into(), vec![Arc::new(StubDriver)], hostd::Journal::open_in_memory(100).unwrap()).restricted(pol);
    let err = sup.handle(HostCommand::RunCheck { seat: "dev.a@r".into(), command: "true".into(), cwd: dir.path().display().to_string(), own_copy: None }).await.unwrap_err();
    assert!(err.contains("Checks are turned off on this computer"), "{err}");
}

#[tokio::test]
async fn a_launch_asking_for_its_own_copy_runs_in_a_worktree_on_this_computer() {
    use drivers::stub::StubDriver;
    use host_proto::{HostCommand, OwnCopy};
    use std::sync::Arc;
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    let git = |args: &[&str]| assert!(std::process::Command::new("git").current_dir(&proj).args(["-c", "user.name=T", "-c", "user.email=t@t"]).args(args).status().unwrap().success());
    git(&["init", "-q", "-b", "main"]);
    std::fs::write(proj.join("a.txt"), "a").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-qm", "first"]);
    let mut pol = RunnerPolicy::default();
    pol.allow(&proj).unwrap();
    let copies = tmp.path().join("runner/copies");
    let cx = localops::Context { home: tmp.path().into(), copies_root: copies.clone(), claude: "/no".into(), codex: "/no".into(), codex_home: tmp.path().join(".codex") };
    let sup = hostd::Supervisor::new("dev-1".into(), vec![Arc::new(StubDriver)], hostd::Journal::open_in_memory(100).unwrap())
        .restricted(pol).with_ops(hostd::ops::OpsHost { context: cx });
    let params = host_proto::LaunchParams { cwd: proj.display().to_string(), own_copy: Some(OwnCopy { team: "t".into(), parent: None }), ..Default::default() };
    sup.handle(HostCommand::Launch { seat: "team.a@t".into(), driver: "stub".into(), params }).await.unwrap();
    assert!(copies.join("t/a/a.txt").exists(), "the agent's own worktree was made on this computer");
}

#[tokio::test]
async fn a_check_for_an_agent_with_its_own_copy_runs_in_that_copy() {
    use drivers::stub::StubDriver;
    use host_proto::{HostCommand, OwnCopy};
    use std::sync::Arc;
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    let git = |args: &[&str]| assert!(std::process::Command::new("git").current_dir(&proj).args(["-c", "user.name=T", "-c", "user.email=t@t"]).args(args).status().unwrap().success());
    git(&["init", "-q", "-b", "main"]);
    std::fs::write(proj.join("a.txt"), "a").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-qm", "first"]);
    let mut pol = RunnerPolicy::default();
    pol.allow(&proj).unwrap();
    let copies = tmp.path().join("runner/copies");
    let cx = localops::Context { home: tmp.path().into(), copies_root: copies.clone(), claude: "/no".into(), codex: "/no".into(), codex_home: tmp.path().join(".codex") };
    let sup = hostd::Supervisor::new("dev-1".into(), vec![Arc::new(StubDriver)], hostd::Journal::open_in_memory(100).unwrap())
        .restricted(pol).with_ops(hostd::ops::OpsHost { context: cx });
    let own = Some(OwnCopy { team: "t".into(), parent: None });
    let params = host_proto::LaunchParams { cwd: proj.display().to_string(), own_copy: own.clone(), ..Default::default() };
    sup.handle(HostCommand::Launch { seat: "team.a@t".into(), driver: "stub".into(), params }).await.unwrap();
    sup.handle(HostCommand::RunCheck { seat: "team.a@t".into(), command: "touch checked-here".into(), cwd: proj.display().to_string(), own_copy: own }).await.unwrap();
    for _ in 0..100 {
        if copies.join("t/a/checked-here").exists() { break; }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(copies.join("t/a/checked-here").exists(), "the check ran where the agent works");
    assert!(!proj.join("checked-here").exists(), "not in the shared project");
}

#[test]
fn generation_is_allowed_by_default_and_can_be_turned_off() {
    use localops::LocalOp;
    let op = LocalOp::Generate { cli: "claude".into(), model: None, prompt: "p".into(), schema: serde_json::json!({}) };
    assert!(RunnerPolicy::default().check_op(&op).is_ok());
    let sandboxed = RunnerPolicy { access: hostd::policy::Access::Sandboxed, ..Default::default() };
    assert!(sandboxed.check_op(&op).is_ok(), "no tools run, so sandboxed computers allow it too");
    assert!(RunnerPolicy::default().info().capabilities.iter().any(|c| c == "generate"));
    let mut off = RunnerPolicy::default();
    off.capabilities.generate = Some(false);
    let e = off.check_op(&op).unwrap_err();
    assert_eq!(e.code, "not_allowed_here");
    assert!(e.message.contains("--enable generate"), "{}", e.message);
}
