import type { EventRecord } from '../generated/EventRecord';
import { splitTeamMessage } from '../teamMessage';
export type TrafficMessage={id:string;from:string[];to:string;queued:number;delivered?:number;read?:number;failure?:string;text:string;briefing?:string;bytes:number;contextBytes:number;event:EventRecord};
const encoder=new TextEncoder();
// Events never change once recorded: each message's split and sizes are worked out once.
const parsed=new WeakMap<EventRecord,{text:string;briefing?:string;bytes:number;contextBytes:number}>();
function messageParts(e:EventRecord,text:string){let p=parsed.get(e);if(!p){const split=splitTeamMessage(text);p={text:split.text,briefing:split.briefing,bytes:encoder.encode(text).length,contextBytes:encoder.encode(split.briefing??'').length};parsed.set(e,p);}return p;}
type Event = EventRecord;
type Route = { key: string; from: string; to: string; count: number; bytes: number; failed: number; read: number; latency: number[] };

const uniqueBySeq = (input: Event[]) => [...new Map(input.map(e => [e.seq, e])).values()].sort((a, b) => a.seq - b.seq);

function handoffSenders(events: Event[]) {
  const handoffs = new Map<string, string[]>();
  for (const e of events) if (e.body.type === 'handed-over') handoffs.set(e.body.message_id, e.body.from.map(f => f.seat));
  return handoffs;
}

const sender = (e: Event) => [e.actor.kind === 'seat' ? e.actor.id : e.actor.kind === 'human' ? 'You' : 'System'];

/** Apply one delivery receipt to its message. */
function applyReceipt(m: TrafficMessage, e: Event) {
  const b = e.body;
  if (b.type === 'message-delivered') m.delivered = e.at_ms;
  if (b.type === 'message-consumed') m.read = e.at_ms;
  if (b.type === 'message-failed') m.failure = b.reason;
}

function trackMessages(events: Event[]) {
  const handoffs = handoffSenders(events);
  const messages = new Map<string, TrafficMessage>();
  let orphanReceipts = 0;
  for (const e of events) {
    const b = e.body;
    if (b.type === 'message-queued' && e.subject.kind === 'seat') {
      const from = handoffs.get(b.message_id) ?? sender(e);
      messages.set(b.message_id, { id: b.message_id, from, to: e.subject.id, queued: e.at_ms, ...messageParts(e, b.text), event: e });
    } else if (b.type === 'message-delivered' || b.type === 'message-consumed' || b.type === 'message-failed') {
      const m = messages.get(b.message_id);
      if (m) applyReceipt(m, e); else orphanReceipts++;
    }
  }
  return { messages, orphanReceipts };
}

function addToRoute(routes: Map<string, Route>, m: TrafficMessage, from: string) {
  const key = JSON.stringify([from, m.to]);
  const r = routes.get(key) ?? { key, from, to: m.to, count: 0, bytes: 0, failed: 0, read: 0, latency: [] };
  r.count++;
  r.bytes += m.bytes;
  r.failed += Number(!!m.failure);
  r.read += Number(m.read !== undefined);
  if (m.delivered !== undefined && m.delivered >= m.queued) r.latency.push(m.delivered - m.queued);
  routes.set(key, r);
}

function routesOf(rows: TrafficMessage[]) {
  const routes = new Map<string, Route>();
  for (const m of rows) for (const from of m.from) addToRoute(routes, m, from);
  return [...routes.values()].sort((a, b) => a.key.localeCompare(b.key));
}

function usageSince(events: Event[], since: number, agent: string) {
  const usage = { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, reports: 0 };
  let lost = 0;
  for (const e of events) {
    if (e.at_ms < since) continue;
    if (e.body.type === 'events-lost') lost += e.body.count;
    if (agent && (e.subject.kind !== 'seat' || e.subject.id !== agent)) continue;
    const b = e.body;
    if (b.type !== 'usage-reported') continue;
    usage.input += b.input_tokens;
    usage.output += b.output_tokens;
    usage.cacheRead += b.cache_read_tokens;
    usage.cacheWrite += b.cache_write_tokens;
    usage.reports++;
  }
  return { usage, lost };
}

export function communicationStats(input: Event[], since = 0, agent = '') {
  const events = uniqueBySeq(input);
  const { messages, orphanReceipts } = trackMessages(events);
  const rows = [...messages.values()].filter(m => m.queued >= since && (!agent || m.to === agent || m.from.includes(agent)));
  const routes = routesOf(rows);
  const { usage, lost } = usageSince(events, since, agent);
  return { rows: rows.sort((a, b) => b.queued - a.queued), routes, usage, lost, orphanReceipts, bytes: rows.reduce((n, m) => n + m.bytes, 0), contextBytes: rows.reduce((n, m) => n + m.contextBytes, 0) };
}
export function percentile(values:number[],q:number){if(!values.length)return null;const s=[...values].sort((a,b)=>a-b);return s[Math.max(0,Math.ceil(s.length*q)-1)];}
export function trafficBins(rows:TrafficMessage[],start:number,end:number,count=30){const span=Math.max(1,end-start);const bins=Array.from({length:count},(_,i)=>({at:start+i*span/count,count:0,bytes:0}));for(const m of rows){if(m.queued<start||m.queued>end)continue;const b=bins[Math.min(count-1,Math.floor((m.queued-start)/span*count))];b.count++;b.bytes+=m.bytes;}return bins;}
