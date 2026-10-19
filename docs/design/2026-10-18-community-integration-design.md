# Community inside the app: design

Date: 2026-10-18. Status: reviewed; ready to plan.

## Intent

Community stops being a separate site with its own sign-in. It becomes a section of the app, next to Library, so a person can browse blocks and people, open member profiles, publish from their Library and add a block to a project without leaving. The user asked for this directly: "integrate community directly as opposed to be a fully different thing", "allow users with profiles and other stuff", and "same sign in".

Success means three things. A signed-in person never signs in twice. Community looks and feels like the rest of the app (the top bar and the page-per-job structure from the redesign). A block goes from Community into a project in two clicks, and from a project into Community in about three.

## Goals

1. **One identity.** The Sculptured account (the cloud gateway's `users` and `identities`) is the Community identity. Community's own Google sign-in is retired for app use.
2. **Community is a top-bar section in every build** (local `serve`, hosted beta, desktop), with these pages:
   - **Explore:** blocks, searchable by title, tag and author.
   - **People:** a directory of members who have a public profile.
   - **Profile pages:** `/community/u/:handle`.
   - **Your profile:** where you edit your own profile.
   - **Saved.**
   - **Your publications.**
   - **Publish:** from your Library.
   - **Moderation:** admins only.
3. **Profiles:**
   - **Handle:** unique, lowercase `a-z0-9-`, 3–30 characters, changeable at most once every 30 days. Old handles redirect for 90 days.
   - **Display name:** 1–80 characters, as today.
   - **Bio:** up to 280 characters, plain text.
   - **Links:** up to 3 https URLs, shown as their hostnames.
   - **Avatar:** an uploaded square image (PNG, JPEG or WebP, ≤ 256 KB, re-encoded server-side to 256×256 WebP and stored in the Community database). Without one, initials on a colour derived from the handle.
   - **Joined date.**
   - **Their published blocks.**
   - **Counts:** blocks published, and how many times their blocks were saved.
   - **Listing:** a member appears in People only after choosing a handle.
- **Handles are required to publish.** The publish flow asks for one if missing.
  - Existing publishers get a suggested unique handle from their display name at migration (one free change).
  - The built-in examples author takes the reserved handle `sculptured`.
  - Every block author therefore has a profile page. Every field except handle and display name is optional, and empty fields don't render.
4. **Attribution everywhere.** Block cards and pages show the author's avatar and display name, linking to their profile. The internal owner UUID is no longer exposed in public responses; the handle is.
5. **Direct transfer.**
   - "Add to project" writes the block straight into the local Library through the app's own API, with no URL-fragment hop. The existing "review before adding" step stays.
   - Publish reads a group straight from the Library, and the server sanitizes it as today.
6. **Works signed out.** Explore, People and profiles are public and readable without signing in. Saving, publishing, reporting and editing a profile require sign-in.

## Non-goals

- Following, feeds, comments, likes, notifications and direct messages. These are plausible "other stuff" for later, and the profile and route model leaves room for them.
- Self-service account deletion. It doesn't exist today (OPERATIONS.md:109) and stays operator-handled.
- Moving private code into the public `sculptured` repo. See Architecture.
- Community running inside the local app process. It needs Postgres, and the local app is SQLite-only and loopback-only.

## Anti-goals (would make this a failure even if it works)

- **Two sign-ins, or a "Community account" that differs from the Sculptured account.**
- **The local app's browser calling a third-party origin directly.** All Community traffic goes through the app's own server (same origin, no CORS, no `SameSite=None` cookies).
- **Any Community path that can reach a runner, files, transcripts or Git.** Community stays data-isolated: a separate database and no runner APIs, as it is today.
- **Silently publishing.** Publishing remains an explicit, previewed action, and a profile shows only what the member filled in.

## Architecture

### Hosted beta (app.domain)

- **Embed the Community router in the cloud gateway process.** `sculptured_community::router(App, web)` is already a library function. It keeps its own `PgPool` against the existing `community-db`, so migrations stay separate (both repos number from `0001`).
- **The gateway owns `/api/community/*` and routes it before `proxy::forward`.** For a signed-in session, it attaches a **gateway assertion**, a set of signed identity headers produced with the existing `signing` crate HMAC, the same mechanism `proxy.rs` uses for instances. The headers carry the gateway user id, display name, role and the request's CSRF-validated status. Signed-out requests pass with no assertion.
- **Community gains a "trusted gateway" auth mode** next to cookie sessions:
  - It verifies the HMAC.
  - It maps the user to `community_users(issuer = 'urn:sculptured:gateway', subject = <gateway user uuid>)`.
  - It creates the row on first use, seeding `display_name` from the gateway `users.name`.
  - CSRF is enforced by the gateway (its session CSRF). Community trusts the assertion's "write-checked" bit and does not use its own CSRF token for asserted requests.
- **The standalone `community.{domain}` site:**
  - Its read-only pages redirect to `app.{domain}/community/...`.
  - Its sign-in endpoints are removed after migration.
  - Caddy keeps the subdomain only for redirects.
- **Admins** come from gateway `role = admin` instead of `COMMUNITY_ADMIN_SUBJECTS`. The env var remains a fallback for one release.

### Local app (`sculptured serve`) and desktop

- **"Sign in to Sculptured"** is on the Account page. It reuses the gateway's device pairing flow (`/gw/api/device/start`, `token`, `code`, `approve`): controld starts it, the page shows the code and link, the user approves at app.{domain}, and controld receives a key.
- **The key is a new kind of device: scope `community`.** It authenticates as the user for Community requests only. It cannot open the runner link (`/host`) or reach the user's instance. It is listed and revocable on the Computers page as "Community access from ‘Alex's laptop’".
  - Storage: `~/.sculptured/community.json`, mode 0600, holding `{origin, device_id, key}`. This mirrors `device.json`. It is never sent to the browser.
- **controld proxies `/api/community/*`** to `{origin}/api/community/*`:
  - It attaches `Authorization: Bearer <device_id>.<key>`.
  - It forwards only the documented Community endpoints and methods (an allow-list).
  - It strips cookies.
  - It caps request and response sizes.
  - It applies the local guard (Host and Origin) first.
- **The gateway authenticates the bearer** with `devices::authenticate` (requiring scope `community`), then attaches the same gateway assertion as above.
- **Opt-in on self-run installs (user's decision).** A local or desktop install makes no request to any Sculptured server until the user turns Community on.
  - The Community section's first screen is a short "Turn on Community" page. It says what turning it on sends: Community pages and searches you open, and, if you sign in, your publications and saves. It also says what never leaves the machine: projects, files, transcripts and runs.
  - It offers **Turn on and browse** and **Turn on and sign in**.
- **Signed out but turned on:** controld proxies read-only GETs with no credentials, so Explore, People and profiles work.
- **`community_url`:** the existing setting becomes `sculptured_origin`.
  - It defaults to unset, which means Community is off.
  - Turning Community on sets it to `https://app.usesculptured.com`. A self-hoster can change it in Settings.
  - The old `community_url` key is read once and migrated. A value pointing at `community.{domain}` maps to `app.{domain}`.
  - Turning Community off clears the origin and the stored key, and revokes the key on the server when reachable.
- **Tests:** routine tests use a stub Community upstream in `crates/fixtures`. No real network (AGENTS.md).

### Community service changes (`sculptured-community`)

- **Migration `0002`:**
  - `community_users` gains `handle` (unique, nullable until chosen), `handle_changed_at`, `bio`, `links jsonb`, `avatar bytea` and `avatar_etag`.
  - A `community_handle_redirects(old_handle, user_id, until)` table.
- **New endpoints:**
  - `GET /api/community/people?q=&offset=`
  - `GET /api/community/users/{handle}`: profile plus published blocks.
  - `GET /api/community/users/{handle}/avatar`: cacheable by ETag.
  - `PATCH /api/community/me`: now also takes handle, bio and links.
  - `PUT` and `DELETE /api/community/me/avatar`
- **Changed endpoints:**
  - Block responses carry `author: {handle, name, avatar_url}` instead of `owner`.
  - `blocks?q=` also matches author handle and name.
- **Validation and moderation:**
  - A reserved-handle list (admin, sculptured, official, support, …).
  - Profanity-free defaults are not attempted; reports can target a profile (`report` gains `target_kind: block | profile`).
  - Moderators can reset a bio, avatar or handle.
- **Rate limits:** once behind the gateway, the existing per-peer limit would be shared by every user, so it becomes per-asserted-user (and per-IP for signed-out reads, using a gateway-supplied client-IP header that only the gateway can set, inside the signed assertion).

### Who can join (user's decision: anyone)

- **Anyone can sign in** with Google or GitHub on the Community sign-in path, or with a password where the gateway supports it.
  - This creates a **Community-only Sculptured account**, `users.workspace = false`. It gives a profile, saves, publishing and reporting, and no hosted workspace.
  - The hosted-beta invite flow is unchanged: redeeming an invite on an existing Community-only account sets `workspace = true`.
- **Gateway changes:**
  - An invite-free sign-up path, limited to purpose `community`.
  - `proxy::forward`, `instances.ensure`, `/host` and device pairing with `computer` scope all refuse accounts with `workspace = false`, in plain words: "This account has Community only. Ask for an invite to run agents here."
  - HostedGate, the Computers page and the admin views handle a no-workspace account. A Community-only user who opens `app.{domain}` lands on `/community`.
  - Pairing with `community` scope is allowed for every account.
- **Abuse controls:**
  - sign-up rate limits per IP and per identity provider;
  - a new account can publish at most 5 blocks in its first 24 hours;
  - profile edits are rate-limited;
  - admins can disable an account (the existing `disabled` flag), which hides its profile and blocks.

### Migration of existing Community members

- On a member's first gateway sign-in with Google, the gateway's Google `identities` subject is matched to `community_users(issuer = google, subject)`.
- The member's existing row is relinked to the gateway issuer and subject, keeping their blocks, saves and display name. This is done by a one-time, idempotent migration endpoint called by the gateway at sign-in.
- **Transition window:** the old `community.{domain}` Google sign-in keeps working for one release plus 60 days.
  - During it, a signed-in Sculptured user of any provider (Google, GitHub or password) can **claim** an old Community account. They sign in once more with that old Google identity from their profile page, which relinks the row.
  - Unclaimed accounts keep their published blocks after the window, attributed under their display name, without edit rights. An operator can relink on request.
- After the window, the Community service's own OIDC code is removed. Its only auth is the gateway assertion.

### Web app (`sculptured/web`)

- **Retire the build-time `VITE_COMMUNITY` flag and the separate Community build.**
  - Community routes live inside the Shell and top bar: `/community` (Explore), `/community/people`, `/community/u/:handle`, `/community/me`, `/community/saved`, `/community/mine`, `/community/publish`, `/community/moderation`.
  - Old `/community/*` paths redirect.
  - The Learn guide stays at `/community/learn`.
- **One `communityApi` module** with relative `/api/community/*` calls, the same in every build. CSRF is the app's own: in local mode none is needed for the proxy beyond controld's guard; hosted uses the gateway session CSRF.
- **Pages follow the redesign:** one job per page, the top-bar section, and a tab row Explore · People · Saved · Yours. Profile is reached from the avatar in the top bar.
  - Block pages show a large preview of the group (reusing the board renderer read-only), "Add to project", Save and the author card.
  - Profile pages show the avatar, name, @handle, bio, links, joined date and the blocks grid.
- **"Add to project":** keeps today's flow. The sanitized block goes straight into a chosen project after the existing review step (`GroupLibrary` → `AddToProject`), without saving a Library copy.
  - **"Save to Library"** is a separate action. It stores a copy through `PUT /api/group-blocks/{id}` with `source: {community_block, version}`.
  - Saving again updates that copy instead of duplicating it, and offers "update to v{n}" when a newer version exists.
- **Publish:** pick a group from the Library, preview it, choose the title, description, tags and license, then publish.
- **Remove LocalCommunity's "Service address" form and the loopback-only fragment transfer** (`groups/blockTransfer.ts` and `release/workspaceLink.ts` become dead code).

## Constraints

- Keep the AGENTS.md rules:
  - plain words (Community, People, Profile, Block; "group" stays the Library word)
  - status as shape plus text
  - errors in plain words through `errors.ts`
  - no third-party requests in routine tests
- Community keeps no access to runners, files, transcripts or Git, and the docs say so.
- **Docs to update:**
  - docs-site `guides/community.md` ("opens on its own site" changes)
  - `security.md` and `privacy.md` (identity is now the Sculptured account, profiles are public, and what each profile field exposes)
  - `sculptured-community/docs/OPERATIONS.md`
  - `sculptured-cloud/docs/DEPLOY.md`: one Google redirect URI fewer, plus the new route
- **Hosted-beta changes deploy from three repos:** keep each repo's tests green at every step. The gateway and Community changes must land together for `/api/community/*` routing.

## Decided by the builder (Claude's calls, flagged for review)

- Embedding in the gateway (Option C), not path-routing in Caddy. It gives one process for the signed assertion and one deploy.
- A device key with scope `community`, rather than reusing full computer keys. Least privilege.
- Avatars are stored in the Community Postgres, not object storage. It's small, has no new infrastructure, and is cacheable by ETag.
- Handle rules, reserved names and bio and link limits.
- The People directory lists only members who chose a handle.
- Handles are required to publish. Existing publishers get suggested handles, and examples use the reserved `sculptured`.
- "Add to project" keeps today's direct-to-project flow; "Save to Library" is separate and remembers the source and version.
- The old sign-in stays for one release plus 60 days, with a claim path for GitHub and password accounts.

## Decided by the user

- **Who can join:** anyone, with a Community-only account until invited to the hosted beta.
- **Code home:** `sculptured-community` stays a private repo, compiled into the private cloud gateway as a library.
  - There is no standalone or self-hosted Community mode; its OIDC is removed after the transition window.
  - TS types for Community endpoints are generated in `sculptured-community` with ts-rs and copied into `sculptured/web/src/generated/community/` by a small export step in that repo (documented, so the public repo's copies have one source).
- **Local default:** opt-in. Self-run installs contact no Sculptured server until the user turns Community on.

## Builder notes from the review

- The gateway `devices` table needs a `scope` column (`computer` | `community`), defaulting to `computer` for existing rows.
- The Community router's own Host/Origin guard and SPA catch-all must be bypassed or reconfigured when embedded. Mount only its `/api/community/*` routes under the gateway's guard; the app's SPA serves the pages.
- The gateway assertion carries more fields than `signing::sign` covers today. Extend the signed payload, or sign a canonical header set: user id, name, role, workspace flag, write-checked, client IP and timestamp, all of them covered by the MAC.
