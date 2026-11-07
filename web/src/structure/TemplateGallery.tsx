import { track } from '../hosted/analytics';
import {DismissibleError} from '../shell/DismissibleError';
import { setKindAll } from '../draft/models';
import { KindSwitch } from './ModelControls';
import { errorText } from '../errors';
import { useEffect, useId, useRef, useState } from 'react';
import type { ReactNode, RefObject } from 'react';
import { ImportRun } from './ImportRun';
import { api } from '../api';
import { useRequestOnce } from '../useRequestOnce';
import type { AgentKind } from '../generated/AgentKind';
import type { TeamDraft } from '../generated/TeamDraft';
import type { TemplateInfo } from '../generated/TemplateInfo';
import { Btn } from '../shell/ui';
import { GenerateTeam } from './GenerateTeam';
import { ImportFromFile } from './ImportFromFile';
import { previewCaption, TemplatePreview } from './TemplatePreview';

export { TemplatePreview, previewCaption, previewNameLines } from './TemplatePreview';

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

/** The templates, read once on open; the dialog form returns focus to what opened it. */
function useTemplates(inline: boolean | undefined) {
  const [templates, setTemplates] = useState<TemplateInfo[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
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
  return { templates, loadError };
}

type CardProps = { t: TemplateInfo; ids: string; busy: string | null; as: (d: TeamDraft) => TeamDraft; onCreate: (t: TemplateInfo) => void };

function TemplateCard({ t, ids, busy, as, onCreate }: CardProps) {
  return (
    <li>
      <button type="button" className="tpl-card" onClick={() => onCreate(t)} disabled={!!busy} aria-labelledby={`${ids}-${t.id}-t`} aria-describedby={`${ids}-${t.id}`}>
        <TemplatePreview draft={as(t.preview)} />
        <span className="tpl-title" id={`${ids}-${t.id}-t`}>
          {t.title}
        </span>
        <span id={`${ids}-${t.id}`} className="tpl-summary">
          {plainSummary(t)} <span className="tpl-caption">{previewCaption(as(t.preview))}</span>
        </span>
        {busy === t.id && <span className="tpl-busy">creating…</span>}
      </button>
    </li>
  );
}

function MoreTemplates({ more, card }: { more: TemplateInfo[]; card: (t: TemplateInfo) => ReactNode }) {
  const [moreOpen, setMoreOpen] = useState(readMore);
  if (more.length === 0) return null;
  return (
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
  );
}

function GalleryOptions({ ids, kind, setKind, name, setName }: { ids: string; kind: AgentKind; setKind: (k: AgentKind) => void; name: string; setName: (n: string) => void }) {
  return (
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
      <p className="gallery-warn tone-amber">{kind === 'codex' ? 'Codex' : 'Claude'} agents use your own CLI logins and may cost money once started. You can switch any agent later.</p>
    </div>
  );
}

function DescribeIt({ onCreated }: { onCreated: (id: string) => void }) {
  const [describing, setDescribing] = useState(false);
  if (describing) return <GenerateTeam onCreated={(id) => onCreated(id)} onCancel={() => setDescribing(false)} />;
  return (
    <p className="gallery-describe">
      <span>Rather describe what you need?</span>{' '}
      <Btn icon="bot" onClick={() => { track('describe-it'); setDescribing(true); }}>
        Describe it
      </Btn>{' '}
      <span className="muted">Your own Claude Code or Codex drafts the team for you to review.</span>
    </p>
  );
}

const FLOW = [
  ['Design', 'Give each agent a job and a folder, and connect who hands work to whom.'],
  ['Start team', 'Every agent starts, in order, and gets its job as its first message.'],
  ['Run', 'Watch each agent work, answer the ones that need you, and message any of them.'],
];

function HowItRuns() {
  return (
    <ol className="flow" aria-label="How a team runs">
      {FLOW.map(([title, text], i) => (
        <li key={title}>
          <span className="flow-n">{i + 1}</span>
          <span className="flow-t">{title}</span>
          <span className="flow-d">{text}</span>
        </li>
      ))}
    </ol>
  );
}

/** Creates the chosen template as a team, every agent on the chosen kind. */
function useCreateTeam(kind: AgentKind, name: string, onCreated: (id: string) => void) {
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const createOnce = useRequestOnce();
  /** The template as it will be created: every agent on the chosen kind. */
  const as = (d: TeamDraft) => setKindAll(d, kind);
  const create = async (t: TemplateInfo) => {
    setBusy(t.id);
    setError(null);
    try {
      const rec = await createOnce((id) => api.createTeam(t.id, name.trim() || null, id));
      if (rec.draft.agents.some((a) => a.kind !== kind)) await api.saveTeam(rec.draft.id, rec.draft.revision, as(rec.draft));
      onCreated(rec.draft.id);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };
  return { busy, error, as, create };
}

type Props = { onCreated: (id: string, open?: { history?: boolean }) => void; onClose?: () => void; inline?: boolean; /** The three steps of a run, for a first visit. */ flow?: boolean; /** More expert ways in, under Advanced. */ advanced?: ReactNode };

/** The gallery's frame: an inline panel, or a dialog over the page that Esc and a press outside close. */
function GalleryFrame({ ids, inline, onClose, box, children }: { ids: string; inline?: boolean; onClose?: () => void; box: RefObject<HTMLDivElement | null>; children: ReactNode }) {
  const body = (
    <div ref={box} className={`panel gallery ux-gallery${inline ? ' inline' : ''}`} role={inline ? 'region' : 'dialog'} aria-modal={inline ? undefined : true} aria-labelledby={`${ids}-title`}
      onKeyDown={(e) => { if (e.key === 'Escape' && onClose) { e.stopPropagation(); onClose(); } }}>
      <div className="panel-bar">
        <span className="panel-title" id={`${ids}-title`}>
          New project
        </span>
        <span className="panel-rule" aria-hidden="true" />
        {onClose && <Btn onClick={onClose} variant="ghost" icon="x" iconOnly aria-label="Close" kbd="esc" />}
      </div>
      {children}
    </div>
  );
  if (inline) return body;
  return (
    <div className="overlay" onPointerDown={(e) => e.target === e.currentTarget && onClose?.()}>
      {body}
    </div>
  );
}

/**
 * New project: the template gallery. A few starting teams come first, the rest sit under More
 * templates, Describe it is the second way in, and file import stays under Advanced. Every agent
 * runs on the kind chosen above the cards (Claude by default). Choosing a template creates the team and opens its Design board.
 */
export function TemplateGallery({ onCreated, onClose, inline, flow = inline, advanced }: Props) {
  const ids = useId();
  const { templates, loadError } = useTemplates(inline);
  const [kind, setKind] = useState<AgentKind>('claude');
  const [name, setName] = useState('');
  const make = useCreateTeam(kind, name, onCreated);
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (templates && !inline) box.current?.querySelector<HTMLElement>('.tpl-card')?.focus();
  }, [templates, inline]);
  const featured = templates?.filter((t) => FEATURED_TEMPLATES.includes(t.id)) ?? [];
  const more = templates?.filter((t) => !FEATURED_TEMPLATES.includes(t.id)) ?? [];
  const card = (t: TemplateInfo) => <TemplateCard key={t.id} t={t} ids={ids} busy={make.busy} as={make.as} onCreate={make.create} />;
  return (
    <GalleryFrame ids={ids} inline={inline} onClose={onClose} box={box}>
      <h2 className="ux-gallery-lead">Pick a team to start from</h2>
      <p className="gallery-intro muted">You can change every agent, job, folder and connection on the Design board. Nothing runs until you press Start team.</p>
      <GalleryOptions ids={ids} kind={kind} setKind={setKind} name={name} setName={setName} />
      {loadError && <DismissibleError className="tone-red pad">Could not load templates: {loadError}</DismissibleError>}
      {!templates && !loadError && <p role="status" className="muted pad">Loading templates…</p>}
      <ul className="tpl-grid ux-featured" aria-label="Templates">
        {featured.map(card)}
      </ul>
      <MoreTemplates more={more} card={card} />
      {make.error && <DismissibleError className="tone-red pad">Could not create the team: {make.error}</DismissibleError>}
      <div className="ux-gallery-secondary"><DescribeIt onCreated={(id) => onCreated(id)} /></div>
      {flow && <HowItRuns />}
      <details className="gallery-advanced" data-advanced>
        <summary>Advanced: import a team file or a past run</summary>
        <ImportFromFile onCreated={onCreated} />
        <ImportRun onCreated={onCreated} />
        {advanced}
      </details>
    </GalleryFrame>
  );
}
