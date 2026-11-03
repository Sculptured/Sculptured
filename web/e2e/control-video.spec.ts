import { expect, test, type APIRequestContext, type Browser, type BrowserContext, type Page } from '@playwright/test';
import { AGENTS, centre, drag, ORDER, practiceTeam, say, stackOf, startFromControl } from './fixtures/control-team';
import { beat, caption, glide, keepVideo, press, recorder, shot, SHOTS, stills } from './fixtures/recording';

/**
 * The screenshots and usage videos of docs/guide/control.md, recorded from the real app with Practice
 * agents (crates/e2e-server). Runs only with RECORD_VIDEOS=1:
 *
 *   RECORD_VIDEOS=1 npx playwright test control-video.spec.ts
 *
 * Screenshots go to docs/images/control/, videos to docs/videos/ (WebM and, with ffmpeg, MP4). The
 * Arena captures need a running arenad that serve reaches (SCULPTURED_ARENA_URL) with the retail
 * example and a scripted OpenAI-compatible stand-in as a local model server (ARENA_LOCAL_MODELS), like
 * the fake upstream of arena's own tests, so no real model is called; without them they are skipped
 * and say so. The screenshots expect at least one finished run of task 27.
 */

test.skip(!process.env.RECORD_VIDEOS, 'set RECORD_VIDEOS=1 to record the guide screenshots and videos');
test.describe.configure({ mode: 'serial', timeout: 240_000 });

const DESKTOP = { width: 1440, height: 900 };
const desktopOnly = (name: string) => test.skip(name !== 'desktop', 'recorded on the desktop project');

async function dismissToasts(page: Page) {
  for (const close of await page.locator('.toast-x').all()) await close.click({ timeout: 1000 }).catch(() => {});
}

/** A guide screenshot, with any notices that popped up dismissed first. */
async function guideShot(page: Page, name: string) {
  await dismissToasts(page);
  await shot(page, name);
}

/** A started team, its Buyer waiting on an approval, an alarm lit and the Buyer selected. */
async function busyTeam(page: Page, request: APIRequestContext, name: string) {
  const id = await practiceTeam(request, name);
  await page.goto(`/p/${id}/control`);
  await startFromControl(page);
  // Spread over a few seconds, so the Timeline has room to show them apart.
  await page.waitForTimeout(4000);
  await say(request, id, 'buyer', ORDER);
  await page.waitForTimeout(4000);
  await say(request, id, 'inspector', 'limit');
  await expect(page.getByRole('button', { name: /^Master caution: \d+ not acknowledged/ })).toBeVisible({ timeout: 30_000 });
  await expect.poll(async () => (await request.get(`/api/teams/${id}/questions`)).text(), { timeout: 30_000 }).toContain('approval');
  return id;
}

const stop = (request: APIRequestContext, id: string) => request.post(`/api/teams/${id}/stop`, { data: { request_id: `stop-${id}-${Date.now()}` } });

test('video 1: open a project on Control and start the team', async ({ browser, request, baseURL }, info) => {
  desktopOnly(info.project.name);
  const id = await practiceTeam(request, 'Press line 4 upkeep');
  const { context, page } = await recorder(browser, info, baseURL!);
  await page.goto('/');
  await caption(page, 'Open a project from Projects');
  await beat(page, 1500);
  await press(page, page.getByRole('link', { name: 'Press line 4 upkeep' }).first());
  if (!/\/control$/.test(page.url())) await press(page, page.getByRole('link', { name: 'Control', exact: true }));
  await expect(page).toHaveURL(new RegExp(`/p/${id}/control$`));
  await caption(page, 'Control opens with the live board first; the header holds the run status and the team controls');
  await beat(page, 3500);
  await glide(page, page.getByRole('list', { name: 'Run status' }));
  await beat(page, 1500);
  await caption(page, 'The master caution lights when an alarm needs you');
  await glide(page, page.getByRole('button', { name: /^Master caution/ }));
  await beat(page, 2000);
  await caption(page, 'Start runs the team');
  await press(page, page.getByRole('group', { name: 'Team controls' }).getByRole('button', { name: 'Start' }));
  for (const a of AGENTS) await expect(stackOf(page, 'live-overview').getByRole('button', { name: new RegExp(`^${a} — `) })).toBeVisible({ timeout: 30_000 });
  await caption(page, 'The board, Status and Timeline fill in as the agents work');
  await beat(page, 5000);
  await dismissToasts(page);
  await caption(page, 'Click an agent to select it everywhere');
  await press(page, stackOf(page, 'live-overview').getByRole('button', { name: /^Buyer — / }));
  await beat(page, 3000);
  await keepVideo(context, page, info, 'control-1-open-and-start');
  await stop(request, id);
});

