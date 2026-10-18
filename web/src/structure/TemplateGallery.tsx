import { track } from '../hosted/analytics';
import {DismissibleError} from '../shell/DismissibleError';
import { workflowLinks, inputBox, stageBoxes } from '../workflow/WorkflowGraph';
import { TextArea } from '../shell/TextArea';
import { setKindAll } from '../draft/models';
import { KindSwitch } from './ModelControls';
import { diagnosticsOf, errorText } from '../errors';
import { ApiError } from '../api';
import type { DraftIssue } from '../generated/DraftIssue';
import { useEffect, useId, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { ImportRun } from './ImportRun';
import { api } from '../api';
import { useRequestOnce } from '../useRequestOnce';
import type { AgentKind } from '../generated/AgentKind';
import type { Diagnostic } from '../generated/Diagnostic';
import type { TeamDraft } from '../generated/TeamDraft';
import type { TeamImport } from '../generated/TeamImport';
import type { TemplateInfo } from '../generated/TemplateInfo';
import { Btn, Icon, type IconName } from '../shell/ui';
import { CARD_H, CARD_W } from '../layout';
import { isMadl } from './madl';
import { kindVendor } from './Panels';
import { plural } from '../format';
import { GenerateTeam } from './GenerateTeam';

const KIND_WORD: Record<AgentKind, string> = { claude: 'Claude', codex: 'Codex', practice: 'Practice' };
const kindIcon = (k: AgentKind): IconName => k;

/** "2 agents · Claude, Codex", or "2 Practice agents" when Practice is on. */
export function previewCaption(d: TeamDraft, practice: boolean): string {
  const n = d.agents.length;
  if (n === 0) return 'empty · add agents yourself';
  if (practice) return plural(n, 'Practice agent');
  const kinds = [...new Set(d.agents.map((a) => KIND_WORD[a.kind]))];
  return `${plural(n, 'agent')} · ${kinds.join(', ')}`;
}

/** A card name on one line, or on two smaller lines when it is long; never cut mid-word unless one word is too long. */
export function previewNameLines(name: string): { lines: string[]; small: boolean } {
  const n = name.trim();
  if (n.length <= 9) return { lines: [n], small: false };
  const MAX = 12;
  const cut = (w: string) => (w.length > MAX ? `${w.slice(0, MAX - 1)}…` : w);
  const words = n.split(/\s+/);
  if (words.length === 1) return { lines: [cut(n)], small: true };
  let first = words[0];
  let i = 1;
  while (i < words.length && `${first} ${words[i]}`.length <= MAX) first = `${first} ${words[i++]}`;
  const rest = words.slice(i).join(' ');
  return { lines: rest ? [cut(first), cut(rest)] : [cut(first)], small: true };
}

function PreviewName({ x, y, name }: { x: number; y: number; name: string }) {
  const { lines, small } = previewNameLines(name);
  if (!small) return <text x={x} y={y} className="tpl-name">{lines[0]}</text>;
  return (
    <text x={x} y={lines.length > 1 ? y - 12 : y} className="tpl-name ux-tpl-name-small">
      {lines.map((l, i) => <tspan key={i} x={x} dy={i === 0 ? 0 : 22}>{l}</tspan>)}
    </text>
  );
}

/** A small drawing of a template's agents and who hands work to whom. */
export function TemplatePreview({ draft, practice }: { draft: TeamDraft; practice: boolean }) {
  const arrowId=useId();
  if (draft.agents.length === 0)
    return (
      <svg className="tpl-svg" viewBox="0 0 240 120" aria-hidden="true">
        <rect x="70" y="30" width="100" height="60" rx="10" className="tpl-empty" />
        <text x="120" y="65" textAnchor="middle" className="tpl-plus">
          +
        </text>
      </svg>
    );
  const stages = Object.entries(stageBoxes(draft)).map(([id, b]) => ({ id, name: draft.workflow!.stages.find(s => `$stage:${s.id}` === id)?.name ?? id, kind: 'practice' as const, ...b }));
  const nodes = draft.workflow?.dispatcher ? [{id:'$workflow-input',name:'Input',kind:'practice' as const,...inputBox(draft)},...draft.agents,...stages] : draft.agents;
  const links = draft.workflow ? workflowLinks(draft) : draft.connections;
  const boxes=[...nodes.map(a=>({...a,w:CARD_W,h:CARD_H})),...draft.groups];
  const x0=Math.min(...boxes.map(b=>b.x))-24, y0=Math.min(...boxes.map(b=>b.y))-24;
  const w=Math.max(...boxes.map(b=>b.x+b.w))+24-x0, h=Math.max(...boxes.map(b=>b.y+b.h))+24-y0;
  const at = (id: string) => nodes.find((a) => a.id === id)!;
  return (
    <svg className="tpl-svg" viewBox={`${x0} ${y0} ${w} ${h}`} preserveAspectRatio="xMidYMid meet" aria-hidden="true">
      <defs>
        <marker id={arrowId} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="5" markerHeight="5" orient="auto-start-reverse">
          <path d="M0,0 L10,5 L0,10 z" className="tpl-arrowhead" />
        </marker>
      </defs>
      {draft.groups.map(g=><g key={g.id}><rect x={g.x} y={g.y} width={g.w} height={g.h} rx={18} className="tpl-empty" /><text x={g.x+16} y={g.y+28} className="tpl-name">{g.label}</text></g>)}
      {links.map((c) => {
        const a = at(c.from);
        const b = at(c.to);
        if (!a || !b) return null;
        const x1 = a.x + CARD_W;
        const y1 = a.y + CARD_H / 2;
        const x2 = b.x;
        const y2 = b.y + CARD_H / 2;
        const dx = Math.max(30, (x2 - x1) / 2);
        return <path key={c.id} className="tpl-edge" d={`M${x1},${y1} C${x1 + dx},${y1} ${x2 - dx},${y2} ${x2 - 6},${y2}`} markerEnd={`url(#${arrowId})`} />;
      })}
      {nodes.map((a) => {
        const v = kindVendor(practice ? 'practice' : a.kind);
        return (
          <g key={a.id} className={`tpl-agent ${v.cls}`}>
            <rect x={a.x} y={a.y} width={CARD_W} height={CARD_H} rx={22} />
            <rect x={a.x + 16} y={a.y + 14} width={36} height={36} rx={10} className="tpl-head" />
            <svg x={a.x + 22} y={a.y + 20} width={24} height={24} className="tpl-mark">
              <Icon name={kindIcon(practice ? 'practice' : a.kind)} size={24} strokeWidth={2.4} />
            </svg>
            <PreviewName x={a.x + 64} y={a.y + 42} name={a.name} />
            <line x1={a.x + 16} x2={a.x + CARD_W - 40} y1={a.y + 80} y2={a.y + 80} className="tpl-line" />
            <line x1={a.x + 16} x2={a.x + CARD_W - 70} y1={a.y + 108} y2={a.y + 108} className="tpl-line" />
            <line x1={a.x + 16} x2={a.x + CARD_W - 100} y1={a.y + 136} y2={a.y + 136} className="tpl-line" />
          </g>
        );
      })}
    </svg>
  );
}

function ImportFromFile({ onCreated }: { onCreated: (id: string) => void }) {
  const ids = useId();
  const [yaml, setYaml] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<{ message: string; diagnostics: Diagnostic[]; issues?: DraftIssue[] } | null>(null);
  const [done, setDone] = useState<TeamImport | null>(null);
  const importOnce = useRequestOnce();

  const readFile = async (f: File | undefined) => {
    if (f) setYaml(await f.text());
  };
  const run = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    setDone(null);
    try {
      // A MADL file (YAML or JSON), a Sculptured team file (JSON), or else an OpenRig YAML spec.
      const madl = isMadl(yaml);
      if (!madl && yaml.trim().startsWith('{')) {
        let file: unknown;
        try {
          file = JSON.parse(yaml);
        } catch (e) {
          setError({ message: `This is not valid JSON: ${(e as Error).message}`, diagnostics: [] });
          return;
        }
        const r = await importOnce((id) => api.importTeamFile(file, id));
        onCreated(r.team.draft.id);
        return;
      }
      const r = await importOnce((id) => (madl ? api.importMadl(yaml, id) : api.importTeam(yaml, id)));
      if (r.dropped.length === 0) onCreated(r.team.draft.id);
      else setDone(r);
    } catch (err) {
      setError({ message: errorText(err), diagnostics: diagnosticsOf(err), issues: err instanceof ApiError ? err.issues : undefined });
    } finally {
      setBusy(false);
    }
  };
  return (
    <form className="import-form" onSubmit={run} aria-label="Import a team from a file">
      <p className="muted">Import a MADL file (.madl.yaml), a Sculptured team file (.sculptured-team.json) or an OpenRig YAML file. Anything that can't be kept is listed.</p>
      <div className="import-line">
        <label htmlFor={`${ids}-file`} className="prompt">
          file
        </label>
        <input id={`${ids}-file`} type="file" accept=".json,.yaml,.yml,application/json,text/yaml" onChange={(e) => void readFile(e.target.files?.[0])} />
      </div>
      <label htmlFor={`${ids}-yaml`} className="sr-only">
        Team file contents
      </label>
      <TextArea id={`${ids}-yaml`} className="fld-input import-yaml" rows={6} value={yaml} spellCheck={false} placeholder="…or paste the file here" onChange={(e) => setYaml(e.target.value)} />
      <Btn type="submit" disabled={busy || !yaml.trim()}>
        Import
      </Btn>
      {error && (
        <DismissibleError className="tone-red">
          <p>{error.message}</p>
          {error.diagnostics.length > 0 && (
            <ul aria-label="Problems in the file">
              {error.diagnostics.map((d, i) => (
                <li key={i}>
                  <code>{d.path}</code> — {d.message}
                </li>
              ))}
            </ul>
          )}
          {error.issues && error.issues.length > 0 && (
            <ul aria-label="Issues in the team">
              {error.issues.map((i, n) => (
                <li key={n}>{i.message}</li>
              ))}
            </ul>
          )}
        </DismissibleError>
      )}
      {done && (
        <div role="status" className="import-dropped">
          <p>
            Imported {done.team.draft.name}. {plural(done.dropped.length, 'part was', 'parts were')} not imported:
          </p>
          <ul aria-label="Not imported">
            {done.dropped.map((d, i) => (
              <li key={i}>
                <code>{d.path}</code> — {d.message}
              </li>
            ))}
          </ul>
          <Btn onClick={() => onCreated(done.team.draft.id)}>Open {done.team.draft.name}</Btn>
        </div>
      )}
    </form>
  );
}

