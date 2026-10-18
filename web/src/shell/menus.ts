import { useEffect } from 'react';

/** The small `<details>` menus of the header: More, Status, the Design status and a project's •••. */
export const MENU_SELECTOR = 'details.more-menu, details.team-status, details.structure-status, details.project-menu';

const openMenus = (root: ParentNode = document) => [...root.querySelectorAll<HTMLDetailsElement>(MENU_SELECTOR)].filter((d) => d.open);

function close(d: HTMLDetailsElement) {
  d.open = false;
  d.removeAttribute('open');
}

function onToggle(e: Event) {
  const d = e.target;
  if (!(d instanceof HTMLDetailsElement) || !d.open || !d.matches(MENU_SELECTOR)) return;
  // One menu at a time: opening one closes the others (never one that holds it).
  for (const other of openMenus()) if (other !== d && !other.contains(d)) close(other);
}

function onPointerDown(e: PointerEvent) {
  const t = e.target as Node | null;
  for (const d of openMenus()) if (!t || !d.contains(t)) close(d);
}

function onKeyDown(e: KeyboardEvent) {
  if (e.key !== 'Escape') return;
  const open = openMenus();
  if (open.length === 0) return;
  e.preventDefault();
  e.stopPropagation();
  const focusBack = open.find((d) => d.contains(document.activeElement)) ?? open[0];
  for (const d of open) close(d);
  focusBack.querySelector<HTMLElement>(':scope > summary')?.focus();
}

let users = 0;

/**
 * Makes the header's `<details>` menus behave like menus: one open at a time, and Esc or a
 * click outside closes them (Esc returns focus to the menu's button). Installed once however
 * many views use it.
 */
export function useDetailsMenus() {
  useEffect(() => {
    if (users++ === 0) {
      document.addEventListener('toggle', onToggle, true);
      document.addEventListener('pointerdown', onPointerDown, true);
      document.addEventListener('keydown', onKeyDown, true);
    }
    return () => {
      if (--users === 0) {
        document.removeEventListener('toggle', onToggle, true);
        document.removeEventListener('pointerdown', onPointerDown, true);
        document.removeEventListener('keydown', onKeyDown, true);
      }
    };
  }, []);
}
