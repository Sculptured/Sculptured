# Community

Community (blocks, saves, profiles, reports) is its own service and database (`sculptured-community`). In the hosted app it is compiled into the gateway and served at `/api/community/*` for the signed-in Sculptured account. The web app's Community section lives in `web/src/community/`.

## Request path (hosted)

```
browser ──/api/community/*──> gateway (sculptured-cloud, community.rs)
                                 │ session lookup (no workspace is started)
                                 │ + Option<AssertedUser>{gateway_user_id, name, admin, workspace, write_checked, client_ip}
                                 │ + ClientIp; strips cookie, authorization, x-csrf-token
                                 ▼
                         sculptured_community::api_router ──> community-db
```

- `write_checked` is true for reads, and for writes that carry the session's CSRF token from our Origin. Community refuses other writes with `403 csrf`, and signed-out writes with `401 signed_out`.
- Any `Authorization` header is refused (`401 device_unknown`) until device keys carry a Community scope (the self-run app's pairing, not built yet).
- Without `COMMUNITY_DATABASE_URL` the routes answer `503 community_unavailable`.
- Rate limits: per user, per client address, per signed-out address, writes per member, profile edits per hour (`sculptured_community::Limits`).

## Accounts

A gateway user maps to `community_users(issuer = 'urn:sculptured:gateway', subject = <gateway user id>)`, created on first sight with the gateway name. An old Community account made with Google on the standalone site is relinked once (`link_legacy_google`) at the member's next Google sign-in and at each gateway start; its blocks, saves and handle are kept.

## Web

- `community/api.ts`: every call, at relative `/api/community/…` URLs. Errors use the coded shape `{"error":{"code","message"}}`; an old `{"error":"text"}` answer becomes `community_bad_answer`. Words for every code are in `errors.ts` (`COMMUNITY_ERROR_WORDS`).
- `community/routes.tsx`: hosted pages under `CommunityLayout` (tabs, your profile in `CommunityContext`). Signed out, `CommunityHost` renders a light frame instead of the app shell, and `HostedGate` lets `/community*` through. The self-run app still renders `release/Community`.
- Block kinds: a block without `kind` is a group; other kinds show "Opens in a later version."
- Library copies saved from Community carry `GroupBlock.source = {community_block, version}`, which drives **Save to Library** / **In your Library** / **Update to v{n}** and the Library card's "From Community" link. Export and publish drop it (`sanitizeExport`).
- Types in `web/src/generated/community/` come from the community repo: `cargo test export_bindings` there writes them here (its `.cargo/config.toml` sets the path).

## Tests

- Community: `cargo test` in `sculptured-community` with a disposable Postgres (see its README).
- Gateway: `tests/community.rs` and `tests/oidc.rs` in `sculptured-cloud`.
- Web: `web/src/community/*.test.ts(x)`, which use `fakeFetch` and `renderCommunity` from `community/testing.tsx`.