/** Starts the team through the API and opens its Control, recording. */
async function recordedTeam(browser: Browser, request: APIRequestContext, info: Parameters<typeof recorder>[1], baseURL: string) {
  const id = await practiceTeam(request, 'Press line 4 upkeep');
  expect((await request.post(`/api/teams/${id}/start`, { data: { request_id: `start-${id}` } })).ok()).toBe(true);
  const rec = await recorder(browser, info, baseURL);
  await rec.page.goto(`/p/${id}/control`);
  await expect(stackOf(rec.page, 'live-overview').getByRole('button', { name: /^Inspector — / })).toBeVisible({ timeout: 30_000 });
  await rec.page.waitForTimeout(2500);
  await dismissToasts(rec.page);
  await stackOf(rec.page, 'live-overview').getByRole('button', { name: 'Fit' }).click();
  return { id, ...rec };
}

async function dragTab(page: Page, pane: string, to: { x: number; y: number }) {
  const from = await centre(page.locator(`[data-tab="${pane}"]`));
  await glide(page, page.locator(`[data-tab="${pane}"]`));
  await drag(page, from, to, 40);
}

test('video 2: rearrange panes and switch layouts', async ({ browser, request, baseURL }, info) => {
  desktopOnly(info.project.name);
  const { id, context, page } = await recordedTeam(browser, request, info, baseURL!);
  await caption(page, 'Drag a tab to the edge of a pane to split it');
  await beat(page);
  const timeline = (await stackOf(page, 'live-timeline').boundingBox())!;
  await dragTab(page, 'live-status', { x: timeline.x + timeline.width - 20, y: timeline.y + timeline.height / 2 });
  await beat(page, 2000);
  await caption(page, 'Drop it in the middle to join it as a tab');
  await dragTab(page, 'live-raw', await centre(stackOf(page, 'live-status').locator('.ws-body')));
  await beat(page, 2000);
  await caption(page, 'Drag the gap between panes to resize them (or focus it and use the arrow keys)');
  const gap = await centre(page.getByRole('separator').first());
  await page.mouse.move(gap.x, gap.y, { steps: 18 });
  await drag(page, gap, { x: gap.x - 160, y: gap.y }, 30);
  await beat(page, 2000);
  await caption(page, 'Every move has a keyboard path: ⋯ then Move to…');
  await press(page, page.getByRole('button', { name: 'Timeline options' }));
  await beat(page, 800);
  await page.keyboard.press('ArrowRight');
  await beat(page, 1200);
  await press(page, page.getByRole('menuitem', { name: 'As a tab with System overview' }));
  await beat(page, 2000);
  await caption(page, 'Pop a pane out into its own window');
  await press(page, page.locator('[data-tab="live-alarms"]'));
  await press(page, page.getByRole('button', { name: 'Alarms options' }));
  const popup = context.waitForEvent('page');
  await press(page, page.getByRole('menuitem', { name: 'Pop out' }));
  await (await popup).waitForLoadState();
  await page.bringToFront();
  await expect(page.getByText('Shown in another window.')).toBeVisible();
  await beat(page, 2000);
  await caption(page, 'Bring it back when you are done');
  await press(page, page.getByRole('button', { name: 'Bring back' }));
  await beat(page, 1500);
  await caption(page, 'Add pane offers every kind of pane; ✕ closes one');
  await press(page, page.getByRole('button', { name: '+ Add pane' }));
  await beat(page, 1000);
  await press(page, page.getByRole('menuitem', { name: /Metrics/ }));
  await beat(page, 2000);
  await press(page, page.getByRole('button', { name: 'Close Metrics' }));
  await beat(page, 1200);
  await caption(page, 'Layouts: Live, Agent focus and Cost, or your own with +');
  for (const name of ['Agent focus', 'Cost', 'Live']) { await press(page, page.getByRole('tab', { name, exact: true })); await beat(page, 1800); }
  await caption(page, 'Look: Mission, or follow the app’s own look');
  await page.getByLabel('Look').selectOption('app');
  await beat(page, 2000);
  await page.getByLabel('Look').selectOption('mission');
  await caption(page, 'Reset to default (⋯ beside the layouts) puts a layout back as it was');
  await press(page, page.getByRole('button', { name: 'Layout options' }));
  await press(page, page.getByRole('menuitem', { name: 'Reset to default' }));
  await beat(page, 2500);
  await keepVideo(context, page, info, 'control-2-rearrange');
  await stop(request, id);
});

