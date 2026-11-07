import { expect, type APIRequestContext, type Locator, type Page } from '@playwright/test';
import { claudeAgent, confirmStart, liveTeam, OK_JOB } from './live';

/**
 * The team the Control spec runs (control.spec.ts), live: real Claude Code agents on haiku (see
 * ./live.ts; run with RIG_E2E_LIVE=1). A Planner hands work to a Buyer and an Inspector, with a rule
 * that a purchase order over $5,000 needs a person. Every job is one short turn.
 */

export const AGENTS = ['Planner', 'Buyer', 'Inspector'] as const;

export async function controlTeam(request: APIRequestContext, name: string): Promise<string> {
  const { id } = await liveTeam(request, name, (draft, folder) => {
    draft.agents = [
      claudeAgent('planner', 'Planner', OK_JOB, folder, 60, 160),
      claudeAgent('buyer', 'Buyer', OK_JOB, folder, 400, 40),
      claudeAgent('inspector', 'Inspector', OK_JOB, folder, 400, 300),
    ];
    draft.connections = [
      { id: 'c1', from: 'planner', to: 'buyer', kind: 'hands-work-to' },
      { id: 'c2', from: 'planner', to: 'inspector', kind: 'hands-work-to' },
    ];
    draft.mandate = { approve_within: true, rules: [{ id: 'limit', action: 'create_purchase_order', field: 'amount', test: { kind: 'at-most', value: 5000 } }] };
  });
  return id;
}

export async function centre(l: Locator) {
  const b = (await l.boundingBox())!;
  return { x: b.x + b.width / 2, y: b.y + b.height / 2 };
}

/** Drags with the mouse, past the workspace's 4 px threshold, in steps a viewer can follow. */
export async function drag(page: Page, from: { x: number; y: number }, to: { x: number; y: number }, steps = 14) {
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  await page.mouse.move(from.x + 8, from.y + 8, { steps: 2 });
  await page.mouse.move(to.x, to.y, { steps });
  await page.mouse.up();
}

/** A stack (a group of tabbed panes) by one of its pane ids. */
export const stackOf = (page: Page, pane: string) => page.locator('section.ws-stack', { has: page.locator(`[data-tab="${pane}"]`) });

/** Started from Control's own Start; waits for every agent to be on the board. */
export async function startFromControl(page: Page) {
  await page.getByRole('group', { name: 'Team controls' }).getByRole('button', { name: 'Start' }).click();
  await confirmStart(page);
  const overview = stackOf(page, 'live-overview');
  for (const a of AGENTS) await expect(overview.getByRole('button', { name: new RegExp(`^${a} — `) })).toBeVisible({ timeout: 60_000 });
}
