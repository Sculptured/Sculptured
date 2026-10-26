import { readFile } from 'node:fs/promises';
import { expect, test, type APIRequestContext, type Locator, type Page } from '@playwright/test';

/**
 * The demo path a new user walks, against the real binary, Practice agents only:
 * gallery → Structure edits → Start → Live → message an agent → Progress → Advanced
 * (View as spec, Export) → Stop. The server runs with a throwaway HOME holding only
 * projects/alpha and projects/beta (crates/e2e-server), so the folder picker never shows
 * the real home folder.
 */

/**
 * The gallery offers the template; the team is then created through the API with Practice agents,
 * since the gallery only offers Claude and Codex and this test must never start a real model.
 */
async function newTeam(page: Page, request: APIRequestContext, template: string, name: string) {
  await page.goto('/new');
  const gallery = page.getByRole('region', { name: 'New project' });
  await expect(gallery.getByRole('button', { name: new RegExp(`^${template}`) })).toBeVisible();
  const t = ((await (await request.get('/api/team-templates')).json()) as { id: string; title: string }[]).find((x) => x.title === template)!;
  const created = await request.post('/api/teams', { data: { request_id: `create-${name}`, template: t.id, practice: true, name } });
  expect(created.ok()).toBe(true);
  const id: string = (await created.json()).draft.id;
  await page.goto(`/p/${id}/board?mode=design`);
  await expect(page.getByRole('region', { name: 'Design board' })).toBeVisible();
  return id;
}

async function centre(l: Locator) {
  const b = (await l.boundingBox())!;
  return { x: b.x + b.width / 2, y: b.y + b.height / 2 };
}

async function drag(page: Page, from: { x: number; y: number }, to: { x: number; y: number }) {
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  await page.mouse.move(from.x + 8, from.y + 8, { steps: 2 });
  await page.mouse.move(to.x, to.y, { steps: 12 });
  await page.mouse.up();
}

type Rect = { x: number; y: number; width: number; height: number };
const overlap = (a: Rect, b: Rect) => a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;

// The save status is hidden on a phone's narrow header, so its text is checked, not its visibility.
// Saving is announced to screen readers only (no visible pill), so the page never jumps.
const saved = (page: Page) => expect(page.locator('.save-state')).toHaveText('saved');

/** Run: every agent of the team on one list, the default view. */
async function checkRun(page: Page, agent: string) {
  await page.getByRole('link', { name: 'Board' }).click();
  await page.getByRole('group', { name: 'Board mode' }).getByRole('button', { name: 'Run' }).click();
  await expect(page).toHaveURL(/\/p\/[^/]+\/board\?mode=run$/);
  await expect(page.getByRole('button', { name: new RegExp(`^${agent} — `) }).first()).toBeVisible();
}

/** Advanced, under Settings → Export: View as spec shows the read-only spec; Export downloads it. */
async function checkAdvanced(page: Page, scope: Locator, teamId: string, agent: string) {
  const advanced = scope.locator('details.team-advanced');
  await advanced.locator('summary').click();
  await advanced.getByRole('button', { name: 'View as spec' }).click();
  const spec = advanced.getByRole('region', { name: 'Team spec' });
  await expect(spec).toContainText(`${teamId}.rig.yaml`);
  await expect(spec.locator('pre')).toContainText('pods:');
  await expect(spec.locator('pre')).toContainText(agent);
  const [download] = await Promise.all([page.waitForEvent('download'), advanced.getByRole('button', { name: 'Export' }).click()]);
  expect(download.suggestedFilename()).toBe(`${teamId}.rig.yaml`);
  const text = await readFile((await download.path())!, 'utf8');
  expect(text).toContain('pods:');
  expect(text).toContain(agent);
  await expect(scope.getByRole('status')).toContainText(`Exported ${teamId}.rig.yaml`);
}

