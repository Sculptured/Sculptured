/** Fixtures for the workspace shell tests: a small registry, two default layouts, a fake BroadcastChannel. */
import type { PageLayouts } from './model';
import { createRegistry, type PaneProps } from './registry';
import { useSelection } from './selection';

function Plain({ instance }: PaneProps) {
  return <div data-testid={`body-${instance}`}>Body of {instance}</div>;
}

function Picker({ instance }: PaneProps) {
  const { selection, select } = useSelection();
  return (
    <div data-testid={`body-${instance}`}>
      <output data-testid={`sel-${instance}`}>{selection.agent ?? 'none'}</output>
      <button type="button" onClick={() => select({ agent: 'A' })}>Select A</button>
      <button type="button" onClick={() => select({ agent: 'B' })}>Select B</button>
    </div>
  );
}

export const registry = createRegistry([
  { type: 'board', title: 'Overview', glyph: '◫', Body: Plain },
  { type: 'agent', title: 'Agent detail', glyph: '◉', multi: true, selection: true, Body: Picker },
  { type: 'feed', title: 'Status feed', glyph: '≡', Body: Plain },
]);

export const defaults = (): PageLayouts => ({
  version: 1,
  active: 'main',
  layouts: [
    {
      id: 'main',
      name: 'Main',
      root: {
        kind: 'split', id: 'r', dir: 'row', sizes: [0.5, 0.5],
        kids: [
          { kind: 'stack', id: 's1', panes: [{ id: 'b1', type: 'board' }], active: 0 },
          { kind: 'stack', id: 's2', panes: [{ id: 'a1', type: 'agent' }], active: 0 },
        ],
      },
      popped: [],
    },
  ],
});

/** Delivers synchronously to every other open channel of the same name, as a structured clone. */
export class FakeChannel {
  static open = new Set<FakeChannel>();
  onmessage: ((e: MessageEvent) => void) | null = null;
  constructor(public name: string) { FakeChannel.open.add(this); }
  postMessage(data: unknown) {
    for (const c of [...FakeChannel.open]) if (c !== this && c.name === this.name) c.onmessage?.({ data: structuredClone(data) } as MessageEvent);
  }
  close() { FakeChannel.open.delete(this); }
}

/** jsdom has no matchMedia: answer `matches` for every query. */
export function fakeMatchMedia(matches: boolean) {
  return (query: string) => ({ matches, media: query, addEventListener: () => {}, removeEventListener: () => {} }) as unknown as MediaQueryList;
}
