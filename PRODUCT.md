# Product

## Platform

web

## Users
Three audiences, all confirmed:
- Solo developers and researchers who run teams of Claude Code or Codex agents on their own machine for coding and research projects.
- Small engineering teams who share boards and oversee agent work together. The hosted beta and the community library serve this audience.
- Non-developers, such as educators, writers and analysts, who use agent teams for work that isn't code. The Textbook creator template is an example.

Sessions vary. Sometimes the user designs and steers closely, with the board open and frequent intervention. Sometimes they set a team up, start it and check back over hours while doing other work. Both patterns are equally common.

## Product Purpose
Sculptured is a visual workspace for teams of agents doing complex tasks. You give each agent a job and a folder, connect their handoffs, and follow the work from one board. Opening any agent shows its conversation, lets you answer its questions and lets you send guidance. Success means a newcomer can run a first team without learning internal concepts, and an expert can design, run and steer large teams quickly.

## Positioning
The team is a board. A multi-agent team is designed and steered visually: each agent's job, its folder and who hands work to whom are laid out on one board, and the user can open and talk to any agent from that board. Visualizations are the primary interface. Raw events stay available beside them as evidence.

## Operating Context
- Agents run locally through the user's installed Claude Code or Codex CLI, using the user's existing sign-in. They may cost money or consume plan usage.
- `sculptured serve` provides the board, journal and workflows in the browser. There is also a hosted beta that connects to the user's computer.
- The work spans two modes. Design is the Structure board, where the team is composed. Run is the Live graph or list, where the team is watched and steered.

## Capabilities and Constraints
- **Default wording:** the default UI uses plain words: Team, Group, Agent, Job, Folder, "hands work to". OpenRig words (rig, pod, seat, member, edge, runtime, RigSpec, YAML) appear only inside Advanced disclosures (see AGENTS.md).
- **Work states stay distinct:** planned, in progress, reported complete, verified and blocked are separate states. Provenance is labelled: protocol-observed, agent-reported, filesystem-observed, captured-tool, verified, heuristic or operator. Unknown stays "unknown".
- **Status display:** status is shown as shape plus text, never colour alone. Disabled controls say why in visible text. Layout is stable: updates never move cards or reset selection.
- **Agent kinds:** every agent is a Claude Code, Codex or OpenCode agent (open models through OpenRouter). Practice agents (free, scripted) are not offered anywhere in the UI: not when adding or switching agents, generating a team, or in Setup. The engine keeps them as a hidden kind for tests only.
- **Feature depth:** advanced features (workflows, stages, review panels, checks, merges, schedules, decision log, budgets) must stay reachable without burdening newcomers.

## Brand Commitments
- The name is Sculptured.
- Existing assets are the logo and banner in `docs/images/` and `web/public`, plus the brand mark component.

## Evidence on Hand
- Real screenshots of the board are in `docs/images/` and `docs-site/screenshots/`.
- Example teams and templates are served by the app.
- User guides and tutorials must be recordings of the real application.
- There are no testimonials, customer names or benchmarks, and none should be invented.

## Product Principles
1. The board is the product: make teams visible, and let the user open and talk to them from there.
2. Make it simple to start and deep when needed: progressive disclosure, never removal.
3. Be honest about state: what an agent claims is not proof.
4. Stay stable under change: live updates never move the user's world.
