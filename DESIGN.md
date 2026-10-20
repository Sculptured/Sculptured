---
name: Sculptured
description: A visual workspace for teams of agents. One token layer, two looks (Coastal and Standard), with honest status.
colors:
  # Coastal (default look, dark only). :root in web/src/tokens.css
  coastal-bg: "#111b22"
  coastal-panel: "#17232b"
  coastal-raised: "#1d2c34"
  coastal-hover: "#2a3e47"
  coastal-well: "#142028"
  coastal-line: "#31434c"
  coastal-line-2: "#435963"
  coastal-line-3: "#718b92"
  coastal-text: "#f1f2ee"
  coastal-muted: "#b1c0c2"
  coastal-text-disabled: "#7d8f94"
  coastal-accent: "#add5ce"
  coastal-accent-ink: "#172b31"
  coastal-red: "#f1a8ae"
  coastal-green: "#9cd9bb"
  coastal-amber: "#e9cb98"
  coastal-violet: "#c3a6ff"
  # Standard, light. :root[data-look='standard']
  standard-light-bg: "#f7f8fa"
  standard-light-panel: "#ffffff"
  standard-light-raised: "#ffffff"
  standard-light-hover: "#f1f2f4"
  standard-light-well: "#f7f8fa"
  standard-light-line: "#e6e8eb"
  standard-light-line-2: "#d9dce0"
  standard-light-line-3: "#a9aeb6"
  standard-light-text: "#0f1115"
  standard-light-muted: "#5f646c"
  standard-light-text-disabled: "#9a9fa7"
  standard-light-accent: "#5e6ad2"
  standard-light-accent-ink: "#ffffff"
  standard-light-red: "#c9302c"
  standard-light-green: "#1a7f4b"
  standard-light-amber: "#9a5b00"
  standard-light-violet: "#7c4dce"
  standard-light-canvas-bg: "#fafbfc"
  standard-light-canvas-dot: "#dcdfe4"
  standard-light-group-label: "#3d47a8"
  # Standard, dark. data-mode='dark', or prefers-color-scheme: dark with data-mode not 'light'
  standard-dark-bg: "#0f1115"
  standard-dark-panel: "#16181d"
  standard-dark-raised: "#1b1e24"
  standard-dark-hover: "#262930"
  standard-dark-well: "#121418"
  standard-dark-line: "#2a2d34"
  standard-dark-line-2: "#363a43"
  standard-dark-line-3: "#5d626c"
  standard-dark-text: "#edeef0"
  standard-dark-muted: "#9ba0a8"
  standard-dark-text-disabled: "#656a73"
  standard-dark-accent: "#8a92f0"
  standard-dark-accent-fill: "#5e6ad2"
  standard-dark-accent-ink: "#ffffff"
  standard-dark-red: "#f2827a"
  standard-dark-green: "#4cc38a"
  standard-dark-amber: "#e5b04a"
  standard-dark-violet: "#b49cf4"
  standard-dark-canvas-bg: "#121418"
typography:
  display:
    fontFamily: "Inter, Segoe UI Variable, Segoe UI, system-ui, sans-serif"
    fontSize: "1.84615rem"
    fontWeight: 600
    lineHeight: 1.25
  headline:
    fontFamily: "Inter, Segoe UI Variable, Segoe UI, system-ui, sans-serif"
    fontSize: "1.38462rem"
    fontWeight: 600
    lineHeight: 1.25
  title:
    fontFamily: "Inter, Segoe UI Variable, Segoe UI, system-ui, sans-serif"
    fontSize: "1.15385rem"
    fontWeight: 600
    lineHeight: 1.25
    letterSpacing: "-0.018em"
  body:
    fontFamily: "Inter, Segoe UI Variable, Segoe UI, system-ui, sans-serif"
    fontSize: "1rem"
    fontWeight: 400
    lineHeight: 1.5
  label:
    fontFamily: "Inter, Segoe UI Variable, Segoe UI, system-ui, sans-serif"
    fontSize: "0.92308rem"
    fontWeight: 500
    lineHeight: 1.5
  meta:
    fontFamily: "Inter, Segoe UI Variable, Segoe UI, system-ui, sans-serif"
    fontSize: "0.84615rem"
    fontWeight: 400
    lineHeight: 1.5
  mono:
    fontFamily: "JetBrains Mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace"
    fontSize: "0.84615rem"
    fontWeight: 400
    lineHeight: 1.5
rounded:
  sm: "5px"
  md: "8px"
  lg: "12px"
  pill: "999px"
  standard-sm: "4px"
  standard-md: "6px"
  standard-lg: "8px"
spacing:
  space-1: "4px"
  space-2: "8px"
  space-3: "12px"
  space-4: "16px"
  space-5: "24px"
  space-6: "32px"
  row: "30px"
  control-h: "30px"
  control-h-sm: "26px"
