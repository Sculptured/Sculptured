import { useState, type ReactNode } from 'react';
import type { ToolItem } from '../conversation';
import { durationText } from '../herdr';
import { editOf, KIND_WORD, lineDiff, toolKind, toolSummary, type ToolKind } from '../visual/tools';
import { Hl, matches, useQuery } from './highlight';
import { Icon, type IconName } from './icons';
import { plural } from '../format';

export const KIND_ICON: Record<ToolKind, IconName> = {
  read: 'file-text',
  edit: 'pencil',
  bash: 'terminal',
  search: 'search',
  web: 'globe',
  task: 'bot',
  other: 'tool',
};

export type ToolStatus = 'running' | 'ok' | 'error';

export function toolStatus(t: Pick<ToolItem, 'output' | 'isError'>): ToolStatus {
  return t.output === undefined ? 'running' : t.isError ? 'error' : 'ok';
}

/** Status as glyph plus word; the running glyph spins unless motion is reduced. */
export function StatusMark({ status }: { status: ToolStatus }) {
  if (status === 'running')
    return (
      <span className="tstat t-running">
        <span className="spin-ring" aria-hidden="true" /> running
      </span>
    );
  return <span className={`tstat t-${status}`}>{status === 'ok' ? '✓ ok' : '✗ error'}</span>;
}

/** Text everything a tool call says, for search. */
export function toolSearchText(t: ToolItem): string {
  let input = '';
  try {
    input = typeof t.input === 'string' ? t.input : JSON.stringify(t.input);
  } catch {
    /* unprintable input */
  }
  return `${t.name}\n${toolSummary(t.name, t.input)}\n${input}\n${t.output ?? ''}`;
}

const MAX_LINES = 24;
const MAX_CHARS = 2400;

/** Long text cut to its first lines, with a button for the rest. Search shows it all. */
function Clipped({ text, className }: { text: string; className?: string }) {
  const q = useQuery();
  const [all, setAll] = useState(false);
  const lines = text.split('\n');
  const long = lines.length > MAX_LINES || text.length > MAX_CHARS;
  const showAll = all || !long || matches(q, text);
  let shown = text;
  if (!showAll) {
    shown = lines.slice(0, MAX_LINES).join('\n');
    if (shown.length > MAX_CHARS) shown = shown.slice(0, MAX_CHARS);
  }
  const hidden = lines.length - shown.split('\n').length;
  return (
    <>
      <pre className={className}>
        <Hl text={shown} />
      </pre>
      {long && !matches(q, text) && (
        <button type="button" className="link-btn show-more" onClick={() => setAll((v) => !v)} aria-expanded={showAll}>
          {showAll ? 'Show less' : hidden > 0 ? `Show more (${plural(hidden, 'more line')})` : 'Show more'}
        </button>
      )}
    </>
  );
}

/** A line diff of one change, cut after 40 lines with a button for the rest. */
export function Diff({ old, next }: { old: string; next: string }) {
  const q = useQuery();
  const [all, setAll] = useState(false);
  const d = lineDiff(old, next);
  const long = d.length > 40;
  const shown = long && !all && !matches(q, old, next) ? d.slice(0, 40) : d;
  return (
    <>
      <pre className="diff">
        {shown.map((l, i) => (
          <span key={i} className={`dl dl-${l.op === '+' ? 'add' : l.op === '-' ? 'del' : 'ctx'}`}>
            <span className="dl-op" aria-hidden="true">
              {l.op}
            </span>
            <span className="sr-only">{l.op === '+' ? 'added: ' : l.op === '-' ? 'removed: ' : ''}</span>
            <Hl text={l.text || ' '} />
          </span>
        ))}
      </pre>
      {long && (
        <button type="button" className="link-btn show-more" onClick={() => setAll((v) => !v)} aria-expanded={all}>
          {all ? 'Show less' : `Show more (${d.length - 40} more lines)`}
        </button>
      )}
    </>
  );
}

function Section({ title, children, error }: { title: string; children: ReactNode; error?: boolean }) {
  return (
    <div className="tc-sec">
      <div className={`tc-sec-title${error ? ' err' : ''}`}>{title}</div>
      {children}
    </div>
  );
}

type Edit = NonNullable<ReturnType<typeof editOf>>;

const editTitle = (edit: Edit) => (edit.action === 'edited' ? `Changes to ${edit.path}` : `${edit.action === 'created' ? 'Created' : 'Wrote'} ${edit.path}`);

/** The command a shell tool ran, when its input has one. */
function commandOf(t: ToolItem, kind: ToolKind): string | string[] | undefined {
  const cmd = kind === 'bash' && t.input && typeof t.input === 'object' ? (t.input as { command?: unknown }).command : undefined;
  return typeof cmd === 'string' || Array.isArray(cmd) ? cmd : undefined;
}

function InputSection({ edit, cmd, raw }: { edit: Edit | null; cmd: string | string[] | undefined; raw: string }) {
  if (edit && edit.hunks.length > 0) {
    return (
      <Section title={editTitle(edit)}>
        {edit.hunks.map((h, i) => (
          <Diff key={i} old={h.old} next={h.new} />
        ))}
      </Section>
    );
  }
  if (cmd !== undefined) {
    return (
      <Section title="Command">
        <pre className="tc-cmd">
          <span aria-hidden="true">$ </span>
          <Hl text={Array.isArray(cmd) ? cmd.join(' ') : cmd} />
        </pre>
      </Section>
    );
  }
  return (
    <Section title="Input">
      <Clipped text={raw} />
    </Section>
  );
}

