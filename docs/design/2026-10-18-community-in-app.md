# Community in the App (sculptured repo) Implementation Plan


**Goal:** Make Community a top-bar section of the app in every build. Locally it is opt-in: controld pairs a `community`-scope device key and proxies an allow-listed `/api/community/*` to the Sculptured server. The web app gets Explore, People, profiles, Saved, Yours, Publish and Moderation pages. "Add to project" and "Save to Library" work directly against the local Library.

**Architecture:**
- **controld** gains a `community` module:
  - the `sculptured_origin` setting;
  - `community.json` credentials (0600);
  - `/api/community-link` endpoints;
  - a pairing task that reuses the device-pairing protocol now shared with `sculptured connect`;
  - a header-allow-listing reverse proxy built on the `reqwest` client controld already uses for analytics.
- **The web app** talks only to relative URLs:
  - `community/api.ts` for C3;
  - `community/link.ts` for C5, with hosted mode deriving link state from the gateway session instead.
- **Pages** follow the redesign's page-per-job pattern:
  - they sit inside the Shell;
  - each page is built and tested on its own;
  - the pages are routed together in Task 14;
  - old code is removed in Task 15.
- **Tests:** an in-process and standalone stub upstream (`crates/fixtures`) stands in for the gateway and Community in every test.

**Tech Stack:** Rust 2024 (axum 0.8, reqwest 0.13 + rustls/ring, ts-rs 12, tokio), React 19 + TypeScript, react-router 7, vitest 5, Playwright, Mintlify docs (`.mdx`).

**Spec:**
- `docs/design/2026-10-18-community-integration-design.md`
- `docs/design/2026-10-18-community-contracts.md`

The contracts win where they refine the design.

## Global Constraints

**Tooling and generated types**
- Tooling is Rust only. No shell or Python scripts. JS tooling runs through `npm --prefix …`. Work on a branch (`community-in-app`), never on `main`. Commit only the paths a task names; other sessions are editing the same tree.
- TS API types come from ts-rs: `cargo test --workspace export_bindings` writes `web/src/generated/`. Community types are written to `web/src/generated/community/` by `cargo test export_bindings` run in `../sculptured-community`. Never hand-edit either.

**Plain words**
- UI words are Community, People, Profile and Block. "Group" stays the Library word.
- Never write "member", "rig", "pod" or "seat" in Community copy. New copy lives in `web/src/vocab.ts` (`COMMUNITY_WORDS`).
- `profile` is an OpenRig word in `test-vocab.ts`. Community pages are checked with `openRigWords(root, ['profile', 'profiles'])` (Task 6).

**Errors and status**
- Server errors appear only as plain words from `web/src/errors.ts`, keyed by code. The upstream's `message` is never shown.
- Status is shown as shape plus text.
- A disabled control says why in visible text.

**Opt-in and credentials**
- A self-run install sends no request to any Sculptured server until the user turns Community on. When `Settings.sculptured_origin` is `None`, Community is off.
- Credentials live in `<data_dir>/community.json`, mode `0600`, as `{origin, device_id, key}`.
- No browser-facing response ever contains `key`, `device_id` or a `Bearer` value.

**Fixed values (C3, C5)**

| What | Value |
|---|---|
| Default origin | `https://app.usesculptured.com` |
| Allowed origins | `https://…`, or `http://127.0.0.1:<port>` / `http://localhost:<port>` |
| Proxy request body | ≤ 300 KB (`307200` bytes) |
| Proxy response | ≤ 2 MB (`2097152` bytes) |
| Proxy timeout | 15 s |
| Avatar | ≤ `262144` bytes, `image/png\|jpeg\|webp` |
| Handle | `^[a-z0-9](?:[a-z0-9-]{1,28}[a-z0-9])$` |
| Bio | ≤ 280 characters |
| Links | ≤ 3, each `https://` and ≤ 200 characters |

Reserved handles: `admin, administrator, sculptured, official, support, help, root, system, moderator, community, api, about, settings, me, people, u`.

**Lint limits**
- Web lint applies `max-lines-per-function` 30 (tests exempt) and `max-lines` 300 per file. Clippy warns at `too_many_lines` 30. Split components and handlers to fit.
- `npm --prefix web run lint` must not exceed the problem count recorded before Task 1. It was 136 on 2026-10-18. Re-record it because other sessions are editing.

**Areas to leave alone**
- Board files (`web/src/structure/*`, `web/src/draft/*`), the Input block, and the Advanced section of `pages/SettingsPage.tsx`. Community imports `TemplatePreview`, `BlockDetails`, `BlockFeatures`, `AddToProject` and `insertBlock` read-only.
- `shell/TopBar.tsx` and `shell/commands.ts` changed recently. Re-read them before editing and keep each edit to the lines named.

**Block kinds**
- A Community block may gain a `kind`. "Add to project" and "Save to Library" are keyed on `blockKind(pub)`.
- `group`, the default when `kind` is absent, is implemented.
- Any other kind shows "Opens in a later version." in place of both actions.

**No real network**
- Routine tests make no third-party requests. controld tests use `fixtures::community_stub`.
- Playwright runs `fake-community`. `e2e-server` points `sculptured serve --community-origin` at it.

### Contract refinements this plan implements (reported upstream; see the report)

- **R1.** The proxy strips `Origin` and does not replace it. C4 exempts bearer requests from the gateway's Origin check.
  - The unauthenticated device endpoints (`/gw/api/device/start`, `/token`) are POSTs that do go through that check. controld sends `Origin: {origin}` on them, as `connect.rs` already does.
  - The best-effort revoke (R3) sends `Origin: {origin}` for the same reason.
  - **401 `device_unknown`:** an invalid, revoked or wrong-scope bearer gets `401 {"error":{"code":"device_unknown"}}` from the gateway (C4, C5). This applies both to the account check (`GET {origin}/api/community/me`) and to any proxied answer. controld then deletes `community.json`, keeps the origin, and passes the 401 on. The web shows "Your Community sign-in was removed. Sign in again." and refreshes the link.
- **R2.** `GET /api/community-link/pair` can also answer `"denied"`. It does so when the token poll gets any 4xx other than 428 or 410.
- **R3.** `DELETE /api/community-link` revokes the key best-effort with `POST {origin}/gw/api/device/revoke` and `Authorization: Bearer …`. That endpoint is needed in C4. Until it exists, the 404 is ignored.
- **R4.** controld's own proxy error codes:
  - `community_unreachable` (502)
  - `community_timeout` (504)
  - `community_too_large` (413)
  - `community_bad_answer` (502)
  - `community_origin_invalid` (400)
  - a path outside the allow-list gets `not_found` (404)
- **R5.** The allow-list is enumerated in Task 5. It is the C3 table plus the "as today" routes. It excludes the legacy cookie routes `login`, `callback`, `logout` and `sessions/revoke`.
- **R6.** The proxy never follows redirects. It rewrites `Location: {origin}/api/community/…` to `/api/community/…` and drops any other `Location`.
- **R7.** Every proxied response gets `Content-Security-Policy: sandbox; default-src 'none'; img-src 'self' data:; style-src 'unsafe-inline'` and `X-Content-Type-Options: nosniff`. An upstream SVG avatar opened directly on the local origin must not run script.
- **R8.** The web app trusts `avatar_url` only when it starts with `/api/community/users/` or `data:image/svg+xml,` (C3 initials). Otherwise it shows initials.
  - The app sets no CSP today: there is none in `web/index.html` and none in controld's `web_cache` layer. Neither is added here.
  - The proxied-response CSP (R7) includes `img-src 'self' data:`, as C3 requires.
- **R9.** Hosted mode never calls `/api/community-link`. Link state comes from `useHosted()`. `HostedUser.workspace?: boolean` is read from `/gw/api/me`, which now has `user.workspace` in C4; a missing value counts as `true`.
- **R10.** A `GET reports` item is read as `{id, reason, kind: 'block'|'profile', block?: string|null, handle?: string|null}`. C3 now says items gain `kind` and `handle`. The exact type of `block` on a profile report is not stated, so it is read as optional.
- **R11.** Community errors use the coded shape `{"error":{"code","message"}}` (C3). If an older Community answers the legacy `{"error":"<message>"}` shape, `apiRequest` treats it as `community_bad_answer` and never shows the string.

## Review Focus

1. **Community off sends nothing.** Before "Turn on", no request reaches any upstream: not from controld (stub request log empty) and not from the browser (no `/api/community/` fetch). Pinned in Task 3 (`community_is_off_until_turned_on_and_off_makes_no_request`), Task 14 (`the community section asks first and fetches nothing while off`) and Task 16 (`Community stays off and silent until turned on`).
2. **A pairing that expires or is turned down.** The page says so in words and offers "Get a new code". Nothing is stored. Pinned in Task 4 (`a_denied_pairing_reports_denied_and_stores_nothing`, `an_expired_pairing_reports_expired`) and Task 7 (`an expired or turned-down code says so and offers a new one`).
3. **Upstream down or slow.** After 15 s at most, the page shows plain words and a Retry, never a spinner forever or raw text. Pinned in Task 5 (`a_slow_server_times_out_as_community_timeout`, `a_server_that_is_down_is_community_unreachable`) and Task 8 (`a slow or unreachable Community shows plain words and Retry`).
4. **Oversized avatar.** Refused before upload when the cropped image is still over 262144 bytes, and server `avatar_too_large` is shown in words. Pinned in Task 11 (`a picture still over 256 KB after cropping is refused before upload`, `server avatar errors are shown in words`).
5. **A handle taken while editing.** `handle_taken` shows under the Handle field. The other fields keep what was typed. Pinned in Task 11 (`a handle taken while editing shows under Handle and keeps the other fields`) and Task 16.
6. **A proxy path outside the allow-list.** Answers 404 `not_found` and never reaches upstream. This includes encoded slashes and `..`. Pinned in Task 5 (`only_allow_listed_methods_and_paths_reach_the_server`).
7. **Credentials in browser responses.** Neither the key, the device id nor `Bearer` appears in any body or header controld returns. Upstream `Set-Cookie` is dropped. Pinned in Task 5 (`credentials_never_appear_in_any_browser_response`, `upstream_cookies_and_unknown_headers_never_reach_the_browser`).
8. **A script inside an upstream SVG avatar.** Opened directly on `127.0.0.1:4410`, it must not run. Pinned in Task 5 (`every_proxied_answer_is_sandboxed_and_nosniff`).
9. **A key removed on the Computers page while the app is open.** The next request answers `device_unknown`. The key is deleted, the origin is kept, and the page says "Your Community sign-in was removed. Sign in again." Pinned in Task 3 (`a_key_the_server_no_longer_accepts_signs_out_and_keeps_the_origin`), Task 5 (`a_revoked_key_on_any_request_signs_out_and_keeps_the_origin`) and Task 6 (`a device_unknown answer refreshes the link so the page shows signed out`).

---

## File map

**controld (Rust)**

| File | Change |
|---|---|
| `crates/controld/src/views/settings.rs` | Modify: `community_url` → `sculptured_origin` |
| `crates/controld/src/settings.rs` | Modify: `migrate_origin`, `valid_origin`, PUT validation |
| `crates/controld/src/device_pairing.rs` | Create: device-code protocol, shared with `connect.rs` |
| `crates/controld/src/community/mod.rs` | Create: `CommunityLink`, constants, routes, TS views |
| `crates/controld/src/community/creds.rs` | Create: `community.json` |
| `crates/controld/src/community/link.rs` | Create: `/api/community-link` GET, enable, DELETE |
| `crates/controld/src/community/pairing.rs` | Create: POST/GET pair and the poll task |
| `crates/controld/src/community/allow.rs` | Create: allow-list |
| `crates/controld/src/community/proxy.rs` | Create: the forwarder |
| `crates/controld/src/{lib,state,api}.rs`, `crates/controld/Cargo.toml` | Modify: wiring |
| `crates/sculptured/src/main.rs` | Modify: `--community-origin` |
| `crates/sculptured/src/connect.rs` | Modify: `pair` uses `device_pairing` |
| `crates/model/src/group_block.rs` | Modify: `source: Option<BlockSource>` |
| `crates/controld/src/group_blocks.rs` | Modify: validate `source` |

**Stub upstream**

| File | Change |
|---|---|
| `crates/fixtures/src/community_stub/{mod,devices,api,seed}.rs` | Create: stub gateway and Community |
| `crates/fixtures/src/bin/fake-community.rs` | Create: standalone stub for e2e |
| `crates/fixtures/community/review-pair.group.json` | Create: seed group block |
| `crates/e2e-server/src/{main,lib}.rs` | Modify: run the stub |

**Web**
- `web/src/community/`, all created:
  - data: `api.ts`, `link.ts`, `context.ts`, `kinds.ts`, `handles.ts`, `avatarCrop.ts`, `saveToLibrary.ts`;
  - test helpers: `testing.tsx`;
  - styles: `community.css`;
  - one file per page and component, as named in each task;
  - routing: `routes.tsx`.
- Modified:
  - `web/src/api.ts`: exports `apiRequest`, adds PATCH and Blob bodies.
  - `web/src/errors.ts`, `web/src/vocab.ts`, `web/src/test-vocab.ts`.
  - `web/src/App.tsx`, `shell/TopBar.tsx`, `shell/commands.ts`.
  - `hosted/HostedGate.tsx`, `hosted/session.ts`, `account/AccountPage.tsx`.
  - `groups/GroupLibrary.tsx`, `release/sharing.ts`, `shell/LibraryPage.tsx`.
- Deleted:
  - `release/{Community,Profile,LocalCommunity,Nav,ReleasePage,workspaceLink}.tsx|ts` and their tests;
  - `groups/blockTransfer.ts`;
  - `generated/{Publication,CommunitySession,CommunityUser}.ts`.

**Docs**
- `docs-site/guides/community.mdx`, `guides/groups.mdx`, `security.mdx`, `privacy.mdx`
- `docs/internals/community.md` (new), `docs/internals/README.md`, `docs/internals/testing.md`
- `CHANGELOG.md`

**Order of work**
- Tasks 1–5 are controld work and need nothing from the other repos.
- Task 6 needs the community repo's ts-rs export. Its Step 1 says how to tell whether that is ready. Do not hand-write the types.
- Tasks 7–13 build pages that are not yet routed.
- Task 14 routes them.
- Task 15 deletes the old code.
- Task 16 is e2e.
- Task 17 is docs.

---

### Task 1: `sculptured_origin` replaces `community_url`

**Files:**
- Modify: `crates/controld/src/views/settings.rs:6-25`
- Modify: `crates/controld/src/settings.rs` (`settings_in`, `put_settings`, new fns, new `#[cfg(test)] mod tests`)
- Modify: `web/src/release/LocalCommunity.tsx`, `web/src/release/LocalCommunity.test.tsx` (field rename only; both are deleted in Task 15)
- Modify: `desktop/main.mjs`: delete `useAccountForCommunity` and its call. Desktop must not turn Community on silently.
- Regenerate: `web/src/generated/Settings.ts`
- Test: `crates/controld/tests/community_settings.rs`