test('desktop: gallery → Structure edits → Start → Live → message → Progress → Advanced → Stop', async ({ page, request }, info) => {
  test.skip(info.project.name !== 'desktop', 'desktop flow');
  const id = await newTeam(page, request, 'Builder → Reviewer', `E2E ${Date.now()}`);

  // Structure: rename, a Job, and a folder from the picker. The throwaway home holds only "projects".
  await page.getByRole('button', { name: /^Builder — / }).click();
  await page.getByRole('textbox', { name: 'Name' }).fill('Maker');
  await page.getByRole('textbox', { name: 'Job' }).fill('tool:npm test');
  await page.getByRole('button', { name: 'Choose a folder for Maker' }).click();
  const picker = page.getByRole('dialog', { name: 'Choose a folder' });
  await expect(picker.getByRole('button', { name: 'Up' })).toBeDisabled();
  const homeList = picker.getByRole('list', { name: /^Folders in/ });
  // Other specs may leave their own folders in the shared throwaway home.
  await homeList.getByRole('button', { name: 'Open projects' }).click();
  const projects = picker.getByRole('list', { name: /^Folders in/ });
  await expect(projects.getByRole('button')).toHaveCount(2);
  await projects.getByRole('button', { name: 'Open alpha' }).click();
  await picker.getByRole('button', { name: /^Use ~\/projects\/alpha$/ }).click();
  await expect(page.getByRole('region', { name: 'Folder' })).toContainText('~/projects/alpha');
  await saved(page);
  // Clicking a card opened the agent's Setup page; back to the Design board.
  await page.getByRole('button', { name: 'Board', exact: true }).click();
  await expect(page.getByRole('region', { name: 'Design board' })).toBeVisible();

  // Drag an agent in, then connect Reviewer ● → it. The palette offers only Claude and Codex,
  // so the new Claude card is undone again before Start: this test never runs a real model.
  const vp = (await page.getByTestId('structure-viewport').boundingBox())!;
  await drag(page, await centre(page.getByRole('button', { name: 'Add a Claude agent' })), { x: vp.x + vp.width * 0.5, y: vp.y + vp.height * 0.8 });
  await expect(page.getByRole('button', { name: /^Claude 1 — / })).toBeVisible();
  const port = page.locator('.sb-card', { has: page.getByRole('button', { name: /^Reviewer — / }) }).locator('.port.out');
  await drag(page, await centre(port), await centre(page.getByRole('button', { name: /^Claude 1 — / })));
  await expect(page.getByRole('button', { name: 'Reviewer hands work to Claude 1', exact: true })).toBeVisible();
  // Labels are never clipped and never sit on a card.
  const cards = await page.locator('.sb-card').evaluateAll((els) => els.map((e) => e.getBoundingClientRect().toJSON() as Rect));
  for (const label of await page.locator('.sb-edge-label').all()) {
    const lb = (await label.boundingBox())!;
    for (const c of cards) expect(overlap(lb, c)).toBe(false);
    expect(await label.evaluate((el) => el.scrollWidth <= el.clientWidth + 1)).toBe(true);
  }
  // Undo the connection and the Claude card: only the two Practice agents start.
  await page.getByRole('button', { name: 'Undo' }).click();
  await page.getByRole('button', { name: 'Undo' }).click();
  await expect(page.getByRole('button', { name: /^Claude 1 — / })).toHaveCount(0);
  await expect(page.locator('.sb-card')).toHaveCount(2);
  await expect(page.getByRole('button', { name: /^(Maker|Reviewer) — Practice/ })).toHaveCount(2);
  await saved(page);

  // Start → Live.
  await page.getByRole('button', { name: 'Start team' }).click();
  await expect(page).toHaveURL(new RegExp(`/p/${id}/board\\?mode=run$`));
  await expect(page.getByRole('button', { name: /^Maker — / })).toBeVisible();
  await expect(page.getByRole('button', { name: /^Reviewer — / })).toBeVisible();

  // Message an agent: its Job came first ("sent at start"), then what you send.
  await page.getByRole('button', { name: /^Maker — / }).click();
  const convo = page.getByRole('list', { name: 'Conversation' });
  await expect(convo.getByText('Job · sent at start')).toBeVisible();
  await expect(convo.getByText('stub received: tool:npm test')).toBeVisible();
  await page.getByRole('textbox', { name: 'Message' }).fill('hello');
  await page.getByRole('button', { name: 'Send' }).click();
  await expect(convo.getByText('stub received: hello')).toBeVisible();
  await page.keyboard.press('Escape');

  await checkRun(page, 'Maker');

  // Structure is read-only while the team runs; Advanced still shows the spec and exports it.
  await page.getByRole('group', { name: 'Board mode' }).getByRole('button', { name: 'Design' }).click();
  await expect(page.getByText(/Running: you can rename agents/)).toBeVisible();
  await expect(page.getByRole('button', { name: 'Add a Claude agent' })).toBeDisabled();
  await page.getByRole('link', { name: 'Settings' }).click();
  await checkAdvanced(page, page.getByRole('region', { name: 'Export' }), id, 'Maker');

  // Stop (the project bar has it on every page).
  await page.getByRole('link', { name: 'Board' }).click();
  await page.getByRole('button', { name: 'Stop team…' }).click();
  await page.getByRole('button', { name: 'Confirm stop team' }).click();
  await expect(page.getByRole('button', { name: 'Start team' })).toBeVisible({ timeout: 20_000 });
  // A stopped team can be started again. Stopped agents leave the Run board; the Agents page lists them.
  await page.getByRole('link', { name: 'Agents' }).click();
  await expect(page.getByRole('row', { name: /\bMaker\b.*stopped/ })).toBeVisible({ timeout: 20_000 });
});

