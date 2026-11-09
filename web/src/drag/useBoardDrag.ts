import { useEffect, useMemo, useRef, useState } from 'react';
import type { AgentKind } from '../generated/AgentKind';
import type { TeamDraft } from '../generated/TeamDraft';
import type { Edit } from '../structure/Panels';
import { moveAgents } from '../draft/bulk';
import { moveGroup } from '../draft/edits';
import { drawnGroups } from '../draft/geometry';
import { groupOf } from '../draft/ids';
import { CARD_W } from '../layout';
import { adding, agentDrag, drawnAt, groupDrag, moving, selectionDrag, type Place } from './board';
import { dist, type Behavior, type Frame, type Point } from './spec';
import { useTween } from './useTween';

type Follow = (e: React.PointerEvent, move: (dx: number, dy: number, ev: PointerEvent) => void, end: (moved: boolean, ev: PointerEvent) => void) => () => void;
/** The drag on screen: what is dragged (`fresh`: a new agent), whether it floats under the pointer, and the frame previewed. */
type Live = { id: string; float: boolean; group: boolean; fresh?: boolean; frame: Frame<TeamDraft> | null };

type Options = {
  draft: TeamDraft;
  edit: Edit;
  locked: boolean;
  follow: Follow;
  /** The board's zoom. */
  zoom: () => number;
  /** Board coordinates of a pointer over the board, or null when it is elsewhere. */
  boardPoint: (ev: { clientX: number; clientY: number }) => Point | null;
  /** Agents not drawn (inside a collapsed group): no slots beside them. */
  hidden: () => ReadonlySet<string>;
  /** A drag from the palette added this agent. */
  onAdded: (id: string) => void;
  /** A card or group drag ended (`moved`: it was a drag, not a click). */
  onEnd: (moved: boolean) => void;
};

/** How long the drop keeps animating once the pointer is released. */
const SETTLE_MS = 450;

type Point2Board = (ev: PointerEvent, dx: number, dy: number) => Point | null;
type Commit = (f: Frame<TeamDraft>) => void;

/**
 * Runs one drag: declared on its first move (a click enumerates nothing), every frame shown with
 * `show`, and on release the frame under the pointer committed.
 */
function runner(o: Options, show: (l: Live | null) => void, dropped: () => void) {
  const run = (e: React.PointerEvent, what: Omit<Live, 'frame'>, declare: () => Behavior<TeamDraft>, point: Point2Board, commit: Commit) => {
    let declared: Behavior<TeamDraft> | null = null;
    const behave = (p: Point) => (declared ??= declare())(p);
    const known = new Set(o.draft.agents.map((a) => a.id));
    // A new agent's id is made with each state it could be added in: read it from the frame.
    const idIn = (f: Frame<TeamDraft> | null) => (what.fresh ? f?.state.agents.find((a) => !known.has(a.id))?.id ?? '' : what.id);
    // Esc puts the element back: nothing changes.
    const esc = (k: KeyboardEvent) => {
      if (k.key !== 'Escape') return;
      k.preventDefault();
      cancel();
      show(null);
      if (!what.fresh) o.onEnd(false);
    };
    window.addEventListener('keydown', esc, true);
    const cancel = () => { window.removeEventListener('keydown', esc, true); stop(); };
    const stop = o.follow(
      e,
      (dx, dy, ev) => {
        const p = point(ev, dx, dy);
        const frame = p && behave(p);
        show(p ? { ...what, id: idIn(frame), frame } : null);
      },
      (moved, ev) => {
        window.removeEventListener('keydown', esc, true);
        show(null);
        if (!what.fresh) o.onEnd(moved);
        const p = moved ? point(ev, ev.clientX - e.clientX, ev.clientY - e.clientY) : null;
        const f = p && behave(p);
        if (!f) return;
        dropped();
        commit(f);
      },
    );
  };
  const from = (start: Point): Point2Board => (_ev, dx, dy) => ({ x: start.x + dx / o.zoom(), y: start.y + dy / o.zoom() });
  /** Saves the frame's state, or redoes the drop on a draft that changed during the drag. */
  const keep = (base: TeamDraft, f: Frame<TeamDraft>, redo: (d: TeamDraft) => ReturnType<Parameters<Edit>[0]>) =>
    o.edit((d) => (d === base ? f.state : redo(d)));
  return { run, from, keep };
}

