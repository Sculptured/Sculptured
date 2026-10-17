# Release verification — 17 October 2026

Scope: the Sculptured core at `96e9000` (with the Community repo at `8c2e386` and the cloud repo at `9b8642c`), and the documentation commit that follows it. This records what was checked and what was not; it is not a security audit.

## What changed since the last verified release

Knowledge (a generated project map, `search_history`, `record_lesson`, constraints, lessons kept with the project), agent skills, Kindex and Signet-Eval as per-agent tools, safety rules, review-panel angles, shared values in contracts and claim checks. See `CHANGELOG.md`.

## Passing checks

| Check | Result |
|---|---|
| `cargo test --workspace` | 1,131 passed, after regenerating the published team-file schema (`UPDATE_SCHEMAS=1`) |
| `npm --prefix web test`, on the committed code alone | 793 passed, 1 skipped |
| `npm --prefix docs-site test` and build | 37 pages build; build test passes |
| Internal links and anchors (docs site and internals) | All resolve |
| `cargo clippy` on new files | No new warnings beyond the repository's existing length lints |
| Community repo `cargo check` against the new types | Passes |
| Live run with Claude Haiku (8 short turns, local test server) | The map was built and used; `search_history` found a teammate's work; a lesson reached a running teammate after the fix this run found |

## Not checked

- **Skills, Kindex and Signet-Eval with real agents.** Covered by fixture-CLI tests only. `claude plugin details` confirmed a per-agent plugin folder loads its skill. Kindex (`kin-mcp`) and `signet-eval` were not installed here, so neither ran for real.
- **Claim checks after a real turn.** The comparison is unit-tested; the watcher that runs it after each turn has no end-to-end test.
- **Real browser checks** of the new panels (Skills, Safety rules, review angles, shared values). Component tests only.
- **Runners in the field** need reinstalling to get skills, integrations, shared-value checks and kept lessons; until then those agents start without them and their pages say so.
- **Linux arm64 and Windows on Arm** have no runner builds (see [Releasing](RELEASING.md)).

## Known issues

- One web test (`StructureBoard` › *keeps the board mounted…*) fails under full-suite load and passes alone.
- Repository ESLint fails on existing function and file length limits; this is not a clean-lint release.
- Another editor's UI work (design tokens, a command palette and more) is uncommitted in the working tree and is not part of this release.
