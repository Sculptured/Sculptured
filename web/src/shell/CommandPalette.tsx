import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent, type RefObject } from 'react';
import { shortAge } from '../herdr';
import { pushRecent, readRecent, type Command } from './commands';
import type { AgentRow, TeamRow } from './model';
import { buildSections, FILTERS, type Filter, type Row, type Scope, type Section } from './paletteModel';
import { Badge, Icon, StateMark, VendorMark } from './ui';

export { matches, type Scope } from './paletteModel';

type Props = {
  teams: TeamRow[];
  commands: Command[];
  /** The open project, whose agents the short list shows. */
  project?: string;
  now: number;
  scope?: Scope;
  onJump: (a: AgentRow) => void;
  onClose: () => void;
};

/**
 * Ctrl+K: search agents and run commands. Before you type it shows what you ran recently, the
 * likely next steps here and the open project's agents; typing searches everything. `>` lists
 * only commands, `@` only agents. Each command shows its key, and a command that cannot run says
 * why in its row.
 */
export function CommandPalette(p: Props) {
  const ids = useId();
  const input = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  useFocusIn(input);
  const q = useQuery(p.scope ?? 'all');
  const sections = useSections(p, q);
  const flat = sections.flatMap((s) => s.rows);
  const c = useCursor(flat, listRef, [q.query, q.filters, q.scope]);
  const choose = (r: Row | undefined) => chooseRow(r, p);
  const optionId = (r: Row) => `${ids}-o-${r.key.replace(/[^a-zA-Z0-9_-]/g, '_')}`;
  const agentsOnly = q.scope === 'agents';
  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && p.onClose()}>
      <div className="palette panel cmd-palette" role="dialog" aria-modal="true" aria-label={agentsOnly ? 'Go to agent' : 'Command palette'} onKeyDown={paletteKeys({ q, flat, setCursor: c.setCursor, choose: () => choose(c.sel), onClose: p.onClose })}>
        <PaletteInput q={q} input={input} listId={`${ids}-list`} active={c.sel ? optionId(c.sel) : undefined} count={agentsOnly ? `${flat.length} of ${p.teams.reduce((n, t) => n + t.agents.length, 0)}` : null} />
        {agentsOnly && <AgentFilters filters={q.filters} toggle={(f) => { q.toggle(f); input.current?.focus(); }} />}
        <div className="palette-list" role="listbox" id={`${ids}-list`} aria-label={agentsOnly ? 'Agents' : 'Results'} ref={listRef}>
          {sections.length === 0 && <p className="muted pad">{agentsOnly ? 'No agents match.' : 'Nothing matches. Try fewer letters.'}</p>}
          {sections.map((s) => (
            <PaletteSection key={s.label} s={s} teams={p.teams} now={p.now} sel={c.sel} optionId={optionId} onHover={(r) => c.setCursor(() => flat.indexOf(r))} onChoose={choose} />
          ))}
        </div>
        <PaletteFoot sel={c.sel} scope={q.scope} />
      </div>
    </div>
  );
}

/** Focus goes to the search box, and back where it was when the palette closes. */
function useFocusIn(input: RefObject<HTMLInputElement | null>) {
  useEffect(() => {
    const restore = document.activeElement as HTMLElement | null;
    input.current?.focus();
    return () => {
      if (restore && document.contains(restore)) restore.focus?.();
    };
  }, [input]);
}

type Query = ReturnType<typeof useQuery>;

function useSections(p: Props, q: Query): Section[] {
  const [recent] = useState(readRecent);
  return useMemo(
    () => buildSections({ scope: q.scope, teams: p.teams, commands: p.commands, query: q.query, filters: q.filters, recent, project: p.project }),
    [q.scope, p.teams, p.commands, q.query, q.filters, recent, p.project],
  );
}

/** The typed text, the scope it chose (`>` commands, `@` agents) and the agent state filters. */
function useQuery(initial: Scope) {
  const [scope, setScope] = useState<Scope>(initial);
  const [query, setQuery] = useState('');
  const [filters, setFilters] = useState<Set<Filter>>(new Set());
  const onChange = (value: string) => {
    if (scope === 'all' && (value.startsWith('>') || value.startsWith('@'))) {
      setScope(value[0] === '>' ? 'commands' : 'agents');
      setQuery(value.slice(1).trimStart());
    } else setQuery(value);
  };
  const toggle = (f: Filter) =>
    setFilters((cur) => {
      const next = new Set(cur);
      if (next.has(f)) next.delete(f);
      else next.add(f);
      return next;
    });
  return { scope, setScope, query, onChange, filters, toggle };
}

