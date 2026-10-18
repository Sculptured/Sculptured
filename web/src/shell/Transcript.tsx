import {DismissibleError} from './DismissibleError';
import { splitTeamMessage } from '../teamMessage';
import { TextArea } from './TextArea';
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import type { EventRecord } from '../generated/EventRecord';
import { tokenSum, tokensLabel } from '../format';
import { conversationItems, openTurn, type AgentItem, type Item, type NoticeItem, type OperatorItem, type ToolItem, type TurnEndItem } from '../conversation';
import { durationText, tierOf, type Vendor } from '../herdr';
import { GAP_NOTICE, useSeatEvents } from '../useSeatEvents';
import { Fold } from './Fold';
import { Highlight, Hl, matches, scrollWithin, useQuery } from './highlight';
import { CopyButton, Markdown } from './Markdown';
import { toolSearchText, ToolCard, ToolGroup, toolStatus } from './ToolCard';
import { Icon, VendorMark } from './ui';
import { WORDS } from '../vocab';
import './chat.css';

/** Within this many px of the end counts as "at the bottom". */
const STICK_PX = 24;
/** Blocks drawn at first; older ones wait behind "Show earlier". */
const PAGE = 150;

type Part = { kind: 'text'; item: AgentItem } | { kind: 'tools'; key: string; tools: ToolItem[] } | { kind: 'unrecognized'; key: string; raw: unknown };

export type Block =
  | { kind: 'message'; key: string; item: OperatorItem }
  | { kind: 'agent'; key: string; parts: Part[] }
  | { kind: 'divider'; key: string; items: NoticeItem[] }
  | { kind: 'turn-end'; key: string; item: TurnEndItem };

/**
 * Items as chat blocks: a message, an agent reply (its text and tool calls, consecutive calls
 * folded into one group), or a quiet divider for session and turn events. Consecutive quiet
 * session events share one divider; an alarming one stands alone.
 */
export function buildBlocks(items: Item[]): Block[] {
  const out: Block[] = [];
  let agent: Extract<Block, { kind: 'agent' }> | null = null;
  const reply = (key: string) => {
    if (!agent) {
      agent = { kind: 'agent', key: `r-${key}`, parts: [] };
      out.push(agent);
    }
    return agent;
  };
  for (const it of items) {
    switch (it.kind) {
      case 'agent':
        reply(it.key).parts.push({ kind: 'text', item: it });
        break;
      case 'tool': {
        const r = reply(it.key);
        const last = r.parts[r.parts.length - 1];
        if (last?.kind === 'tools') last.tools.push(it);
        else r.parts.push({ kind: 'tools', key: `g-${it.key}`, tools: [it] });
        break;
      }
      case 'unrecognized':
        reply(it.key).parts.push({ kind: 'unrecognized', key: it.key, raw: it.raw });
        break;
      case 'operator':
        agent = null;
        out.push({ kind: 'message', key: it.key, item: it });
        break;
      case 'notice': {
        agent = null;
        const prev = out[out.length - 1];
        if (prev?.kind === 'divider' && !it.alarm && !prev.items[prev.items.length - 1].alarm) prev.items.push(it);
        else out.push({ kind: 'divider', key: it.key, items: [it] });
        break;
      }
      case 'turn-end':
        agent = null;
        out.push({ kind: 'turn-end', key: it.key, item: it });
        break;
    }
  }
  return out;
}

/** Every item key inside a block, for jumping to an item. */
function blockKeys(b: Block): string[] {
  if (b.kind === 'divider') return b.items.map((n) => n.key);
  if (b.kind !== 'agent') return [b.key];
  return b.parts.flatMap((p) => (p.kind === 'text' ? [p.item.key] : p.kind === 'tools' ? p.tools.map((t) => t.key) : [p.key]));
}

function blockMatches(b: Block, q: string): boolean {
  if (!q) return false;
  switch (b.kind) {
    case 'message':
      return matches(q, b.item.text, b.item.reason);
    case 'divider':
      return b.items.some((n) => matches(q, n.text));
    case 'turn-end':
      return false;
    case 'agent':
      return b.parts.some((p) =>
        p.kind === 'text' ? matches(q, p.item.text) : p.kind === 'tools' ? p.tools.some((t) => matches(q, toolSearchText(t))) : matches(q, JSON.stringify(p.raw)),
      );
  }
}

