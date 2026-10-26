import type { CostSource } from '../generated/CostSource';
import type { TurnTokens } from '../generated/TurnTokens';
import type { WorkflowRun } from '../generated/WorkflowRun';
import { itemBoard, waves, type Card, type Wave } from './itemBoardModel';

const TOKENS_NOTE = 'Input + output tokens as Claude Code or Codex reported them (cache reads not included); they may arrive up to one turn late.';

function duration(ms: number): string {
  const m = Math.max(0, Math.round(ms / 60000));
  return m < 60 ? `${m} min` : `${Math.floor(m / 60)} h ${m % 60} min`;
}

const SOURCE_WORDS: Record<CostSource, string> = {
  'claude-code': "Claude Code's own estimate",
  'openai-list-prices': 'OpenAI list prices (checked 12 Oct 2026)',
  opencode: "OpenCode's own estimate",
};
/** Estimated cost in words: always "≈", always where it came from; plan subscribers are not billed this. */
export function costText(usd: number, partial: boolean): string {
  return `${partial ? 'at least ' : ''}≈ $${usd < 0.01 ? usd.toFixed(4) : usd.toFixed(2)}`;
}
export function costTitle(sources: CostSource[]): string {
  return `Estimate at API prices from ${sources.map((s) => SOURCE_WORDS[s]).join(' and ')}. Claude or ChatGPT plan subscribers are not billed this amount.`;
}

/** The waves of topics, in order: each needs the one before; where each is, in words beside its bar. */
function Waves({ list }: { list: Wave[] }) {
  if (!list.length) return null;
  const state = (w: Wave) => (w.done === w.topics.length ? 'done' : w.started ? 'in progress' : `waiting for wave ${w.wave - 1}`);
  return <ol className="item-waves" aria-label="Waves">
    {list.map((w) => <li key={w.wave} className={`wave-${state(w) === 'done' ? 'done' : w.started ? 'active' : 'waiting'}`}>
      <progress value={w.done} max={w.topics.length} aria-hidden="true" />
      <span>Wave {w.wave} · {state(w)} · {w.done} of {w.topics.length}: {w.topics.join(', ')}</span>
    </li>)}
  </ol>;
}

/** Where each item is, what it's waiting for and what it has used. A card opens the item's timeline below. */
export function ItemBoard({ run, usage, now = Date.now() }: { run: WorkflowRun; usage: Record<string, TurnTokens>; now?: number }) {
  const { columns, summary } = itemBoard(run, usage, now);
  const open = (c: Card) => {
    const details = document.getElementById(`item-${c.task}`) as HTMLDetailsElement | null;
    if (details) { details.open = true; details.scrollIntoView?.({ block: 'nearest' }); }
  };
  const sendBacks = Object.entries(summary.sendBacks).map(([stage, n]) => `${run.config.stages.find((s) => s.id === stage)?.name ?? stage}: ${n}`).join(' · ');
  return <section className="item-board-section" aria-label="Item board">
    <Waves list={waves(run)} />
    <dl className="item-board-summary">
      <dt>Done</dt><dd>{summary.done}</dd>
      <dt>Waiting for a person</dt><dd>{summary.waiting}{summary.waiting > 0 && ` (${duration(summary.waitingMs)} in all)`}</dd>
      <dt>Blocked</dt><dd>{summary.blocked}</dd>
      <dt>Tokens per done item</dt><dd>{summary.tokensPerDone == null ? 'not reported' : `${summary.tokensPerDone.toLocaleString()} (${summary.doneReported} of ${summary.done} reported)`}</dd>
      {summary.costPerDone != null && <><dt>Cost per done item</dt><dd title="Estimate at API prices; plan subscribers are not billed this.">≈ ${summary.costPerDone.toFixed(2)} (estimate)</dd></>}
      {sendBacks && <><dt>Sent back</dt><dd>{sendBacks}</dd></>}
    </dl>
    <div className="item-board">
      {columns.map((col) => <section key={col.stage} className="item-board-column" aria-label={`${col.name}: ${col.cards.length}`}>
        <h3>{col.name} <span className="muted">{col.cards.length}</span></h3>
        {col.cards.map((c) => <button key={c.task} className={`item-card state-${c.status}`} onClick={() => open(c)}
          aria-label={`${c.title} · ${col.name} · ${c.badge ?? c.status}`} aria-describedby={`card-${c.task}`}>
          <strong>{c.title}</strong>
          {c.badge && <span className="item-card-badge">{c.badge}</span>}
          <small id={`card-${c.task}`}>round {c.rounds}{c.sinceMs != null ? ` · ${duration(now - c.sinceMs)} here` : ' · not started'}
            {c.tokens != null && <> · <span title={TOKENS_NOTE}>{c.partial ? 'at least ' : ''}{c.tokens.toLocaleString()} tokens{c.partial ? ' (some turns not reported)' : ''}</span></>}
            {c.costUsd != null && <> · <span title={costTitle(c.costSources)}>{costText(c.costUsd, c.costPartial)}</span></>}</small>
        </button>)}
      </section>)}
    </div>
    <p className="muted">{TOKENS_NOTE}</p>
  </section>;
}
