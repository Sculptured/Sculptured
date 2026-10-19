import { useEffect, useState } from 'react';
import { api, type GeneratedTeam } from '../api';
import { errorText } from '../errors';
import type { TeamDraft } from '../generated/TeamDraft';
import { Btn } from '../shell/ui';
import type { Edit } from './Panels';

type Cli = 'claude' | 'codex';
const CLI_NAME: Record<Cli, string> = { claude: 'Claude Code', codex: 'Codex' };

export type DiffRow = { change: 'added' | 'removed' | 'changed'; what: string };

const key = (s: string) => s.trim().toLowerCase();

/** What applying `proposed` would change, by agent name (ids differ between the two) and arrow. */
export function diffTeams(current: TeamDraft, proposed: TeamDraft): DiffRow[] {
  const byName = new Map(current.agents.map((a) => [key(a.name), a]));
  const kept = new Set(proposed.agents.map((a) => key(a.name)));
  const rows: DiffRow[] = proposed.agents.flatMap((a): DiffRow[] => {
    const was = byName.get(key(a.name));
    if (!was) return [{ change: 'added', what: a.name }];
    const same = was.job === a.job && was.kind === a.kind && was.advanced.model === a.advanced.model;
    return same ? [] : [{ change: 'changed', what: a.name }];
  });
  rows.push(...current.agents.filter((a) => !kept.has(key(a.name))).map((a): DiffRow => ({ change: 'removed', what: a.name })));
  const arrows = (t: TeamDraft) => {
    const name = new Map(t.agents.map((a) => [a.id, a.name]));
    return t.connections.map((c) => `arrow ${name.get(c.from) ?? c.from} → ${name.get(c.to) ?? c.to}`);
  };
  const [before, after] = [arrows(current), arrows(proposed)];
  rows.push(...after.filter((a) => !before.some((b) => key(b) === key(a))).map((what): DiffRow => ({ change: 'added', what })));
  rows.push(...before.filter((b) => !after.some((a) => key(a) === key(b))).map((what): DiffRow => ({ change: 'removed', what })));
  return rows;
}

/** The team's imported run, if it has one (History numbers imported runs below zero). */
function useImportedRun(teamId: string): number | null {
  const [seq, setSeq] = useState<number | null>(null);
  useEffect(() => {
    Promise.resolve()
      .then(() => api.deployments(teamId))
      .then((runs) => setSeq(runs?.find((r) => r.source?.startsWith('imported:'))?.start_seq ?? null), () => setSeq(null));
  }, [teamId]);
  return seq;
}

/**
 * Advanced, for a team rebuilt from a Claude Code or Codex session: the person's own CLI proposes a better,
 * reusable version from a summary of the session. The proposal is shown as a diff, every row labelled
 * AI-reconstructed, and changes nothing until Apply.
 */
export function ImproveFromRun({ teamId, draft, edit, locked = false }: { teamId: string; draft: TeamDraft; edit: Edit; locked?: boolean }) {
  const startSeq = useImportedRun(teamId);
  const [open, setOpen] = useState(false);
  const [offer, setOffer] = useState<GeneratedTeam | null>(null);
  if (startSeq === null) return null;
  const apply = (p: TeamDraft) => {
    edit((d) => ({ ...d, agents: p.agents, connections: p.connections, groups: p.groups, ...(p.workflow ? { workflow: p.workflow } : {}) }), 'improve-from-run');
    setOffer(null);
    setOpen(false);
  };
  return (
    <div className="improve-from-run">
      {!open && <Btn disabled={locked} onClick={() => setOpen(true)}>Improve this team with AI</Btn>}
      {open && !offer && <Ask teamId={teamId} startSeq={startSeq} first={draft.agents[0]?.kind === 'codex' ? 'codex' : 'claude'} onOffer={setOffer} onCancel={() => setOpen(false)} />}
      {offer && <Offer current={draft} offer={offer} onApply={() => apply(offer.draft)} onDiscard={() => { setOffer(null); setOpen(false); }} />}
    </div>
  );
}

function Ask({ teamId, startSeq, first, onOffer, onCancel }: { teamId: string; startSeq: number; first: Cli; onOffer: (o: GeneratedTeam) => void; onCancel: () => void }) {
  const [cli, setCli] = useState<Cli>(first);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const ask = () => {
    setBusy(true);
    setError('');
    api.improveFromRun(teamId, startSeq, { cli, model: null }).then(onOffer, (e: unknown) => setError(errorText(e))).finally(() => setBusy(false));
  };
  return (
    <div className="improve-ask" role="group" aria-label="Improve this team with AI">
      <label>Ask <select value={cli} disabled={busy} onChange={(e) => setCli(e.target.value as Cli)}><option value="claude">Claude Code</option><option value="codex">Codex</option></select></label>
      <p className="muted">Uses your own {CLI_NAME[cli]}; it reads a summary of the session (no file contents): each agent, what it was asked, which tools it used, and what it said last.</p>
      <div className="import-actions">
        <Btn variant="primary" disabled={busy} onClick={ask}>Ask {CLI_NAME[cli]}</Btn>
        <Btn disabled={busy} onClick={onCancel}>Cancel</Btn>
      </div>
      {busy && <p role="status">Asking {CLI_NAME[cli]}… this usually takes under a minute.</p>}
      {error && <p className="tone-red" role="alert">{error}</p>}
    </div>
  );
}

function Offer({ current, offer, onApply, onDiscard }: { current: TeamDraft; offer: GeneratedTeam; onApply: () => void; onDiscard: () => void }) {
  const rows = diffTeams(current, offer.draft);
  return (
    <div className="improve-offer" role="group" aria-label="What the AI proposes">
      <p>{offer.rationale}</p>
      {rows.length === 0 ? <p className="muted">It proposes no changes.</p> : (
        <ul aria-label="Proposed changes">
          {rows.map((r) => (
            <li key={`${r.change} ${r.what}`}><span className={`diff-${r.change}`}>{r.change}</span> {r.what} <span className="badge ai-badge">AI-reconstructed</span></li>
          ))}
        </ul>
      )}
      <div className="import-actions">
        <Btn variant="primary" disabled={rows.length === 0} onClick={onApply}>Apply</Btn>
        <Btn onClick={onDiscard}>Discard</Btn>
      </div>
    </div>
  );
}
