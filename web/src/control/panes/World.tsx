import { useEffect, useRef, useState, type FormEvent } from 'react';
import { ArenaProblem } from '../../arena/gate';
import { pretty } from '../../tests/shared';
import { useSelection } from '../../workspace/selection';
import { useControl } from '../context';
import { useCursor } from './cursor';

/**
 * World state: the world's rows as they were saved at the cursor (the latest save at or before
 * it). The key comes from the selection when there is one; otherwise every row the run had touched
 * by then. A row's key selects it for every pane that follows the selection.
 */

type Rows = { state: 'loading' } | { state: 'ready'; rows: Record<string, unknown> } | { state: 'failed'; error: unknown };

/** The rows at `save`, read again when the key, the branch or Arena's return (`epoch`) changes. */
function useRows(save: number | null, key: string | undefined, branch: string, epoch: number) {
  const { source } = useControl();
  const [rows, setRows] = useState<Rows>({ state: 'loading' });
  // The source's reader is new on each render; the rows depend on the save, the key and the branch.
  const read = useRef(source.world?.rows);
  useEffect(() => { read.current = source.world?.rows; });
  useEffect(() => {
    if (save === null || !read.current) return;
    let alive = true;
    setRows({ state: 'loading' });
    read.current(key).then((r) => { if (alive) setRows({ state: 'ready', rows: r }); }).catch((error: unknown) => { if (alive) setRows({ state: 'failed', error }); });
    return () => { alive = false; };
  }, [save, key, branch, epoch]);
  return rows;
}

function KeyField({ value, onKey }: { value?: string; onKey(k: string | undefined): void }) {
  const [text, setText] = useState(value ?? '');
  const [shown, setShown] = useState(value);
  if (shown !== value) { setShown(value); setText(value ?? ''); }
  const submit = (e: FormEvent) => { e.preventDefault(); onKey(text.trim() || undefined); };
  return (
    <form className="ctl-world-key" onSubmit={submit} role="search" aria-label="Row key">
      <label className="ctl-field">Key <input className="ctl-mono" value={text} placeholder="kind/id, e.g. orders/#W1" onChange={(e) => setText(e.target.value)} /></label>
      <button type="submit" className="btn btn-sm btn-secondary">Show</button>
      {value && <button type="button" className="btn btn-sm btn-ghost" onClick={() => onKey(undefined)}>All touched rows</button>}
    </form>
  );
}

function RowTable({ rows, picked, onPick }: { rows: Record<string, unknown>; picked?: string; onPick(k: string): void }) {
  const keys = Object.keys(rows).sort();
  if (keys.length === 0) return <p className="ctl-empty">No rows were touched by this step.</p>;
  return (
    <table className="ctl-table ctl-world-table" aria-label="Rows at the cursor">
      <thead><tr><th scope="col">Key</th><th scope="col">Value (as saved)</th></tr></thead>
      <tbody>
        {keys.map((k) => (
          <tr key={k} className={k === picked ? 'is-picked' : undefined}>
            <td><button type="button" className="ctl-key" aria-pressed={k === picked} onClick={() => onPick(k)}>{k}</button></td>
            <td>{rows[k] == null ? <span className="ctl-muted">absent</span> : <pre className="ctl-json" data-raw-evidence>{pretty(rows[k])}</pre>}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Body({ rows, picked, onPick }: { rows: Rows; picked?: string; onPick(k: string): void }) {
  if (rows.state === 'loading') return <p className="ctl-empty">Reading the saved rows…</p>;
  if (rows.state === 'failed') return <div className="ctl-empty"><ArenaProblem error={rows.error} /></div>;
  return <RowTable rows={rows.rows} picked={picked} onPick={onPick} />;
}

export function WorldPane() {
  const { source } = useControl();
  const { selection, select } = useSelection();
  const tl = source.timeline!;
  const b = source.arena!.branch!;
  useCursor(tl, b);
  const save = source.world?.save ?? null;
  const rows = useRows(save, selection.key, b.id, source.arena!.reconnects);
  const pick = (k: string | undefined) => select({ key: k });
  return (
    <div className="ctl-world">
      <p className="ctl-pane-head">
        <span>{save === null ? `Nothing was saved by step ${tl.cursor}.` : <>As saved after step <b className="ctl-num">{save}</b>{save !== tl.cursor ? ` (the latest save at or before step ${tl.cursor})` : ''}</>}</span>
        <span>{selection.key ? <>Key <code className="ctl-mono">{selection.key}</code> (selected)</> : 'Every row touched by then'}</span>
      </p>
      <KeyField value={selection.key} onKey={pick} />
      {save === null ? <p className="ctl-empty">Move the cursor to a step with a save (◆ in the Timeline’s Saves lane).</p> : <Body rows={rows} picked={selection.key} onPick={pick} />}
    </div>
  );
}
