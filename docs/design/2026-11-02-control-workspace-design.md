# Control workspace and Arena control: design

Status: agreed with the user in conversation on 2026-11-02. Where this says "my call", the
controller decided it; everything else the user said or confirmed.

## Intent

Sculptured is becoming a platform for testing, watching, controlling and deploying agent teams.
The user wants it to feel like the control software of a mission-control room or a ship's bridge:
one dense screen where the operator watches a running deployment and steps in, arranged however
the operator likes. That is "a custom deployment space": every window is a pane that can be
dragged, docked, tabbed, resized, closed and brought back.

It must be **very configurable, with sensible defaults**: someone who never touches the layout
gets a good screen on first open.

## What gets built

Two pages that share one look and one docking system.

1. **Control**: one per project, at `/p/:team/control`. It is the control system for a Sculptured
   deployment (a team running for real) and also the interface for a **simulated deployment** (a team
   running in Arena). Both open the same workspace. A simulated one adds rewind controls.
2. **Arena control**: a separate page for running Arena itself: worlds and their integrations,
   runs, forks, restore plans, saves and disk, and the Arena service's health. It uses the same
   look and docking, with Arena-specific panes. It replaces today's Tests section.

## Goals

- **Everything is a pane.** Panes can be:
  - dragged by their tab onto another pane: drop on an edge to split, or in the middle to join
    its tabs;
  - resized by dragging the gaps between them;
  - closed with ✕ and brought back from "Add pane";
  - duplicated, where the pane allows it (two agent-detail panes side by side, for example);
  - popped out into their own browser window, for a second monitor.
- **Layouts are tabs** across the top of the workspace. Each page has sensible default layouts.
  The user can add, rename, duplicate, delete and reset them. They're kept between sessions in the
  browser, per page.
- **Panes follow a selection.** Clicking an agent, a moment on the timeline or a world key selects
  it. Panes that have "follow selection" on (the default) show it. Popped-out windows follow too.
- **Each pane has its own settings.** For example: which lanes the timeline shows, which event
  kinds the status feed shows, alarm thresholds.
- **The look is mission control by default.** Near-black with a cyan accent and high contrast,
  like the "A" mockup. It is a new **Mission** look in the token layer, offered both in the
  workspace's own setting ("Mission" by default, or "Follow app look") and as a third app-wide look
  in Appearance. Same fonts and scale as the rest of the app; tabular numbers for clocks and counts.
- **Controls for when something goes wrong** (on Control):
  - an alarms panel like a master caution light: alarms come from rules, show in the status feed
    and in the workspace header, and are acknowledged one by one;
  - per agent: pause, resume, stop, send a message, open its full context;
  - approvals: approve or reject a waiting change (with a reason);
  - whole deployment: pause everything, resume everything, and a guarded emergency stop that asks
    for confirmation;
  - simulated runs also: jump back to a save, fork from there with a changed instruction and an
    exact or approximate restore, and compare the fork with the original.

## Non-goals (this round)

- **Editing a proposed change before approving it.** The server can only approve or reject today.
  Rejecting with a note asking for a revision is the path. *My call:* adding an edit path needs a
  new answer kind end to end, so it waits.
- **Auto-approve rules.** These must run on the server to be safe; rules held in a browser tab only
  work while the tab is open. *My call:* deferred, and named in the report.
- **A true freeze of an agent's process.** The runner has no such command. "Pause" means "stop at
  the next safe point, keeping the session so Resume continues it" (see Backend).
- **A plant camera or 3-D view.** No data source exists.
- **Daemon logs.** The server writes logs to stdout only. The "Raw events" pane shows the event
  journal instead, labelled as such.
- **Pausing an Arena run.** arenad has no pause. Simulated deployments show pause and stop as
  disabled, with the reason in visible text.

## Anti-goals

- A dashboard that looks impressive but hides or overstates the truth. Sculptured's honesty rules
  apply in every pane:
  - status is a glyph plus a word, never colour alone;
  - what an agent reports about itself is "reported";
  - missing information says "unknown";
  - raw evidence sits next to the summaries.
- A layout that moves under the operator. Live updates never move panes, reset selection, or
  scroll a pane the operator has scrolled.
- Something that only works with a mouse. Every drag has a keyboard path:
  - the pane menu's "Move to…" lists each other pane, with the choice of left, right, above, below
    or as a tab;
  - splitters are focusable and move with the arrow keys.