/** Starts the board's drags. */
function starters(o: Options, show: (l: Live | null) => void, dropped: () => void) {
  const { run, from, keep } = runner(o, show, dropped);

  /** A card, or (`together`) the selection it belongs to. */
  const agent = (e: React.PointerEvent, id: string, together: string[] | null) => {
    const base = o.draft;
    const a = base.agents.find((x) => x.id === id);
    if (!a) return;
    if (together) {
      run(e, { id, float: false, group: false }, () => selectionDrag(base, together, id, o.locked), from(a), (f) =>
        keep(base, f, (d) => moveAgents(d, together, f.at.x - a.x, f.at.y - a.y, o.locked)));
      return;
    }
    const place: Place = moving(id, o.locked);
    run(e, { id, float: true, group: false }, () => agentDrag(base, id, place, o.hidden()), from(a), (f) => keep(base, f, (d) => place(d, f.at)));
  };

  const group = (e: React.PointerEvent, id: string) => {
    const base = o.draft;
    const g = base.groups.find((x) => x.id === id);
    if (!g) return;
    run(e, { id, float: false, group: true }, () => groupDrag(base, id), from(g), (f) => keep(base, f, (d) => moveGroup(d, id, f.at.x - g.x, f.at.y - g.y)));
  };

  /** A new agent from the palette: grabbed at its top centre, it exists only in the previewed states until the drop. */
  const palette = (e: React.PointerEvent, kind: AgentKind) => {
    const base = o.draft;
    const place = adding(kind);
    const grab: Point2Board = (ev) => {
      const w = o.boardPoint(ev);
      return w && { x: w.x - CARD_W / 2, y: w.y - 24 };
    };
    run(e, { id: '', float: true, group: false, fresh: true }, () => agentDrag(base, null, place, o.hidden()), grab, (f) => {
      let id = f.state.agents.find((a) => !base.agents.includes(a))?.id ?? '';
      const reason = o.edit((d) => {
        if (d === base) return f.state;
        const r = place(d, f.at);
        id = r.id;
        return r;
      });
      if (!reason) o.onAdded(id);
    });
  };
  return { agent, group, palette };
}

/** What the canvas marks during a drag: the dragged element, where a floating card lands, the group it joins. */
function marks(live: Live | null) {
  const f = live?.frame;
  if (!live || !f) return { dragging: live && { id: live.id, group: live.group }, landing: null, dropGroup: null };
  const landed = live.group ? undefined : f.state.agents.find((x) => x.id === live.id);
  const joins = landed ? groupOf(landed) : null;
  return {
    dragging: { id: live.id, group: live.group },
    landing: live.float && dist(f.drawAt, f.at) > 1 ? f.at : null,
    dropGroup: joins && drawnGroups(f.state).some((g) => g.id === joins) ? joins : null,
  };
}

/**
 * The board's pointer drags, declared (see `board.ts`): a card, several selected cards, a group,
 * and a new agent from the palette. While dragging, the board draws the previewed state; release
 * keeps exactly that state, so what was shown is what is saved. `shown` is the board to draw.
 */
export function useBoardDrag(o: Options) {
  const [live, setLive] = useState<Live | null>(null);
  const [settling, setSettling] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  const dropped = () => {
    clearTimeout(timer.current);
    setSettling(true);
    timer.current = setTimeout(() => setSettling(false), SETTLE_MS);
  };
  const target = useMemo(() => {
    const f = live?.frame;
    if (!live || !f) return o.draft;
    return live.float ? drawnAt(f.state, live.id, f.drawAt) : f.state;
  }, [o.draft, live]);
  const shown = useTween(target, !!live || settling, live?.float ? live.id : null);
  return { shown, ...marks(live), ...starters(o, setLive, dropped) };
}