/** The highlighted row: back to the first whenever the list changes, and kept in view. */
function useCursor(flat: Row[], listRef: RefObject<HTMLDivElement | null>, resetOn: unknown[]) {
  const [cursor, setCursor] = useState(0);
  const sel = flat[Math.min(cursor, flat.length - 1)];
  // eslint-disable-next-line react-hooks/exhaustive-deps -- resetOn is the list's inputs
  useEffect(() => setCursor(0), resetOn);
  useEffect(() => {
    if (sel) listRef.current?.querySelector(`[data-key="${CSS.escape(sel.key)}"]`)?.scrollIntoView?.({ block: 'nearest' });
  }, [sel, listRef]);
  return { sel, setCursor };
}

/** Runs a command (one that can run now) or jumps to an agent, and remembers it as recent. */
function chooseRow(r: Row | undefined, p: Pick<Props, 'onJump' | 'onClose'>) {
  if (!r || (r.kind === 'cmd' && r.cmd.disabled)) return;
  pushRecent(r.key);
  if (r.kind === 'agent') return p.onJump(r.a);
  p.onClose();
  r.cmd.run();
}

type KeyArgs = { q: Query; flat: Row[]; setCursor: (f: (c: number) => number) => void; choose: () => void; onClose: () => void };

function paletteKeys({ q, flat, setCursor, choose, onClose }: KeyArgs) {
  return (e: KeyboardEvent) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      return onClose();
    }
    if (e.altKey && !e.ctrlKey && !e.metaKey) return altFilter(e, q.toggle);
    const keys: Record<string, () => void> = {
      ArrowDown: () => setCursor((c) => Math.min(flat.length - 1, c + 1)),
      ArrowUp: () => setCursor((c) => Math.max(0, c - 1)),
      Enter: choose,
      // Keep focus in the dialog.
      Tab: () => {},
    };
    if (e.key === 'Backspace' && q.query === '' && q.scope !== 'all' && q.filters.size === 0) keys.Backspace = () => q.setScope('all');
    if (!keys[e.key]) return;
    e.preventDefault();
    keys[e.key]();
  };
}

/** Alt+b, Alt+w, Alt+i, Alt+d: filter agents by state. */
function altFilter(e: KeyboardEvent, toggle: (f: Filter) => void) {
  // e.code: Alt+letter types a symbol on some layouts.
  const letter = e.code.startsWith('Key') ? e.code.slice(3).toLowerCase() : e.key.toLowerCase();
  const f = FILTERS.find((x) => x.alt === letter);
  if (!f) return;
  e.preventDefault();
  toggle(f.key);
}

type InputProps = { q: Query; input: RefObject<HTMLInputElement | null>; listId: string; active?: string; count: string | null };

function PaletteInput({ q, input, listId, active, count }: InputProps) {
  const agentsOnly = q.scope === 'agents';
  return (
    <div className="palette-input">
      <span className="prompt" aria-hidden="true">
        <Icon name="search" size={18} />
      </span>
      {q.scope !== 'all' && <ScopeChip agentsOnly={agentsOnly} onAll={() => { q.setScope('all'); input.current?.focus(); }} />}
      <input
        ref={input}
        role="combobox"
        aria-expanded="true"
        aria-controls={listId}
        aria-activedescendant={active}
        aria-label={agentsOnly ? 'Search agents by name, team or folder' : 'Search agents and commands'}
        placeholder={agentsOnly ? 'search name, team or folder' : q.scope === 'commands' ? 'search commands' : 'search agents and commands'}
        value={q.query}
        spellCheck={false}
        autoComplete="off"
        onChange={(e) => q.onChange(e.target.value)}
      />
      {count && <Badge>{count}</Badge>}
    </div>
  );
}

function ScopeChip({ agentsOnly, onAll }: { agentsOnly: boolean; onAll: () => void }) {
  return (
    <button type="button" className="cmd-scope" onClick={onAll} aria-label={`Showing ${agentsOnly ? 'agents' : 'commands'} only; show everything`} data-tip="Backspace in an empty box shows everything">
      {agentsOnly ? 'Agents' : 'Commands'}
      <Icon name="x" size={12} />
    </button>
  );
}

function AgentFilters({ filters, toggle }: { filters: Set<Filter>; toggle: (f: Filter) => void }) {
  return (
    <div className="palette-filters" role="group" aria-label="Filters">
      {FILTERS.map((f) => (
        <button key={f.key} type="button" className="filter" aria-pressed={filters.has(f.key)} onClick={() => toggle(f.key)}>
          <kbd>alt+{f.alt}</kbd> {f.label}
        </button>
      ))}
    </div>
  );
}

