import { useEffect, useId, useLayoutEffect, useRef, useState, type RefObject } from 'react';
import './setup.css';

/** One tip, pinned to the first of `anchors` on the page. */
export type TourTip = { id: string; title: string; text: string; anchors: string[]; where: string };

export const TOUR_TIPS: TourTip[] = [
  { id: 'agent', title: 'Each card is an agent', text: 'An agent is one Claude Code, Codex or Practice worker with a job. Click its card, or press Enter on it, to see and change what it does.', anchors: ['[data-agent]'], where: 'Agent cards are on a project’s Design board.' },
  { id: 'where', title: 'Where it works', text: 'Each agent works in a folder on one of your computers. Open an agent to choose its Computer and Folder.', anchors: ['[data-tour="agent-folder"]', '[data-agent]'], where: 'Computer and Folder are in an agent’s setup.' },
  { id: 'start', title: 'Start the team', text: 'Start runs every agent. This practice team makes no model calls, so it costs nothing.', anchors: ['[data-tour="start"]'], where: 'Start is at the top of a project.' },
  { id: 'run', title: 'Watch it on the Run board', text: 'Once the team has started, switch to Run to see what each agent is doing, live, and to message it.', anchors: ['[data-tour="board-mode"]'], where: 'Design and Run switch the board, at the top of a project.' },
  { id: 'fix', title: 'Questions and Things to fix', text: 'Agents ask you things under Questions. Things to fix lists what keeps a team from starting; each item opens what it is about.', anchors: ['[data-tour="fix"]', '[data-tour="questions"]'], where: 'Questions is in a project’s tabs.' },
];

const GAP = 12;
const NARROW = 600;

function findAnchor(tip: TourTip): Element | null {
  for (const s of tip.anchors) { const el = document.querySelector(s); if (el) return el; }
  return null;
}

type Place = { top: number; left: number } | null;

/** Where the tip goes: below its element, else above it, kept on screen. Docked at the bottom on a phone or with no element. */
function placeFor(el: Element | null, box: HTMLElement | null): Place {
  if (!el || window.innerWidth < NARROW) return null;
  const r = el.getBoundingClientRect();
  const w = box?.offsetWidth || 340;
  const h = box?.offsetHeight || 180;
  const below = r.bottom + GAP + h <= window.innerHeight;
  const top = below ? r.bottom + GAP : Math.max(GAP, r.top - GAP - h);
  const left = Math.min(Math.max(16, r.left), Math.max(16, window.innerWidth - w - 16));
  return { top, left };
}

/**
 * A short guided tour of the real screens: about five tips, each pinned to an element that is on the
 * page (outlined while its tip shows). Next, Back and Skip; Esc ends it. `onEnd(true)` when finished.
 */
export function Tour({ onEnd }: { onEnd: (finished: boolean) => void }) {
  const ids = useId();
  const [i, setI] = useState(0);
  const tip = TOUR_TIPS[i];
  const last = i === TOUR_TIPS.length - 1;
  const box = useRef<HTMLDivElement>(null);
  const next = useRef<HTMLButtonElement>(null);
  const end = useRef(onEnd);
  end.current = onEnd;
  const { anchor, place } = useAnchor(tip, box);
  useEffect(() => { next.current?.focus(); }, [i]);
  useTourKeys(end);
  return (
    <div ref={box} className={`tour-tip panel${place ? '' : ' docked'}`} role="dialog" aria-modal="false" aria-labelledby={`${ids}-t`} aria-describedby={`${ids}-d`}
      style={place ? { top: place.top, left: place.left } : undefined}>
      <p className="tour-count muted">Tip {i + 1} of {TOUR_TIPS.length}</p>
      <h2 id={`${ids}-t`} className="tour-title">{tip.title}</h2>
      <p id={`${ids}-d`}>{tip.text}</p>
      {!anchor && <p className="muted">That part is not on this page right now. {tip.where}</p>}
      <div className="tour-actions">
        <button type="button" className="btn btn-ghost" onClick={() => end.current(false)}>Skip tour</button>
        <span className="tb-fill" />
        <button type="button" className="btn" disabled={i === 0} onClick={() => setI(i - 1)}>Back</button>
        <button ref={next} type="button" className="btn btn-primary" onClick={() => (last ? end.current(true) : setI(i + 1))}>{last ? 'Finish' : 'Next'}</button>
      </div>
    </div>
  );
}

/** The tip's element (outlined while its tip shows) and where the tip sits. The page may still be
 * loading (a board after navigation), so it looks again until the element appears. */
function useAnchor(tip: TourTip, box: RefObject<HTMLDivElement | null>) {
  const [anchor, setAnchor] = useState<Element | null>(() => findAnchor(tip));
  const [place, setPlace] = useState<Place>(null);
  useEffect(() => {
    const find = () => {
      const el = findAnchor(tip);
      setAnchor((old) => (old === el ? old : el));
      const p = placeFor(el, box.current);
      setPlace((old) => (old?.top === p?.top && old?.left === p?.left ? old : p));
    };
    find();
    const t = setInterval(find, 400);
    window.addEventListener('resize', find);
    window.addEventListener('scroll', find, true);
    return () => { clearInterval(t); window.removeEventListener('resize', find); window.removeEventListener('scroll', find, true); };
  }, [tip, box]);
  useLayoutEffect(() => {
    if (!anchor) return;
    anchor.setAttribute('data-tour-on', '');
    if (typeof (anchor as HTMLElement).scrollIntoView === 'function') (anchor as HTMLElement).scrollIntoView({ block: 'nearest', inline: 'nearest' });
    return () => anchor.removeAttribute('data-tour-on');
  }, [anchor]);
  return { anchor, place };
}

/** Esc ends the tour; focus goes back where it was when the tour ends. */
function useTourKeys(end: RefObject<(finished: boolean) => void>) {
  useEffect(() => {
    const opener = document.activeElement;
    const key = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); end.current(false); } };
    window.addEventListener('keydown', key, true);
    return () => {
      window.removeEventListener('keydown', key, true);
      if (opener instanceof HTMLElement && opener !== document.body && document.contains(opener)) opener.focus();
    };
  }, [end]);
}
