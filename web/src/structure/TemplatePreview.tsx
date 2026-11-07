import { useId } from 'react';
import { workflowLinks, inputBox, stageBoxes } from '../workflow/WorkflowGraph';
import type { AgentKind } from '../generated/AgentKind';
import type { TeamDraft } from '../generated/TeamDraft';
import { Icon, type IconName } from '../shell/ui';
import { CARD_H, CARD_W } from '../layout';
import { kindVendor } from './Panels';
import { vendor } from '../herdr';
import { plural } from '../format';

const KIND_WORD: Record<AgentKind, string> = { claude: 'Claude', codex: 'Codex', opencode: 'OpenCode', practice: 'Practice' };
const kindIcon = (k: AgentKind): IconName => k;

/** "2 agents · Claude, Codex". */
export function previewCaption(d: TeamDraft): string {
  const n = d.agents.length;
  if (n === 0) return 'empty · add agents yourself';
  const kinds = [...new Set(d.agents.map((a) => KIND_WORD[a.kind]))];
  return `${plural(n, 'agent')} · ${kinds.join(', ')}`;
}

/** A card name on one line, or on two smaller lines when it is long; never cut mid-word unless one word is too long. */
export function previewNameLines(name: string): { lines: string[]; small: boolean } {
  const n = name.trim();
  if (n.length <= 9) return { lines: [n], small: false };
  const MAX = 12;
  const cut = (w: string) => (w.length > MAX ? `${w.slice(0, MAX - 1)}…` : w);
  const words = n.split(/\s+/);
  if (words.length === 1) return { lines: [cut(n)], small: true };
  let first = words[0];
  let i = 1;
  while (i < words.length && `${first} ${words[i]}`.length <= MAX) first = `${first} ${words[i++]}`;
  const rest = words.slice(i).join(' ');
  return { lines: rest ? [cut(first), cut(rest)] : [cut(first)], small: true };
}

function PreviewName({ x, y, name }: { x: number; y: number; name: string }) {
  const { lines, small } = previewNameLines(name);
  if (!small) return <text x={x} y={y} className="tpl-name">{lines[0]}</text>;
  return (
    <text x={x} y={lines.length > 1 ? y - 12 : y} className="tpl-name ux-tpl-name-small">
      {lines.map((l, i) => <tspan key={i} x={x} dy={i === 0 ? 0 : 22}>{l}</tspan>)}
    </text>
  );
}

function EmptyPreview() {
  return (
    <svg className="tpl-svg" viewBox="0 0 240 120" aria-hidden="true">
      <rect x="70" y="30" width="100" height="60" rx="10" className="tpl-empty" />
      <text x="120" y="65" textAnchor="middle" className="tpl-plus">
        +
      </text>
    </svg>
  );
}

type Node = { id: string; name: string; kind: AgentKind | null; x: number; y: number };

/** What the drawing shows as cards: the agents, and for a workflow its input and stages. */
function previewNodes(draft: TeamDraft): Node[] {
  const stages = Object.entries(stageBoxes(draft)).map(([id, b]) => ({ id, name: draft.workflow!.stages.find(s => `$stage:${s.id}` === id)?.name ?? id, kind: null, ...b }));
  return draft.workflow?.dispatcher ? [{ id: '$workflow-input', name: 'Input', kind: null, ...inputBox(draft) }, ...draft.agents, ...stages] : draft.agents;
}

/** The drawing's view box: everything drawn, with a margin. */
function previewBounds(nodes: Node[], draft: TeamDraft) {
  const boxes = [...nodes.map(a => ({ ...a, w: CARD_W, h: CARD_H })), ...draft.groups];
  const x0 = Math.min(...boxes.map(b => b.x)) - 24, y0 = Math.min(...boxes.map(b => b.y)) - 24;
  return { x0, y0, w: Math.max(...boxes.map(b => b.x + b.w)) + 24 - x0, h: Math.max(...boxes.map(b => b.y + b.h)) + 24 - y0 };
}

function PreviewEdge({ a, b, id, arrowId }: { a: Node; b: Node; id: string; arrowId: string }) {
  const x1 = a.x + CARD_W;
  const y1 = a.y + CARD_H / 2;
  const x2 = b.x;
  const y2 = b.y + CARD_H / 2;
  const dx = Math.max(30, (x2 - x1) / 2);
  return <path key={id} className="tpl-edge" d={`M${x1},${y1} C${x1 + dx},${y1} ${x2 - dx},${y2} ${x2 - 6},${y2}`} markerEnd={`url(#${arrowId})`} />;
}

function PreviewAgent({ a }: { a: Node }) {
  const v = a.kind ? kindVendor(a.kind) : vendor(undefined);
  return (
    <g className={`tpl-agent ${v.cls}`}>
      <rect x={a.x} y={a.y} width={CARD_W} height={CARD_H} rx={22} />
      <rect x={a.x + 16} y={a.y + 14} width={36} height={36} rx={10} className="tpl-head" />
      <svg x={a.x + 22} y={a.y + 20} width={24} height={24} className="tpl-mark">
        <Icon name={a.kind ? kindIcon(a.kind) : 'bot'} size={24} strokeWidth={2.4} />
      </svg>
      <PreviewName x={a.x + 64} y={a.y + 42} name={a.name} />
      <line x1={a.x + 16} x2={a.x + CARD_W - 40} y1={a.y + 80} y2={a.y + 80} className="tpl-line" />
      <line x1={a.x + 16} x2={a.x + CARD_W - 70} y1={a.y + 108} y2={a.y + 108} className="tpl-line" />
      <line x1={a.x + 16} x2={a.x + CARD_W - 100} y1={a.y + 136} y2={a.y + 136} className="tpl-line" />
    </g>
  );
}

/** A small drawing of a template's agents and who hands work to whom. */
export function TemplatePreview({ draft }: { draft: TeamDraft }) {
  const arrowId = useId();
  if (draft.agents.length === 0) return <EmptyPreview />;
  const nodes = previewNodes(draft);
  const links = draft.workflow ? workflowLinks(draft) : draft.connections;
  const { x0, y0, w, h } = previewBounds(nodes, draft);
  const at = (id: string) => nodes.find((a) => a.id === id)!;
  return (
    <svg className="tpl-svg" viewBox={`${x0} ${y0} ${w} ${h}`} preserveAspectRatio="xMidYMid meet" aria-hidden="true">
      <defs>
        <marker id={arrowId} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="5" markerHeight="5" orient="auto-start-reverse">
          <path d="M0,0 L10,5 L0,10 z" className="tpl-arrowhead" />
        </marker>
      </defs>
      {draft.groups.map(g => <g key={g.id}><rect x={g.x} y={g.y} width={g.w} height={g.h} rx={18} className="tpl-empty" /><text x={g.x + 16} y={g.y + 28} className="tpl-name">{g.label}</text></g>)}
      {links.map((c) => {
        const a = at(c.from), b = at(c.to);
        return a && b ? <PreviewEdge key={c.id} id={c.id} a={a} b={b} arrowId={arrowId} /> : null;
      })}
      {nodes.map((a) => <PreviewAgent key={a.id} a={a} />)}
    </svg>
  );
}