**Interfaces:**
- Produces:
  - `views::Settings.sculptured_origin: Option<String>`, declared as `#[serde(default, alias = "community_url", skip_serializing_if = "Option::is_none")] #[ts(optional)]`.
  - `pub fn controld::settings::migrate_origin(url: &str) -> String`: `https://community.X…` → `https://app.X…`; anything else unchanged.
  - `pub fn controld::settings::valid_origin(raw: &str) -> Result<String, ApiError>`. It returns the origin with no trailing slash. It errors with `400 community_origin_invalid` "Use an https:// address".
  - `settings_in` applies `migrate_origin` to the value it reads.

- [ ] **Step 1: Write the failing tests.** Unit tests go in `settings.rs`. Integration tests go in `tests/community_settings.rs` and use `common::m2::{serve, call, get}`.

```rust
#[test]
fn old_community_url_is_read_as_sculptured_origin_and_community_host_maps_to_app() {
    let store = Store::open_in_memory().unwrap();
    store.connection().execute("INSERT INTO settings (k, json) VALUES ('global', ?1)",
        [r#"{"community_url":"https://community.example.com","posture":"unrestricted","question_timeout_min":10}"#]).unwrap();
    assert_eq!(store.settings().unwrap().sculptured_origin.as_deref(), Some("https://app.example.com"));
}
#[test]
fn a_self_hosted_origin_is_kept_as_is() {
    assert_eq!(migrate_origin("https://sculptured.my.org"), "https://sculptured.my.org");
    assert_eq!(migrate_origin("http://127.0.0.1:9000"), "http://127.0.0.1:9000");
}
#[test]
fn saving_writes_sculptured_origin_and_never_community_url() { /* save Settings{sculptured_origin:Some("https://app.x")}, read raw json: contains "sculptured_origin", not "community_url" */ }
#[test]
fn valid_origin_takes_https_and_loopback_http_only() {
    assert_eq!(valid_origin("https://app.x.com/").unwrap(), "https://app.x.com");
    assert!(valid_origin("http://127.0.0.1:14411").is_ok() && valid_origin("http://localhost:3000").is_ok());
    for bad in ["http://app.x.com", "https://u:p@x.com", "https://x.com/path", "https://x.com?q=1", "ftp://x", "", &format!("https://{}.com", "a".repeat(300))] {
        assert_eq!(valid_origin(bad).unwrap_err().code, "community_origin_invalid", "{bad}");
    }
}
```

```rust
// tests/community_settings.rs
#[tokio::test]
async fn put_settings_refuses_a_plain_http_origin_with_community_origin_invalid() {
    let s = serve().await;
    let (_, cur) = get(&s.router, "/api/settings").await;
    let mut next = cur.clone(); next["sculptured_origin"] = json!("http://evil.example");
    let (st, body) = call(&s.router, "PUT", "/api/settings", json!({"settings": next})).await;
    assert_eq!((st.as_u16(), body["error"]["code"].as_str()), (400, Some("community_origin_invalid")));
}
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p controld settings::tests` and `cargo test -p controld --test community_settings`. Expected: compile errors (`sculptured_origin`, `migrate_origin` and `valid_origin` are not defined).
- [ ] **Step 3: Implement.**
  - Rename the field in `views/settings.rs` and its `Default`.
  - Add `migrate_origin` and `valid_origin` to `settings.rs`. Parse with `reqwest::Url`, which is already a dependency.
  - `put_settings` calls `valid_origin` on `Some` values and stores the normalized string.
  - In `LocalCommunity.tsx` and its test, rename `community_url` → `sculptured_origin`.
  - Remove `useAccountForCommunity` and its call from `desktop/main.mjs`, and the header comment's mention of it.
- [ ] **Step 4: Regenerate and run.** Run `cargo test --workspace export_bindings && cargo test -p controld settings:: --test community_settings && npm --prefix web test -- src/release && npm --prefix desktop test && npm --prefix web run build`. Expected: all PASS. `web/src/generated/Settings.ts` shows `sculptured_origin?: string`.
- [ ] **Step 5: Lint and commit.** Run `cargo clippy -p controld --all-targets` and `npm --prefix web run lint 2>&1 | tail -1`. Expected: the count is at or below baseline.

```bash
git add crates/controld/src/views/settings.rs crates/controld/src/settings.rs crates/controld/tests/community_settings.rs web/src/generated/Settings.ts web/src/release/LocalCommunity.tsx web/src/release/LocalCommunity.test.tsx desktop/main.mjs
git commit -m "Rename the Community setting to sculptured_origin and migrate community_url"
```

---

### Task 2: Stub upstream in `crates/fixtures`

**Files:**
- Modify: `crates/fixtures/Cargo.toml`. Add `[dependencies]`: `axum`, `tokio`, `serde`, `serde_json` and `model` (workspace).
- Create:
  - `crates/fixtures/src/community_stub/mod.rs`: state, `spawn`, control routes.
  - `crates/fixtures/src/community_stub/devices.rs`: `/gw/api/device/*`.
  - `crates/fixtures/src/community_stub/api.rs`: `/api/community/*`.
  - `crates/fixtures/src/community_stub/seed.rs`: seed users and blocks.
- Create: `crates/fixtures/src/bin/fake-community.rs`, run as `fake-community --port <u16>`. It prints `listening on http://127.0.0.1:<port>` and serves until killed.
- Create: `crates/fixtures/community/review-pair.group.json`. It is a valid `model::GroupBlock` with one group `g`, two `practice` agents `a` and `b` in it, one `hands-work-to` connection a→b and no workflow.
- Modify: `crates/fixtures/src/lib.rs` to add `pub mod community_stub;`.
- Test: `crates/fixtures/tests/community_stub.rs`

**Interfaces:**
- Produces, for Tasks 3–5 and 16:

```rust
pub struct Stub { pub origin: String, pub state: Arc<Mutex<StubState>> }
pub async fn spawn() -> Stub                                    // 127.0.0.1:0
pub async fn spawn_on(port: u16) -> std::io::Result<Stub>
pub fn router(state: Arc<Mutex<StubState>>, origin: String) -> axum::Router
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)] #[serde(rename_all = "lowercase")]
pub enum PairingMode { Pending, Approve, Deny, Expire }
#[derive(Clone, Debug, Serialize)]
pub struct Seen { pub method: String, pub path_and_query: String, pub headers: Vec<(String, String)>, pub body: Vec<u8> }
impl Stub {
    pub fn requests(&self) -> Vec<Seen>;
    pub fn set_pairing(&self, m: PairingMode);
    pub fn set_delay(&self, d: Duration);            // held before every /api/community/* answer
    pub fn issue_key(&self) -> (String, String);     // (device_id, key) for the user "you", handle None
    pub fn bump_version(&self, block_id: &str);      // the block's version + 1
    pub fn revoke_all(&self);                         // every issued key now gets 401 device_unknown
}
pub const SEED_BLOCK: &str = "b-review-pair";        // group kind, author ana, version 1
pub const SEED_OTHER_KIND: &str = "b-kindex-setup";  // "kind": "component"
```

- Stub behaviour:
  - **Every request** is appended to `requests`.
  - **Every `/api/community/*` answer** carries `Set-Cookie: stub=1`, `X-Upstream-Secret: s` and `Cache-Control: no-store`. Avatar answers carry `ETag` and `Cache-Control: public, max-age=3600` instead.
  - **Device endpoints:**

    | Endpoint | Answer |
    |---|---|
    | `POST /gw/api/device/start` | `{device_code:"dc-1", user_code:"ABCD-EFGH", verify_url:"{origin}/connect?code=ABCD-EFGH", interval:1, expires_in:600}`; records the JSON body |
    | `POST /gw/api/device/token` | by `PairingMode`: Pending → `428 authorization_pending`; Approve → `200 {device_id, device_key}` once, then 410; Deny → `403 {"error":{"code":"access_denied"}}`; Expire → `410 expired_token` |
    | `POST /gw/api/device/revoke` | bearer → `{ok:true}`; the key stops working |

  - **Bearer:** `Authorization: Bearer <id>.<key>` resolves to the user "you" (`name "You Example"`, handle starts `None`, `admin: false`). No `Authorization` header counts as signed out. An `Authorization: Bearer` that is unknown or revoked gets `401 {"error":{"code":"device_unknown"}}` on every `/api/community/*` route (C4).
  - **Community endpoints:**

    | Endpoint | Answer |
    |---|---|
    | `GET me` | `{user:null}` when signed out |
    | `PATCH me` | applies C3 rules: `handle_invalid`, `handle_reserved`, `handle_taken` (409, for `taken` or any handle in use), `bio_too_long`, `link_invalid`; 401 `signed_out` without a bearer |
    | `PUT me/avatar` | `avatar_too_large` (413) over 262144 bytes; `avatar_type` (415) for other content types |
    | `DELETE me/avatar` | as C3 |
    | `GET people` | as C3 |
    | `GET users/{h}` | as C3; `users/old-ana` → `301 Location: {origin}/api/community/users/ana` (absolute, to exercise R6) |
    | `GET users/{h}/avatar` | `image/svg+xml` containing `<script>alert(1)</script>` |
    | `GET blocks` | `?q=huge` returns a JSON body over 2 MB; `?q=html` returns `text/html` |
    | `GET`/`DELETE blocks/{id}` | as today |
    | `POST blocks` | 409 `handle_required` when "you" has no handle |
    | `POST`/`DELETE blocks/{id}/save`, `GET saved`, `GET mine` | as today |
    | `POST blocks/{id}/report` | as today |
    | `POST users/{h}/report` | `{ok:true}` |
    | `GET reports` | admin only, otherwise a coded 403; one block report and one profile report (R10 shape: `kind`, `handle`) |
    | `POST blocks/{id}/moderate` | as today |
    | `POST users/{h}/moderate` | `{ok:true}` |

    Publications carry `author: {handle, name, avatar_url}` and no `owner`, plus `kind: "group"` (`SEED_OTHER_KIND` has `"component"`). `avatar_url` is `"/api/community/users/{handle}/avatar"` for ana and a `data:image/svg+xml,…` initials URL for an author with no picture.

    `Me.user` carries `settings: {compact: boolean}`; `PATCH me` accepts `settings`.

    Every error uses `{"error":{"code","message"}}`:
    - writes without a bearer → 401 `signed_out`;
    - `?q=legacy` on `GET blocks` → 400 with the legacy `{"error":"text"}` shape, for R11;
    - a user disabled via `moderate` → 403 `account_disabled`.
  - **Control routes**, used by Playwright only:
    - `POST /stub/reset`
    - `POST /stub/pairing {mode}`
    - `POST /stub/delay {ms}`
    - `POST /stub/blocks/{id}/bump`
    - `GET /stub/requests`, which excludes `/stub/*`

- [ ] **Step 1: Write the failing tests** (`crates/fixtures/tests/community_stub.rs`, using `reqwest`, which is added as a dev-dependency with `json`).

```rust
#[tokio::test] async fn pairing_is_pending_until_approved_then_gives_a_key_once() { /* start; token→428; set_pairing(Approve); token→200 with device_id+device_key; token again→410 */ }
#[tokio::test] async fn a_denied_pairing_answers_403_and_an_expired_one_410() {}
#[tokio::test] async fn the_bearer_of_a_paired_key_is_signed_in_and_others_are_not() {
    // GET me with issue_key() bearer → user.name == "You Example"; without → {"user":null}
}
#[tokio::test] async fn patch_me_refuses_a_taken_handle_with_handle_taken() { /* 409, error.code == "handle_taken" */ }
#[tokio::test] async fn avatar_over_262144_bytes_is_avatar_too_large() { /* 262145 bytes image/png → 413 avatar_too_large; 1000 bytes → 200 */ }
#[tokio::test] async fn every_request_is_recorded_with_its_headers() { /* send Cookie: a=1; requests()[0].headers has ("cookie","a=1") */ }
#[tokio::test] async fn delay_holds_every_community_answer() { /* set_delay(300ms); GET blocks takes >= 300ms */ }
#[test] fn the_seed_block_is_a_valid_group_block() {
    let b: model::GroupBlock = serde_json::from_str(include_str!("../community/review-pair.group.json")).unwrap();
    assert_eq!((b.draft.groups.len(), b.draft.agents.len(), b.draft.connections.len()), (1, 2, 1));
}
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p fixtures --test community_stub`. Expected: compile error (no `community_stub`).
- [ ] **Step 3: Implement the stub.** Use one `Arc<Mutex<StubState>>`. Add a `middleware::from_fn` layer that records each request (buffering the body) and applies `delay`. Keep each handler at 30 lines or fewer. `fake-community` parses `--port` from `std::env::args` and calls `spawn_on`.
- [ ] **Step 4: Run them to see them pass.** Run `cargo test -p fixtures && cargo build -p fixtures --bin fake-community && cargo clippy -p fixtures --all-targets`. Expected: PASS, no new warnings.
- [ ] **Step 5: Commit.**

```bash
git add crates/fixtures && git commit -m "Add a stub gateway and Community upstream for tests"
```

---

### Task 3: controld Community link (on, off, signed-in state)

**Files:**
- Create:
  - `crates/controld/src/community/mod.rs`
  - `crates/controld/src/community/creds.rs`
  - `crates/controld/src/community/link.rs`
- Modify:
  - `crates/controld/src/lib.rs`: add `pub mod community;`.
  - `crates/controld/src/state.rs`: add the field and builder.
  - `crates/controld/src/api.rs`: `.merge(crate::community::routes())`.
- Modify `crates/controld/Cargo.toml`:
  - `reqwest` features `["json", "rustls-no-provider"]`, because controld now makes https calls in local mode;
  - dev-dependency `fixtures = { path = "../fixtures" }`.
- Modify: `crates/sculptured/src/main.rs`.
  - `Serve` gets `#[arg(long, env = "SCULPTURED_COMMUNITY_ORIGIN", default_value = controld::community::DEFAULT_ORIGIN)] community_origin: String`.
  - When not `--hosted`, `serve()` chains `.with_community(CommunityLink::new(data_dir.clone(), community_origin))`.
- Create: `crates/controld/tests/common/community.rs` (and `pub mod community;` in `tests/common/mod.rs`)
- Test: `crates/controld/tests/community_link.rs`

**Interfaces:**
- Consumes: Task 1 `valid_origin`, `Settings.sculptured_origin`; Task 2 `community_stub::{spawn, Stub}`.
- Produces:

