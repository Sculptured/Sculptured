import { expect, test, type APIRequestContext, type Page } from '@playwright/test';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

/**
 * Community on this computer, against the stub Sculptured server (`fake-community`, started by
 * crates/e2e-server on RIG_E2E_COMMUNITY_PORT). No test reaches a real server.
 * Set SHOTS=<dir> to also save a screenshot at the end of each test.
 */

const STUB = `http://127.0.0.1:${process.env.RIG_E2E_COMMUNITY_PORT ?? '14411'}`;
const SHOTS = process.env.SHOTS;
const AVATAR = path.join(path.dirname(fileURLToPath(import.meta.url)), 'fixtures', 'avatar.png');

test.beforeEach(async ({ request }) => {
  await request.post(`${STUB}/stub/reset`);
});

test.afterEach(async ({ page, request }, info) => {
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/community-${info.project.name}-${info.title.replace(/[^a-z0-9]+/gi, '-').toLowerCase()}.png`, fullPage: true });
  await request.delete('/api/community-link');
});

async function turnOnAndBrowse(page: Page) {
  await page.goto('/community');
  await page.getByRole('button', { name: 'Turn on and browse' }).click();
  await expect(page.getByRole('searchbox', { name: 'Search blocks' })).toBeVisible();
}

/** Turn on and sign in, approving the code on the stub server as its owner would. */
async function signIn(page: Page, request: APIRequestContext) {
  await page.goto('/community');
  await page.getByRole('button', { name: 'Turn on and sign in' }).click();
  await expect(page.getByText('ABCD-EFGH')).toBeVisible();
  await expect(page.getByRole('link', { name: 'Open the sign-in page' })).toHaveAttribute('href', `${STUB}/connect?code=ABCD-EFGH`);
  await request.post(`${STUB}/stub/pairing`, { data: { mode: 'approve' } });
  await expect(page.getByText('Signed in as You Example')).toBeVisible({ timeout: 15_000 });
}

test('Community stays off and silent until turned on', async ({ page, request }) => {
  const seen: string[] = [];
  page.on('request', (r) => seen.push(new URL(r.url()).pathname));
  await page.goto('/community');
  await expect(page.getByRole('heading', { name: 'Turn on Community' })).toBeVisible();
  await expect(page.getByText('Your projects, files, transcripts and runs.')).toBeVisible();
  await page.goto('/');
  await page.goto('/community/learn');
  expect(seen.filter((p) => p.startsWith('/api/community/'))).toEqual([]);
  expect(await (await request.get(`${STUB}/stub/requests`)).json()).toEqual([]);
});

test('turn on and browse, then add a block to a new project in two clicks', async ({ page }) => {
  await turnOnAndBrowse(page);
  const card = page.locator('article.cm-card', { hasText: 'Review pair' });
  await expect(card.getByText('Ana Lee')).toBeVisible();
  await card.getByRole('link', { name: 'Review pair' }).click();
  await expect(page.getByRole('heading', { name: 'Review pair' })).toBeVisible();
  await page.getByRole('button', { name: 'Add to project' }).click();
  await page.getByRole('button', { name: 'Add & open project' }).click();
  await expect(page).toHaveURL(/\/p\/[^/]+\/board/);
  await expect(page.getByText('Writer').first()).toBeVisible();
  await expect(page.getByText('Reviewer').first()).toBeVisible();
});

test('sign in with a code, edit the profile, meet a taken handle, upload a picture', async ({ page, request }) => {
  await signIn(page, request);
  await page.goto('/community/me');
  await expect(page.getByRole('heading', { name: 'Your profile' })).toBeVisible();
  await page.getByLabel('Bio').fill('I review drafts.');
  await page.getByLabel('Handle').fill('taken');
  await page.getByRole('button', { name: 'Save profile' }).click();
  await expect(page.getByText('Someone already has that handle. Choose another.')).toBeVisible();
  await expect(page.getByLabel('Bio')).toHaveValue('I review drafts.');
  await page.getByLabel('Handle').fill('you-example');
  await page.getByRole('button', { name: 'Save profile' }).click();
  await expect(page.getByRole('link', { name: 'See how others see it' })).toBeVisible();
  await page.getByLabel('Choose a picture').setInputFiles(AVATAR);
  await page.getByRole('button', { name: 'Use this picture' }).click();
  await expect(page.locator('img[src^="/api/community/users/you-example/avatar"]').first()).toBeVisible();
});

test('save to Library, then update to the next version', async ({ page, request }) => {
  // The Library outlives one run of this file (desktop, then mobile): start past any saved copy.
  const library = (await (await request.get('/api/group-blocks')).json()) as { source?: { community_block: string; version: number } }[];
  const had = library.find((b) => b.source?.community_block === 'b-review-pair')?.source?.version ?? 0;
  for (let i = 0; i < had; i++) await request.post(`${STUB}/stub/blocks/b-review-pair/bump`);
  await turnOnAndBrowse(page);
  await page.goto('/community/blocks/b-review-pair');
  await page.getByRole('button', { name: had ? `Update to v${had + 1}` : 'Save to Library' }).click();
  await expect(page.getByRole('button', { name: 'In your Library' })).toBeVisible();
  await request.post(`${STUB}/stub/blocks/b-review-pair/bump`);
  await page.reload();
  await page.getByRole('button', { name: `Update to v${had + 2}` }).click();
  await expect(page.getByRole('button', { name: 'In your Library' })).toBeVisible();
  await page.goto('/library/groups');
  await expect(page.getByRole('link', { name: `From Community · v${had + 2}` })).toHaveCount(1);
});

test('publish a group from the Library after choosing a handle', async ({ page, request }) => {
  const id = `e2e-publish-${Date.now()}`;
  const block = {
    id, name: 'Draft checker', revision: 0, boundary_notes: [],
    draft: {
      id, name: 'Draft checker', revision: 0, connections: [],
      groups: [{ id: 'g', label: 'Draft checker', x: 0, y: 0, w: 300, h: 200 }],
      agents: [{ id: 'checker', group_id: 'g', name: 'Checker', kind: 'practice', job: 'Check the draft.', folder: null, x: 20, y: 20 }],
    },
  };
  expect((await request.put(`/api/group-blocks/${id}`, { data: { request_id: `put-${id}`, block } })).ok()).toBe(true);
  await signIn(page, request);
  await page.goto('/community/publish');
  await expect(page.getByRole('heading', { name: 'Choose your handle' })).toBeVisible();
  await page.getByLabel('Handle').fill('you-example');
  await page.getByRole('button', { name: 'Use this handle' }).click();
  await page.getByRole('radio', { name: /Draft checker/ }).first().check();
  await page.getByLabel('Title').fill('Draft checker');
  await page.getByLabel('Description').fill('One agent that checks a draft.');
  await page.getByRole('checkbox', { name: /I reviewed every prompt/ }).check();
  await page.getByRole('button', { name: 'Publish publicly' }).click();
  await expect(page).toHaveURL(/\/community\/blocks\/b-new-/);
  await expect(page.getByText('You Example').first()).toBeVisible();
});

test('a block of another kind says it opens in a later version', async ({ page }) => {
  await turnOnAndBrowse(page);
  await page.goto('/community/blocks/b-kindex-setup');
  await expect(page.getByText('Opens in a later version.').first()).toBeVisible();
  await expect(page.getByRole('button', { name: 'Add to project' })).toHaveCount(0);
});

test('turn off forgets the sign-in', async ({ page, request }) => {
  await signIn(page, request);
  await page.goto('/account');
  const card = page.getByRole('region', { name: 'Community' });
  await expect(card.getByText('Signed in as You Example')).toBeVisible();
  await card.getByRole('button', { name: 'Turn off Community' }).click();
  await expect(card.getByRole('link', { name: 'Open Community' })).toBeVisible();
  await page.goto('/community');
  await expect(page.getByRole('heading', { name: 'Turn on Community' })).toBeVisible();
  expect(((await (await request.get('/api/community-link')).json()) as { enabled: boolean }).enabled).toBe(false);
  const revoked = ((await (await request.get(`${STUB}/stub/requests`)).json()) as { path_and_query: string }[]).some((r) => r.path_and_query === '/gw/api/device/revoke');
  expect(revoked, 'turning off revokes the key on the server').toBe(true);
});