const DELIVERY: Record<string, string> = { queued: 'queued', delivered: 'delivered', consumed: 'read', failed: 'failed' };

/** Provenance tier, shown on hover or focus of a block. */
function Tier({ provenance, claim }: { provenance?: string; claim?: boolean }) {
  const t = tierOf(provenance, claim);
  return (
    <span className={`tier tier-${t.word}`} title={`evidence: ${t.word} · ${t.detail}`}>
      <span aria-hidden="true">{t.glyph}</span> {t.word}
    </span>
  );
}

/** The key of the newest agent reply, or null. */
export function latestReplyKey(items: Item[]): string | null {
  for (let i = items.length - 1; i >= 0; i--) if (items[i].kind === 'agent') return items[i].key;
  return null;
}

function Delivery({ m }: { m: OperatorItem }) {
  const failed = m.state === 'failed';
  return (
    <span className={`delivery d-${m.state}`}>
      {m.intent === 'instruct' ? 'instruct · ' : ''}
      {failed ? (m.atStart || m.origin === 'handoff' ? '✗ not sent' : '✗ failed') : DELIVERY[m.state]}
      {m.reason ? (
        <>
          {' — '}
          <span data-raw-evidence>{m.reason}</span>
        </>
      ) : null}
    </span>
  );
}

function HandoffCard({ m }: { m: OperatorItem }) {
  const h = m.handoff;
  const failed = m.state === 'failed';
  return (
    <div className={`msg-card handoff${failed ? ' failed' : ''}`}>
      <div className="msg-card-head">
        <Icon name="arrow-right" size={14} />
        <span className="msg-card-title">{h ? <Hl text={h.title} /> : 'Handed over'}</span>
        <span className="pane-spacer" />
        <Delivery m={m} />
      </div>
      {h ? (
        <>
          {h.job && (
            <div className="msg-card-sec">
              <div className="msg-card-label">Job</div>
              <FoldText text={h.job}>
                <div className="msg-card-text" data-raw-evidence>
                  <Hl text={h.job} />
                </div>
              </FoldText>
            </div>
          )}
          {h.results.map((r, i) => (
            <details key={i} className="msg-card-result" open={h.results.length === 1 && r.text.length < 600}>
              <summary>
                <Icon name="chevron-right" size={13} className="chev" />
                <span>
                  <span data-raw-evidence>
                    <Hl text={r.name} />
                  </span>
                  ’s result
                </span>
              </summary>
              <div className="msg-card-text md-wrap" data-raw-evidence>
                {r.text ? <Markdown text={r.text} /> : <p className="muted">No reply text.</p>}
              </div>
            </details>
          ))}
        </>
      ) : (
        <div className="msg-card-text" data-raw-evidence>
          <Hl text={m.text} />
        </div>
      )}
    </div>
  );
}

/** Long text folds behind "Show more"; a search hit in it, or a jump to it, opens it. */
function FoldText({ text, force = false, children }: { text: string; force?: boolean; children: ReactNode }) {
  const q = useQuery();
  return (
    <Fold text={text} force={force || matches(q, text)}>
      {children}
    </Fold>
  );
}

function MessageBlock({ m }: { m: OperatorItem }) {
  const { text, briefing } = splitTeamMessage(m.text);
  return <><MessageContent m={{ ...m, text }} />{briefing && <details className="msg-card-result">
    <summary>Captured team instructions</summary>
    <TextArea aria-label="Captured team instructions" className="fld-input" value={briefing} readOnly rows={6} />
  </details>}</>;
}

function MessageContent({ m }: { m: OperatorItem }) {
  if (m.origin === 'handoff') return <HandoffCard m={m} />;
  if (m.atStart) {
    const notSent = m.state === 'failed';
    return (
      <div className={`msg-card job${notSent ? ' failed' : ''}`}>
        <div className="msg-card-head">
          <Icon name="file-text" size={14} />
          <span className="msg-card-title">{notSent ? 'Job · not sent' : 'Job · sent at start'}</span>
          <span className="pane-spacer" />
          {!notSent && <Delivery m={m} />}
        </div>
        <FoldText text={m.text}>
          <div className="msg-card-text">
            <Hl text={m.text} />
          </div>
        </FoldText>
        {notSent && (
          <p className="msg-card-fail">
            ✗ not sent — <span data-raw-evidence>{m.reason ?? 'reason unknown'}</span>
          </p>
        )}
      </div>
    );
  }
  return (
    <div className={`bubble-row${m.state === 'failed' ? ' failed' : ''}`}>
      <div className="bubble">
        {m.origin === "agent" ? <strong>Agent {m.sender}: </strong> : m.origin === "system" ? <strong>System: </strong> : <span className="sr-only">You: </span>}
        <FoldText text={m.text}>
          <Hl text={m.text} />
        </FoldText>
      </div>
      <Delivery m={m} />
    </div>
  );
}