```rust
// community/mod.rs
pub const DEFAULT_ORIGIN: &str = "https://app.usesculptured.com";
pub const PROXY_TIMEOUT: Duration = Duration::from_secs(15);
pub const REVOKE_TIMEOUT: Duration = Duration::from_secs(5);
pub const ACCOUNT_TTL: Duration = Duration::from_secs(60);
pub struct CommunityLink { /* data_dir, default_origin, http: reqwest::Client (redirect none, timeout), timeout, pairing, account cache */ }
impl CommunityLink {
    pub fn new(data_dir: PathBuf, default_origin: String) -> Self;   // installs the ring provider like analytics.rs
    pub fn with_timeout(self, d: Duration) -> Self;                   // tests only shorten it
    pub fn data_dir(&self) -> &Path;
}
#[derive(Serialize, TS)] #[ts(export)] pub struct CommunityLinkView { pub enabled: bool, pub origin: Option<String>, pub signed_in: bool, pub account: Option<CommunityLinkAccount> }
#[derive(Serialize, TS)] #[ts(export)] pub struct CommunityLinkAccount { pub name: String, pub handle: Option<String> }
pub fn routes() -> Router<AppState>;  // /api/community-link (GET, DELETE), /api/community-link/enable (POST)
// community/creds.rs
pub const FILE: &str = "community.json";
#[derive(Clone, Serialize, Deserialize)] pub struct Credentials { pub origin: String, pub device_id: String, pub key: String }
impl Credentials { pub fn bearer(&self) -> String; }                 // "<device_id>.<key>"
pub fn load(dir: &Path) -> Option<Credentials>;                        // unreadable or invalid = None
pub fn save(dir: &Path, c: &Credentials) -> std::io::Result<()>;       // tmp file 0600 + rename
pub fn remove(dir: &Path) -> std::io::Result<()>;                      // NotFound is Ok
// state.rs
pub community: Option<Arc<crate::community::CommunityLink>>,
pub fn with_community(self, link: CommunityLink) -> Self;
// tests/common/community.rs
pub async fn with_stub() -> (Served, fixtures::community_stub::Stub);   // link → data dir s.home.path(), default origin = stub.origin, timeout 300 ms
pub async fn raw(router: &Router, req: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>);
```

- `GET /api/community-link` behaviour:
  - Off, or `st.community` is `None`: `{enabled:false, origin:null, signed_in:false, account:null}`, with no network.
  - On:
    - `signed_in` is true only when credentials load with `origin == settings origin`.
    - When signed in, `account` comes from upstream `GET {origin}/api/community/me` with the bearer, cached `ACCOUNT_TTL`.
    - If upstream answers `401 device_unknown`, the credentials are removed, the origin is kept and the view is signed out (C5). This is the only key check: there is no `/gw/api/device/check` for community keys.
    - If upstream is unreachable, `account:null` and `signed_in:true`.
- `POST /api/community-link/enable {origin?}`:
  - `valid_origin(origin.unwrap_or(default_origin))`;
  - removes credentials for any other origin;
  - saves the setting;
  - answers as GET.
  - `st.community == None` → `404 not_found`.
- `DELETE /api/community-link`:
  - cancels any pairing;
  - when signed in, sends `POST {origin}/gw/api/device/revoke` with the bearer and `Origin: {origin}`, timeout `REVOKE_TIMEOUT`, result ignored (R3);
  - removes credentials and clears `sculptured_origin`;
  - answers `{enabled:false}`.
  - The routes add no request ids, so the C5 shapes stay exact.

- [ ] **Step 1: Write the failing tests** (`tests/community_link.rs`).

```rust
#[tokio::test]
async fn community_is_off_until_turned_on_and_off_makes_no_request() {
    let (s, stub) = with_stub().await;
    let (st, v) = get(&s.router, "/api/community-link").await;
    assert_eq!((st.as_u16(), v), (200, json!({"enabled":false,"origin":null,"signed_in":false,"account":null})));
    for (m, p) in [("GET","/api/community/blocks"),("GET","/api/community/me"),("POST","/api/community/blocks")] {
        let (st, v) = call(&s.router, m, p, json!({})).await;
        assert_eq!((st.as_u16(), v["error"]["code"].as_str()), (409, Some("community_off")), "{m} {p}");
    }
    assert!(stub.requests().is_empty(), "nothing reached the upstream: {:?}", stub.requests());
}
#[tokio::test] async fn turning_on_uses_the_default_origin_or_a_given_https_one() { /* POST enable {} → origin == stub.origin; enable {"origin":"https://app.other.org"} → origin "https://app.other.org"; stub saw nothing */ }
#[tokio::test] async fn turning_on_refuses_plain_http_except_loopback() { /* {"origin":"http://example.com"} → 400 community_origin_invalid; settings unchanged */ }
#[tokio::test] async fn signed_in_reports_the_account_from_upstream_me_and_never_the_key() {
    // enable; (id,key)=stub.issue_key(); PATCH stub user handle "you-ex" via stub.state; creds::save(home,{origin:stub.origin,id,key})
    // GET → signed_in true, account == {"name":"You Example","handle":"you-ex"}; body text contains neither id nor key
    // stub saw GET /api/community/me with authorization "Bearer {id}.{key}"
}
#[cfg(unix)] #[tokio::test] async fn credentials_file_is_owner_only() { /* creds::save → mode & 0o777 == 0o600 */ }
#[tokio::test] async fn credentials_for_another_origin_count_as_signed_out() {}
#[tokio::test] async fn a_key_the_server_no_longer_accepts_signs_out_and_keeps_the_origin() {
    // save creds with an unknown key (stub answers 401 device_unknown) → GET signed_in false, enabled true, origin == stub.origin; community.json gone
    // the check was GET /api/community/me with the bearer; stub saw no /gw/api/device/check
}
#[tokio::test] async fn turning_off_forgets_origin_and_credentials_and_tries_to_revoke() {
    // DELETE → {"enabled":false}; stub saw POST /gw/api/device/revoke with the bearer and origin == stub.origin;
    // community.json gone; GET /api/settings has no sculptured_origin
}
#[tokio::test] async fn turning_off_works_when_the_server_is_down() { /* creds for origin http://127.0.0.1:<closed port> → DELETE 200 within 6 s, file gone */ }
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p controld --test community_link`. Expected: compile error (`community` module missing).
- [ ] **Step 3: Implement** `creds.rs`, `link.rs` and `mod.rs` as specified.
  - Write the 0600 file with `OpenOptions` + `OpenOptionsExt::mode(0o600)` + `set_permissions`, as `hostd::private::write_file` does. controld must not depend on hostd.
  - Handlers return `Result<crate::extract::Json<_>, ApiError>`.
- [ ] **Step 4: Run, regenerate and build.** Run `cargo test -p controld --test community_link && cargo test --workspace export_bindings && cargo build -p sculptured && cargo clippy -p controld -p sculptured --all-targets`. Expected: PASS. `web/src/generated/CommunityLinkView.ts` and `CommunityLinkAccount.ts` exist.
- [ ] **Step 5: Commit.**

```bash
git add crates/controld/src/community crates/controld/src/{lib,state,api}.rs crates/controld/Cargo.toml crates/controld/tests/common crates/controld/tests/community_link.rs crates/sculptured/src/main.rs web/src/generated/CommunityLink*.ts Cargo.lock
git commit -m "controld: turn Community on and off and report the signed-in account"
```

---

### Task 4: Device pairing with scope `community`

**Files:**
- Create: `crates/controld/src/device_pairing.rs` (`pub mod device_pairing;` in `lib.rs`)
- Create: `crates/controld/src/community/pairing.rs` (routes merged in `community::routes`)
- Modify: `crates/sculptured/src/connect.rs:42-68`. `pair` keeps its signature and calls `device_pairing::{start, poll_once}` with `scope: None`, so the request body is unchanged for computers.
- Test: `crates/controld/tests/community_pairing.rs`

**Interfaces:**
- Consumes: Task 3 `CommunityLink`, `creds::save`, `link` view.
- Produces:

```rust
// device_pairing.rs
pub struct Started { pub device_code: String, pub user_code: String, pub verify_url: String, pub interval: u64, pub expires_in: u64 }
pub enum Poll { Pending, Approved { device_id: String, key: String }, Expired, Denied }
/// POST {origin}/gw/api/device/start with Origin: {origin}; body {name, platform} plus "scope" only when Some.
pub async fn start(http: &reqwest::Client, origin: &str, name: &str, platform: &str, scope: Option<&str>) -> Result<Started, String>;
/// One POST {origin}/gw/api/device/token: 200 Approved, 428 Pending, 410 Expired, other 4xx Denied; 5xx or network Err (caller retries).
pub async fn poll_once(http: &reqwest::Client, origin: &str, device_code: &str) -> Result<Poll, String>;
// community/mod.rs (ts exported)
#[derive(Serialize, TS)] #[ts(export)] pub struct CommunityPairStarted { pub user_code: String, pub verify_url: String, #[ts(type = "number")] pub expires_in: u64 }
#[derive(Clone, Copy, Serialize, TS, PartialEq, Eq)] #[serde(rename_all = "lowercase")] #[ts(export)] pub enum CommunityPairState { Pending, Approved, Expired, Denied }
#[derive(Serialize, TS)] #[ts(export)] pub struct CommunityPairStatus { pub status: CommunityPairState }
```

- `POST /api/community-link/pair`:
  - Off → `409 community_off`.
  - Calls `start` with:
    - name `sysinfo::System::host_name()`, falling back to `"This computer"`, truncated to 80 characters;
    - platform `std::env::consts::OS`;
    - scope `Some("community")`.
  - Upstream failure:
    - unreachable → `502 community_unreachable`;
    - a coded 4xx from the gateway whose code is one of `bad_scope`, `too_many_attempts`, `community_unavailable` or `rate_limited` → the same status and code (C4 codes);
    - anything else → `502 community_bad_answer`.
    - `device_pairing::start` returns `Err(StartError::Refused { status: u16, code: String })` or `Err(StartError::Network(String))` for this. `connect.rs` turns both into its existing text.
  - Replaces any earlier pairing: a generation counter makes the old poll task exit.
  - Spawns a tokio task. Every `interval.clamp(1, 30)` s it calls `poll_once` until `Approved`, `Expired`, `Denied` or the `expires_in` deadline, which counts as `Expired`.
  - On `Approved`, it calls `creds::save` with the link's current origin and clears the account cache.
  - Answers `{user_code, verify_url, expires_in}`.
- `GET /api/community-link/pair` answers `{status}`:
  - the pairing's state if one exists;
  - otherwise `approved` when signed in, else `expired`.

- [ ] **Step 1: Write the failing tests.**

```rust
#[tokio::test] async fn pairing_starts_with_scope_community_and_shows_code_and_link() {
    let (s, stub) = with_stub().await; enable(&s).await;
    let (st, v) = call(&s.router, "POST", "/api/community-link/pair", json!({})).await;
    assert_eq!(st.as_u16(), 200);
    assert_eq!((v["user_code"].as_str(), v["expires_in"].as_u64()), (Some("ABCD-EFGH"), Some(600)));
    assert_eq!(v["verify_url"], json!(format!("{}/connect?code=ABCD-EFGH", stub.origin)));
    assert!(v.get("device_code").is_none());
    let start = stub.requests().into_iter().find(|r| r.path_and_query == "/gw/api/device/start").unwrap();
    let body: Value = serde_json::from_slice(&start.body).unwrap();
    assert_eq!(body["scope"], "community"); assert!(!body["name"].as_str().unwrap().is_empty());
    assert!(start.headers.contains(&("origin".into(), stub.origin.clone())));
}
#[tokio::test] async fn approval_stores_the_key_owner_only_and_signs_in() { /* set_pairing(Approve); poll GET pair until "approved" (≤ 10 s); creds::load(home) is Some with origin stub.origin; GET community-link signed_in true */ }
#[tokio::test] async fn a_denied_pairing_reports_denied_and_stores_nothing() { /* set_pairing(Deny) → status "denied"; community.json absent */ }
#[tokio::test] async fn an_expired_pairing_reports_expired() { /* set_pairing(Expire) → "expired"; nothing stored */ }
#[tokio::test] async fn pairing_while_off_is_community_off() { /* 409 community_off; stub saw nothing */ }
#[tokio::test] async fn a_second_pair_replaces_the_first() { /* two POSTs; set Approve; exactly one community.json; one device saved */ }
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p controld --test community_pairing`. Expected: 404 for `/api/community-link/pair`, or compile errors.
- [ ] **Step 3: Implement** `device_pairing.rs` and `community/pairing.rs`. Rewrite `connect::pair` as a loop over `poll_once`:
  - `Expired` returns the existing "The code expired…" text;
  - `Denied` returns "Pairing was not approved in time or was refused…";
  - `Err` continues until the deadline.
- [ ] **Step 4: Run, regenerate and build.** Run `cargo test -p controld --test community_pairing && cargo test -p sculptured && cargo test --workspace export_bindings && cargo clippy -p controld -p sculptured --all-targets`. Expected: PASS. Three more files appear in `web/src/generated/`.
- [ ] **Step 5: Commit.**

```bash
git add crates/controld/src/device_pairing.rs crates/controld/src/community crates/controld/src/lib.rs crates/controld/tests/community_pairing.rs crates/sculptured/src/connect.rs web/src/generated/CommunityPair*.ts
git commit -m "controld: sign in to Sculptured for Community with a community-scope device key"
```

---

### Task 5: The allow-listed proxy `/api/community/*`

**Files:**
- Create: `crates/controld/src/community/allow.rs`, `crates/controld/src/community/proxy.rs`
- Modify: `crates/controld/src/community/mod.rs` with `.route("/api/community/{*path}", any(proxy::forward))`. axum's static prefix beats `api.rs`'s `/api/{*rest}`.
- Test: `crates/controld/tests/community_proxy.rs`

**Interfaces:**
- Consumes: Task 3 `CommunityLink`, `creds`, `PROXY_TIMEOUT`.
- Produces:

```rust
pub const MAX_REQUEST_BYTES: usize = 300 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
/// `path` is what follows /api/community/ (no query). Segments must be [A-Za-z0-9_-]{1,100}.
pub fn allowed(method: &axum::http::Method, path: &str) -> bool;
pub async fn forward(State(st): State<AppState>, req: Request) -> Response;
```

- The allow-list in `allow.rs` (R5), as `(method, pattern)` with `*` matching exactly one segment:

```
GET me | PATCH me | PUT me/avatar | DELETE me/avatar | GET people | GET users/* | GET users/*/avatar
POST users/*/report | POST users/*/moderate | GET blocks | POST blocks | GET blocks/* | DELETE blocks/*
POST blocks/*/save | DELETE blocks/*/save | POST blocks/*/report | GET saved | GET mine | GET reports | POST blocks/*/moderate
```

