import { expect, test, type Page } from '@playwright/test';
import { BUILDER, mockApi, RIG } from './fixtures/layout-api';
import { DRAFT_ONLY, mockProgress } from './fixtures/progress-api';

/**
 * LAYOUT ONLY, MOCKED API. Long runtime text (an unbroken 150-character path, a long URL, a
 * 600-character message, a long reply with an "all tests pass" claim, a long attention reason)
 * must never push content past its panel, scroll the page sideways or grow a card. The API is
 * answered by Playwright route fixtures (`fixtures/layout-api.ts`), so this pins the browser
 * layout only; the real-binary specs cover the server. Run: `npm run e2e:layout`.
 *
 * Set LAYOUT_SHOTS=<dir> to also save a screenshot of each view.
 */

const SHOTS = process.env.LAYOUT_SHOTS;
const WIDTHS = [
  { w: 1440, h: 900 },
  { w: 900, h: 900 },
  { w: 390, h: 844 },
];
/** Appearance text sizes: the smallest and the largest the app offers. */
const SIZES = [12, 16];
const CARD = 200;

async function setup(page: Page, w: number, h: number, size: number, mock = mockApi) {
  await page.setViewportSize({ width: w, height: h });
  await page.addInitScript((s) => {
    localStorage.setItem('sculptured.appearance.v1', JSON.stringify({ font: 'system', size: s, glass: true }));
  }, size);
  await mock(page);
}