test('mobile: gallery → form edits → Start → list → message → Progress → Advanced → Stop', async ({ page, request }, info) => {
  test.skip(info.project.name !== 'mobile', 'mobile flow');
  const id = await newTeam(page, request, 'Builder → Reviewer', `E2E phone ${Date.now()}`);

  // On a phone an agent is set up on its own one-column page.
  await page.getByRole('link', { name: 'Agents' }).tap();
  await page.getByRole('table').getByRole('link', { name: /^Builder/ }).tap();
  await page.getByRole('region', { name: 'Setup' }).getByRole('textbox', { name: 'Job' }).fill('Write the change notes');
  await saved(page);

  await page.getByRole('button', { name: 'Start team' }).tap();
  await expect(page).toHaveURL(new RegExp(`/p/${id}/board\\?mode=run$`));
  await page.getByRole('link', { name: 'Agents' }).tap();
  await expect(page.getByText('Builder hands work to Reviewer')).toBeVisible();
  const row = page.getByRole('table').getByRole('link', { name: /^Builder/ });
  await expect(row).toBeVisible();
  await row.tap();
  const convo = page.getByRole('list', { name: 'Conversation' });
  await expect(convo.getByText('stub received: Write the change notes')).toBeVisible();
  await page.getByRole('textbox', { name: 'Message' }).fill('hello from the phone');
  await page.getByRole('button', { name: 'Send' }).tap();
  await expect(convo.getByText('stub received: hello from the phone')).toBeVisible();
  await page.goBack();

  await checkRun(page, 'Builder');

  await page.getByRole('link', { name: 'Settings' }).tap();
  await checkAdvanced(page, page.getByRole('region', { name: 'Export' }), id, 'Builder');

  await page.getByRole('button', { name: 'Stop team…' }).tap();
  await page.getByRole('button', { name: 'Confirm stop team' }).tap();
  // Stopped agents leave the Run board; the Agents page lists them.
  await page.getByRole('link', { name: 'Agents' }).click();
  await expect(page.getByRole('row', { name: /\bBuilder\b.*stopped/ })).toBeVisible({ timeout: 20_000 });
});