components:
  button:
    backgroundColor: "{colors.coastal-raised}"
    textColor: "{colors.coastal-text}"
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    padding: "6px 11px"
    height: "30px"
  button-hover:
    backgroundColor: "{colors.coastal-hover}"
  button-primary:
    backgroundColor: "{colors.coastal-accent}"
    textColor: "{colors.coastal-accent-ink}"
    rounded: "{rounded.md}"
    padding: "6px 11px"
    height: "30px"
  button-primary-standard-light:
    backgroundColor: "{colors.standard-light-accent}"
    textColor: "{colors.standard-light-accent-ink}"
    rounded: "{rounded.standard-md}"
  button-primary-standard-dark:
    backgroundColor: "{colors.standard-dark-accent-fill}"
    textColor: "{colors.standard-dark-accent-ink}"
    rounded: "{rounded.standard-md}"
  button-ghost:
    textColor: "{colors.coastal-muted}"
    rounded: "{rounded.md}"
    padding: "6px 11px"
  button-danger:
    textColor: "{colors.coastal-red}"
    rounded: "{rounded.md}"
    padding: "6px 11px"
    height: "30px"
  button-sm:
    typography: "{typography.label}"
    padding: "4px 8px"
    height: "26px"
  seg-segment:
    textColor: "{colors.coastal-muted}"
    rounded: "{rounded.sm}"
    padding: "3px 9px"
    height: "24px"
  input:
    backgroundColor: "{colors.coastal-well}"
    textColor: "{colors.coastal-text}"
    rounded: "{rounded.md}"
    padding: "9px 11px"
    height: "34px"
  popover:
    backgroundColor: "{colors.coastal-raised}"
    rounded: "{rounded.lg}"
  command-palette-trigger:
    backgroundColor: "{colors.coastal-well}"
    textColor: "{colors.coastal-muted}"
    typography: "{typography.label}"
    rounded: "{rounded.md}"
    padding: "0 8px"
    height: "26px"
  agent-card:
    rounded: "{rounded.lg}"
    width: "200px"
    height: "200px"
  status-pill:
    typography: "{typography.label}"
    rounded: "{rounded.pill}"
    padding: "2px 10px 2px 8px"
    height: "26px"
  usage-strip:
    backgroundColor: "{colors.standard-light-panel}"
    textColor: "{colors.standard-light-muted}"
    typography: "{typography.label}"
    padding: "6px 16px"
    height: "36px"
---

# Design System: Sculptured

## Overview

**Creative North Star: "The Honest Board"**

Sculptured is a working instrument: a board where a team of agents is laid out, started, watched and steered. The system exists so that the board, and the state of each agent on it, reads at a glance and never overstates what is known. The chrome around the board stays quiet. The board (dot grid, tinted groups, cards, handoff lines) is the only expressive surface. Density is that of a desktop tool: a 13px default UI size (user-adjustable 12 to 16px), 30px controls and rows, and tabular numbers wherever counts, clocks, tokens or costs change.

The system ships two looks over one token layer. **Coastal** is the default and incumbent identity: dark only, "ink, chalk and quiet sea-glass", with translucent glass bars, soft depth and a pale sea-glass accent. **Standard** is the category standard played straight, with Linear and GitHub Primer as the craft bar: neutral, flat and exact, white or near-black surfaces, hairline borders, one indigo accent, light and dark. Standard refuses the dark-glass AI-dashboard look: it has no glass, no gradients, and shadows only on things that float. Both looks render the same components from the same CSS. Only tokens change, plus three Standard-only elements (the header status pill, the footer usage strip, and the plain brand mark).

Honesty is the one rule above taste. Status is always a glyph plus a word, never colour alone. What an agent says about itself is "reported" and grey. Green is reserved for a check Sculptured ran that passed. Missing information says "unknown".

**Key Characteristics:**
- One token layer (`web/src/tokens.css`). Component CSS reads tokens only. There is no raw hex outside the token file, except a third-party sign-in button in `hosted/hosted.css`.
- Two looks, switched by `data-look` on `<html>`. Standard has light and dark, set by `data-mode` or the system.
- Six-step type scale relative to the user's UI size. Inter for the chrome, JetBrains Mono only for code, paths, ids and key caps.
- A 4px spacing grid. Controls are 30px tall (26px small).
- Status is shape plus text. Reported completion is grey and verified is green.
- Progressive disclosure, never removal: newcomers see a few featured choices, and everything else is one labelled step back.
- Keyboard depth for experts: Ctrl K palette with `>` and `@`, Shift-click and marquee multi-select, and messaging several agents at once.

### Two looks, one token layer

The token file is layered so that a look is a single override block:

1. **Canonical tokens** on `:root` (the Coastal values): surfaces back to front (`--bg`, `--panel`, `--raised`, `--raised-2`, `--hover`, `--well`, `--scrim`, `--glass`), lines (`--line`, `--line-2`, `--line-3`), text (`--text`, `--muted`, `--text-disabled`), the accent family (`--accent`, `--accent-strong`, `--accent-ink`, `--accent-deep`, `--on-hue`), status hues (`--red`, `--green`, `--amber`, `--violet`, `--blue`), provider colours (`--claude`, `--codex`, `--practice`, `--other`), data series (`--k-*`, `--series-1..6`), type, space, radius, depth and motion. `--light` and `--shade` are the two mixing ends used for every inset highlight, shadow and scrim.
2. **Derived tokens** built with `var()` and `color-mix(in srgb, …)` on the canonical ones: `--accent-soft` (accent at 10%), `--accent-line` (40%), `--red-soft`, `--green-soft`, `--amber-soft`, `--violet-soft`, `--focus-ring`, `--selection`, scrollbar colours, and all `--status-*` tokens. They follow a look automatically.
3. **Look-level material tokens**, so a look can be flat without touching component CSS: `--glass-blur`, `--overlay-blur`, `--app-bg`, `--sidebar-bg`, `--sidebar-line`, `--nav-current-bg`, `--nav-current-ring`, `--panel-shadow`, `--canvas-bg`, `--canvas-dot`, `--canvas-dot-size`, `--group-bg`, `--group-line`, `--group-label`, `--card-bg`, `--card-bg-needs-you`, `--card-bg-idle`, `--accent-fill`, `--accent-fill-strong`, `--btn-primary-shadow`, `--brand-mark-bg`, `--wash`, `--avatar-bg`.
4. **Legacy aliases** (`--r`, `--r-sm`, `--r-lg`, `--surface`, `--border`, `--mono`) point at canonical names. New CSS uses the canonical names.