/** The starting teams a newcomer sees first; every other template sits under More templates. */
export const FEATURED_TEMPLATES = ['solo', 'builder-reviewer', 'planner-builders', 'blank'];
const MORE_KEY = 'sculptured.gallery.more.v1';

/**
 * A template's blurb as the gallery shows it. The gallery runs every agent on the kind chosen
 * above it, so a blurb naming a kind ("Codex reviews") would be wrong; course references are
 * dropped. Unknown templates keep the server's words.
 */
export function plainSummary(t: Pick<TemplateInfo, 'id' | 'summary'>): string {
  const own: Record<string, string> = {
    solo: 'One agent with one job. The simplest way to start.',
    'builder-reviewer': 'One agent builds, then hands the work to a second agent to review.',
    'planner-builders': 'One planner splits the work and hands it to two builders.',
  };
  return own[t.id] ?? t.summary.replace(/\s*CS2680[^.]*\.?/g, '').trim();
}

function readMore(): boolean {
  try { return globalThis.localStorage?.getItem(MORE_KEY) === 'true'; } catch { return false; }
}

/**
 * New project: the template gallery. A few starting teams come first, the rest sit under More
 * templates, Describe it is the second way in, and file import stays under Advanced. Every agent
 * runs on the kind chosen above the cards (Claude by default); Practice agents are not offered
 * here. Choosing a template creates the team and opens its Design board.
 */