function NoticeText({ n }: { n: NoticeItem }) {
  const at = n.raw ? n.text.lastIndexOf(n.raw) : -1;
  const text = n.text.charAt(0).toUpperCase() + n.text.slice(1);
  if (at < 0) return <Hl text={text} />;
  return (
    <>
      <Hl text={text.slice(0, at)} />
      <span data-raw-evidence>
        <Hl text={n.raw!} />
      </span>
      <Hl text={text.slice(at + n.raw!.length)} />
    </>
  );
}

/** One line for a run of session events: "Restarted · new conversation". */
export function chainSummary(items: NoticeItem[]): string {
  const started = items.find((n) => n.session?.phase === 'started');
  const ended = items.some((n) => n.session?.phase === 'ended');
  const last = items[items.length - 1].text;
  return WORDS.sessionChain(!!started, ended, started?.session?.cause, last.charAt(0).toUpperCase() + last.slice(1));
}

/**
 * Session events between turns. One quiet event is one short line. A run of them ("Stopping ›
 * Session #1 ended · exit 0 › … › Running") folds to one summary line that opens, on click, to
 * the verbatim chain (evidence). A search hit inside opens it; an alarm always shows in full.
 */
function Chain({ items, shown }: { items: NoticeItem[]; shown: boolean }) {
  return (
    <span className="divider-chain" data-raw-evidence hidden={!shown}>
      {items.map((n, i) => (
        <span key={n.key} data-item-key={i > 0 ? n.key : undefined}>
          {i > 0 && (
            <span className="divider-sep" aria-hidden="true">
              {' › '}
            </span>
          )}
          <NoticeText n={n} />
        </span>
      ))}
    </span>
  );
}

function Divider({ items }: { items: NoticeItem[] }) {
  const q = useQuery();
  const [open, setOpen] = useState(false);
  const alarm = items.some((n) => n.alarm);
  const foldable = items.length > 1 && !alarm;
  const shown = !foldable || open || items.some((n) => matches(q, n.text));
  return (
    <div className={`divider${alarm ? ' alarm' : ''}${foldable ? ' chain' : ''}`}>
      <span className="divider-text">
        {foldable && (
          <button type="button" className="divider-toggle" aria-expanded={shown} onClick={() => setOpen(!open)} data-tip={shown ? undefined : WORDS.showSessionEvents}>
            {chainSummary(items)}
            <Icon name={shown ? 'chevron-down' : 'chevron-right'} size={12} />
          </button>
        )}
        <Chain items={items} shown={shown} />
      </span>
    </div>
  );
}

function TurnDivider({ t }: { t: TurnEndItem }) {
  const tokens = tokensLabel(tokenSum(t.tokens));
  const word = t.status === 'completed' ? 'Turn completed' : t.status === 'failed' ? '✗ Turn failed' : 'Turn interrupted';
  return (
    <div className={`divider turn t-${t.status}`}>
      <span className="divider-text" title={t.tokens ? `${t.tokens.input} in / ${t.tokens.output} out` : undefined}>
        {word} · {t.durationMs === null ? 'duration unknown' : durationText(t.durationMs)} · {tokens}
      </span>
    </div>
  );
}

/** An agent reply. A long one folds, except the latest, which you most likely came to read. */
function ReplyText({ item, flash, latest }: { item: AgentItem; flash: boolean; latest: boolean }) {
  const [raw, setRaw] = useState(false);
  return (
    <div className={`reply-part${flash ? ' flash' : ''}`} data-item-key={item.key}>
      <FoldText text={item.text} force={flash || latest}>
        <div className="reply-text" data-raw-evidence>
          {raw ? (
            <pre className="reply-raw">
              <Hl text={item.text} />
            </pre>
          ) : (
            <Markdown text={item.text} />
          )}
        </div>
      </FoldText>
      <div className="reply-actions">
        <CopyButton text={item.text} label="Copy reply" />
        <button type="button" className="copy-btn" aria-pressed={raw} onClick={() => setRaw((v) => !v)}>
          {raw ? 'Formatted' : 'Raw text'}
        </button>
        <Tier provenance={item.provenance} claim />
      </div>
    </div>
  );
}

