import { DismissibleError } from '../shell/DismissibleError';
import { ContextOverview } from './ContextOverview';
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { EventRecord } from '../generated/EventRecord';
import { type AgentState } from '../herdr';
import { useSeatEvents, GAP_NOTICE } from '../useSeatEvents';
import { plural } from '../format';
import { scrollWithin } from '../shell/highlight';
import { Segmented } from '../shell/ui';
import { deriveProcess, type Process } from './process';
import { Files } from './VisualFiles';
import { Checks } from './VisualChecks';
import { NowBar, Summary } from './VisualSummary';
import { PhaseCard, ProcessList, Waiting } from './VisualPhases';
import './visual.css';

export { plainLine } from './visualParts';

/** Phases drawn at first; earlier ones wait behind "Show earlier". */
const PHASES = 30;

type Mode = 'diagram' | 'list';
type ViewState = { mode: Mode; file: string | null; all: boolean; scroll: number };

/** Per-agent view state, kept while you visit the Transcript and come back. */
const VIEWS = new Map<string, ViewState>();

type Props = {
  seatId: string;
  feed: EventRecord[];
  epoch: number;
  /** The agent's current state, from its projected status (◆ observed). */
  state: AgentState;
  /** Show this transcript item in the Transcript tab. */
  onJump: (key: string) => void;
};

/** The per-agent view (diagram or list, selected file, scroll), remembered across visits. */
function useViewState(seatId: string, loading: boolean) {
  const [view, setView] = useState<ViewState>(VIEWS.get(seatId) ?? { mode: 'diagram', file: null, all: false, scroll: 0 });
  const scroller = useRef<HTMLDivElement>(null);
  const restored = useRef(false);
  const update = (patch: Partial<ViewState>) =>
    setView((v) => {
      const next = { ...v, ...patch };
      VIEWS.set(seatId, next);
      return next;
    });
  // Come back to where you were once the events are in.
  useLayoutEffect(() => {
    if (restored.current || loading || !scroller.current) return;
    restored.current = true;
    scroller.current.scrollTop = view.scroll;
  }, [loading, view.scroll]);
  const onScroll = () => {
    const el = scroller.current;
    if (!el || !restored.current) return;
    VIEWS.set(seatId, { ...(VIEWS.get(seatId) ?? view), scroll: el.scrollTop });
  };
  return { view, update, scroller, onScroll };
}

/** A clock for "2m ago" labels, ticking every 15 seconds. */
function useNow() {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 15_000);
    return () => clearInterval(t);
  }, []);
  return now;
}

type BodyProps = { p: Process; mode: Mode; loading: boolean; hidden: number; now: number; onJump: (key: string) => void };

/** The process: its turns as cards or as a list, or why there are none. */
function ProcessBody({ p, mode, loading, hidden, now, onJump }: BodyProps) {
  const phases = p.phases.slice(hidden);
  if (p.phases.length === 0) return <p className="muted vp-empty">{loading ? 'Loading…' : 'No turns yet. The process appears here as the agent works.'}</p>;
  if (mode === 'list') return <ProcessList phases={phases} onJump={onJump} />;
  return (
    <ol className="vp-phases">
      {phases.map((ph, i) => (
        <PhaseCard key={ph.id} p={ph} onJump={onJump} now={now} last={i === phases.length - 1} />
      ))}
    </ol>
  );
}

function ProcessPanel({ p, view, update, hidden, loading, now, onJump }: Omit<BodyProps, 'mode'> & { view: ViewState; update: (patch: Partial<ViewState>) => void }) {
  return (
    <section className="vp-panel vp-process" aria-label="Process">
      <div className="vp-panel-head">
        <h3>Process</h3>
        <span className="vp-panel-count">{plural(p.summary.turns, 'turn')}</span>
        <span className="pane-spacer" />
        <Segmented<Mode>
          label="Process view"
          value={view.mode}
          onChange={(mode) => update({ mode })}
          options={[
            { value: 'diagram', label: 'Diagram', icon: 'route' },
            { value: 'list', label: 'List', icon: 'list' },
          ]}
        />
      </div>
      <p className="vp-note">
        Captured work, reported progress and next actions, turn by turn. Inspect a summary for tools and the agent’s stated explanations. Tool calls (<span className="tier-inline tier-observed">◆ observed</span>) and its last reply (
        <span className="tier-inline tier-reported">◇ reported</span>). Select any part to see it in the transcript.
      </p>
      {hidden > 0 && (
        <button type="button" className="show-earlier" onClick={() => update({ all: true })}>
          Show {hidden} earlier turns
        </button>
      )}
      <ProcessBody p={p} mode={view.mode} loading={loading} hidden={hidden} now={now} onJump={onJump} />
      <Waiting items={p.waiting} onJump={onJump} />
    </section>
  );
}

/** What the history load says above the picture: an error, or what was left out. */
function LoadNotices({ error, truncated, gap }: { error: string | null; truncated: boolean; gap: boolean }) {
  return <>
    {error && (
      <DismissibleError className="tone-red pad">
        Could not load history: {error}
      </DismissibleError>
    )}
    {truncated && <p className="notice-line">Older events not loaded: the picture starts partway through.</p>}
    {gap && <p className="notice-line tone-amber">{GAP_NOTICE}</p>}
  </>;
}

/**
 * The agent's process as a picture: one phase per turn (what started it, its tool calls as
 * nodes, its reply), the files it read and changed, and the checks it ran. Built from the same
 * events as the Transcript; every node opens the matching point there.
 */
export function VisualProgress({ seatId, feed, epoch, state, onJump }: Props) {
  const { events, loading, error, truncated, gap } = useSeatEvents(seatId, feed, epoch);
  const p = useMemo(() => deriveProcess(events), [events]);
  const { view, update, scroller, onScroll } = useViewState(seatId, loading);
  const now = useNow();
  const hidden = view.all ? 0 : Math.max(0, p.phases.length - PHASES);
  const showTurn = (n: number) => {
    const el = scroller.current?.querySelector<HTMLElement>(`[data-phase="${n}"]`);
    scrollWithin(scroller.current, el ?? null, 'start');
    el?.querySelector<HTMLElement>('.vp-trigger, .vp-node')?.focus({ preventScroll: true });
  };
  return (
    <div className="vp" ref={scroller} onScroll={onScroll} data-testid="visual-progress">
      <LoadNotices error={error} truncated={truncated} gap={gap} />
      <Summary p={p} state={state} now={now} />
      <ContextOverview seatId={seatId} events={events} partial={truncated || gap || !!error} loading={loading} onJump={onJump} />
      <NowBar p={p} now={now} onShow={showTurn} />
      <div className="vp-grid">
        <ProcessPanel p={p} view={view} update={update} hidden={hidden} loading={loading} now={now} onJump={onJump} />
        <div className="vp-side">
          <Files files={p.files} selected={view.file} onSelect={(file) => update({ file })} onJump={onJump} />
          <Checks checks={p.checks} claims={p.claims} onJump={onJump} />
        </div>
      </div>
    </div>
  );
}
