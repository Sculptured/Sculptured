import '../groups/groups.css';
import { GroupPalette } from '../groups/GroupPalette';
import { folderForAll } from '../draft/folders';
import { fullTextLayout } from '../draft/fullText';
import { insertBlock } from '../groups/blocks';
import type { GroupBlock } from '../generated/GroupBlock';
import { reconnectWorkflowRoute } from '../workflow/routeEdits';
import { ArrowHandles } from './ArrowHandles';
import { closedGroupBox, GroupVisibilityButton, projectGroups, routeGroupEdges, useGroupVisibility } from '../shell/groupVisibility';
import { effectiveModel } from '../draft/models';
import { modelLabel } from './ModelControls';
import { useCallback, useEffect, useId, useImperativeHandle, useMemo, useRef, useState, type ReactNode, type Ref } from 'react';
import { isTextField, trackPointer } from '../dom';
import { moveAgents, removeAgents } from '../draft/bulk';
import { SelectionBar } from './SelectionBar';
import type { AgentKind } from '../generated/AgentKind';
import type { CompiledEdge } from '../generated/CompiledEdge';
import type { ConnectionKind } from '../generated/ConnectionKind';
import type { DraftIssue } from '../generated/DraftIssue';
import type { EdgeKind } from '../generated/EdgeKind';
import type { TeamDraft } from '../generated/TeamDraft';
import { addAgent, addGroup, connect, reconnect, moveAgent, moveGroup, removeElement, type Selection } from '../draft/edits';
import { agentAt, agentBox, alignCard, BIG_STEP, drawnGroups, groupAt, nextFreeSlot, overlaps, STEP, type Point } from '../draft/geometry';
import { BoardMenu, useBoardClipboard, type MenuOpen } from './BoardMenu';
import { inputAllowed, teamInputBox, INPUT_H, INPUT_W } from '../draft/input';
import { InputEdges, InputEditor, InputNode, INPUT_WIRE } from './InputNode';
import { useTeamInput } from './useTeamInput';
import { OFFERED_KINDS } from './ModelControls';
import { groupOf } from '../draft/ids';
import { errorsOf, FOLDER_ISSUE_WORDS, issuesFor, MARK } from '../draft/issues';
import { CARD_H, CARD_W, type Box } from '../layout';
import { placeLabels } from '../shell/labels';
import { Btn, Icon, VendorMark } from '../shell/ui';
import { usePanZoom, type Bounds } from '../shell/viewport';
import { CONNECTION_WORDS, NOT_ENFORCED } from '../vocab';
import { ConnectionLegend } from '../shell/HelpOverlay';
import { FolderPicker, shortPath, useHome } from './FolderPicker';
import { connectionSentence, kindVendor, LOCK_REASON, nameOf, type Edit } from './Panels';
import { markClass, type Marks } from './assistant/compare';
import { ChangeBadge } from './assistant/ChangeBadge';
import { QUIET_AFTER, useArrowFocus } from './arrowFocus';
import { handoffWords } from '../shell/model';
import { plural } from '../format';
import { logoRouteBox, logoRouteBoxes, useBoardStyle } from '../shell/boardStyle';
import { LogoFace, logoClasses } from '../shell/LogoNode';
import { ComponentChips } from './ComponentChips';
import { useHosted } from '../hosted/HostedGate';
import { useAgentComputers } from '../hosted/RunOn';
import { workflowBounds, inputBox, stageBoxes, workflowLinks, WorkflowGraph, WorkflowAgentActions, type WorkflowTarget } from '../workflow/WorkflowGraph';

export type CanvasHandle = {
  fit: () => void;
  zoomBy: (f: number) => void;
  focusPalette: () => void;
  /** Pans to the element and moves keyboard focus to it. */
  show: (s: Selection) => void;
  add: (kind: AgentKind) => void;
  addGroup: () => void;
  /** Selects every agent (Ctrl+A). */
  selectAll: () => void;
};

const EDGE_KIND: Record<ConnectionKind, EdgeKind> = { 'hands-work-to': 'delegates_to', 'reports-to': 'escalates_to', watches: 'can_observe' };
const PALETTE: AgentKind[] = OFFERED_KINDS;
const PALETTE_WORD: Record<AgentKind, string> = { claude: 'Claude', codex: 'Codex', practice: 'Practice' };

/** Whether More tools is open, per browser: newcomers see one Add agent button. */
export const MORE_TOOLS_KEY = 'sculptured.design.more-tools.v1';
function readMoreTools(): boolean {
  try { return globalThis.localStorage?.getItem(MORE_TOOLS_KEY) === 'true'; } catch { return false; }
}
function writeMoreTools(open: boolean) {
  try { globalThis.localStorage?.setItem(MORE_TOOLS_KEY, String(open)); } catch { /* Per-browser convenience only. */ }
}
/** The kind "Add agent" adds: the team's most common offered kind, else the first offered. */
export function mainKindOf(d: TeamDraft): AgentKind {
  const counts = PALETTE.map((k) => [k, d.agents.filter((a) => a.kind === k).length] as const);
  const best = counts.reduce((a, b) => (b[1] > a[1] ? b : a), counts[0]);
  return best[1] > 0 ? best[0] : PALETTE[0];
}

/** The model shown on a card: the one it will run with, or the CLI's default. */
const cardModel = (d: TeamDraft, a: TeamDraft['agents'][number]) =>
  a.kind === 'practice' ? 'free' : modelLabel(a.kind, effectiveModel(d, a).model);

/** `ids`: several selected agents dragged together (the card under the pointer is `id`). */
type Drag = { kind: 'agent' | 'group' | 'input' | 'stage' | 'team-input'; id: string; dx: number; dy: number; ids?: string[] };
type Wire = { from: string; to: Point };
type Ghost = { kind: AgentKind; at: Point } | null;

type Props = {
  teamId: string;
  groupOnly?: boolean;
  draft: TeamDraft;
  issues: DraftIssue[];
  selection: Selection | null;
  onSelect: (s: Selection | null) => void;
  onInspect?: (s: Selection) => void;
  /** Opens an agent's details page (a click on its card). */
  onOpen?: (id: string) => void;
  onWorkflow?: (target: WorkflowTarget) => void;
  edit: Edit;
  /** The team is running: structure is read-only, positions, names and jobs are not. */
  locked: boolean;
  onOpenAgent: (id: string) => void;
  canUndo: boolean;
  canRedo: boolean;
  onUndo: () => void;
  onRedo: () => void;
  ref?: Ref<CanvasHandle>;
  /** A proposal is shown: what each card, group and arrow is in it (see `assistant/compare.ts`). */
  marks?: Marks;
  /** Tools at the start of the toolbar row (the board's Design | Run switch). */
  leadTools?: ReactNode;
  /** Extra tools at the end of the toolbar row (the design assistant, the save state). */
  extraTools?: ReactNode;
};

