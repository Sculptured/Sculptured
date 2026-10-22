//! `/api/community/*` (C3) for the stub: the signed-in user is "you" with a live bearer key.
use super::{Block, Shared, StubState, User};
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::MutexGuard;

const RESERVED: &[&str] = &[
    "admin",
    "administrator",
    "sculptured",
    "official",
    "support",
    "help",
    "root",
    "system",
    "moderator",
    "community",
    "api",
    "about",
    "settings",
    "me",
    "people",
    "u",
];
const AVATAR_MAX: usize = 262_144;

pub(super) fn routes() -> Router<Shared> {
    Router::new()
        .route(
            "/api/community/me",
            axum::routing::get(get_me).patch(patch_me),
        )
        .route(
            "/api/community/me/avatar",
            axum::routing::put(put_avatar).delete(delete_avatar),
        )
        .route("/api/community/people", axum::routing::get(people))
        .route("/api/community/users/{h}", axum::routing::get(user_page))
        .route(
            "/api/community/users/{h}/avatar",
            axum::routing::get(user_avatar),
        )
        .route(
            "/api/community/users/{h}/report",
            axum::routing::post(ok_write),
        )
        .route(
            "/api/community/users/{h}/moderate",
            axum::routing::post(moderate_user),
        )
        .route(
            "/api/community/blocks",
            axum::routing::get(list_blocks).post(publish),
        )
        .route(
            "/api/community/blocks/{id}",
            axum::routing::get(get_block).delete(unpublish),
        )
        .route(
            "/api/community/blocks/{id}/save",
            axum::routing::post(save).delete(unsave),
        )
        .route(
            "/api/community/blocks/{id}/report",
            axum::routing::post(ok_write),
        )
        .route(
            "/api/community/blocks/{id}/moderate",
            axum::routing::post(moderate_block),
        )
        .route("/api/community/saved", axum::routing::get(saved))
        .route("/api/community/mine", axum::routing::get(mine))
        .route("/api/community/reports", axum::routing::get(reports))
}

type Guard<'a> = MutexGuard<'a, StubState>;

/// A write: a live key for an enabled "you", else the coded refusal.
fn writer<'a>(sh: &'a Shared, headers: &HeaderMap) -> Result<Guard<'a>, Response> {
    let s =
        sh.0.lock()
            .expect("shared state mutex must not be poisoned");
    match s.caller(headers)? {
        None => Err(crate::community_stub::error(
            StatusCode::UNAUTHORIZED,
            "signed_out",
        )),
        Some(()) if s.you.disabled => Err(crate::community_stub::error(
            StatusCode::FORBIDDEN,
            "account_disabled",
        )),
        Some(()) => Ok(s),
    }
}
/// A read: signed in or not; a bad key is still refused.
fn reader<'a>(sh: &'a Shared, headers: &HeaderMap) -> Result<(Guard<'a>, bool), Response> {
    let s =
        sh.0.lock()
            .expect("shared state mutex must not be poisoned");
    let signed_in = s.caller(headers)?.is_some();
    Ok((s, signed_in))
}

