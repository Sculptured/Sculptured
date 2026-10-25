import { useEffect, useId, useRef, useState } from 'react';
import { api, type StatusPage, type TeamStatus } from '../api';
import { errorText } from '../errors';
import { shortAge } from '../herdr';
import { Markdown } from '../shell/Markdown';
import { Btn } from '../shell/ui';
import { usePolled } from '../usePolling';
import '../shell/chat.css';
import './status-page.css';

/** How often the page is read again while it is open: often enough to follow it as it changes. */
export const STATUS_POLL_MS = 3000;

const when = (ms: number) => new Date(ms).toLocaleString();

/** "Written by Reviewer · agent-reported · 14:05 (4m ago) · ✓ Final version". */
function About({ page, latest }: { page: StatusPage; latest: boolean }) {
  const age = shortAge(page.at_ms, Date.now());
  return (
    <p className="sp-about">
      <span>Written by <b>{page.author_name}</b></span>
      <span className="muted">agent-reported</span>
      <span className="muted">{latest ? 'updated' : 'written'} {when(page.at_ms)}{age === 'now' ? ' (just now)' : ` (${age} ago)`}</span>
      {page.done
        ? <span className="sp-state sp-done"><span aria-hidden="true">✓ </span>Final version</span>
        : latest && <span className="sp-state sp-live"><span aria-hidden="true">◌ </span>Kept up to date while the team works</span>}
    </p>
  );
}

function Versions({ s, picked, onPick }: { s: TeamStatus; picked: number | null; onPick: (seq: number | null) => void }) {
  const ids = useId();
  if (s.versions.length < 2) return null;
  return (
    <span className="sp-versions">
      <label htmlFor={`${ids}-v`} className="muted">Version</label>
      <select id={`${ids}-v`} className="fld-input" value={picked ?? ''} onChange={(e) => onPick(e.target.value ? Number(e.target.value) : null)}>
        <option value="">Latest</option>
        {s.versions.slice(1).map((v) => <option key={v.seq} value={v.seq}>{when(v.at_ms)} · {v.author_name}{v.done ? ' · final' : ''}</option>)}
      </select>
    </span>
  );
}

/** An earlier version, read once when it is picked. */
function useVersion(rig: string, seq: number | null): { page: StatusPage | null; error: string } {
  const [got, setGot] = useState<{ seq: number; page: StatusPage | null; error: string } | null>(null);
  useEffect(() => {
    if (seq === null) return;
    let live = true;
    api.teamStatusVersion(rig, seq).then(
      (page) => { if (live) setGot({ seq, page, error: '' }); },
      (e) => { if (live) setGot({ seq, page: null, error: errorText(e) }); },
    );
    return () => { live = false; };
  }, [rig, seq]);
  return got && got.seq === seq ? got : { page: null, error: '' };
}

function Empty({ s }: { s: TeamStatus }) {
  if (!s.owner) return <p className="muted">Add an agent to the team: one of them writes this page.</p>;
  return (
    <div className="sp-empty">
      <p>No status page yet.</p>
      <p className="muted">{s.owner.name} writes it while the team works, and once more when the work is done{s.owner.chosen ? '' : ' (chosen automatically: the agent at the end of the line)'}. To choose another agent, open it and turn on <b>Status page</b> under Advanced.</p>
    </div>
  );
}

/**
 * Results → Status: the one page the team's owner agent keeps saying where the work stands and what
 * changed, so someone coming back later can read it instead of the transcripts. It is re-read every few
 * seconds while open; earlier versions stay readable.
 */
export function StatusPageView({ rig, onClose }: { rig: string; onClose: () => void }) {
  const { data, error } = usePolled(() => api.teamStatus(rig), STATUS_POLL_MS, [rig]);
  const [picked, setPicked] = useState<number | null>(null);
  const old = useVersion(rig, picked);
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => { heading.current?.focus(); setPicked(null); }, [rig]);
  const page = picked === null ? data?.page ?? null : old.page;
  return (
    <section className="status-page" aria-label="Status" onKeyDown={(e) => { if (e.key === 'Escape') { e.stopPropagation(); onClose(); } }}>
      <header>
        <h2 ref={heading} tabIndex={-1}>Status</h2>
        {data && <Versions s={data} picked={picked} onPick={setPicked} />}
        <Btn onClick={onClose}>Back to board</Btn>
      </header>
      {(error || old.error) && <p role="alert">{old.error || error}</p>}
      {!data && !error && <p className="muted">Loading…</p>}
      {data && !data.page && <Empty s={data} />}
      {picked !== null && page && (
        <p className="sp-old" role="status">An earlier version. <Btn size="sm" onClick={() => setPicked(null)}>Show the latest</Btn></p>
      )}
      {page && (
        <article className="sp-page" aria-label="Status page">
          <About page={page} latest={picked === null} />
          <div className="sp-body"><Markdown text={page.markdown} /></div>
        </article>
      )}
    </section>
  );
}