/** The draft with a drag in progress applied, for drawing only. */
function withDrag(d: TeamDraft, drag: Drag | null, k: number): TeamDraft {
  if (!drag) return d;
  const dx = drag.dx / k;
  const dy = drag.dy / k;
  if (drag.kind === 'team-input' && d.input) return { ...d, input: { ...d.input, x: d.input.x + dx, y: d.input.y + dy } };
  if (drag.kind === 'input' && d.workflow) { const b=inputBox(d); return {...d,workflow:{...d.workflow,input_position:{x:b.x+dx,y:b.y+dy}}}; }
  if (drag.kind === 'stage' && d.workflow) { const b=stageBoxes(d)[`$stage:${drag.id}`]; return b ? {...d,workflow:{...d.workflow,stages:d.workflow.stages.map(s=>s.id===drag.id?{...s,position:{x:b.x+dx,y:b.y+dy}}:s)}} : d; }
  if (drag.kind === 'agent' && drag.ids) return { ...d, agents: d.agents.map((a) => (drag.ids!.includes(a.id) ? { ...a, x: a.x + dx, y: a.y + dy } : a)) };
  // A dragged card shows where it will land: where it is, or lined up with a nearby card.
  if (drag.kind === 'agent') return { ...d, agents: d.agents.map((a) => (a.id === drag.id ? { ...a, ...alignCard(d, a.id, { x: a.x + dx, y: a.y + dy }) } : a)) };
  return {
    ...d,
    groups: d.groups.map((g) => (g.id === drag.id ? { ...g, x: g.x + dx, y: g.y + dy } : g)),
    agents: d.agents.map((a) => (groupOf(a) === drag.id ? { ...a, x: a.x + dx, y: a.y + dy } : a)),
  };
}

function boundsOf(d: TeamDraft): Bounds | null {
  const boxes: Box[] = [...d.agents.map((a) => agentBox(a)), ...drawnGroups(d)];
  if (d.workflow) boxes.push(...workflowBounds(d));
  const input = teamInputBox(d);
  if (input) boxes.push(input);
  if (boxes.length === 0) return null;
  return {
    x0: Math.min(...boxes.map((b) => b.x)),
    y0: Math.min(...boxes.map((b) => b.y)) - 24,
    x1: Math.max(...boxes.map((b) => b.x + b.w)),
    y1: Math.max(...boxes.map((b) => b.y + b.h)) + 24,
  };
}

/**
 * The Structure board's drawing: agents, groups and connections on the shared layout. Every
 * pointer action has a keyboard path: add from the palette with Enter, move with the arrow
 * keys, connect and group from the agent panel.
 */
const bundleWords = (n: number) => `${n} arrows`;