type Open = {
  group: (key: string) => boolean;
  tool: (key: string) => boolean;
  toggleGroup: (key: string) => void;
  toggleTool: (key: string) => void;
  /** Key of the latest agent reply; it starts unfolded. */
  latest: string | null;
};

function AgentBlock({ b, vendor, name, open, flashKey }: { b: Extract<Block, { kind: 'agent' }>; vendor: Vendor; name: string; open: Open; flashKey: string | null }) {
  return (
    <div className="reply">
      <div className="reply-who">
        <VendorMark v={vendor} size="sm" />
        <span className="reply-name">{name}</span>
      </div>
      <div className="reply-body">
        {b.parts.map((p) => {
          if (p.kind === 'text') return <ReplyText key={p.item.key} item={p.item} flash={flashKey === p.item.key} latest={open.latest === p.item.key} />;
          if (p.kind === 'unrecognized')
            return (
              <details key={p.key} className="unrecognized" data-item-key={p.key}>
                <summary>Unrecognized event (kept as evidence)</summary>
                <pre data-raw-evidence>{JSON.stringify(p.raw, null, 2)}</pre>
              </details>
            );
          if (p.tools.length === 1) {
            const t = p.tools[0];
            return <ToolCard key={t.key} t={t} open={open.tool(t.key)} onToggle={() => open.toggleTool(t.key)} flash={flashKey === t.key} />;
          }
          return (
            <ToolGroup
              key={p.key}
              tools={p.tools}
              open={open.group(p.key)}
              onToggle={() => open.toggleGroup(p.key)}
              isOpen={open.tool}
              toggleTool={open.toggleTool}
              flashKey={flashKey}
            />
          );
        })}
      </div>
    </div>
  );
}

function BlockView({ b, vendor, name, open, flashKey }: { b: Block; vendor: Vendor; name: string; open: Open; flashKey: string | null }): ReactNode {
  switch (b.kind) {
    case 'message':
      return <MessageBlock m={b.item} />;
    case 'agent':
      return <AgentBlock b={b} vendor={vendor} name={name} open={open} flashKey={flashKey} />;
    case 'divider':
      return <Divider items={b.items} />;
    case 'turn-end':
      return <TurnDivider t={b.item} />;
  }
}

function isTextField(t: EventTarget | null): boolean {
  if (!(t instanceof HTMLElement)) return false;
  return t.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(t.tagName);
}

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

/**
 * The conversation as a chat: your messages on the right, the agent's replies as Markdown
 * with its tool calls folded in, and quiet dividers for sessions and turns. It follows new
 * output only while you are at the bottom; otherwise a "▼ N new" chip counts what arrived.
 */
