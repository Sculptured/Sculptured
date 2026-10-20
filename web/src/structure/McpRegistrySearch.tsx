import { useState } from 'react';
import { Btn } from '../shell/ui';
import { searchRegistry, toComponentForm, type RegistryForm, type RegistryServer } from './mcpRegistry';

/** Searches the official MCP Registry; "Use" fills the new-component form with the server, to check before adding. */
export function McpRegistrySearch({ onUse, onClose }: { onUse: (f: RegistryForm) => void; onClose: () => void }) {
  const [query, setQuery] = useState('');
  const [found, setFound] = useState<RegistryServer[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const search = () => {
    if (!query.trim()) return;
    setBusy(true); setError('');
    searchRegistry(query).then(setFound, (e: Error) => setError(e.message)).finally(() => setBusy(false));
  };
  return (
    <div className="component-new" role="group" aria-label="MCP Registry">
      <p className="fld-hint muted">Servers listed in the official <a href="https://registry.modelcontextprotocol.io" target="_blank" rel="noreferrer">MCP Registry</a>. Anyone can list one there: read what it runs before you add it.</p>
      <div className="fld-actions">
        <input type="search" className="fld-input" aria-label="Search the MCP Registry" placeholder="slurm, github, postgres…" value={query} onChange={(e) => setQuery(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter') search(); }} />
        <Btn size="sm" onClick={search} disabled={busy}>Search</Btn>
        <Btn size="sm" onClick={onClose}>Close</Btn>
      </div>
      {busy && <p role="status" className="muted">Searching…</p>}
      {error && <p role="alert" className="fld-error">{error}</p>}
      {found && found.length === 0 && <p className="muted">Nothing found.</p>}
      {found && found.length > 0 && <ul className="components-list">{found.map((s) => <Result key={s.name} s={s} onUse={onUse} />)}</ul>}
    </div>
  );
}

function Result({ s, onUse }: { s: RegistryServer; onUse: (f: RegistryForm) => void }) {
  const form = toComponentForm(s);
  return (
    <li>
      <span className="row"><b>{form?.name ?? s.name}</b>
        {form ? <Btn size="sm" aria-label={`Use ${form.name}`} onClick={() => onUse(form)}>Use</Btn> : <span className="muted">no way to run it here</span>}
      </span>
      <span className="muted">{s.description}</span>
      <span className="muted"><code>{s.name}</code> {s.version}{form ? <> · <code>{form.kind === 'mcp' ? `${form.program} ${form.args}` : form.url}</code></> : null}</span>
    </li>
  );
}
