import { DismissibleError } from './DismissibleError';
import { useMemo, useRef, useState, type KeyboardEvent } from 'react';
import { createPortal } from 'react-dom';
import type { EventRecord } from '../generated/EventRecord';
import { conversationItems, openTurn, type Item } from '../conversation';
import type { Vendor } from '../herdr';
import { GAP_NOTICE, useSeatEvents } from '../useSeatEvents';
import { Highlight, matches } from './highlight';
import { toolSearchText } from './ToolCard';
import { VendorMark } from './ui';
import { BlockView, Tier, type Open } from './TranscriptParts';
import { blockMatches, buildBlocks, focusLocation, isWorking, latestReplyKey, type Block } from './transcriptModel';
import { SearchTools, useTranscriptKeys, useTranscriptSearch } from './TranscriptSearch';
import { isTextField, useFocusJump, useStickToBottom } from './transcriptScroll';
import './chat.css';

export { buildBlocks, latestReplyKey, type Block } from './transcriptModel';
export { chainSummary } from './TranscriptParts';

/** Blocks drawn at first; older ones wait behind "Show earlier". */
const PAGE = 150;

type Props = {
  seatId: string;
  feed: EventRecord[];
  epoch: number;
  vendor: Vendor;
  /** The agent's display name, shown above its replies. */
  name?: string;
  /** A transcript item to scroll to and highlight (from Visual progress). */
  focusKey?: string | null;
  /** Set when you came here from another view: Esc and the button go back. */
  onReturn?: () => void;
  returnLabel?: string;
  /**
   * Where the search lives: a slot in the agent pane's header. There it is a search icon that opens
   * the field (also with `/`) and folds back when left empty. Without a slot it is a row of its own.
   */
  toolsSlot?: HTMLElement | null;
};

/** Which tool groups and calls are unfolded: what you toggled, what a search or a jump needs, else closed. */
function useOpenState(blocks: Block[], items: Item[], focus: { group: string | null } | null, focusKey: string | null, q: string): Open {
  const [toggled, setToggled] = useState<Map<string, boolean>>(new Map());
  return useMemo(
    () => ({
      group: (key) => {
        if (q) {
          const g = blocks.flatMap((b) => (b.kind === 'agent' ? b.parts : [])).find((p) => p.kind === 'tools' && p.key === key);
          if (g?.kind === 'tools' && g.tools.some((t) => matches(q, toolSearchText(t)))) return true;
        }
        if (focus?.group === key && toggled.get(key) === undefined) return true;
        // Closed until you open it, even while a call runs: its header shows the current step,
        // so the layout does not jump open and shut between steps.
        return toggled.get(key) ?? false;
      },
      tool: (key) => toggled.get(key) ?? key === focusKey,
      toggleGroup: (key) => setToggled((m) => new Map(m).set(key, !(m.get(key) ?? focus?.group === key))),
      toggleTool: (key) => setToggled((m) => new Map(m).set(key, !(m.get(key) ?? key === focusKey))),
      latest: latestReplyKey(items),
    }),
    [toggled, q, blocks, focus, focusKey, items],
  );
}

/** ↑/↓ move between blocks so each one's evidence can be read from the keyboard. */
function onListKeys(e: KeyboardEvent<HTMLOListElement>) {
  if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
  if (isTextField(e.target)) return;
  const lines = [...e.currentTarget.querySelectorAll<HTMLElement>(':scope > li')];
  if (lines.length === 0) return;
  const i = lines.indexOf((document.activeElement as HTMLElement | null)?.closest('li') as HTMLElement);
  const next = e.key === 'ArrowDown' ? Math.min(lines.length - 1, i + 1) : i < 0 ? lines.length - 1 : Math.max(0, i - 1);
  e.preventDefault();
  lines[next].focus();
}

function LiveIndicator({ vendor, working }: { vendor: Vendor; working: boolean }) {
  return (
    <div className="cv-live" role="status">
      <VendorMark v={vendor} size="sm" />
      <span className="dots" aria-hidden="true">
        <span />
        <span />
        <span />
      </span>
      <span>{working ? 'Working…' : 'Thinking…'}</span>
    </div>
  );
}

type ListProps = { shown: Block[]; q: string; matched: Set<string>; flashKey: string | null; vendor: Vendor; name: string; open: Open };