test('video 3: an alarm, an approval, pause, resume and emergency stop', async ({ browser, request, baseURL }, info) => {
  desktopOnly(info.project.name);
  const { id, context, page } = await recordedTeam(browser, request, info, baseURL!);
  const controls = page.getByRole('group', { name: 'Team controls' });
  const strip = page.getByRole('list', { name: 'Run status' });
  await caption(page, 'An agent reaches a usage limit: the master caution lights');
  await say(request, id, 'inspector', 'limit');
  const caution = page.getByRole('button', { name: /^Master caution: \d+ not acknowledged/ });
  await expect(caution).toBeVisible({ timeout: 30_000 });
  await beat(page, 2500);
  await caption(page, 'Press it to open Alarms, then acknowledge each alarm');
  await press(page, caution);
  await beat(page, 1500);
  const ack = page.getByRole('button', { name: 'Acknowledge', exact: true });
  for (let i = 0; i < 3 && (await ack.count()) > 0; i++) { await press(page, ack.first()); await beat(page, 1200); }
  await beat(page, 1000);
  await caption(page, 'The Buyer proposes an order over the team’s $5,000 rule: it waits in Approvals');
  await say(request, id, 'buyer', ORDER);
  await press(page, page.locator('[data-tab="live-approvals"]'));
  await expect(page.getByRole('list', { name: 'Waiting for you' })).toBeVisible({ timeout: 30_000 });
  await beat(page, 2500);
  await press(page, page.getByRole('button', { name: 'Approve' }));
  await expect(page.getByText('Nothing is waiting for you.')).toBeVisible({ timeout: 30_000 });
  await beat(page, 1500);
  await caption(page, 'Pause all: each agent stops at its next safe point and keeps its session');
  await press(page, controls.getByRole('button', { name: 'Pause all' }));
  await expect(strip).toContainText('3 paused', { timeout: 15_000 });
  await beat(page, 2500);
  await caption(page, 'Resume all: they carry on');
  await press(page, controls.getByRole('button', { name: 'Resume all' }));
  await expect(strip).toContainText('3 running', { timeout: 15_000 });
  await beat(page, 2500);
  await caption(page, 'Emergency stop asks first, naming how many agents it stops');
  await press(page, controls.getByRole('button', { name: 'Emergency stop' }));
  await beat(page, 2500);
  await caption(page, 'Stop now stops every agent at once');
  await press(page, page.getByRole('alertdialog', { name: 'Emergency stop' }).getByRole('button', { name: 'Stop now' }));
  await expect(strip).toContainText('3 stopped', { timeout: 30_000 });
  await beat(page, 3500);
  await keepVideo(context, page, info, 'control-3-trouble');
});

test('screenshots: alarms, an approval waiting and the emergency-stop question', async ({ browser, request, baseURL }, info) => {
  desktopOnly(info.project.name);
  const { context, page } = await stills(browser, baseURL!);
  const id = await practiceTeam(request, 'Press line 4 upkeep');
  await page.goto(`/p/${id}/control`);
  await startFromControl(page);
  await stackOf(page, 'live-overview').getByRole('button', { name: 'Fit' }).click();
  await page.waitForTimeout(2500);
  await caption(page, 'An agent reaches a usage limit: the master caution lights; press it to open Alarms');
  await say(request, id, 'inspector', 'limit');
  const caution = page.getByRole('button', { name: /^Master caution: \d+ not acknowledged/ });
  await expect(caution).toBeVisible({ timeout: 30_000 });
  await caution.click();
  await expect(page.getByRole('button', { name: 'Acknowledge', exact: true }).first()).toBeVisible();
  await beat(page, 1500);
  await guideShot(page, 'alarms-caution');
  await caption(page, 'The Buyer proposes an order over the team’s $5,000 rule: it waits in Approvals');
  await say(request, id, 'buyer', ORDER);
  await page.locator('[data-tab="live-approvals"]').click();
  await expect(page.getByRole('list', { name: 'Waiting for you' })).toBeVisible({ timeout: 30_000 });
  await beat(page, 800);
  await guideShot(page, 'approval-waiting');
  await caption(page, 'Emergency stop asks first, naming how many agents it stops');
  await page.getByRole('group', { name: 'Team controls' }).getByRole('button', { name: 'Emergency stop' }).click();
  await expect(page.getByRole('alertdialog', { name: 'Emergency stop' })).toBeVisible();
  await beat(page, 800);
  await guideShot(page, 'emergency-stop-confirm');
  await page.getByRole('alertdialog', { name: 'Emergency stop' }).getByRole('button', { name: 'Cancel' }).click();
  await stop(request, id);
  await context.close();
});

