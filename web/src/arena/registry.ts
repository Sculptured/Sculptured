import { createRegistry } from '../workspace/registry';
import { ComparePane } from './panes/Compare';
import { EnginePane } from './panes/Engine';
import { ForkTreePane } from './panes/ForkTree';
import { RunsPane } from './panes/Runs';
import { StoragePane } from './panes/Storage';
import { WorldPane } from './panes/World';

/** Arena control's pane types. Made once, here: the workspace keeps the wrapped types by identity. */
export const arenaRegistry = createRegistry([
  { type: 'arena.runs', title: 'Runs', glyph: '▶', Body: RunsPane },
  { type: 'arena.forks', title: 'Fork tree', glyph: '⑂', Body: ForkTreePane },
  { type: 'arena.world', title: 'World', glyph: '◎', Body: WorldPane },
  { type: 'arena.storage', title: 'Saves and disk', glyph: '▤', Body: StoragePane },
  { type: 'arena.engine', title: 'Engine', glyph: '⚙', Body: EnginePane },
  { type: 'arena.compare', title: 'Compare', glyph: '⇄', multi: true, defaultSettings: {}, Body: ComparePane },
]);
