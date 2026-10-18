import { expect, test, type APIRequestContext, type Page } from '@playwright/test';

/**
 * Real handoffs, against the real binary, Practice agents only: a chain of three agents
 * (Planner → Builder → Reviewer, all "hands work to") runs in order after Start, each
 * downstream agent waiting, with a clock, until its upstream finishes.
 * Screenshots of the real app land in web/test-results/screens/handoff/.
 */

const SHOTS = 'test-results/screens/handoff';

type Event = { seq: number; actor: { kind: string; id: string }; body: { type: string; text?: string } };

/** A Practice chain team built through the API, so the test is about running it. */
async function chainTeam(request: APIRequestContext, name: string, plannerJob: string): Promise<string> {
  const created = await request.post('/api/teams', { data: { request_id: `create-${name}`, template: 'blank', practice: true, name } });
  expect(created.ok()).toBe(true);
  const draft = (await created.json()).draft;
  const agents: [string, string, string][] = [
    ['planner', 'Planner', plannerJob],
    ['builder', 'Builder', 'Build what the planner hands you.'],
    ['reviewer', 'Reviewer', 'Review what the builder hands you.'],
  ];
  draft.agents = agents.map(([id, agentName, job], i) => ({ id, group_id: null, name: agentName, kind: 'practice', job, folder: null, x: 40 + 320 * i, y: 80 }));
  draft.connections = [
    { id: 'c1', from: 'planner', to: 'builder', kind: 'hands-work-to' },
    { id: 'c2', from: 'builder', to: 'reviewer', kind: 'hands-work-to' },
  ];
  const saved = await request.put(`/api/teams/${draft.id}`, { data: { request_id: `put-${name}`, revision: draft.revision, draft } });
  expect(saved.ok()).toBe(true);
  return draft.id;
}

const seat = (team: string, agent: string) => `team.${agent}@${team}`;

async function events(request: APIRequestContext, seatId: string): Promise<Event[]> {
  const r = await request.get(`/api/seats/${encodeURIComponent(seatId)}/events?after=0&limit=2000`);
  expect(r.ok()).toBe(true);
  return r.json();
}

/** Seqs of the messages queued to an agent by a system actor (`team-start` or `handoff`). */
async function queuedBy(request: APIRequestContext, seatId: string, actor: string): Promise<{ seq: number; text: string }[]> {
  return (await events(request, seatId))
    .filter((e) => e.body.type === 'message-queued' && e.actor.kind === 'system' && e.actor.id === actor)
    .map((e) => ({ seq: e.seq, text: e.body.text ?? '' }));
}

async function startFromStructure(page: Page, id: string) {
  await page.goto(`/team/${id}/structure`);
  await expect(page.getByRole('region', { name: 'Structure board' })).toBeVisible();
  await page.getByRole('button', { name: 'Start team' }).click();
  await expect(page).toHaveURL(new RegExp(`/p/${id}/board\\?mode=run$`));
}

test('desktop: a Practice chain of three runs in order after Start', async ({ page, request }, info) => {
  test.skip(info.project.name !== 'desktop', 'desktop flow');
  const id = await chainTeam(request, `Chain ${Date.now()}`, 'Plan the login page.');

  // Structure: the connection says what it does at run time.
  await page.goto(`/team/${id}/structure`);
  await page.locator('[data-connection="c1"]').hover();
  await expect(page.locator('.tooltip')).toHaveText("Builder starts when Planner finishes; Planner's result is passed along");
  await page.screenshot({ path: `${SHOTS}/handoff-1-structure-tooltip.png` });

  await startFromStructure(page, id);
  const [planner, builder, reviewer] = ['planner', 'builder', 'reviewer'].map((a) => seat(id, a));
  // The whole chain runs: the reviewer gets the builder's result.
  await expect.poll(async () => (await queuedBy(request, reviewer, 'handoff')).length, { timeout: 20_000 }).toBe(1);
  const jobs = await queuedBy(request, planner, 'team-start');
  const toBuilder = await queuedBy(request, builder, 'handoff');
  const toReviewer = await queuedBy(request, reviewer, 'handoff');
  expect(jobs).toHaveLength(1);
  expect(await queuedBy(request, builder, 'team-start')).toHaveLength(0);
  expect(await queuedBy(request, reviewer, 'team-start')).toHaveLength(0);
  expect(toBuilder).toHaveLength(1);
  expect(jobs[0].seq).toBeLessThan(toBuilder[0].seq);
  expect(toBuilder[0].seq).toBeLessThan(toReviewer[0].seq);
  expect(toBuilder[0].text).toMatch(/^Handed over by Planner\n\nBuild what the planner hands you\.\n\n--- Planner's result ---\nstub received: Plan the login page\./);
  expect(toReviewer[0].text).toMatch(/^Handed over by Builder\n\nReview what the builder hands you\.\n\n--- Builder's result ---\n/);

  // Live: nobody waits any more.
  await expect(page.getByRole('button', { name: /^Reviewer — (idle|done)$/ })).toBeVisible();
  await expect(page.locator('.agent-box .waiting-mark')).toHaveCount(0);
  await page.screenshot({ path: `${SHOTS}/handoff-2-live-chain-done.png` });

  // The reviewer's transcript holds the handoff as evidence.
  await page.getByRole('button', { name: /^Reviewer — / }).click();
  await expect(page.getByRole('list', { name: 'Conversation' })).toContainText('Builder');
  await page.screenshot({ path: `${SHOTS}/handoff-3-reviewer-transcript.png` });

  // Talking to the planner hands nothing over; "Hand over again" in its pane does, once.
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: /^Planner — / }).click();
  await page.getByRole('textbox', { name: 'Message' }).fill('Add a forgot-password link.');
  await page.getByRole('button', { name: 'Send' }).click();
  await expect(page.getByRole('list', { name: 'Conversation' })).toContainText('stub received: Add a forgot-password link.');
  await page.waitForTimeout(500);
  expect(await queuedBy(request, builder, 'handoff')).toHaveLength(1);
  // Hand over again sits under Details (and in Ctrl K).
  await page.getByRole('button', { name: 'Details & controls' }).click();
  await page.getByRole('button', { name: 'Hand over again' }).click();
  await expect.poll(async () => (await queuedBy(request, reviewer, 'handoff')).length, { timeout: 20_000 }).toBe(2);
  const more = await queuedBy(request, builder, 'handoff');
  expect(more).toHaveLength(2);
  expect(more[1].text).toMatch(/^More work from Planner\n\n--- Planner's result ---\nstub received: Add a forgot-password link\./);
  await page.screenshot({ path: `${SHOTS}/handoff-6-hand-over-again.png` });

  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: 'Stop team…' }).click();
  await page.getByRole('button', { name: 'Confirm stop team' }).click();
  await expect(page.getByRole('link', { name: 'Reviewer — stopped' })).toBeVisible({ timeout: 20_000 });
});

