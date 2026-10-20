import { afterEach, expect, it, vi } from 'vitest';
import { searchRegistry, toComponentForm, type RegistryServer } from './mcpRegistry';

afterEach(() => vi.restoreAllMocks());

it('searches the official registry for the latest version of each server', async () => {
  const spy = vi.spyOn(globalThis, 'fetch').mockResolvedValue(new Response(JSON.stringify({ servers: [{ server: { name: 'io.github.iowarp/slurm-mcp', description: 'Slurm jobs', version: '2.2.0', packages: [] } }] })));
  const found = await searchRegistry('slurm jobs');
  expect(String(spy.mock.calls[0][0])).toBe('https://registry.modelcontextprotocol.io/v0/servers?search=slurm%20jobs&version=latest&limit=30');
  expect(found.map((s) => s.name)).toEqual(['io.github.iowarp/slurm-mcp']);
});

it('runs an npm package with npx, a PyPI one with uvx and an image with docker, naming what it needs', () => {
  const npm: RegistryServer = { name: 'io.github.acme/notes', title: 'Notes', description: 'Team notes.', version: '1.2.0', packages: [{ registryType: 'npm', identifier: '@acme/notes-mcp', version: '1.2.0', transport: { type: 'stdio' }, packageArguments: [{ type: 'positional', value: '--read-only' }], environmentVariables: [{ name: 'NOTES_TOKEN', description: 'API token', isSecret: true }] }] };
  expect(toComponentForm(npm)).toMatchObject({ kind: 'mcp', name: 'Notes', description: 'Team notes.', program: 'npx', args: '-y @acme/notes-mcp@1.2.0 --read-only' });
  expect(toComponentForm(npm)!.install).toMatch(/Node\.js.*NOTES_TOKEN/s);
  const pypi: RegistryServer = { name: 'io.github.iowarp/slurm-mcp', description: 'Slurm jobs', version: '2.2.0', packages: [{ registryType: 'pypi', identifier: 'clio-kit', version: '2.2.0', transport: { type: 'stdio' } }] };
  expect(toComponentForm(pypi)).toMatchObject({ kind: 'mcp', name: 'slurm-mcp', program: 'uvx', args: 'clio-kit==2.2.0' });
  const oci: RegistryServer = { name: 'io.github.acme/pbs', description: 'PBS', version: '1', packages: [{ registryType: 'oci', identifier: 'ghcr.io/acme/pbs-mcp:1', transport: { type: 'stdio' } }] };
  expect(toComponentForm(oci)).toMatchObject({ program: 'docker', args: 'run -i --rm ghcr.io/acme/pbs-mcp:1' });
});

it('makes a remote server a web address whose secret headers are named, never filled', () => {
  const remote: RegistryServer = { name: 'ai.smithery/x', description: 'Remote', version: '1', remotes: [{ type: 'streamable-http', url: 'https://server.example/mcp', headers: [{ name: 'Authorization', value: 'Bearer {smithery_api_key}', isSecret: true }, { name: 'X-Team', isSecret: true }] }] };
  expect(toComponentForm(remote)).toMatchObject({ kind: 'mcp-remote', url: 'https://server.example/mcp', headers: [['Authorization', 'Bearer ${SMITHERY_API_KEY}'], ['X-Team', '${X_TEAM}']] });
});

it('cannot add a server with no way to run it', () => {
  expect(toComponentForm({ name: 'a/b', description: 'x', version: '1', packages: [{ registryType: 'nuget', identifier: 'X', transport: { type: 'stdio' } }] })).toBeNull();
});
