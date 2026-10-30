const e = encodeURIComponent;

export const TESTS_PATH = '/tests';
export const runPath = (run: string, branch: string, step?: number) =>
  `/tests/runs/${e(run)}/${e(branch)}${step != null ? `?step=${step}` : ''}`;
export const comparePath = (a: string, b: string) => `/tests/compare/${e(a)}/${e(b)}`;
