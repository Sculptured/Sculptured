import type { PaneProps } from '../../workspace/registry';
import { useSelection } from '../../workspace/selection';
import { clockAt, nameIn, useControl } from '../context';
import type { FeedItem } from '../source';

/**
 * Raw events: the team's event journal, every recorded entry verbatim, newest first. It is
 * evidence (`data-raw-evidence`), shown as recorded. (The server keeps no other logs to show.)
 */

const SHOWN = 300;

export type RawSettings = { onlySelected?: boolean };

/** What a deployment's record carries besides its type; a simulated source's records may lack them. */
type Recorded = { seq?: number; provenance?: string; actor?: { kind?: string; id?: string } };

function Row({ item }: { item: FeedItem }) {
  const { source } = useControl();
  const r = (item.raw ?? {}) as Recorded;
  return (
    <tr>
      <td className="ctl-num">{r.seq ?? '—'}</td>
      <td className="ctl-t">{clockAt(item.at)}</td>
      <td>{nameIn(source, item.agent)}</td>
      <td className="ctl-mono">{item.kind}</td>
      <td className="ctl-mono">{r.provenance ?? 'unknown'}</td>
      <td><details><summary>JSON</summary><pre>{JSON.stringify(item.raw, null, 2)}</pre></details></td>
    </tr>
  );
}

/** What the table holds: the whole journal, or (cut here, or when loading) only its latest events. */
export function rawNote(count: number, cut: boolean): string {
  if (count > SHOWN || cut) return `Latest ${Math.min(count, SHOWN)} recorded events, verbatim, newest first; older ones are not shown here.`;
  return 'The event journal: what was recorded, verbatim, newest first.';
}

export function RawEventsPane({ settings }: PaneProps) {
  const s = settings as RawSettings;
  const { source } = useControl();
  const { selection } = useSelection();
  const rows = source.feed.filter((i) => !s.onlySelected || !selection.agent || i.agent === selection.agent).slice(-SHOWN).reverse();
  return (
    <div className="ctl-raw">
      <p className="ctl-note">{rawNote(source.feed.length, source.historySince !== undefined)}</p>
      <table className="ctl-table ctl-raw-table" data-raw-evidence aria-label="Raw events">
        <thead><tr><th scope="col" className="ctl-num">#</th><th scope="col">Time</th><th scope="col">Agent</th><th scope="col">Type</th><th scope="col">Evidence</th><th scope="col">Record</th></tr></thead>
        <tbody>{rows.map((i) => <Row key={i.id} item={i} />)}</tbody>
      </table>
      {rows.length === 0 && <p className="ctl-empty">Nothing recorded yet.</p>}
    </div>
  );
}

export function RawSettingsForm({ settings, setSettings }: PaneProps) {
  const s = settings as RawSettings;
  return <label className="ctl-check"><input type="checkbox" checked={!!s.onlySelected} onChange={(e) => setSettings({ ...s, onlySelected: e.target.checked })} />Only the selected agent</label>;
}
