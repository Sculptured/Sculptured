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

## After the refactor

The web lint went from 210 findings (65 function-length, 124 complexity, 21 other) to 21
(3 function-length, 10 complexity, 5 other). All findings in non-test code are in one component,
`web/src/structure/StructureCanvas.tsx` (3 function-length, 6 complexity, 1 file-length); the other 8 are in
test files.

| web/src module | Function length (before / after) | Complexity (before / after) | Other lint (before / after) |
| --- | ---: | ---: | ---: |
| App.tsx, api.ts, arena, control, herdr, errors, conversation.ts, hooks | 1 / 0 | 22 / 0 | 0 / 0 |
| collaboration | 3 / 0 | 5 / 0 | 1 / 0 |
| draft | 0 / 0 | 5 / 2 (tests) | 1 / 1 (test file length) |
| groups | 1 / 0 | 5 / 0 | 0 / 0 |
| hosted | 3 / 0 | 3 / 0 | 0 / 0 |
| pages | 2 / 0 | 8 / 0 | 0 / 0 |
| release | 4 / 0 | 3 / 0 | 0 / 0 |
| setup | 1 / 0 | 2 / 0 | 0 / 0 |
| shell | 24 / 0 | 26 / 2 (perf test) | 8 / 1 (test file length) |
| structure | 14 / 3 | 18 / 6 | 7 / 3 |
| tests (Arena UI) | 0 / 0 | 5 / 0 | 0 / 0 |
| useTeamDraft.ts | 1 / 0 | 1 / 0 | 0 / 0 |
| visual | 9 / 0 | 6 / 0 | 4 / 0 |
| workflow | 2 / 0 | 13 / 0 | 0 / 0 |
| workspace | 0 / 0 | 2 / 0 | 0 / 0 |
| **Total** | **65 / 3** | **124 / 10** | **21 / 5** |

Duplication is 3.63% of lines (6,858 of 189,000; 714 clones), under the 3.69% budget and below the
initial 3.687%. In the Rust crates `jscpd` reports 5.0%, almost all in tests and fixtures.

Shared helpers added instead of copies: `closeOnEscape` (six panels), `Composer`, `AgentBox`, the live
graph's geometry, pulse and layer modules, `transcriptModel`, `TemplatePreview`, `ImportFromFile`,
`removal.ts` (what is left of a draft when agents go), and `process::stop_session` /
`hard_stop_session` for the Codex and OpenCode drivers.

## What remains

- `web/src/structure/StructureCanvas.tsx`: one 824-line component with about 40 closures over shared drag,
  wire, ghost and selection state. It needs its pointer handling split into hooks that share one context
  object (card, group, port, marquee, palette) and its render split into layers, as was done for
  `LiveGraph`. It was left because it cannot be refactored safely without browser-level checks of drag and
  keyboard behaviour; it is the only non-test violation.
- Test files over 300 lines: `structure/StructureBoard.test.tsx` (701), `structure/TeamFlow.test.tsx` (531),
  `draft/edits.invariants.test.ts` (441), `shell/Transcript.test.tsx` (398). Split each by describe block.
- Test helpers over complexity 12: `draft/edits.invariants.test.ts` (two) and
  `shell/routeEdges.perf.test.ts` (two; its reference router is a deliberately independent oracle).
- `shell/routeEdges.perf.test.ts` has a wall-clock budget: it can fail when the full suite shares a busy
  machine (it passes alone and in a quiet run).
- Rust: `cargo clippy --workspace --all-targets -- -D warnings` still fails on `too_many_lines` in
  `drivers` (13 in the library), `rigspec` (18), `fixtures` (2), and in tests of `model`, `host-proto` and
  `interpreter`. The `mandate.rs` failure is fixed. `controld` and crates after the failing ones were not
  linted because `--keep-going` stops at a failing dependency.
- Rust duplication worth consolidating next: the Codex and OpenCode session start-up (about 15 lines in
  `drivers/src/{codex,opencode}/session.rs`), `portable/convert/{claude,codex}.rs`, and the repeated test
  setup in `controld/tests` and `workflow/tests.rs`.

## Verification

Each commit passed `tsc --noEmit`, ESLint on the files it touched, and the vitest files for the modules it
changed (one worker). The drivers crate tests pass after the stop-sequence change.
