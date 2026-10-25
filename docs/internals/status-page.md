# Status page

A team's status page is one Markdown page that says where the work stands and what changed, so a person coming back later reads it instead of the transcripts. One agent, the owner, writes it with the `write_status` agent tool. People read it under Results → Status (`web/src/collaboration/StatusPageView.tsx`).

| Part | Where |
| --- | --- |
| Owner | `TeamDraft::status_owner` (`crates/model/src/draft.rs`), mirrored for the UI in `web/src/draft/statusOwner.ts`: the agent with `advanced.writes_status`; else the workflow's collector; else the one agent at the end of the line (handed work, hands none on); else the one agent that leads (hands work on, is handed none); else the first agent |
| Tool | `write_status { markdown, done? }`: offered by hostd (`seat_tools/collaboration.rs`) to every agent and refused by controld (`controld/src/status_page.rs`) for any agent but the owner, naming the owner. At most 64 KB; `done: true` marks the final version |
| Storage | One `EventBody::StatusPageWritten { markdown, done }` per write, on `Subject::Rig(team)`, actor the owner's seat, provenance agent-reported. The newest is the page; the 100 newest are its versions |
| Briefing | The owner's briefing (full or minimal) says it owns the page, what goes in it, to rewrite all of it each time, to update it when the state changes in a way someone coming back would need, and to write it with `done: true` when the work is finished |
| API | `GET /api/teams/{id}/status` → `{ owner: {id, name, chosen} \| null, page \| null, versions }` (versions without their Markdown, newest first); `GET /api/teams/{id}/status/{seq}` → one version, or 404 |
| View | Re-read every 3 s while open (`STATUS_POLL_MS`). Shows the author, "agent-reported", when, and Final version or Kept up to date; a Version menu reads an earlier one. Shown whether or not the team is live |

Only the owner writes, so the page changes while the team works only when the owner is working. A runner older than the tool does not offer it to its agents.
