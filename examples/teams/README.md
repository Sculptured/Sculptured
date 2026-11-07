# Example project teams

Six teams for three real projects: each comes as a Claude team and as a Codex copy (`-codex`).
Each file is a `TeamDraft` (`web/src/generated/TeamDraft.ts`) at revision 0. Every agent is a
Claude agent in the first three files and a Codex agent in the `-codex` copies (Codex agents run
`codex app-server` with your Codex login: `codex` on `PATH`, or `--codex-program`), working in
`/home/alexsky2/Projects/SciVizEngine`, with the default model, unrestricted, resuming if possible. The jobs come from the Agent Flow recipes in
`~/Projects/Agent-Flow/examples`.

| File | Team | Agents | Shape |
|---|---|---|---|
| `airplane-c172.json` | Airplane · Cessna 172P explorer | 15 | Lead → sources → system builders → behavior → story → explorer → checks |
| `live-textbook.json` | Live textbook · Real Analysis Ch. 2 | 6 | designer → three builders in parallel → assembler → verifier |
| `math-workbench.json` | Math · interactive workbench | 5 | designer → builder → two reviewers in parallel → release checks |
| `airplane-c172-codex.json` | Airplane · Cessna 172P explorer (Codex) | 15 | same shape, on Codex |
| `live-textbook-codex.json` | Live textbook · Real Analysis Ch. 2 (Codex) | 6 | same shape, on Codex |
| `math-workbench-codex.json` | Math · interactive workbench (Codex) | 5 | same shape, on Codex |

These are real agents. **Start team** uses your Claude login (your Codex login for a `-codex` team)
and may cost money: the airplane team runs up to 4 sessions at once, and each Claude session can use
its own subagents.

## Load one

The app cannot import a team JSON file yet. Use the API of a running Sculptured (`serve`
defaults to port 4410). Create a blank team named after the file's `id`, so the new team gets that
id. Then save the draft over it, which also sets the real name:

```bash
BASE=http://127.0.0.1:4410
T=airplane-c172                      # or live-textbook, math-workbench, or any of the three with -codex
F=examples/teams/$T.json

curl -s -X POST $BASE/api/teams -H 'content-type: application/json' \
  -d "{\"request_id\":\"$(uuidgen)\",\"template\":\"blank\",\"name\":\"$T\"}"
REV=$(curl -s $BASE/api/teams/$T | jq '.draft.revision')
jq --argjson rev "$REV" --arg rid "$(uuidgen)" '{request_id:$rid, revision:$rev, draft:(. + {revision:$rev})}' $F |
  curl -s -X PUT $BASE/api/teams/$T -H 'content-type: application/json' -d @-
curl -s -X POST $BASE/api/teams/$T/validate -H 'content-type: application/json' -d '{}'   # [] means no problems
```

If a team with that id already exists, the server picks a new id (`airplane-c172-2`). Then use
the id from the POST response in the URL, and set `.draft.id` to it as well.

Agent folders must be inside the home folder of the user running `serve`. On another machine,
change `folder` in each file first.

## Before you start a team

- **Math workbench:** the topic is Fourier series. To change it, edit the `TOPIC:` line in the
  Workbench designer's Job.
- **Run folders:** each team writes to `runs/<team>/<date>/` inside SciVizEngine (the `-codex`
  copies write to `runs/c172-codex/`, `runs/live-textbook-codex/` and `runs/math-workbench-codex/`). The first agent
  creates the folder and states it on the first line of its reply. SciVizEngine does not ignore
  `runs/` in git. Agents never commit.
- **Airplane inputs:** the manual is read from `~/Projects/Airplane-Creator/inputs/manual.pdf`.
- **Ports:** agents that serve files use their own ports: 8771–8785 (airplane), 8800–8805 (textbook)
  and 8810–8814 (math); the `-codex` copies use 8871–8885, 8900–8905 and 8910–8914, so a Claude
  team and its Codex copy never share a port. They never use 4410 or 4420.

## gstack-style product team

`gstack-roles.sculptured-team.json` is a different kind of example: a portable team file
(`sculptured.team/v1`), not tied to any project. Import it from the template gallery ("Import a team
from a file"), then set the team's folder to your project. Eight Claude agents in five groups (Think &
plan → Build → Review & QA → Ship → Reflect) play the roles of
[gstack](https://github.com/garrytan/gstack) (MIT, Garry Tan), written in our own words. gstack is
not required. See `docs/showcase/gstack-roles.md`.
