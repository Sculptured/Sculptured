# Control workspace and Arena control: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A dockable, mission-control-style workspace. It is Sculptured's Control page for real and simulated deployments, and the Arena control page that replaces the Tests section.

**Architecture:** One in-house docking engine sits in `web/src/workspace/`. It has a pure layout-tree model, persistence, pane registry, selection bus and React views. Two pages register panes: `web/src/control/` and `web/src/arena/`. Panes read a `ControlSource` interface, which has a deployment implementation (the shell stream plus controld routes) and a simulated one (`/api/arena/*`). The backend gets small controld routes for pause and resume, and arenad routes for world, usage and gc.

**Tech Stack:**
- React 19, TypeScript 7, react-router 7, vitest plus testing-library, Playwright.
- Rust: axum in controld and arenad, ts-rs.

**Spec:** `docs/design/2026-11-02-control-workspace-design.md`. Read it; it is binding.

## Global Constraints

- **Where the work happens:**
  - Sculptured work is in the worktree `/home/alexsky2/MLSys/Sculptured/sculptured-control`, branch `control-workspace`.
  - Arena work is in `/home/alexsky2/MLSys/Sculptured/arena`, branch `integrations`.
- **Rules:**
  - `AGENTS.md` and `DESIGN.md` hold.
  - No raw hex outside `web/src/tokens.css`.
  - Plain words: OpenRig words only inside `data-advanced`, and words kept in `web/src/vocab.ts`.
  - Server errors go through `web/src/errors.ts`, and raw evidence carries `data-raw-evidence`.
  - Status is a glyph plus a word.
  - Disabled controls say why in visible text.
  - Every drag has a keyboard path.
- **Web lint:** every new or changed web file passes `cd web && lint/node_modules/.bin/eslint --max-warnings 0 <files>`: 30 lines per function and 300 per file, with no suppressions. (The whole repo does not pass lint today, so lint only your files.)
- **No new runtime npm dependency.**
- **Rust:**
  - `cargo fmt --all -- --check`, Clippy with no warnings for the changed crates, and behaviour tests.
  - Regenerate TS types with `cargo test --workspace export_bindings`; never hand-edit `web/src/generated/`.
- **Disk** (about 60 GB free at start):
  - Build with `CARGO_INCREMENTAL=0`.
  - Keep at least 20 GB free. Check with `df -h /` before a cargo build.
  - Delete only your own build output.
- **Never touch:**
  - the user's Sculptured on :4410, its data in `~/.sculptured`, or its runners;
  - the user's arenad;
  - other Docker containers or volumes;
  - `Sculptured-Examples`;
  - other worktrees.
- **Tests:**
  - never call real models or third parties;
  - Playwright uses Practice agents only;
  - Arena web tests mock `/api/arena/*`.
- **Commits:** on the branch named above, with explicit paths only, in the repository's message style ("Control: …", "Arena: …").

## Review Focus

1. **A saved layout from an older version, or one naming a pane type that no longer exists.** It must load the defaults or drop the unknown pane, never crash or blank the page. Owner: Task 1, `loads_with_unknown_pane_dropped` and `corrupt_value_falls_back`.
2. **Dropping a stack's only pane onto that same stack** (an edge or the centre). It must do nothing. Owner: Task 1, `move_onto_itself_is_noop`.
3. **A live stream update while the user is dragging or has scrolled a pane.** The layout and scroll must not jump. Owner: Task 2, `stream_update_keeps_drag_and_scroll`.
4. **Emergency stop pressed with no agents running, or pressed twice.** It must say "nothing running", or stay disabled, never error. Owner: Task 6, `emergency_stop_confirm_and_idle`.
5. **Arena not running or unreachable.** The Arena control panes and the simulated Control panes show a plain "Arena is not running" message with how to start it, never a blank pane or raw error. Owner: Task 9, `arena_down_is_plain`.

---

### Task 1: Layout model and persistence (pure TypeScript)

**Files:**
- Create: `web/src/workspace/model.ts`, `web/src/workspace/store.ts`
- Test: `web/src/workspace/model.test.ts`, `web/src/workspace/store.test.ts`

