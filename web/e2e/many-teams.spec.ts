import { expect, test } from '@playwright/test';

/**
 * A browser allows only six connections per server over HTTP/1.1. With one live stream per
 * team, seven started teams used them all and the page hung. Every team now shares one
 * stream, so the page still answers with seven teams running.
 */
test('seven started teams share one live connection, and an agent still opens', async ({ page }, info) => {
  const stamp = `${info.project.name}-${Date.now()}`;
  const ids: string[] = [];
  for (let i = 1; i <= 7; i++) {
    const created = await page.request.post('/api/teams', {
      data: { request_id: `many-create-${stamp}-${i}`, template: 'solo', practice: true, name: `Many ${i} ${stamp}` },
    });
    expect(created.ok(), await created.text()).toBe(true);
    const rec = await created.json();
    const started = await page.request.post(`/api/teams/${rec.draft.id}/start`, {
      data: { request_id: `many-start-${stamp}-${i}`, revision: rec.draft.revision },
    });
    expect(started.ok(), await started.text()).toBe(true);
    ids.push(rec.draft.id);
  }

  const streams: string[] = [];
  page.on('request', (r) => {
    if (r.url().includes('/api/stream')) streams.push(r.url());
  });
  const last = ids[ids.length - 1];
  await page.goto(`/rig/${last}`);
  const agent = page.getByRole('button', { name: /^Claude — / });
  await expect(agent).toBeVisible();
  await agent.click();
  await expect(page).toHaveURL(new RegExp(`/p/${last}/agents/`));
  await page.getByRole('textbox', { name: 'Message' }).fill('still answering');
  await page.getByRole('button', { name: 'Send' }).click();
  await expect(page.getByRole('list', { name: 'Conversation' }).getByText('stub received: still answering')).toBeVisible();

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