- A screen that is empty or confusing on first open. The defaults must be good.

## Constraints (from the repository)

- `AGENTS.md` and `DESIGN.md` hold.
  - **Tokens:** one token layer (`web/src/tokens.css`), and no raw hex anywhere else.
  - **Language:** plain words, with OpenRig words only inside Advanced. Server errors go through
    `web/src/errors.ts`, and raw evidence is marked `data-raw-evidence`.
  - **Behaviour:** every drag has a keyboard path, and layout is stable.
  - **Mobile:** a mobile layout exposes the same evidence.
- **The entry point changes, but the board stays first.** The project's landing page becomes
  Control (`/p/:team` goes to `control`). `AGENTS.md` says the team board is the entry point. That
  still holds, because the default Control layout puts the live team board first, as the "System
  overview" pane. Update the `AGENTS.md` wording to match.
- **Web lint** (`web/lint`): `max-lines-per-function` 30 and `max-lines` 300 per file, with no
  warnings. New and changed files must pass it, even though some old files don't.
- **Rust:**
  - controld routes follow the `routes()` per module pattern;
  - generated TS types come from ts-rs (`cargo test --workspace export_bindings`);
  - behaviour tests go in `crates/controld/tests`, with the stub driver.
- **Libraries:** no new runtime dependency for docking. Build it in-house on pointer events, as
  the board's drags are. *My call:* keeps the bundle and the look ours, and the needs are modest.
- **Testing:** vitest for the model and components, and Playwright with Practice agents only for
  the end-to-end path. Real captures of the running app for the visual check.

## How it works

### The workspace (shared by both pages)

- **Layout tree.** A layout is a tree:
  - a *split* is a row or column of children with relative sizes;
  - a *stack* is a set of pane tabs with one active;
  - a *pane* is an instance id, a pane type and that instance's settings.

  Pure functions edit the tree: move a pane to another stack's edge or centre, close, add,
  duplicate, resize. Emptied stacks and single-child splits are pruned. Moving a stack's only pane
  onto itself does nothing.
- **Persistence.** Each page's layouts are saved in localStorage under a versioned key. An
  unreadable or old-version value falls back to the defaults, without losing the user's other
  pages. Pane types that no longer exist are dropped when a layout is loaded.
- **Pane registry.** Each pane type declares:
  - its title and glyph;
  - which page and which kind of deployment it applies to;
  - whether it allows several copies;
  - its default settings and settings form;
  - its body component.

  "Add pane" lists the types that apply. One already on screen is marked "shown"; choosing it
  again focuses it, unless the type allows copies.
- **Selection.** A workspace context holds the selection (an agent, a time, a world key). It is
  shared with popped-out windows of the same workspace over `BroadcastChannel`.
- **Popping out.** A popped-out pane opens in a window at the page's `…/pane/:instance` route,
  which renders only that pane. Its slot in the layout says "Shown in another window", with a
  "Bring back" button. Closing the window brings the pane back.
- **Mobile.** Below the mobile breakpoint, the active layout's panes show one at a time, in a
  single column with a pane switcher. There is no dragging there, but the same panes and evidence
  are available.
- **Look.** The workspace root carries `data-look`. Its value is the Mission look by default, or
  the app's look if the user picks "Follow app look". The token file gains a Mission token set that
  applies at `:root[data-look='mission']` and on the workspace element.

### Where the data comes from

Panes don't call the server directly. They read a **source** with one interface, and there are
two implementations:

- **A deployment source**, for a Sculptured team. It is built on what exists:
  - the shell's live stream (`shell.teams`, `RigState`) and `useTeamEvents`;
  - questions and changes;
  - usage and budget;
  - the seat and team control routes.
- **A simulated source**, for an Arena run branch. It is built on the `/api/arena/*` proxy:
  - the branch;
  - its stream;
  - rows;
  - restore-plan, fork and compare.

The interface covers:
- agents, each with a status word and glyph;
- a merged event list (time, agent, kind, plain text, raw record);
- approvals, with a decide action;
- control actions, each either available or disabled with a reason;
- metrics;
- the timeline's saves and fork action (simulated only);
- world rows (simulated only).

### Control panes