test('screenshots: Control Live, Agent focus, Cost and the Follow app look', async ({ browser, request, baseURL }, info) => {
  desktopOnly(info.project.name);
  const context = await browser.newContext({ baseURL, viewport: DESKTOP });
  const page = await context.newPage();
  const id = await busyTeam(page, request, 'Press line 4 upkeep');
  await page.locator('[data-tab="live-approvals"]').click();
  await expect(page.getByRole('list', { name: 'Waiting for you' })).toBeVisible({ timeout: 30_000 });
  await stackOf(page, 'live-overview').getByRole('button', { name: 'Fit' }).click();
  await beat(page, 3000);
  await guideShot(page, 'control-live');
  // Clicking an agent opens it (Agent detail) and selects it everywhere.
  await stackOf(page, 'live-overview').getByRole('button', { name: /^Buyer — / }).click();
  await page.getByRole('tab', { name: 'Agent focus' }).click();
  await expect(page.locator('.ctl-agent-name')).toHaveText('Buyer');
  await beat(page);
  await guideShot(page, 'agent-focus');
  await page.getByRole('tab', { name: 'Cost' }).click();
  await beat(page);
  await guideShot(page, 'cost');
  await page.getByRole('tab', { name: 'Live' }).click();
  await page.getByLabel('Look').selectOption('app');
  await dismissToasts(page);
  await beat(page);
  await guideShot(page, 'follow-app-look');
  await stop(request, id);
  await context.close();
});

test('screenshots: Control on a phone', async ({ page, request }, info) => {
  test.skip(info.project.name !== 'mobile', 'the phone layout');
  const id = await practiceTeam(request, 'Press line 4 upkeep (phone)');
  await page.goto(`/p/${id}/control`);
  const more = page.getByText('Agents, tokens, cost and team controls');
  await more.click();
  await page.getByRole('group', { name: 'Team controls' }).getByRole('button', { name: 'Start' }).click();
  await expect(page.getByRole('button', { name: /^Planner — / })).toBeVisible({ timeout: 30_000 });
  await more.click();
  await page.getByRole('button', { name: 'Fit' }).click();
  await beat(page, 3000);
  await dismissToasts(page);
  // The one-line header over the Live layout's first pane.
  await page.screenshot({ path: `${SHOTS}phone-live.png` });
  // "Agents, tokens, cost and team controls" opens the rest of the header.
  await more.click();
  await beat(page, 800);
  await dismissToasts(page);
  await page.screenshot({ path: `${SHOTS}phone-more.png` });
  await more.click();
  // The pane switcher: one pane at a time.
  await page.getByRole('combobox', { name: 'Pane', exact: true }).selectOption({ label: 'Status' });
  await page.getByRole('combobox', { name: 'Pane', exact: true }).focus();
  await beat(page, 1200);
  await dismissToasts(page);
  await page.screenshot({ path: `${SHOTS}phone-panes.png` });
  await stop(request, id);
});

/** True when serve reaches an arenad (SCULPTURED_ARENA_URL). */
async function arenaUp(request: APIRequestContext) {
  const r = await request.get('/api/arena/health').catch(() => null);
  return !!r?.ok();
}

/**
 * Keeps Arena's suggested team in this browser (as Edit team would), each agent in API mode on the
 * first local model server arenad offers. The guide's arenad has a scripted stand-in there
 * (ARENA_LOCAL_MODELS), so no real model is called; without one, the Arena captures are skipped.
 */
async function scriptedArenaTeam(context: BrowserContext, request: APIRequestContext) {
  const providers: { id: string; kind: string; available: boolean; models?: string[] }[] = await (await request.get('/api/arena/providers')).json();
  const local = providers.find((p) => p.kind === 'local' && p.available && p.models?.length);
  test.skip(!local, 'arenad offers no local model server (ARENA_LOCAL_MODELS) for a scripted stand-in');
  const team = await (await request.get('/api/arena/team/default')).json();
  for (const a of team.agents) Object.assign(a, { kind: 'open', provider: local!.id, model: local!.models![0] });
  await context.addInitScript((t) => localStorage.setItem('sculptured.tests.team.v1', t), JSON.stringify(team));
}

