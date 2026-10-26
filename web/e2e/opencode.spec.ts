import { expect, test } from '@playwright/test';

/**
 * OpenCode agents in the real app, at design time only: no OpenCode agent is started (routine tests
 * call no model). An agent is switched to OpenCode on the board and given an open model; Start then
 * says OpenCode is not installed on this computer, with the command to install it.
 * Screenshots of the real app land in web/test-results/screens/opencode/.
 */
const SHOTS = 'test-results/screens/opencode';

test('an agent switched to OpenCode keeps its open model, and Start asks for OpenCode first', async ({ page, request }, info) => {
  const stamp = `${Date.now()}-${info.project.name}`;
  const home = (await (await request.get('/api/folders')).json()).home as string;
  const made = await request.post('/api/folders', { data: { request_id: `mk-${stamp}`, parent: home, name: `opencode-${stamp}` } });
  expect(made.ok()).toBe(true);
  const folder = (await made.json()).path as string;
  const created = await request.post('/api/teams', { data: { request_id: `create-${stamp}`, template: 'blank', practice: false, name: `Open models ${stamp}` } });
  expect(created.ok()).toBe(true);
  const draft = (await created.json()).draft;
  draft.agents = [{ id: 'writer', group_id: null, name: 'Writer', kind: 'claude', job: 'Write the page.', folder, x: 60, y: 80 }];
  draft.connections = [];
  const saved = await request.put(`/api/teams/${draft.id}`, { data: { request_id: `put-${stamp}`, revision: draft.revision, draft } });
  expect(saved.ok()).toBe(true);

  await page.goto(`/team/${draft.id}/structure`);
  await page.getByRole('button', { name: /^Writer — / }).click();
  const runsOn = page.getByRole('radiogroup', { name: 'Writer runs on' });
  await runsOn.getByRole('radio', { name: /OpenCode/ }).click();
  await expect(runsOn.getByRole('radio', { name: /OpenCode/ })).toHaveAttribute('aria-checked', 'true');
  await page.getByRole('combobox', { name: 'OpenCode model for Writer' }).selectOption({ label: 'Kimi K2.7 Code' });
  await expect
    .poll(async () => {
      const d = (await (await request.get(`/api/teams/${draft.id}`)).json()).draft;
      return [d.agents[0].kind, d.agents[0].advanced.model];
    })
    .toEqual(['opencode', 'openrouter/moonshotai/kimi-k2.7-code']);
  await page.screenshot({ path: `${SHOTS}/agent-on-opencode-${info.project.name}.png`, fullPage: true });

  await page.getByRole('button', { name: 'Start team' }).click();
  const dialog = page.getByRole('alertdialog', { name: 'Install before starting' });
  await expect(dialog).toContainText("OpenCode isn't installed on the computer that runs this team.");
  await expect(page.getByText('curl -fsSL https://opencode.ai/install | bash')).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/start-needs-opencode-${info.project.name}.png`, fullPage: true });
});