**Interfaces (produces):**
```ts
export type PaneRef = { id: string; type: string; settings?: Record<string, unknown> };
export type Stack = { kind: 'stack'; id: string; panes: PaneRef[]; active: number };
export type Split = { kind: 'split'; id: string; dir: 'row' | 'col'; kids: Node[]; sizes: number[] };
export type Node = Stack | Split;
export type Zone = 'left' | 'right' | 'top' | 'bottom' | 'center';
export type Layout = { id: string; name: string; root: Node; popped: PaneRef[] };
export type PageLayouts = { version: 1; active: string; layouts: Layout[] };
export function movePane(root: Node, paneId: string, targetStackId: string, zone: Zone): Node;
export function closePane(root: Node, paneId: string): Node;
export function addPane(root: Node, pane: PaneRef, targetStackId?: string): Node; // centre of target, else first stack
export function duplicatePane(root: Node, paneId: string, newId: string): Node;   // as a tab beside it
export function resize(root: Node, splitId: string, index: number, delta: number): Node; // delta in size units, min 0.15
export function setActive(root: Node, stackId: string, index: number): Node;
export function stacks(root: Node): Stack[];
export function panes(root: Node): PaneRef[];
export function dropUnknown(root: Node, known: (type: string) => boolean): Node;
// store.ts
export function loadLayouts(page: string, defaults: () => PageLayouts, known: (type: string) => boolean): PageLayouts;
export function saveLayouts(page: string, value: PageLayouts): void; // key `sculptured.workspace.v1.<page>`
```
All functions are immutable: they return a new tree and never mutate their input. Ids come from a counter passed in or from `crypto.randomUUID()`. An empty tree becomes a single empty stack.

- [ ] Step 1. Write the failing tests:
  - `move_to_edge_splits`: moving to `right` of a stack in a column wraps it in a row with the new stack second.
  - `move_into_same_direction_split_inserts_sibling`: moving to `right` of a stack that is already in a row inserts a sibling, splitting that stack's size in half.
  - `move_center_joins_tabs_and_activates`
  - `move_onto_itself_is_noop`: covers a stack's only pane onto that stack at any zone, and the centre of the stack it is already in.
  - `close_prunes_empty_stack_and_single_child_split`
  - `resize_clamps_min`
  - `duplicate_adds_tab_with_new_id`
  - `inputs_not_mutated`
  - `loads_with_unknown_pane_dropped`
  - `corrupt_value_falls_back`: covers bad JSON, a wrong version and a missing `active`.
  - `save_then_load_roundtrip_per_page`: two pages don't clobber each other.
- [ ] Step 2. Run `cd web && npx vitest run src/workspace`; it fails.
- [ ] Step 3. Implement.
- [ ] Step 4. The tests pass and lint is clean on the new files.
- [ ] Step 5. Before the first run, `npm ci` in `web` (and in `web/lint` if its `node_modules` is missing). Commit "Control: workspace layout model and persistence".

### Task 2: Docking views

**Files:**
- Create: `web/src/workspace/{Workspace.tsx,StackView.tsx,Splitter.tsx,DropOverlay.tsx,PaneMenu.tsx,registry.ts,useDrag.ts,workspace.css}`
- Modify: `web/src/main.tsx` (import `workspace.css` after `pages.css`)
- Test: `web/src/workspace/Workspace.test.tsx`

**Interfaces:**
- Consumes: Task 1.
- Produces:
```ts
export type PaneProps<S = Record<string, unknown>> = { instance: string; settings: S; setSettings(s: S): void };
export type PaneType = {
  type: string; title: string; glyph: string; multi?: boolean;
  appliesTo?: (ctx: unknown) => boolean;          // page-specific context; default: always
  defaultSettings?: Record<string, unknown>;
  Settings?: React.FC<PaneProps>; Body: React.FC<PaneProps>;
};
export function createRegistry(types: PaneType[]): { get(type: string): PaneType | undefined; list(ctx?: unknown): PaneType[] };
export function Workspace(props: { root: Node; registry: Registry; ctx?: unknown; onChange(root: Node): void;
  onPopOut?(paneId: string): void }): JSX.Element;
```

