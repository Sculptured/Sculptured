# Code health — platform, 2026-11-06

System 1 is in progress. These are measured findings, not a claim of completion.

## Checks

Run `npm --prefix web run lint`, `npm --prefix web test -- --maxWorkers=1`,
`npm --prefix web run duplication`, and `CARGO_BUILD_JOBS=2 cargo clippy -p controld --all-targets -- -D warnings`.
The duplication JSON is written to `target/code-health/jscpd-report.json`.
Generated bindings are excluded; tests and fixtures are included. No semantic/model scan is enabled.
The duplication budget is 3.69% of lines, rounded up from the initial 3.6871646%.

ESLint enforces 40 lines per function, excluding blank lines and comments, and complexity 12.
Tests retain complexity checks and are exempt only from function length. The existing 300-line
file limit and other lint rules remain enabled.

## Initial violations by module

| web/src module | Function length | Complexity | Other lint |
| --- | ---: | ---: | ---: |
| App.tsx | 0 | 1 | 0 |
| api.ts | 0 | 1 | 0 |
| arena | 0 | 2 | 0 |
| collaboration | 3 | 5 | 1 |
| control | 0 | 11 | 0 |
| conversation.ts | 1 | 1 | 0 |
| draft | 0 | 5 | 1 |
| errors.ts | 0 | 1 | 0 |
| groups | 1 | 5 | 0 |
| herdr.ts | 0 | 1 | 0 |
| hosted | 3 | 3 | 0 |
| pages | 2 | 8 | 0 |
| release | 4 | 3 | 0 |
| setup | 1 | 2 | 0 |
| shell | 24 | 26 | 8 |
| structure | 14 | 18 | 7 |
| tests (Arena UI) | 0 | 5 | 0 |
| useRigStream.ts | 0 | 1 | 0 |
| useSeatEvents.ts | 0 | 1 | 0 |
| useTeamDraft.ts | 1 | 1 | 0 |
| useTeams.ts | 0 | 2 | 0 |
| visual | 9 | 6 | 4 |
| workflow | 2 | 13 | 0 |
| workspace | 0 | 2 | 0 |
| **Total** | **65** | **124** | **21** |

Other findings: 13 file-length errors, two unused variables, one useless assignment,
one unnecessary escape, and four hook dependency warnings.

## Initial duplication

1,188 files, 186,539 lines, 712 clones, 6,878 duplicated lines (3.687%).
Paths below are relative to web/src or crates as indicated; line numbers describe the initial scan.

| Lines | First block | Second block | Review |
| ---: | --- | --- | --- |
| 48 | web/src/shell/edgeGeometry.ts:98 | web/src/shell/routeEdges.perf.test.ts:21 | Independent reference implementation for performance equivalence; preserve its oracle independence. |
| 42 | crates/model/src/safety-presets.json:1 | web/src/structure/safety-presets.json:1 | Shared policy data; investigate a single source. |
| 40 | crates/hostd/tests/policy.rs:430 | same file:491 | Repeated test setup. |
| 34 | crates/controld/tests/budget_pause.rs:26 | crates/controld/tests/run_budget.rs:44 | Candidate shared fixture. |
| 30 | web/src/tokens.css:346 | same file:377 | CSS fallback behavior needs preservation. |
| 29 | crates/controld/tests/handoffs.rs:1009 | same file:1351 and 2393 | Repeated scenario setup. |
| 29 | web/src/tokens.css:474 | same file:618 | Theme tokens; retain inherited-token semantics. |
| 28 | crates/controld/tests/handoffs.rs:3125 | same file:3222 | Repeated scenario setup. |

## Verification and remaining work

The preceding Control room commit passed TypeScript, changed-file ESLint, 139 targeted web tests,
six Arena proxy tests, formatting, and desktop/mobile layout checks with captures.
Clippy initially fails in `crates/model/src/mandate.rs`: `judge` is 32 lines against 30.
Full platform baseline tests and module refactors are pending.