async function shot(page: Page, name: string) {
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/${name}.png` });
}

async function noPageScrollX(page: Page) {
  const over = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(over, 'page scrolls sideways').toBeLessThanOrEqual(0);
}

async function noScrollX(page: Page, sel: string) {
  const over = await page.locator(sel).first().evaluate((el) => el.scrollWidth - el.clientWidth);
  expect(over, `${sel} scrolls sideways`).toBeLessThanOrEqual(0);
}

/**
 * Every element under each `scope` element stays inside its right edge, unless an element
 * between them clips or scrolls it (overflow other than visible) and itself stays inside.
 */
async function contained(page: Page, scope: string) {
  const bad = await page.locator(scope).evaluateAll((roots) =>
    roots.flatMap((root) => {
      const edge = root.getBoundingClientRect().right + 1;
      const clippedInside = (el: Element) => {
        for (let a = el.parentElement; a && a !== root; a = a.parentElement)
          if (getComputedStyle(a).overflowX !== 'visible' && a.getBoundingClientRect().right <= edge) return true;
        return false;
      };
      return [...root.querySelectorAll('*')]
        .filter((el) => el.getBoundingClientRect().right > edge && !clippedInside(el))
        .map((el) => `${el.tagName.toLowerCase()}.${[...el.classList].join('.')} +${Math.round(el.getBoundingClientRect().right - edge)}px`);
    }),
  );
  expect(bad.slice(0, 5), `content past the edge of ${scope}`).toEqual([]);
}

/** Lines of text each matching element shows: its unscaled inner height less padding, over its line height. */
function lines(page: Page, sel: string) {
  return page.locator(sel).evaluateAll((els) =>
    els.map((el) => {
      const s = getComputedStyle(el);
      const text = el.clientHeight - parseFloat(s.paddingTop) - parseFloat(s.paddingBottom);
      return Math.round(text / (parseFloat(s.lineHeight) || 16));
    }),
  );
}

/** Size, inner overflow and reply position of every Live card. */
function liveCards(page: Page) {
  return page.locator('.agent-box').evaluateAll((els) =>
    (els as HTMLElement[]).map((el) => {
      const main = el.querySelector<HTMLElement>('.box-main')!;
      const card = el.getBoundingClientRect();
      const last = el.querySelector('.box-last')!.getBoundingClientRect();
      return {
        w: el.offsetWidth,
        h: el.offsetHeight,
        innerOverflow: main.scrollHeight - main.clientHeight,
        innerScroll: getComputedStyle(main).overflowY,
        lastInside: last.top >= card.top - 1 && last.bottom <= card.bottom + 1 && last.right <= card.right + 1,
      };
    }),
  );
}

async function openLive(page: Page) {
  await page.goto(`/p/${RIG}/board?mode=run`);
  await expect(page.locator('.agent-box')).toHaveCount(3);
}

async function checkLive(page: Page) {
  for (const c of await liveCards(page)) {
    expect([c.w, c.h], 'card size').toEqual([CARD, CARD]);
    expect(c.innerOverflow, 'card content overflows the card').toBeLessThanOrEqual(1);
    expect(c.innerScroll, 'card scrolls inside').not.toMatch(/auto|scroll/);
    expect(c.lastInside, 'last reply leaves its card').toBe(true);
  }
  for (const n of await lines(page, '.agent-box .box-last')) expect(n).toBeLessThanOrEqual(2);
  await noPageScrollX(page);
}

async function checkVisual(page: Page, w: number, size: number) {
  await noScrollX(page, '.pane-body');
  await noScrollX(page, '.vp');
  for (const scope of ['.vp-panel', '.vp-card', '.vp-claim']) await contained(page, scope);
  await noPageScrollX(page);
  for (const n of await lines(page, '.vp-trigger-text, .vp-reply-text')) expect(n, 'turn text clamped to two lines').toBeLessThanOrEqual(2);
  if (w < 1280) return; // narrower, the card head wraps; the clamps above still hold
  // A turn card's height, less its tool-call nodes (one row per ~20 calls), stays small however long its text.
  const heights = await page.locator('.vp-card').evaluateAll((els) =>
    els.map((e) => e.getBoundingClientRect().height - (e.querySelector('.vp-nodes-wrap')?.getBoundingClientRect().height ?? 0)),
  );
  for (const ch of heights) expect(ch, 'turn card height without its nodes').toBeLessThan(220 + (size - 12) * 16);
}

/** "Show more" opens the whole trigger and reply in the card, still inside the panel. */
async function expandTurn(page: Page) {
  const card = page.locator('.vp-phase').first();
  await card.getByRole('button', { name: 'Show more' }).click();
  await expect(card.getByRole('button', { name: 'Show less' })).toBeVisible();
  expect(Math.max(...(await lines(page, '.vp-phase:first-child .vp-reply-text')))).toBeGreaterThan(2);
  await contained(page, '.vp-card');
  await noScrollX(page, '.vp');
}

/** The long job starts folded; "Show more" opens it in place. */
async function unfoldFirst(page: Page) {
  const fold = page.locator('.fold').first();
  await expect(fold).toHaveAttribute('data-open', 'false');
  const before = await fold.evaluate((el) => el.getBoundingClientRect().height);
  await fold.getByRole('button', { name: 'Show more' }).click();
  await expect(fold).toHaveAttribute('data-open', 'true');
  expect(await fold.evaluate((el) => el.getBoundingClientRect().height)).toBeGreaterThan(before);
  await contained(page, '.cv-column');
}

for (const { w, h } of WIDTHS) {
  for (const size of SIZES) {
    test.describe(`${w}px, text ${size}px`, () => {
      test.beforeEach(async ({ page }) => setup(page, w, h, size));

      test('Live cards keep their size and clamp the reply', async ({ page }) => {
        await openLive(page);
        await shot(page, `fixture-live-${w}-${size}`);
        await checkLive(page);
      });

      test('Visual progress keeps every card inside its panel', async ({ page }) => {
        await page.goto(`/rig/${RIG}/agent/${encodeURIComponent(BUILDER)}/visual`);
        await expect(page.locator('.vp-phase')).toHaveCount(2);
        await page.locator('.vp-process').scrollIntoViewIfNeeded();
        await shot(page, `fixture-visual-${w}-${size}`);
        await page.locator('.vp-claims').scrollIntoViewIfNeeded();
        await shot(page, `fixture-visual-claims-${w}-${size}`);
        await checkVisual(page, w, size);
        await expandTurn(page);
      });

      test('Transcript wraps long tokens and folds long text', async ({ page }) => {
        await page.goto(`/rig/${RIG}/agent/${encodeURIComponent(BUILDER)}`);
        await expect(page.locator('.reply-text').first()).toBeVisible();
        await shot(page, `fixture-transcript-${w}-${size}`);
        await page.getByTestId('transcript-scroll').evaluate((el) => (el.scrollTop = 0));
        await shot(page, `fixture-transcript-top-${w}-${size}`);
        await noScrollX(page, '.transcript-scroll');
        await contained(page, '.cv-column');
        await noPageScrollX(page);
        await unfoldFirst(page);
      });
    });
  }
}


test.describe('Project tab row', () => {
  test('a team not started yet keeps its name, tabs and Start on one row', async ({ page }) => {
    await setup(page, 1440, 900, 13, mockProgress);
    await page.goto(`/team/${DRAFT_ONLY}/structure`);
    // Why Run is off is its tooltip and description, not a printed line.
    const run = page.getByRole('group', { name: 'Board mode' }).getByRole('button', { name: /Run/ });
    await expect(run).toHaveAttribute('data-tip', 'Run opens once the team has started');
    await expect(run).toBeDisabled();
    await shot(page, 'fixture-design-not-started');
    const [name, tabs, start] = await Promise.all(['.pb-name', '.pb-tabs', '.start-btn'].map((sel) => page.locator(sel).evaluate((el) => el.getBoundingClientRect())));
    expect(tabs.top, 'tabs wrapped below the project name').toBeLessThan(name.bottom);
    expect(start.top, 'Start wrapped below the tabs').toBeLessThan(tabs.bottom);
  });
});
