import { expect, test, type APIRequestContext, type Page } from '@playwright/test';

/**
 * Live specs: teams of real Claude Code agents on haiku. They cost a little and the models' replies
 * vary, so they skip unless RIG_E2E_LIVE=1 and assert only what Sculptured records and controls
 * (statuses, handoffs delivered, events, counts), never exact replies. The `claude` CLI must be
 * installed and signed in where crates/e2e-server runs serve. Run them with:
 *
 *   RIG_E2E_LIVE=1 npm --prefix web run e2e               # every spec, live ones included
 *   RIG_E2E_LIVE=1 npm --prefix web run e2e -- handoff    # one live spec
 *
 * Every agent works in its own folder under serve's throwaway HOME (see `workFolder`).
 */

export const LIVE = process.env.RIG_E2E_LIVE === '1';
export const LIVE_REASON = 'live: runs real Claude Code agents on haiku; set RIG_E2E_LIVE=1';

/** Skips the calling test (or, at the top of a file, every test in it) unless live specs are on. */
export const liveOnly = () => test.skip(!LIVE, LIVE_REASON);

export const CLAUDE_MODEL = 'haiku';

/** The cheapest job: one short turn. */
export const OK_JOB = 'Reply with the single word OK and nothing else.';

/** A job that makes the agent answer with exactly `text`. */
export const replyWith = (text: string) => `Reply with exactly the text between the markers, without the markers, and nothing else, every time you are asked.\n<<<\n${text}\n>>>`;

export type LiveAgent = { id: string; group_id: string | null; name: string; kind: 'claude'; job: string; folder: string; x: number; y: number; advanced?: Record<string, unknown> };

/** A fresh folder under the e2e server's HOME (an allowed folder), for agents to work in. */
export async function workFolder(request: APIRequestContext, name: string): Promise<string> {
  const home = (await (await request.get('/api/folders')).json()).home as string;
  const made = await request.post('/api/folders', { data: { request_id: `mk-${name}`, parent: home, name } });
  expect(made.ok(), await made.text()).toBe(true);
  return (await made.json()).path as string;
}

/** A Claude agent on the board. */
export const claudeAgent = (id: string, name: string, job: string, folder: string, x: number, y: number, extra: Partial<LiveAgent> = {}): LiveAgent =>
  ({ id, group_id: null, name, kind: 'claude', job, folder, x, y, ...extra });

const slug = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 60);

/**
 * A team made through the API on `template` (blank by default), on haiku, with a work folder of its
 * own. `shape` sets its agents and anything else on the draft; it is saved and its draft returned.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export async function liveTeam(request: APIRequestContext, name: string, shape: (draft: any, folder: string) => void, template = 'blank') {
  const stamp = `${slug(name)}-${Date.now()}`;
  const folder = await workFolder(request, stamp);
  const created = await request.post('/api/teams', { data: { request_id: `create-${stamp}`, template, name } });
  expect(created.ok(), await created.text()).toBe(true);
  const draft = (await created.json()).draft;
  draft.models = { ...(draft.models ?? {}), claude: CLAUDE_MODEL, codex: draft.models?.codex ?? null };
  shape(draft, folder);
  const saved = await request.put(`/api/teams/${draft.id}`, { data: { request_id: `put-${stamp}`, revision: draft.revision, draft } });
  expect(saved.ok(), await saved.text()).toBe(true);
  return { id: draft.id as string, folder, draft: (await saved.json()).draft };
}

export type SeatEvent = { seq: number; actor: { kind: string; id: string }; body: { type: string; text?: string } };

export async function seatEvents(request: APIRequestContext, seat: string): Promise<SeatEvent[]> {
  const r = await request.get(`/api/seats/${encodeURIComponent(seat)}/events?after=0&limit=2000`);
  expect(r.ok()).toBe(true);
  return r.json();
}

/** How many turns an agent has finished. */
export const turnsEnded = async (request: APIRequestContext, seat: string) => (await seatEvents(request, seat)).filter((e) => e.body.type === 'turn-ended').length;

/** Real agents ask before they start ("Start 2 Claude agents?"): confirms it after a Start was clicked. */
export async function confirmStart(page: Page) {
  const dialog = page.getByRole('alertdialog', { name: /^Start / });
  await expect(dialog).toContainText('Claude');
  await dialog.getByRole('button', { name: 'Start team' }).click();
}

/** A model turn can take a while on a busy machine. */
export const TURN_MS = 240_000;