fn avatar_url(u: &User) -> String {
    match (&u.handle, u.avatar) {
        (Some(h), true) => format!("/api/community/users/{h}/avatar"),
        _ => {
            let initial = u.name.chars().next().unwrap_or('?');
            format!(
                "data:image/svg+xml,%3Csvg%20xmlns=%22http://www.w3.org/2000/svg%22%3E%3Ctext%3E{initial}%3C/text%3E%3C/svg%3E"
            )
        }
    }
}
fn me_json(u: &User) -> Value {
    json!({"handle": u.handle, "name": u.name, "bio": u.bio, "links": u.links, "avatar_url": avatar_url(u), "admin": u.admin, "workspace": true, "settings": {"compact": u.compact}})
}
fn member<'a>(s: &'a StubState, handle: &str) -> Option<&'a User> {
    std::iter::once(&s.you)
        .chain(&s.others)
        .find(|u| u.handle.as_deref() == Some(handle) && !u.disabled)
}
fn publication(s: &StubState, b: &Block) -> Value {
    let author = member(s, &b.author).map_or_else(
        || json!({"handle": b.author, "name": b.author, "avatar_url": ""}),
        |u| json!({"handle": b.author, "name": u.name, "avatar_url": avatar_url(u)}),
    );
    json!({"kind": b.kind, "id": b.id, "author": author, "version": b.version, "title": b.title, "description": b.description, "license": "MIT",
           "tags": ["review"], "digest": format!("sha256-{}-{}", b.id, b.version), "content": b.content, "published": b.published})
}
fn profile_json(s: &StubState, u: &User) -> Value {
    let handle = u.handle.clone().unwrap_or_default();
    let published = s
        .blocks
        .iter()
        .filter(|b| b.author == handle && b.published)
        .count();
    json!({"handle": handle, "name": u.name, "bio": u.bio, "links": u.links, "avatar_url": avatar_url(u), "joined_at": "2026-10-01T00:00:00Z", "counts": {"published": published, "saved_by_others": 0}})
}
fn listed<'a>(s: &'a StubState, keep: impl Fn(&Block) -> bool + 'a) -> Value {
    json!({"blocks": s.blocks.iter().filter(|b| b.published && !b.hidden && keep(b)).map(|b| publication(s, b)).collect::<Vec<_>>(), "next": null})
}

async fn get_me(State(sh): State<Shared>, h: HeaderMap) -> Response {
    let (s, signed_in) = match reader(&sh, &h) {
        Ok(r) => r,
        Err(e) => return e,
    };
    match signed_in {
        false => Json(json!({"user": null})).into_response(),
        true if s.you.disabled => {
            crate::community_stub::error(StatusCode::FORBIDDEN, "account_disabled")
        }
        true => Json(json!({"user": me_json(&s.you)})).into_response(),
    }
}

fn valid_handle(h: &str) -> bool {
    let edge = |c: Option<char>| c.is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    (3..=30).contains(&h.len())
        && h.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && edge(h.chars().next())
        && edge(h.chars().last())
}
fn handle_problem(s: &StubState, h: &str) -> Option<(StatusCode, &'static str)> {
    if RESERVED.contains(&h) {
        return Some((StatusCode::BAD_REQUEST, "handle_reserved"));
    }
    if !valid_handle(h) {
        return Some((StatusCode::BAD_REQUEST, "handle_invalid"));
    }
    let in_use = h == "taken" || s.others.iter().any(|u| u.handle.as_deref() == Some(h));
    in_use.then_some((StatusCode::CONFLICT, "handle_taken"))
}
#[derive(Deserialize)]
struct MePatch {
    name: Option<String>,
    handle: Option<String>,
    bio: Option<String>,
    links: Option<Vec<String>>,
    settings: Option<Value>,
}
fn patch_problem(s: &StubState, p: &MePatch) -> Option<(StatusCode, &'static str)> {
    if let Some(problem) = p
        .handle
        .as_deref()
        .filter(|h| s.you.handle.as_deref() != Some(*h))
        .and_then(|h| handle_problem(s, h))
    {
        return Some(problem);
    }
    if p.bio.as_ref().is_some_and(|b| b.chars().count() > 280) {
        return Some((StatusCode::BAD_REQUEST, "bio_too_long"));
    }
    let bad_link = |l: &String| !l.starts_with("https://") || l.len() > 200;
    p.links
        .as_ref()
        .is_some_and(|l| l.len() > 3 || l.iter().any(bad_link))
        .then_some((StatusCode::BAD_REQUEST, "link_invalid"))
}
async fn patch_me(State(sh): State<Shared>, h: HeaderMap, Json(p): Json<MePatch>) -> Response {
    let mut s = match writer(&sh, &h) {
        Ok(s) => s,
        Err(e) => return e,
    };
    if let Some((status, code)) = patch_problem(&s, &p) {
        return crate::community_stub::error(status, code);
    }
    let you = &mut s.you;
    if let Some(n) = p.name {
        you.name = n
    }
    if let Some(hd) = p.handle {
        you.handle = Some(hd)
    }
    if let Some(b) = p.bio {
        you.bio = b
    }
    if let Some(l) = p.links {
        you.links = l
    }
    if let Some(c) = p.settings.as_ref().and_then(|v| v["compact"].as_bool()) {
        you.compact = c
    }
    Json(json!({"user": me_json(you)})).into_response()
}

