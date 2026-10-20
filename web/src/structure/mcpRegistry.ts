/** The official MCP Registry (registry.modelcontextprotocol.io): servers searched there become component forms. */

type Arg = { type?: string; name?: string; value?: string };
type EnvVar = { name: string; description?: string; isSecret?: boolean; isRequired?: boolean };
type Package = { registryType: string; identifier: string; version?: string; transport?: { type: string }; runtimeArguments?: Arg[]; packageArguments?: Arg[]; environmentVariables?: EnvVar[] };
type Remote = { type: string; url: string; headers?: { name: string; value?: string; isSecret?: boolean; description?: string }[] };
export type RegistryServer = { name: string; title?: string; description: string; version: string; packages?: Package[] | null; remotes?: Remote[] | null; repository?: { url?: string } };

/** What the new-component form is filled with. */
export type RegistryForm = { kind: 'mcp' | 'mcp-remote'; name: string; description: string; install: string; url: string; headers: [string, string][]; program: string; args: string };

const REGISTRY = 'https://registry.modelcontextprotocol.io/v0/servers';

/** The latest version of each server matching `query`. */
export async function searchRegistry(query: string): Promise<RegistryServer[]> {
  const res = await fetch(`${REGISTRY}?search=${encodeURIComponent(query.trim())}&version=latest&limit=30`);
  if (!res.ok) throw new Error(`The MCP Registry answered ${res.status}`);
  const body = (await res.json()) as { servers?: { server: RegistryServer }[] };
  return (body.servers ?? []).map((s) => s.server);
}

/** `{api_key}` (the registry's variables) as `${API_KEY}`, the secret the agent's computer fills in. */
const secret = (s: string) => s.replace(/\{([A-Za-z0-9_-]+)\}/g, (_, n: string) => `\${${n.toUpperCase().replace(/-/g, '_')}}`);
const words = (args: Arg[] | undefined) => (args ?? []).map((a) => a.value ?? '').filter(Boolean);

const RUNNERS: Record<string, { program: string; needs: string; args: (p: Package) => string[] }> = {
  npm: { program: 'npx', needs: 'Node.js (npx)', args: (p) => ['-y', p.version ? `${p.identifier}@${p.version}` : p.identifier] },
  pypi: { program: 'uvx', needs: 'uv (uvx)', args: (p) => [p.version ? `${p.identifier}==${p.version}` : p.identifier] },
  oci: { program: 'docker', needs: 'Docker', args: (p) => ['run', '-i', '--rm', p.identifier] },
};

/** The component a registry server makes: a program on the agent's computer or a web address. Null when it has neither. */
export function toComponentForm(s: RegistryServer): RegistryForm | null {
  const base = { name: s.title || s.name.split('/').pop() || s.name, description: s.description.slice(0, 500), url: '', headers: [] as [string, string][], program: '', args: '' };
  const pkg = (s.packages ?? []).find((p) => RUNNERS[p.registryType] && (p.transport?.type ?? 'stdio') === 'stdio');
  if (pkg) {
    const r = RUNNERS[pkg.registryType];
    const env = (pkg.environmentVariables ?? []).map((e) => `${e.name}${e.description ? ` (${e.description})` : ''}`);
    const install = [`Needs ${r.needs} on the agent's computer.`, env.length ? `Set ${env.join(', ')} there.` : ''].filter(Boolean).join(' ');
    return { ...base, kind: 'mcp', install, program: r.program, args: [...words(pkg.runtimeArguments), ...r.args(pkg), ...words(pkg.packageArguments)].join(' ') };
  }
  const remote = (s.remotes ?? []).find((r) => r.type === 'streamable-http') ?? (s.remotes ?? [])[0];
  if (!remote) return null;
  const headers = (remote.headers ?? []).map((h): [string, string] => [h.name, h.value ? secret(h.value) : `\${${h.name.toUpperCase().replace(/-/g, '_')}}`]);
  return { ...base, kind: 'mcp-remote', install: headers.length ? 'Get the secrets it names from its provider.' : '', url: secret(remote.url), headers };
}
