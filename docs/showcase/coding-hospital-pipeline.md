# Showcase 2: a "coding hospital" on Sculptured

**Source:** Cockroach Labs, [The experiment: running a hospital for code](https://www.cockroachlabs.com/blog/experiment-running-hospital-code/) ("MOLT Sinai").
- **What it is:** a GitHub Actions pipeline of Claude agents.
- **Roles:** Triage Nurse, Fellow (workup, then treatment), Review Attending (plan review, then parallel code reviewers), Discharge Nurse, plus humans as "Chiefs".
- **Results:** over five months, about 1,238 PRs and more than 1M lines landed, with 7 reverts, at about $84 per issue. The IBM Db2 support work took under two days and $4,172 in tokens.

**The idea:** rebuild their pipeline on Sculptured on a small open-source repo, run the same kind of work, and compare evidence, cost and human time.

## What their diagram needs, and what Sculptured has today

| In the diagram | In Sculptured now | Gap |
|---|---|---|
| Patients (issues) flowing through stages, many at once | **Workflow engine:** a dispatcher creates tasks; stages with an agent and parallel `workers`; per-task history; `max_tasks` and `max_steps` | Intake from an issue tracker; the runs are long-lived (days) rather than a team you start and stop |
| Outcome routing: approved → next; rejected or changes requested → back (dashed arrows) | **Stage `routes`:** an outcome picks the next stage, loops allowed | Outcomes are agent-reported only; no schema for each stage's note (treatment plan, I-PASS) |
| CI: "red: bounce back" | **Checks:** commands Sculptured runs itself, marked *verified* | A check as a **stage**: its pass/fail result picks the route |
| Code review with **parallel reviewers** on one PR | Parallel `workers` (different items), checks | **Fan-out on one item and a join** (all approve / any blocks) |
| Human lane: Chief admits, decides escalations, signs off, Won't fix | Agents can **ask a person** (questions, answerable on a phone); team posture settings | **Human stages:** a person picks the outcome in the UI (admit, approve, won't fix), with a queue of items waiting on people |
| Circuit breaker: after 4 or 6 rework rounds, escalate | Global `max_steps` | **Per-item round limits** with an "escalate" route |
| Each item's branch and PR | **Own copies:** a worktree and branch per *agent*, merge on Copies | **One copy per item**, shared by the stages working on it; open a PR; merge on discharge |
| GitHub: issues in, labels as state, PRs, review threads, merge | Local git page (status, stage, commit, branch) | **GitHub connector:** an issue with a label → item; stage → label sync; PR open, update and merge; CI status in; comments out |
| Scheduled agents (Charge Nurse every 30 min, Research daily, Safety weekly) | Agents run when a team starts | **Schedules** (cron-like triggers for an agent or a team) |
| Precedent log (human-only, agents must read) | Questions and answers per agent | **Decision log:** an append-only team memory people write and agents read through a tool |
| Model per stage; escalate to a stronger model when struggling | Model per agent, group or team | **Model escalation rule** per stage (after N rounds, switch model) |
| "Infection control" pauses everything when main breaks | Stop team; budgets; plan-usage holds | A **pause-all trigger** fed by a check on main |
| Per-stage cost (model, tokens, duration) in an archive branch | Token use per agent, budgets, run files, M4 analytics (in progress) | **Cost per item and per stage**, rounds, send-backs, escalations, time waiting on people |

**Summary:** about half of the diagram exists today: the stage graph with loops, verified checks, own copies, questions, the visual board, run files and token tracking. The other half is a set of "pipeline" features:
1. human stages;
2. check stages;
3. fan-out and join on one item;
4. per-item round limits;
5. per-item copies and PRs;
6. a GitHub connector;
7. schedules;
8. a decision log;
9. model escalation;
10. per-item cost analytics.

## Why Sculptured instead of GitHub Actions

GitHub Actions is a fair choice for them. It has the repo, issues and PRs built in, needs no infrastructure, comes with GitHub's audit trail and secrets, and spreads easily across runners. Its costs show up in the post:
- **Every stage is a cold, stateless job.** It re-reads its instructions (the Discharge Nurse loads about 29k tokens on *every* run), re-checks out the repo, and rebuilds context from labels and comments. Sculptured keeps role sessions alive: context carries over, prompt caching helps, and nothing is re-cloned.
- **State is hacked into labels and HTML-comment sentinels.** Sculptured has a typed state machine per item, visible live.
- **Observability is by hand.** They audited prompt bloat, found the eleven-round one-line fix, and spent $631 of $1,046 in one window on three issues, all after the fact. Sculptured can show, per item, stage, role and model:
  - cost and rounds, send-back and rejection rates, escalation reasons;
  - instruction tokens per run;
  - time waiting on humans.

  It can alert *while* a loop is running (a live circuit breaker) instead of in a weekly safety review. Coding-agent observability is an emerging category, and cost per merged PR is becoming the headline metric. Most tools there only observe; Sculptured also controls.
