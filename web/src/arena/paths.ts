// Every Arena address, in one place. A branch's page is the old run page for now; when it becomes
// the simulated Control workspace, `runPath` is the one link to change.
const e = encodeURIComponent;

export const ARENA_PATH = '/arena';
export const runPath = (run: string, branch: string, step?: number) =>
  `${ARENA_PATH}/runs/${e(run)}/${e(branch)}${step != null ? `?step=${step}` : ''}`;
export const comparePath = (a: string, b: string) => `${ARENA_PATH}/compare/${e(a)}/${e(b)}`;
/** Where a popped-out Arena control pane opens. */
export const arenaPanePath = (instance: string) => `${ARENA_PATH}/pane/${e(instance)}`;

/** The Arena address an old `/tests…` one now lives at. The test team is edited in the Runs pane. */
export function legacyTestsPath(pathname: string, search = ''): string {
  const rest = pathname.replace(/^\/tests\/?/, '');
  if (rest === '' || rest === 'team' || rest.startsWith('team/')) return ARENA_PATH;
  return `${ARENA_PATH}/${rest}${search}`;
}
