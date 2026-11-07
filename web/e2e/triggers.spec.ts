import { expect, test } from '@playwright/test';
import { claudeAgent, liveOnly, liveTeam, OK_JOB, TURN_MS } from './fixtures/live';

/**
 * Triggers in the real app, with a real Claude Code agent on haiku (live, see fixtures/live.ts): a plant
 * team has a trigger for machine alarms; another system posts an alarm to its address while the team is
 * stopped; Sculptured starts the team and the dispatcher gets the alarm after its job. Settings shows the
 * address and the event with what came of it. Screenshots land in web/test-results/screens/triggers/.
 */

liveOnly();

const SHOTS = 'test-results/screens/triggers';

test('an alarm posted to a trigger starts the team and reaches its agent', async ({ page, request }, info) => {
  test.setTimeout(TURN_MS + 60_000);
  const { id } = await liveTeam(request, `Plant ${info.project.name}`, (draft, folder) => {
    draft.agents = [claudeAgent('dispatch', 'Dispatch', OK_JOB, folder, 60, 80)];
    draft.connections = [];
    draft.triggers = [{ id: 'alarm', agent: 'dispatch', message: `A machine raised an alarm. ${OK_JOB}`, starts_team: true }];
  });

  await page.goto(`/p/${id}/settings`);
  const section = page.getByRole('region', { name: 'Events from other systems' });
  await expect(section.getByLabel('Address')).toContainText('/api/hooks/');
  const path = new URL((await section.getByLabel('Address').textContent())!).pathname;

  const posted = await request.post(path, { data: { machine: 'Press 4', code: 'VIB-HIGH', rms_mm_s: 11.2 } });
  expect(posted.status()).toBe(202);
  await expect.poll(async () => (await (await request.get(`/api/teams/${id}/triggers`)).json()).received[0]?.outcome ?? null, { timeout: TURN_MS })
    .toBe('started the team and sent');

  await page.reload();
  await expect(section.getByText('✓ started the team and sent')).toBeVisible({ timeout: 15_000 });
  await section.getByText('✓ started the team and sent').click();
  await expect(section.getByRole('list', { name: 'Trigger 1: latest events' }).getByText('VIB-HIGH')).toBeVisible();
  await section.screenshot({ path: `${SHOTS}/settings-${info.project.name}.png` });
  await request.post(`/api/teams/${id}/stop`, { data: { request_id: `stop-${id}` } });
});