async fn put_avatar(State(sh): State<Shared>, h: HeaderMap, body: Bytes) -> Response {
    let mut s = match writer(&sh, &h) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let ty = h
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if !matches!(ty, "image/png" | "image/jpeg" | "image/webp") {
        return crate::community_stub::error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "avatar_type");
    }
    if body.len() > AVATAR_MAX {
        return crate::community_stub::error(StatusCode::PAYLOAD_TOO_LARGE, "avatar_too_large");
    }
    s.you.avatar = true;
    Json(json!({"avatar_url": avatar_url(&s.you)})).into_response()
}
async fn delete_avatar(State(sh): State<Shared>, h: HeaderMap) -> Response {
    let mut s = match writer(&sh, &h) {
        Ok(s) => s,
        Err(e) => return e,
    };
    s.you.avatar = false;
    Json(json!({"avatar_url": avatar_url(&s.you)})).into_response()
}

async fn people(State(sh): State<Shared>, h: HeaderMap) -> Response {
    let (s, _) = match reader(&sh, &h) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let all: Vec<Value> = std::iter::once(&s.you)
        .chain(&s.others)
        .filter(|u| u.handle.is_some() && !u.disabled)
        .map(|u| profile_json(&s, u))
        .collect();
    Json(json!({"people": all, "next": null})).into_response()
}
async fn user_page(State(sh): State<Shared>, h: HeaderMap, Path(handle): Path<String>) -> Response {
    let (s, _) = match reader(&sh, &h) {
        Ok(r) => r,
        Err(e) => return e,
    };
    if handle == "old-ana" {
        // Absolute on purpose: the app must turn it into a local path (R6).
        return (
            StatusCode::MOVED_PERMANENTLY,
            [(
                header::LOCATION,
                format!("{}/api/community/users/ana", sh.1),
            )],
        )
            .into_response();
    }
    match member(&s, &handle) {
        Some(u) => Json(json!({"profile": profile_json(&s, u), "blocks": listed(&s, |b| b.author == handle)["blocks"]})).into_response(),
        None => crate::community_stub::error(StatusCode::NOT_FOUND, "not_found"),
    }
}
/// Every picture is an SVG with a script in it, so the app's sandbox on it is tested.
async fn user_avatar() -> Response {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><script>alert(1)</script><rect width="8" height="8"/></svg>"#;
    (
        [
            (header::CONTENT_TYPE, "image/svg+xml"),
            (header::ETAG, "\"av-1\""),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        svg,
    )
        .into_response()
}
async fn ok_write(State(sh): State<Shared>, h: HeaderMap) -> Response {
    match writer(&sh, &h) {
        Ok(_) => Json(json!({"ok": true})).into_response(),
        Err(e) => e,
    }
}
async fn moderate_user(
    State(sh): State<Shared>,
    h: HeaderMap,
    Path(handle): Path<String>,
    Json(b): Json<Value>,
) -> Response {
    let mut s = match writer(&sh, &h) {
        Ok(s) => s,
        Err(e) => return e,
    };
    if b["disable"].as_bool() == Some(true) && s.you.handle.as_deref() == Some(handle.as_str()) {
        s.you.disabled = true
    }
    Json(json!({"ok": true})).into_response()
}

#[derive(Deserialize)]
struct Search {
    #[serde(default)]
    q: String,
}
async fn list_blocks(State(sh): State<Shared>, h: HeaderMap, Query(p): Query<Search>) -> Response {
    let (s, _) = match reader(&sh, &h) {
        Ok(r) => r,
        Err(e) => return e,
    };
    match p.q.as_str() {
        "huge" => (
            [(header::CONTENT_TYPE, "application/json")],
            format!(
                "{{\"blocks\":[],\"pad\":\"{}\"}}",
                "x".repeat(2 * 1024 * 1024 + 10)
            ),
        )
            .into_response(),
        "html" => ([(header::CONTENT_TYPE, "text/html")], "<html>hi</html>").into_response(),
        "legacy" => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid search"})),
        )
            .into_response(),
        q => Json(listed(&s, |b| {
            b.title.to_lowercase().contains(&q.to_lowercase())
        }))
        .into_response(),
    }
}
async fn get_block(State(sh): State<Shared>, h: HeaderMap, Path(id): Path<String>) -> Response {
    let (s, _) = match reader(&sh, &h) {
        Ok(r) => r,
        Err(e) => return e,
    };
    match s
        .blocks
        .iter()
        .find(|b| b.id == id && b.published && !b.hidden)
    {
        Some(b) => Json(publication(&s, b)).into_response(),
        None => crate::community_stub::error(StatusCode::NOT_FOUND, "not_found"),
    }
}
async fn unpublish(State(sh): State<Shared>, h: HeaderMap, Path(id): Path<String>) -> Response {
    let mut s = match writer(&sh, &h) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let mine = s.you.handle.clone();
    let owned_blocks = s
        .blocks
        .iter_mut()
        .filter(|block| block.id == id && Some(&block.author) == mine.as_ref());
    for block in owned_blocks {
        block.published = false;
    }
    Json(json!({"ok": true})).into_response()
}
async fn publish(State(sh): State<Shared>, h: HeaderMap, Json(b): Json<Value>) -> Response {
    let mut s = match writer(&sh, &h) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(author) = s.you.handle.clone() else {
        return crate::community_stub::error(StatusCode::CONFLICT, "handle_required");
    };
    let id = format!("b-new-{}", s.blocks.len() + 1);
    let text = |k: &str| b[k].as_str().unwrap_or_default().to_string();
    s.blocks.push(Block {
        id: id.clone(),
        kind: "group",
        author,
        version: 1,
        title: text("title"),
        description: text("description"),
        content: b["content"].clone(),
        published: true,
        hidden: false,
    });
    Json(json!({"id": id, "version": 1})).into_response()
}
async fn save(State(sh): State<Shared>, h: HeaderMap, Path(id): Path<String>) -> Response {
    let mut s = match writer(&sh, &h) {
        Ok(s) => s,
        Err(e) => return e,
    };
    s.saved.insert(id);
    Json(json!({"ok": true})).into_response()
}
async fn unsave(State(sh): State<Shared>, h: HeaderMap, Path(id): Path<String>) -> Response {
    let mut s = match writer(&sh, &h) {
        Ok(s) => s,
        Err(e) => return e,
    };
    s.saved.remove(&id);
    Json(json!({"ok": true})).into_response()
}
async fn saved(State(sh): State<Shared>, h: HeaderMap) -> Response {
    let s = match writer(&sh, &h) {
        Ok(s) => s,
        Err(e) => return e,
    };
    Json(listed(&s, |b| s.saved.contains(&b.id))).into_response()
}
async fn mine(State(sh): State<Shared>, h: HeaderMap) -> Response {
    let s = match writer(&sh, &h) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let me = s.you.handle.clone();
    Json(listed(&s, move |b| Some(&b.author) == me.as_ref())).into_response()
}
/// Moderators only; "you" is not one, so this is the coded 403 unless a test makes "you" an admin.
async fn reports(State(sh): State<Shared>, h: HeaderMap) -> Response {
    let s = match writer(&sh, &h) {
        Ok(s) => s,
        Err(e) => return e,
    };
    if !s.you.admin {
        return crate::community_stub::error(StatusCode::FORBIDDEN, "forbidden");
    }
    Json(json!({"reports": [
        {"id": "r1", "reason": "Spam", "kind": "block", "block": {"id": super::SEED_BLOCK, "title": "Review pair"}, "handle": null, "created_at": "2026-10-18T10:00:00Z"},
        {"id": "r2", "reason": "Rude bio", "kind": "profile", "block": null, "handle": "ana", "created_at": "2026-10-18T11:00:00Z"},
    ]})).into_response()
}
async fn moderate_block(
    State(sh): State<Shared>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<Value>,
) -> Response {
    let mut s = match writer(&sh, &h) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let hidden = b["hidden"].as_bool().unwrap_or(true);
    let matching_blocks = s.blocks.iter_mut().filter(|block| block.id == id);
    for block in matching_blocks {
        block.hidden = hidden;
    }
    Json(json!({"ok": true})).into_response()
}