- `forward` handles a request in this order:
  1. **Off:** `409 community_off`.
  2. **Not allowed:** `404 not_found` (it checks the raw `uri.path()`, so `%2F` or `..` fails the segment rule).
  3. **Write while signed out:** `401 signed_out`.
  4. **Body:** read with `axum::body::to_bytes(body, MAX_REQUEST_BYTES)`; over the limit → `413 community_too_large`.
  5. **Build the upstream request:**
     - URL `{origin}/api/community/{path}?{query}`;
     - copy only `content-type`, `accept` and `if-none-match`;
     - add `Authorization: Bearer …` when signed in;
     - no `Origin` (R1);
     - an upstream `401` with code `device_unknown` removes credentials before the answer is relayed (R1, C5).
  6. **Send** with `timeout`:
     - timeout → `504 community_timeout`;
     - connect error → `502 community_unreachable`.
  7. **Response:**
     - accepted content types: `application/json`, `image/png`, `image/jpeg`, `image/webp`, `image/svg+xml`; anything else → `502 community_bad_answer`;
     - read with `Response::chunk()` up to `MAX_RESPONSE_BYTES`; over → `502 community_bad_answer`;
     - copy the status;
     - copy only `content-type`, `etag`, `cache-control` and `location`, rewritten per R6;
     - add the R7 CSP and `X-Content-Type-Options: nosniff`.

- [ ] **Step 1: Write the failing tests.** Use `common::community::{with_stub, raw}` and helpers `on(&s)` (enable) and `signed_in(&s, &stub) -> (id, key)` (issue_key + `creds::save`).

```rust
#[tokio::test] async fn only_allow_listed_methods_and_paths_reach_the_server() {
    let (s, stub) = with_stub().await; on(&s).await;
    assert_eq!(get(&s.router, "/api/community/blocks?q=x&offset=0").await.0.as_u16(), 200);
    for (m, p) in [("GET","/api/community/login"),("GET","/api/community/callback"),("POST","/api/community/logout"),
                   ("POST","/api/community/sessions/revoke"),("POST","/api/community/me"),("GET","/api/community/blocks/a%2Fb"),
                   ("GET","/api/community/users/x/../../../gw/api/devices"),("GET","/api/community/"),("PUT","/api/community/blocks")] {
        let (st, v) = call(&s.router, m, p, json!({})).await;
        assert_eq!((st.as_u16(), v["error"]["code"].as_str()), (404, Some("not_found")), "{m} {p}");
    }
    assert_eq!(stub.requests().len(), 1, "only the allowed GET went upstream");
}
#[tokio::test] async fn cookies_origin_and_csrf_are_stripped_and_the_bearer_added() {
    // PATCH /api/community/me with Cookie, Origin http://test, X-CSRF-Token, X-Other → upstream saw no cookie/origin/x-csrf-token/x-other,
    // authorization == "Bearer {id}.{key}"
}
#[tokio::test] async fn a_revoked_key_on_any_request_signs_out_and_keeps_the_origin() {
    // signed in; revoke the key in the stub; GET /api/community/blocks → 401 device_unknown relayed;
    // community.json gone; GET /api/community-link → enabled true, signed_in false
}
#[tokio::test] async fn writes_signed_out_are_refused_locally_with_signed_out() { /* POST blocks/x/save → 401 signed_out; stub saw nothing */ }
#[tokio::test] async fn reads_signed_out_go_without_credentials() { /* GET people → 200; upstream saw no authorization header */ }
#[tokio::test] async fn upstream_cookies_and_unknown_headers_never_reach_the_browser() {
    // GET blocks → no set-cookie, no x-upstream-secret; GET users/ana/avatar → etag and cache-control "public, max-age=3600" kept
}
#[tokio::test] async fn every_proxied_answer_is_sandboxed_and_nosniff() {
    // GET users/ana/avatar → content-security-policy contains "sandbox" and "default-src 'none'"; x-content-type-options nosniff
}
#[tokio::test] async fn a_request_body_over_300_kb_is_refused_before_sending() {
    // PUT me/avatar with 307_201 bytes → 413 community_too_large, stub saw no PUT; 262_144 bytes → reaches stub (200)
}
#[tokio::test] async fn an_answer_over_2_mb_is_community_bad_answer() { /* GET blocks?q=huge → 502 community_bad_answer */ }
#[tokio::test] async fn a_non_json_non_image_answer_is_community_bad_answer() { /* GET blocks?q=html → 502 community_bad_answer */ }
#[tokio::test] async fn a_slow_server_times_out_as_community_timeout() {
    assert_eq!(controld::community::PROXY_TIMEOUT, std::time::Duration::from_secs(15));
    // link timeout 300 ms (with_stub); stub.set_delay(1 s); GET blocks → 504 community_timeout in < 1 s
}
#[tokio::test] async fn a_server_that_is_down_is_community_unreachable() { /* enable origin http://127.0.0.1:<closed> → GET blocks 502 community_unreachable */ }
#[tokio::test] async fn an_old_handle_redirect_stays_on_this_origin() { /* GET users/old-ana → 301, location == "/api/community/users/ana" */ }
#[tokio::test] async fn credentials_never_appear_in_any_browser_response() {
    // signed in; for every GET/POST/PATCH/DELETE on /api/community-link* and a sample of /api/community/* (me, blocks, people, users/ana):
    // neither id nor key nor "Bearer" appears in the body bytes or any header value
}
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p controld --test community_proxy`. Expected: FAIL (`409 community_off` everywhere, or 404 from `api_not_found`).
- [ ] **Step 3: Implement `allow.rs` and `proxy.rs`.** Use the same segment matching as `hosted::local_only`. Split `forward` into `check`, `build_upstream`, `send` and `relay` so each is 30 lines or fewer.
- [ ] **Step 4: Run all controld Community tests.** Run `cargo test -p controld --test community_proxy --test community_link --test community_pairing && cargo clippy -p controld --all-targets`. Expected: PASS.
- [ ] **Step 5: Commit.**

```bash
git add crates/controld/src/community crates/controld/tests/community_proxy.rs
git commit -m "controld: allow-listed Community proxy with size, time and header limits"
```

---

### Task 6: Web data layer: generated types, `api.ts`, `link.ts`, words, test helpers

**Files:**
- Generated (not written): `web/src/generated/community/*.ts`, from `../sculptured-community`
- Modify: `web/src/api.ts`
  - Export `request` as `apiRequest`.
  - Methods add `'PATCH'`.
  - `RequestOptions` gains `bytes?: { body: Blob; type: string }`, sent as-is with that content type.
  - When `json.error` is a string (the legacy Community shape), it throws `ApiError(status, 'community_bad_answer', '')` (R11).
- `community/api.ts`: every call goes through one wrapper. On `ApiError` code `device_unknown`, it calls `linkStore.refresh()` before rethrowing, so the top bar and pages show signed out.
- Create:
  - `web/src/community/api.ts`
  - `web/src/community/link.ts`
  - `web/src/community/context.ts`
  - `web/src/community/kinds.ts`
  - `web/src/community/testing.tsx`
- Modify: `web/src/errors.ts`: add `COMMUNITY_WORDS` codes and spread them into `ERROR_WORDS`. Add `community_only` to `VERBATIM`; its C4 message is already plain.
- Modify: `web/src/vocab.ts`: add the `COMMUNITY_WORDS` copy object.
- Modify: `web/src/test-vocab.ts`: `openRigWords(root: HTMLElement, allow: readonly string[] = [])`.
- Test:
  - `web/src/community/api.test.ts`
  - `web/src/community/link.test.ts`
  - `web/src/community/kinds.test.ts`
  - `web/src/errors.test.ts` (added cases)

**Interfaces:**
- Consumes:
  - Tasks 3 and 4 generated `CommunityLinkView`, `CommunityPairStarted`, `CommunityPairStatus`, `CommunityPairState`;
  - from the community repo: `generated/community/{Publication,Profile,Me,Author}.ts`;
  - `useHosted()` and `HostedSession`.
- Produces:

```ts
// community/api.ts — every URL relative, identical in every build
export type MeUser = NonNullable<Me['user']>;
export type PeoplePage = { people: Profile[]; next: number | null };
export type ProfilePage = { profile: Profile; blocks: Publication[] };
export type BlockList = { blocks: Publication[] };
export type PublishRequest = { title: string; description: string; license: string; tags: string[]; content: GroupBlock; reviewed: true };
export type Report = { id: string; reason: string; kind: 'block' | 'profile'; block?: string | null; handle?: string | null };  // R10
export type ProfileModeration = { reset: ('bio' | 'avatar' | 'handle')[]; disable?: boolean };
export const communityApi: {
  me(): Promise<Me>; updateMe(p: { name?: string; handle?: string; bio?: string; links?: string[] }): Promise<Me>;
  putAvatar(image: Blob): Promise<{ avatar_url: string }>; deleteAvatar(): Promise<{ avatar_url: string }>;
  people(q: string, offset: number): Promise<PeoplePage>; profile(handle: string): Promise<ProfilePage>;
  blocks(q: string, offset: number): Promise<BlockList>; block(id: string): Promise<Publication>;
  publish(p: PublishRequest): Promise<{ id: string; version: number }>; unpublish(id: string): Promise<unknown>;
  save(id: string): Promise<unknown>; unsave(id: string): Promise<unknown>; saved(): Promise<BlockList>; mine(): Promise<BlockList>;
  reportBlock(id: string, reason: string): Promise<unknown>; reportProfile(handle: string, reason: string): Promise<unknown>;
  reports(): Promise<{ reports: Report[] }>; moderateBlock(id: string, hidden: boolean): Promise<unknown>;
  moderateProfile(handle: string, m: ProfileModeration): Promise<unknown>;
};
export function safeAvatarUrl(url: string | null | undefined): string | null; // R8
export function avatarSrc(handle: string): string;  // `/api/community/users/${encodeURIComponent(handle)}/avatar`
// community/link.ts
export const linkApi: { get(): Promise<CommunityLinkView>; enable(origin?: string): Promise<CommunityLinkView>;
  pair(): Promise<CommunityPairStarted>; pairStatus(): Promise<CommunityPairStatus>; turnOff(): Promise<{ enabled: false }> };
export function hostedLink(s: HostedSession): CommunityLinkView | null;   // null for kind 'local'; else {enabled:true, origin:null, signed_in: s.kind==='signed-in', account: s.kind==='signed-in' ? {name: s.user.name, handle: null} : null}
export function useCommunityLink(): { link: CommunityLinkView | null; error: string; refresh: () => Promise<void> }; // one shared store (useSyncExternalStore); hosted: never fetches
// community/context.ts
export type CommunityState = { link: CommunityLinkView; me: MeUser | null; hosted: boolean; reloadMe: () => Promise<void> };
export const CommunityContext: React.Context<CommunityState | null>;
export function useCommunity(): CommunityState;  // throws outside a provider
// community/kinds.ts
export function blockKind(p: { id: string; kind?: string | null }): string;  // kind ?? 'group'
export function opensHere(p: { id: string; kind?: string | null }): boolean; // blockKind(p) === 'group'
// community/testing.tsx
export function renderCommunity(ui: ReactElement, o: { path: string; route?: string; state?: Partial<CommunityState> }): RenderResult;
export function fakeFetch(routes: Record<string, (init?: RequestInit) => [number, unknown] | Response>): { calls: string[] }; // key "METHOD /url"; unmatched → 404 not_found
export function publication(o?: Partial<Publication>): Publication; export function profile(o?: Partial<Profile>): Profile; export function meUser(o?: Partial<MeUser>): MeUser;
```

- Error words, added to `errors.ts`. The text is exact.

```ts
export const COMMUNITY_ERROR_WORDS: Record<string, string> = {
  handle_taken: 'Someone already has that handle. Choose another.',
  handle_invalid: 'A handle is 3 to 30 lowercase letters, digits or dashes, and starts and ends with a letter or digit.',
  handle_reserved: 'That handle is reserved. Choose another.',
  handle_too_soon: 'You can change your handle once every 30 days.',
  bio_too_long: 'Keep your bio to 280 characters.',
  link_invalid: 'Each link must start with https:// and be at most 200 characters. You can add up to 3.',
  avatar_too_large: 'That picture is too large. Use one under 256 KB.',
  avatar_type: 'Use a PNG, JPEG or WebP picture.',
  handle_required: 'Choose a handle before you publish.',
  publish_limit: 'A new account can publish 5 blocks in its first day. Try again tomorrow.',
  signed_out: 'Sign in to Sculptured to do this.',
  csrf: `Your sign-in could not be checked. ${RELOAD}`,
  community_off: 'Community is off on this computer. Turn it on to use it.',
  community_unreachable: 'Could not reach Community. Check your internet connection, then try again.',
  community_timeout: 'Community did not answer within 15 seconds. Try again.',
  community_too_large: 'That is too large to send to Community (the limit is 300 KB).',
  community_bad_answer: 'Community answered in a way Sculptured does not accept. Try again later.',
  community_origin_invalid: 'Use an https:// address, such as https://app.usesculptured.com.',
  // C3 generic and new codes
  account_disabled: 'This Community account is disabled. Contact Sculptured support if you think this is a mistake.',
  rate_limited: 'Too many requests to Community just now. Wait a minute, then try again.',
  invalid: 'Community did not accept that. Check what you entered, then try again.',
  // C4/C5: the sign-in key was refused
  device_unknown: 'Your Community sign-in was removed. Sign in again.',
  // C4 gateway codes seen while signing in or joining
  bad_scope: `Sculptured refused this kind of sign-in. ${RELOAD}`,
  bad_purpose: `That sign-in link is not valid. ${RELOAD}`,
  already_member: 'You already have a Sculptured account. Sign in instead.',
  community_unavailable: 'Community is not available on this Sculptured server right now.',
  too_many_attempts: 'Too many sign-in attempts. Wait a few minutes, then try again.',
};
// `not_found` already has words in ERROR_WORDS. In `apiRequest`, an `error` that is a string (the legacy Community shape) becomes
// code `community_bad_answer` (R11).
```

- Copy, added to `vocab.ts` as `COMMUNITY_WORDS`. The text is exact.
  - **Tabs:** `Explore`, `People`, `Saved`, `Yours`, `Moderation`.
  - **`optIn`:**
    - `title`: 'Turn on Community'
    - `intro`: 'Community is where people share blocks of agents and their profiles. It runs on Sculptured’s servers, so this computer sends nothing there until you turn it on.'
    - `sendsTitle`: 'What turning it on sends'
    - `sends`: ['The Community pages and searches you open.', 'If you sign in: what you publish, save and report, and what you put on your profile.']
    - `neverTitle`: 'What never leaves this computer'
    - `never`: 'Your projects, files, transcripts and runs.'
    - `browse`: 'Turn on and browse'
    - `signIn`: 'Turn on and sign in'
    - `otherServer`: 'Use another Sculptured server'
  - **`signIn`:**
    - `title`: 'Sign in to Sculptured'
    - `enter`: 'Enter this code on the sign-in page:'
    - `open`: 'Open the sign-in page'
    - `waiting`: 'Waiting for you to approve…'
    - `expired`: 'The code expired before it was approved.'
    - `denied`: 'The sign-in was turned down.'
    - `again`: 'Get a new code'
    - `done`: `(name: string) => \`Signed in as ${name}\``
  - **Other:**
    - `laterVersion`: 'Opens in a later version.'
    - `addToProject`: 'Add to project'
    - `saveToLibrary`: 'Save to Library'
    - `inLibrary`: 'In your Library'
    - `updateTo`: `(n: number) => \`Update to v${n}\``
    - `save`: 'Save'
    - `unsave`: 'Remove save'
    - `turnOff`: 'Turn off Community'