The panes:
- **System overview:** the live team board for a deployment (reusing `TeamLive`). For a simulated
  run, the agents plus a grid of the world's parts.
- **Status:** a plain-word feed of what's happening, with filters in the pane's settings. Alarms are
  highlighted.
- **Timeline:** one lane per agent, plus lanes for saves and alarms, with a time cursor.
  - On a deployment, it scrolls back through history.
  - On a simulated run: step back and forward, "Fork from here" (with exact or approximate
    restore, and a changed instruction), and the per-part restore plan.
- **Alarms:** the master caution list.
  - It is built from rules over the event stream: an agent failed or needs attention, a stall is
    suspected, the context ceiling was reached, a limit was hit, a question has waited longer than a
    threshold, and a part's error event on a simulated run.
  - The user can switch each rule on or off and set its threshold.
  - Acknowledgements are kept per team.
- **Approvals:** waiting questions and change approvals. Approve, or reject with a reason.
- **Agent detail:** one agent's activity and conversation (reusing `AgentActivity` and
  `AgentConversation`), with pause, resume, stop and send message. It allows several copies.
- **Metrics:** tokens and cost from usage and budget, tool calls per minute derived from events,
  and machine metrics.
- **Raw events:** the team-wide event journal, marked `data-raw-evidence`.
- **Results:** today's Results sections, chosen in the pane's settings.
- **World state** (simulated only): the world's keys and values from the save at the cursor.
- **Compare** (simulated only): this branch against another.

The workspace header holds:
- the run status;
- the elapsed clock;
- the master caution light, with a count of unacknowledged alarms;
- Start, Pause all / Resume all, and Emergency stop. Emergency stop opens a confirm step naming how
  many agents it stops.

The default layouts (my call) are:
- **Live:** System overview (with Raw events as a tab), Status (with Approvals and Alarms as tabs),
  and Timeline across the bottom.
- **Agent focus:** Agent detail, Timeline and Raw events.
- **Cost:** Metrics and Results.
- **Replay** (simulated only): Timeline large, World state and Compare.

### Arena control panes

The panes:
- **Runs:** start a run (team and task) and delete a run. Opening a run opens its simulated Control
  workspace.
- **Fork tree:** a run's branches, parent to child.
- **World:** the benchmark's parts. For each: its integration, version, how it restores (exact from
  a save, by replay, or approximate), whether it uses the arena clock, and its tool count.
- **Saves and disk:** disk used against the limit, the bytes held by each part, and a "Clean up"
  action that runs gc.
- **Engine:** arenad's health, its tools and its benchmark.
- **Compare:** any two branches.

The default layouts (my call) are **Overview** (Runs, World, Engine) and **Storage** (Saves and
disk, and Fork tree).

Routes:
- `/arena` is Arena control.
- `/arena/runs/:run/:branch` is that branch's simulated Control workspace, replacing RunPage.
- `/arena/compare/:a/:b` stays.
- `/tests/*` redirects to the matching `/arena` address.
- The top bar's "Tests" link becomes "Arena".

### Backend additions

controld, in the sculptured repo:
- **Per-agent pause and resume.** Pause stops the agent at the next safe point and keeps its
  session; Resume continues that session.
  - Use the existing `Control::Checkpoint` if it does that (the budget pause uses it), by
    accepting `"checkpoint"` in `Control::parse`. If it doesn't, use `soft` then `resume`.
  - The builder confirms which, and documents the behaviour in plain words in the UI.
- **Whole-team pause and resume.** `POST /api/teams/{id}/pause` and `POST /api/teams/{id}/resume`
  apply that to every live agent, modelled on `stop-now`.
- Both have behaviour tests and generated TS types.

arenad, in the arena repo on branch `integrations`:
- `GET /api/world`: the benchmark's parts, each with its integration's describe summary (name,
  version, restore, `uses_clock`, tool count) and whether it answered.
- `GET /api/usage`: bytes per part, the total, and the limit.
- `POST /api/gc`: runs the same gc as startup and returns what it removed.
- Each has a unit test. The `/api/arena/*` proxy already allows GET and POST.

## Left to the builder

- Component and file names inside the layout above.
- Exact alarm rule thresholds and defaults (sensible, and editable).
- The exact Mission palette values, as long as contrast is high and the status hues stay
  distinguishable.
- The mobile breakpoint, which should match what the app already uses.