test('video 4: Arena control, a run’s Replay and a fork', async ({ browser, request, baseURL }, info) => {
  desktopOnly(info.project.name);
  test.skip(!(await arenaUp(request)), 'no arenad answers (start one and set SCULPTURED_ARENA_URL)');
  const { context, page } = await recorder(browser, info, baseURL!);
  await scriptedArenaTeam(context, request);
  await page.goto('/arena');
  await caption(page, 'Arena control: the runs, the world and the engine at a glance');
  await expect(page.getByRole('table', { name: 'Runs' })).toBeVisible();
  await beat(page, 3000);
  await caption(page, 'Start a run: a task and a team. The run opens on Replay');
  await glide(page, page.getByRole('region', { name: 'New run' }));
  await beat(page, 1500);
  await press(page, page.getByRole('button', { name: 'Start run' }));
  await expect(page).toHaveURL(/\/arena\/runs\//, { timeout: 30_000 });
  await caption(page, 'The timeline fills in as the agents work (a scripted stand-in plays the model here)');
  await expect(page.getByText('Passed the check').first()).toBeVisible({ timeout: 90_000 });
  await beat(page, 2000);
  await caption(page, 'Step through it with ◀ ▶ or the arrow keys');
  for (let i = 0; i < 2; i++) { await press(page, page.getByRole('button', { name: 'Previous step' })); await beat(page, 1200); }
  await caption(page, 'Fork from here: the world is restored to just before this step');
  await press(page, page.getByRole('button', { name: /Fork from here/ }).first());
  const panel = page.getByRole('region', { name: /^Fork from step/ });
  await expect(panel).toBeVisible();
  await beat(page, 1500);
  await caption(page, 'Exact or Approximate: the plan says how each part of the world is restored');
  await press(page, panel.getByRole('radio', { name: 'Approximate' }).or(panel.getByRole('button', { name: 'Approximate' })));
  await beat(page, 2000);
  await press(page, panel.getByRole('radio', { name: 'Exact' }).or(panel.getByRole('button', { name: 'Exact' })));
  await beat(page, 1500);
  await caption(page, 'Change an agent’s instructions, then start the fork');
  const instructions = panel.getByRole('textbox', { name: 'Instructions' }).last();
  await press(page, instructions);
  await instructions.press('End');
  await instructions.pressSequentially(' Confirm the new item is in stock first.', { delay: 35 });
  await beat(page, 800);
  await press(page, panel.getByRole('button', { name: /^Fork with/ }));
  await expect(page.getByText('Passed the check').first()).toBeVisible({ timeout: 90_000 });
  await caption(page, 'The fork runs on its own branch; Compare shows how it differs');
  await beat(page, 4000);
  await keepVideo(context, page, info, 'control-4-arena', 44);
});

test('screenshots: Arena control, Storage, a simulated run’s Replay and a fork', async ({ browser, request, baseURL }, info) => {
  desktopOnly(info.project.name);
  test.skip(!(await arenaUp(request)), 'no arenad answers (start one and set SCULPTURED_ARENA_URL)');
  const { context, page } = await stills(browser, baseURL!);
  await scriptedArenaTeam(context, request);
  await page.goto('/arena');
  await caption(page, 'Arena control: the runs, the world and the engine at a glance');
  await expect(page.getByRole('row', { name: 'Run of task 27' }).first()).toBeVisible();
  await beat(page, 2000);
  await guideShot(page, 'arena-control');
  await caption(page, 'Storage: saves against the limit, and each run’s fork tree');
  await page.getByRole('tab', { name: 'Storage' }).click();
  await beat(page, 2000);
  await guideShot(page, 'arena-storage');
  await page.getByRole('tab', { name: 'Overview' }).click();
  await page.getByRole('row', { name: 'Run of task 27' }).first().getByRole('link').first().click();
  await expect(page.getByText('Passed the check').first()).toBeVisible({ timeout: 30_000 });
  await caption(page, 'A simulated run on Replay: step through it with ◀ ▶ or the arrow keys');
  await page.getByRole('button', { name: 'Previous step' }).click();
  await page.getByRole('button', { name: 'Previous step' }).click();
  await beat(page, 1500);
  await guideShot(page, 'arena-replay');
  await caption(page, 'Fork from here: the plan says how each part of the world is restored');
  await page.getByRole('button', { name: /Fork from here/ }).first().click();
  await expect(page.getByRole('region', { name: /^Fork from step/ })).toBeVisible();
  await beat(page, 1500);
  await guideShot(page, 'arena-fork');
  await context.close();
});
