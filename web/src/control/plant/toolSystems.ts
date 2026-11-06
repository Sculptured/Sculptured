import type { ActionClass } from '../../generated/ActionClass';
import type { Component } from '../../generated/Component';
import type { Basis, Effect, SystemUnit } from './model';

/**
 * Which system a deployment's tool call acts on, from what Sculptured knows about the agent's tools:
 * its own team tools (the `rig` MCP server), the team's components (MCP servers, by id), and the
 * CLI's built-in tools as the interpreter classified them (files, shell, web). An MCP server that is
 * not one of the team's is still its own system, named by the server. Pure.
 */

/** Sculptured's own MCP server for the team tools (drivers `MCP_SERVER`). */
const TEAM_SERVER = 'rig';
/** The MCP server Run in Arena gives each agent (controld `arena_runs`). */
export const ARENA_SERVER = 'arena';

const TEAM_READS = new Set(['team_status', 'search_history', 'read_decisions', 'workflow_status', 'read_skill', 'read_pdf', 'ping']);
const GATE_TOOLS = new Set(['propose_change', 'request_input', 'approve']);

export const SYSTEMS = {
  team: { id: 'team', kind: 'messaging', name: 'Team messages', product: 'Sculptured team tools', basis: 'sculptured' },
  approvals: { id: 'approvals', kind: 'approvals', name: 'Approvals', product: 'Sculptured approvals', basis: 'sculptured' },
  files: { id: 'files', kind: 'files', name: 'Agent folders', basis: 'sculptured' },
  shell: { id: 'shell', kind: 'shell', name: 'Shell', basis: 'sculptured' },
  web: { id: 'web', kind: 'api', name: 'Web and HTTP APIs', basis: 'sculptured' },
  skills: { id: 'skills', kind: 'skills', name: 'Skills', basis: 'sculptured' },
  unknown: { id: 'unknown', kind: 'unknown', name: 'Not classified', basis: 'unknown' },
} satisfies Record<string, SystemUnit>;

export type ToolPlace = { system: SystemUnit; tool: string; effect: Effect; effectBasis: Basis };

const known = (effect: Effect): Pick<ToolPlace, 'effect' | 'effectBasis'> => ({ effect, effectBasis: effect === 'unknown' ? 'unknown' : 'sculptured' });

/** `mcp__server__tool` (Claude Code) as server and tool. */
export function mcpName(name: string): { server: string; tool: string } | null {
  const m = /^mcp__(.+?)__(.+)$/.exec(name);
  return m ? { server: m[1]!, tool: m[2]! } : null;
}

function mcpPlace(server: string, tool: string, components: Component[]): ToolPlace {
  if (server === TEAM_SERVER) {
    if (GATE_TOOLS.has(tool)) return { system: SYSTEMS.approvals, tool, ...known('write') };
    return { system: SYSTEMS.team, tool, ...known(TEAM_READS.has(tool) ? 'read' : 'write') };
  }
  if (server === ARENA_SERVER) return { system: { id: `mcp:${server}`, kind: 'mcp', name: 'Arena world', product: 'Arena (staging)', part: server, basis: 'sculptured' }, tool, ...known('unknown') };
  const c = components.find((x) => x.id === server);
  return { system: { id: `mcp:${server}`, kind: 'mcp', name: c?.name ?? server, part: server, basis: c ? 'sculptured' : 'unknown' }, tool, ...known('unknown') };
}

/** Built-in tools by name, for a call the interpreter did not classify. */
const BY_NAME: Record<string, [keyof typeof SYSTEMS, Effect]> = {
  Read: ['files', 'read'], Glob: ['files', 'read'], Grep: ['files', 'read'], LS: ['files', 'read'],
  Edit: ['files', 'write'], MultiEdit: ['files', 'write'], Write: ['files', 'write'], NotebookEdit: ['files', 'write'],
  Bash: ['shell', 'unknown'], WebFetch: ['web', 'read'], WebSearch: ['web', 'read'], Skill: ['skills', 'read'],
};

function classPlace(c: ActionClass, name: string): ToolPlace | null {
  switch (c.kind) {
    case 'file-read': return { system: SYSTEMS.files, tool: name, ...known('read') };
    case 'file-edit': return { system: SYSTEMS.files, tool: name, ...known('write') };
    case 'web': return { system: SYSTEMS.web, tool: name, ...known('read') };
    case 'skill': return { system: SYSTEMS.skills, tool: name, ...known('read') };
    case 'subagent-spawn': return { system: SYSTEMS.team, tool: name, ...known('write') };
    case 'shell-test-run': case 'shell-build': case 'shell-git': case 'shell-install': case 'shell-other':
      return { system: SYSTEMS.shell, tool: name, ...known('unknown') };
    default: return null;
  }
}

/** Where a tool call landed: by its MCP server, else the interpreter's class, else its name; else not classified. */
export function placeTool(name: string, cls: ActionClass | undefined, components: Component[]): ToolPlace {
  const mcp = cls?.kind === 'mcp' ? { server: cls.server, tool: cls.tool } : mcpName(name);
  if (mcp) return mcpPlace(mcp.server, mcp.tool, components);
  const byClass = cls ? classPlace(cls, name) : null;
  if (byClass) return byClass;
  const byName = BY_NAME[name];
  if (byName) return { system: SYSTEMS[byName[0]], tool: name, ...known(byName[1]) };
  return { system: SYSTEMS.unknown, tool: name, ...known('unknown') };
}
