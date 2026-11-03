// Every Arena address, in one place. A branch's page is its simulated Control workspace.
const e = encodeURIComponent;

export const ARENA_PATH = '/arena';
export const runPath = (run: string, branch: string, step?: number) =>
  `${ARENA_PATH}/runs/${e(run)}/${e(branch)}${step != null ? `?step=${step}` : ''}`;
/** Where a popped-out pane of a branch's simulated Control workspace opens. */
export const runPanePath = (run: string, branch: string, instance: string) => `${ARENA_PATH}/runs/${e(run)}/${e(branch)}/pane/${e(instance)}`;
export const comparePath = (a: string, b: string) => `${ARENA_PATH}/compare/${e(a)}/${e(b)}`;
/** Where a popped-out Arena control pane opens. */
export const arenaPanePath = (instance: string) => `${ARENA_PATH}/pane/${e(instance)}`;

/** The Arena address an old `/tests…` one now lives at. The test team is edited in the Runs pane. */
export function legacyTestsPath(pathname: string, search = ''): string {
  const rest = pathname.replace(/^\/tests\/?/, '');
  if (rest === '' || rest === 'team' || rest.startsWith('team/')) return ARENA_PATH;
  return `${ARENA_PATH}/${rest}${search}`;
}
