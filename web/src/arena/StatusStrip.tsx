import type { ReactNode } from 'react';
import { isArenaDown, type ArenaCtx } from './data';
import { agoWords, sizeWords } from './words';

type Tone = 'up' | 'warn' | 'down' | 'plain';
type TileProps = { label: string; glyph: string; tone: Tone; children: ReactNode };

/** One reading: a small caps label, then glyph and value. The glyph and the words carry it; colour only repeats them. */
function Tile({ label, glyph, tone, children }: TileProps) {
  return (
    <li className={`arena-tile arena-tile-${tone}`}>
      <span className="arena-tile-label">{label}</span>
      <span className="arena-tile-value"><span aria-hidden="true">{glyph}</span> {children}</span>
    </li>
  );
}

function ArenaTile({ ctx }: { ctx: ArenaCtx }) {
  if (ctx.health === undefined) return <Tile label="Arena" glyph="◌" tone="plain">Asking</Tile>;
  if (ctx.health) return <Tile label="Arena" glyph="●" tone="up">Up</Tile>;
  return <Tile label="Arena" glyph="■" tone="down">{isArenaDown(ctx.healthError) ? 'Down' : 'Not answering'}</Tile>;
}

function PartsTile({ ctx }: { ctx: ArenaCtx }) {
  const parts = ctx.health ? ctx.world.value?.parts : undefined;
  if (!parts) return <Tile label="Parts" glyph="?" tone="plain">Unknown</Tile>;
  const ok = parts.filter((p) => p.answered).length;
  return <Tile label="Parts" glyph={ok === parts.length ? '✓' : '⚠'} tone={ok === parts.length ? 'plain' : 'warn'}>{ok}/{parts.length} answering</Tile>;
}

function SavesTile({ ctx }: { ctx: ArenaCtx }) {
  const u = ctx.usage.value;
  const d = u ? { bytes: u.total_bytes, limit: u.limit_bytes } : ctx.health?.disk ? { bytes: ctx.health.disk.bytes, limit: ctx.health.disk.limit_bytes } : null;
  if (!ctx.health || !d || d.limit <= 0) return <Tile label="Saves" glyph="?" tone="plain">Unknown</Tile>;
  const share = Math.round((100 * d.bytes) / d.limit);
  return <Tile label="Saves" glyph={share >= 90 ? '⚠' : '▤'} tone={share >= 90 ? 'warn' : 'plain'}>{share}% of {sizeWords(d.limit)}</Tile>;
}

function RunningTile({ ctx }: { ctx: ArenaCtx }) {
  const runs = ctx.health ? ctx.runs.value : null;
  if (!runs) return <Tile label="Running" glyph="?" tone="plain">Unknown</Tile>;
  const n = runs.reduce((sum, r) => sum + r.branches.filter((b) => b.status === 'running').length, 0);
  return <Tile label="Running" glyph={n ? '◐' : '○'} tone="plain">{n} {n === 1 ? 'branch' : 'branches'}</Tile>;
}

/** Arena at a glance: up or down, parts answering, saves against the limit, branches running, the last cleanup from here. */
export function StatusStrip({ ctx }: { ctx: ArenaCtx }) {
  const c = ctx.lastCleanup;
  return (
    <ul className="arena-strip" aria-label="Arena status">
      <ArenaTile ctx={ctx} />
      <PartsTile ctx={ctx} />
      <SavesTile ctx={ctx} />
      <RunningTile ctx={ctx} />
      {c && <Tile label="Cleaned up from here" glyph="↺" tone="plain">{agoWords(c.at, Date.now())} · freed {sizeWords(c.freed_bytes)}</Tile>}
    </ul>
  );
}
