import { expect, test } from '@playwright/test';

/**
 * Codex agents in the real app: a two-agent Codex team, built through the API, started from the
 * Structure board after the "Start 2 Codex agents?" confirmation. The e2e server runs Codex agents
 * on fake-codex, so no model is called. Screenshots of the real app land in web/test-results/screens/codex/.
 */
const SHOTS = 'test-results/screens/codex';

type Event = { seq: number; actor: { kind: string; id: string }; body: { type: string; text?: string } };

test('desktop: two Codex agents start, run and hand work over', async ({ page, request }, info) => {
  test.skip(info.project.name !== 'desktop', 'desktop flow');
  const stamp = Date.now();
  const home = (await (await request.get('/api/folders')).json()).home as string;
  const made = await request.post('/api/folders', { data: { request_id: `mk-${stamp}`, parent: home, name: `codex-${stamp}` } });
  expect(made.ok()).toBe(true);
  const folder = (await made.json()).path as string;

  const created = await request.post('/api/teams', { data: { request_id: `create-${stamp}`, template: 'blank', practice: false, name: `Codex pair ${stamp}` } });
  expect(created.ok()).toBe(true);
  const draft = (await created.json()).draft;
  draft.agents = [
    { id: 'lead', group_id: null, name: 'Lead', kind: 'codex', job: 'Plan the page.', folder, x: 40, y: 80 },
    { id: 'builder', group_id: null, name: 'Builder', kind: 'codex', job: 'Build what the lead hands you.', folder, x: 380, y: 80 },
  ];
  draft.connections = [{ id: 'c1', from: 'lead', to: 'builder', kind: 'hands-work-to' }];
  const saved = await request.put(`/api/teams/${draft.id}`, { data: { request_id: `put-${stamp}`, revision: draft.revision, draft } });
  expect(saved.ok()).toBe(true);

  await page.goto(`/team/${draft.id}/structure`);
  await expect(page.getByRole('region', { name: 'Design board' })).toBeVisible();
  await page.getByRole('button', { name: 'Start team' }).click();
  const dialog = page.getByRole('alertdialog');
  await expect(dialog).toContainText('Start 2 Codex agents?');
  await page.screenshot({ path: `${SHOTS}/codex-start-confirm.png` });
  await dialog.getByRole('button', { name: 'Start team' }).click();
  await expect(page).toHaveURL(new RegExp(`/p/${draft.id}/board\\?mode=run$`));

  const builder = `team.builder@${draft.id}`;
  await expect
    .poll(
      async () => {
        const r = await request.get(`/api/seats/${encodeURIComponent(builder)}/events?after=0&limit=2000`);
        const events: Event[] = await r.json();
        return events.some(
          (e) =>
            e.body.type === 'message-queued' &&
            e.actor.kind === 'system' &&
            e.actor.id === 'handoff' &&
            (e.body.text ?? '').includes("--- Lead's result ---\ncodex received: Plan the page."),
        );
      },
      { timeout: 30_000 },
    )
    .toBe(true);
  await page.screenshot({ path: `${SHOTS}/codex-live.png`, fullPage: true });

  await page.goto(`/rig/${draft.id}/agent/${encodeURIComponent(builder)}`);
  await expect(page.getByText('Handed over by Lead').first()).toBeVisible();
  await expect(page.getByText(/codex received: Handed over by Lead/).first()).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/codex-builder-transcript.png`, fullPage: true });

  const stopped = await request.post(`/api/teams/${draft.id}/stop`, { data: { request_id: `stop-${stamp}` } });
  expect(stopped.ok()).toBe(true);
});
