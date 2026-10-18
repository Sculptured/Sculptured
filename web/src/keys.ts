/** The page on screen: the board's two modes are `structure` (Design) and `team` (Run). */
export type View = 'home' | 'team' | 'agent' | 'structure' | 'agents' | 'results' | 'settings' | 'workflow' | 'other';

export type KeyHelp = { keys: string; what: string; where: 'anywhere' | 'team' | 'structure' | 'agent' | 'goto' | 'agents' };

/** Headings for the keymap's sections. */
export const WHERE_LABEL: Record<KeyHelp['where'], string> = { anywhere: 'Anywhere', team: 'Board · Run', structure: 'Board · Design', agent: 'Agent', agents: 'Agents list', goto: 'Ctrl K' };

/** The full keymap shown by `?`. Every action also has a click target. */
export const KEYMAP: KeyHelp[] = [
  { keys: 'Ctrl+K', what: 'search or run a command: start, stop, add, switch views and projects', where: 'anywhere' },
  { keys: 'g', what: 'go to an agent', where: 'anywhere' },
  { keys: '/', what: 'search agents (opens Go to)', where: 'anywhere' },
  { keys: '?', what: 'show this keymap', where: 'anywhere' },
  { keys: 'n', what: 'new project', where: 'anywhere' },
  { keys: '↑  ·  ↓  ·  j  ·  k', what: 'move through the agents', where: 'agents' },
  { keys: 'Enter', what: 'open the agent', where: 'agents' },
  { keys: 'Shift+click  ·  Shift+Space', what: 'select agents to message them together', where: 'agents' },
  { keys: 'l', what: 'open the Agents list', where: 'team' },
  { keys: 'f', what: 'fit the graph to the view', where: 'team' },
  { keys: '+  ·  -', what: 'zoom in or out', where: 'team' },
  { keys: 'drag  ·  wheel', what: 'pan and zoom the graph', where: 'team' },
  { keys: 'Shift+click  ·  Shift+Space', what: 'select agents to message them together', where: 'team' },
  { keys: 'Esc', what: 'clear the selection', where: 'team' },
  { keys: 'n  then  Enter', what: 'add an agent at the next free place', where: 'structure' },
  { keys: 'Tab  ·  click', what: 'select an agent, group or connection', where: 'structure' },
  { keys: 'Shift+click  ·  Shift+Space', what: 'add an agent to the selection, or take it out', where: 'structure' },
  { keys: 'Shift+drag', what: 'select the agents inside a box (a plain drag pans)', where: 'structure' },
  { keys: 'Ctrl+A', what: 'select every agent', where: 'structure' },
  { keys: 'drag  ·  ←↑→↓  ·  Del', what: 'with several selected: move or delete them together', where: 'structure' },
  { keys: 'Esc', what: 'clear the selection', where: 'structure' },
  { keys: '←  ↑  →  ↓', what: 'move the selection (Shift: bigger steps)', where: 'structure' },
  { keys: 'click  ·  Enter', what: "open the agent: its conversation, setup and activity", where: 'structure' },
  { keys: 'Del  ·  Backspace', what: 'delete the selection', where: 'structure' },
  { keys: 'Ctrl+C  ·  Ctrl+V', what: 'copy · paste the selected agent or group (Command on Mac)', where: 'structure' },
  { keys: 'Ctrl+D', what: 'duplicate the selected agent or group', where: 'structure' },
  { keys: 'right-click', what: 'actions for the board, a group or an agent', where: 'structure' },
  { keys: 'Ctrl+Z  ·  Ctrl+Shift+Z', what: 'undo · redo', where: 'structure' },
  { keys: 'drag ●', what: 'connect: hands work to', where: 'structure' },
  { keys: 'f  ·  +  ·  -', what: 'fit · zoom', where: 'structure' },
  { keys: 'Esc', what: 'back to the board, keeping view and selection', where: 'agent' },
  { keys: '/', what: 'search the transcript (Enter: next match, Shift+Enter: previous)', where: 'agent' },
  { keys: 'Esc', what: 'after opening a transcript item from Visual progress: back to the picture', where: 'agent' },
  { keys: 'i', what: 'interrupt the current turn', where: 'agent' },
  { keys: '[  ·  ]', what: 'previous · next agent of the team', where: 'agent' },
  { keys: 'Enter', what: 'send the message', where: 'agent' },
  { keys: 'Shift+Enter', what: 'new line in the message', where: 'agent' },
  { keys: 'Ctrl+C / Ctrl+V', what: 'copy / paste text (Command on Mac)', where: 'agent' },
  { keys: '↑  ·  ↓', what: 'move through the list', where: 'goto' },
  { keys: 'Alt+b  ·  Alt+w  ·  Alt+i  ·  Alt+d', what: 'filter: needs you or failed · working · idle · turn finished (✓)', where: 'goto' },
  { keys: '>  ·  @', what: 'commands only · agents only (Backspace in the empty box: everything)', where: 'goto' },
  { keys: 'Enter', what: 'run the command, or jump to the agent', where: 'goto' },
  { keys: 'Esc', what: 'close an overlay', where: 'anywhere' },
];