test("desktop: downstream agents wait with a clock; a jobless first agent's first message is its work", async ({ page, request }, info) => {
  test.skip(info.project.name !== 'desktop', 'desktop flow');
  // The planner has no Job: nothing runs until you write to it.
  const id = await chainTeam(request, `Waiting ${Date.now()}`, '');
  await startFromStructure(page, id);
  const [planner, builder, reviewer] = ['planner', 'builder', 'reviewer'].map((a) => seat(id, a));

  await expect(page.getByRole('button', { name: 'Builder — idle, waiting for Planner' })).toBeVisible({ timeout: 20_000 });
  await expect(page.getByRole('button', { name: 'Reviewer — idle, waiting for Builder' })).toBeVisible();
  await expect(page.getByRole('link', { name: 'Builder — waiting for Planner' })).toContainText('◷');
  await expect(page.locator('.agent-box .stuck-mark')).toHaveCount(0);
  await page.screenshot({ path: `${SHOTS}/handoff-4-live-waiting.png` });

  // Your first message to the planner is its work: with the graph on screen, it moves down the chain.
  const sent = await request.post(`/api/seats/${encodeURIComponent(planner)}/send`, {
    data: { request_id: `send-${id}`, text: 'Plan the settings page.', intent: 'instruct' },
  });
  expect(sent.ok()).toBe(true);
  await expect(page.locator(`[data-testid="edge-${planner}-${builder}"]`)).toHaveAttribute('data-pulse', 'handoff');
  await page.screenshot({ path: `${SHOTS}/handoff-5-live-pulse.png` });
  await expect.poll(async () => (await queuedBy(request, reviewer, 'handoff')).length, { timeout: 20_000 }).toBe(1);
  await expect(page.locator('.agent-box .waiting-mark, .agent-box .stuck-mark')).toHaveCount(0);
  await expect(page.locator(`[data-testid="edge-${planner}-${builder}"]`)).not.toHaveAttribute('data-pulse', 'handoff', { timeout: 5_000 });
  const toBuilder = await queuedBy(request, builder, 'handoff');
  const toReviewer = await queuedBy(request, reviewer, 'handoff');
  expect(toBuilder[0].text).toMatch(/--- Planner's result ---\nstub received: Plan the settings page\./);
  expect(toBuilder[0].seq).toBeLessThan(toReviewer[0].seq);

  // A second message is conversation: nothing more moves.
  await request.post(`/api/seats/${encodeURIComponent(planner)}/send`, { data: { request_id: `send2-${id}`, text: 'Thanks.', intent: 'instruct' } });
  await page.waitForTimeout(800);
  expect(await queuedBy(request, builder, 'handoff')).toHaveLength(1);

  await page.getByRole('button', { name: 'Stop team…' }).click();
  await page.getByRole('button', { name: 'Confirm stop team' }).click();
  await expect(page.getByRole('link', { name: 'Reviewer — stopped' })).toBeVisible({ timeout: 20_000 });
});
