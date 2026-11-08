import { expect, test } from '@playwright/test';
import { liveOnly, liveTeam, OK_JOB, seatEvents, TURN_MS, turnsEnded } from './fixtures/live';

/**
 * Codex agents in the real app, live (see fixtures/live.ts): a two-agent team on the real `codex`
 * CLI with its cheapest model, built through the API and started from the Structure board after the
 * "Start 2 Codex agents?" confirmation. The lead's turn ends and its result is handed to the
 * builder. Replies vary; the test checks what Sculptured records and delivers.
 * Screenshots of the real app land in web/test-results/screens/codex/.
 */

liveOnly();

const SHOTS = 'test-results/screens/codex';

/** The cheapest Codex model (crates/model/src/pricing.rs). */
const CODEX_MODEL = 'gpt-6-luna';

test('desktop: two Codex agents start, run and hand work over', async ({ page, request }, info) => {
  test.skip(info.project.name !== 'desktop', 'desktop flow');
  test.setTimeout(900_000);
  const { id } = await liveTeam(request, `Codex pair ${Date.now()}`, (draft, folder) => {
    draft.models = { ...draft.models, codex: CODEX_MODEL };
    draft.agents = [
      { id: 'lead', group_id: null, name: 'Lead', kind: 'codex', job: OK_JOB, folder, x: 40, y: 80 },
      { id: 'builder', group_id: null, name: 'Builder', kind: 'codex', job: OK_JOB, folder, x: 380, y: 80 },
    ];
    draft.connections = [{ id: 'c1', from: 'lead', to: 'builder', kind: 'hands-work-to' }];
  });

  await page.goto(`/team/${id}/structure`);
  await expect(page.getByRole('region', { name: 'Design board' })).toBeVisible();
  await page.getByRole('button', { name: 'Start team' }).click();
  const dialog = page.getByRole('alertdialog');
  await expect(dialog).toContainText('Start 2 Codex agents?');
  await page.screenshot({ path: `${SHOTS}/codex-start-confirm.png` });
  await dialog.getByRole('button', { name: 'Start team' }).click();
  await expect(page).toHaveURL(new RegExp(`/p/${id}/board\\?mode=run$`));

  // The lead's result reaches the builder as a handoff, whatever the lead replied.
  const builder = `team.builder@${id}`;
  const handoffs = async () =>
    (await seatEvents(request, builder)).filter(
      (e) => e.body.type === 'message-queued' && e.actor.kind === 'system' && e.actor.id === 'handoff',
    );
  await expect.poll(async () => (await handoffs()).length, { timeout: 2 * TURN_MS }).toBeGreaterThan(0);
  expect((await handoffs())[0].body.text ?? '').toMatch(/^Handed over by Lead\n\n[\s\S]*--- Lead's result ---\n/);
  await expect.poll(() => turnsEnded(request, builder), { timeout: TURN_MS }).toBeGreaterThan(0);
  await page.screenshot({ path: `${SHOTS}/codex-live.png`, fullPage: true });

  await page.goto(`/rig/${id}/agent/${encodeURIComponent(builder)}`);
  await expect(page.getByText('Handed over by Lead').first()).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/codex-builder-transcript.png`, fullPage: true });

  const stopped = await request.post(`/api/teams/${id}/stop`, { data: { request_id: `stop-${id}` } });
  expect(stopped.ok()).toBe(true);
});
