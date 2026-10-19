# Community inside the app: cross-repo contracts

This file is a companion to `2026-10-18-community-integration-design.md`. The three implementation plans (`sculptured-community`, `sculptured-cloud` and `sculptured`) must match these names and shapes exactly. If a plan needs to change one, it changes it here first.

## Decision refinement (vs the design)

Community is compiled into the gateway process, so the gateway hands Community the signed-in user **in process**, as an axum request extension. The design's "signed identity headers" become this extension, because there is no network hop between them and so no shared secret to manage. Network hops (local controld → gateway) are authenticated with the paired device key.

## C1. `AssertedUser` (crate `sculptured-community`, public)

```rust
#[derive(Clone, Debug)]
pub struct AssertedUser {
    pub gateway_user_id: uuid::Uuid,   // users.id in the gateway
    pub name: String,                  // users.name, seeds display_name on first use
    pub admin: bool,                   // users.role == "admin"
    pub workspace: bool,               // false = Community-only account
    pub write_checked: bool,           // gateway verified session CSRF (browser) or bearer device key (local)
    pub client_ip: std::net::IpAddr,   // for per-user and per-IP rate limits
}
```

- The gateway inserts `Option<AssertedUser>` into request extensions on every `/api/community/*` request, with `None` when signed out.
- Community resolves the user from it. It maps the user to `community_users(issuer = "urn:sculptured:gateway", subject = gateway_user_id.to_string())` and creates the row on first sight.
- A write with `None` gets `401 {"error":{"code":"signed_out"}}`. A write with `Some(u)` where `!u.write_checked` gets `403 {"error":{"code":"csrf"}}`.
- The gateway also inserts `sculptured_community::ClientIp(pub IpAddr)` on **every** `/api/community/*` request, signed in or not, so signed-out reads can be rate-limited per IP.
- During the transition window, the legacy cookie session path still works on the standalone origin only.

## C2. Embedding API (crate `sculptured-community`)

```rust
pub fn api_router(app: App) -> axum::Router;   // only /api/community/* routes; no Host/Origin guard, no SPA fallback
pub async fn migrate(pool: &sqlx::PgPool) -> anyhow::Result<()>;  // runs community migrations (its own sqlx migrator)
pub async fn link_legacy_google(pool: &PgPool, google_subject: &str, gateway_user_id: Uuid) -> anyhow::Result<bool>; // idempotent relink
impl App { pub fn embedded(db: sqlx::PgPool, admins: Vec<String>) -> App }   // no OIDC; App.oidc becomes Option
pub async fn official::install(pool: &PgPool) -> anyhow::Result<()>;         // the gateway calls it after migrate(), so example blocks are published
#[derive(Clone, Copy, Debug)] pub struct ClientIp(pub std::net::IpAddr);                                    // request extension, see C1
```

The existing standalone `router(App, web)` remains during the transition and gains nothing new.

## C3. Community HTTP API (JSON, under `/api/community/`)

```
Author   = { handle: string, name: string, avatar_url: string }       // avatar_url always set (initials SVG when none)
Profile  = { handle, name, bio: string, links: string[], avatar_url, joined_at: rfc3339,
             counts: { published: number, saved_by_others: number } }
Me       = { user: { handle: string|null, name, bio, links, avatar_url, admin: bool, workspace: bool, settings: { compact: boolean } } | null }
```

| Method & path | Body / query | Response |
|---|---|---|
| GET `me` | | `Me` (no csrf field any more) |
| PATCH `me` | `{name?, handle?, bio?, links?, settings?}` | `Me` |
| PUT `me/avatar` | raw bytes; `Content-Type: image/png\|jpeg\|webp`; ≤ 262144 bytes | `{avatar_url}` |
| DELETE `me/avatar` | | `{avatar_url}` |
| GET `people` | `?q=&offset=` (24 per page) | `{people: Profile[], next: number\|null}` |
| GET `users/{handle}` | | `{profile: Profile, blocks: Publication[]}`; an old handle within 90 days gives `301` with a **relative** `Location: /api/community/users/{new}` |
| GET `users/{handle}/avatar` | | an image with `ETag`, `Cache-Control: public, max-age=3600` |
| GET `blocks` | `?q=` (also matches author handle and name) `&offset=` | as today, but each item has `author: Author` and no `owner` |
| POST `blocks` | as today | as today; `409 {"error":{"code":"handle_required"}}` when the publisher has no handle; `429 {"error":{"code":"publish_limit"}}` past 5 in the first 24 h of an account |
| POST `blocks/{id}/report` | `{reason, target_kind?: "block"\|"profile"}` | as today |
| POST `users/{handle}/report` | `{reason}` | `{ok:true}` |
| POST `users/{handle}/moderate` | `{reset: ("bio"\|"avatar"\|"handle")[] , disable?: bool}` (admin) | `{ok:true}` |