function ConversationList({ shown, q, matched, flashKey, vendor, name, open }: ListProps) {
  return (
    <ol className="conversation" aria-label="Conversation" aria-live="polite" tabIndex={0} onKeyDown={onListKeys}>
      {shown.map((b) => (
        <li
          key={b.key}
          className={`cv cv-${b.kind}${q && !matched.has(b.key) ? ' dim' : ''}${b.kind !== 'agent' && flashKey === b.key ? ' flash' : ''}`}
          tabIndex={-1}
          data-item-key={b.kind === 'agent' ? undefined : b.key}
        >
          <BlockView b={b} vendor={vendor} name={name} open={open} flashKey={flashKey} />
          {b.kind !== 'agent' && <Tier provenance={b.kind === 'divider' ? b.items[b.items.length - 1].provenance : b.item.provenance} />}
        </li>
      ))}
    </ol>
  );
}

/** What the history load says above the list: loading, an error, or what was left out. */
function HistoryNotices({ loading, error, truncated, gap }: { loading: boolean; error: string | null; truncated: boolean; gap: boolean }) {
  return <>
    {loading && (
      <p role="status" className="muted pad">
        Loading history…
      </p>
    )}
    {error && (
      <DismissibleError className="tone-red pad">
        Could not load history: {error}
      </DismissibleError>
    )}
    {truncated && <p className="notice-line">Older events not loaded.</p>}
    {gap && <p className="notice-line tone-amber">{GAP_NOTICE}</p>}
  </>;
}

/** The blocks to draw: all of them while searching, else the newest PAGE (or from a focused item). */
function visibleStart(blocks: Block[], limit: number, q: string, focusIndex: number | undefined) {
  return q ? 0 : Math.max(0, Math.min(blocks.length - limit, focusIndex ?? Infinity));
}

/** The seat's events as conversation items, chat blocks and the turn still open. */
function useTranscriptModel(events: EventRecord[]) {
  const items = useMemo(() => conversationItems(events), [events]);
  const blocks = useMemo(() => buildBlocks(items), [items]);
  const live = useMemo(() => openTurn(events), [events]);
  return { items, blocks, live };
}

/**
 * The conversation as a chat: your messages on the right, the agent's replies as Markdown
 * with its tool calls folded in, and quiet dividers for sessions and turns. It follows new
 * output only while you are at the bottom; otherwise a "▼ N new" chip counts what arrived.
 */
export function Transcript({ seatId, feed, epoch, vendor, name, focusKey = null, onReturn, returnLabel, toolsSlot }: Props) {
  const { events, loading, error, truncated, gap } = useSeatEvents(seatId, feed, epoch);
  const { items, blocks, live } = useTranscriptModel(events);
  const scroller = useRef<HTMLDivElement>(null), jumped = useRef<string | null>(null);
  const [limit, setLimit] = useState(PAGE);
  const stick = useStickToBottom(scroller, items, focusKey, jumped);
  const search = useTranscriptSearch(scroller, items, stick, toolsSlot);
  useTranscriptKeys(search, onReturn);
  const focus = useMemo(() => focusLocation(blocks, focusKey), [blocks, focusKey]);
  const start = visibleStart(blocks, limit, search.q, focus?.index), shown = blocks.slice(start);
  const flashKey = useFocusJump(scroller, focusKey, focus, shown.length, jumped, stick);
  const matched = useMemo(() => new Set(search.q ? blocks.filter((b) => blockMatches(b, search.q)).map((b) => b.key) : []), [blocks, search.q]);
  const open = useOpenState(blocks, items, focus, focusKey, search.q);
  const tools = <SearchTools search={search} onReturn={onReturn} returnLabel={returnLabel} />;
  return (
    <div className="transcript">
      {toolsSlot ? createPortal(<span className="pane-search-tools">{tools}</span>, toolsSlot) : toolsSlot === undefined && <div className="cv-tools">{tools}</div>}
      <div className="transcript-scroll" ref={scroller} onScroll={stick.onScroll} data-testid="transcript-scroll">
        <Highlight query={search.query}>
          <div className="cv-column">
            <HistoryNotices loading={loading} error={error} truncated={truncated} gap={gap} />
            {start > 0 && (
              <button type="button" className="show-earlier" onClick={() => setLimit((l) => l + PAGE)}>
                Show {Math.min(PAGE, start)} earlier
              </button>
            )}
            <ConversationList shown={shown} q={search.q} matched={matched} flashKey={flashKey} vendor={vendor} name={name ?? vendor.name} open={open} />
            {live && <LiveIndicator vendor={vendor} working={isWorking(items, live)} />}
            {!loading && items.length === 0 && <p className="muted pad empty-chat">No conversation yet.</p>}
          </div>
        </Highlight>
      </div>
      {stick.unread > 0 && (
        <button type="button" className="new-chip" onClick={stick.jump}>
          ▼ {stick.unread} new
        </button>
      )}
    </div>
  );
}
