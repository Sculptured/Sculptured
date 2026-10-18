import { Icon } from './ui';
import { createContext, useContext, useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';
import './chat-text-mode.css';

type ChatMode = { active: boolean; open: () => void; close: () => void };
const ChatModeContext = createContext<ChatMode | null>(null);
export const useChatTextMode = () => useContext(ChatModeContext);

/** Keep the same transcript and composer mounted, including draft, search and scroll state. */
export function ChatTextMode({ children, className, name, onOpen }: { children: ReactNode; className: string; name: string; onOpen: () => void }) {
  const [active, setActive] = useState(false);
  const pane = useRef<HTMLElement>(null);
  const origin = useRef<HTMLElement | null>(null);
  useEffect(() => active && pane.current ? isolateChat(pane.current) : undefined, [active]);
  const open = () => { origin.current = document.activeElement as HTMLElement; onOpen(); setActive(true); };
  const close = () => { setActive(false); requestAnimationFrame(() => origin.current?.focus()); };
  const onKeyDown = (e: KeyboardEvent<HTMLElement>) => {
    if (!active) return;
    e.stopPropagation();
    if (e.defaultPrevented) return;
    if (e.key === 'Escape') { e.preventDefault(); close(); }
    if (e.key === 'Tab') trapFocus(e, pane.current!);
  };
  return <ChatModeContext value={{ active, open, close }}>
    <section ref={pane} className={`${className}${active ? ' chat-text-mode' : ''}`} role={active ? 'dialog' : undefined}
      aria-modal={active || undefined} aria-label={active ? `Full screen · ${name}` : `Agent ${name}`} onKeyDown={onKeyDown}>
      {children}
    </section>
  </ChatModeContext>;
}

/** Disable background controls for keyboard and assistive technology without moving DOM nodes. */
function isolateChat(pane: HTMLElement) {
  const siblings: Array<[HTMLElement, boolean]> = [];
  for (let node = pane; node.parentElement; node = node.parentElement) {
    for (const sibling of node.parentElement.children) {
      if (sibling !== node && sibling instanceof HTMLElement) {
        siblings.push([sibling, sibling.inert]); sibling.inert = true;
      }
    }
  }
  const overflow = document.body.style.overflow;
  document.body.style.overflow = 'hidden';
  pane.querySelector<HTMLButtonElement>('.chat-mode-toggle')?.focus();
  return () => { for (const [node, inert] of siblings) node.inert = inert; document.body.style.overflow = overflow; };
}

function trapFocus(e: KeyboardEvent<HTMLElement>, pane: HTMLElement) {
  const controls = [...pane.querySelectorAll<HTMLElement>('button, a[href], input, textarea, select, summary, [tabindex]')]
    .filter(el => el.tabIndex >= 0 && !el.matches(':disabled') && el.getClientRects().length > 0);
  const first = controls[0], last = controls.at(-1);
  if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last?.focus(); }
  else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first?.focus(); }
}

export function ChatTextModeButton() {
  const mode = useChatTextMode();
  if (!mode) return null;
  return <button type="button" className="btn chat-mode-toggle" aria-label={mode.active ? 'Exit full screen' : 'Full screen'} aria-pressed={mode.active} onClick={mode.active ? mode.close : mode.open}>
    <Icon name="fit" size={15} className="chat-mode-icon" /><span className="chat-mode-label">{mode.active ? 'Exit full screen' : 'Full screen'}</span>
  </button>;
}