export function TemplateGallery({ onCreated, onClose, inline, flow = inline, advanced }: { onCreated: (id: string, open?: { history?: boolean }) => void; onClose?: () => void; inline?: boolean; /** The three steps of a run, for a first visit. */ flow?: boolean; /** More expert ways in, under Advanced. */ advanced?: ReactNode }) {
  const [describing, setDescribing] = useState(false);
  const ids = useId();
  const [templates, setTemplates] = useState<TemplateInfo[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [kind, setKind] = useState<AgentKind>('claude');
  const practice = kind === 'practice';
  const [name, setName] = useState('');
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [moreOpen, setMoreOpen] = useState(readMore);
  const createOnce = useRequestOnce();
  /** The template as it will be created: every agent on the chosen kind. */
  const as = (d: TeamDraft) => (practice ? d : setKindAll(d, kind));
  const box = useRef<HTMLDivElement>(null);
  const opener = useRef<Element | null>(null);

  useEffect(() => {
    let live = true;
    opener.current = document.activeElement;
    Promise.resolve()
      .then(() => api.templates())
      .then((t) => live && setTemplates(Array.isArray(t) ? t : []))
      .catch((e: Error) => live && setLoadError(errorText(e)));
    return () => {
      live = false;
      if (!inline && opener.current instanceof HTMLElement && document.contains(opener.current)) opener.current.focus();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (templates && !inline) box.current?.querySelector<HTMLElement>('.tpl-card')?.focus();
  }, [templates, inline]);

  const create = async (t: TemplateInfo) => {
    setBusy(t.id);
    setError(null);
    try {
      const rec = await createOnce((id) => api.createTeam(t.id, practice, name.trim() || null, id));
      if (!practice && rec.draft.agents.some((a) => a.kind !== kind)) await api.saveTeam(rec.draft.id, rec.draft.revision, as(rec.draft));
      onCreated(rec.draft.id);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  const featured = templates?.filter((t) => FEATURED_TEMPLATES.includes(t.id)) ?? [];
  const more = templates?.filter((t) => !FEATURED_TEMPLATES.includes(t.id)) ?? [];
  const card = (t: TemplateInfo) => (
    <li key={t.id}>
      <button type="button" className="tpl-card" onClick={() => create(t)} disabled={!!busy} aria-labelledby={`${ids}-${t.id}-t`} aria-describedby={`${ids}-${t.id}`}>
        <TemplatePreview draft={as(t.preview)} practice={practice} />
        <span className="tpl-title" id={`${ids}-${t.id}-t`}>
          {t.title}
        </span>
        <span id={`${ids}-${t.id}`} className="tpl-summary">
          {plainSummary(t)} <span className="tpl-caption">{previewCaption(as(t.preview), practice)}</span>
        </span>
        {busy === t.id && <span className="tpl-busy">creating…</span>}
      </button>
    </li>
  );
  const kindWord = kind === 'codex' ? 'Codex' : 'Claude';

  const body = (
    <div
      ref={box}
      className={`panel gallery ux-gallery${inline ? ' inline' : ''}`}
      role={inline ? 'region' : 'dialog'}
      aria-modal={inline ? undefined : true}
      aria-labelledby={`${ids}-title`}
      onKeyDown={(e) => {
        if (e.key === 'Escape' && onClose) {
          e.stopPropagation();
          onClose();
        }
      }}
    >
      <div className="panel-bar">
        <span className="panel-title" id={`${ids}-title`}>
          New project
        </span>
        <span className="panel-rule" aria-hidden="true" />
        {onClose && (
          <Btn onClick={onClose} variant="ghost" icon="x" iconOnly aria-label="Close" kbd="esc" />
        )}
      </div>
      <h2 className="ux-gallery-lead">Pick a team to start from</h2>
      <p className="gallery-intro muted">You can change every agent, job, folder and connection on the Design board. Nothing runs until you press Start team.</p>
      <div className="gallery-opts ux-gallery-opts">
        <span className="gallery-kind">
          <span className="prompt">Agents run on</span>
          <KindSwitch label="Agents run on" value={kind} onChange={setKind} />
        </span>
        <span className="gallery-name">
          <label htmlFor={`${ids}-name`} className="prompt">
            Name
          </label>
          <input id={`${ids}-name`} className="field" value={name} spellCheck={false} placeholder="optional · rename any time" onChange={(e) => setName(e.target.value)} />
        </span>
        {!practice && <p className="gallery-warn tone-amber">{kindWord} agents use your own CLI logins and may cost money once started. You can switch any agent later.</p>}
      </div>
      {loadError && (
        <DismissibleError className="tone-red pad">
          Could not load templates: {loadError}
        </DismissibleError>
      )}
      {!templates && !loadError && (
        <p role="status" className="muted pad">
          Loading templates…
        </p>
      )}
      <ul className="tpl-grid ux-featured" aria-label="Templates">
        {featured.map(card)}
      </ul>
      {more.length > 0 && (
        <details className="ux-more-templates" open={moreOpen} onToggle={(e) => {
          const open = e.currentTarget.open;
          setMoreOpen(open);
          try { localStorage.setItem(MORE_KEY, String(open)); } catch { /* Per-browser convenience only. */ }
        }}>
          <summary>More templates ({more.length})</summary>
          <ul className="tpl-grid" aria-label="More templates">
            {more.map(card)}
          </ul>
        </details>
      )}
      {error && (
        <DismissibleError className="tone-red pad">
          Could not create the team: {error}
        </DismissibleError>
      )}
      <div className="ux-gallery-secondary">
        {describing ? (
          <GenerateTeam onCreated={(id) => onCreated(id)} onCancel={() => setDescribing(false)} />
        ) : (
          <p className="gallery-describe">
            <span>Rather describe what you need?</span>{' '}
            <Btn icon="bot" onClick={() => { track('describe-it'); setDescribing(true); }}>
              Describe it
            </Btn>{' '}
            <span className="muted">Your own Claude Code or Codex drafts the team for you to review.</span>
          </p>
        )}
      </div>
      {flow && (
        <ol className="flow" aria-label="How a team runs">
          <li>
            <span className="flow-n">1</span>
            <span className="flow-t">Design</span>
            <span className="flow-d">Give each agent a job and a folder, and connect who hands work to whom.</span>
          </li>
          <li>
            <span className="flow-n">2</span>
            <span className="flow-t">Start team</span>
            <span className="flow-d">Every agent starts, in order, and gets its job as its first message.</span>
          </li>
          <li>
            <span className="flow-n">3</span>
            <span className="flow-t">Run</span>
            <span className="flow-d">Watch each agent work, answer the ones that need you, and message any of them.</span>
          </li>
        </ol>
      )}
      <details className="gallery-advanced" data-advanced>
        <summary>Advanced: import a team file or a past run</summary>
        <ImportFromFile onCreated={onCreated} />
        <ImportRun onCreated={onCreated} />
        {advanced}
      </details>
    </div>
  );
  if (inline) return body;
  return (
    <div className="overlay" onPointerDown={(e) => e.target === e.currentTarget && onClose?.()}>
      {body}
    </div>
  );
}