Behaviour (pointer events, not HTML5 drag-and-drop; follow the pointer style of `shell/LiveGraph.tsx`):
- **Drag a tab:**
  - A 4 px threshold starts a drag.
  - The stack under the pointer shows a drop overlay for the zone. The edges are 25%; the rest is the centre.
  - Releasing calls `movePane`, and Escape cancels.
- **Tab strip:** clicking a tab activates it, ✕ closes it, and a `⋯` button opens the PaneMenu with these items:
  - "Move to…": a submenu with every other stack by its active pane's title × Left/Right/Above/Below/As a tab;
  - Duplicate (only if `multi`);
  - Settings (only if the type has a Settings form; it opens inline above the body);
  - Pop out (only if `onPopOut`);
  - Close.
  - The menu is fully keyboard-operable (arrow keys, Enter, Escape).
- **Splitter:**
  - It is `role="separator"` with `aria-orientation`, and focusable.
  - Pointer drag resizes it.
  - ArrowLeft/Right (or Up/Down) resizes by 0.05, using the `resize` model function.
- **Settings:** a pane's settings live in its `PaneRef.settings` and are saved with the layout.
- **Stability:** an empty stack says "Empty. Use Add pane." Updates from outside don't remount pane bodies. Key them by pane id, so scroll position and internal state survive moves elsewhere in the tree.

Steps:
- [ ] Step 1. Write the failing tests:
  - `drag_tab_to_edge_splits` (pointer events with `getBoundingClientRect` stubbed)
  - `escape_cancels_drag`
  - `menu_move_to_as_tab_and_left`: the keyboard path.
  - `splitter_keyboard_resizes`
  - `close_and_settings_and_duplicate`
  - `stream_update_keeps_drag_and_scroll`: rerender with a new `ctx` mid-drag. The drag continues, and the pane body element is the same node.
  - `empty_stack_message`
- [ ] Step 2. They fail.
- [ ] Step 3. Implement. `workspace.css` uses tokens only. Pane title tabs are small uppercase labels; the active tab has an accent inset line, as in the agreed mock. Tabular numbers throughout.
- [ ] Step 4. The tests pass and lint is clean.
- [ ] Step 5. Commit "Control: docking views (drag, keyboard move, splitters)".

### Task 3: Workspace shell (layout tabs, Add pane, selection, pop-out, mobile, Mission look)

**Files:**
- Create: `web/src/workspace/{WorkspacePage.tsx,LayoutTabs.tsx,AddPane.tsx,selection.tsx,PopOutPane.tsx,MobileWorkspace.tsx}`
- Modify:
  - `web/src/tokens.css`: add the Mission token set at `:root[data-look='mission'], .workspace[data-look='mission']`;
  - `web/src/appearance.ts` and its Appearance UI: add `mission` to `LOOKS`. It is dark like Coastal, so `resolvedScheme` returns dark;
  - `DESIGN.md`: a "Mission look" section and its tokens in the front matter.
- Test: `web/src/workspace/WorkspacePage.test.tsx`, `web/src/appearance.test.ts` (extend it if it exists)

**Interfaces:**
- Consumes: Tasks 1 and 2.
- Produces:
```ts
export function WorkspacePage(props: { page: string; registry: Registry; ctx: unknown;
  defaults: () => PageLayouts; header?: React.ReactNode; paneRoute(instance: string): string }): JSX.Element;
export type Selection = { agent?: string; time?: number; key?: string };
export function useSelection(): { selection: Selection; select(s: Partial<Selection>): void };
export function PopOutPane(props: { page: string; registry: Registry; ctx: unknown; instance: string }): JSX.Element;
```

