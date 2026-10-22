use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use controld::AppState;
use controld::store::Store;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use tower::ServiceExt;

/// A home folder with dirs, a file, a hidden dir, and symlinks inside and outside.
struct Fixture {
    _root: tempfile::TempDir,
    home: PathBuf,
    outside: PathBuf,
    router: Router,
}

fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("home")).unwrap();
    let home = root.path().join("home").canonicalize().unwrap();
    let outside = root.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), "SECRET-OUTSIDE").unwrap();
    std::fs::create_dir_all(home.join("projects/alpha")).unwrap();
    std::fs::create_dir(home.join("Beta")).unwrap();
    std::fs::create_dir(home.join(".hidden")).unwrap();
    std::fs::write(home.join("notes.txt"), "SECRET-CONTENT").unwrap();
    std::os::unix::fs::symlink(home.join("projects"), home.join("proj-link")).unwrap();
    std::os::unix::fs::symlink(&outside, home.join("escape")).unwrap();
    let st = AppState::new(
        Store::open_in_memory().unwrap(),
        "tok".into(),
        vec!["test".into()],
    )
    .with_home(&home);
    Fixture {
        router: controld::app(st, None),
        _root: root,
        home,
        outside,
    }
}

async fn call(
    router: &Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, String) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "test");
    let body = match body {
        Some(b) => {
            req = req.header("content-type", "application/json");
            Body::from(b.to_string())
        }
        None => Body::empty(),
    };
    let resp = router
        .clone()
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

async fn list(f: &Fixture, path: Option<&std::path::Path>) -> (StatusCode, Value) {
    let uri = match path {
        Some(p) => format!("/api/folders?path={}", urlencode(&p.display().to_string())),
        None => "/api/folders".to_string(),
    };
    list_uri(f, &uri).await
}

