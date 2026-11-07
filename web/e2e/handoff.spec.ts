import { expect, test, type APIRequestContext, type Page } from '@playwright/test';
import { claudeAgent, confirmStart, liveOnly, liveTeam, OK_JOB, seatEvents, TURN_MS, turnsEnded } from './fixtures/live';

/**
 * Real handoffs, against the real binary, with real Claude Code agents on haiku (live, see
 * fixtures/live.ts): a chain of three agents (Planner → Builder → Reviewer, all "hands work to")
 * runs in order after Start, each downstream agent waiting, with a clock, until its upstream
 * finishes. Replies vary; the test checks the handoffs Sculptured delivers.
 * Screenshots of the real app land in web/test-results/screens/handoff/.
 */

liveOnly();

const SHOTS = 'test-results/screens/handoff';
const BUILDER_JOB = `Builder: ${OK_JOB}`;
const REVIEWER_JOB = `Reviewer: ${OK_JOB}`;

/** A chain team built through the API, so the test is about running it. */
async function chainTeam(request: APIRequestContext, name: string, plannerJob: string): Promise<string> {
  const { id } = await liveTeam(request, name, (draft, folder) => {
    draft.agents = [
      claudeAgent('planner', 'Planner', plannerJob, folder, 40, 80),
      claudeAgent('builder', 'Builder', BUILDER_JOB, folder, 360, 80),
      claudeAgent('reviewer', 'Reviewer', REVIEWER_JOB, folder, 680, 80),
    ];
    draft.connections = [
      { id: 'c1', from: 'planner', to: 'builder', kind: 'hands-work-to' },
      { id: 'c2', from: 'builder', to: 'reviewer', kind: 'hands-work-to' },
    ];
  });
  return id;
}

const seat = (team: string, agent: string) => `team.${agent}@${team}`;

/** Seqs of the messages queued to an agent by a system actor (`team-start` or `handoff`). */
async function queuedBy(request: APIRequestContext, seatId: string, actor: string): Promise<{ seq: number; text: string }[]> {
  return (await seatEvents(request, seatId))
    .filter((e) => e.body.type === 'message-queued' && e.actor.kind === 'system' && e.actor.id === actor)
    .map((e) => ({ seq: e.seq, text: e.body.text ?? '' }));
}

async function startFromStructure(page: Page, id: string) {
  await page.goto(`/team/${id}/structure`);
  await expect(page.getByRole('region', { name: 'Design board' })).toBeVisible();
  await page.getByRole('button', { name: 'Start team' }).click();
  await confirmStart(page);
  await expect(page).toHaveURL(new RegExp(`/p/${id}/board\\?mode=run$`));
}

test('desktop: a chain of three runs in order after Start', async ({ page, request }, info) => {
  test.skip(info.project.name !== 'desktop', 'desktop flow');
  test.setTimeout(900_000);
  const id = await chainTeam(request, `Chain ${Date.now()}`, OK_JOB);

  // Structure: the connection says what it does at run time.
  await page.goto(`/team/${id}/structure`);
  await page.locator('[data-connection="c1"]').hover();
  await expect(page.locator('.tooltip')).toHaveText("Builder starts when Planner finishes; Planner's result is passed along");
  await page.screenshot({ path: `${SHOTS}/handoff-1-structure-tooltip.png` });

  await startFromStructure(page, id);
  const [planner, builder, reviewer] = ['planner', 'builder', 'reviewer'].map((a) => seat(id, a));
  // The whole chain runs: the reviewer gets the builder's result.
  await expect.poll(async () => (await queuedBy(request, reviewer, 'handoff')).length, { timeout: 2 * TURN_MS }).toBe(1);
  const jobs = await queuedBy(request, planner, 'team-start');
  const toBuilder = await queuedBy(request, builder, 'handoff');
  const toReviewer = await queuedBy(request, reviewer, 'handoff');
  expect(jobs).toHaveLength(1);
  expect(await queuedBy(request, builder, 'team-start')).toHaveLength(0);
  expect(await queuedBy(request, reviewer, 'team-start')).toHaveLength(0);
  expect(toBuilder).toHaveLength(1);
  expect(jobs[0].seq).toBeLessThan(toBuilder[0].seq);
  expect(toBuilder[0].seq).toBeLessThan(toReviewer[0].seq);
  expect(toBuilder[0].text).toMatch(/^Handed over by Planner\n\nBuilder: Reply with the single word OK and nothing else\.\n\n--- Planner's result ---\n/);
  expect(toReviewer[0].text).toMatch(/^Handed over by Builder\n\nReviewer: Reply with the single word OK and nothing else\.\n\n--- Builder's result ---\n/);

  // Live: nobody waits any more.
  await expect(page.getByRole('button', { name: /^Reviewer — (idle|done \(reported\))$/ })).toBeVisible({ timeout: TURN_MS });
  await expect(page.locator('.agent-box .waiting-mark')).toHaveCount(0);
  await page.screenshot({ path: `${SHOTS}/handoff-2-live-chain-done.png` });

  // The reviewer's transcript holds the handoff as evidence.
  await page.getByRole('button', { name: /^Reviewer — / }).click();
  await expect(page.getByRole('list', { name: 'Conversation' })).toContainText('Builder');
  await page.screenshot({ path: `${SHOTS}/handoff-3-reviewer-transcript.png` });

  // Talking to the planner hands nothing over; "Hand over again" in its pane does, once.
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: /^Planner — / }).click();
  const turns = await turnsEnded(request, planner);
  await page.getByRole('textbox', { name: 'Message' }).fill('Reply with the single word DONE and nothing else.');
  await page.getByRole('button', { name: 'Send' }).click();
  await expect(page.getByRole('list', { name: 'Conversation' })).toContainText('Reply with the single word DONE and nothing else.');
  await expect.poll(() => turnsEnded(request, planner), { timeout: TURN_MS }).toBeGreaterThan(turns);
  await page.waitForTimeout(2000);
  expect(await queuedBy(request, builder, 'handoff')).toHaveLength(1);
  // Hand over again sits on the agent's Activity tab (and in Ctrl K); agents on your Claude account ask first.
  await page.getByRole('link', { name: 'Activity' }).click();
  await page.getByRole('button', { name: 'Hand over again' }).click();
  const ask = page.getByRole('alertdialog', { name: /^Re-run 2 agents downstream of Planner/ });
  await expect(ask).toContainText('Claude account');
  await ask.getByRole('button', { name: 'Re-run 2 agents' }).click();
  await expect.poll(async () => (await queuedBy(request, reviewer, 'handoff')).length, { timeout: 2 * TURN_MS }).toBe(2);
  const more = await queuedBy(request, builder, 'handoff');
  expect(more).toHaveLength(2);
  expect(more[1].text).toMatch(/^More work from Planner\n\n--- Planner's result ---\n/);
  await page.screenshot({ path: `${SHOTS}/handoff-6-hand-over-again.png` });

  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: 'Stop team…' }).click();
  await page.getByRole('button', { name: 'Confirm stop team' }).click();
  // Stopped agents leave the Run board; the Agents page lists them.
  await page.getByRole('link', { name: 'Agents' }).click();
  await expect(page.getByRole('row', { name: /\bReviewer\b.*stopped/ })).toBeVisible({ timeout: 20_000 });
});