- [ ] **Step 1: Make the generated types available.** Check that they exist: run `ls web/src/generated/community/{Publication,Profile,Me,Author}.ts`.
  - **If they are missing**, check whether the community repo has them: run `grep -l "pub struct Profile" ../sculptured-community/src/*.rs`.
    - **If it does**, run `cargo test --manifest-path ../sculptured-community/Cargo.toml export_bindings`, then repeat the `ls`. Expected: the four files exist, and `Publication.ts` has `author: Author`, `kind` and no `owner`. `Me.ts`'s user has `settings: { compact: boolean }`.
      - That repo's `.cargo/config.toml` sets `export_to = "community/"`.
      - The same run also rewrites the model types in `web/src/generated/` root.
      - Run `git diff --stat web/src/generated/*.ts`. Expected: no changes, because the files come out identical.
      - If they differ, the two repos are on different model revisions. Re-run `cargo test --workspace export_bindings` here, keep this repo's versions, and report the drift.
    - **If it does not**, stop. Report "BLOCKED: waiting for the sculptured-community C3 export". Do not hand-write the types. Tasks 1–5 do not depend on this.
- [ ] **Step 2: Write the failing tests.**

```ts
// community/api.test.ts
it('calls only relative /api/community URLs and sends PATCH, Blob and the hosted CSRF', async () => {
  const { calls } = fakeFetch({ 'PATCH /api/community/me': () => [200, { user: null }], 'PUT /api/community/me/avatar': () => [200, { avatar_url: '/api/community/users/a/avatar' }] });
  setCsrf('t'); await communityApi.updateMe({ bio: 'hi' }); await communityApi.putAvatar(new Blob([new Uint8Array(3)], { type: 'image/png' }));
  expect(calls).toEqual(['PATCH /api/community/me', 'PUT /api/community/me/avatar']);
  // the PUT carried content-type image/png and x-csrf-token t; the body was the Blob
});
it('encodes handles and ids in paths', async () => { /* profile('a b') → GET /api/community/users/a%20b */ });
it('keeps only same-origin avatar URLs', () => {
  expect(safeAvatarUrl('/api/community/users/ana/avatar?v=1')).toBe('/api/community/users/ana/avatar?v=1');
  expect(safeAvatarUrl('data:image/svg+xml,%3Csvg%3E%3C/svg%3E')).toBe('data:image/svg+xml,%3Csvg%3E%3C/svg%3E');
  for (const u of ['https://app.x/api/community/users/a/avatar', '//evil/x', 'javascript:alert(1)', 'data:text/html,x', '/api/teams', null]) expect(safeAvatarUrl(u)).toBeNull();
});
// community/link.test.ts
it('hosted mode derives the link from the session and never fetches /api/community-link', () => { /* hostedLink({kind:'signed-in',…}) → enabled true, signed_in true; fetch not called */ });
it('local mode fetches the link once for every subscriber and refresh updates them', async () => {});
// community/kinds.test.ts
it('treats a missing kind as a group and anything else as not openable here', () => {
  expect(opensHere({ id: 'x' })).toBe(true); expect(opensHere({ id: 'x', kind: 'group' })).toBe(true);
  expect(opensHere({ id: 'x', kind: 'component' })).toBe(false);
});
// errors.test.ts
it('says every Community code in plain words and never the server text', () => {
  for (const code of ['handle_taken','handle_invalid','handle_reserved','handle_too_soon','bio_too_long','link_invalid','avatar_too_large','avatar_type','handle_required','publish_limit','signed_out','csrf','community_off','community_unreachable','community_timeout','community_too_large','community_bad_answer','community_origin_invalid',
                    'account_disabled','rate_limited','invalid','device_unknown','bad_scope','bad_purpose','already_member','community_unavailable','too_many_attempts'])
    expect(errorText(new ApiError(400, code, 'SERVER TEXT'))).not.toMatch(/SERVER TEXT|could not do that/);
  expect(errorText(new ApiError(401, 'device_unknown', 'x'))).toBe('Your Community sign-in was removed. Sign in again.');
});
// api.test.ts
it('a legacy {"error":"text"} answer is community_bad_answer and the text is never shown', async () => {
  fakeFetch({ 'GET /api/community/blocks?q=&offset=0': () => [400, { error: 'Invalid search' }] });
  await expect(communityApi.blocks('', 0)).rejects.toMatchObject({ code: 'community_bad_answer' });
});
it('a device_unknown answer refreshes the link so the page shows signed out', async () => { /* 401 device_unknown → linkStore refresh called (GET /api/community-link) */ });
```

- [ ] **Step 3: Run them to see them fail.** Run `npm --prefix web test -- src/community src/errors.test.ts`. Expected: FAIL (the modules are missing).
- [ ] **Step 4: Implement** the files and signatures above. `testing.tsx` wraps `ui` in `MemoryRouter initialEntries={[path]}` + `Routes/Route path={route ?? path}` + `CommunityContext.Provider`. The default state is `{link: {enabled:true, origin:'http://stub', signed_in:false, account:null}, me:null, hosted:false, reloadMe: async () => {}}`.
- [ ] **Step 5: Run tests, build and lint.** Run `npm --prefix web test && npm --prefix web run build && npm --prefix web run lint 2>&1 | tail -1`. Expected: PASS, and the count is at or below baseline.
- [ ] **Step 6: Commit.**

```bash
git add web/src/generated/community web/src/api.ts web/src/community web/src/errors.ts web/src/errors.test.ts web/src/vocab.ts web/src/test-vocab.ts
git commit -m "web: Community data layer, link state, words and test helpers"
```

---

### Task 7: Turn on Community, Sign in to Sculptured, and the Account page card

**Files:**
- Create:
  - `web/src/community/TurnOnCommunity.tsx`
  - `web/src/community/SignInToSculptured.tsx`
  - `web/src/community/SignInPrompt.tsx`
  - `web/src/community/CommunityCard.tsx`
  - `web/src/community/community.css`
- Modify: `web/src/account/AccountPage.tsx`. Add `<CommunityCard />` as a `<section aria-label="Community">` after the sign-in section, local mode only.
- Test:
  - `web/src/community/TurnOnCommunity.test.tsx`
  - `web/src/community/SignInToSculptured.test.tsx`
  - `web/src/account/AccountPage.test.tsx` (added case)

**Interfaces:**
- Consumes: Task 6 `linkApi`, `useCommunityLink`, `COMMUNITY_WORDS`, `errorText`.
- Produces:
  - `TurnOnCommunity({ onDone }: { onDone: (signIn: boolean) => void })`
  - `SignInToSculptured({ onSignedIn }: { onSignedIn?: () => void })`
  - `SignInPrompt({ why }: { why: string })`. Locally it is a button that reveals `SignInToSculptured`. When hosted, it is `<Link to="/login">` after `remember(path)`.
  - `CommunityCard()`

- **`TurnOnCommunity`:**
  - Renders the exact `optIn` copy as a heading, two lists and two buttons.
  - Also has `<details>` "Use another Sculptured server" with an origin input.
  - "Turn on and browse" → `linkApi.enable(origin || undefined)` → `onDone(false)`. "Turn on and sign in" does the same → `onDone(true)`.
  - `community_origin_invalid` shows under the input.
- **`SignInToSculptured`:**
  - Mounting calls `linkApi.pair()` and shows `user_code` large.
  - It shows the link "Open the sign-in page" (`target="_blank" rel="noopener noreferrer"`). The link appears only when `verify_url` starts with `link.origin + '/'`.
  - It polls `pairStatus()` every 2000 ms while `pending`:
    - `approved` → `refresh()`, `onSignedIn?.()`, then "Signed in as {account.name}";
    - `expired` or `denied` → the matching words, plus a "Get a new code" button that pairs again.
  - The poll stops on unmount.
- **`CommunityCard`:**
  - Off: one line and a link "Open Community" to `/community`.
  - On: the origin's host, then "Signed in as {name} (@handle)" or a "Sign in to Sculptured" button, then "Turn off Community" (→ `linkApi.turnOff()` + `refresh()`).

- [ ] **Step 1: Write the failing tests.**

```tsx
it('says what is sent and what never leaves, and turns on only when asked', async () => {
  const { calls } = fakeFetch({ 'POST /api/community-link/enable': () => [200, { enabled: true, origin: 'https://app.usesculptured.com', signed_in: false, account: null }] });
  const onDone = vi.fn(); render(<TurnOnCommunity onDone={onDone} />);
  for (const t of [COMMUNITY_WORDS.optIn.title, ...COMMUNITY_WORDS.optIn.sends, COMMUNITY_WORDS.optIn.never]) expect(screen.getByText(t)).toBeInTheDocument();
  expect(calls).toEqual([]);
  await userEvent.click(screen.getByRole('button', { name: 'Turn on and browse' }));
  expect(calls).toEqual(['POST /api/community-link/enable']); expect(onDone).toHaveBeenCalledWith(false);
  expect(openRigWords(document.body, ['profile', 'profiles'])).toEqual([]);
});
it('refuses a plain http server address in words', async () => { /* enable → 400 community_origin_invalid; text COMMUNITY_ERROR_WORDS.community_origin_invalid */ });
it('shows the code and the sign-in link, then the signed-in name once approved', async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  // pair → {user_code:'ABCD-EFGH', verify_url:'http://stub/connect?code=ABCD-EFGH', expires_in:600}; status pending then approved; link refresh → account {name:'You Example'}
  expect(await screen.findByText('ABCD-EFGH')).toBeInTheDocument();
  expect(screen.getByRole('link', { name: 'Open the sign-in page' })).toHaveAttribute('href', 'http://stub/connect?code=ABCD-EFGH');
  await vi.advanceTimersByTimeAsync(4000);
  expect(await screen.findByText('Signed in as You Example')).toBeInTheDocument();
});
it('an expired or turned-down code says so and offers a new one', async () => {
  // status expired → 'The code expired before it was approved.' + button 'Get a new code' → second POST pair
  // status denied → 'The sign-in was turned down.'
});
it('hides a sign-in link that points away from the Sculptured server', async () => { /* verify_url 'https://evil/x' → no link; code still shown */ });
// AccountPage.test.tsx
it('local: shows the Community card and turns Community off', async () => { /* link on+signed in → 'Signed in as You Example'; click Turn off Community → DELETE /api/community-link */ });
```

- [ ] **Step 2: Run them to see them fail.** Run `npm --prefix web test -- src/community src/account`. Expected: FAIL (the components are missing).
- [ ] **Step 3: Implement** the components. Keep each under 30 lines by splitting `CodePanel`, `PairOutcome` and `ServerField` into small functions in the same file.
- [ ] **Step 4: Run tests, build and lint.** Run `npm --prefix web test && npm --prefix web run build && npm --prefix web run lint 2>&1 | tail -1`. Expected: PASS, at or below baseline.
- [ ] **Step 5: Commit.**

```bash
git add web/src/community web/src/account/AccountPage.tsx web/src/account/AccountPage.test.tsx
git commit -m "web: Turn on Community and Sign in to Sculptured with a code"
```

---

### Task 8: Explore, block page and "Add to project"

**Files:**
- Create:
  - `web/src/community/Avatar.tsx`
  - `web/src/community/AuthorChip.tsx`
  - `web/src/community/BlockCard.tsx`
  - `web/src/community/ExplorePage.tsx`
  - `web/src/community/BlockPage.tsx`
  - `web/src/community/BlockActions.tsx`
  - `web/src/community/LoadProblem.tsx`
- Test: `web/src/community/ExplorePage.test.tsx`, `web/src/community/BlockPage.test.tsx`

**Interfaces:**
- Consumes:
  - Task 6 `communityApi`, `useCommunity`, `opensHere`, `safeAvatarUrl`;
  - existing `TemplatePreview`, `BlockDescription`, `BlockFeatures`, `BlockDetails`, `AddToProject({block, onClose})`, `sanitizeExport`, `downloadBlock`.
- Produces:

```ts
Avatar({ name, src, size = 32 }: { name: string; src?: string | null; size?: number })  // <img alt=""> when safeAvatarUrl(src) else initials(name) on a colour from the name
AuthorChip({ author }: { author: Author })        // Avatar + name, <Link to={`/community/u/${author.handle}`}>
BlockCard({ pub, actions }: { pub: Publication; actions?: ReactNode })  // preview, title <Link to=/community/blocks/:id>, tags, AuthorChip, "v{n} · {license}"
BlockActions({ pub }: { pub: Publication })       // Add to project (opens AddToProject) | Save/Remove save; non-group kind → laterVersion note
LoadProblem({ error, onRetry }: { error: unknown; onRetry: () => void })  // role=alert, errorText(error), Retry button
ExplorePage()   // /community : search (q) + grid + Previous/Next (24 per page)
BlockPage()     // /community/blocks/:id : large TemplatePreview, title, AuthorChip card, description, BlockFeatures, BlockActions, Download group, Inspect details, report, Unpublish when me.handle === pub.author.handle
```

- **Compact cards:** the Explore, Saved and Yours grids get class `community-compact` unless `me?.settings.compact === false` (C3 `Me.user.settings`; the default is compact).
- **Explore:**
  - Search matches title, tag and author handle and name; the server does the matching.
  - Signed out, "Save" is disabled with visible text "Sign in to save" and a `SignInPrompt`.
- **"Add to project":**
  - It is two clicks: "Add to project" opens the existing `AddToProject` review dialog with `sanitizeExport(pub.content)`, then "Add & open project".
  - The dialog keeps its review details and boundary notes.
  - There is no Library copy.
- **Footer note on the block page:** "Community supplied instructions. Publishing is not verification. Review them before starting agents."

- [ ] **Step 1: Write the failing tests.**

```tsx
it('searches blocks and shows each author with a link to their profile', async () => {
  const { calls } = fakeFetch({ 'GET /api/community/blocks?q=&offset=0': () => [200, { blocks: [publication()] }], 'GET /api/community/blocks?q=review&offset=0': () => [200, { blocks: [] }] });
  renderCommunity(<ExplorePage />, { path: '/community' });
  expect(await screen.findByRole('link', { name: /Ana Lee/ })).toHaveAttribute('href', '/community/u/ana');
  await userEvent.type(screen.getByRole('searchbox', { name: 'Search blocks' }), 'review{Enter}');
  expect(calls.at(-1)).toBe('GET /api/community/blocks?q=review&offset=0');
  expect(openRigWords(document.body, ['profile', 'profiles'])).toEqual([]);
});
it('signed out: Save says why it is disabled', async () => { /* button Save disabled; text 'Sign in to save' visible */ });
it('a slow or unreachable Community shows plain words and Retry', async () => {
  // blocks → 504 community_timeout → alert has COMMUNITY_ERROR_WORDS.community_timeout; Retry refetches; then 502 community_unreachable words
});
it('Add to project opens the review step and adds a sanitized copy to a new project', async () => {
  // GET blocks/b1 → publication with content folder '/secret'; click Add to project → dialog 'Add Review pair to project'
  // POST /api/teams (create) then PUT draft: saved draft agents have folder null and permission 'restricted'; no PUT /api/group-blocks
});
it('a block of another kind shows "Opens in a later version." instead of Add to project and Save to Library', async () => {
  // publication({ ...kind: 'component' } as Publication) → no 'Add to project' button; text COMMUNITY_WORDS.laterVersion
});
it('an avatar URL pointing elsewhere falls back to initials', () => { /* Avatar src 'https://evil/a.png' → no <img>, text 'AL' */ });
```

