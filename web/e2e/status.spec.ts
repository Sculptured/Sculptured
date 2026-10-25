import { expect, test, type APIRequestContext } from '@playwright/test';

/**
 * The status page, against the real binary, Practice agents only: the agent chosen for it writes
 * the team's status page with write_status, and Results → Status shows it as a document.
 * Screenshots of the real app land in web/test-results/screens/status/.
 */

const SHOTS = 'test-results/screens/status';

const PAGE = [
  '# Login page',
  '',
  '**State:** done. The login form, its checks and the session cookie are merged.',
  '',
  '## What changed',
  '- `web/src/Login.tsx`: the form, with inline errors',
  '- `server/session.rs`: a 14-day cookie, renewed on use',
  '',
  '## Open',
  '- Password reset is not built yet (out of scope for this run).',
].join('\n');

/** Builder and Reviewer both start on their jobs; Reviewer is chosen to write the status page. */
async function team(request: APIRequestContext, name: string): Promise<string> {
  const created = await request.post('/api/teams', { data: { request_id: `create-${name}`, template: 'blank', practice: true, name } });
  expect(created.ok()).toBe(true);
  const draft = (await created.json()).draft;
  const write = `write_status:${JSON.stringify({ markdown: PAGE, done: true })}`;
  draft.agents = [
    { id: 'builder', group_id: null, name: 'Builder', kind: 'practice', job: 'Build the login page.', folder: null, x: 40, y: 80 },
    { id: 'reviewer', group_id: null, name: 'Reviewer', kind: 'practice', job: write, folder: null, x: 360, y: 80, advanced: { writes_status: true } },
  ];
  draft.connections = [];
  const saved = await request.put(`/api/teams/${draft.id}`, { data: { request_id: `put-${name}`, revision: draft.revision, draft } });
  expect(saved.ok()).toBe(true);
  return draft.id;
}

test('the owner writes the status page and Results → Status shows it', async ({ page, request }, info) => {
  const id = await team(request, `Status ${Date.now()}-${info.project.name}`);
  await page.goto(`/p/${id}/results/status`);
  await expect(page.getByText('No status page yet.')).toBeVisible();
  await expect(page.getByText(/Reviewer writes it while the team works/)).toBeVisible();

  // Reviewer's first message is its job: the write_status script.
  await page.goto(`/team/${id}/structure`);
  await page.getByRole('button', { name: 'Start team' }).click();
  await expect.poll(async () => (await (await request.get(`/api/teams/${id}/status`)).json()).page?.done ?? false, { timeout: 30_000 }).toBe(true);

  await page.goto(`/p/${id}/results/status`);
  const doc = page.getByRole('article', { name: 'Status page' });
  await expect(doc.getByRole('heading', { name: 'Login page' })).toBeVisible();
  await expect(doc.getByRole('heading', { name: 'What changed' })).toBeVisible();
  await expect(doc.getByText('Final version')).toBeVisible();
  await expect(doc.getByText('Written by')).toContainText('Reviewer');
  await page.screenshot({ path: `${SHOTS}/status-${info.project.name}.png`, fullPage: true });
});