type OptionProps = { sel?: Row; optionId: (r: Row) => string; onHover: (r: Row) => void; onChoose: (r: Row) => void };

function PaletteSection({ s, teams, now, ...o }: OptionProps & { s: Section; teams: TeamRow[]; now: number }) {
  return (
    <div role="group" aria-label={s.label} className="palette-group">
      <div className="palette-team" aria-hidden="true">
        {s.label}
      </div>
      {s.rows.map((r) =>
        r.kind === 'agent' ? (
          <AgentOption key={r.key} r={r} where={s.team ? r.a.group : (teams.find((t) => t.rig === r.a.rig)?.title ?? r.a.group)} now={now} {...o} />
        ) : (
          <CommandOption key={r.key} r={r} showSection={s.label !== r.cmd.section} {...o} />
        ),
      )}
    </div>
  );
}

function AgentOption({ r, where, now, sel, optionId, onHover, onChoose }: OptionProps & { r: Extract<Row, { kind: 'agent' }>; where: string; now: number }) {
  return (
    <div id={optionId(r)} role="option" aria-selected={r === sel} aria-label={`${r.a.name} — ${r.a.state.word}`} data-key={r.key} data-agent={r.a.id}
      className={`palette-row ${r.a.vendor.cls}${r === sel ? ' sel' : ''}`} onMouseMove={() => onHover(r)} onClick={() => onChoose(r)}>
      <StateMark state={r.a.state} />
      <VendorMark v={r.a.vendor} />
      <span className="row-name">{r.a.name}</span>
      <span className="row-group muted">{where}</span>
      <span className="row-age">{shortAge(r.a.lastMs, now)}</span>
    </div>
  );
}

function CommandOption({ r, showSection, sel, optionId, onHover, onChoose }: OptionProps & { r: Extract<Row, { kind: 'cmd' }>; showSection: boolean }) {
  const why = r.cmd.disabled;
  return (
    <div id={optionId(r)} role="option" aria-selected={r === sel} aria-disabled={why ? true : undefined} aria-label={r.cmd.title} aria-describedby={why ? `${optionId(r)}-why` : undefined}
      data-key={r.key} className={`palette-row cmd-row${r === sel ? ' sel' : ''}${why ? ' off' : ''}`} onMouseMove={() => onHover(r)} onClick={() => onChoose(r)}>
      <span className="cmd-mark" aria-hidden="true">
        <Icon name={why ? 'minus' : 'chevron-right'} size={13} />
      </span>
      <span className="cmd-title">
        {r.cmd.title}
        {why && <span className="cmd-why" id={`${optionId(r)}-why`}>{why}</span>}
      </span>
      {showSection && <span className="cmd-section muted">{r.cmd.section}</span>}
      {r.cmd.keys && <kbd className="cmd-keys">{r.cmd.keys}</kbd>}
    </div>
  );
}

function PaletteFoot({ sel, scope }: { sel?: Row; scope: Scope }) {
  return (
    <div className="palette-foot">
      {sel?.kind === 'agent' ? (
        <>
          <Icon name="folder" size={14} className="muted" />
          <span className="sr-only">folder </span>
          <span className="palette-folder" data-testid="palette-folder">{sel.a.folder}</span>
        </>
      ) : (
        <span className="muted cmd-hint">
          {scope === 'all' ? <><kbd>&gt;</kbd> commands only · <kbd>@</kbd> agents only</> : <><kbd>⌫</kbd> show everything</>}
        </span>
      )}
      <span className="pane-spacer" />
      <span className="muted" aria-hidden="true">
        <kbd>↑↓</kbd> move <kbd>⏎</kbd> {sel?.kind === 'agent' ? 'jump' : 'run'} <kbd>esc</kbd> close
      </span>
    </div>
  );
}

const MAC = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);

/** The quiet way in: looks like a search box, says its key. */
export function CommandTrigger({ onOpen }: { onOpen: () => void }) {
  return (
    <button type="button" className="cmd-trigger" onClick={onOpen} aria-keyshortcuts="Control+K Meta+K" data-tip="Go to an agent or run any command">
      <Icon name="search" size={14} />
      <span className="cmd-trigger-text">Search or run a command</span>
      <kbd aria-hidden="true">{MAC ? '⌘K' : 'Ctrl K'}</kbd>
    </button>
  );
}
