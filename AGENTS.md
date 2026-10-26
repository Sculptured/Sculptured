# Sculptured — workspace instructions

## Product
Sculptured runs and supervises local Claude Code, Codex and OpenCode agents. Users design a team on a
board, start it, and watch and talk to it live. Internally it follows the OpenRig RigSpec model
(rig → pod → seat, typed edges). It owns its runtime: `sculptured serve` (controld) and
`sculptured hostd` (the agent runner). How it fits together is in `docs/internals/`, starting
with `docs/internals/overview.md`.

Agents are the primary way users work: compose their roles and communication, inspect
intermediate results, and steer a team toward a shared goal. Center new features on this
ongoing collaboration. Routing and workflow automation support that experience.

## Stack
- Backend and all tooling are Rust (edition 2024). The browser UI is React + TypeScript.
- No shell or Python scripts. Tooling that would usually be a script is a small workspace crate
  (for example `crates/e2e-server`, the Playwright web server). Invoking JS tooling such as
  `npm run build` from Rust is fine.
- TypeScript API types are generated from Rust into `web/src/generated/` by ts-rs. Never hand-edit
  them; run `cargo test --workspace export_bindings` to regenerate.

## Rust code quality
- Follow the [Google Rust Style Guide](https://google.github.io/styleguide/rustguide.html),
  applying its API guidance in the context of this Cargo application. Chromium-specific build
  and test conventions do not apply here.
- Keep modules focused on one responsibility. Put implementation in leaf modules and preserve
  public import paths with re-exports when splitting a module.
- Keep functions focused and reasonably small. The existing 30-line Clippy threshold is advisory,
  not a reason to split coherent code. Extract helpers around meaningful responsibilities, and share
  repeated logic without introducing speculative abstractions.
- Prefer borrowing to unnecessary deep clones, named results to ambiguous tuples, and pattern
  matching to check-then-unwrap. Use `expect` with an invariant explanation when a panic is required.
- Run `cargo fmt --all -- --check`, relevant behavior tests, and Clippy for changed crates.
  Do not suppress warnings or raise thresholds just to make a refactor pass.

## Visualizations are the primary interface
- The team board is the entry point. Clicking, tapping or pressing Enter on an agent opens it.
  Keep raw events available as evidence next to the visual summaries.
- The default UI speaks plain words: Team, Group, Agent, "hands work to", Job, Folder.
  OpenRig words (rig, pod, seat, member, edge, runtime, RigSpec, YAML) appear only inside an Advanced
  disclosure (`data-advanced`). UI tests check this with `web/src/test-vocab.ts`; keep words in `web/src/vocab.ts`.
- Server errors are shown in plain words (`web/src/errors.ts`), never as the server's own text.
  Runtime reasons (lifecycle reasons, a host's reason) are evidence: shown verbatim and marked
  `data-raw-evidence`.
- Every drag has a keyboard path, and team positions persist: Structure and Live show agents at the same place.
- Keep these distinct: planned, in-progress, reported-complete, verified and blocked.
  An agent saying "done", a tool succeeding, or a file existing is never proof of correctness.
- Label provenance: protocol-observed, agent-reported, filesystem-observed, captured-tool,
  verified, heuristic or operator. Missing information stays "unknown". Never invent
  architecture, completion, usage or verification.
- Layout is stable. Updates never move cards or reset selection, viewport or filters.
  Connection labels never sit on a line or a card (`web/src/shell/labels.ts`).
- Show status as shape plus text, never colour alone. Disabled controls say why in visible text.
  Provide a list/text fallback, keyboard-operable controls, and a mobile layout that exposes the
  same evidence.
- Restrict source inspection to allowed workspace paths; the folder picker is rooted at home.
  An incomplete or invalid agent report never interrupts the agent.

## Testing
- Routine tests never make real model calls or third-party requests. Use `crates/fixtures`
  (fake CLIs) and the stub driver. Playwright uses Practice agents (`runtime: stub`) only, against
  the real binary started by `crates/e2e-server` with a throwaway data dir and `HOME`.
- Add behaviour tests for meaningful visual interactions, report validation and source
  boundaries.
- Visual changes are checked with real captures of the running app; report any check you could
  not run.
- User guides and video tutorials must be recordings of the real application. Never use
  illustrated, mocked, reconstructed or simulated screens.