- **The human loop happens in GitHub comments.** Sculptured gives people a queue of items waiting for them, one-tap decisions on a phone, and clear evidence: verified checks, diffs and run replay.
- **The environment is GitHub's runners.** Sculptured runs on your machines (any always-on box can be a connected computer), with your real databases, Docker and tools. It uses your Claude and Codex *plans* rather than per-token API billing.
- **Hard to share and reuse.** Their pipeline became its own repo, "Sinai", with 25 skill files (about 100k words). On Sculptured the whole hospital is a **team file**, with a generator and templates.
- **Keep GitHub as the system of record.** Integrate with GitHub (issues in, PRs out, labels mirrored) rather than replacing it.

**Similar efforts to know about:**
- [Composio Agent Orchestrator](https://www.mintlify.com/ComposioHQ/agent-orchestrator/concepts/workflows): spawn → worktree → PR → CI → review → merge.
- [Case](https://awesome.ecosyste.ms/projects/github.com%2Fworkos%2Fcase): scout → implementer → verifier → reviewer → closer → retrospective.
- [Agent Orchestra](https://skillselion.com/marketplace/Grimblaz/agent-orchestra): 16 agents for Claude Code.
- GitHub's own agentic workflows in Actions.
- Coding-agent observability: [xtrace](https://xtrace.ai/blog/coding-agent-observability), [Coralogix](https://coralogix.com/blog/code-agents-need-observability), AWS CloudWatch's coding-agent insights.

None of them combine visual design, live control of agents on your own machines, verified evidence and per-role analytics in one place.

## The experiment

1. **Recreate it as a team, with no new features** (a spike):
   - Triage → Workup → Plan review → Treatment → Check (tests) → Code review (2 reviewers) → Discharge, with outcome routes as in their diagram;
   - humans through questions;
   - own copies on;
   - a small open-source repo with 5–10 real issues pasted in as dispatch tasks.

   Record what's awkward. That becomes the pipeline-features spec.
2. **Build the pipeline features** in this order:
   1. check stages;
   2. human stages and the human queue;
   3. per-item round limits;
   4. per-item copies, then PRs;
   5. a GitHub connector (issue label → item, PR out);
   6. fan-out and join;
   7. per-item cost analytics;
   8. schedules;
   9. the decision log;
   10. model escalation.
3. **Run it for a week on one repo.** Compare against their published numbers:
   - cost per merged PR;
   - rework rounds;
   - escalations;
   - human minutes per PR;
   - reverts.

   Publish the run files and a real recording.

## The hospital team file (designed, not run)

**Status:** `docs/showcase/coding-hospital.team.json` passes the schema and every draft check
(`cargo test -p controld --test showcase_hospital`). **It has not been run:** no agent has worked on
an item, so nothing here shows that the team fixes bugs, how many rounds it takes, or what it costs.

**Set it up:**
1. `docs/showcase/hospital-target/setup.sh` copies the patient, *corner-shop* (a tiny Node library),
   to `~/sculptured-hospital` (or the folder you pass) and makes it a git repo with one commit on `main`.
   `npm test` passes there: each of the 7 bug issues (8 bugs; HOSP-5 has two) has failing tests marked `todo` (todo tests don't fail
   the run), so the guard on `main` starts green. `ISSUES.md` lists 8 issues (HOSP-1–HOSP-7 are real
   bugs, one of them a path traversal; HOSP-8 is documented behaviour, there to test triage).
2. Import the team file (`POST /api/teams/import-file` with `{request_id, file}`). Its project folder
   is `/home/alexsky2/sculptured-hospital`, the setup script's default on this computer; the checks
   need an absolute folder for the per-item copies, the Tests stage and the guard. Elsewhere, change
   it in team settings.
3. Starting it uses your Claude login and plan. Wait until you mean to run it.

**The design:**
- **Intake:** the Triage Nurse is the dispatcher. It reads `ISSUES.md` and dispatches one item per issue,
  with an I-PASS triage note in the item's context. Only issues in `ISSUES.md` become items.
- **Stages:**
  - Admit? (person): admit, not-a-bug or wont-fix;
  - Workup (Fellow), then Plan review (Review Attending): approved, or changes back to Workup;
  - Treatment (Fellow): at most 6 rounds; from round 3 the Senior Fellow takes over; past the limit,
    Escalation;
  - Limits (check, verified): the diff is at most 1,000 lines and no existing test line was removed or changed;
  - Tests (check: `npm test` in the item's copy, verified), then Code review (panel: Correctness and
    Safety Reviewers, all must approve);
  - Audit (Chart Auditor): checks the reviewers' work before anything is done;
  - Sign-off (person), then Merge: the item's branch into `main`, locally; a conflict goes back to
    Treatment.
- **Discharge:** the Discharge Nurse is the collector. Once every item has finished, it reports each
  item's outcome and writes changelog lines.
- **Board rounds:** the Charge Nurse is the watchdog. Every 30 minutes it gets a message and reports
  stalled items first, with `workflow_status`.
- **Settings:** guard `npm test` on `main` every 15 minutes, decision log on, a copy per item,
  `max_tasks` 10, `max_steps` 32.
- **Models:** Sonnet by default; Opus for the Senior Fellow, the Review Attending and the Chart
  Auditor; Haiku for the Discharge and Charge Nurses.
- Every number above is a default you can change.

**Lessons from Cockroach Labs' second post** ([Experiment: running hospital code](https://www.cockroachlabs.com/blog/experiment-running-hospital-code/), Storm and Shamim, 2026-10-09), and where each lives:

| Lesson | In the design |
|---|---|
| No code before a plan, no plan without review | Treatment starts only after Plan review approved; the Fellow checks the newest Plan review step first and reports `replan` if it did not approve. The plan must list the files it will touch, the tests, and an estimated size. |
| Stop on scope change; don't improvise | Treatment has a `replan` route back to Workup. A fix that needs anything outside the approved plan stops there. |
| Structured handoff (I-PASS) | Triage notes and the Fellow, Senior Fellow and reviewers' results end in Illness, Patient, Actions, Situation. The receiver opens its result with `Received:` and restates the Actions and Situation. Synthesis is a convention in the job text; the engine does not check it. |
| A fault-finding reviewer, audited | The panel reviewers are not the author, and their jobs are written around finding fault (panel angles `adversarial` and `red-team`). The Chart Auditor audits the reviewers at the Audit stage. It is a separate agent from the Discharge Nurse because the engine does not let the assembly agent work a stage. |
| Circuit breaker | Treatment allows 6 rounds, then Escalation, a person's stage. An agent that sees four consecutive rounds with new blocking findings reports `escalate` unless a precedent covers it. Workup (4) and Audit (3) have limits to Escalation too. Reviewers mark findings blocking or not, new or repeated, so agents can count. |
| Append-only precedent log | The decision log is on. People write it, from any decision card (Admit?, Sign-off, Escalation); every agent reads it first and before escalating. |
| Watchdog for stalled stages | The Charge Nurse's 30-minute board round lists items with no step for 30 minutes first. It cannot re-run a stage itself; a person presses Retry. |
| Rate-limit self-generated work | Nobody but the Triage Nurse makes items, and only from `ISSUES.md`. Agents leave at most one `Follow-up:` line; the Discharge Nurse lists at most three, as suggestions. |
| Size ceiling; tests out of the fix | The Limits check fails a diff over 1,000 lines or any removed or changed line in `test/` other than a `todo` marker. A failure goes back to Treatment, and the Fellow reports `replan` or `too-big`. A change too big to plan goes to Escalation. |
| Records as first-class state | Plans, verdicts and audits are each stage's result, kept on the item with who settled it. Cost per step is on the item board. The agents cannot see it, so none states costs. |
| Prompt hygiene | Each job opens with a one-sentence persona. One short "Always" paragraph replaces the old five-sentence one, and the GitHub clause is gone. |

Not encoded, because Sculptured does not carry them yet: an agent-callable retry (the watchdog only reports), and `triggers` and a change `mandate` in MADL.

**Decisions applied:**
- No GitHub intake and no PR stage: the GitHub connector was dropped. Agents may use `gh` only if
  you configure it, and they never push, open or merge PRs.
- Merging is local, with a copy per item.

**Where the engine differs from the §3 diagram:**
- **Triage is done at intake, not in a stage.** The dispatcher can't also work a stage, so the Triage
  Nurse triages while dispatching. Its "not a bug" route became an Admit? outcome: the Chief closes the
  item there.
- **Discharge is assembly, not a per-item stage.** The collector can't work a stage either, so the
  Discharge Nurse runs once, after all items, instead of after each merge.
- **Retry at Escalation takes two clicks.** Choosing *retry* can't grant rounds by itself: an item past
  Treatment's limit would go straight back to Escalation. So Escalation allows one round. *retry* parks
  the item at Treatment as blocked, and the item's **Retry** grants one more round each time.
- **The decision log tool may be missing.** Agents are told to read the log with `read_decisions`,
  which this branch's base doesn't have yet (only the `decision_log` switch exists).

**Cost, in principle:**
- Each agent has a token limit for one run. Limits count new tokens: input, output and cache writes,
  not cache reads.

  | Model | Agents and limits | Total |
  |---|---|---|
  | Sonnet | Triage Nurse 0.3M, Fellow 2M, two reviewers 0.8M each | 3.9M |
  | Opus | Senior Fellow 1M, Review Attending 0.8M, Chart Auditor 0.6M | 2.4M |
  | Haiku | Charge Nurse 0.3M, Discharge Nurse 0.2M | 0.5M |

- **6.8M new tokens in all.** That is a ceiling by design, not an estimate of use.
- Reaching a limit soft-stops the agent. Usage is reported after each turn, so a turn in progress can
  overshoot.
- The team runs on your Claude plan, so what it costs depends on your plan's usage limits, not
  per-token prices. No pricing has been checked.
- Real numbers (cost per merged fix, rounds, escalations, minutes spent by people) need a real run.