function OutputSection({ t }: { t: ToolItem }) {
  if (t.output === undefined) return <p className="muted tc-empty">No result yet.</p>;
  return (
    <Section title={t.isError ? '✗ Output (error result)' : 'Output'} error={t.isError}>
      {t.output === '' ? <p className="muted tc-empty">No output.</p> : <Clipped text={t.output} className={t.isError ? 'tool-result error' : 'tool-result'} />}
    </Section>
  );
}

function ToolBody({ t }: { t: ToolItem }) {
  const kind = toolKind(t.name);
  const edit = kind === 'edit' ? editOf(t.name, t.input, t.output) : null;
  const cmd = commandOf(t, kind);
  const raw = JSON.stringify(t.input, null, 2) ?? String(t.input);
  return (
    <div className="tc-body" data-raw-evidence>
      <InputSection edit={edit} cmd={cmd} raw={raw} />
      {(edit || typeof cmd === 'string') && (
        <details className="tc-raw">
          <summary>Raw input</summary>
          <pre>
            <Hl text={raw} />
          </pre>
        </details>
      )}
      <OutputSection t={t} />
    </div>
  );
}

/** One tool call: icon, name, what it acts on, duration and status. Expands to input and output. */
export function ToolCard({ t, open, onToggle, flash }: { t: ToolItem; open: boolean; onToggle: () => void; flash?: boolean }) {
  const kind = toolKind(t.name);
  const status = toolStatus(t);
  const dur = t.resultMs !== undefined ? durationText(Math.max(0, t.resultMs - t.calledMs)) : '';
  const summary = toolSummary(t.name, t.input);
  return (
    <div className={`tc k-${kind} t-${status}${t.parentCallId ? ' helper' : ''}${flash ? ' flash' : ''}`} data-item-key={t.key}>
      <button type="button" className="tc-head" aria-expanded={open} onClick={onToggle} title={`${KIND_WORD[kind]} · evidence: ◆ observed (captured tool call)`}>
        <span className={`tc-icon k-${kind}`} aria-hidden="true">
          <Icon name={KIND_ICON[kind]} size={13} />
        </span>
        <span className="tc-name" data-raw-evidence>
          <Hl text={t.name} />
        </span>
        {t.parentCallId ? <span className="tc-helper">↳ helper</span> : null}
        <span className="tc-sum" data-raw-evidence>
          <Hl text={summary} />
        </span>
        <span className="tc-meta">
          {dur && <span className="dur">{dur}</span>}
          <StatusMark status={status} />
          <Icon name={open ? 'chevron-down' : 'chevron-right'} size={13} className="tc-chev" />
        </span>
      </button>
      {open && <ToolBody t={t} />}
    </div>
  );
}

/** How many calls of each kind a group made, in the order each kind first appears. */
function kindCounts(tools: ToolItem[]) {
  const counts = new Map<ToolKind, number>();
  for (const t of tools) counts.set(toolKind(t.name), (counts.get(toolKind(t.name)) ?? 0) + 1);
  return counts;
}

function KindIcons({ counts }: { counts: Map<ToolKind, number> }) {
  return (
    <span className="tg-kinds" aria-label={[...counts].map(([k, n]) => `${KIND_WORD[k]} ${n}`).join(', ')}>
      {[...counts].map(([k, n]) => (
        <span key={k} className={`tg-kind k-${k}`} aria-hidden="true" title={`${KIND_WORD[k]} ×${n}`}>
          <Icon name={KIND_ICON[k]} size={12} />
          {n > 1 ? <span>{n}</span> : null}
        </span>
      ))}
    </span>
  );
}

function GroupStatus({ running, failed }: { running: ToolItem | undefined; failed: number }) {
  if (running) return <StatusMark status="running" />;
  return failed ? <span className="tstat t-error">✗ {failed} failed</span> : <span className="tstat t-ok">✓ all ok</span>;
}

type GroupProps = {
  tools: ToolItem[];
  open: boolean;
  onToggle: () => void;
  isOpen: (key: string) => boolean;
  toggleTool: (key: string) => void;
  flashKey: string | null;
};

/** "Used 6 tools": consecutive calls folded into one row, like a chat app. */
export function ToolGroup({ tools, open, onToggle, isOpen, toggleTool, flashKey }: GroupProps) {
  const failed = tools.map(toolStatus).filter((s) => s === 'error').length;
  const running = tools.find((t) => toolStatus(t) === 'running');
  return (
    <div className={`tg${open ? ' open' : ''}`} data-group-key={tools[0].key}>
      <button type="button" className="tg-head" aria-expanded={open} onClick={onToggle}>
        <Icon name={open ? 'chevron-down' : 'chevron-right'} size={13} className="tg-chev" />
        <span className="tg-title">Used {tools.length} tools</span>
        <KindIcons counts={kindCounts(tools)} />
        {running && !open ? (
          <span className="tg-now" data-raw-evidence>
            <Hl text={toolSummary(running.name, running.input)} />
          </span>
        ) : null}
        <span className="tc-meta"><GroupStatus running={running} failed={failed} /></span>
      </button>
      {open && (
        <div className="tg-body">
          {tools.map((t) => (
            <ToolCard key={t.key} t={t} open={isOpen(t.key)} onToggle={() => toggleTool(t.key)} flash={flashKey === t.key} />
          ))}
        </div>
      )}
    </div>
  );
}