- [ ] **Step 2: Run them to see them fail.** Run `npm --prefix web test -- src/community/ExplorePage.test.tsx src/community/BlockPage.test.tsx`. Expected: FAIL.
- [ ] **Step 3: Implement** the components. `BlockPage` uses `useParams().id`. Loading shows "Loading block…" as `role="status"`.
- [ ] **Step 4: Run tests, build and lint.** Run `npm --prefix web test && npm --prefix web run build && npm --prefix web run lint 2>&1 | tail -1`. Expected: PASS, at or below baseline.
- [ ] **Step 5: Commit.**

```bash
git add web/src/community && git commit -m "web: Community Explore and block pages with Add to project"
```

---

### Task 9: "Save to Library" with source and "Update to v{n}"

**Files:**
- Modify: `crates/model/src/group_block.rs`. Add:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
#[ts(optional)]
pub source: Option<BlockSource>
```

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct BlockSource {
    pub community_block: String,
    #[ts(type = "number")]
    pub version: u32,
}
```

  Re-export it from `model/src/lib.rs`. Fix every struct literal of `GroupBlock` that the compiler flags by adding `source: None`.
- Modify: `crates/controld/src/group_blocks.rs` `validate`. When `source` is set, `community_block` must be `valid_id` and `version >= 1`.
- Modify: `web/src/release/sharing.ts` `sanitizeExport`: `delete b.source`. Publishing and exporting never carry it.
- Modify: `web/src/groups/GroupLibrary.tsx`. A card with `source` shows "From Community · v{n}" as a link to `/community/blocks/{id}`.
- Create: `web/src/community/saveToLibrary.ts`
- Modify: `web/src/community/BlockActions.tsx`, to add the Save to Library button.
- Test:
  - `crates/controld/tests/group_block_source.rs`
  - `web/src/community/saveToLibrary.test.ts`
  - `web/src/release/sharing.test.ts` (added case)
  - `web/src/groups/GroupLibrary.test.tsx` (added case)

**Interfaces:**
- Consumes: Task 8 `BlockActions`; `api.groupBlocks()`, `api.saveGroupBlock(block)`.
- Produces:

```ts
export type LibraryState = { kind: 'none' } | { kind: 'same'; copy: GroupBlock } | { kind: 'older'; copy: GroupBlock; has: number };
export function libraryState(pub: Publication, library: GroupBlock[]): LibraryState;    // match by source.community_block === pub.id
export function libraryCopy(pub: Publication, existing?: GroupBlock): GroupBlock;      // sanitizeExport(pub.content) with id existing?.id ?? `community-${pub.id}`, revision existing?.revision ?? 0, name pub.title, source {community_block: pub.id, version: pub.version}
export async function saveToLibrary(pub: Publication): Promise<GroupBlock>;            // loads the library, PUTs libraryCopy
```

- Button labels:

  | Library state | Button |
  |---|---|
  | `none` | "Save to Library" |
  | `same` | disabled "In your Library", with visible text "This version is already saved." |
  | `older` | "Update to v{pub.version}"; the PUT carries the same local id, so there is never a duplicate |

  A non-group kind shows the later-version note instead (Task 8).

- [ ] **Step 1: Write the failing tests.**

```rust
// crates/controld/tests/group_block_source.rs
#[tokio::test] async fn a_library_copy_remembers_its_community_block_and_version() {
    // PUT /api/group-blocks/community-b1 with the fixtures seed block + id "community-b1" + source {community_block:"b1",version:2} → 200;
    // GET returns source unchanged
}
#[tokio::test] async fn a_bad_source_is_group_block_invalid() { /* community_block "../x" or version 0 → 400 group_block_invalid */ }
```

```ts
it('saves once, says it is in the Library, and offers Update to v2 when Community has a newer version', async () => {
  // library [] → PUT /api/group-blocks/community-b1 body.block.source {community_block:'b1',version:1}, revision 0
  expect(libraryState(publication({ id: 'b1', version: 1 }), [copyV1]).kind).toBe('same');
  const s = libraryState(publication({ id: 'b1', version: 2 }), [copyV1]); expect(s).toMatchObject({ kind: 'older', has: 1 });
  expect(libraryCopy(publication({ id: 'b1', version: 2 }), copyV1)).toMatchObject({ id: copyV1.id, revision: copyV1.revision, source: { version: 2 } });
});
it('export and publish drop the source', () => { expect(sanitizeExport({ ...block(), source: { community_block: 'b1', version: 1 } }).source).toBeUndefined(); });
it('a Library card from Community links back to its block', async () => { /* 'From Community · v1' link href '/community/blocks/b1' */ });
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p controld --test group_block_source` and `npm --prefix web test -- src/community/saveToLibrary.test.ts src/release/sharing.test.ts src/groups`. Expected: FAIL.
- [ ] **Step 3: Implement** the model field, validation, `saveToLibrary.ts`, the button and the Library card line.
- [ ] **Step 4: Regenerate and run.** Run `cargo test --workspace export_bindings && cargo test -p model -p controld && npm --prefix web test && npm --prefix web run build && npm --prefix web run lint 2>&1 | tail -1`. Expected: PASS. `generated/GroupBlock.ts` has `source?: BlockSource`.
  - The model crate is also built by `../sculptured-community`. Run `cargo check --manifest-path ../sculptured-community/Cargo.toml` and note the result. `source` is optional and serde-defaulted, so it should compile unchanged.
- [ ] **Step 5: Commit.**

```bash
git add crates/model/src crates/controld/src/group_blocks.rs crates/controld/tests/group_block_source.rs web/src/generated/GroupBlock.ts web/src/generated/BlockSource.ts web/src/community web/src/release/sharing.ts web/src/release/sharing.test.ts web/src/groups/GroupLibrary.tsx web/src/groups/GroupLibrary.test.tsx
git commit -m "Save Community blocks to the Library with their source and offer updates"
```

---

### Task 10: People and profile pages

**Files:**
- Create: `web/src/community/PeoplePage.tsx`, `web/src/community/ProfilePage.tsx`, `web/src/community/ProfileView.tsx`
- Test: `web/src/community/PeoplePage.test.tsx`, `web/src/community/ProfilePage.test.tsx`