**Unchanged endpoints that are still served** (they are part of the C5 allow-list too):
- GET and DELETE `blocks/{id}`;
- POST and DELETE `blocks/{id}/save`;
- GET `saved`, GET `mine`;
- GET `reports` (admin): items are `{id, reason, kind: "block"|"profile", block?: {id, title}, handle?: string, created_at}`. `block` is set for block reports and `handle` for profile reports.
- POST `blocks/{id}/moderate` (admin).

Block list and detail responses also carry `kind: "group" | "component"` (added by the components work in a parallel session; old blocks are `group`).

**Handle rules:**
- `^[a-z0-9](?:[a-z0-9-]{1,28}[a-z0-9])$`, so 3–30 characters.
- At most one change per 30 days; the first change after a migration-assigned handle is free.
- Reserved: `admin, administrator, sculptured, official, support, help, root, system, moderator, community, api, about, settings, me, people, u`.

**Other limits:**
- Bio: up to 280 characters, with no control characters except `\n`.
- Links: up to 3, each an `https://` URL of at most 200 characters.

**Errors:** Community **adopts** the coded shape `{"error":{"code","message"}}`; today it returns `{"error":"<message>"}`. So the web app and Community must deploy together. Generic codes `invalid` and `not_found` also occur, along with `account_disabled` (403) and `rate_limited` (429). These are the codes the web app maps to plain words:
- `handle_taken`
- `handle_invalid`
- `handle_reserved`
- `handle_too_soon`
- `bio_too_long`
- `link_invalid`
- `avatar_too_large`
- `avatar_type`
- `handle_required`
- `publish_limit`
- `signed_out`
- `csrf`
- `account_disabled`
- `rate_limited`

**Avatar URLs** may be `data:image/svg+xml,...` (initials, when there is no upload or no handle). Every CSP serving the app (gateway, controld, `web/index.html`) must allow `img-src 'self' data:`.

