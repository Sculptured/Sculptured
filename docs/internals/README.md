# Sculptured internals

These pages explain how Sculptured works for contributors changing its code. Start with the [overview](overview.md) for the architecture and code map, then follow the topic relevant to your change. For instructions on using the app, see the [user documentation](../../docs-site/README.md).

This repository contains the app, runner and shared types. The hosted gateway and Community server live in separate private repositories; these pages describe their boundaries where they connect to the core.

## Start here

Read these pages to understand how a design becomes a running team and how its activity reaches the UI:

1. [Overview and component map](overview.md): local and hosted modes, crate responsibilities and architectural boundaries.
2. [Team lifecycle](team-lifecycle.md): design validation, compilation, startup, handoffs, questions and shutdown.
3. [Events and storage](events-and-storage.md): the append-only journal, projections, live updates, SQLite schema and restore behavior.
4. [Testing](testing.md): test suites, fixture CLIs and commands for validating a change.

## Find a subsystem

| Area | Page | What it covers |
| --- | --- | --- |
| Community | [Community](community.md) | Community in the hosted app: the gateway's asserted user, accounts, the web section and Library sources |
| Agent execution | [Drivers](drivers.md) | CLI protocols, normalized output, sessions, interruption and process shutdown |
| Resource control | [Limits and usage](limits-and-usage.md) | Managed subagents, token limits, plan usage holds and cost estimates |
| Agent efficiency | [Protections and contracts](protections.md) | The tool-call guard, shell output cap, context ceiling, and what each agent reads, writes and decides |
| What agents know | [Knowledge](knowledge.md) | The generated project map, lessons, history search and the Knowledge view |
| Agent skills | [Skills and integrations](skills-and-integrations.md) | Skills per agent, how each CLI loads them, and tools on the agent's computer such as Kindex |
| Work coordination | [Workflows](workflows.md) | Stages, items, reviews, checks, merges, guards and schedules |
| Files and Git | [Local operations](local-ops.md) | Folder access, working copies, checks, CLI sign-in and operations on connected computers |
| Import and export | [Portable formats](portable-formats.md) | Team and run schemas, validation, export and imported history |
| Team generation | [AI team generator](ai-team-generator.md) | How Describe it drafts a team through the user's CLI and converts it into a design |
| Connected computers | [Devices](devices.md) | Pairing, runner connections, folder policy and computer selection |
| Runner communication | [Host protocol](host-protocol.md) | Messages exchanged between the server and a runner |
| Distribution | [Desktop and installers](desktop-and-installers.md) | Connection commands, release artifacts and desktop account mode |

## Keep the docs aligned with the code

Each page lists its source files. When you change one of those files, review the corresponding explanation, invariants and test references. Label planned work and unused implementations explicitly.

The [style guide](STYLE.md) defines the page structure. It separates explanation, reference and procedures, uses Mermaid diagrams for architecture and runtime behavior, and ties important properties to tests. Link to the page that owns a concept instead of duplicating its explanation.
