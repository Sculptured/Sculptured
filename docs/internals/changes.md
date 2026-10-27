# Proposed changes and rule checks

An agent proposes each change to a business system (an order, a schedule, a record in an ERP) with the `propose_change` seat tool before making it. Sculptured checks the team's rules itself, then approves the change or asks a person. User guide: `docs-site/guides/changes.mdx`.

Sources:
- `crates/model/src/mandate.rs`: `Mandate`, `ChangeRule`, `RuleTest`, `RuleResult`, `evaluate`.
- `crates/controld/src/seat_tools/propose.rs`: the tool.
- `crates/controld/src/changes.rs`: the briefing line and `GET /api/teams/{id}/changes`.
- `crates/controld/src/questions/answers.rs`: `rejected_on_timeout`.
- `web/src/shell/ChangeApproval.tsx`, `web/src/structure/MandateFields.tsx`, `web/src/collaboration/ChangesView.tsx`.

## Flow

1. **The call.** It has `{action, summary, payload, system?, reason?}`, with `payload` an object of at most 32 KB.
2. **`ChangeProposed`.** Recorded on the team (`Subject::Rig`), with the agent as actor, `agent-reported`.
3. **`ChangeChecked`.** `mandate::evaluate` runs every rule whose `action` is `*` or matches, case-insensitive. The results are recorded on the team, with `system:controld` as actor, `verified`.
4. **Decided by the rules.** When `mandate.approve_within` is set and at least one rule applied and all passed, `ChangeDecided { approved: true, by: mandate }` (verified) is recorded. No rule applying is never approval.
5. **Decided by a person.** Otherwise `seat_tools::ask::ask` raises an `approval` question. Its prompt `tool` is the action, and `tool_input` is `{proposal_id, system, payload, reason, checks}`. The call blocks like `request_input`, and the agent is `waiting-input`. Then:
   - **Allow** is approved by a person (`operator`);
   - **Deny**, with an optional reason, is not approved;
   - **Expired** is not approved, `by: nobody-in-time` (verified). `questions::answers::rejected_on_timeout` closes the question with `DecisionSource::RejectedOnTimeout`, and `undecided_expired` leaves approvals out, so no agent decision is awaited.
6. **The reply.** It tells the agent whether the change is approved and by whom, each rule's result with ✓ or ✗ and its words, and the proposal id. It also says to carry out exactly the approved change, or not to make it.

## Rules

| Test | Passes when |
|---|---|
| `at-most {value}` / `at-least {value}` | The field is a number, or text that is one once `$ € £` and `,` are dropped, within the bound. |
| `one-of {values}` | The field's text equals one value, ignoring case and surrounding spaces. |
| `present` | The field exists and is not null, empty text, an empty list or an empty object. |

- **Paths** are dotted, and a number segment indexes a list (`lines.0.item`).
- **A missing or empty field** fails every test: "`field` is missing".

## Invariants

- A proposal changes nothing. Sculptured has no connector yet, so the agent makes an approved change itself. Connectors that write approved changes (ERPNext first) are planned in `plans/2026-10-26-operations-systems-design.md`, outside the repository.
- An agent cannot approve its own change. Only the mandate (rules evaluated by Sculptured) or a person approves.
- An unanswered approval is never approved. This is the opposite of a permission prompt, which D8 allows on timeout.
- An empty mandate is left out of team files, and MADL does not carry it yet.
