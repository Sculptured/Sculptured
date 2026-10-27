import { expect, test } from '@playwright/test';

/**
 * Triggers in the real app, Practice agents only: a plant team has a trigger for machine alarms; another
 * system posts an alarm to its address while the team is stopped; Sculptured starts the team and the
 * dispatcher gets the alarm after its job. Settings shows the address and the event with what came of
 * it. Screenshots land in web/test-results/screens/triggers/.
 */
const SHOTS = 'test-results/screens/triggers';

test('an alarm posted to a trigger starts the team and reaches its agent', async ({ page, request }, info) => {
  const name = `Plant ${Date.now()}-${info.project.name}`;
  const created = await request.post('/api/teams', { data: { request_id: `create-${name}`, template: 'blank', practice: true, name } });
  expect(created.ok()).toBe(true);
  const draft = (await created.json()).draft;
  draft.agents = [{ id: 'dispatch', group_id: null, name: 'Dispatch', kind: 'practice', job: 'Handle machine alarms.', folder: null, x: 60, y: 80 }];
  draft.connections = [];
  draft.triggers = [{ id: 'alarm', agent: 'dispatch', message: 'A machine raised an alarm. Open a work order and find the part.', starts_team: true }];
  const saved = await request.put(`/api/teams/${draft.id}`, { data: { request_id: `put-${name}`, revision: draft.revision, draft } });
  expect(saved.ok()).toBe(true);

  await page.goto(`/p/${draft.id}/settings`);
  const section = page.getByRole('region', { name: 'Events from other systems' });
  await expect(section.getByLabel('Address')).toContainText('/api/hooks/');
  const path = new URL((await section.getByLabel('Address').textContent())!).pathname;

  const posted = await request.post(path, { data: { machine: 'Press 4', code: 'VIB-HIGH', rms_mm_s: 11.2 } });
  expect(posted.status()).toBe(202);
  await expect.poll(async () => (await (await request.get(`/api/teams/${draft.id}/triggers`)).json()).received[0]?.outcome ?? null, { timeout: 60_000 })
    .toBe('started the team and sent');

  await page.reload();
  await expect(section.getByText('✓ started the team and sent')).toBeVisible({ timeout: 15_000 });
  await section.getByText('✓ started the team and sent').click();
  await expect(section.getByRole('list', { name: 'Trigger 1: latest events' }).getByText('VIB-HIGH')).toBeVisible();
  await section.screenshot({ path: `${SHOTS}/settings-${info.project.name}.png` });
  await request.post(`/api/teams/${draft.id}/stop`, { data: { request_id: `stop-${name}` } });
});