export function StructureCanvas(p: Props) {
  const mark = (id: string) => markClass(p.marks, id);
  const af = useArrowFocus(p.draft, p.selection);
  const lit = (from: string, to: string, sel: boolean) => (sel || af.isLit(from, to) ? ' is-focus' : '');
  const { teamId, draft, issues, selection, onSelect, edit, locked } = p;
  const ids = useId();
  const { collapsed, setCollapsed } = useGroupVisibility(teamId);
  const home = useHome();
  // Hosted: an agent on another computer than the team's says which, in its folder line.
  const computers = useAgentComputers(teamId, useHosted().kind === 'signed-in');
  const { el, vp, vpRef, fit, zoomAt, startPan, reveal, toWorld, hold } = usePanZoom(teamId, useMemo(() => boundsOf(draft), [draft]));
  const [drag, setDrag] = useState<Drag | null>(null);
  const [wire, setWire] = useState<Wire | null>(null);
  const [groupGhost,setGroupGhost] = useState<{block:GroupBlock;at:Point}|null>(null);
  const groupDragged=useRef(false);
  const [ghost, setGhost] = useState<Ghost>(null);
  const [focusNext, setFocusNext] = useState<Selection | null>(null);
  const stops = useRef(new Set<() => void>());
  const pointerFocus = useRef(false);
  // Several agents selected at once (Shift or Ctrl+click, Shift+drag, Ctrl+A): moved, changed and deleted together.
  const [multi, setMulti] = useState<string[]>([]);
  const picked = useMemo(() => multi.filter((id) => draft.agents.some((a) => a.id === id)), [multi, draft.agents]);
  const many = picked.length > 1;
  const pickedRef = useRef(picked);
  pickedRef.current = picked;
  const [marquee, setMarquee] = useState<{ a: Point; b: Point } | null>(null);
  const togglePick = (id: string) =>
    setMulti((cur) => {
      const base = cur.length ? cur : selection?.kind === 'agent' && selection.id !== id ? [selection.id] : [];
      return base.includes(id) ? base.filter((x) => x !== id) : [...base, id];
    });
  // The Add toolbar: one primary "Add agent"; every other tool sits one step back, under More tools.
  const [moreTools, setMoreTools] = useState(() => readMoreTools());
  const toggleMoreTools = () => setMoreTools((m) => { writeMoreTools(!m); return !m; });
  const paletteRef = useRef<HTMLDivElement>(null);

  // Drags still going when the board unmounts stop listening.
  useEffect(() => {
    const all = stops.current;
    return () => {
      for (const s of all) s();
      all.clear();
    };
  }, []);
  const follow = (e: React.PointerEvent, move: (dx: number, dy: number, ev: PointerEvent) => void, end: (moved: boolean, ev: PointerEvent) => void) => {
    const stop = trackPointer(e, (dx,dy,ev) => { hold(); move(dx,dy,ev); }, (moved, ev) => {
      stops.current.delete(stop);
      end(moved, ev);
    });
    stops.current.add(stop);
  };

  // Logos style: each agent is its provider's logo, in the same slot; arrows end at the logo.
  const logos = useBoardStyle() === 'logos';
  // Full text: every card grows to show all its text; cards below move down (display only, never saved). Cards style only.
  const [fullTextWanted, setFullText] = useState(() => { try { return localStorage.getItem('sculptured.board.fullText') === '1'; } catch { return false; } });
  const fullText = fullTextWanted && !logos;
  const toggleFullText = () => setFullText((on) => { try { localStorage.setItem('sculptured.board.fullText', on ? '0' : '1'); } catch { /* per-viewer convenience only */ } return !on; });
  const [heights, setHeights] = useState<Record<string, number>>({});
  const slots = useRef(new Map<string, HTMLElement>());
  useEffect(() => {
    if (!fullText || typeof ResizeObserver === 'undefined') return;
    const measure = () => setHeights((old) => {
      const next: Record<string, number> = {};
      slots.current.forEach((el, id) => { next[id] = el.offsetHeight; });
      return Object.keys(next).length === Object.keys(old).length && Object.entries(next).every(([k, v]) => old[k] === v) ? old : next;
    });
    const observer = new ResizeObserver(measure);
    slots.current.forEach((el) => observer.observe(el));
    measure();
    return () => observer.disconnect();
  }, [fullText, draft.agents.length]);
  const cardHeight = useCallback((id: string) => (fullText ? Math.max(CARD_H, heights[id] ?? CARD_H) : CARD_H), [fullText, heights]);
  const shown = fullText ? fullTextLayout(withDrag(draft, drag, vp.k), heights) : withDrag(draft, drag, vp.k);
  const boxes = useMemo(() => Object.fromEntries(shown.agents.map((a) => [a.id, { ...agentBox(a), h: cardHeight(a.id) }])), [shown, cardHeight]);
  const edges: CompiledEdge[] = useMemo(
    () => shown.connections.map((c) => ({ kind: EDGE_KIND[c.kind], from: c.from, to: c.to, enforced: c.kind === 'hands-work-to' })),
    [shown.connections],
  );
  const projection = useMemo(() => projectGroups(boxes, Object.fromEntries(shown.agents.map(a => [a.id, groupOf(a)])), Object.fromEntries(drawnGroups(shown).map(g => [g.id, g])), collapsed), [boxes, shown, collapsed]);
  const routes = useMemo(() => routeGroupEdges(edges, logos ? { ...projection, boxes: logoRouteBoxes(projection.boxes) } : projection), [edges, projection, logos]);
  // A bundled arrow stands for several connections; it selects the collapsed group it leaves (or enters).
  const bundleGroup = (c: { from: string; to: string }): Selection => ({ kind: 'group', id: projection.hidden.get(c.from) ?? projection.hidden.get(c.to)! });
  const endName = (agent: string) => { const g = projection.hidden.get(agent); return g === undefined ? nameOf(draft, agent) : (draft.groups.find(x => x.id === g)?.label || 'Unnamed group'); };
  // Labels sit beside their curves, never on a curve, a card or another label.
  const spots = useMemo(
    () =>
      placeLabels(
        shown.connections.flatMap((c, i) => {
          const g = routes.get(edges[i]);
          return g ? [{ key: c.id, geom: g, text: g.merged.length > 1 ? bundleWords(g.merged.length) : `${CONNECTION_WORDS[c.kind]}${c.kind !== 'hands-work-to' ? ` · ${NOT_ENFORCED}` : ''}` }] : [];
        }),
        Object.values(projection.boxes),
      ),
    [shown.connections, routes, edges, projection],
  );
  const groups = drawnGroups(shown);

  const elementOf = useCallback(
    (s: Selection) =>
      el.current?.querySelector<HTMLElement>(
        s.kind === 'agent' ? `[data-agent="${CSS.escape(s.id)}"]` : s.kind === 'group' ? `[data-group="${CSS.escape(s.id)}"]` : `[data-connection="${CSS.escape(s.id)}"]`,
      ) ?? null,
    [el],
  );
  const boxOf = useCallback(
    (s: Selection): Box | null => {
      if (s.kind === 'agent') {
        const a = draft.agents.find((x) => x.id === s.id);
        return a ? agentBox(a) : null;
      }
      if (s.kind === 'group') return draft.groups.find((g) => g.id === s.id) ?? null;
      const c = draft.connections.find((x) => x.id === s.id);
      const a = c && draft.agents.find((x) => x.id === c.from);
      const b = c && draft.agents.find((x) => x.id === c.to);
      if (!a || !b) return null;
      const x = Math.min(a.x, b.x);
      const y = Math.min(a.y, b.y);
      return { x, y, w: Math.max(a.x, b.x) + CARD_W - x, h: Math.max(a.y, b.y) + CARD_H - y };
    },
    [draft],
  );

  useImperativeHandle(
    p.ref,
    () => ({
      fit,
      zoomBy: (f: number) => zoomAt(f),
      focusPalette: () => {
        paletteRef.current?.querySelector<HTMLElement>('.palette-btn:not(:disabled)')?.focus();
      },
      show: (s: Selection) => {
        const b = boxOf(s);
        if (b) reveal(b);
        setFocusNext(s);
      },
      add: (kind: AgentKind) => add(kind),
      addGroup: () => newGroup(),
      selectAll: () => selectAllRef.current(),
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- add and newGroup act through edit, which reads the latest draft
    [fit, zoomAt, reveal, boxOf],
  );

  // Ctrl+A selects every agent; Esc lets go of a selection of several. Not while typing or in a dialog.
  const selectAllRef = useRef(() => {});
  selectAllRef.current = () => {
    const all = draft.agents.filter((a) => !projection.hidden.has(a.id)).map((a) => a.id);
    setMulti(all);
    if (all.length === 1) onSelect({ kind: 'agent', id: all[0] });
  };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || isTextField(e.target) || document.querySelector('[aria-modal="true"]')) return;
      const t = e.target as HTMLElement;
      if (t !== document.body && !el.current?.closest('.sb-canvas')?.contains(t)) return;
      if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'a') {
        e.preventDefault();
        selectAllRef.current();
      } else if (e.key === 'Escape' && pickedRef.current.length > 0) {
        setMulti([]);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [el]);

  // Focus an element once it is drawn (a new card, a problem that was clicked).
  useEffect(() => {
    if (!focusNext) return;
    if (focusNext.kind === 'agent' && projection.hidden.has(focusNext.id)) {
      setCollapsed(projection.hidden.get(focusNext.id)!, false);
      return;
    }
    const node = elementOf(focusNext);
    if (node) {
      node.focus({ preventScroll: true });
      setFocusNext(null);
    }
  }, [focusNext, elementOf, draft, projection, setCollapsed]);

  const add = (kind: AgentKind, at?: Point) => {
    let id = '';
    const reason = edit((d) => {
      const r = addAgent(d, kind, at ?? nextFreeSlot(d));
      id = r.id;
      return r.draft;
    });
    if (!reason && id) {
      onSelect({ kind: 'agent', id });
      setFocusNext({ kind: 'agent', id });
    }
  };

  const [menu, setMenu] = useState<MenuOpen | null>(null);
  const selectNew = (s: Selection) => { onSelect(s); setFocusNext(s); };
  const clip = useBoardClipboard({ draft, selection, edit, locked, groupOnly: p.groupOnly, onPasted: selectNew });
  const menuAt = (e: React.MouseEvent, target: { kind: 'agent'; id: string } | { kind: 'group'; id: string } | { kind: 'board' } | { kind: 'input' }) => {
    e.preventDefault();
    e.stopPropagation();
    const at = toWorld(e.clientX, e.clientY);
    if (target.kind === 'agent' || target.kind === 'group') onSelect(target);
    setMenu(target.kind === 'agent' || target.kind === 'input' ? { ...target, x: e.clientX, y: e.clientY } : { ...target, at, x: e.clientX, y: e.clientY });
  };
  const boardActions = {
    add: (k: AgentKind, at: Point) => add(k, { x: at.x - CARD_W / 2, y: at.y - 24 }),
    addGroup: (at: Point) => newGroup(at),
    copy: (s: Selection) => void clip.copy(s),
    paste: clip.paste,
    duplicate: clip.duplicate,
    inspect: (s: Selection) => { onSelect(s); p.onInspect?.(s); },
    openAgent: (id: string) => (p.onOpen ? p.onOpen(id) : p.onOpenAgent(id)),
    addInput: (at: Point) => team.add(at),
    openInput: () => team.setOpen(true),
    fit,
  };

  const addBlock = (block:GroupBlock,at?:Point) => {
    if (locked) return;
    if (!at && groupDragged.current) {groupDragged.current=false;return;}
    let id:string|undefined;
    const reason=edit(d=>{const source=block.draft.groups[0]; const box=source && closedGroupBox(source); const placed=at && source && box ? {x:at.x-(box.x-source.x),y:at.y-(box.y-source.y)} : at; const r=insertBlock(d,block,placed);id=r.id;return r;});
    if (!reason&&id) {setCollapsed(id,true);onSelect({kind:'group',id});setFocusNext({kind:'group',id});}
    if (!reason&&block.boundary_notes.length) edit(d=>({draft:d,reason:'Group added. Reconnect its external arrows: '+block.boundary_notes.join('; ')}));
  };
  const blockDown = (block:GroupBlock,e:React.PointerEvent) => {
    if(e.button!==0||locked)return;
    e.stopPropagation();groupDragged.current=false;
    follow(e,(_dx,_dy,ev)=>setGroupGhost(overViewport(ev)?{block,at:toWorld(ev.clientX,ev.clientY)}:null),(moved,ev)=>{
      setGroupGhost(null);if(!moved)return;groupDragged.current=true;setTimeout(()=>{groupDragged.current=false;},0);
      if(overViewport(ev))addBlock(block,toWorld(ev.clientX,ev.clientY));
    });
  };

  const newGroup = (at?: Point) => {
    let id = '';
    edit((d) => {
      const r = addGroup(d, at);
      id = r.id;
      return r.draft;
    });
    if (id) {
      onSelect({ kind: 'group', id });
      setFocusNext({ kind: 'group', id });
    }
  };

  const overViewport = (ev: PointerEvent) => {
    const r = el.current?.getBoundingClientRect();
    return !!r && ev.clientX >= r.left && ev.clientX <= r.right && ev.clientY >= r.top && ev.clientY <= r.bottom;
  };

  /** Drag a palette entry onto the board; a click (no drag) adds at the next free place. */
  const paletteDown = (kind: AgentKind) => (e: React.PointerEvent) => {
    if (e.button !== 0 || locked) return;
    follow(
      e,
      (_dx, _dy, ev) => {
        if (!overViewport(ev)) return setGhost(null);
        const w = toWorld(ev.clientX, ev.clientY);
        setGhost({ kind, at: { x: w.x - CARD_W / 2, y: w.y - 24 } });
      },
      (moved, ev) => {
        setGhost(null);
        if (!moved || !overViewport(ev)) return;
        const w = toWorld(ev.clientX, ev.clientY);
        add(kind, { x: w.x - CARD_W / 2, y: w.y - 24 });
      },
    );
  };

  const dragged = useRef(false);
  /** A click (not the end of a drag) selects the agent and opens its details. */
  const openCard = (id: string, e?: React.MouseEvent) => {
    onSelect({ kind: 'agent', id });
    if (dragged.current) dragged.current = false;
    // Shift, Ctrl or Command+click adds the agent to the selection (or takes it out).
    else if (e && (e.shiftKey || e.ctrlKey || e.metaKey)) togglePick(id);
    else {
      setMulti([]);
      p.onOpen?.(id);
    }
  };

  const cardDown = (id: string) => (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    pointerFocus.current = true;
    dragged.current = false;
    // A card of a selection of several drags them all.
    const together = many && picked.includes(id) ? picked : null;
    follow(
      e,
      (dx, dy) => setDrag({ kind: 'agent', id, dx, dy, ids: together ?? undefined }),
      (moved, ev) => {
        pointerFocus.current = false;
        setDrag(null);
        if (!moved) return;
        dragged.current = true;
        if (together) {
          const k = vpRef.current.k;
          edit((d) => moveAgents(d, together, (ev.clientX - e.clientX) / k, (ev.clientY - e.clientY) / k, locked));
          return;
        }
        const a = draft.agents.find((x) => x.id === id);
        if (!a) return;
        const k = vpRef.current.k;
        const dx = ev.clientX - e.clientX;
        const dy = ev.clientY - e.clientY;
        // One edit on release, never one per pointer move.
        edit((d) => moveAgent(d, id, alignCard(d, id, { x: a.x + dx / k, y: a.y + dy / k }), locked));
      },
    );
  };

  // The Input box and stage boxes move the same way: drag, arrow keys, undo; a click (not a drag) opens them.
  type BoxTarget = 'input' | { stage: string };
  const placedBox = (d:TeamDraft, t:BoxTarget):Box => t === 'input' ? inputBox(d) : stageBoxes(d)[`$stage:${t.stage}`];
  const moveBox = (d:TeamDraft, t:BoxTarget, at:Point):TeamDraft => {
    if (!d.workflow) return d;
    const position = {x:Math.round(at.x/8)*8,y:Math.round(at.y/8)*8};
    return t === 'input' ? {...d,workflow:{...d.workflow,input_position:position}}
      : {...d,workflow:{...d.workflow,stages:d.workflow.stages.map(s=>s.id===t.stage?{...s,position}:s)}};
  };
  const boxControls = (t:BoxTarget) => ({
    onPointerDown: (e: React.PointerEvent<HTMLButtonElement>) => {
      if (e.button !== 0) return;
      e.stopPropagation(); pointerFocus.current = true; dragged.current = false;
      const start = placedBox(draft,t), k = vpRef.current.k;
      follow(e, (dx,dy) => setDrag(t === 'input' ? {kind:'input',id:'input',dx,dy} : {kind:'stage',id:t.stage,dx,dy}), (moved,ev) => {
        pointerFocus.current = false; setDrag(null);
        if (!moved) return;
        dragged.current = true;
        edit(d => moveBox(d,t,{x:start.x+(ev.clientX-e.clientX)/k,y:start.y+(ev.clientY-e.clientY)/k}));
      });
    },
    onFocus: () => { onSelect(null); if (!pointerFocus.current) reveal(placedBox(draft,t)); },
    onClick: () => { if (dragged.current) dragged.current = false; else p.onWorkflow?.(t); },
    onKeyDown: (e:React.KeyboardEvent<HTMLButtonElement>) => {
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      const step=e.shiftKey ? BIG_STEP : STEP, delta:Record<string,Point>={ArrowLeft:{x:-step,y:0},ArrowRight:{x:step,y:0},ArrowUp:{x:0,y:-step},ArrowDown:{x:0,y:step}};
      if (!delta[e.key]) return;
      e.preventDefault(); e.stopPropagation(); hold();
      const b=placedBox(draft,t), at={x:b.x+delta[e.key].x,y:b.y+delta[e.key].y};
      edit(d=>moveBox(d,t,at),t === 'input' ? 'move:input' : `move:stage:${t.stage}`); reveal({...b,...at});
    },
    title:'Drag to move · Arrow keys to move · Enter to open',
  });
  const inputControls = boxControls('input');
  const team = useTeamInput({ draft, edit, locked, follow, setDrag, vpRef, reveal, dragged, pointerFocus, onSelect, setWire, toWorld, agentAt: (at: Point) => agentAt({ ...draft, agents: draft.agents.filter(a => !projection.hidden.has(a.id)) }, at), menuAt, overViewport });

  const groupDown = (id: string) => (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    dragged.current = false;
    pointerFocus.current = true;
    follow(
      e,
      (dx, dy) => setDrag({ kind: 'group', id, dx, dy }),
      (moved, ev) => {
        pointerFocus.current = false;
        setDrag(null);
        if (!moved) return;
        const k = vpRef.current.k;
        dragged.current = true;
        edit((d) => moveGroup(d, id, (ev.clientX - e.clientX) / k, (ev.clientY - e.clientY) / k));
      },
    );
  };

  const portDown = (from: string) => (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    follow(
      e,
      (_dx, _dy, ev) => setWire({ from, to: toWorld(ev.clientX, ev.clientY) }),
      (moved, ev) => {
        setWire(null);
        if (!moved) return;
        const to = agentAt({ ...draft, agents: draft.agents.filter(a => !projection.hidden.has(a.id)) }, toWorld(ev.clientX, ev.clientY));
        if (!to || to === from) return;
        const reason = edit((d) => connect(d, from, to));
        if (!reason) onSelect({ kind: 'agent', id: from });
      },
    );
  };

  const reconnectDown = (id: string, end: 'from' | 'to') => (e: React.PointerEvent) => {
    if (e.button !== 0 || locked) return;
    e.preventDefault(); e.stopPropagation();
    const c = draft.connections.find(c => c.id === id);
    if (!c) return;
    const from = end === 'to' ? c.from : c.to;
    follow(e, (_dx,_dy,ev) => setWire({from,to:toWorld(ev.clientX,ev.clientY)}), (moved,ev) => {
      setWire(null);
      if (!moved) return;
      const to = agentAt({...draft,agents:draft.agents.filter(a => !projection.hidden.has(a.id))},toWorld(ev.clientX,ev.clientY));
      if (to) edit(d => reconnect(d,id,end,to));
    });
  };

  const workflowReconnect = (id: string, e: React.PointerEvent<HTMLButtonElement>) => {
    if (e.button !== 0 || locked) return;
    e.preventDefault(); e.stopPropagation();
    const link = workflowLinks(draft).find(l => l.id === id);
    if (!link) return;
    follow(e, (_dx,_dy,ev) => setWire({from:link.from,to:toWorld(ev.clientX,ev.clientY)}), (moved, ev) => {
      setWire(null);
      if (!moved) return;
      const at = toWorld(ev.clientX,ev.clientY);
      const box = Object.entries(stageBoxes(draft)).find(([,b]) => at.x >= b.x && at.x <= b.x+b.w && at.y >= b.y && at.y <= b.y+b.h)?.[0];
      const to = box ?? agentAt({...draft, agents:draft.agents.filter(a => !projection.hidden.has(a.id))}, at);
      if (to) edit(d => reconnectWorkflowRoute(d,id,to));
    });
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const t = e.target as HTMLElement;
    if (!t.closest('[data-agent], [data-group], [data-connection]') || !selection) return;
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const step = e.shiftKey ? BIG_STEP : STEP;
    const arrows: Record<string, [number, number]> = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step] };
    const move = arrows[e.key];
    // The card is one of several selected: arrows, Delete and Esc act on them all.
    const group = many && selection.kind === 'agent' && picked.includes(selection.id) ? picked : null;
    if (e.key === ' ' && e.shiftKey && selection.kind === 'agent' && t.closest('[data-agent]')) {
      e.preventDefault();
      togglePick(selection.id);
      return;
    }
    if (move && group) {
      e.preventDefault();
      hold();
      edit((d) => moveAgents(d, group, move[0], move[1], locked), `move:${group.join(',')}`);
      const b = boxOf(selection);
      if (b) reveal({ ...b, x: b.x + move[0], y: b.y + move[1] });
      return;
    }
    if (move) {
      e.preventDefault();
      hold();
      if (selection.kind === 'agent') {
        const a = draft.agents.find((x) => x.id === selection.id);
        if (a) edit((d) => moveAgent(d, a.id, { x: a.x + move[0], y: a.y + move[1] }, locked), `move:${a.id}`);
      } else if (selection.kind === 'group') edit((d) => moveGroup(d, selection.id, move[0], move[1]), `move:${selection.id}`);
      const b = boxOf(selection);
      if (b) reveal({ ...b, x: b.x + move[0], y: b.y + move[1] });
      return;
    }
    if (e.key === 'Delete' || e.key === 'Backspace') {
      e.preventDefault();
      if (locked) {
        edit((d) => ({ draft: d, reason: LOCK_REASON }));
        return;
      }
      if (group) {
        edit((d) => removeAgents(d, group));
        setMulti([]);
        onSelect(null);
        el.current?.focus();
        return;
      }
      if (!edit((d) => removeElement(d, selection))) {
        onSelect(null);
        el.current?.focus();
      }
      return;
    }
    if (e.key === 'Escape') {
      // Leaves the selection; the board keeps keyboard focus.
      e.preventDefault();
      setMulti([]);
      onSelect(null);
      el.current?.focus();
      return;
    }
    if (e.key === 'Enter' && selection.kind === 'agent' && t.closest('[data-agent]')) {
      e.preventDefault();
      p.onOpenAgent(selection.id);
    }
  };

  const onViewportDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest('button, a, .port')) return;
    // Shift+drag on empty board draws a box: the agents it touches join the selection. A plain drag pans.
    if (e.shiftKey) {
      e.preventDefault();
      const a = toWorld(e.clientX, e.clientY);
      follow(
        e,
        (_dx, _dy, ev) => setMarquee({ a, b: toWorld(ev.clientX, ev.clientY) }),
        (moved, ev) => {
          setMarquee(null);
          if (!moved) return;
          const b = toWorld(ev.clientX, ev.clientY);
          const box = { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), w: Math.abs(a.x - b.x), h: Math.abs(a.y - b.y) };
          const hit = draft.agents.filter((x) => !projection.hidden.has(x.id) && overlaps(agentBox(x), box)).map((x) => x.id);
          setMulti((cur) => [...new Set([...cur, ...hit])]);
          if (hit.length > 0 && selection?.kind !== 'agent') onSelect({ kind: 'agent', id: hit[0] });
        },
      );
      return;
    }
    setMulti([]);
    onSelect(null);
    startPan(e);
  };

  // While dragging a card, the group under its centre is where it would land.
  const dropGroup =
    drag?.kind === 'agent'
      ? (() => {
          const a = shown.agents.find((x) => x.id === drag.id);
          return a ? groupAt(draft, { x: a.x + CARD_W / 2, y: a.y + CARD_H / 2 }) : null;
        })()
      : null;
  const wireTarget = wire ? agentAt({ ...draft, agents: draft.agents.filter(a => !projection.hidden.has(a.id)) }, wire.to) : null;
  const wireFrom = wire ? (wire.from === INPUT_WIRE ? teamInputBox(draft) : boxes[wire.from] && (logos ? logoRouteBox(boxes[wire.from]) : boxes[wire.from])) : null;
  const lockedNote = !locked ? null : p.marks ? 'Approve or reject the proposal to change the board.' : 'Stop the team to add components or change arrows.';
  const empty = draft.agents.length === 0;
  const mainKind = mainKindOf(draft);

  return (
    <div className={`sb-canvas${af.quiet ? ' arrows-quiet' : ''}`}>
      <div className={`sb-tools-disclosure ux-tools${moreTools ? ' ux-tools-more' : ''}`}>
      <div className="sb-toolbar" role="toolbar" aria-label="Add to the team" ref={paletteRef}>
        {p.leadTools}
        {(() => {
          const v = kindVendor(mainKind);
          return (
            <Btn
              className={`palette-btn ux-add-agent ${v.cls}`}
              disabled={locked}
              aria-describedby={locked ? `${ids}-lock` : undefined}
              aria-label={`Add a ${PALETTE_WORD[mainKind]} agent`}
              tip={`Drag onto the board, or click to add a ${PALETTE_WORD[mainKind]} agent. More tools has the other kinds, groups and the Input.`}
              kbd="n"
              onPointerDown={paletteDown(mainKind)}
              onClick={() => add(mainKind)}
            >
              <Icon name="plus" size={13} />
              Add agent
              <VendorMark v={v} size="xs" />
            </Btn>
          );
        })()}
        {moreTools && <>
        {PALETTE.filter((k) => k !== mainKind).map((k) => {
          const v = kindVendor(k);
          return (
            <Btn
              key={k}
              className={`palette-btn ${v.cls}`}
              disabled={locked}
              aria-describedby={locked ? `${ids}-lock` : undefined}
              aria-label={`Add a ${PALETTE_WORD[k]} agent`}
              tip={`Drag onto the board, or click to add a ${PALETTE_WORD[k]} agent`}
              onPointerDown={paletteDown(k)}
              onClick={() => add(k)}
            >
              <VendorMark v={v} size="xs" />
              {PALETTE_WORD[k]}
              <Icon name="plus" size={13} className="muted" />
            </Btn>
          );
        })}
        {!p.groupOnly && <Btn onClick={() => newGroup()} icon="group" disabled={locked} aria-describedby={locked ? `${ids}-lock` : undefined} aria-label="Add a group" tip="Add a group">
          Group
          <Icon name="plus" size={13} className="muted" />
        </Btn>}
        {!p.groupOnly && inputAllowed(draft) && !draft.input && <Btn onPointerDown={team.paletteDown} onClick={() => team.add()} icon="list" disabled={locked} aria-describedby={locked ? `${ids}-lock` : undefined} aria-label="Add the Input" tip="Drag onto the board, or click: the goal, links and notes the starting agents get">
          Input
          <Icon name="plus" size={13} className="muted" />
        </Btn>}
        {!p.groupOnly && <GroupPalette draft={draft} locked={locked} onDrag={blockDown} onAdd={b=>addBlock(b)} />}
        {!p.groupOnly && <TeamFolderButton draft={draft} locked={locked} onChoose={folder => edit(d => folderForAll(d, folder))} />}
        <Btn onClick={toggleFullText} aria-pressed={fullTextWanted} icon="list" disabled={logos} aria-describedby={logos ? `${ids}-fulltext` : undefined} tip={logos ? 'Full text shows in the Cards board style' : "Show every card's full text"}>Full text</Btn>
        {logos && <span id={`${ids}-fulltext`} className="sr-only">Full text shows in the Cards board style</span>}
        {draft.connections.length > QUIET_AFTER && <Btn onClick={af.toggleAll} aria-pressed={af.allArrows} icon="route" tip="Draw every arrow at full strength. Otherwise a busy board shows a card's arrows when you point at it or select it.">All arrows</Btn>}
        </>}
        <Btn variant="ghost" className="ux-more-tools" onClick={toggleMoreTools} aria-expanded={moreTools} aria-label={moreTools ? 'Fewer tools' : 'More tools'} icon={moreTools ? 'chevron-down' : 'chevron-right'} tip={moreTools ? 'Show only Add agent' : 'Codex and other kinds, groups, the Input, saved groups, the project folder and Full text'}>
          {moreTools ? 'Fewer' : 'More'}<span className="ux-wide-word"> tools</span>
        </Btn>
        <span className="tb-sep" aria-hidden="true">
          ·
        </span>
        <Btn variant="ghost" icon="undo" iconOnly onClick={p.onUndo} disabled={!p.canUndo} aria-label="Undo" kbd="Ctrl+Z" />
        <Btn variant="ghost" icon="redo" iconOnly onClick={p.onRedo} disabled={!p.canRedo} aria-label="Redo" kbd="Ctrl+Shift+Z" />
        {p.extraTools}
        {lockedNote && (
          <span id={`${ids}-lock`} className="reason tb-reason">
            {lockedNote}
          </span>
        )}
      </div>
      </div>
      <div className="sb-viewport-wrap">
      {many && (
        <SelectionBar
          draft={draft}
          ids={picked}
          edit={edit}
          locked={locked}
          onClear={() => {
            setMulti([]);
            el.current?.focus();
          }}
          onGrouped={(id) => selectNew({ kind: 'group', id })}
        />
      )}
      {/* Solid lines need no key; the legend shows only while a dashed (not enforced) line is drawn. `?` always has it. */}
      {draft.connections.some((c) => c.kind !== 'hands-work-to') && <ConnectionLegend className="sb-legend muted" />}
      <span role="group" aria-label="Zoom" className="zoom-float">
        <Btn variant="ghost" size="sm" icon="minus" iconOnly onClick={() => zoomAt(1 / 1.2)} aria-label="Zoom out" kbd="-" />
        <Btn variant="ghost" size="sm" icon="plus" iconOnly onClick={() => zoomAt(1.2)} aria-label="Zoom in" kbd="+" />
        <Btn variant="ghost" size="sm" icon="fit" iconOnly onClick={fit} aria-label="Fit" tip="Fit to view" kbd="f" />
      </span>
      <div
        className="graph-viewport sb-viewport"
        ref={el}
        tabIndex={-1}
        onPointerDown={onViewportDown}
        onKeyDown={onKeyDown}
        onContextMenu={(e) => { if (!(e.target as HTMLElement).closest('button, a, input, textarea')) menuAt(e, { kind: 'board' }); }}
        onScroll={draft.workflow ? e => { e.currentTarget.scrollTop = 0; e.currentTarget.scrollLeft = 0; } : undefined}
        data-testid="structure-viewport"
        aria-label="Design board"
        role="region"
      >
        {empty && !ghost && (
          <div className="sb-empty" aria-live="polite">
            <p className="sb-empty-title">This {p.groupOnly ? 'group' : 'team'} has no agents yet.</p>
            <p className="muted">
              Drag <span className="kbdish">+ Claude</span> or <span className="kbdish">+ Codex</span> onto the board, right-click it, or{' '}
              {p.groupOnly ? <>use <kbd>Tab</kbd> to reach Add, then <kbd>⏎</kbd>.</> : <>press <kbd>n</kbd> then <kbd>⏎</kbd>.</>}
            </p>
          </div>
        )}
        <div
          className="graph-world"
          data-testid="structure-world"
          style={{ transform: `translate(${vp.x}px, ${vp.y}px) scale(${vp.k})` }}
        >
          {groups.map((original) => {
            const hidden = collapsed.has(original.id);
            const g = hidden ? { ...original, ...closedGroupBox(original) } : original;
            const gi = issuesFor(issues, 'group', g.id);
            const sel = selection?.kind === 'group' && selection.id === g.id;
            return (
              <div
                key={g.id}
                className={`group-box sb-group${hidden ? ' group-collapsed' : ''}${sel ? ' selected' : ''}${dropGroup === g.id ? ' drop-target' : ''}${drag?.kind === 'group' && drag.id === g.id ? ' dragging' : ''}${mark(g.id)}`}
                style={{ left: g.x, top: g.y, width: g.w, height: g.h }}
                onPointerDown={hidden ? groupDown(g.id) : undefined}
                onContextMenu={(e) => menuAt(e, { kind: 'group', id: g.id })}
                {...af.hoverProps(g.id)}
              >
                <button
                  type="button"
                  className="group-title group-head"
                  data-group={g.id}
                  aria-label={`Group: ${g.label || 'Unnamed group'}${gi.length ? `, ${gi.length} to fix` : ''}`}
                  aria-pressed={sel}
                  data-tip={gi.map((i) => i.message).join('\n') || 'Drag to move the group'}
                  onPointerDown={hidden ? undefined : groupDown(g.id)}
                  onFocus={() => {
                    onSelect({ kind: 'group', id: g.id });
                    if (!pointerFocus.current) reveal(g);
                  }}
                  onClick={() => { if (dragged.current) { dragged.current = false; return; } onSelect({ kind: 'group', id: g.id }); p.onInspect?.({kind:'group',id:g.id}); }}
                >
                  <Icon name="group" size={12} />
                  {g.label || 'Unnamed group'}
                  {gi.length > 0 && <span className={`marker ${errorsOf(gi).length ? 'tone-red' : 'tone-amber'}`}> {MARK[errorsOf(gi).length ? 'error' : 'warning']}</span>}
                </button>
                {hidden && <span className="group-hidden-count">{shown.agents.filter(a => groupOf(a) === g.id).length} agents · contents hidden</span>}
                <GroupVisibilityButton name={g.label || 'Unnamed group'} collapsed={hidden} onFocus={() => reveal({x:g.x+g.w-145,y:hidden ? g.y+g.h-36 : g.y-14,w:145,h:28})} onToggle={() => setCollapsed(g.id, !hidden)} />
              </div>
            );
          })}
          <svg className="edges" width={(boundsOf(shown)?.x1 ?? 0) + 400} height={(boundsOf(shown)?.y1 ?? 0) + 400} aria-hidden="true">
            <defs>
              <marker id={`sb-arrow-${ids}`} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                <path d="M0,0 L10,5 L0,10 L2.5,5 z" className="arrow-head" />
              </marker>
              <marker id={`sb-arrow-sel-${ids}`} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                <path d="M0,0 L10,5 L0,10 L2.5,5 z" className="arrow-head sel" />
              </marker>
            </defs>
            {shown.connections.map((c, i) => {
              const g = routes.get(edges[i]);
              if (!g) return null;
              const sel = selection?.kind === 'connection' && selection.id === c.id;
              return (
                <g key={c.id}><path className="edge-hit" d={g.d} onPointerDown={e => e.stopPropagation()} onClick={() => onSelect(g.merged.length > 1 ? bundleGroup(c) : {kind:'connection',id:c.id})} />
                <path
                  d={g.d}
                  className={`edge ${c.kind === 'hands-work-to' ? 'enforced' : 'intent'}${sel ? ' selected' : ''}${mark(c.id)}${lit(c.from, c.to, sel)}`}
                  markerEnd={`url(#${sel ? `sb-arrow-sel-${ids}` : `sb-arrow-${ids}`})`}
                /></g>
              );
            })}
            <InputEdges draft={shown} boxes={logos ? logoRouteBoxes(boxes) : boxes} marker={`sb-arrow-${ids}`} />
            {wire && wireFrom && (
              <path
                className="edge wire"
                d={`M${wireFrom.x + wireFrom.w},${wireFrom.y + wireFrom.h / 2} C${wireFrom.x + wireFrom.w + 60},${wireFrom.y + wireFrom.h / 2} ${wire.to.x - 60},${wire.to.y} ${wire.to.x},${wire.to.y}`}
                markerEnd={`url(#sb-arrow-sel-${ids})`}
              />
            )}
          </svg>
          {!locked && selection?.kind === 'connection' && (() => {
            const index = shown.connections.findIndex(c => c.id === selection.id), c = shown.connections[index];
            const g = routes.get(edges[index]);
            return c && g && g.merged.length === 1 && <ArrowHandles geometry={g} label={connectionSentence(draft,c)} onDrag={(end,e) => reconnectDown(c.id,end)(e)} onEdit={() => p.onInspect?.({kind:'connection',id:c.id})} />;
          })()}
          {shown.workflow && p.onWorkflow && <WorkflowGraph collapsed={collapsed} draft={shown} cardHeight={cardHeight} agentBox={logos ? logoRouteBox : undefined} onOpen={p.onWorkflow} onReveal={reveal} inputControls={inputControls} stageControls={locked ? undefined : (stage: string) => boxControls({stage})} readOnly={locked} onReconnect={locked ? undefined : workflowReconnect} />}
          {shown.connections.map((c) => {
            const g = spots.get(c.id);
            if (!g) return null;
            const sel = selection?.kind === 'connection' && selection.id === c.id;
            const ci = issuesFor(issues, 'connection', c.id);
            const n = routes.get(edges[shown.connections.indexOf(c)])?.merged.length ?? 1;
            if (n > 1) {
              const target = bundleGroup(c);
              return (
                <button key={`l-${c.id}`} type="button" className={`edge-label sb-edge-label enforced${g.wrap ? ' wrap' : ''}${lit(c.from, c.to, false)}`} style={{ left: g.x, top: g.y, maxWidth: g.maxWidth }}
                  data-connection={c.id} aria-label={`${n} arrows from ${endName(c.from)} to ${endName(c.to)}; show the group's contents to see each`}
                  onClick={() => onSelect(target)}>{bundleWords(n)}</button>
              );
            }
            return (
              <button
                key={`l-${c.id}`}
                type="button"
                className={`edge-label sb-edge-label ${c.kind === 'hands-work-to' ? 'enforced' : 'intent'}${g.wrap ? ' wrap' : ''}${sel ? ' selected' : ''}${mark(c.id)}${lit(c.from, c.to, sel)}`}
                style={{ left: g.x, top: g.y, maxWidth: g.maxWidth }}
                data-connection={c.id}
                aria-label={`${connectionSentence(draft, c)}${ci.length ? `, ${ci.length} to fix` : ''}`}
                aria-pressed={sel}
                data-tip={[...ci.map((x) => x.message), ...(c.kind === 'hands-work-to' ? [handoffWords(nameOf(draft, c.from), nameOf(draft, c.to))] : [])].join('\n') || undefined}
                onFocus={() => onSelect({ kind: 'connection', id: c.id })}
                onClick={() => { onSelect({ kind: 'connection', id: c.id }); p.onInspect?.({kind:'connection',id:c.id}); }}
              >
                {CONNECTION_WORDS[c.kind]}
                {c.kind !== 'hands-work-to' ? ` · ${NOT_ENFORCED}` : ''}
                {ci.length > 0 && <span className="marker tone-red"> {MARK.error}</span>}
              </button>
            );
          })}
          {shown.agents.filter(a => !projection.hidden.has(a.id)).map((a) => {
            const ai = issuesFor(issues, 'agent', a.id);
            const errs = errorsOf(ai);
            const sel = selection?.kind === 'agent' && selection.id === a.id;
            const v = kindVendor(a.kind);
            const folderIssue = ai.find((i) => i.code.startsWith('folder') || i.code === 'home-unknown');
            const state = errs.length ? `${plural(errs.length, 'thing')} to fix` : 'ready';
            const dragging = drag?.kind === 'agent' && drag.id === a.id;
            return (
              <div
                key={a.id}
                className={`card-slot sb-slot${dragging ? ' dragging' : ''}${fullText ? ' full-text' : ''}`}
                {...af.hoverProps(a.id)}
                style={{ left: a.x, top: a.y, width: CARD_W, height: fullText ? 'auto' : CARD_H, minHeight: CARD_H }}
                ref={(el) => { if (el) slots.current.set(a.id, el); else slots.current.delete(a.id); }}
              >
                <div className={`sb-card ${logos ? logoClasses(v, false, false) : v.cls}${sel ? ' selected' : ''}${many && picked.includes(a.id) ? ' multi-selected' : ''}${errs.length ? ' has-error' : ''}${wireTarget === a.id && wire?.from !== a.id ? ' wire-target' : ''}${mark(a.id)}`}>
                  <button
                    type="button"
                    className={`sb-main${logos ? ' logo-main' : ''}`}
                    data-agent={a.id}
                    aria-label={`${a.name.trim() || 'Unnamed agent'} — ${PALETTE_WORD[a.kind]}, ${state}`}
                    aria-describedby={ai.length ? `${ids}-p-${a.id}` : undefined}
                    aria-pressed={sel}
                    onPointerDown={cardDown(a.id)}
                    onFocus={() => {
                      onSelect({ kind: 'agent', id: a.id });
                      if (!pointerFocus.current) reveal(agentBox(a));
                    }}
                    onClick={(e) => openCard(a.id, e)}
                    onContextMenu={(e) => menuAt(e, { kind: 'agent', id: a.id })}
                  >
                    {logos ? (
                      <LogoFace
                        v={v}
                        name={a.name.trim() || 'Unnamed agent'}
                        badge={<ChangeBadge marks={p.marks} id={a.id} />}
                        extra={<ComponentChips agent={a} draft={draft} />}
                        status={ai.length > 0 ? (
                          <span className={`state pill logo-status ${errs.length ? 'tone-red' : 'tone-amber'}`} title={ai.map((i) => i.message).join('\n')}>
                            <span className="glyph" aria-hidden="true">{MARK[errs.length ? 'error' : 'warning']}</span>
                            <span className="word">{errs.length ? `${errs.length} to fix` : `${ai.length} to check`}</span>
                          </span>
                        ) : (
                          <span className="logo-kind">{v.name} · {a.kind === 'practice' ? 'free' : cardModel(draft, a)}</span>
                        )}
                      />
                    ) : <>
                    <span className="box-head">
                      <VendorMark v={v} size="md" />
                      <span className="box-name" title={a.name.trim() || undefined}>{a.name.trim() || 'Unnamed agent'}</span>
                      <ChangeBadge marks={p.marks} id={a.id} />
                      {ai.length > 0 && (
                        <span className={`marker ${errs.length ? 'tone-red' : 'tone-amber'}`} title={ai.map((i) => i.message).join('\n')}>
                          {MARK[errs.length ? 'error' : 'warning']}
                          {ai.length > 1 ? ai.length : ''}
                        </span>
                      )}
                    </span>
                    {/* One small muted line: kind and model. The job speaks for itself, so it has no label. */}
                    <span className={`sb-kind ${v.cls}`}>
                      {v.name} · {a.kind === 'practice' ? 'free' : cardModel(draft, a)}
                    </span>
                    <ComponentChips agent={a} draft={draft} />
                    <span className={`sb-job${a.job.trim() ? '' : ' muted'}`}>{a.job.trim() || 'No job yet. Select the agent to write one.'}</span>
                    <span className={`sb-folder${folderIssue ? ' tone-red' : ''}`} title={(a.folder || draft.folder) ?? undefined}>
                      {folderIssue ? <span aria-hidden="true">▲</span> : <Icon name="folder" size={11} />}
                      {folderIssue && a.folder && <span className="folder-why">{FOLDER_ISSUE_WORDS[folderIssue.code] ?? 'folder problem'} ·</span>}
                      <span className={a.folder ? 'mono' : undefined}>{(a.folder || draft.folder) ? shortPath((a.folder || draft.folder)!, home) : a.kind === 'practice' ? '~ (home folder)' : 'needs a folder'}</span>
                      {computers[a.id] && <span className="sb-on"> · on {computers[a.id]}</span>}
                    </span>
                    </>}
                  </button>
                  {/* The arrows show who it hands work to (the agent's panel lists them); only a workflow's actions need a footer. */}
                  {draft.workflow && p.onWorkflow && !logos && (
                    <div className="box-foot" onFocus={() => reveal(agentBox(a))}>
                      <WorkflowAgentActions draft={draft} agent={a.id} onOpen={p.onWorkflow} />
                    </div>
                  )}
                  {!locked && !draft.workflow && (
                    <span className="port out" role="presentation" data-tip="Drag to another agent to connect" onPointerDown={portDown(a.id)} />
                  )}
                  <span className="port in" aria-hidden="true" />
                  {ai.length > 0 && (
                    <span id={`${ids}-p-${a.id}`} className="sr-only">
                      {ai.map((i) => i.message).join('. ')}
                    </span>
                  )}
                </div>
              </div>
            );
          })}
          {shown.input && teamInputBox(shown) && <InputNode draft={shown} box={teamInputBox(shown)!} selected={team.open} locked={locked} controls={team.controls} onPortDown={team.portDown} />}
          {team.ghost && <div className="team-input-slot sb-ghost" style={{ left: team.ghost.x, top: team.ghost.y, width: INPUT_W, height: INPUT_H }} aria-hidden="true"><span>Input</span></div>}
          {marquee && (
            <div
              className="sb-marquee"
              aria-hidden="true"
              style={{ left: Math.min(marquee.a.x, marquee.b.x), top: Math.min(marquee.a.y, marquee.b.y), width: Math.abs(marquee.a.x - marquee.b.x), height: Math.abs(marquee.a.y - marquee.b.y) }}
            />
          )}
          {groupGhost && <div className="group-box sb-group-ghost" style={{left:groupGhost.at.x,top:groupGhost.at.y,width:200,height:132}} aria-hidden="true">{groupGhost.block.name}</div>}
          {ghost && (
            <div className="card-slot sb-ghost" style={{ left: ghost.at.x, top: ghost.at.y, width: CARD_W, height: CARD_H }} aria-hidden="true">
              <span className={kindVendor(ghost.kind).cls}>
                <VendorMark v={kindVendor(ghost.kind)} size="md" /> new {PALETTE_WORD[ghost.kind]} agent
              </span>
            </div>
          )}
        </div>
      </div>
      </div>
      {team.open && draft.input && <InputEditor draft={draft} edit={edit} locked={locked} onClose={() => team.setOpen(false)} />}
      {menu && <BoardMenu open={menu} draft={draft} locked={locked} groupOnly={p.groupOnly} edit={edit} act={boardActions} onClose={() => setMenu(null)} />}
    </div>
  );
}

/** The project folder, set for every agent at once (their own folders are cleared). */
function TeamFolderButton({ draft, locked, onChoose }: { draft: TeamDraft; locked: boolean; onChoose: (folder: string) => void }) {
  const [picking, setPicking] = useState(false);
  const home = useHome();
  const shown = draft.folder ? shortPath(draft.folder, home) : 'none chosen';
  return <>
    <Btn onClick={() => setPicking(true)} icon="folder" disabled={locked} aria-label={`Project folder: ${shown}. Choose a folder for every agent`} tip="The folder every agent works in">
      {draft.folder ? shown : 'Choose folder'}
    </Btn>
    {picking && <FolderPicker initialPath={draft.folder ?? null} onClose={() => setPicking(false)} onChoose={folder => { onChoose(folder); setPicking(false); }} />}
  </>;
}
