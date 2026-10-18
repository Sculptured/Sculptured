import { useMemo, useState } from 'react';
import type { EventRecord } from '../generated/EventRecord';
import { contextEvents, contextReports, deriveContext, CONTEXT_LABEL, type ContextKind } from './context';
import './context.css';
import { plural } from '../format';

type View = { category: ContextKind | null; snapshot: string };
const views = new Map<string, View>();
const number = (n: number) => n.toLocaleString();
const time = (n: number) => new Date(n).toLocaleString();
export function ContextOverview({ seatId, events, partial, loading, onJump }: { seatId: string; events: EventRecord[]; partial: boolean; loading: boolean; onJump: (key:string)=>void }) {
  const scope = useMemo(()=>contextEvents(events),[events]);
  const reports = useMemo(()=>contextReports(scope.events).slice(-20),[scope.events]);
  const [view,setView] = useState<View>(()=>views.get(seatId) ?? {category:null,snapshot:'latest'});
  const [more,setMore] = useState(false);
  const update = (patch:Partial<View>) => { setMore(false); setView(v=>{ const next={...v,...patch}; views.set(seatId,next); if(views.size>40)views.delete(views.keys().next().value!); return next; }); };
  const cutoff = view.snapshot === 'latest' ? Infinity : Number(view.snapshot);
  const missing = view.snapshot !== 'latest' && !reports.some(e=>String(e.seq)===view.snapshot);
  const data = useMemo(()=>deriveContext(scope.events.filter(e=>e.seq<=cutoff)),[scope.events,cutoff]);
  const report = data.report, used = report?.body.used_tokens, window = report?.body.window_tokens;
  const percentage = used !== undefined && window && window > 0 ? Math.round(used/window*100) : null;
  const rows = data.entries.filter(e=>!view.category || e.kind===view.category).slice().reverse();
  const latestReportIsOld = !!report && scope.events.some(e=>e.seq>report.seq && ['message-consumed','tool-called','tool-resulted','agent-text'].includes(e.body.type));
  return <section className="ctx-panel" aria-label="Context information">
    <header><div><h3>Context</h3><p>Captured content since the latest session start or resume.</p></div>
      <label>Context snapshot<select value={view.snapshot} onChange={e=>update({snapshot:e.target.value})}><option value="latest">Latest captured content</option>{missing && <option value={view.snapshot}>Snapshot no longer loaded</option>}{reports.slice().reverse().map(e=><option key={e.seq} value={e.seq}>{time(e.at_ms)} · {number(e.body.used_tokens)} tokens</option>)}</select></label>
    </header>
    {loading && <p role="status">Loading context evidence…</p>}
    {missing ? <p role="status">This snapshot is outside loaded history. Select the latest content to continue.</p> : <>
      <div className="ctx-usage"><strong>{used === undefined ? 'Context usage unknown' : `${number(used)}${window ? ` / ${number(window)}` : ''} tokens${percentage === null ? '' : ` · ${percentage}%`}`}</strong>
        <span>{report ? `${report.body.basis === 'estimated' ? 'CLI-derived estimate' : 'CLI-reported usage'} · ${time(report.at_ms)}` : 'The CLI has not reported usage in this loaded session.'}</span>
        {percentage !== null && <meter aria-label="Last reported context usage" min={0} max={window!} value={Math.min(used!,window!)} />}
        {used !== undefined && !window && <span>Context window size unknown.</span>}
        {latestReportIsOld && view.snapshot==='latest' && <span>More content was captured after this usage report.</span>}
      </div>
      <p className="ctx-caveat">{partial || !scope.boundaryKnown || data.gap ? 'Partial history. ' : ''}Captured text is not an exact map of the current model context. Compacted, inherited, hidden instructions and omitted tool content are unknown.</p>
      <div className="ctx-total"><strong>{number(data.chars)} captured characters</strong><span>{plural(data.entries.length, 'record')} · {plural(data.paths.length, 'structured file path')} · {plural(data.urls.length, 'URL reference')}</span></div>
      {data.chars > 0 && <div className="ctx-bar" aria-hidden="true">{data.categories.filter(c=>c.chars>0).map(c=><span key={c.kind} className={`ctx-${c.kind}`} style={{flexGrow:c.chars}} />)}</div>}
      <div className="ctx-categories" role="group" aria-label="Captured content categories">{data.categories.map(c=><button key={c.kind} className={`ctx-category ctx-${c.kind}`} aria-pressed={view.category===c.kind} onClick={()=>update({category:view.category===c.kind?null:c.kind})}>
        <strong>{CONTEXT_LABEL[c.kind]}</strong><span>{plural(c.count, 'record')} · {number(c.chars)} chars</span><small>≈ {number(c.estimatedTokens)} tokens</small>
      </button>)}</div>
      <p className="ctx-caveat">Category tokens are a rough character ÷ 4 estimate, not tokenizer counts or shares of the reported context window. Repeated content is counted each time captured.</p>
      {data.pending>0 && <p>{data.pending} messages without captured consumption are excluded.</p>}
      {data.hiddenThinking>0 && <p>{plural(data.hiddenThinking, 'reasoning record')} excluded; their retained size is unknown.</p>}
      <details className="ctx-sources"><summary>Inspect referenced paths and URLs</summary><p>URLs come from captured text; file paths come from explicit fields in tool inputs. Paths embedded in shell commands or prose are not counted. References do not prove files or pages were read.</p><ul>{data.paths.map(p=><li key={`p:${p}`}><code>{p}</code></li>)}{data.urls.map(u=><li key={`u:${u}`}><a href={u} target="_blank" rel="noopener noreferrer">{u}</a></li>)}</ul>{!data.paths.length&&!data.urls.length&&<p>No references captured.</p>}</details>
      <details className="ctx-records" open={view.category ? true : undefined}><summary>Inspect captured content</summary><h4>{view.category?CONTEXT_LABEL[view.category]:'All captured content'}</h4>{view.category&&<button className="link-btn" onClick={()=>update({category:null})}>Show all categories</button>}
        {!rows.length&&<p>No captured content in this selection.</p>}
        <ol>{rows.slice(0,more?100:6).map(e=><li key={e.id}><details><summary>{e.label} · {number(e.chars)} chars · {time(e.at)}</summary><p>{e.provenance} · event {e.seq}</p><pre data-raw-evidence>{e.text.slice(0,3000)}{e.text.length>3000?'\n… Open transcript for the full captured record.':''}</pre>{e.key?<button className="link-btn" onClick={()=>onJump(e.key!)}>Open transcript record</button>:<p>Original call is outside loaded history; inspect Events for this result.</p>}</details></li>)}</ol>
        {rows.length>6&&<button className="btn" onClick={()=>setMore(v=>!v)}>{more?'Show fewer records':`Show more records (${Math.min(rows.length,100)})`}</button>}{more&&rows.length>100&&<p>Showing the newest 100 matching records; use the transcript for earlier evidence.</p>}
      </details><footer>{data.lastAt?`Last captured event ${time(data.lastAt)} · ${seatId}`:'No captured events yet.'}</footer>
    </>}
  </section>;
}