Behaviour:
- **Layout tabs** across the top: switch layouts; "+" adds a copy of the first default; double-click or the menu renames. The menu also duplicates, deletes (not the last one) and resets to default.
- **Add pane:** lists `registry.list(ctx)`; a type already shown and not `multi` says "shown" and focuses it when chosen. It adds the new pane to the focused stack, or else the first stack.
- **Workspace setting "Look":** Mission (default) or Follow app look. The value is saved in the page's layouts, and the root's `data-look` is set accordingly.
- **Selection:**
  - held in context;
  - mirrored over `BroadcastChannel('sculptured.workspace.<page>')`;
  - panes whose settings have `follow: false` ignore it. `follow` defaults to true for every pane type that reads the selection.
- **Pop out:**
  - opens `window.open(paneRoute(id))`;
  - moves the pane to `layout.popped`, and its slot shows "Shown in another window" with a "Bring back" button;
  - on unload, the popped window posts a "back" message on the channel, which returns the pane.
- **Mobile:** below `720px` (the app's breakpoint), render `MobileWorkspace`: one pane at a time, with a pane switcher listing the active layout's panes.

Steps:
- [ ] Step 1. Write the failing tests:
  - `layout_tabs_add_rename_duplicate_delete_reset`
  - `add_pane_marks_shown_and_focuses`
  - `layouts_persist_across_remount`
  - `selection_shared_between_two_workspaces`: two `WorkspacePage` instances with a fake `BroadcastChannel`.
  - `popout_moves_and_bring_back`
  - `mobile_shows_one_pane_with_switcher`: stub `matchMedia`.
  - `look_setting_sets_data_look`
  - `appearance_accepts_mission`
- [ ] Step 2. They fail.
- [ ] Step 3. Implement. Choose the Mission palette:
  - near-black background (around `#0a0f14`), panels slightly lighter, hairline lines;
  - cyan accent (around `#3fd0ff`);
  - text around `#c9d6e0`;
  - status hues that stay distinct, with contrast ≥ 4.5:1 for text.
  - Define every token the other looks define.
- [ ] Step 4. The tests pass and lint is clean.
- [ ] Step 5. Commit "Control: workspace shell, Mission look".

### Task 4: controld pause and resume (Rust)

**Files:**
- Modify: `crates/controld/src/collaboration/controls.rs` (and the route module that registers `/api/seats/{id}/control` and `stop-now`).
- Create: route handlers for `POST /api/teams/{id}/pause` and `POST /api/teams/{id}/resume`, in the same module as `stop-now` or a sibling leaf module.
- Test: `crates/controld/tests/pause_resume.rs`

**Interfaces (produces):**
- `POST /api/seats/{id}/control` with `{mode: "pause"}` (new) and `{mode: "resume"}` (existing). "pause" maps to `Control::Checkpoint` if that keeps the session and resumes it; otherwise it maps to soft. Read `budget/pause.rs` and `controls.rs`, decide, and write the reason in the report.
- `POST /api/teams/{id}/pause` and `/resume` take a body `{request_id}` and are idempotent like the other POSTs. The response is `TeamControlResult { affected: Vec<String>, skipped: Vec<SkippedSeat { seat, reason }> }`, exported by ts-rs.

Steps:
- [ ] Step 1. Write failing behaviour tests with the `common::m2::serve()` harness and the stub driver:
  - `pause_agent_then_resume_continues_session`
  - `team_pause_affects_all_live_agents_and_skips_stopped`
  - `team_resume_restarts_paused`
  - `pause_when_nothing_running_is_empty_result_not_error`
- [ ] Step 2. They fail.
- [ ] Step 3. Implement, and add an error code to `web/src/errors.ts` if a new one is needed.
- [ ] Step 4. Run the tests, then `cargo fmt --all -- --check`, `cargo clippy -p controld --all-targets` and `cargo test --workspace export_bindings`. Add the web API functions `api.pauseTeam`, `api.resumeTeam` and `api.controlSeat(seat, mode)` in `web/src/api.ts`, with a vitest in `api.test.ts`.
- [ ] Step 5. Commit "Control: pause and resume agents and teams".

### Task 5: ControlSource and the deployment source

**Files:**
- Create: `web/src/control/source.ts` (the interface), `web/src/control/deploymentSource.ts` (`useDeploymentSource(team)`), and `web/src/control/alarms.ts` (the pure rule engine)
- Test: `web/src/control/deploymentSource.test.tsx`, `web/src/control/alarms.test.ts`

**Interfaces (produces):**
```ts
export type Availability = { ok: true } | { ok: false; why: string };
export type AgentLine = { id: string; name: string; status: { glyph: string; word: string; tone: 'ok'|'warn'|'bad'|'idle'|'unknown' };
  activity?: string; tokens?: { in: number; out: number } };
export type FeedItem = { id: string; at: number; agent?: string; kind: string; text: string; raw: unknown; alarm?: string };
export type Approval = { id: string; agent?: string; title: string; detail?: string; kind: 'change'|'question' };
export type ControlSource = {
  kind: 'deployment' | 'simulated'; title: string; running: boolean; startedAt?: number;
  agents: AgentLine[]; feed: FeedItem[]; approvals: Approval[];
  decide(id: string, d: { allow: true } | { allow: false; reason: string }): Promise<void>;
  agentControl(agent: string, action: 'pause'|'resume'|'stop'): { can: Availability; run(): Promise<void> };
  message(agent: string, text: string): { can: Availability; run(): Promise<void> };
  team: Record<'start'|'pauseAll'|'resumeAll'|'emergencyStop', { can: Availability; run(): Promise<void> }>;
  metrics: { tokensIn?: number; tokensOut?: number; costUsd?: number; toolCallsPerMin: number[] };
  timeline?: SimulatedTimeline;                          // simulated only, Task 8
  world?: { rows(key?: string): Promise<Record<string, unknown>> }; // simulated only
};
// alarms.ts
export type AlarmRule = { id: string; on: boolean; threshold?: number };
export const DEFAULT_RULES: AlarmRule[]; // failed, attention, stall, context-ceiling, limit, question-waiting (threshold 300 s), part-error
export function alarmsFrom(feed: FeedItem[], agents: AgentLine[], rules: AlarmRule[], now: number): Alarm[];
export type Alarm = { id: string; rule: string; at: number; agent?: string; text: string };
```

How the deployment source is built:
- From `useShell().teams[team]` (`RigState`), `useTeamEvents`, `api.teamQuestions` (approvals are questions; a question whose `source === 'approval'` is a change) and `api.answer`.
- It uses `progress/activity.ts describeActivity` for the feed text.
- Usage and budget come from `api.teamUsage` / `api.teamBudget` through `usePolled`. Tool calls per minute come from tool-called events in the last 15 minutes.
- Controls use Task 4's routes, plus `stop-now` for the emergency stop, `/send` for messages and the team start route.
- An action that can't run says why (for example, "Nothing is running", "Already paused").

Steps:
- [ ] Step 1. Write the failing tests:
  - `deployment_source_maps_agents_and_feed`: use `test-fixtures` (`openFake`, `seatView`, `ev`).
  - `approvals_from_questions_and_decide_calls_answer`
  - `emergency_stop_unavailable_when_nothing_running`
  - `alarm_rules_each_fire_and_can_be_switched_off`
  - `question_waiting_threshold`
- [ ] Step 2. They fail.
- [ ] Step 3. Implement.
- [ ] Step 4. The tests pass and lint is clean.
- [ ] Step 5. Commit "Control: control source for deployments, alarm rules".

### Task 6: Control page and its panes

**Files:**
- Create:
  - `web/src/control/ControlPage.tsx`, `ControlHeader.tsx`, `registry.ts`, `defaults.ts`;
  - `web/src/control/panes/{Overview,Status,Timeline,Alarms,Approvals,AgentDetail,Metrics,RawEvents,Results}.tsx`.
- Modify:
  - `web/src/App.tsx`: the `/p/:team` index goes to `control`; add `control` and `control/pane/:instance`.
  - `web/src/pages/ProjectBar.tsx`: a "Control" tab first.
  - `web/src/paths.ts`.
  - `AGENTS.md`: the entry-point line, so the board is the first pane of Control's default layout.
- Test: `web/src/control/ControlPage.test.tsx`, plus `web/src/pages/Pages.test.tsx` for the index redirect.

**Interfaces:**
- Consumes: Tasks 3 and 5.
- Produces: `controlRegistry` (pane types `overview`, `status`, `timeline`, `alarms`, `approvals`, `agent`, `metrics`, `raw`, `results`, and later `world` and `compare`) and `controlDefaults(kind)`.

Panes, as in the spec's "Control panes":
- **Overview** reuses `shell/TeamLive` for a deployment. Lift any `useSearchParams` it needs into props, or wrap it so it works inside a pane.
- **AgentDetail** reuses `AgentActivity` and `AgentConversation`, with `multi: true`. Its agent comes from the selection unless "follow" is off, in which case it uses a picked agent. It shows the agent controls.
- **Timeline** draws one lane per agent from the feed, plus an alarms lane, and puts the cursor at the selection's time. Clicking selects a time and an agent. The lanes to show are chosen in Settings.
- **Status** shows the feed with kind filters in Settings, and highlights alarms.
- **Alarms** lists alarms; Acknowledge stores the alarm id in localStorage `sculptured.alarms.ack.<team>`; the rule switches and thresholds are in Settings.
- **Approvals** shows Approve, and Reject with a reason field.
- **Metrics** shows numbers plus a small bar sparkline (tokens use tabular numbers) and `api.machineMetrics()`.
- **RawEvents** is a team-wide table (adapt `EventsTab`) marked `data-raw-evidence`.
- **Results** has a section picker in Settings and renders the existing Results section components.

ControlHeader:
- the run status (a glyph plus a word);
- the elapsed clock;
- the master caution: a count of unacknowledged alarms that opens or focuses the Alarms pane;
- Start, Pause all / Resume all;
- Emergency stop, which opens a confirm popover: "Stop all N agents now?" with Stop now and Cancel.

Default layouts:
- **Live:** a col split [row [stack overview+raw, stack status+approvals+alarms], stack timeline] with sizes [2.2, 1] and row sizes [1.6, 1].
- **Agent focus:** a row split [agent, col [timeline, raw]].
- **Cost:** a row split [metrics, results].

Steps:
- [ ] Step 1. Write the failing tests (mock the source):
  - `project_index_opens_control`
  - `default_live_layout_shows_board_first`
  - `selecting_agent_in_overview_updates_agent_detail_and_timeline`
  - `approve_and_reject_with_reason`
  - `emergency_stop_confirm_and_idle`: covers confirm, cancel, the disabled state with its visible reason when idle, and a double press sending one request.
  - `alarm_ack_persists`
  - `pane_route_renders_single_pane`
  - `no_openrig_words`: use `test-vocab.ts`.
- [ ] Step 2. They fail.
- [ ] Step 3. Implement.
- [ ] Step 4. Pass `npm test` for the touched areas, `npm run build`, and lint on the new and changed files.
- [ ] Step 5. Commit "Control: Control page, panes and header controls".

### Task 7: arenad world, usage and gc routes (arena repo)

**Files** (in `/home/alexsky2/MLSys/Sculptured/arena/core/crates/arenad/src/`):
- Modify: `api.rs` to register the routes.
- Create: a leaf module for the three handlers (for example, `api/world.rs`). Use the existing `saves.rs` (`integrations`, `world_of`, `usage`) and `gc.rs` (`usage`, `run_startup_gc`).
- Test: unit tests next to the handlers, in the same style as the existing api tests.

**Interfaces (produces):**
- `GET /api/world` returns `{ parts: [{ name, integration, version, restore: {exact, approximate}, uses_clock, tools, answered: bool, error?: string }] }`.
- `GET /api/usage` returns `{ parts: [{ name, bytes }], total_bytes, limit_bytes }`.
- `POST /api/gc` returns `{ removed: [String], freed_bytes }`. It must never remove saves that a run still references; reuse the startup gc's keep set.

Steps:
- [ ] Step 1. Write the failing tests:
  - `world_lists_parts_with_restore`
  - `world_marks_unanswering_part`
  - `usage_sums_parts`
  - `gc_keeps_referenced_saves`
- [ ] Step 2. They fail.
- [ ] Step 3. Implement.
- [ ] Step 4. Run `CARGO_INCREMENTAL=0 cargo test -p arenad` in `arena/core`, then fmt and Clippy. Update the HTTP route list in `arena/README.md` (or `docs/protocol.md` if routes are listed there).
- [ ] Step 5. Commit on `integrations`: "arenad: world, usage and gc routes".

### Task 8: Simulated source and the simulated Control workspace

**Files:**
- Create: `web/src/control/simulatedSource.ts` (`useSimulatedSource(run, branch)`) and `web/src/control/panes/{World,Compare}.tsx`
- Modify:
  - `web/src/control/panes/Timeline.tsx`: the simulated controls;
  - `web/src/control/registry.ts` and `defaults.ts`: the Replay layout;
  - `web/src/tests/api.ts`: move it to `web/src/arena/api.ts` if Task 9 is easier that way, but do it in this task if you move it, and keep the re-export.
- Test: `web/src/control/simulatedSource.test.tsx`, `web/src/control/SimulatedControl.test.tsx`

**Interfaces:**
- Consumes: the `arena.*` API (`branch`, `follow`, `rows`, `fork`, `restorePlan`, `compare`), `tests/model.ts` `layout()`, and `tests/restore.ts`.
- Produces: `SimulatedTimeline = { steps: Step[]; saves: number[]; cursor: number; setCursor(seq: number): void; plan(seq: number): Promise<RestorePlan>; fork(seq: number, changes: unknown, restore: 'exact'|'approximate'): Promise<{ run: string; branch: string }> }`.

Behaviour:
- Agents and feed come from the branch's events. Approvals come from world events, if the `sculptured` integration is in the world; otherwise there are none.
- Pause, resume and stop are unavailable, with the reason "Arena runs can't be paused or stopped from here yet". Emergency stop is likewise unavailable, with a reason.
- On Timeline:
  - ←/→ and j/k step through steps, as RunPage does today;
  - "Fork from here" reuses `RerunPanel` and `RestoreChoice`, showing the per-part plan with "Approximate restore" labels;
  - a successful fork navigates to the new branch's workspace.
- **World** shows the rows at the cursor's save. The key comes from the selection, if any.
- **Compare** shows this branch against a chosen one (reuse the ComparePage pieces).

Steps:
- [ ] Step 1. Write the failing tests (mock `/api/arena/*`):
  - `simulated_source_maps_events`
  - `controls_unavailable_with_reason`
  - `timeline_steps_with_keys`
  - `fork_shows_plan_and_navigates`
  - `world_rows_at_cursor`
- [ ] Step 2. They fail.
- [ ] Step 3. Implement.
- [ ] Step 4. The tests pass, plus the build and lint.
- [ ] Step 5. Commit "Control: simulated deployments from Arena".

### Task 9: Arena control page

**Files:**
- Create:
  - `web/src/arena/ArenaControlPage.tsx`, `registry.ts`, `defaults.ts`;
  - `web/src/arena/panes/{Runs,ForkTree,World,Storage,Engine,Compare}.tsx`;
  - `web/src/arena/routes.tsx`.
- Modify:
  - `web/src/App.tsx`: `/arena/*`, with `/tests/*` redirecting to the matching `/arena` path;
  - `web/src/shell/TopBar.tsx`: the label becomes "Arena";
  - the `arena` API: `world()`, `usage()` and `gc()` for Task 7's routes.
- Remove: `tests/` pages that are now fully replaced. Keep shared helpers that moved, or re-export them.
- Test: `web/src/arena/ArenaControlPage.test.tsx`, and move or adjust the existing `tests/*.test.tsx` so coverage isn't lost.

Panes, as in the spec's "Arena control panes":
- **Runs** starts a run (team and task, reusing TestsHome and TeamPage pieces), deletes a run (with a confirm step) and opens a branch at `/arena/runs/:run/:branch`.
- **Storage** shows usage against the limit, and "Clean up" calls `gc()` and shows what it freed.
- **Engine** reuses `useArenaHealth` and `EngineLine`.

Defaults:
- **Overview:** row [Runs, col [World, Engine]].
- **Storage:** row [Storage, ForkTree].

Steps:
- [ ] Step 1. Write the failing tests:
  - `arena_overview_layout`
  - `tests_routes_redirect`
  - `world_pane_shows_restore_words`: shows "Exact from a save", "Exact by replay" and "Approximate", and "Can't be restored" for none.
  - `storage_clean_up`
  - `arena_down_is_plain`
  - `delete_run_confirms`
- [ ] Step 2. They fail.
- [ ] Step 3. Implement.
- [ ] Step 4. Run all web tests (`npm test`), the build, and lint on the new and changed files.
- [ ] Step 5. Commit "Arena: Arena control page".

### Task 10: End-to-end, captures and docs

**Files:**
- Create: `web/e2e/control.spec.ts`
- Modify: `docs/internals/` (add `workspace.md`: the model, registry, sources, and how to add a pane), `README.md` or the user guide where it describes the project page and Tests, and `CHANGELOG.md`.
- Captures go in the ledger folder only (not committed).

Steps:
- [ ] Step 1. Write the Playwright spec (Practice agents, through `crates/e2e-server`) covering:
  - `/p/<team>` opens Control with the board pane;
  - start the team;
  - drag the Status tab to the right edge of the Timeline; reload and confirm the layout persisted;
  - the "Move to…" keyboard path;
  - pause all, then resume all;
  - emergency stop with confirm;
  - Pixel 7: the pane switcher.
- [ ] Step 2. Run `npm run e2e -- control.spec.ts` (desktop and mobile projects). It passes.
- [ ] Step 3. Take real captures of the running app:
  - Control Live, Agent focus and Cost, in the Mission look and in Follow app look;
  - Arena control Overview;
  - a simulated run's Replay, using a throwaway arenad on a free port with the retail world, if it runs within the disk limit;
  - mobile.
  - Save working copies in the ledger folder `captures/`. Report any capture that could not be taken.
- [ ] Step 4. **Screenshots and usage videos for the docs** (added by the user on 2026-11-02). All are recordings of the real application, never mocked (AGENTS.md).
  - **Screenshots:** commit optimised PNGs (each ≤ 400 kB, 1440×900 desktop, plus one mobile) to `docs/images/control/`. Cover:
    - Control Live (Mission look);
    - Agent focus;
    - Cost;
    - Follow app look;
    - Arena control Overview;
    - a simulated run's Replay;
    - mobile.
  - **Usage videos:** short Playwright `recordVideo` recordings of real flows with Practice agents, driven by a dedicated spec `web/e2e/control-video.spec.ts` that runs only when `RECORD_VIDEOS=1`. Commit them to `docs/videos/` as `.webm`, also converted to `.mp4` with ffmpeg if it is installed. Keep each 20–60 s at 1280×800, with pauses between steps so a viewer can follow, and ≤ 3 MB each. The flows:
    1. open a project's Control and start the team;
    2. rearrange panes: drag to split, join as tabs, resize, pop out, add and close a pane, switch layouts;
    3. handle trouble: an alarm trips, acknowledge it, approve a change, pause all, then resume all, then emergency stop with confirm;
    4. Arena control: open a run's Replay, step the timeline, and fork from a save showing the restore plan (needs a throwaway arenad; skip with a stated reason if it can't run within the disk limit).
  - **Guide:** write `docs/guide/control.md`, a user guide with the screenshots inline and the videos linked, and link it from `README.md`.
- [ ] Step 5. Write the internals docs (`docs/internals/workspace.md`) and the `CHANGELOG.md` entry.
- [ ] Step 6. Commit "Control: end-to-end test, screenshots, videos and docs".