test("desktop: downstream agents wait with a clock; a jobless first agent's first message is its work", async ({ page, request }, info) => {
  test.skip(info.project.name !== 'desktop', 'desktop flow');
  test.setTimeout(600_000);
  // The planner has no Job: nothing runs until you write to it.
  const id = await chainTeam(request, `Waiting ${Date.now()}`, '');
  await startFromStructure(page, id);
  const [planner, builder, reviewer] = ['planner', 'builder', 'reviewer'].map((a) => seat(id, a));

  await expect(page.getByRole('button', { name: 'Builder — idle, waiting for Planner' })).toBeVisible({ timeout: 60_000 });
  await expect(page.getByRole('button', { name: 'Reviewer — idle, waiting for Builder' })).toBeVisible();
  await expect(page.locator('.agent-box', { has: page.getByRole('button', { name: 'Builder — idle, waiting for Planner' }) }).locator('.waiting-mark')).toContainText('◷');
  await expect(page.locator('.agent-box .stuck-mark')).toHaveCount(0);
  await page.screenshot({ path: `${SHOTS}/handoff-4-live-waiting.png` });

  // Your first message to the planner is its work: with the graph on screen, it moves down the chain.
  const sent = await request.post(`/api/seats/${encodeURIComponent(planner)}/send`, {
    data: { request_id: `send-${id}`, text: OK_JOB, intent: 'instruct' },
  });
  expect(sent.ok()).toBe(true);
  await expect(page.locator(`[data-testid="edge-${planner}-${builder}"]`)).toHaveAttribute('data-pulse', 'handoff', { timeout: TURN_MS });
  await page.screenshot({ path: `${SHOTS}/handoff-5-live-pulse.png` });
  await expect.poll(async () => (await queuedBy(request, reviewer, 'handoff')).length, { timeout: 2 * TURN_MS }).toBe(1);
  await expect(page.locator('.agent-box .waiting-mark, .agent-box .stuck-mark')).toHaveCount(0, { timeout: TURN_MS });
  await expect(page.locator(`[data-testid="edge-${planner}-${builder}"]`)).not.toHaveAttribute('data-pulse', 'handoff', { timeout: 5_000 });
  const toBuilder = await queuedBy(request, builder, 'handoff');
  const toReviewer = await queuedBy(request, reviewer, 'handoff');
  expect(toBuilder[0].text).toMatch(/--- Planner's result ---\n/);
  expect(toBuilder[0].seq).toBeLessThan(toReviewer[0].seq);

  // A second message is conversation: nothing more moves.
  const turns = await turnsEnded(request, planner);
  await request.post(`/api/seats/${encodeURIComponent(planner)}/send`, { data: { request_id: `send2-${id}`, text: 'Reply with the single word THANKS and nothing else.', intent: 'instruct' } });
  await expect.poll(() => turnsEnded(request, planner), { timeout: TURN_MS }).toBeGreaterThan(turns);
  await page.waitForTimeout(2000);
  expect(await queuedBy(request, builder, 'handoff')).toHaveLength(1);

  await page.getByRole('button', { name: 'Stop team…' }).click();
  await page.getByRole('button', { name: 'Confirm stop team' }).click();
  // Stopped agents leave the Run board; the Agents page lists them.
  await page.getByRole('link', { name: 'Agents' }).click();
  await expect(page.getByRole('row', { name: /\bReviewer\b.*stopped/ })).toBeVisible({ timeout: 20_000 });
});