**To add a look:** add one block at the end of `tokens.css`, `:root[data-look='<name>'] { color-scheme: …; --bg: …; --panel: …; … }`, that overrides canonical tokens (and material tokens where the look's material differs). Then add the look to `LOOKS` in `web/src/appearance.ts`, and teach `public/appearance-boot.js` to set it before first paint. Derived tokens recompute through `color-mix`. A look may retune a derived token's mix strength, as Standard does for `--accent-soft` at 8% and `--amber-soft` from a brighter amber. Component CSS never changes for a new look. If the look has a dark variant, repeat the dark values twice: once under `@media (prefers-color-scheme: dark)` guarded by `:not([data-mode='light'])`, and once under `[data-mode='dark']`, as Standard does.

**Switching.** `appearance.ts` holds the per-browser preference (`sculptured.appearance.v1` in localStorage, never sent to the server): look, mode (system, light or dark), font (Inter, System, IBM Plex Sans or Humanist), size (12 to 16px) and Coastal's translucency. `applyAppearance` sets `data-look`, `data-mode`, `data-surface` (glass or solid), `--font-ui` and `--ui-size` on `<html>`, and updates `<meta name="color-scheme">`. `public/appearance-boot.js` sets `data-look` and `data-mode` before first paint so a light look never flashes dark. `shell/AppearanceControl.tsx` is the Appearance dialog, opened by the "Aa" button or from the palette. It shows Look as a segmented control, Mode only for Standard, and "Translucent surfaces" only for Coastal. `useLook()` lets the few look-dependent elements re-render when the look changes in this tab or another one.

**Accessibility preferences.** `prefers-reduced-transparency: reduce` and the "solid surfaces" setting replace glass with `--glass-solid`. In Coastal, `prefers-contrast: more` raises `--muted` and the line tokens. Every animation sits under `prefers-reduced-motion: no-preference`.

### Motion

Motion is short, eased and informative. State transitions use `--dur-fast` (120ms) for background, border, colour and shadow changes on buttons, rows and tabs. `--dur` (180ms) is the standard duration, and `--ease-out` (`cubic-bezier(0.2, 0.8, 0.2, 1)`) is the curve. Popovers, menus, tooltips and dialog panels enter with a 140ms "pop-in" (4px rise, 0.985 scale). Overlays fade in over 120ms. The selection bar and its popovers rise 3px over `--dur-fast`. Live meaning is the only looping motion: the working glyph spins (0.9s), the needs-you glyph pulses (1.6s), a needs-you card breathes a red halo (2s), a flowing handoff line runs dashes (0.9s), and a just-travelled handoff pulses three times. In Logos board style, a working agent's logo loops slowly (`--logo-walk` 1.2s for the Claude Code guy, whose two leg pairs step in turn, `--logo-turn` 4.8s for the Codex mark, `--logo-sway` 2.8s plus rising bubbles for the Practice flask, and a `--logo-orbit` 3.2s arc round the disc); it stops while the tab is hidden, and off-screen nodes are not drawn. With reduced motion, all of it stops and the static glyph (`⣟` for working) and a still working ring still carry the state.

## Colors

Each look has a quiet neutral ground, one accent, and four status hues that only ever colour a glyph and a word.

### Primary
- **Sea-glass** (Coastal `coastal-accent`): focus rings, the current tab underline, selected cards and the selection ring, primary-button fill (with dark `coastal-accent-ink` text), caret, group tint. Hover on accent fills uses `--accent-strong`. Text selection uses `--accent-deep`.
- **Quiet Indigo** (Standard `standard-light-accent`, lifted to `standard-dark-accent` in dark): the same roles. In Standard dark the primary-button fill stays the saturated indigo (`standard-dark-accent-fill`) so white text keeps its contrast, while text-level accents use the lifted indigo. Group labels on the light board use a deeper indigo (`standard-light-group-label`).

### Neutral
- **Harbour Ink / Cool Paper / Night Ground** (`*-bg`): the app canvas behind panels. Standard light's sidebar sits directly on it (`--sidebar-bg: var(--bg)`).
- **Panel** (`*-panel`): panels, sheets and the header. **Raised** (`*-raised`): cards, controls and popovers on a panel. In Standard light, panel and raised are both white and separated by hairlines alone.
- **Hover** (`*-hover`): the fill for hovered rows and quiet buttons. **Well** (`*-well`): inset areas such as inputs, code and raw output.
- **Lines** (`*-line`, `*-line-2`, `*-line-3`): hairlines and quiet borders, control borders, then strong or hover borders and in-text separators.
- **Chalk / Ink** (`*-text`), **Muted** (`*-muted`), **Disabled** (`*-text-disabled`). `--faint` is pinned to `--muted`, so every secondary text colour stays readable on every surface.
- **Board canvas** (`--canvas-bg`, `--canvas-dot`): Coastal draws its dot grid on `--bg` at 24px. Standard uses a slightly lifted canvas (`standard-light-canvas-bg`, `standard-dark-canvas-bg`) with 20px dots.

### Status hues
- **Red** (`*-red`): needs you, blocked, failed. Also danger buttons and error text.
- **Amber** (`*-amber`): waiting, needs setup, and heuristic provenance. In Standard, a needs-you card is tinted with amber-soft over the raised surface.
- **Green** (`*-green`): verified. A check Sculptured ran has passed.
- **Violet** (`*-violet`): unknown.
- Providers (`--claude`, `--codex`, `--practice`, `--other`) colour vendor marks, the working accent of a card (its 2px top rule and running chips), and the vendor name. They are identity, never status.

### Named Rules
**The Shape-Plus-Word Rule.** Colour only ever adds to a glyph and a word. Every state has a mark (`◉ needs you`, `▲ stuck`, `⧗ waiting for a slot`, `⣟ working`, `✓ done (reported)`, `○ idle`, `∅ unknown`, `◌ launching`, `◑ stopping`, `■ stopped`, `✕ failed`, `○ planned`), and a selected card carries a `✓` badge as well as the accent ring.

**The Grey Done Rule.** "done (reported)" means a turn ended. It is coloured `--status-done`, which is `--muted` (grey), never green. Green (`--status-verified`) is the only green agent status, and it means a check that Sculptured ran has passed. Run summaries say "N reported done · M verified" and never more.

**The One Accent Rule.** Each look has exactly one accent. Provider colours and data series are categorical identity, not a second accent.

## Typography

**Display Font:** Inter (with Segoe UI Variable, Segoe UI, system-ui)
**Body Font:** Inter (same stack; the user can switch to System, IBM Plex Sans or Humanist)
**Label/Mono Font:** JetBrains Mono (with ui-monospace, SFMono-Regular, Menlo, Consolas)

**Character:** One neutral UI face does all the talking, with synthetic bold turned off (`font-synthesis-weight: none`). Monospace is a signal: it marks code, paths, ids, tool chips, raw events and key caps, and nothing else.

### Hierarchy
All sizes are rem steps of the user's UI size (default 13px, range 12 to 16px). The px values below are at the default.
- **Display** (600, 24px, 1.25): the library header, the auth card heading and template names. Empty states are rare and get this size.
- **Headline** (600, 18px, 1.25): page, sheet and dialog headings, and dashboard tile values.
- **Title** (600, 15px, 1.25, -0.018em): panel titles, the team title in the header, the agent pane name and home team names. Board card names are deliberately one step down (13px, 600, two-line clamp), because cards are a fixed 200 × 200px.
- **Body** (400, 13px, 1.5): controls, rows and reading text. The transcript column is capped at 820px.
- **Label** (500, 12px): pills, badges, secondary text, field labels, the usage strip and the palette trigger.
- **Meta** (400, 11px): counters, ages, legends, chips, kbd and the palette's "why it is off" lines.
- Leading tokens: `--leading-tight` 1.25, `--leading` 1.5, `--leading-loose` 1.65. Weights: 400, 500, 600 and 700. 700 is kept for small emphatic marks (selection check badges, the numbered flow markers and the needs-you rollup).

### Named Rules
**The Tabular Numbers Rule.** Any number that changes or lines up (counts, ages, clocks, tokens, costs, meters and budgets) uses `font-variant-numeric: tabular-nums`, so live updates never jiggle the layout.

**The Six Steps Rule.** Type sizes come from `--text-xs` through `--text-2xl` only. Relative `em` sizes appear only for inline code scaling.

## Layout

The app is one top bar, then the page; nothing is stacked in a side column. Every project page adds a project tab row under the top bar. One job per page, at most one primary action per page, and a comfortable measure (forms 720px, the conversation 820px, lists and Results up to 1280px) with generous space. The structure works like air-traffic control: the board is the radar scope that shows everything at a glance, and every subsystem (an agent, the agents list, results, settings, a workflow part) has its own page.

**Information architecture (routes).**
- `/` **Projects**: one table row per project: name (opens its board), status (shape plus words), agents, last activity, and one action (Start… or Stop…, through the project's own confirmation) with a `⋯` menu (Hide, Move to Trash). Hidden projects and the Trash fold below. With no projects, the template gallery is the page. New project is the one primary action.
- `/new` **New project**: the template gallery full-page: featured templates, More templates (N), Describe it, and Advanced (import a team file, a past run, or start a team from a file).
- `/p/:team/board` **Board**: the canvas only, full width. Design and Run are a segmented control at the start of the board toolbar (`?mode=design|run`; without it, Run while the team is set to run, else Design). A card clicked without dragging, or Enter on a focused card, opens the agent's page; Shift-click and marquee multi-select still work, and dragging still moves. Back to the board restores its viewport and selection. A group or an arrow shows a small floating bar of its settings (name, models, Save to Library, Delete; or its two agents, kind and Delete). The design assistant is a floating window over the board's corner. Once every agent has stayed finished, a slim bar under the Run board says "Run finished · N reported done · M verified" and links to Results.
- `/p/:team/agents` **Agents**: a table (select, name, runs on and model, status with any waiting or stuck mark and a raw reason, folder, last activity, last message as agent-reported evidence), then the connections as sentences: the text fallback for the board. Ticking rows (or Shift-click) and Message selected… or Message all… open the Message agents dialog.
- `/p/:team/agents/:agent/{conversation|setup|activity}` **Agent**: a header (Board back, name, status, what it runs on) and three tabs. Conversation (the default once the team has started) is the transcript and message box. Setup (the default before a first start) is one roomy column: Basics (Name; Runs on and model on one row), Job (a large editor), Folder, Connections (each link's kind as a select, Connect to…), then Advanced folded (group, limits, check, skills, and the raw configuration under `data-advanced`), and Delete agent… Problems are one compact line at the top that names the fields, and each problem also shows beside its field. Explanations are a `?` hint whose words are the field's description. Activity holds the controls (soft and hard stop, interrupt, Hand over again), the check, folder, session, tokens and status provenance, then Progress (the visual process, files built and changed, checks) or Raw events (verbatim, `data-raw-evidence`).
- `/p/:team/results[/:section]` **Results**: Summary (what each agent reported, files changed, checks with the verdict, usage this run, and spec notes under Advanced), then its own tabs: Team conversation, Usage, Communications, Knowledge and History.
- `/p/:team/settings` **Settings**: team name and project folder (with own git copies), default models, run budget, workflow links, Git changes, More settings (Claude setup, protection, limits, improve from a run), Export (team file, MADL; the spec under Advanced) and Remove (Hide project, Move to Trash…).
- `/p/:team/workflow/:target` a workflow part (Input, dispatch, a stage, work evidence, the workflow list) as its own page; `/p/:team/git` Git changes.
- `/library/:collection`, `/account` and `/community` keep their pages inside the frame.
- Old addresses redirect: `/team/:t/structure` → Board in Design; `/team/:t`, `/team/:t/live`, `/rig/:r` → Board; `/rig/:r?view=history` → Results › History; `/team/:t/progress` → Results; `/team/:t/git` → `/p/:t/git`; `/rig/:r/agent/:seat[/:tab]` (and the older `/seat/`) → the agent page, with `visual` → Activity and `events` → Activity with Raw events. An agent's address is its id without the trailing `@team`; when it moves to another group the page follows it to its new address.

Spacing follows a 4px grid: `--space-1` to `--space-6`. Rows and controls are 30px (`--row`, `--control-h`), and small controls are 26px. Board cards are a fixed 200 × 200px. Team positions persist, so Design and Run show each agent in the same place, and live updates never move cards or reset selection, viewport or filters. Connection labels are placed so they never sit on a line or a card.

The main breakpoint is 720px. Below it, the top bar keeps the brand mark, the project switcher, a search icon and `⋯` (which then also holds Library and Account); the project tab row puts the name and the status with the primary action on its first line and scrolls the tabs horizontally on the second; tables drop their secondary columns; the selection bar spans the width. Every board view has a list and text fallback (the Agents page) that exposes the same evidence.

## Elevation & Depth

The two looks differ most here. **Coastal** is softly layered. Glass bars and the sidebar use `--glass` with `--glass-blur` (`blur(20px) saturate(115%)`). The app background has a 145° wash from `--app-tint`. Every surface has a 1px inset highlight mixed from `--light`. The board itself is flat in both looks: see the Flat Board Rule below. **Standard** is flat. Cards, panels and controls have a hairline border and a transparent shadow (`0 0 0 0 transparent`, not `none`, so the token still composes in a selection ring). Real shadows exist only on things that float: popovers, menus, tooltips, the selection bar and dialogs. Blur is `none`, gradients are gone, and `--wash` is 0.

### Shadow Vocabulary
- **Control** (`--shadow-control`). Coastal: `0 1px 0 color-mix(in srgb, var(--light) 3%, transparent) inset, 0 1px 2px color-mix(in srgb, var(--shade) 25%, transparent)`. Standard: transparent.
- **Card** (`--shadow-card`). Coastal: `0 1px 0 color-mix(in srgb, var(--text) 3%, transparent) inset, 0 2px 8px color-mix(in srgb, var(--shade) 10%, transparent)`. Standard: transparent.
- **Pop** (`--shadow-pop`): popovers, menus, tooltips, the selection bar and the new-messages chip. Coastal: `0 1px 0 color-mix(in srgb, var(--light) 8%, transparent) inset, 0 16px 45px color-mix(in srgb, var(--shade) 35%, transparent)`. Standard light: `0 0 0 1px rgb(15 17 21 / 2%), 0 4px 12px rgb(15 17 21 / 8%), 0 12px 32px -8px rgb(15 17 21 / 14%)`. Standard dark: `0 0 0 1px rgb(0 0 0 / 30%), 0 8px 24px rgb(0 0 0 / 40%), 0 16px 40px -8px rgb(0 0 0 / 50%)`.
- **Overlay** (`--shadow-overlay`): dialogs. Coastal: `0 24px 64px color-mix(in srgb, var(--shade) 60%, transparent), 0 4px 14px color-mix(in srgb, var(--shade) 40%, transparent)`. Standard light: `0 8px 24px rgb(15 17 21 / 12%), 0 24px 64px -12px rgb(15 17 21 / 22%)`. Standard dark: `0 12px 32px rgb(0 0 0 / 50%), 0 32px 80px -16px rgb(0 0 0 / 60%)`.
- **Scrim** (`--scrim`) behind dialogs. Coastal blurs it 8px (`--overlay-blur`) and Standard does not blur it.

### Named Rules
**The Floating-Only Shadow Rule (Standard).** In Standard, a shadow means "this floats above the page". Resting surfaces are separated by hairlines and tone, never by shadow.

**The Flat Board Rule.** On the board (Design and Run), cards, groups, labels, the Add toolbar, the zoom control and Things to fix carry no shadow, gradient or glow in either look. The zoom control is three ghost buttons with no frame. State shows as flat tinted chips (no outline ring), connection labels sit on the canvas colour without a border, and Things to fix starts folded when it lists more than three items so it never covers cards. The line legend ("hands work to" solid, "reports to / watches" dashed) shows on the Design board only while a dashed line is drawn; the `?` overlay always has it.

**The Material-As-Token Rule.** Glass, blur, gradients and washes are reached only through look-level tokens (`--glass-blur`, `--app-bg`, `--card-bg`, `--wash`), so a flat look switches them off without touching component CSS.

## Shapes

Corners are gently rounded, on three steps and a pill. Coastal: `--radius-sm` for chips, kbd and small toggles, `--radius-md` for controls, rows, cards and inputs, `--radius-lg` for panels, popovers, dialogs and board cards, and `--radius-pill` for pills, badges, edge labels and the palette's scope chip. Standard tightens them to 4, 6 and 8 (`standard-sm`, `standard-md` and `standard-lg`). The 4px step is kept only for the smallest parts: kbd chips, and segments nested 2px inside a 6px segmented tray, so the corners stay concentric. Borders are 1px hairlines throughout. Kbd keys have a 2px bottom border. Placeholders and an empty board Input use a dashed border. Intent-only connections use dashed lines and dashed edge labels. Selection is drawn as a 2px accent ring outside the border plus a corner `✓` badge.

## Components

Every component has one definition in the shared CSS. Looks restyle it through tokens only.

### Buttons
Tactile in Coastal and exact in Standard. One definition (`.btn` in `styles.css`), with variants primary, danger and ghost, and sizes sm and icon.
- **Shape:** gently rounded (`--radius-md`), 30px minimum height, `6px 11px` padding, 6px icon gap, label weight 500.
- **Default:** `--raised-2` fill, `--line-2` border, `--shadow-control`. On hover it moves to `--hover` with a `--line-3` border. On press it moves to `--raised` and drops 0.5px. Focus shows a 2px `--focus-ring` outline at 2px offset.
- **Primary:** `--accent-fill` with `--accent-ink` text at weight 600, and `--btn-primary-shadow` (an inset highlight in Coastal, none in Standard). Hover uses `--accent-fill-strong`.
- **Danger:** red text on `--red-soft` with a 26% red border.
- **Ghost:** transparent and muted. On hover it takes an 8% accent tint and full text colour.
- **Pressed toggle** (`aria-pressed`): accent text, `--accent-line` border, `--accent-soft` fill.
- **Disabled:** `--text-disabled` on a near-transparent fill. Every disabled control says why: in visible text nearby (`.reason`), or, where the reason is a standing fact (the Run tab before a first start, Send with an empty box), as its tooltip on hover and focus plus `aria-describedby`.
- **Sizes:** `btn-sm` is 26px with label type. `btn-icon` is square at the control height.

### Segmented control
A tray (`.seg`) with 2px padding, `--radius-md`, a `--line` border and a 65% `--bg` fill. Segments are 24px, transparent and muted. The pressed segment takes `--raised-2`, a `--line-2` border and a 1px shade. It is used for Design | Run, Graph | List, and the Appearance dialog's Look and Mode.

### Disclosure buttons and popovers
More, Status, Edit tools and project menus are `<details>` whose `<summary>` is drawn as a small 26px button with a `⌄` caret. All popover surfaces share one definition: `--raised` fill, `--line-2` border, `--radius-lg` and `--shadow-pop`. The open disclosure in use draws above any other open one.

### Inputs / Fields
- **Style:** one shared field surface. It has a `--well` fill, a `--line-2` border, `--radius-md`, `9px 11px` padding, a 34px minimum height and an accent caret. Selects get a drawn chevron.
- **Hover / Focus:** hover raises the border to `--line-3`. Focus turns the border `--accent` with a 3px `--accent-soft` halo and no outline.
- **Disabled:** muted text on transparent, with a not-allowed cursor.

### Status pills and marks
- **Agent state mark** (`StateMark`): the glyph and word, coloured by tone. The pill form has a tinted background. A working glyph spins, and a needs-you glyph pulses.
- **Header status pill (Standard):** a summary-button pill (26px, `2px 10px 2px 8px`, `--radius-pill`) with an icon and a sentence such as "Finished (reported) · 2 of 2 done". Its colour comes from `--status-*` through `--sp-c`, as a 9% fill and a 26% border. It opens the Status menu.
- **Status tokens:** `--status-working` is the accent. `--status-needs-you`, `--status-blocked` and `--status-failed` are red. `--status-waiting` is amber. `--status-done` and `--status-idle` are muted grey. `--status-verified` is green. `--status-unknown` is violet.
- **Provenance marks:** every claim shows its evidence tier as a drawn icon plus a word. `◆ verified` uses a shield-check icon and green. `◆ observed` (protocol-observed, filesystem-observed, captured-tool or operator) uses an eye icon. `◇ reported` (agent-reported) uses a quote icon. `≈ heuristic` is amber. `∅ unknown` stands for missing provenance. An agent's own claim is never shown as observed or verified. Inline provenance labels (such as "agent-reported" and "filesystem-observed" in the run summary) are 11px muted pills with a `--line-2` border and a tooltip that explains them. Badges come in accent, red, green, amber, violet and vendor tints, each as a soft fill with an inset 1px ring.

### Cards / Containers
- **Agent card:** fixed 200 × 200px, `--radius-md`, a 1px `--line` border (`--line-3` on hover) and a solid `--card-bg` (panel in Coastal, raised in Standard). No shadow, gradient or top rule. A needs-you card uses `--card-bg-needs-you`, a solid red tint in Coastal and an amber tint in Standard, plus the halo animation. Stopped and planned cards use `--card-bg-idle`. A selected card gets a 1px accent ring and border, never a glow. Connection dots are 10px and appear only on the card you point at or select (always on touch screens). Everything is clamped, so nothing scrolls inside a card. A **Design card** shows the name, one small muted kind line ("Practice · free", "Claude · Claude's default"), the job with no label (up to five lines) and the folder. It has no footer: the arrows say who it hands work to, and the agent's panel lists them. A workflow card keeps its footer of stage actions, and its job gets four lines. A **Run card** shows the name, the state, a tool line only while there is a tool (or a stuck or waiting mark), and the last reply as plain muted UI-face text with no inset box, clamped to three lines.
- **Groups:** a flat tinted region (`--group-bg`, a 4% accent in Coastal, 5% in Standard light and 8% in Standard dark) with no outline (`--group-line: transparent`) and `--radius-lg`. Its label is plain small text in `--group-label` inside the top-left corner. The show/hide contents toggle is a plain text button. A collapsed group becomes a card with a hidden-count line and a show button.
- **Panels:** `--panel` fill, a `--line` (glass-border) edge, `--radius-lg` and a 48px title bar (title, flexible rule, tools).
- **Edges:** 1.75px `--line-3` strokes with round caps. Intent-only edges are dashed. Flowing edges mix in the accent with running dashes. Labels are pills placed beside the line.

### Board style (Cards | Logos)
A per-browser choice (`sculptured.board.style.v1`, `shell/boardStyle.ts`) shown as a "Board style" segmented control after Design | Run on both boards (icons only on a phone), and in Ctrl K as "Board style: Cards" and "Board style: Logos". **Cards** is the default. **Logos** draws each agent as its provider's mark (`shell/LogoNode.tsx`): the Claude Code guy (the pixel creature from Claude Code's start-up block art, drawn here; the small `claude` icon is the same shape), the Codex mark, the Practice flask, in `--claude`, `--codex` and `--practice`, on an 88px disc of `--card-bg` with a `--line` hairline. The name sits under the disc, then the status as a pill with glyph and word (Run) or the kind line, or `▲ N to fix` (Design). Needs you adds a round `!` badge on the disc. Positions never change: each logo is centred in its card's unchanged 200 × 200 slot, and only the arrows' ends move in to a 96px column round the logo (`logoRouteBox`); labels keep the same placement rules. Selection, multi-select (`✓` on the disc), drag, click, Enter and the focus ring all work as on cards, drawn on the disc. Full text and a workflow card's stage footer are Cards-only; the stages stay reachable from their route labels and the Workflow list. Logos move only while working (see Motion).

### Input block
The Input (the project's Input box, 200 × 140, and a workflow's Input, 200 × 200) is the team's starting material, drawn as `.input-card` (`structure/InputMaterial.tsx`, `structure/input.css`): an inbox icon tile and "Input", one muted line ("Links, notes and files the team starts from"), then what was added as counts in words ("2 links · 1 note") and the first items with a kind icon, "+N more"; empty, a dashed inner box says "Nothing yet. Add links, notes or files" and the card's own border is dashed. An accent pill in the corner names the open action ("Open to edit", "Open to add", "Open to read", or "Edit" and who gets it on the project's box). Its border is `--accent-line`, flat like every board node; the project's box keeps its ● port. The text is saved as written; items are read from it (`structure/inputItems.ts`): a line with a web address is a link, a path is a file, a bulleted line or a paragraph is a note. The editor (the workflow's Input page, one 720px column like Setup, and the project's Input panel) has Add a link (a typo is explained, never guessed), Add a note (Ctrl+Enter) or Add files…, the list "Added · counts" with Remove per item, and "Edit everything as text" folded one step back (remembered per browser). Logos style keeps the card.

### Navigation
**Top bar** (52px, glass in Coastal, flat in Standard): the brand (home, `/`), the project switcher (a `<details>` button with the open project's name and its status as a drawn shape; its popover lists every project with status in shape and words, then New project and All projects), Library and Account (with a small `▲` while a CLI is signed out) as nav links whose current one takes `--nav-current-bg`, then right-aligned the Ctrl K box and the `⋯` app menu (Community, Appearance…, Keyboard shortcuts; Computers and Admin on the hosted beta). **Project tab row** (52px, `--panel`): the project's name (double-click to rename), underline tabs Board · Agents · Results · Settings (an agent's page belongs to Agents), and right-aligned the status pill (`.pstatus`: a drawn icon and words, tinted from `--status-*`: Draft · N to fix, Draft · ready to start, Running · 2 of 3 working, Finished (reported) · 2 of 2 done, Needs you · 1 of 3, Stopped) and the one primary action: Start team (disabled with its reason as description and tooltip) or Stop team…, whose inline confirmation (Stop team, Stop everything now, Cancel) opens in a strip under the row. A refused start opens the StartRefused dialog on any page; a plan hold offers Start anyway. Agent tabs, Results tabs and the project tabs share one underline-tab definition with a 2px `--accent` underline on the current one.

### Usage strip (Standard footer)
A 36px footer on `--panel` with a top hairline. It shows tokens this run, cost and run budget in 12px tabular numbers, with values in `--text` at weight 600. A run-budget meter (64 × 6px) has a `--line` track and an `--accent` fill. Anything not reported reads "unknown" with its reason in brackets, never a guess.

### Progressive disclosure
Simple to start, deep when needed. Nothing is removed, and every closed disclosure says what it holds.
- **Featured templates plus "More templates (N)":** the gallery shows four starting teams (template ids `solo`, `builder-reviewer`, `planner-builders` and `blank`). Every other template sits in a "More templates (N)" `<details>` below a hairline. Whether it is open is remembered per browser.
- **"More tools":** the Design board's Add toolbar shows one primary "Add agent". A ghost "More tools" / "Fewer tools" button reveals the other agent kinds, Group, Input, saved groups, the team folder, Full text and All arrows. The choice is remembered per browser.
- **"Advanced" on an agent's Setup:** an agent's essentials (name, job, folder, kind, connections) come first. Group, limits, check, skills and the raw configuration sit under "Advanced · <summary>", where the summary names what is set ("group X · limits set") or what is there ("group, limits, check").
- **Raw events one step back:** the agent's Activity tab shows Progress first; Raw events is the second segment, shown verbatim as evidence. Controls and Hand over again live on Activity too; Hand over again shows above the conversation only when it is the obvious next step (the agent's CLI was signed out), and is always in Ctrl K (which opens Activity and asks there).
- **Board tools, one step back:** the Design toolbar's save state shows only while saving, not saved or in conflict (saved is announced, not drawn). A team with a workflow has a quiet "Workflow list" board tool and the same in Ctrl K; it opens on its own page. The transcript search is a search icon in the agent's tab row (or `/`) that opens the field and folds back when left empty.
- **Phone chrome:** the top bar and the project tab row (two lines: name, status and action; then the tabs, scrolling) sit above every project page. The board has one toolbar row that wraps; an agent is set up on its Setup page, which is already one column.
- **Advanced vocabulary:** OpenRig words appear only inside `data-advanced` disclosures.

### Transcript and composer
- **Meta lines:** a turn end ("Turn completed · 3.5s · 1.5k tokens") is one small muted line with no rules. A run of session events folds to one muted summary ("Restarted · new conversation") that opens on click to the verbatim chain, marked `data-raw-evidence`. A search hit inside opens it, and an alarm always shows in full.
- **Composer:** the message box and Send, nothing else. How the message is delivered (inform waits for the turn to end, instruct may interrupt; "Interrupt current turn") sits behind the `⋯` beside Send, and a small accent chip ("instruct", "interrupts now") names a choice that is not the default. Expand editor shows on the box you point at or type in.
- **Run summary (Results):** a heading that never overstates (Run finished only once every agent has stayed finished; else Run in progress or Last run) and one short sentence, then per agent its name (which opens it), its state and its reply, clamped, behind a small quote icon whose tooltip says "agent-reported". Files changed (filesystem-observed, or unknown with its reason) and Checks follow as their own sections; a check is verified only when Sculptured ran it and it passed, and the verdict says what is verified once. Stop team stays in the project tab row.

### Command palette (Ctrl K)
- **Way in:** a quiet search box with the text "Search or run a command" and a `Ctrl K` key cap (`⌘K` on Mac). It sits in the top bar in both looks (an icon only on a phone). It uses `--well` (or `--panel` in the Standard header), a `--line` border and `--radius-md`.
- **Before typing,** it lists recent actions, likely next steps and the open project's agents. Typing searches everything.
- **Scopes:** typing `>` scopes to commands and `@` scopes to agents. The scope shows as an accent-filled pill chip ("Showing commands only"). Backspace in an empty box, or clicking the chip, returns to everything. The footer teaches `>` and `@`. Alt keys toggle agent-state filters.
- **Rows:** a 16px mark, a title with an optional "why it is off" line, a section and the key. A command that cannot run stays visible with `--text-disabled` title and its reason in muted 11px text. The selected row's mark turns accent. The palette surface is the shared popover.

### Multi-select and the selection bar
- **Selecting:** Shift-click (or Ctrl/⌘-click) cards to pick them. On the Design board you can also drag a marquee (a dashed accent rectangle with an `--accent-soft` fill) or press Ctrl+A. Picked cards get an accent border, a 2px `--accent-line` ring, and a round 18px `✓` badge in the corner. Picked list rows get `--accent-soft` and a `✓` before the name.
- **Selection bar:** a floating toolbar (`--panel`, `--line-2` border, `--radius-lg`, `--shadow-pop`, 12px text) that starts with "N agents selected" in tabular numbers. In Design it floats at the top of the board and offers Model, Group, Folder, Limits (each a small popover with the pop shadow), Delete (Ctrl+Z restores) and Esc to clear. In Run it is fixed near the bottom of the screen and offers "Message…" and Esc. A selected group or arrow gets the same floating bar with its own few settings. On the Agents page, ticked rows (or Shift-click) take the place of the bar: Message selected… and Message all… sit above the table, with the reason when they cannot run. When the action cannot apply, it says why ("None of them is running.").

### Message all
"Message all agents…" and "Message selected agents…" are palette commands, and Run's selection bar also has "Message…". Both open one dialog (`.panel.broadcast`, up to 560px wide). It has a recipient list in a `--well` box with "All running" and "None" quick picks, where agents that cannot receive show why ("stopped; only running agents get messages"), a message box, an intent choice, a preview, and a confirm step. Results come back per agent as a glyph and a word: `◌ sending…`, `✓ sent` or `▲ not sent: <reason>`, with "Send again to N agents" for failures. When the command is unavailable, its row says why ("Start the team first." or "Select agents first: Shift+click their cards.").

## Do's and Don'ts

### Do:
- **Do** read every colour, size, radius, shadow and duration from `web/src/tokens.css`. Component CSS holds no raw values.
- **Do** add a look as one `:root[data-look='<name>']` block that overrides canonical (and material) tokens, and let derived tokens follow through `color-mix`.
- **Do** pair every status colour with a glyph and a word, and give every selection a shape (a `✓` badge or check) as well as the accent ring.
- **Do** colour "done (reported)" with `--status-done` (grey). Use `--status-verified` (green) only when a check Sculptured ran has passed.
- **Do** label provenance with the tier mark and word (`◆ verified`, `◆ observed`, `◇ reported`, `≈ heuristic`, `∅ unknown`), and write "unknown" with a reason when data is missing.
- **Do** use tabular numbers for every live or aligned figure.
- **Do** keep a disabled control's reason visible in text beside it, or as its tooltip and description where the reason is a standing fact.
- **Do** put secondary features one labelled step back ("More templates (N)", "More tools", "Advanced · …" on Setup, "More settings" on Settings, Raw events on Activity) and remember the open state per browser.
- **Do** give every new command a palette row with its key, and give an unavailable command a reason instead of hiding it.
- **Do** keep monospace for code, paths, ids, tool chips and key caps only.

### Don't:
- **Don't** use green for an agent's own claim of completion. An agent saying "done", a tool succeeding or a file existing is not verification.
- **Don't** show status by colour alone.
- **Don't** put shadows on resting surfaces in Standard. Shadows belong to popovers, menus, tooltips, the selection bar and dialogs.
- **Don't** add glass, blur or gradients to Standard, and don't hard-code them in component CSS. Reach them only through `--glass-blur`, `--app-bg`, `--card-bg` and `--wash`.
- **Don't** add a second accent. Provider and series colours are identity, not emphasis.
- **Don't** let a live update move cards, reset selection, viewport or filters, or place a connection label on a line or a card.
- **Don't** show OpenRig vocabulary (rig, pod, seat, member, edge, runtime, RigSpec, YAML) outside an Advanced disclosure.
- **Don't** add a type size or radius outside the token scale.
