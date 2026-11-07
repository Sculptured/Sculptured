import { expect, test } from '@playwright/test';
import { liveOnly, liveTeam, OK_JOB, TURN_MS, turnsEnded } from './fixtures/live';

/**
 * A browser allows only six connections per server over HTTP/1.1. With one live stream per
 * team, seven started teams used them all and the page hung. Every team now shares one
 * stream, so the page still answers with seven teams running. Live (see fixtures/live.ts):
 * seven one-agent teams of real Claude Code on haiku, each with a one-turn job.
 */

liveOnly();

test('seven started teams share one live connection, and an agent still opens', async ({ page }, info) => {
  test.setTimeout(3 * TURN_MS);
  const stamp = `${info.project.name}-${Date.now()}`;
  const ids: string[] = [];
  let lastAgent = '';
  for (let i = 1; i <= 7; i++) {
    const { id, draft } = await liveTeam(page.request, `Many ${i} ${stamp}`, (d, folder) => {
      for (const a of d.agents) Object.assign(a, { job: OK_JOB, folder });
    }, 'solo');
    const started = await page.request.post(`/api/teams/${id}/start`, {
      data: { request_id: `many-start-${stamp}-${i}`, revision: draft.revision },
    });
    expect(started.ok(), await started.text()).toBe(true);
    ids.push(id);
    lastAgent = draft.agents[0].id;
  }

  const streams: string[] = [];
  page.on('request', (r) => {
    if (r.url().includes('/api/stream')) streams.push(r.url());
  });
  const last = ids[ids.length - 1];
  await page.goto(`/rig/${last}`);
  const agent = page.getByRole('button', { name: /^Claude — / });
  await expect(agent).toBeVisible({ timeout: 60_000 });
  await agent.click();
  await expect(page).toHaveURL(new RegExp(`/p/${last}/agents/`));
  // A solo agent is outside any group, so it is in the `team` pod.
  const seatId = `team.${lastAgent}@${last}`;
  await expect.poll(() => turnsEnded(page.request, seatId), { timeout: TURN_MS }).toBeGreaterThan(0);
  const turns = await turnsEnded(page.request, seatId);
  await page.getByRole('textbox', { name: 'Message' }).fill('Reply with the single word STILL and nothing else.');
  await page.getByRole('button', { name: 'Send' }).click();
  await expect(page.getByRole('list', { name: 'Conversation' }).getByText('Reply with the single word STILL and nothing else.')).toBeVisible();
  // The agent still answers over the shared connection: its next turn ends.
  await expect.poll(() => turnsEnded(page.request, seatId), { timeout: TURN_MS }).toBeGreaterThan(turns);

  // Every team's agents are listed, live, over the one connection.
  await page.goto('/');
  // The Projects home lists every team as a row that opens its board.
  for (const id of ids) await expect(page.locator(`a[href="/p/${id}/board"]`).first()).toBeAttached();
  expect(streams.length).toBeGreaterThan(0);
  expect(streams.every((u) => u.endsWith('/api/stream?all=true')), streams.join('\n')).toBe(true);

  // Leave nothing running for the other tests.
  for (const [i, id] of ids.entries()) {
    const stopped = await page.request.post(`/api/teams/${id}/stop`, { data: { request_id: `many-stop-${stamp}-${i}` } });
    expect(stopped.ok(), await stopped.text()).toBe(true);
  }
});
