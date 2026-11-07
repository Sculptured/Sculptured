import { expect, test, type APIRequestContext } from '@playwright/test';
import { claudeAgent, confirmStart, liveOnly, liveTeam, OK_JOB, TURN_MS } from './fixtures/live';

/**
 * The status page, against the real binary, with real Claude Code agents on haiku (live, see
 * fixtures/live.ts): the agent chosen for it writes the team's status page with its write_status
 * tool, and Results → Status shows it as a document.
 * Screenshots of the real app land in web/test-results/screens/status/.
 */

liveOnly();

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
  const write = `Call your write_status tool exactly once, with done set to true and markdown set to exactly the text between the markers (without the markers). Do nothing else, then reply with the single word OK.\n<<<\n${PAGE}\n>>>`;
  const { id } = await liveTeam(request, name, (draft, folder) => {
    draft.agents = [
      claudeAgent('builder', 'Builder', OK_JOB, folder, 40, 80),
      claudeAgent('reviewer', 'Reviewer', write, folder, 360, 80, { advanced: { writes_status: true } }),
    ];
    draft.connections = [];
  });
  return id;
}

test('the owner writes the status page and Results → Status shows it', async ({ page, request }, info) => {
  test.setTimeout(TURN_MS + 60_000);
  const id = await team(request, `Status ${Date.now()}-${info.project.name}`);
  await page.goto(`/p/${id}/results/status`);
  await expect(page.getByText('No status page yet.')).toBeVisible();
  await expect(page.getByText(/Reviewer writes it while the team works/)).toBeVisible();

  // Reviewer's first message is its job: write the page.
  await page.goto(`/team/${id}/structure`);
  await page.getByRole('button', { name: 'Start team' }).click();
  await confirmStart(page);
  await expect.poll(async () => (await (await request.get(`/api/teams/${id}/status`)).json()).page?.done ?? false, { timeout: TURN_MS }).toBe(true);

  await page.goto(`/p/${id}/results/status`);
  const doc = page.getByRole('article', { name: 'Status page' });
  await expect(doc.getByRole('heading', { name: 'Login page' })).toBeVisible();
  await expect(doc.getByRole('heading', { name: 'What changed' })).toBeVisible();
  await expect(doc.getByText('Final version')).toBeVisible();
  await expect(doc.getByText('Written by')).toContainText('Reviewer');
  await page.screenshot({ path: `${SHOTS}/status-${info.project.name}.png`, fullPage: true });
  await request.post(`/api/teams/${id}/stop`, { data: { request_id: `stop-${id}` } });
});
