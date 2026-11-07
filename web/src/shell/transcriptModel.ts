import { matches } from './highlight';
import { toolSearchText, toolStatus } from './ToolCard';
import type { AgentItem, Item, NoticeItem, OperatorItem, ToolItem, TurnEndItem } from '../conversation';

export type Part = { kind: 'text'; item: AgentItem } | { kind: 'tools'; key: string; tools: ToolItem[] } | { kind: 'unrecognized'; key: string; raw: unknown };

export type AgentBlock = { kind: 'agent'; key: string; parts: Part[] };

export type Block =
  | { kind: 'message'; key: string; item: OperatorItem }
  | AgentBlock
  | { kind: 'divider'; key: string; items: NoticeItem[] }
  | { kind: 'turn-end'; key: string; item: TurnEndItem };

/** Builds the blocks one item at a time; the open agent reply takes the text and tool calls that follow it. */
class Blocks {
  out: Block[] = [];
  private agent: AgentBlock | null = null;

  private reply(key: string) {
    if (!this.agent) {
      this.agent = { kind: 'agent', key: `r-${key}`, parts: [] };
      this.out.push(this.agent);
    }
    return this.agent;
  }

  private close(block: Block) {
    this.agent = null;
    this.out.push(block);
  }

  add(it: Item) {
    switch (it.kind) {
      case 'agent': this.reply(it.key).parts.push({ kind: 'text', item: it }); break;
      case 'tool': this.addTool(it); break;
      case 'unrecognized': this.reply(it.key).parts.push({ kind: 'unrecognized', key: it.key, raw: it.raw }); break;
      case 'operator': this.close({ kind: 'message', key: it.key, item: it }); break;
      case 'notice': this.addNotice(it); break;
      case 'turn-end': this.close({ kind: 'turn-end', key: it.key, item: it }); break;
    }
  }

  private addTool(it: ToolItem) {
    const r = this.reply(it.key);
    const last = r.parts[r.parts.length - 1];
    if (last?.kind === 'tools') last.tools.push(it);
    else r.parts.push({ kind: 'tools', key: `g-${it.key}`, tools: [it] });
  }

  /** Consecutive quiet session events share one divider; an alarming one stands alone. */
  private addNotice(it: NoticeItem) {
    const prev = this.out[this.out.length - 1];
    if (prev?.kind === 'divider' && !it.alarm && !prev.items[prev.items.length - 1].alarm) {
      this.agent = null;
      prev.items.push(it);
    } else this.close({ kind: 'divider', key: it.key, items: [it] });
  }
}

/**
 * Items as chat blocks: a message, an agent reply (its text and tool calls, consecutive calls
 * folded into one group), or a quiet divider for session and turn events. Consecutive quiet
 * session events share one divider; an alarming one stands alone.
 */
export function buildBlocks(items: Item[]): Block[] {
  const blocks = new Blocks();
  for (const it of items) blocks.add(it);
  return blocks.out;
}

/** Every item key inside a block, for jumping to an item. */
export function blockKeys(b: Block): string[] {
  if (b.kind === 'divider') return b.items.map((n) => n.key);
  if (b.kind !== 'agent') return [b.key];
  return b.parts.flatMap(partKeys);
}

const partKeys = (p: Part): string[] => (p.kind === 'text' ? [p.item.key] : p.kind === 'tools' ? p.tools.map((t) => t.key) : [p.key]);

const partMatches = (p: Part, q: string) =>
  p.kind === 'text' ? matches(q, p.item.text) : p.kind === 'tools' ? p.tools.some((t) => matches(q, toolSearchText(t))) : matches(q, JSON.stringify(p.raw));

export function blockMatches(b: Block, q: string): boolean {
  if (!q) return false;
  switch (b.kind) {
    case 'message': return matches(q, b.item.text, b.item.reason);
    case 'divider': return b.items.some((n) => matches(q, n.text));
    case 'turn-end': return false;
    case 'agent': return b.parts.some((p) => partMatches(p, q));
  }
}

/** The key of the newest agent reply, or null. */
export function latestReplyKey(items: Item[]): string | null {
  for (let i = items.length - 1; i >= 0; i--) if (items[i].kind === 'agent') return items[i].key;
  return null;
}

/** Where a focused item lives: its block, and the tool group around it. */
export function focusLocation(blocks: Block[], focusKey: string | null | undefined): { index: number; group: string | null } | null {
  if (!focusKey) return null;
  for (let i = 0; i < blocks.length; i++) {
    const b = blocks[i];
    if (!blockKeys(b).includes(focusKey)) continue;
    const group = b.kind === 'agent' ? b.parts.find((p) => p.kind === 'tools' && p.tools.length > 1 && p.tools.some((t) => t.key === focusKey)) : undefined;
    return { index: i, group: group?.kind === 'tools' ? group.key : null };
  }
  return null;
}

/** The last tool call, and whether it is still running in the open turn: the agent is "working", else "thinking". */
export function isWorking(items: Item[], liveTurn: string | null): boolean {
  let lastTool: ToolItem | undefined;
  for (let i = items.length - 1; i >= 0 && !lastTool; i--) if (items[i].kind === 'tool') lastTool = items[i] as ToolItem;
  return !!liveTurn && !!lastTool && lastTool.turnId === liveTurn && toolStatus(lastTool) === 'running';
}