async fn list_uri(f: &Fixture, uri: &str) -> (StatusCode, Value) {
    let (status, text) = call(&f.router, "GET", uri, None).await;
    assert!(!text.contains("SECRET"), "file contents leaked: {text}");
    (status, serde_json::from_str(&text).unwrap())
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'/' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn names(v: &Value) -> Vec<String> {
    v["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn home_lists_directories_only_without_dot_folders_or_escaping_links() {
    let f = fixture();
    let (status, v) = list(&f, None).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(names(&v), ["Beta", "proj-link", "projects"]);
    assert_eq!(v["path"], f.home.display().to_string());
    assert_eq!(v["home"], f.home.display().to_string());
    assert_eq!(v["parent"], Value::Null);
    let link = v["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "proj-link")
        .unwrap();
    assert_eq!(
        link["path"],
        f.home.join("projects").display().to_string(),
        "links resolve to their real place"
    );
}

#[tokio::test]
async fn subfolders_list_with_a_parent() {
    let f = fixture();
    let (status, v) = list(&f, Some(&f.home.join("projects"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(names(&v), ["alpha"]);
    assert_eq!(v["parent"], f.home.display().to_string());
}

#[tokio::test]
async fn escapes_are_refused() {
    let f = fixture();
    for p in [
        f.home.join(".."),
        f.outside.clone(),
        f.home.join("escape"),
        PathBuf::from("/"),
    ] {
        let (status, v) = list(&f, Some(&p)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{} → {v}", p.display());
        assert_eq!(v["error"]["code"], "outside_home");
    }
    let (status, v) = list(&f, Some(std::path::Path::new("relative/dir"))).await;
    assert_eq!(
        (status, v["error"]["code"].clone()),
        (StatusCode::BAD_REQUEST, json!("path_not_absolute"))
    );
    let (status, v) = list(&f, Some(&f.home.join("missing"))).await;
    assert_eq!(
        (status, v["error"]["code"].clone()),
        (StatusCode::NOT_FOUND, json!("folder_missing"))
    );
    let (status, v) = list(&f, Some(&f.home.join("notes.txt"))).await;
    assert_eq!(
        (status, v["error"]["code"].clone()),
        (StatusCode::BAD_REQUEST, json!("not_a_folder"))
    );
}

#[tokio::test]
async fn create_folder_inside_home_only() {
    let f = fixture();
    let parent = f.home.join("projects").display().to_string();
    let (status, body) = call(
        &f.router,
        "POST",
        "/api/folders",
        Some(json!({ "request_id": "c1", "parent": parent, "name": "gamma" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        v["path"],
        f.home.join("projects/gamma").display().to_string()
    );
    assert!(f.home.join("projects/gamma").is_dir());

    let again = |rid: &str, parent: String, name: &str| json!({ "request_id": rid, "parent": parent, "name": name });
    let (status, body) = call(
        &f.router,
        "POST",
        "/api/folders",
        Some(again("c2", parent.clone(), "gamma")),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    for bad in ["../x", "a/b", ".dot", "", ".."] {
        let (status, body) = call(
            &f.router,
            "POST",
            "/api/folders",
            Some(again(&format!("bad-{bad}"), parent.clone(), bad)),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad:?}: {body}");
    }
    let (status, _) = call(
        &f.router,
        "POST",
        "/api/folders",
        Some(again("c3", f.outside.display().to_string(), "x")),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(!f.outside.join("x").exists());
    let (status, _) = call(
        &f.router,
        "POST",
        "/api/folders",
        Some(again(
            "c4",
            f.home.join("escape").display().to_string(),
            "x",
        )),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &f.router,
        "POST",
        "/api/folders",
        Some(json!({ "parent": parent, "name": "y" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "request_id is required");
}

#[tokio::test]
async fn an_unknown_home_is_reported() {
    let mut st = AppState::new(
        Store::open_in_memory().unwrap(),
        "tok".into(),
        vec!["test".into()],
    );
    st.home = None;
    let (status, body) = call(&controld::app(st, None), "GET", "/api/folders", None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(body.contains("home_unknown"));
}

// ---- Added attack coverage (beyond the brief's tests) ----

fn code(v: &Value) -> &str {
    v["error"]["code"].as_str().unwrap_or("")
}

/// `.` and `..` components are refused before the filesystem is asked anything.
#[tokio::test]
async fn dot_and_dot_dot_components_are_refused_before_resolving() {
    let f = fixture();
    // Even when, read literally, they stay inside home: the picker only sends real paths.
    for p in [
        f.home.join("projects/alpha/../.."),
        f.home.join("projects/.."),
        PathBuf::from(format!("{}/./projects", f.home.display())),
        PathBuf::from(format!("{}/projects/.", f.home.display())),
        f.home.join("missing/.."),
    ] {
        let (status, v) = list(&f, Some(&p)).await;
        assert_eq!(
            (status, code(&v)),
            (StatusCode::BAD_REQUEST, "bad_path"),
            "{} → {v}",
            p.display()
        );
    }
    // `..` that climbs out is refused as outside home, also through a link or a deep chain.
    for p in [
        f.home.join("projects/../../outside"),
        f.home.join("proj-link/../../outside"),
        f.home.join("projects/alpha/../../../.."),
        f.home.join("Beta/../../home/../outside"),
    ] {
        let (status, v) = list(&f, Some(&p)).await;
        assert_eq!(
            (status, code(&v)),
            (StatusCode::FORBIDDEN, "outside_home"),
            "{} → {v}",
            p.display()
        );
    }
}

/// Percent-encoded traversal and separators are decoded exactly once, then checked.
#[tokio::test]
async fn encoded_traversal_is_refused() {
    let f = fixture();
    let home = f.home.display().to_string();
    for raw in [
        format!("{}/%2e%2e", urlencode(&home)),
        format!("{}/%2E%2E/outside", urlencode(&home)),
        format!("{}%2F..%2F..", urlencode(&home)),
        format!("{}/%2e%2e%2f%2e%2e%2f%2e%2e", urlencode(&home)),
        "%2Fetc".to_string(),
        "%2F".to_string(),
    ] {
        let (status, v) = list_uri(&f, &format!("/api/folders?path={raw}")).await;
        assert_eq!(
            (status, code(&v)),
            (StatusCode::FORBIDDEN, "outside_home"),
            "{raw} → {v}"
        );
    }
    // Double encoding is not decoded twice: `%252e%252e` is a folder literally called `%2e%2e`.
    let (status, v) = list_uri(
        &f,
        &format!("/api/folders?path={}/%252e%252e", urlencode(&home)),
    )
    .await;
    assert_eq!(
        (status, code(&v)),
        (StatusCode::NOT_FOUND, "folder_missing"),
        "{v}"
    );
    // A NUL byte never reaches the filesystem as a truncated path.
    let (status, v) = list_uri(
        &f,
        &format!("/api/folders?path={}/projects%00", urlencode(&home)),
    )
    .await;
    assert_eq!(
        (status, code(&v)),
        (StatusCode::BAD_REQUEST, "bad_path"),
        "{v}"
    );
    let (status, v) = list_uri(
        &f,
        &format!("/api/folders?path={}/projects%00/../..", urlencode(&home)),
    )
    .await;
    assert_eq!(
        (status, code(&v)),
        (StatusCode::FORBIDDEN, "outside_home"),
        "{v}"
    );
}

/// Paths the filesystem can't resolve for other reasons get `bad_path`, not a permission code.
#[tokio::test]
async fn unusable_paths_are_bad_paths() {
    let f = fixture();
    std::os::unix::fs::symlink(f.home.join("loop-b"), f.home.join("loop-a")).unwrap();
    std::os::unix::fs::symlink(f.home.join("loop-a"), f.home.join("loop-b")).unwrap();
    let (status, v) = list(&f, Some(&f.home.join("loop-a"))).await;
    assert_eq!(
        (status, code(&v)),
        (StatusCode::BAD_REQUEST, "bad_path"),
        "symlink loop: {v}"
    );
    let (status, v) = list(&f, Some(&f.home.join("n".repeat(300)))).await;
    assert_eq!(
        (status, code(&v)),
        (StatusCode::BAD_REQUEST, "bad_path"),
        "name too long: {v}"
    );
    let (_, v) = list(&f, None).await;
    assert!(
        !names(&v).iter().any(|n| n.starts_with("loop")),
        "looping links are not listed: {v}"
    );
}

/// A folder next to home whose name starts with home's name is still outside.
#[tokio::test]
async fn a_sibling_with_a_matching_prefix_is_outside() {
    let f = fixture();
    let sibling = PathBuf::from(format!("{}-evil", f.home.display()));
    std::fs::create_dir(&sibling).unwrap();
    std::fs::create_dir(sibling.join("inner")).unwrap();
    let (status, v) = list(&f, Some(&sibling)).await;
    assert_eq!(
        (status, code(&v)),
        (StatusCode::FORBIDDEN, "outside_home"),
        "{v}"
    );
    std::os::unix::fs::symlink(&sibling, f.home.join("evil-link")).unwrap();
    let (_, v) = list(&f, None).await;
    assert!(!names(&v).contains(&"evil-link".to_string()), "{v}");
}

/// Whether a path outside home exists is never revealed: missing and present answer alike.
#[tokio::test]
async fn outside_home_is_refused_whether_or_not_it_exists() {
    let f = fixture();
    for p in [
        f.outside.join("does-not-exist"),
        f.outside.join("does-not-exist/deeper"),
        f.home.join("escape/does-not-exist"),
        f.outside.join("secret.txt"),
        PathBuf::from("/no-such-top-level-folder-sculptured"),
    ] {
        let (status, v) = list(&f, Some(&p)).await;
        assert_eq!(
            (status, code(&v)),
            (StatusCode::FORBIDDEN, "outside_home"),
            "{} → {v}",
            p.display()
        );
    }
    // `..` after an outside folder: realpath would have to find it first. The answer is the
    // same whether it exists or not, for paths that read as leaving home and as returning.
    std::fs::create_dir(f.outside.join("realdir")).unwrap();
    let home_name = f.home.file_name().unwrap().to_str().unwrap();
    for tail in [
        format!("../../{home_name}"),
        format!("../../{home_name}/projects"),
        "../..".into(),
        "..".into(),
    ] {
        let real = format!("{}/realdir/{tail}", f.outside.display());
        let missing = format!("{}/nosuch/{tail}", f.outside.display());
        let a = call(
            &f.router,
            "GET",
            &format!("/api/folders?path={}", urlencode(&real)),
            None,
        )
        .await;
        let b = call(
            &f.router,
            "GET",
            &format!("/api/folders?path={}", urlencode(&missing)),
            None,
        )
        .await;
        assert_eq!(a, b, "existence leaked through `{tail}`");
        assert!(
            a.0 == StatusCode::BAD_REQUEST || a.0 == StatusCode::FORBIDDEN,
            "{tail}: {a:?}"
        );
    }
}

/// The same for creation: `..` after an outside folder never creates, and never tells.
#[tokio::test]
async fn create_answers_alike_whether_an_outside_folder_exists() {
    let f = fixture();
    std::fs::create_dir(f.outside.join("realdir")).unwrap();
    let home_name = f.home.file_name().unwrap().to_str().unwrap();
    let indexed_items = [
        format!("../../{home_name}"),
        format!("../../{home_name}/projects"),
        "../..".into(),
    ]
    .into_iter()
    .enumerate();
    for (i, tail) in indexed_items {
        let post = |rid: String, parent: String| json!({ "request_id": rid, "parent": parent, "name": "planted" });
        let real = format!("{}/realdir/{tail}", f.outside.display());
        let missing = format!("{}/nosuch/{tail}", f.outside.display());
        let a = call(
            &f.router,
            "POST",
            "/api/folders",
            Some(post(format!("r{i}"), real)),
        )
        .await;
        let b = call(
            &f.router,
            "POST",
            "/api/folders",
            Some(post(format!("m{i}"), missing)),
        )
        .await;
        assert_eq!(a, b, "existence leaked through `{tail}`");
        assert_ne!(a.0, StatusCode::OK, "{tail}: {a:?}");
    }
    assert!(!f.home.join("planted").exists() && !f.home.join("projects/planted").exists());
    assert!(
        !f.outside.join("planted").exists() && !f.home.parent().unwrap().join("planted").exists()
    );
}

/// A hidden folder is left out of listings but can be opened when asked for directly.
#[tokio::test]
async fn a_hidden_folder_can_be_opened_directly() {
    let f = fixture();
    std::fs::create_dir(f.home.join(".hidden/inner")).unwrap();
    let (status, v) = list(&f, Some(&f.home.join(".hidden"))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["path"], f.home.join(".hidden").display().to_string());
    assert_eq!(names(&v), ["inner"]);
}

/// Paths are used exactly as sent: a folder whose name ends in a space opens from its listing.
#[tokio::test]
async fn paths_are_not_trimmed() {
    let f = fixture();
    std::fs::create_dir_all(f.home.join("trail /inside")).unwrap();
    let (_, v) = list(&f, None).await;
    let entry = v["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "trail ")
        .expect("listed")
        .clone();
    let path = entry["path"].as_str().unwrap();
    assert!(path.ends_with("trail "), "{path:?}");
    let (status, v) = list(&f, Some(std::path::Path::new(path))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["path"], path);
    assert_eq!(names(&v), ["inside"]);
    // An all-blank path still means home.
    let (status, v) = list_uri(&f, "/api/folders?path=%20%20").await;
    assert_eq!(
        (status, v["path"].clone()),
        (StatusCode::OK, json!(f.home.display().to_string()))
    );
    // A parent that ends in a space is used as sent.
    let (status, body) = call(
        &f.router,
        "POST",
        "/api/folders",
        Some(json!({ "request_id": "t", "parent": path, "name": "made" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(f.home.join("trail /made").is_dir());
}

/// Entries are sorted case-insensitively, then capped at 2000 with `truncated` set.
#[tokio::test]
async fn listings_sort_case_insensitively_then_truncate() {
    let f = fixture();
    let dir = f.home.join("many");
    std::fs::create_dir(&dir).unwrap();
    for n in ["b", "A", "C", "a"] {
        std::fs::create_dir(dir.join(n)).unwrap();
    }
    let (_, v) = list(&f, Some(&dir)).await;
    assert_eq!(
        names(&v),
        ["A", "a", "b", "C"],
        "case-insensitive, not byte order"
    );
    assert_eq!(v["truncated"], false);
    for n in ["b", "A", "C", "a"] {
        std::fs::remove_dir(dir.join(n)).unwrap();
    }
    // 1999 upper-case names plus one lower-case name that sorts first case-insensitively but
    // last in byte order: exactly the cap, nothing truncated.
    for i in 0..1999 {
        std::fs::create_dir(dir.join(format!("Z{i:04}"))).unwrap();
    }
    std::fs::create_dir(dir.join("a-first")).unwrap();
    let (_, v) = list(&f, Some(&dir)).await;
    let got = names(&v);
    assert_eq!((got.len(), v["truncated"].clone()), (2000, json!(false)));
    assert_eq!(got[0], "a-first");
    // One more: the last in sorted order is dropped, never the case-insensitively first.
    std::fs::create_dir(dir.join("Z1999")).unwrap();
    let (_, v) = list(&f, Some(&dir)).await;
    let got = names(&v);
    assert_eq!((got.len(), v["truncated"].clone()), (2000, json!(true)));
    assert_eq!((got[0].as_str(), got[1999].as_str()), ("a-first", "Z1998"));
}

/// No error message echoes a path, inside or outside home.
#[tokio::test]
async fn errors_echo_no_paths() {
    let f = fixture();
    let outside = f.outside.display().to_string();
    for p in [
        f.outside.clone(),
        f.home.join("escape"),
        f.home.join("missing"),
        f.home.join("notes.txt"),
        f.outside.join("gone"),
    ] {
        let uri = format!("/api/folders?path={}", urlencode(&p.display().to_string()));
        let (status, text) = call(&f.router, "GET", &uri, None).await;
        assert_ne!(status, StatusCode::OK);
        assert!(!text.contains(&outside), "outside path echoed: {text}");
        assert!(
            !text.contains(&f.home.display().to_string()),
            "path echoed in an error: {text}"
        );
    }
}

/// Symlinks to files are files: neither they nor plain files are ever listed.
#[tokio::test]
async fn files_and_links_to_files_are_never_listed() {
    let f = fixture();
    std::os::unix::fs::symlink(f.home.join("notes.txt"), f.home.join("note-link")).unwrap();
    std::os::unix::fs::symlink(f.outside.join("secret.txt"), f.home.join("secret-link")).unwrap();
    std::os::unix::fs::symlink(f.home.join("nowhere"), f.home.join("dangling")).unwrap();
    std::fs::write(f.home.join("projects/readme.md"), "x").unwrap();
    for p in [None, Some(f.home.join("projects"))] {
        let (status, text) = call(
            &f.router,
            "GET",
            &match &p {
                Some(p) => format!("/api/folders?path={}", urlencode(&p.display().to_string())),
                None => "/api/folders".into(),
            },
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        for leaked in [
            "notes.txt",
            "note-link",
            "secret-link",
            "dangling",
            "readme.md",
            "SECRET",
            "outside",
        ] {
            assert!(!text.contains(leaked), "{leaked} leaked: {text}");
        }
    }
    let (status, v) = list(&f, Some(&f.home.join("note-link"))).await;
    assert_eq!(
        (status, code(&v)),
        (StatusCode::BAD_REQUEST, "not_a_folder"),
        "{v}"
    );
}

/// Dot-folders are skipped in every listing, including when reached through a link.
#[tokio::test]
async fn dot_folders_are_skipped_everywhere() {
    let f = fixture();
    std::fs::create_dir(f.home.join("projects/.git")).unwrap();
    // A dot-named link to a visible folder is skipped by its own name.
    std::os::unix::fs::symlink(f.home.join("Beta"), f.home.join(".beta-link")).unwrap();
    let (_, v) = list(&f, Some(&f.home.join("projects"))).await;
    assert_eq!(names(&v), ["alpha"]);
    // Listing through a link (`proj-link` → `projects`) still skips the dot-folder inside.
    let (status, v) = list(&f, Some(&f.home.join("proj-link"))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(names(&v), ["alpha"]);
    assert_eq!(v["path"], f.home.join("projects").display().to_string());
    let (_, v) = list(&f, None).await;
    assert!(!names(&v).iter().any(|n| n.starts_with('.')), "{v}");
}

/// An unreadable folder inside home is reported without listing anything.
#[tokio::test]
async fn an_unreadable_folder_is_reported() {
    if is_root() {
        eprintln!("skipped: permissions are not enforced for root");
        return;
    }
    let f = fixture();
    let locked = f.home.join("locked");
    std::fs::create_dir_all(locked.join("inner")).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let (status, v) = list(&f, Some(&locked)).await;
    let (inner_status, inner) = list(&f, Some(&locked.join("inner"))).await;
    let parent = locked.display().to_string();
    let (create_status, create) = call(
        &f.router,
        "POST",
        "/api/folders",
        Some(json!({ "request_id": "lk", "parent": parent, "name": "x" })),
    )
    .await;
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        (status, code(&v)),
        (StatusCode::FORBIDDEN, "folder_unreadable"),
        "{v}"
    );
    assert_eq!(
        (inner_status, code(&inner)),
        (StatusCode::FORBIDDEN, "folder_unreadable"),
        "{inner}"
    );
    assert_eq!(create_status, StatusCode::FORBIDDEN, "{create}");
    assert!(!locked.join("x").exists());
}

/// A read-only parent refuses creation with folder_unwritable.
#[tokio::test]
async fn a_read_only_parent_is_unwritable() {
    if is_root() {
        eprintln!("skipped: permissions are not enforced for root");
        return;
    }
    let f = fixture();
    let ro = f.home.join("ro");
    std::fs::create_dir(&ro).unwrap();
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
    let (status, body) = call(
        &f.router,
        "POST",
        "/api/folders",
        Some(json!({ "request_id": "ro", "parent": ro.display().to_string(), "name": "x" })),
    )
    .await;
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(body.contains("folder_unwritable"), "{body}");
    assert!(!ro.join("x").exists());
}

fn is_root() -> bool {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .map(|o| o.stdout.starts_with(b"0\n"))
        .unwrap_or(false)
}

/// Every unsafe folder name is refused with bad_folder_name, and nothing is created.
#[tokio::test]
async fn bad_folder_names_are_refused() {
    let f = fixture();
    let parent = f.home.join("projects").display().to_string();
    let long = "a".repeat(256);
    let abs = f.outside.join("abs").display().to_string();
    let indexed_items = [
        "",
        "   ",
        ".",
        "..",
        "../x",
        "..\u{2f}x",
        "a/b",
        "/",
        ".hidden2",
        "a\u{0}b",
        long.as_str(),
        abs.as_str(),
        "line\nbreak",
        "tab\there",
        "cr\rx",
        "esc\u{1b}[31m",
        "del\u{7f}",
        "nel\u{85}x",
        "ls\u{2028}x",
        "ps\u{2029}x",
        "c1\u{9b}x",
    ]
    .into_iter()
    .enumerate();
    for (i, bad) in indexed_items {
        let (status, body) = call(
            &f.router,
            "POST",
            "/api/folders",
            Some(json!({ "request_id": format!("n{i}"), "parent": parent, "name": bad })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad:?}: {body}");
        assert!(body.contains("bad_folder_name"), "{bad:?}: {body}");
    }
    assert!(!f.outside.join("abs").exists());
    assert!(!f.home.join("x").exists());
    let entries: Vec<_> = std::fs::read_dir(f.home.join("projects"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(
        entries,
        [std::ffi::OsString::from("alpha")],
        "nothing new was created"
    );
}

/// Creation through traversal or an escaping link in `parent` is refused before anything is made.
#[tokio::test]
async fn create_refuses_escaping_parents() {
    let f = fixture();
    let indexed_items = [
        f.home.join(".."),
        f.home.join("projects/../.."),
        f.home.join("escape"),
        f.outside.join("not-there"),
        PathBuf::from("/"),
    ]
    .into_iter()
    .enumerate();
    for (i, parent) in indexed_items {
        let (status, body) = call(
            &f.router,
            "POST",
            "/api/folders",
            Some(json!({ "request_id": format!("p{i}"), "parent": parent.display().to_string(), "name": "planted" })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{}: {body}",
            parent.display()
        );
        assert!(body.contains("outside_home"), "{body}");
    }
    let (status, body) = call(
        &f.router,
        "POST",
        "/api/folders",
        Some(json!({ "request_id": "rel", "parent": "relative", "name": "planted" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("path_not_absolute"), "{body}");
    let root = f.home.parent().unwrap();
    assert!(!root.join("planted").exists() && !f.outside.join("planted").exists());
}

/// Creating through an in-home link lands at the real place, and the echoed path is real.
#[tokio::test]
async fn create_through_an_inside_link_returns_the_real_path() {
    let f = fixture();
    let (status, body) = call(
        &f.router,
        "POST",
        "/api/folders",
        Some(json!({ "request_id": "l1", "parent": f.home.join("proj-link").display().to_string(), "name": " delta " })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        v,
        json!({ "name": "delta", "path": f.home.join("projects/delta").display().to_string() })
    );
}

/// A dangling link named like the new folder is not followed: mkdir refuses, nothing is made outside.
#[tokio::test]
async fn create_does_not_follow_a_planted_link() {
    let f = fixture();
    std::os::unix::fs::symlink(f.outside.join("planted"), f.home.join("trap")).unwrap();
    let (status, body) = call(
        &f.router,
        "POST",
        "/api/folders",
        Some(json!({ "request_id": "t1", "parent": f.home.display().to_string(), "name": "trap" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.contains("folder_exists"), "{body}");
    assert!(!f.outside.join("planted").exists());
}

/// A repeated request_id replays the receipt: the folder is made once, the answer is the same.
#[tokio::test]
async fn create_is_idempotent_per_request_id() {
    let f = fixture();
    let req =
        json!({ "request_id": "same", "parent": f.home.display().to_string(), "name": "once" });
    let first = call(&f.router, "POST", "/api/folders", Some(req.clone())).await;
    let second = call(&f.router, "POST", "/api/folders", Some(req)).await;
    assert_eq!(first.0, StatusCode::OK, "{}", first.1);
    assert_eq!(
        first, second,
        "a duplicate replays, it does not answer folder_exists"
    );
    let (status, body) = call(
        &f.router,
        "POST",
        "/api/folders",
        Some(json!({ "request_id": "same", "parent": f.home.display().to_string(), "name": "other" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.contains("request_id_reused"), "{body}");
    assert!(!f.home.join("other").exists());
}

/// Both folder routes sit behind the login seam.
#[tokio::test]
async fn folder_routes_are_behind_the_login_seam() {
    let root = tempfile::tempdir().unwrap();
    let mut st = AppState::new(
        Store::open_in_memory().unwrap(),
        "tok".into(),
        vec!["test".into()],
    )
    .with_home(root.path());
    st.auth = controld::auth::AuthPolicy::DenyAll;
    let router = controld::app(st, None);
    for (method, body) in [
        ("GET", None),
        (
            "POST",
            Some(
                json!({ "request_id": "z", "parent": root.path().display().to_string(), "name": "x" }),
            ),
        ),
    ] {
        let (status, body) = call(&router, method, "/api/folders", body).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method}: {body}");
    }
    assert!(!root.path().join("x").exists());
}

// ---- Backend hardening: dangling links and format characters ----

/// A link inside home whose target does not resolve answers exactly like a link to an
/// outside place that exists: `outside_home`, never "missing". The same holds for paths
/// below the link, for relative targets and for chains of links.
#[tokio::test]
async fn a_dangling_link_to_outside_answers_like_a_live_one() {
    let f = fixture();
    std::fs::create_dir(f.outside.join("realdir")).unwrap();
    std::os::unix::fs::symlink(f.outside.join("realdir"), f.home.join("live")).unwrap();
    std::os::unix::fs::symlink(f.outside.join("nosuch"), f.home.join("dangling")).unwrap();
    std::os::unix::fs::symlink(
        f.outside.join("nosuch/deeper"),
        f.home.join("dangling-deep"),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        "../outside/nosuch",
        f.home.parent().unwrap().join("home/rel-dangling"),
    )
    .unwrap();
    std::os::unix::fs::symlink("dangling", f.home.join("hop")).unwrap();
    std::os::unix::fs::symlink(f.home.join("hop"), f.home.join("hop2")).unwrap();
    let live = list(&f, Some(&f.home.join("live/missing"))).await;
    assert_eq!(
        (live.0, code(&live.1)),
        (StatusCode::FORBIDDEN, "outside_home"),
        "{:?}",
        live.1
    );
    for p in [
        "dangling",
        "dangling/sub",
        "dangling-deep",
        "rel-dangling",
        "rel-dangling/sub",
        "hop",
        "hop2/sub",
    ] {
        let got = list(&f, Some(&f.home.join(p))).await;
        assert_eq!(got, live, "{p}: existence leaked");
    }
    let post = |rid: &str, parent: PathBuf| json!({ "request_id": rid, "parent": parent.display().to_string(), "name": "planted" });
    let a = call(
        &f.router,
        "POST",
        "/api/folders",
        Some(post("c1", f.home.join("live/missing"))),
    )
    .await;
    let b = call(
        &f.router,
        "POST",
        "/api/folders",
        Some(post("c2", f.home.join("dangling"))),
    )
    .await;
    assert_eq!(a, b, "create leaked existence through a dangling link");
    assert!(!f.outside.join("nosuch").exists());
}

/// A dangling link whose target is inside home is just a missing folder.
#[tokio::test]
async fn a_dangling_link_inside_home_is_missing() {
    let f = fixture();
    std::os::unix::fs::symlink(f.home.join("nothing-here"), f.home.join("inlink")).unwrap();
    std::os::unix::fs::symlink("../home/projects/gone", f.home.join("rel-inlink")).unwrap();
    for p in ["inlink", "inlink/sub", "rel-inlink"] {
        let (status, v) = list(&f, Some(&f.home.join(p))).await;
        assert_eq!(
            (status, code(&v)),
            (StatusCode::NOT_FOUND, "folder_missing"),
            "{p}: {v}"
        );
    }
}

/// Invisible Unicode format characters (category Cf: bidi overrides, zero-width
/// characters, the BOM, tag characters) are refused in new folder names like control characters.
#[tokio::test]
async fn format_characters_in_folder_names_are_refused() {
    let f = fixture();
    let parent = f.home.join("projects").display().to_string();
    let indexed_items = [
        "rlo\u{202e}txt.exe",
        "zw\u{200b}sp",
        "zwj\u{200d}x",
        "bom\u{feff}x",
        "shy\u{ad}x",
        "lri\u{2066}x",
        "tag\u{e0041}x",
        "ar\u{61c}x",
        "wj\u{2060}x",
    ]
    .into_iter()
    .enumerate();
    for (i, bad) in indexed_items {
        let (status, body) = call(
            &f.router,
            "POST",
            "/api/folders",
            Some(json!({ "request_id": format!("cf{i}"), "parent": parent, "name": bad })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad:?}: {body}");
        assert!(body.contains("bad_folder_name"), "{bad:?}: {body}");
    }
    let entries: Vec<_> = std::fs::read_dir(f.home.join("projects"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(
        entries,
        [std::ffi::OsString::from("alpha")],
        "nothing new was created"
    );
    let (status, body) = call(
        &f.router,
        "POST",
        "/api/folders",
        Some(json!({ "request_id": "ok", "parent": parent, "name": "café ünïcode 日本" })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "ordinary non-ASCII names are fine: {body}"
    );
}

// ---- Hardening fix round 1: `..` inside a link target ----

/// Links whose targets pass through a place outside home and come back with `..`.
/// Resolving them would look things up outside home, so all three answer alike
/// (`outside_home`), whether the outside place is missing, a folder or a file.
fn dot_links(f: &Fixture) -> [PathBuf; 3] {
    std::fs::create_dir(f.outside.join("realdir")).unwrap();
    let back = "../../home/projects";
    for (name, via) in [
        ("dotmissing", "nosuchA"),
        ("dotdir", "realdir"),
        ("dotfile", "secret.txt"),
    ] {
        std::os::unix::fs::symlink(
            format!("{}/{via}/{back}", f.outside.display()),
            f.home.join(name),
        )
        .unwrap();
    }
    [
        f.home.join("dotmissing"),
        f.home.join("dotdir"),
        f.home.join("dotfile"),
    ]
}

#[tokio::test]
async fn links_that_return_through_dot_dot_answer_alike() {
    let f = fixture();
    let links = dot_links(&f);
    let mut answers = Vec::new();
    let indexed_links = links.iter().enumerate();
    for (i, link) in indexed_links {
        let got = list(&f, Some(link)).await;
        assert_eq!(
            (got.0, code(&got.1)),
            (StatusCode::FORBIDDEN, "outside_home"),
            "{}: {:?}",
            link.display(),
            got.1
        );
        let post = json!({ "request_id": format!("dl{i}"), "parent": link.display().to_string(), "name": "planted" });
        let created = call(&f.router, "POST", "/api/folders", Some(post)).await;
        answers.push((got, created));
    }
    assert!(
        answers.windows(2).all(|w| w[0] == w[1]),
        "existence leaked: {answers:?}"
    );
    assert!(!f.home.join("projects/planted").exists());
    let (_, home) = list(&f, None).await;
    let shown: Vec<String> = names(&home)
        .into_iter()
        .filter(|n| n.starts_with("dot"))
        .collect();
    assert!(
        shown.is_empty(),
        "the home listing must not tell these links apart: {shown:?}"
    );
}

// ---- Hardening fix round 2: a home reached through a symlink ----

/// HOME is spelled through a link (`root/homelink/u`, with `homelink -> home`), as with
/// `/home -> /var/home` or automounted homes. That spelling is the user's own folder.
#[tokio::test]
async fn a_home_reached_through_a_link_is_home() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("home/u/proj")).unwrap();
    std::os::unix::fs::symlink("home", root.path().join("homelink")).unwrap();
    let real = root.path().join("home/u").canonicalize().unwrap();
    let raw = root.path().canonicalize().unwrap().join("homelink/u");
    std::os::unix::fs::symlink(raw.join("proj"), real.join("abs-raw")).unwrap();
    let st = AppState::new(
        Store::open_in_memory().unwrap(),
        "tok".into(),
        vec!["test".into()],
    )
    .with_home(&raw);
    let f = Fixture {
        router: controld::app(st, None),
        _root: root,
        home: real.clone(),
        outside: PathBuf::from("/nonexistent"),
    };

    let (status, v) = list(&f, Some(&raw)).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["path"], real.display().to_string());
    let (status, v) = list(&f, Some(&raw.join("proj"))).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["path"], real.join("proj").display().to_string());
    let (_, v) = list(&f, None).await;
    assert_eq!(
        names(&v),
        ["abs-raw", "proj"],
        "a link to the raw spelling is listed: {v}"
    );
    let (status, body) =
        call(&f.router, "POST", "/api/folders", Some(json!({ "request_id": "c1", "parent": raw.join("proj").display().to_string(), "name": "made" }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(real.join("proj/made").is_dir());
}

/// Links that end inside home are followed and listed, whether relative, absolute or chained.
#[tokio::test]
async fn links_that_end_inside_home_are_followed_and_listed() {
    let f = fixture();
    std::os::unix::fs::symlink("projects/alpha", f.home.join("rel-link")).unwrap();
    std::os::unix::fs::symlink(f.home.join("projects/alpha"), f.home.join("abs-link")).unwrap();
    std::os::unix::fs::symlink("rel-link", f.home.join("chain-link")).unwrap();
    std::os::unix::fs::symlink("../home/Beta", f.home.join("up-and-back")).unwrap();
    let alpha = f.home.join("projects/alpha").display().to_string();
    for name in ["rel-link", "abs-link", "chain-link"] {
        let (status, v) = list(&f, Some(&f.home.join(name))).await;
        assert_eq!(
            (status, v["path"].clone()),
            (StatusCode::OK, json!(alpha)),
            "{name}: {v}"
        );
    }
    let (status, v) = list(&f, Some(&f.home.join("up-and-back"))).await;
    assert_eq!(
        (status, v["path"].clone()),
        (
            StatusCode::OK,
            json!(f.home.join("Beta").display().to_string())
        ),
        "{v}"
    );
    let (_, v) = list(&f, None).await;
    let listed = names(&v);
    for name in ["rel-link", "abs-link", "chain-link", "up-and-back"] {
        assert!(
            listed.contains(&name.to_string()),
            "{name} missing from {listed:?}"
        );
    }
}
