import { splitTeamMessage } from '../teamMessage';
import { TextArea } from './TextArea';
import { useState, type ReactNode } from 'react';
import { tokenSum, tokensLabel } from '../format';
import type { AgentItem, NoticeItem, OperatorItem, TurnEndItem } from '../conversation';
import { durationText, tierOf, type Vendor } from '../herdr';
import { Fold } from './Fold';
import { Hl, matches, useQuery } from './highlight';
import { CopyButton, Markdown } from './Markdown';
import { ToolCard, ToolGroup } from './ToolCard';
import { Icon, VendorMark } from './ui';
import { WORDS } from '../vocab';
import type { Block } from './transcriptModel';

const DELIVERY: Record<string, string> = { queued: 'queued', delivered: 'delivered', consumed: 'read', failed: 'failed' };

/** Provenance tier, shown on hover or focus of a block. */
export function Tier({ provenance, claim }: { provenance?: string; claim?: boolean }) {
  const t = tierOf(provenance, claim);
  return (
    <span className={`tier tier-${t.word}`} title={`evidence: ${t.word} · ${t.detail}`}>
      <span aria-hidden="true">{t.glyph}</span> {t.word}
    </span>
  );
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

function HandoffJob({ job }: { job: string }) {
  return (
    <div className="msg-card-sec">
      <div className="msg-card-label">Job</div>
      <FoldText text={job}>
        <div className="msg-card-text" data-raw-evidence>
          <Hl text={job} />
        </div>
      </FoldText>
    </div>
  );
}

function HandoffResult({ r, open }: { r: { name: string; text: string }; open: boolean }) {
  return (
    <details className="msg-card-result" open={open}>
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
  );
}

function HandoffCard({ m }: { m: OperatorItem }) {
  const h = m.handoff;
  return (
    <div className={`msg-card handoff${m.state === 'failed' ? ' failed' : ''}`}>
      <div className="msg-card-head">
        <Icon name="arrow-right" size={14} />
        <span className="msg-card-title">{h ? <Hl text={h.title} /> : 'Handed over'}</span>
        <span className="pane-spacer" />
        <Delivery m={m} />
      </div>
      {h ? (
        <>
          {h.job && <HandoffJob job={h.job} />}
          {h.results.map((r, i) => <HandoffResult key={i} r={r} open={h.results.length === 1 && r.text.length < 600} />)}
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

export type Open = {
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

export function BlockView({ b, vendor, name, open, flashKey }: { b: Block; vendor: Vendor; name: string; open: Open; flashKey: string | null }): ReactNode {
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

