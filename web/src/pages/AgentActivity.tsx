import { useCallback } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { EventsTab } from '../components/EventsTab';
import type { EventRecord } from '../generated/EventRecord';
import { tierOf, type Tier } from '../herdr';
import { AgentActions, type AgentControls } from '../shell/AgentControls';
import { CheckDetails } from '../shell/CheckMark';
import { HandoffActions } from '../shell/HandoffActions';
import type { AgentRow } from '../shell/model';
import { Icon, Segmented, type IconName } from '../shell/ui';
import { ACTIVITY, LIFECYCLE, turnStateView } from '../status';
import { shortPath, useHome } from '../structure/FolderPicker';
import { VisualProgress } from '../visual/VisualProgress';
import { WORDS } from '../vocab';
import { transcriptItemPath } from './AgentConversation';

/** The evidence behind the status, as a drawn icon: verified, observed, agent-reported, heuristic or unknown. */
const TIER_ICON: Record<Tier['word'], IconName> = { verified: 'shield-check', observed: 'eye', reported: 'quote', heuristic: 'approx', unknown: 'unknown' };

/**
 * The agent's state once, in plain words, following turnStateView: activity, turn and tool are
 * current only while running, last known while stale, and not shown otherwise.
 */
function Detail({ a }: { a: AgentRow }) {
  const seat = a.seat!;
  const view = turnStateView(seat.lifecycle);
  const act = ACTIVITY[seat.activity].label;
  const tool = seat.current_tool ? seat.current_tool.toLowerCase() : null;
  if (view === 'not-running') return <p className="pane-detail">{LIFECYCLE[seat.lifecycle].label} · not running</p>;
  const parts = [view === 'last-known' ? `last known: ${act}` : act, seat.current_turn ? 'turn in progress' : 'no turn in progress'];
  if (tool) parts.push(`tool ${tool}`);
  return <p className="pane-detail">{parts.join(' · ')}</p>;
}

/** Folder, session, tokens and how the status is known. */
function Meta({ a }: { a: AgentRow }) {
  const seat = a.seat!;
  const home = useHome();
  const prov = tierOf(seat.lifecycle_provenance);
  return (
    <div className="pane-meta">
      <span className="folder" title={a.folder}><Icon name="folder" size={13} className="muted" />{a.folder === '~' ? '~ (home folder)' : shortPath(a.folder, home)}</span>
      <span className="sep">·</span>
      <span>{WORDS.session(seat.tenure)}</span>
      <span className="sep">·</span>
      <span>{seat.tokens_in === null || seat.tokens_out === null ? 'tokens unknown' : `${seat.tokens_in} in / ${seat.tokens_out} out tokens`}</span>
      <span className="sep">·</span>
      <span title={`Evidence for the latest status change: ${seat.lifecycle_provenance ?? 'unknown'}`}>
        status <span className={`tier-inline tier-${prov.word}`}><Icon name={TIER_ICON[prov.word]} size={12} /> {prov.word}</span>
      </span>
      <details className="internal-id" data-advanced>
        <summary>{WORDS.advanced}</summary>
        <span>{WORDS.internalId}: <code>{seat.id}</code></span>
      </details>
    </div>
  );
}

type Props = { a: AgentRow; c: AgentControls; feed: EventRecord[]; epoch: number; handOverInline: boolean; askHandOver: number; askOnOpen?: boolean; onAsked?: () => void };

/**
 * The Activity tab: controls, the check, then what the agent did as a picture (progress, files built
 * and changed, checks) or, one step back, every raw event verbatim as evidence.
 */
export function AgentActivity({ a, c, feed, epoch, handOverInline, askHandOver, askOnOpen, onAsked }: Props) {
  const seat = a.seat!;
  const navigate = useNavigate();
  const [params, setParams] = useSearchParams();
  const view = params.get('view') === 'events' ? 'events' : 'progress';
  const showInTranscript = useCallback((key: string) => navigate(transcriptItemPath(a.rig, a.id, key), { state: { from: 'visual' } }), [navigate, a.rig, a.id]);
  return (
    <section className="ap-body ap-activity" aria-label="Activity">
      <div className="ap-section ap-controls">
        <AgentActions c={c} name={a.name} />
        {!handOverInline && <HandoffActions key={`h-${seat.id}`} a={a} part="hand-over" ask={askHandOver} askOnMount={askOnOpen} onAsked={onAsked} />}
        <CheckDetails seat={seat.id} check={seat.check} />
        <Meta a={a} />
        <Detail a={a} />
      </div>
      <div className="ap-view-row">
        <Segmented label="Show" value={view} onChange={(v) => setParams(v === 'events' ? { view: 'events' } : {}, { replace: true })}
          options={[{ value: 'progress', label: 'Progress', icon: 'activity' }, { value: 'events', label: 'Raw events', icon: 'file-text', tip: 'Every recorded event, verbatim (evidence)' }]} />
      </div>
      {view === 'events' ? (
        <div className="ap-events">
          <p className="ux-raw-note muted">Raw events: what was recorded for this agent, shown verbatim as evidence.</p>
          <EventsTab key={seat.id} seatId={seat.id} feed={feed} epoch={epoch} />
        </div>
      ) : (
        <div className="ap-progress">
          <VisualProgress key={seat.id} seatId={seat.id} feed={feed} epoch={epoch} state={a.state} onJump={showInTranscript} />
        </div>
      )}
    </section>
  );
}