export function Transcript({ seatId, feed, epoch, vendor, name, focusKey = null, onReturn, returnLabel, toolsSlot }: Props) {
  const { events, loading, error, truncated, gap } = useSeatEvents(seatId, feed, epoch);
  const items = useMemo(() => conversationItems(events), [events]);
  const blocks = useMemo(() => buildBlocks(items), [items]);
  const live = useMemo(() => openTurn(events), [events]);
  const scroller = useRef<HTMLDivElement>(null);
  const atBottom = useRef(true);
  const count = useRef(0);
  const [unread, setUnread] = useState(0);
  const [limit, setLimit] = useState(PAGE);
  const [query, setQuery] = useState('');
  const [hit, setHit] = useState(0);
  const [hits, setHits] = useState(0);
  const searchRef = useRef<HTMLInputElement>(null);
  // In the header the field folds to an icon until it is used; `focusSearch` focuses it once drawn.
  const [searchOpen, setSearchOpen] = useState(false);
  const focusSearch = useRef(false);
  const fieldShown = !toolsSlot || searchOpen || query !== '';
  useEffect(() => {
    if (focusSearch.current && fieldShown) {
      focusSearch.current = false;
      searchRef.current?.focus();
      searchRef.current?.select();
    }
  });
  const openSearch = () => {
    focusSearch.current = true;
    setSearchOpen(true);
  };
  const [toggled, setToggled] = useState<Map<string, boolean>>(new Map());
  const [flashKey, setFlashKey] = useState<string | null>(null);
  const jumped = useRef<string | null>(null);
  const q = query.trim().toLowerCase();

  // Where a focused item lives: its block, and the group around it.
  const focus = useMemo(() => {
    if (!focusKey) return null;
    for (let i = 0; i < blocks.length; i++) {
      const b = blocks[i];
      if (!blockKeys(b).includes(focusKey)) continue;
      const group = b.kind === 'agent' ? b.parts.find((p) => p.kind === 'tools' && p.tools.length > 1 && p.tools.some((t) => t.key === focusKey)) : undefined;
      return { index: i, group: group?.kind === 'tools' ? group.key : null };
    }
    return null;
  }, [blocks, focusKey]);

  const start = q ? 0 : Math.max(0, Math.min(blocks.length - limit, focus ? focus.index : Infinity));
  const shown = blocks.slice(start);
  const matchedBlocks = useMemo(() => new Set(q ? blocks.filter((b) => blockMatches(b, q)).map((b) => b.key) : []), [blocks, q]);

  const open: Open = useMemo(
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

  // Follow new output while at the bottom (unless a jump is pending); otherwise count it.
  useLayoutEffect(() => {
    const el = scroller.current;
    const before = count.current;
    count.current = items.length;
    if (!el) return;
    const pendingJump = !!focusKey && jumped.current !== focusKey;
    if (atBottom.current && !pendingJump) el.scrollTop = el.scrollHeight;
    else if (items.length > before && before > 0) setUnread((n) => n + items.length - before);
  }, [items, focusKey]);

  // Jump to a focused item once it has loaded: scroll it into view and flash it.
  useEffect(() => {
    if (!focusKey || jumped.current === focusKey || !focus) return;
    const el = scroller.current?.querySelector<HTMLElement>(`[data-item-key="${CSS.escape(focusKey)}"]`);
    if (!el) return;
    jumped.current = focusKey;
    atBottom.current = false;
    scrollWithin(scroller.current, el);
    (el.closest('li') as HTMLElement | null)?.focus({ preventScroll: true });
    setFlashKey(focusKey);
    const t = setTimeout(() => setFlashKey(null), 2400);
    return () => clearTimeout(t);
  }, [focusKey, focus, shown.length]);

  // Search: count the marks drawn, and bring the current one into view.
  useLayoutEffect(() => {
    const marks = q ? [...(scroller.current?.querySelectorAll<HTMLElement>('mark.hit') ?? [])] : [];
    setHits(marks.length);
    const i = marks.length ? Math.min(hit, marks.length - 1) : 0;
    marks.forEach((m, j) => m.classList.toggle('current', j === i));
    if (marks[i]) {
      atBottom.current = false;
      scrollWithin(scroller.current, marks[i]);
    }
  }, [q, hit, items]);

  // `/` searches the transcript; Esc goes back to where you came from.
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.defaultPrevented || e.altKey || e.ctrlKey || e.metaKey || isTextField(e.target)) return;
      if (document.querySelector('[role="dialog"]')) return;
      if (e.key === '/') {
        e.preventDefault();
        if (searchRef.current) {
          searchRef.current.focus();
          searchRef.current.select();
        } else openSearch();
      } else if (e.key === 'Escape' && onReturn) {
        e.preventDefault();
        onReturn();
      }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [onReturn]);

  const onScroll = () => {
    const el = scroller.current;
    if (!el) return;
    atBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight <= STICK_PX;
    if (atBottom.current) setUnread(0);
  };

  const jump = () => {
    const el = scroller.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
    atBottom.current = true;
    setUnread(0);
  };

  const step = useCallback((d: number) => setHit((h) => (hits ? (Math.min(h, hits - 1) + d + hits) % hits : 0)), [hits]);

  const onSearchKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      step(e.shiftKey ? -1 : 1);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      if (query) setQuery('');
      else e.currentTarget.blur();
    }
  };

  // ↑/↓ move between blocks so each one's evidence can be read from the keyboard.
  const onKeyDown = (e: KeyboardEvent<HTMLOListElement>) => {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    if (isTextField(e.target)) return;
    const lines = [...e.currentTarget.querySelectorAll<HTMLElement>(':scope > li')];
    if (lines.length === 0) return;
    const i = lines.indexOf((document.activeElement as HTMLElement | null)?.closest('li') as HTMLElement);
    const next = e.key === 'ArrowDown' ? Math.min(lines.length - 1, i + 1) : i < 0 ? lines.length - 1 : Math.max(0, i - 1);
    e.preventDefault();
    lines[next].focus();
  };

  let lastTool: ToolItem | undefined;
  for (let i = items.length - 1; i >= 0 && !lastTool; i--) if (items[i].kind === 'tool') lastTool = items[i] as ToolItem;
  const working = !!live && !!lastTool && lastTool.turnId === live && toolStatus(lastTool) === 'running';

  const tools = (
    <>
        {onReturn && (
          <button type="button" className="return-btn" onClick={onReturn} aria-keyshortcuts="Escape" data-tip="Go back" data-kbd="esc">
            <Icon name="arrow-left" size={13} />
            {returnLabel ?? 'Back'}
          </button>
        )}
        {!fieldShown && (
          <button type="button" className="icon-btn cv-search-btn" onClick={openSearch} aria-label="Search transcript" aria-keyshortcuts="/" data-tip="Search transcript" data-kbd="/">
            <Icon name="search" size={15} />
          </button>
        )}
        {fieldShown && <label className="cv-search">
          <Icon name="search" size={13} className="muted" />
          <input
            ref={searchRef}
            type="search"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setHit(0);
            }}
            onKeyDown={onSearchKey}
            onBlur={() => { if (!query) setSearchOpen(false); }}
            placeholder="Search transcript"
            aria-label="Search transcript"
            aria-keyshortcuts="/"
          />
          {!query && (
            <kbd className="kbd" aria-hidden="true">
              /
            </kbd>
          )}
        </label>}
        {q && (
          <span className="cv-search-nav">
            <span className="cv-count" role="status">
              {hits ? `${Math.min(hit, hits - 1) + 1} of ${hits}` : 'No matches'}
            </span>
            <button type="button" className="icon-btn" onClick={() => step(-1)} disabled={!hits} aria-label="Previous match" data-tip="Previous match" data-kbd="⇧⏎">
              <Icon name="arrow-up" size={13} />
            </button>
            <button type="button" className="icon-btn flip" onClick={() => step(1)} disabled={!hits} aria-label="Next match" data-tip="Next match" data-kbd="⏎">
              <Icon name="arrow-up" size={13} />
            </button>
            <button type="button" className="icon-btn" onClick={() => setQuery('')} aria-label="Clear search" data-tip="Clear search">
              <Icon name="x" size={13} />
            </button>
          </span>
        )}
    </>
  );

  return (
    <div className="transcript">
      {toolsSlot ? createPortal(<span className="pane-search-tools">{tools}</span>, toolsSlot) : toolsSlot === undefined && <div className="cv-tools">{tools}</div>}
      <div className="transcript-scroll" ref={scroller} onScroll={onScroll} data-testid="transcript-scroll">
        <Highlight query={query}>
          <div className="cv-column">
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
            {start > 0 && (
              <button type="button" className="show-earlier" onClick={() => setLimit((l) => l + PAGE)}>
                Show {Math.min(PAGE, start)} earlier
              </button>
            )}
            <ol className="conversation" aria-label="Conversation" aria-live="polite" tabIndex={0} onKeyDown={onKeyDown}>
              {shown.map((b) => (
                <li
                  key={b.key}
                  className={`cv cv-${b.kind}${q && !matchedBlocks.has(b.key) ? ' dim' : ''}${b.kind !== 'agent' && flashKey === b.key ? ' flash' : ''}`}
                  tabIndex={-1}
                  data-item-key={b.kind === 'agent' ? undefined : b.key}
                >
                  <BlockView b={b} vendor={vendor} name={name ?? vendor.name} open={open} flashKey={flashKey} />
                  {b.kind !== 'agent' && <Tier provenance={b.kind === 'divider' ? b.items[b.items.length - 1].provenance : b.item.provenance} />}
                </li>
              ))}
            </ol>
            {live && (
              <div className="cv-live" role="status">
                <VendorMark v={vendor} size="sm" />
                <span className="dots" aria-hidden="true">
                  <span />
                  <span />
                  <span />
                </span>
                <span>{working ? 'Working…' : 'Thinking…'}</span>
              </div>
            )}
            {!loading && items.length === 0 && <p className="muted pad empty-chat">No conversation yet.</p>}
          </div>
        </Highlight>
      </div>
      {unread > 0 && (
        <button type="button" className="new-chip" onClick={jump}>
          ▼ {unread} new
        </button>
      )}
    </div>
  );
}