**The publish limit's 24 hours** count from the user's first Community use (the row's `created_at`), not from gateway account creation.

## C4. Gateway (crate `sculptured-cloud`)

- **Devices:** `devices.scope text NOT NULL DEFAULT 'computer' CHECK (scope IN ('computer','community'))`.
  - `POST /gw/api/device/start {name, platform, scope?: "computer"|"community"}` (default `computer`).
  - The approval page shows "Community access from ‘{name}’" for scope `community`.
  - A `community` key is rejected by `/host` and by `proxy::forward`.
- **Users:** `users.workspace boolean NOT NULL DEFAULT true`. Community-only sign-ups get `false`; redeeming an invite sets `true`.
  - A user with `workspace = false` gets `403 {"error":{"code":"community_only","message":"This account has Community only. Ask for an invite to run agents here."}}` from `proxy::forward`, `/host`, `instances.ensure` callers and `scope: "computer"` pairing.
- **Sign-up:** `GET /gw/api/oidc/{provider}/start?purpose=community` creates an account with no invite needed, and lands on `/community`.
- **Route:** `/api/community/*` is handled by `sculptured_community::api_router`, before `proxy::forward`.
  - Middleware builds the `AssertedUser` from one of:
    - a browser session: `write_checked` = the session CSRF header is valid, or the method is GET/HEAD;
    - `Authorization: Bearer <device_id>.<key>` with scope `community`: `write_checked = true`.
  - Otherwise it inserts `None`.
- **Bearer requests are exempt from the gateway's Origin check.** The Origin rule applies only to browser writes, where `write_checked` needs both the session CSRF and the app's own Origin.
- **An `Authorization: Bearer` that is invalid, revoked or the wrong scope** gets `401 {"error":{"code":"device_unknown"}}` from the gateway and never reaches Community. A bad key is never treated as signed out.
- **`link_legacy_google`** matches only issuer `https://accounts.google.com`.
  - If no gateway row exists for that user yet, or the row is empty, it folds into the legacy row.
  - If a non-empty gateway row already exists, it returns `Ok(false)`.
  - It never deletes a legacy row.
- **Sign-up for `purpose=community`** works with Google and GitHub only. Password sign-up waits until the gateway can verify email.
- **Claim:** `GET /gw/api/oidc/google/start?purpose=claim`, for GitHub or password users with an old Community account.
- **New gateway API surface:**
  - `/gw/api/me` gains `user.workspace`.
  - Device lookup and lists gain `scope` and `label`.
  - `POST /gw/api/join/{token}/redeem` is new.
  - New codes:
    - `bad_scope`
    - `bad_purpose`
    - `already_member`
    - `community_unavailable`
    - `too_many_attempts` (also on `/login?error=`)
  - A community key is checked with `GET /api/community/me`; `/gw/api/device/check` stays computer-only.
- **The approval page offers "Don't allow".** It marks the code denied, and the next `POST /gw/api/device/token` answers `403 {"error":{"code":"access_denied"}}`.
- **`POST /gw/api/device/revoke`** is authenticated by the device's own `Authorization: Bearer`. It revokes that key, answers `{ok:true}`, and lets the local app turn Community off cleanly.
- **Answers carrying user images** (`/api/community/users/*/avatar`) get `Content-Security-Policy: sandbox; default-src 'none'; img-src 'self' data:` and `X-Content-Type-Options: nosniff`.

## C5. Local controld (crate `controld`, repo `sculptured`)

**Settings:**
- `Settings.community_url` is replaced by `sculptured_origin: Option<String>` (serde alias `community_url` for reading; migrated on load).
- `community.{domain}` maps to `app.{domain}`.
- `None` means Community is off.

**Credentials:** `<data_dir>/community.json`, mode 0600, holding `{origin, device_id, key}`. It is never returned to the browser.

**Local API:**

| Method & path | Response |
|---|---|
| GET `/api/community-link` | `{enabled: bool, origin: string\|null, signed_in: bool, account: {name, handle\|null}\|null}` |
| POST `/api/community-link/enable` | `{origin?: string}` → as GET (`origin` defaults to `https://app.usesculptured.com`; it must be https, or `http://127.0.0.1\|localhost:` in dev) |
| POST `/api/community-link/pair` | `{user_code, verify_url, expires_in}`, which starts device pairing with scope `community` |
| GET `/api/community-link/pair` | `{status: "pending"\|"approved"\|"expired"\|"denied"}`; controld polls the gateway itself and stores the key once approved; `denied` follows the gateway's `access_denied` |
| DELETE `/api/community-link` | `{enabled:false}`; best-effort revoke on the gateway, then deletes `community.json` and the origin |

**Proxy:**
- `/api/community/{path}` is forwarded to `{origin}/api/community/{path}` only when enabled.
- Only the C3 method and path patterns are allowed; anything else gets `404`.
- It strips `Cookie`, `Origin` and `X-CSRF-Token`, and adds `Bearer` when signed in.
- Request body ≤ 300 KB, response ≤ 2 MB, 15 s timeout.
- When off, every `/api/community/*` request returns `409 {"error":{"code":"community_off"}}`.
- **On `401 device_unknown` from upstream,** controld marks the link signed out: it deletes the key and keeps the origin. The page then says "Your Community sign-in was removed. Sign in again."
- **To check a key,** controld calls `GET {origin}/api/community/me`.
- **controld's own proxy errors:**
  - `community_unreachable` (502)
  - `community_timeout` (504)
  - `community_too_large` (413)
  - `community_bad_answer` (502, which also covers a legacy `{"error":"text"}` shape)
  - `community_origin_invalid` (400)
- **On every proxied answer,** controld adds `Content-Security-Policy: sandbox; default-src 'none'; img-src 'self' data:` and `nosniff`. It follows no redirects, and rewrites `Location` to a local path.
- In tests, the upstream is a stub (crate `fixtures`), with no real network.

## C6. Web (repo `sculptured`, `web/`)

**Routes:**
- `/community` (Explore)
- `/community/people`
- `/community/u/:handle`
- `/community/me`
- `/community/saved`
- `/community/mine`
- `/community/publish`
- `/community/moderation`
- `/community/learn` (kept)
- `/community/blocks/:id`

**Data:**
- One module, `web/src/community/api.ts`, with relative URLs, the same in every build.
- Plus `web/src/community/link.ts` for C5.

**Generated TS types:** they come from `sculptured-community` with ts-rs, are exported into `sculptured/web/src/generated/community/` by `cargo test export_bindings` run in that repo (a documented step), and are never hand-edited.

**Cleanup:**
- `VITE_COMMUNITY`, `release/LocalCommunity.tsx`, `groups/blockTransfer.ts` and `release/workspaceLink.ts` are removed.
- The Community build in the Dockerfiles is dropped (in `sculptured-community` and `sculptured-cloud`).
