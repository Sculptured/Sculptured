# Trace export (OpenTelemetry; Laminar first): design

Date: 2026-10-19. Status: agreed with the user ("add it in the next slot"; observability only).

## Intent

People who already use an observability tool can see a team's runs there. Sculptured sends each run as one OpenTelemetry trace to an endpoint the team chooses: Laminar first (it is open source and reads standard OTLP), and any OTLP/HTTP collector (Langfuse, Datadog, Honeycomb, a company's own) the same way. Sculptured's own board stays the main view.

## Goals

- Opt-in per team, off by default. Team Settings → **Send traces**: on/off, destination (Laminar, or another OTLP/HTTP address), the project's API key, and what to send.
- Two levels: **Timings, tokens and costs** (default) or **Also prompts and outputs** (texts capped at 4 KB each).
- Spans built from Sculptured's own events, so Claude Code and Codex both work without their own telemetry:
  - **run** (root): the team, its budget outcome; span events for budget changes, questions asked and answered, hand-overs, stalls and checks;
  - **agent** (one per session of an agent): the agent's name and kind, the computer;
  - **turn**: model (as observed), tokens in/out/cache, cost when priced, the turn's status; `lmnr.span.type = LLM` and `gen_ai.*` attributes;
  - **tool**: the tool's name and whether it failed, under its turn (or the call that started it).
- A turn or tool is sent when it ends; an agent span when its session ends; the run span when the run ends. Children may arrive before their parent; trace and span ids are derived from stable ids, so nothing needs reordering.
- A failed send keeps its place and is retried; nothing is dropped silently, and the team's settings show the last error.

## Non-goals

- Laminar's own features (signals, evals, its trace-analysis model, Slack alerts).
- Receiving telemetry, metrics or logs.
- Changing what the board shows.

## Constraints

- The API key is stored on the server (never in the team file, never sent to the browser after saving). The browser sees only "a key is saved".
- The endpoint must be `https://`, or `http://` to `127.0.0.1`/`localhost` (tests, a local collector).
- Routine tests never call Laminar: a stand-in receiver checks the exact spans.
- Headers: `authorization: Bearer <key>` in lowercase (Laminar requires it), `content-type: application/json`, OTLP/HTTP JSON at `…/v1/traces`. Laminar's endpoint: `https://api.lmnr.ai/v1/traces`.

## Shape

- `crates/controld/src/traces/`:
  - `settings.rs`: `TraceExport { enabled, endpoint, key, content }` in its own settings key; `TraceExportView` without the key; `GET/PUT /api/teams/{id}/traces`.
  - `spans.rs`: a pure builder from events to OTLP JSON spans (unit-tested).
  - `export.rs`: a loop every 5 s for teams with export on; a cursor per team (last exported event), pending starts kept in memory and rebuilt from the run's start after a restart; POST and retry.
- Web: `structure/TraceSettings.tsx` on the Settings page.
- Docs: a guide section and the changelog.