**Interfaces:**
- Consumes: Tasks 6 and 8 `communityApi.people/profile/reportProfile`, `Avatar`, `BlockCard`, `LoadProblem`, `SignInPrompt`.
- Produces:
  - `PeoplePage()` at `/community/people`: a search box "Search people", cards (Avatar, name, @handle, bio's first line), and "More" while `next !== null`.
  - `ProfilePage()` at `/community/u/:handle`.
  - `ProfileView({ profile, blocks }: ProfilePage)`, also used by Task 11's preview.
- **`ProfileView`** shows:
  - the avatar (96 px), name, `@handle` and "Joined {Month YYYY}";
  - the bio as plain text with `white-space: pre-line`;
  - links as `<a href rel="nofollow noopener noreferrer ugc" target="_blank">{hostname}</a>`, only for `https:` URLs;
  - counts: "{n} blocks published · saved {m} times";
  - the blocks grid.
  - Empty fields render nothing.
- **`ProfilePage`** extras:
  - When the answer's `profile.handle` differs from the URL's handle (an old handle), it calls `navigate('/community/u/' + handle, { replace: true })`.
  - When signed in, "Report this profile" shows a `<details>` with a reason (5–1000 characters).

- [ ] **Step 1: Write the failing tests.**

```tsx
it('lists people with their handle and pages with More', async () => { /* people?q=&offset=0 → next 24; click More → offset=24 */ });
it('shows a profile with only the fields it has, links as host names', async () => {
  // profile({bio:'', links:['https://ana.dev/x']}) → link text 'ana.dev', rel contains 'nofollow' and 'noopener'; no bio paragraph
  expect(openRigWords(document.body, ['profile', 'profiles'])).toEqual([]);
});
it('an old handle lands on the new address', async () => { /* route /community/u/old-ana answered with handle 'ana' → location '/community/u/ana' */ });
it('never links a non-https link', async () => { /* links ['javascript:alert(1)'] → no anchor */ });
it('an unknown handle says so in words', async () => { /* 404 not_found → ERROR_WORDS.not_found text */ });
```

- [ ] **Step 2: Run them to see them fail.** Run `npm --prefix web test -- src/community/PeoplePage.test.tsx src/community/ProfilePage.test.tsx`. Expected: FAIL.
- [ ] **Step 3: Implement** the pages. Format the date with `Intl.DateTimeFormat(undefined, { month: 'long', year: 'numeric' })`.
- [ ] **Step 4: Run tests, build and lint.** Run `npm --prefix web test && npm --prefix web run build && npm --prefix web run lint 2>&1 | tail -1`. Expected: PASS, at or below baseline.
- [ ] **Step 5: Commit.**

```bash
git add web/src/community && git commit -m "web: People directory and profile pages"
```

---

### Task 11: Your profile (handle, name, bio, links, avatar)

**Files:**
- Create:
  - `web/src/community/YourProfilePage.tsx`
  - `web/src/community/ProfileForm.tsx`
  - `web/src/community/AvatarPicker.tsx`
  - `web/src/community/avatarCrop.ts`
  - `web/src/community/handles.ts`
- Test:
  - `web/src/community/YourProfilePage.test.tsx`
  - `web/src/community/AvatarPicker.test.tsx`
  - `web/src/community/handles.test.ts`

**Interfaces:**
- Consumes: Task 6 `communityApi.updateMe/putAvatar/deleteAvatar`, `useCommunity().me/reloadMe`; Task 10 `ProfileView`.
- Produces:

```ts
// handles.ts
export const HANDLE_RE = /^[a-z0-9](?:[a-z0-9-]{1,28}[a-z0-9])$/;
export const RESERVED_HANDLES: readonly string[];   // the 16 C3 names
export function handleProblem(h: string): string | null;   // COMMUNITY_ERROR_WORDS.handle_invalid | handle_reserved | null
export function suggestHandle(name: string): string;        // lowercase, non [a-z0-9] → '-', trimmed to 30, padded to 3 with '-1'? → must satisfy HANDLE_RE
// avatarCrop.ts
export const AVATAR_MAX_BYTES = 262144;
export const AVATAR_TYPES = ['image/png', 'image/jpeg', 'image/webp'] as const;
export async function cropToSquare(file: File, size = 256): Promise<Blob>;  // centre square, canvas.toBlob('image/webp', 0.9)
// components
YourProfilePage()                                   // /community/me ; signed out → SignInPrompt why 'Sign in to edit your profile.'
ProfileForm({ me, onSaved }: { me: MeUser; onSaved: () => void })
AvatarPicker({ current, name, onChanged }: { current: string | null; name: string; onChanged: () => void })
```

- **`ProfileForm`:**
  - Fields: Display name (required, 1–80), Handle (hint "3–30 lowercase letters, digits or dashes. You can change it once every 30 days."), Bio (`maxLength` 280 with a "{n}/280" counter), and Links (3 inputs, `type="url"`, placeholder `https://`).
  - Submit sends `updateMe({ name, bio, links: nonEmpty, ...(handle !== me.handle ? { handle } : {}) })`.
  - Errors with codes `handle_*` go under Handle (`aria-describedby`) and focus Handle. `bio_too_long` goes under Bio and `link_invalid` under Links. Anything else goes in a banner.
  - Typed values are always kept.
  - A checkbox "Compact block cards" maps to `settings.compact` and is sent as `settings: { compact }` in the same PATCH.
- **`AvatarPicker`:**
  - File input `accept="image/png,image/jpeg,image/webp"`.
  - A wrong type shows the `avatar_type` words.
  - A file over 5 MB is refused before decoding ("That picture is too large to read.").
  - Otherwise it calls `cropToSquare` → shows the preview `<img alt="Preview of your new picture">`.
  - A blob over `AVATAR_MAX_BYTES` shows the `avatar_too_large` words with no upload.
  - "Use this picture" → `putAvatar(blob)`. "Remove picture" → `deleteAvatar()`.

- [ ] **Step 1: Write the failing tests.** `vi.mock('./avatarCrop', …)` stands in for canvas in jsdom.

```tsx
it('a handle taken while editing shows under Handle and keeps the other fields', async () => {
  fakeFetch({ 'PATCH /api/community/me': () => [409, { error: { code: 'handle_taken', message: 'x' } }] });
  renderCommunity(<YourProfilePage />, { path: '/community/me', state: { me: meUser({ handle: 'you-ex', bio: '' }) } });
  await userEvent.clear(screen.getByLabelText('Handle')); await userEvent.type(screen.getByLabelText('Handle'), 'taken');
  await userEvent.type(screen.getByLabelText('Bio'), 'Builds review teams.');
  await userEvent.click(screen.getByRole('button', { name: 'Save profile' }));
  const handle = screen.getByLabelText('Handle');
  expect(handle).toHaveAccessibleDescription(expect.stringContaining('Someone already has that handle'));
  expect(handle).toHaveFocus(); expect(screen.getByLabelText('Bio')).toHaveValue('Builds review teams.');
});
it('sends the handle only when it changed', async () => { /* PATCH body has no 'handle' key when unchanged */ });
it('checks handle rules before sending', async () => { /* 'Admin' → handle_invalid words, no PATCH; 'support' → handle_reserved words */ });
it('a picture still over 256 KB after cropping is refused before upload', async () => {
  vi.mocked(cropToSquare).mockResolvedValue(new Blob([new Uint8Array(262145)], { type: 'image/webp' }));
  // choose a 1 MB png → text COMMUNITY_ERROR_WORDS.avatar_too_large; no PUT /api/community/me/avatar in calls
});
it('server avatar errors are shown in words', async () => { /* PUT → 415 avatar_type → words */ });
it('previews the cropped picture and uploads it on confirm', async () => { /* img alt 'Preview of your new picture'; click 'Use this picture' → PUT with content-type image/webp */ });
it('signed out: asks to sign in', () => { /* me null → 'Sign in to edit your profile.' */ });
// handles.test.ts
it('follows the C3 handle rules', () => {
  for (const ok of ['abc', 'a-b-c', 'a'.repeat(30)]) expect(handleProblem(ok)).toBeNull();
  for (const bad of ['ab', 'a'.repeat(31), '-ab', 'ab-', 'A-b', 'a_b']) expect(handleProblem(bad)).toBe(COMMUNITY_ERROR_WORDS.handle_invalid);
  for (const r of RESERVED_HANDLES) expect(handleProblem(r)).toBe(COMMUNITY_ERROR_WORDS.handle_reserved);
  expect(RESERVED_HANDLES).toHaveLength(16); expect(handleProblem(suggestHandle('Ana Lee!'))).toBeNull();
});
```

- [ ] **Step 2: Run them to see them fail.** Run `npm --prefix web test -- src/community/YourProfilePage.test.tsx src/community/AvatarPicker.test.tsx src/community/handles.test.ts`. Expected: FAIL.
- [ ] **Step 3: Implement** the files above. `cropToSquare` uses `createImageBitmap(file)`, an `OffscreenCanvas`-free `document.createElement('canvas')` at `size×size`, a centre crop and `toBlob`.
- [ ] **Step 4: Run tests, build and lint.** Run `npm --prefix web test && npm --prefix web run build && npm --prefix web run lint 2>&1 | tail -1`. Expected: PASS, at or below baseline.
- [ ] **Step 5: Commit.**

```bash
git add web/src/community && git commit -m "web: edit your Community profile, handle and picture"
```

---

### Task 12: Saved, Yours and Publish from Library (with handle step)

**Files:**
- Create:
  - `web/src/community/SavedPage.tsx`
  - `web/src/community/YoursPage.tsx`
  - `web/src/community/PublishPage.tsx`
  - `web/src/community/PublishForm.tsx`
  - `web/src/community/HandleStep.tsx`
- Test: `web/src/community/SavedYours.test.tsx`, `web/src/community/PublishPage.test.tsx`

**Interfaces:**
- Consumes:
  - Tasks 6, 8 and 11: `communityApi.saved/mine/unsave/unpublish/publish/updateMe`, `BlockCard`, `handleProblem`, `suggestHandle`;
  - `api.groupBlocks()`, `sanitizeExport`, `TemplatePreview`.
- Produces:
  - `SavedPage()` at `/community/saved`. Each card has "Remove save".
  - `YoursPage()` at `/community/mine`. Each card has "Unpublish" (with confirmation text "Unpublish {title}? It disappears from Explore."). There is a "Publish a group" link to `/community/publish`.
  - `PublishPage()` at `/community/publish`. Signed out, it shows `SignInPrompt`.
  - `HandleStep({ name, onDone }: { name: string; onDone: () => void })`. Its input is pre-filled with `suggestHandle(name)`, its button is "Use this handle", and it sends `updateMe({ handle })`.
  - `PublishForm({ me, onPublished })`.
- **Publish steps:**
  1. If `me.handle === null`, show `HandleStep` with the heading "Choose your handle" and the text "Your blocks and profile appear under it."
  2. Pick a group from the Library: a radio list of `api.groupBlocks()` names with agent counts. An empty Library links to `/library/groups`.
  3. Show a preview (`TemplatePreview` of `sanitizeExport(block).draft`) and `<details open>` "Review exactly what will be published".
  4. Fill in Title (required, ≤150), Description (required, ≤3000), Tags (comma-separated, ≤240) and License (MIT, Apache-2.0 or CC0-1.0).
  5. The reviewed checkbox has the existing wording.
  6. "Publish publicly" sends `communityApi.publish({ …, content: sanitizeExport(block), reviewed: true })`.
  7. Success navigates to `/community/blocks/{id}`. `409 handle_required` returns to step 1. `publish_limit` shows words.

- [ ] **Step 1: Write the failing tests.**

```tsx
it('asks for a handle first when there is none, then publishes a reviewed Library group without private fields', async () => {
  // me handle null; PATCH me {handle:'you-example'} → me with handle; groupBlocks → [block with folder '/secret']
  // pick it, fill title/description, tick review, Publish publicly → POST /api/community/blocks body.content.draft.agents[0].folder === null, body.content.source undefined, body.reviewed true
  // then location '/community/blocks/<id>'
});
it('Publish stays disabled until a group is chosen and reviewed, and says why', async () => { /* visible text 'Choose a group and confirm the review to publish.' */ });
it('a 409 handle_required brings back the handle step', async () => {});
it('publish_limit is said in words', async () => {});
it('Saved removes a save and Yours unpublishes after confirming', async () => { /* DELETE blocks/b1/save; DELETE blocks/b2 */ });
```

- [ ] **Step 2: Run them to see them fail.** Run `npm --prefix web test -- src/community/SavedYours.test.tsx src/community/PublishPage.test.tsx`. Expected: FAIL.
- [ ] **Step 3: Implement** the files above.
- [ ] **Step 4: Run tests, build and lint.** Run `npm --prefix web test && npm --prefix web run build && npm --prefix web run lint 2>&1 | tail -1`. Expected: PASS, at or below baseline.
- [ ] **Step 5: Commit.**

```bash
git add web/src/community && git commit -m "web: Saved, Yours and Publish from Library with a handle step"
```

---

### Task 13: Moderation (admins)

**Files:**
- Create: `web/src/community/ModerationPage.tsx`
- Test: `web/src/community/ModerationPage.test.tsx`

**Interfaces:**
- Consumes: Task 6 `communityApi.reports/moderateBlock/moderateProfile`, `Report` (R10).
- Produces: `ModerationPage()` at `/community/moderation`.
- When `!me?.admin`, it shows only "Only Community moderators can see this page." and makes no request.
- Each report shows its reason as `<blockquote data-raw-evidence>`, because the report text is evidence.
- **Block report:** a link to the block, then "Hide block" (`moderateBlock(id, true)`) and "Keep it and close the report" (`moderateBlock(id, false)`).
- Reports are split by `report.kind` (R10): `'block'` uses `report.block` and `'profile'` uses `report.handle`.
- **Profile report:** a link to `/community/u/{handle}`, then checkboxes "Reset bio", "Reset picture" and "Reset handle", "Also disable this account", and "Apply" (`moderateProfile(handle, {reset, disable})`).
- The list reloads after each action. Empty: "No open reports."

- [ ] **Step 1: Write the failing tests.**

```tsx
it('non-admins see why and nothing is fetched', () => { /* me admin false → text; fetch not called */ });
it('hides a reported block', async () => { /* POST /api/community/blocks/b1/moderate body {hidden:true}; reports reloaded */ });
it('resets a reported profile’s picture and handle and can disable it', async () => { /* POST users/spam/moderate body {reset:['avatar','handle'],disable:true} */ });
```

- [ ] **Step 2: Run them to see them fail.** Run `npm --prefix web test -- src/community/ModerationPage.test.tsx`. Expected: FAIL.
- [ ] **Step 3: Implement** the page.
- [ ] **Step 4: Run tests, build and lint.** Run `npm --prefix web test && npm --prefix web run build && npm --prefix web run lint 2>&1 | tail -1`. Expected: PASS, at or below baseline.
- [ ] **Step 5: Commit.**

```bash
git add web/src/community && git commit -m "web: Community moderation page for admins"
```

---

### Task 14: Routes, layout, top bar, Ctrl K and hosted entry

**Files:**
- Create:
  - `web/src/community/CommunityLayout.tsx`, which provides `CommunityContext`.
  - `web/src/community/CommunityTabs.tsx`
  - `web/src/community/CommunityHost.tsx`, which renders the Shell or the frame.
  - `web/src/community/CommunityFrame.tsx`
  - `web/src/community/CommunityAvatar.tsx`
  - `web/src/community/routes.tsx`
- Modify: `web/src/App.tsx`
  - Remove the `if (hosted)` branch and the `release/Community` and `release/Profile` imports.
  - Mount `communityRoutes(openStream)` *before* the `<Route element={<Shell/>}>` tree.
  - Delete the Shell's `/community/*` route.
- Modify: `web/src/shell/TopBar.tsx`
  - In `tb-nav`, after the Library link, add `<NavLink to="/community">Community</NavLink>` and `<CommunityAvatar />`.
  - In `AppMenu`, mark the Community link `tb-narrow-only`.
- Modify: `web/src/shell/commands.ts` `appCommands`. Add the commands below.
- Modify: `web/src/hosted/HostedGate.tsx`
  - `OPEN` gains `/^\/community(\/|$)/`.
  - When signed in with `user.workspace === false`, any path outside `/community`, `/account` or `OPEN` gets `<Navigate replace to="/community" />`.
- Modify: `web/src/hosted/session.ts`: `HostedUser.workspace?: boolean`.
- Test:
  - `web/src/community/routes.test.tsx`
  - `web/src/shell/Shell.test.tsx`: the Community link is now in `Go to` and also in the app menu.
  - `web/src/shell/commands.test.ts`
  - `web/src/hosted/hosted.test.tsx`

**Interfaces:**
- Consumes: every page from Tasks 7–13; `useCommunityLink`, `hostedLink`, `useHosted`, `Shell`, `AgentDesignGuide`.
- Produces:

```tsx
export function communityRoutes(openStream?: OpenStream): ReactElement;   // a <Route element={<CommunityHost openStream={openStream} />}> fragment
// paths, all C6:
// /community/learn → AgentDesignGuide (outside CommunityLayout: no link fetch gate, no network)
// <Route element={<CommunityLayout/>}>: /community (ExplorePage), /community/people, /community/u/:handle, /community/me,
//   /community/saved, /community/mine, /community/publish, /community/moderation, /community/blocks/:id
// redirects: /community/explore → /community ; /community/* (anything else) → /community
```

- **`CommunityHost`:** it renders `<Shell openStream={openStream} />`, unless the session is hosted and either signed out or `workspace === false`. In that case it renders `<CommunityFrame />`: a brand link, Community, and on the right either the user's name or, when signed out, Sign in (to `/login`) plus "Join Community". "Join Community" offers only "Continue with Google" (`/gw/api/oidc/google/start?purpose=community`) and "Continue with GitHub" (`/gw/api/oidc/github/start?purpose=community`); C4 has no password sign-up. Both wrap `<Outlet />`. On a `?error=` query, the frame shows `errorText` for `bad_purpose`, `already_member` or `too_many_attempts`.
- **`CommunityLayout`:**
  1. If `link === null`, it shows "Loading Community…".
  2. If `!link.enabled`, it shows `<TurnOnCommunity onDone={(signIn) => { refresh(); if (signIn) setShowSignIn(true) }} />` and fetches nothing else.
  3. Otherwise it loads `communityApi.me()` and provides `CommunityState`. It renders `<CommunityTabs />` (Explore · People · Saved · Yours, plus Moderation when `me.admin`), then an inline `SignInToSculptured` when asked, then `<Outlet />`.
  4. Its root `<main className="page page-wide community" data-vocab="community">`.
- **`CommunityAvatar`:**
  - It renders nothing while `link` is null, off, or signed out.
  - Otherwise it renders `<Link to="/community/me" aria-label="Your profile">`, showing `Avatar` with `src = account.handle ? avatarSrc(account.handle) : null`.
  - It makes no request beyond `useCommunityLink`.
- **Commands** added to `commands.ts`, all `section: 'Go to'`:
  - `{ id: 'community.people', title: 'Community: People', words: 'profiles directory', run: () => c.go('/community/people') }`
  - `{ id: 'community.me', title: 'Your Community profile', words: 'handle avatar bio', run: () => c.go('/community/me') }`
  - `{ id: 'community.saved', title: 'Community: Saved blocks', run: () => c.go('/community/saved') }`
  - `{ id: 'community.publish', title: 'Publish a group to Community…', words: 'share upload', run: () => c.go('/community/publish') }`
  - The existing `app.community` stays.

- [ ] **Step 1: Write the failing tests.**

```tsx
it('the community section asks first and fetches nothing while off', async () => {
  const { calls } = fakeFetch({ 'GET /api/community-link': () => [200, { enabled: false, origin: null, signed_in: false, account: null }] });
  renderApp('/community');   // AppRoutes inside MemoryRouter, useHosted → local
  expect(await screen.findByRole('heading', { name: 'Turn on Community' })).toBeInTheDocument();
  expect(calls.filter((c) => c.includes('/api/community/'))).toEqual([]);
});
it('routes every C6 path to its page and keeps Learn without asking to turn on', async () => {
  // table of [path, heading]: '/community'→'Explore', '/community/people'→'People', '/community/u/ana'→'Ana Lee', '/community/me'→'Your profile',
  // '/community/saved'→'Saved', '/community/mine'→'Yours', '/community/publish'→'Publish a group', '/community/moderation'→'Moderation',
  // '/community/blocks/b1'→'Review pair'; '/community/learn' renders AgentDesignGuide with link off
});
it('old Community addresses redirect', async () => { /* '/community/explore' → '/community'; '/community/whatever/x' → '/community' */ });
it('the tab row is Explore · People · Saved · Yours, and Moderation only for admins', async () => {});
it('the top bar shows Community next to Library and the profile avatar when signed in', async () => { /* Shell.test: nav links ['Library','Community',…]; avatar link name 'Your profile' href '/community/me' */ });
// commands.test.ts
it('Community pages are one Ctrl K away', () => { for (const id of ['community.people','community.me','community.saved','community.publish']) expect(find(id)).toBeDefined(); });
// hosted.test.tsx
it('hosted: signed-out visitors may browse Community; a Community-only account lands on /community', async () => {});
```

- [ ] **Step 2: Run them to see them fail.** Run `npm --prefix web test -- src/community/routes.test.tsx src/shell src/hosted`. Expected: FAIL.
- [ ] **Step 3: Implement** the layout, host, frame, routes, top bar lines, commands and HostedGate rules as specified. Re-read `TopBar.tsx` and `commands.ts` first.
- [ ] **Step 4: Run all web tests, build and lint.** Run `npm --prefix web test && npm --prefix web run build && npm --prefix web run lint 2>&1 | tail -1`. Expected: PASS, at or below baseline.
- [ ] **Step 5: Check visually.** Run `cargo run -p sculptured -- serve --web-dir web/dist --community-origin http://127.0.0.1:14411` with `cargo run -p fixtures --bin fake-community -- --port 14411` running. Capture `/community` while off, on, and signed in, with the desktop and narrow layouts. Save the captures in the scratchpad and report them. Report any check you could not run.
- [ ] **Step 6: Commit.**

```bash
git add web/src/community web/src/App.tsx web/src/shell/TopBar.tsx web/src/shell/Shell.test.tsx web/src/shell/commands.ts web/src/shell/commands.test.ts web/src/hosted/HostedGate.tsx web/src/hosted/session.ts web/src/hosted/hosted.test.tsx
git commit -m "web: Community section in the top bar with its tab row, routes and commands"
```

---

### Task 15: Remove the old Community build and transfer code

**Files:**
- Delete:
  - `web/src/release/Community.tsx`, `Community.test.tsx`
  - `web/src/release/Profile.tsx`
  - `web/src/release/LocalCommunity.tsx`, `LocalCommunity.test.tsx`
  - `web/src/release/Nav.tsx`, `ReleasePage.tsx`
  - `web/src/release/workspaceLink.ts`, `workspaceLink.test.ts`
  - `web/src/groups/blockTransfer.ts`
  - `web/src/generated/Publication.ts`, `CommunitySession.ts`, `CommunityUser.ts`, which are the stale copies
- Modify:
  - `web/src/groups/GroupLibrary.tsx`: remove the `#import-block=` effect and the `transferredBlock` import.
  - `web/src/groups/flows.test.tsx`: remove the `blockTransfer` case.
  - `web/src/shell/LibraryPage.tsx`: "Community account" → "Community", still to `/community`.
  - `web/lint/eslint.config.js`: drop `'community-dist'` from `ignores`.
  - `.gitignore`, `.dockerignore`: drop `web/community-dist`.
  - `web/src/hosted/analytics.ts`: keep the `add-from-community` event and fire it from `BlockActions`' Add to project.
  - `web/src/release/release.css`: remove rules used only by the deleted components (`release-shell`, `workspace-nav`, `connect-*`).
- Test: the existing suites, plus `web/src/community/cleanup.test.ts`.

**Interfaces:**
- Consumes: Task 14 routing, which no longer imports `release/*` Community files.
- Produces: none.

- [ ] **Step 1: Write the failing guard test.**

```ts
// community/cleanup.test.ts — the retired build and the fragment transfer are gone for good
import { readdirSync, readFileSync } from 'node:fs';
it('no source mentions VITE_COMMUNITY, LocalCommunity, blockTransfer, workspaceLink or #import-block', () => {
  const files = (readdirSync('src', { recursive: true }) as string[]).filter((f) => /\.(ts|tsx)$/.test(f) && !f.endsWith('cleanup.test.ts'));
  const hits = files.filter((f) => /VITE_COMMUNITY|LocalCommunity|blockTransfer|workspaceLink|#import-block/.test(readFileSync(`src/${f}`, 'utf8')));
  expect(hits).toEqual([]);
});
```

- [ ] **Step 2: Run it to see it fail.** Run `npm --prefix web test -- src/community/cleanup.test.ts`. Expected: FAIL, listing the old files.
- [ ] **Step 3: Delete and modify** as listed. Run `rg "generated/(Publication|CommunitySession|CommunityUser)" web/src`. Expected: no hits; every import uses `generated/community/`.
- [ ] **Step 4: Run tests, build and lint.** Run `npm --prefix web test && npm --prefix web run build && npm --prefix web run lint 2>&1 | tail -1`. Expected: PASS, and the count is at or below baseline. It should be lower, since the deleted files carried lint errors.
- [ ] **Step 5: Commit.**

```bash
git add -A web/src/release web/src/groups web/src/generated web/src/community/cleanup.test.ts web/src/shell/LibraryPage.tsx web/src/hosted/analytics.ts web/lint/eslint.config.js .gitignore .dockerignore
git commit -m "web: remove the separate Community build and the URL-fragment transfer"
```

---

### Task 16: End-to-end with the stub upstream

**Files:**
- Modify: `crates/e2e-server/src/main.rs`
  - Build `fake-community` the same way as `fake_codex`, honouring `RIG_E2E_FAKE_COMMUNITY`.
  - Start it on `RIG_E2E_COMMUNITY_PORT` (default `14411`) before serve.
  - Pass `--community-origin http://127.0.0.1:<port>` to serve.
- Modify: `crates/e2e-server/src/lib.rs`: `Session::adopt_stub(child: Child)`. `shutdown` kills it (SIGTERM, then SIGKILL after 5 s) before removing the directory.
- Create: `web/e2e/community.spec.ts`
- Create: `web/e2e/fixtures/avatar.png`, a 64×64 PNG kept by the test.

**Interfaces:**
- Consumes: Task 2 stub control routes; Tasks 3–15 behaviour.
- Produces: none.
- `const STUB = \`http://127.0.0.1:${process.env.RIG_E2E_COMMUNITY_PORT ?? '14411'}\``.
- `beforeEach`: `POST ${STUB}/stub/reset`.
- `afterEach`: `DELETE /api/community-link` through `request`.
- When `SHOTS` is set, each test saves `page.screenshot` to `${SHOTS}/community-<name>.png`.

- [ ] **Step 1: Write the specs.**

```ts
test('Community stays off and silent until turned on', async ({ page, request }) => {
  const seen: string[] = []; page.on('request', (r) => seen.push(new URL(r.url()).pathname));
  await page.goto('/community');
  await expect(page.getByRole('heading', { name: 'Turn on Community' })).toBeVisible();
  await expect(page.getByText('Your projects, files, transcripts and runs.')).toBeVisible();
  expect(seen.filter((p) => p.startsWith('/api/community/'))).toEqual([]);
  expect(await (await request.get(`${STUB}/stub/requests`)).json()).toEqual([]);
});
test('turn on and browse, then add a block to a new project in two clicks', async ({ page }) => {
  // Turn on and browse → Explore shows 'Review pair' by 'Ana Lee'; open it; 'Add to project' → dialog; 'Add & open project'
  // → URL /p/<id>/board and both agents visible on the board
});
test('sign in with a code, edit the profile, meet a taken handle, upload a picture', async ({ page, request }) => {
  // Turn on and sign in → code 'ABCD-EFGH' visible; POST stub/pairing {mode:'approve'} → 'Signed in as You Example'
  // avatar link 'Your profile' → handle 'taken' → 'Someone already has that handle' under Handle; handle 'you-example' → saved
  // set avatar.png → preview → 'Use this picture' → top-bar avatar <img> src starts with '/api/community/users/you-example/avatar'
});
test('save to Library, then update to v2', async ({ page, request }) => {
  // signed in; block page → 'Save to Library' → 'In your Library'; POST stub/blocks/b-review-pair/bump; reload → 'Update to v2' → click
  // /library/groups shows one 'Review pair' card with 'From Community · v2'
});
test('publish a group from the Library after choosing a handle', async ({ page }) => {
  // save a group to Library via API; signed in with no handle; /community/publish → 'Choose your handle' → 'Use this handle'
  // pick group, fill, review, Publish publicly → block page with your name
});
test('a block of another kind says it opens in a later version', async ({ page }) => { /* /community/blocks/b-kindex-setup */ });
test('turn off forgets the sign-in', async ({ page, request }) => {
  // Account → 'Turn off Community' → /community shows 'Turn on Community'; GET /api/community-link → enabled false
});
```

- [ ] **Step 2: Run them to see them fail.** Run `npm --prefix web run e2e -- community.spec.ts`. Expected: FAIL before the e2e-server change (stub not running, so `/stub/reset` refuses the connection).
- [ ] **Step 3: Implement the `e2e-server` changes.** Run `cargo test -p e2e-server`. Expected: the existing tests pass.
- [ ] **Step 4: Run e2e.** Run `npm --prefix web run e2e -- community.spec.ts`, then the full `npm --prefix web run e2e`. Expected: all pass on the desktop and mobile projects. Report any test that cannot run here.
- [ ] **Step 5: Commit.**

```bash
git add crates/e2e-server web/e2e/community.spec.ts web/e2e/fixtures/avatar.png
git commit -m "e2e: Community flows against the stub upstream"
```

---

### Task 17: Docs

**Files:**
- Modify:
  - `docs-site/guides/community.mdx`
  - `docs-site/guides/groups.mdx` (the "From Community" section and the mermaid diagram)
  - `docs-site/security.mdx` (`## Community`)
  - `docs-site/privacy.mdx` (`## Community`)
  - `CHANGELOG.md`
- Create: `docs/internals/community.md`
- Modify: `docs/internals/README.md` (table row "Community | [Community](community.md) | Opt-in link, pairing, proxy allow-list"), `docs/internals/testing.md` (the `fake-community` stub and `RIG_E2E_COMMUNITY_PORT`)

**Interfaces:** none. Each doc names the real UI words from `COMMUNITY_WORDS`.

- **`guides/community.mdx`** covers:
  - **Turn on Community:** the exact opt-in copy, and that nothing is sent before it.
  - **Sign in to Sculptured:** the code and the sign-in page, which asks you to approve "Community access from ‘{name}’". The key is kept in `~/.sculptured/community.json`, owner-only. It is listed on the Computers page. If it is removed there, the app says "Your Community sign-in was removed. Sign in again."
  - **Explore and block pages:** Add to project (the review step), Save to Library and Update to v{n}.
  - **People and profiles:** each profile field and what is public.
  - **Your profile:** handle rules, the 30-day change limit, old handles redirecting for 90 days, and the picture's ≤256 KB limit and square crop.
  - **Publish from your Library:** the handle step, what publishing removes, and the 5 blocks in the first day.
  - **Saved, Yours and Moderation.**
  - **Turn off Community.**
  - **Hosted beta:** it is the same section. Anyone can join Community with Google or GitHub (there is no password sign-up for Community) and gets a Community-only account until invited.
  - Remove "opens on its own site", "Service address" and the fragment hop.
- **`security.mdx`:**
  - Community is reached only through this app's own server. The browser never calls another site.
  - The local key has scope `community`. It cannot run agents, open your computer's runner link or reach files, transcripts or Git.
  - The proxy allow-list and its limits.
  - The key never reaches the browser.
  - Turning Community off revokes the key.
- **`privacy.mdx`:**
  - Opt-in: what is sent when on and what never leaves.
  - Identity is your Sculptured account.
  - Profiles are public: name, handle, bio, links, picture, joined date, published blocks and counts.
  - Empty fields are not shown.
  - Reports can target a profile.
- **`docs/internals/community.md`:**
  - an architecture diagram as text: browser → controld `/api/community/*` → gateway → Community;
  - the C5 endpoints;
  - the credentials file;
  - the pairing state machine (pending → approved | expired | denied);
  - the allow-list table;
  - header and size rules R1, R6 and R7, the error codes, and `device_unknown` handling (delete the key, keep the origin);
  - hosted mode (R9);
  - block kinds and `source`;
  - how to regenerate `web/src/generated/community/`;
  - test entry points.

- [ ] **Step 1: Run the docs build to see the current state.** Run `npm --prefix docs-site test`. Expected: it passes on the old text. It is the guard for broken links and MDX.
- [ ] **Step 2: Write the pages** as specified.
- [ ] **Step 3: Run the docs build.** Run `npm --prefix docs-site test && npm --prefix docs-site run build`. Expected: PASS. No link to the removed "Service address" anchors remains: run `rg -n "Service address|own site|import-block" docs-site --glob '!dist/**'`, which should find nothing.
- [ ] **Step 4: Commit.**

```bash
git add docs-site/guides/community.mdx docs-site/guides/groups.mdx docs-site/security.mdx docs-site/privacy.mdx docs/internals CHANGELOG.md
git commit -m "docs: Community inside the app, opt-in, profiles and what is sent"
```

---

## Self-review

**1. Spec coverage**

| Spec item | Task |
|---|---|
| C5 settings migration | 1 |
| C5 credentials and link endpoints | 3 |
| C5 pairing | 4 |
| C5 proxy (allow-list, strip, bearer, limits, `community_off`) | 5 |
| Stub upstream in fixtures | 2 |
| C6 `api.ts`, `link.ts`, generated types | 6 |
| Opt-in screen and Sign in to Sculptured | 7 |
| Explore and block page with Add to project | 8 |
| Save to Library, `source`, Update to v{n} | 9 |
| People and profiles | 10 |
| Your profile (handle, name, bio, links, avatar with crop and preview) | 11 |
| Saved, Yours, Publish with handle step | 12 |
| Moderation | 13 |
| Routes, top bar, tab row, avatar, Ctrl K, redirects, hosted entry | 14 |
| Cleanup of `VITE_COMMUNITY`, LocalCommunity, `blockTransfer`, `workspaceLink` | 15 |
| e2e via `crates/e2e-server` | 16 |
| docs-site and internals | 17 |
| Error codes in `errors.ts` | 6, used in 7–13 |

- **Design items left to the other repos:** gateway routing and assertion; the scope column and approval page; Community-only sign-up; the Community migrations and endpoints; Dockerfile Community builds; `OPERATIONS.md`; `DEPLOY.md`.
- **Design item not built here:** "listed and revocable on the Computers page as Community access from …" is the gateway's list. The web `ComputersPage` shows whatever `/gw/api/devices` returns, so no change is planned. Flagged for the cloud plan.

**2. Step scan.** Each code step names its file, signature and pinned values. Test steps carry their assertions. No step says "handle edge cases" or "TBD".

**3. Type consistency.**
- `CommunityLink`, `CommunityLinkView` and `CommunityPairState` keep the same names in Tasks 3, 4, 6 and 7.
- `communityApi`, `useCommunity` and `opensHere` match from Task 6 onward.
- `libraryState`/`libraryCopy` are used only by Task 9.
- `SignInPrompt` is defined in Task 7 and used in Tasks 8, 10, 11 and 12.
- `Avatar`, `BlockCard` and `LoadProblem` are defined in Task 8 and used in Tasks 10–14.
- `handleProblem` and `suggestHandle` are defined in Task 11 and used in Task 12.

**4. Review Focus.** All 9 lines have owning tests named above. They cover the 7 user-named risks, plus the SVG avatar script risk and the revoked-key (`device_unknown`) path from the updated C4/C5.

**6. Contract updates folded in (2026-10-18 revisions).**
- **C3:**
  - the unchanged endpoints are in the allow-list (Task 5);
  - `kind` drives `opensHere`, and the stub seeds `group` and `component`;
  - `Me.user.settings.compact` is used in Tasks 8 and 11;
  - coded errors, with the legacy shape handled by R11;
  - `account_disabled`, `rate_limited` and `invalid` have words;
  - `data:image/svg+xml` avatars (R8, R7 CSP);
  - reports use `kind` and `handle` (R10);
  - the export also rewrites root model types (Task 6 Step 1 diff check).
- **C4/C5:**
  - the Origin is stripped on bearer requests (R1);
  - `device_unknown` deletes the key and keeps the origin (Tasks 3, 5, 6);
  - the key is checked via `GET /api/community/me`;
  - sign-up is Google and GitHub only (Task 14 frame, Task 17);
  - gateway codes are mapped in `errors.ts`, and passed through by `POST pair` (Task 4);
  - the approval wording appears in the docs.
- **Still open, and reported:**
  - R2 (`denied`);
  - R3 (a bearer self-revoke endpoint);
  - R6 (relative redirect `Location`);
  - R10 (the exact report item shape);
  - R4 controld codes, to be added to C5.

**5. Proportion.** Code blocks are signatures and test assertions. Bodies are left to the implementer, except for the exact error words and opt-in copy.

---

## Contract updates after planning (2026-10-18), binding on execution

- The open points this plan raised are now in the contracts:
  - the `denied` pairing status, following the gateway's `403 access_denied`;
  - `POST /gw/api/device/revoke`, authenticated by the device's bearer;
  - controld's proxy error codes;
  - the relative 301 `Location`;
  - the report item shape;
  - avatar CSP and nosniff.
- Use them as written there; the plan's fallbacks (treat denied as expired, ignore a revoke 404) are no longer needed.
- `Profile` as a plain word: add it to the allow-list for Community pages, as planned.
