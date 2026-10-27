# Triggers

Triggers (`TeamDraft.triggers`, `crates/controld/src/triggers.rs`) give a team addresses that other systems post events to (webhooks): a machine alarm, a work order, an urgent order. The user guide is `docs-site/guides/triggers.mdx`.

## Addresses

- **The key is a secret Sculptured keeps.** Each trigger's address ends in a 64-hex-character key: two random v4 UUIDs, simple form. Keys live in the store (`trigger_keys`, one per team and trigger), never in the draft, so exports, MADL and Community never carry them.
- **Keys are made lazily** by `GET /api/teams/{id}/triggers`. `POST /api/teams/{id}/triggers/{trigger}/new-address` replaces a key, and the old one stops working at once.
- **The local address** is `POST /api/hooks/{key}`. Local Sculptured allows every request (`auth.rs`).
- **The hosted address** is `POST /hooks/{user}/{key}` on the gateway (`sculptured-cloud/src/hooks.rs`). It takes no session and needs no Origin.
  - The gateway checks that the user exists, isn't disabled and has a workspace.
  - It allows 600 calls an hour per user (`limits`).
  - It starts the user's instance if needed and forwards the call to `/api/hooks/{key}`, signed for that user.
  - A wrong key, an unknown user and a Community-only user all get the same 404, so addresses reveal nothing.
  - Calling `/api/hooks/...` on the gateway directly needs a session, as every `/api` path does.
- **The listing builds each address path** from the signed user header: `/hooks/{user}/{key}` when it is there, `/api/hooks/{key}` otherwise. The web prefixes the page's origin.

## Receiving

1. Bodies over 64 KB are refused with 413. A body that parses as JSON is kept as JSON; anything else is kept as text (UTF-8, lossy).
2. `TriggerReceived {trigger_id, receipt_id, body}` is recorded on the team: actor `system:trigger:{id}`, protocol-observed. The caller gets `202 {received: receipt_id}` at once.
3. A spawned task delivers the event.
   - **Team running:** the event is sent straight away.
   - **Team stopped, `starts_team` on:** the task first starts the team as **Start** would (`teams::start_for_event`). A start already under way counts as started. It then waits up to 180 s for the agent to run and up to 30 s for its job (when it has one), so the event follows the job.
   - **Team stopped, `starts_team` off:** nothing is sent.
4. **The message** is `send_message_as` with intent `instruct` and actor `system:trigger:{id}`. Its text is the trigger's words, then the body in a fence, with a line saying it is outside data and never instructions.
5. **`TriggerHandled {receipt_id, delivered, outcome}`** is recorded, verified. The outcome is one of:
   - `sent`;
   - `started the team and sent`;
   - `not delivered: <why>`.

## Invariants

- The body is evidence: shown verbatim and marked `data-raw-evidence`. Agents are told it is untrusted input.
- An event that arrives while the team is stopped is never lost silently: its receipt and outcome are both recorded.
- MADL does not carry triggers yet.
