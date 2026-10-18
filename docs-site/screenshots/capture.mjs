// Captures the docs' screenshots, and the README's board image, from the real, running app: never
// drawn or mocked.
//
//   1. Build and serve the app with a fresh data folder and Practice agents (no model is called):
//        cargo build -p sculptured && npm --prefix web run build
//        sculptured serve --port 4499 --data-dir <empty folder> --claude-program <fixture CLI>
//      The fixture CLI (see README.md here) answers "Improve this team with AI" with one canned team.
//   2. node screenshots/capture.mjs --base http://127.0.0.1:4499 --session-folder <home>/app
//
// Every team, prompt and session in the shots is a made-up demo; nothing personal is shown.
// The app's default look (Coastal) and board style (Cards) are used: each browser here starts fresh.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import puppeteer from 'puppeteer-core';

const arg = (name, fallback) => {
  const i = process.argv.indexOf(name);
  return i > 0 ? process.argv[i + 1] : fallback;
};
// Required: the demo server to drive. There is no default, so the script never touches a Sculptured you use.
const base = arg('--base', null);
if (!base) {
  console.error('usage: node screenshots/capture.mjs --base http://127.0.0.1:<demo port> [--session-folder <dir>] [--only <step>] [--from <step>]');
  process.exit(2);
}
// The demo session ran in this folder (the served app's home folder + /app), so its agents' folders are allowed.
const sessionFolder = arg('--session-folder', '/work/app');
const chrome = arg('--chrome', '/usr/bin/google-chrome');
const images = new URL('../images/', import.meta.url).pathname;
// The README's picture of the board, at the top of the repository's README.
const readmeImages = new URL('../../docs/images/', import.meta.url).pathname;
const fixtures = new URL('../../crates/controld/tests/fixtures/sessions/claude/', import.meta.url).pathname;
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
const rid = () => `docs-${Math.random().toString(36).slice(2)}`;
const DESKTOP = { width: 1280, height: 800 };

async function api(method, url, body, type = 'application/json') {
  const res = await fetch(base + url, { method, headers: { 'content-type': type }, body: type === 'application/json' && body !== undefined ? JSON.stringify(body) : body });
  const json = await res.json().catch(() => null);
  if (!res.ok) throw new Error(`${method} ${url}: ${res.status} ${JSON.stringify(json)}`);
  return json;
}

/** A Practice team from a template, renamed, with these jobs and agent names (by template agent name). */
async function team(template, name, jobs = {}, names = {}) {
  const rec = await api('POST', '/api/teams', { request_id: rid(), template, practice: true, name });
  const draft = { ...rec.draft, agents: rec.draft.agents.map((a) => ({ ...a, job: jobs[a.name] ?? a.job, name: names[a.name] ?? a.name })) };
  return save(draft, rec.draft.revision);
}

async function save(draft, revision) {
  return (await api('PUT', `/api/teams/${draft.id}`, { request_id: rid(), revision, draft })).draft;
}

async function start(draft) {
  await api('POST', `/api/teams/${draft.id}/start`, { request_id: rid(), revision: draft.revision });
}

/** The run's agents: [{ id, name, parent }]. */
async function agents(id) {
  const runs = await api('GET', `/api/teams/${id}/deployments`);
  return (await api('GET', `/api/teams/${id}/deployments/${runs[0].start_seq}`)).agents;
}

const send = (seat, text) => api('POST', `/api/seats/${encodeURIComponent(seat)}/send`, { request_id: rid(), text, intent: 'instruct', interrupt: false });

// The app's own addresses (web/src/paths.ts).
const enc = encodeURIComponent;
const board = (team, mode) => `/p/${enc(team)}/board?mode=${mode}`;
const agentPage = (team, seat) => `/p/${enc(team)}/agents/${enc(seat.endsWith(`@${team}`) ? seat.slice(0, -team.length - 1) : seat)}`;
const settings = (team) => `/p/${enc(team)}/settings`;
const results = (team, section) => `/p/${enc(team)}/results/${section}`;

async function shot(page, name, opts = {}) {
  const file = path.join(opts.dir ?? images, name);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  await wait(opts.settle ?? 1200);
  await page.screenshot({ path: file, ...(opts.clip ? { clip: opts.clip } : {}) });
  console.log(`saved ${name} (${Math.round(fs.statSync(file).size / 1024)} KB)`);
}

async function open(page, url) {
  await page.goto(base + url, { waitUntil: 'load' });
  await wait(1500);
}

/** Clicks the first element of `selector` whose text or label contains `text`. */
async function click(page, selector, text) {
  const handle = await page.waitForFunction(
    (sel, t) => [...document.querySelectorAll(sel)].find((e) => (e.getAttribute('aria-label') || e.textContent || '').includes(t)),
    { timeout: 10_000 }, selector, text,
  );
  await handle.asElement().evaluate((e) => {
    e.scrollIntoView({ block: 'center' });
    e.click();
  });
  await wait(600);
}

/** Opens a `<details>` fold by its summary's text, if it is closed. */
async function unfold(page, text) {
  const handle = await page.waitForFunction(
    (t) => [...document.querySelectorAll('details > summary')].find((e) => e.textContent.includes(t)),
    { timeout: 10_000 }, text,
  );
  await handle.asElement().evaluate((e) => { if (!e.parentElement.open) e.click(); });
  await wait(600);
}

/** Scrolls the element into view, `block` as in scrollIntoView; again once what loads above it has settled. */
async function scrollTo(page, selector, block = 'center', text = '') {
  const el = (await page.waitForFunction(
    (sel, t) => [...document.querySelectorAll(sel)].find((e) => e.textContent.includes(t)),
    { timeout: 15_000 }, selector, text,
  )).asElement();
  for (const ms of [1200, 400]) {
    await el.evaluate((e, b) => e.scrollIntoView({ block: b }), block);
    await wait(ms);
  }
  return el;
}

/** Results › History as its tree: each agent's box (last observed state, runtime, tokens, launch time) under the agent that started it. */
async function historyTree(page) {
  await page.waitForSelector('.history-agent', { timeout: 15_000 });
  // The tree has its height once the page has laid out; then fit it, so every box shows.
  await page.waitForFunction(() => document.querySelector('.history-graph .graph-viewport')?.getBoundingClientRect().height > 300, { timeout: 10_000 });
  await click(page, '.history-zoom button', 'Fit history');
}

/** Waits until the Run board has drawn this many agent cards. */
async function cards(page, n) {
  await page.waitForFunction((k) => document.querySelectorAll('.team-live .agent-box').length >= k, { timeout: 20_000 }, n);
}

const launch = () => puppeteer.launch({ executablePath: chrome, headless: true, args: ['--no-first-run', '--hide-scrollbars'] });

/** A step in a browser of its own: over plain HTTP, Chrome allows six connections per host, and the
 * live boards opened before keep theirs busy for a while. */
async function fresh(viewport, run) {
  const browser = await launch();
  try {
    const page = await browser.newPage();
    await page.setViewport(viewport);
    await run(page);
  } finally {
    await browser.close();
  }
}

async function main() {
  const browser = await launch();
  const page = await browser.newPage();
  await page.setViewport(DESKTOP);
  try {
    const only = arg('--only', null);
    const from = arg('--from', null);
    let on = !from;
    for (const step of [startFrom, websiteLaunch, manyChildren, importSession, readmeBoard]) {
      on ||= step.name === from;
      if (on && (!only || step.name === only)) await step(page);
    }
  } finally {
    await browser.close();
  }
}

/** The New project page: the Claude or Codex choice and the templates. */
async function startFrom(page) {
  await open(page, '/new');
  await shot(page, 'quickstart/start-from.png');
}

/** A Practice team: Design board, its limits in Settings, Run board with a question, the card, phone, History. */
async function websiteLaunch(page) {
  const draft = await team('planner-builders', 'Website launch', {
    Planner: 'Plan the landing page and hand each section to a builder.',
    'Builder 1': 'Build the landing page header and hero section.',
    'Builder 2': 'Build the pricing section.',
  });
  await open(page, board(draft.id, 'design'));
  await shot(page, 'quickstart/design-board.png');
  // Team limits: Settings → More settings → Limits on spawned agents.
  await open(page, settings(draft.id));
  await unfold(page, 'More settings');
  await scrollTo(page, 'fieldset', 'center', 'Limits on spawned agents');
  await shot(page, 'guides/team-limits.png');
  await start(draft);
  await wait(4000);
  const planner = (await agents(draft.id)).find((a) => a.name === 'Planner');
  // A Practice agent asks when its message starts with "ask:".
  await send(planner.id, 'ask: Which colour scheme should the new landing page use? | Calm blue | Bold orange');
  await wait(2500);
  await open(page, board(draft.id, 'run'));
  await cards(page, 3);
  await shot(page, 'quickstart/run-board.png');
  await open(page, agentPage(draft.id, planner.id));
  const card = await page.waitForSelector('.open-questions');
  const box = await card.boundingBox();
  await shot(page, 'guides/question-card.png', { clip: { x: box.x - 12, y: box.y - 12, width: box.width + 24, height: box.height + 16 } });
  await fresh({ width: 390, height: 844, isMobile: true, hasTouch: true }, async (phone) => {
    await open(phone, board(draft.id, 'run'));
    await shot(phone, 'quickstart/phone.png', { settle: 2000 });
  });
  await fresh(DESKTOP, async (p) => {
    await open(p, results(draft.id, 'history'));
    await p.waitForFunction(() => [...document.querySelectorAll('option')].some((o) => o.textContent.includes('#1')), { timeout: 15_000 });
    await historyTree(p);
    await shot(p, 'guides/history.png', { settle: 2000 });
  });
}

/** One Practice agent that starts eight helpers: the Run board folds them into "+N more". */
async function manyChildren() {
  const draft = await team('solo', 'Research sprint', { Claude: 'Split the research into topics and start a helper for each.' }, { Claude: 'Lead researcher' });
  await start(draft);
  await wait(3000);
  const lead = (await agents(draft.id))[0];
  const topics = ['pricing pages', 'onboarding emails', 'docs search', 'status pages', 'changelogs', 'API keys', 'billing', 'team invites'];
  for (const [i, t] of topics.entries()) {
    await send(lead.id, `spawn_agent:${JSON.stringify({ request_id: `topic-${i}`, name: `Research ${t}`, task: `Find three good examples of ${t}.` })}`);
    await wait(1500);
  }
  await wait(3000);
  await fresh(DESKTOP, async (page) => {
    await open(page, board(draft.id, 'run'));
    await cards(page, 6);
    // The board fits every saved box, folded ones too: zoom in once (the "+" key) so the cards can be
    // read, then drag the canvas right so the parent, its five drawn helpers and "+3 more" all show.
    await page.mouse.click(640, 720);
    await page.keyboard.press('+');
    await page.mouse.move(400, 720);
    await page.mouse.down();
    await page.mouse.move(470, 720, { steps: 5 });
    await page.mouse.move(540, 720, { steps: 5 });
    await page.mouse.up();
    await shot(page, 'guides/more-children.png', { settle: 1500 });
  });
}

/** Import a Claude Code session (the test fixture: a made-up "login page" session), then improve it. */
async function importSession() {
  await fresh(DESKTOP, async (page) => {
    await open(page, '/new');
    await unfold(page, 'Advanced');
    await scrollTo(page, '[aria-label="Import a past run"]');
    await shot(page, 'import/import-form.png');
    const input = await page.waitForSelector('[aria-label="Import a past run"] input[type=file][multiple]');
    // The fixture session, moved to the demo folder; only one subagent's files, so the preview lists the others as missing.
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'session-'));
    const files = ['main-with-agents.jsonl', 'subagents/agent-a1.jsonl', 'subagents/agent-a1.meta.json'].map((f) => {
      const to = path.join(dir, path.basename(f));
      fs.writeFileSync(to, fs.readFileSync(path.join(fixtures, f), 'utf8').replaceAll('/work/app', sessionFolder));
      return to;
    });
    await input.uploadFile(...files);
    await scrollTo(page, '.import-preview');
    await shot(page, 'import/preview.png');
    // Import opens the run in Results › History.
    await click(page, '.import-preview button', 'Import');
    await page.waitForFunction(() => location.pathname.endsWith('/results/history'), { timeout: 15_000 });
    await historyTree(page);
    await shot(page, 'import/history.png', { settle: 2000 });
    const teams = await api('GET', '/api/teams');
    const imported = teams.find((t) => (t.draft ?? t).name === 'Login page');
    const id = (imported.draft ?? imported).id;
    await open(page, board(id, 'design'));
    await shot(page, 'import/team.png');
    // Improve this team with AI: Settings → More settings.
    await open(page, settings(id));
    await unfold(page, 'More settings');
    await click(page, '.improve-from-run button', 'Improve this team with AI');
    await click(page, 'button', 'Ask Claude Code');
    await page.waitForSelector('.improve-offer', { timeout: 30_000 });
    await scrollTo(page, '.improve-offer');
    await shot(page, 'import/improve.png');
  });
}

/**
 * The README's board: a Run board of a demo team in four groups, with some agents working and one
 * that needs you. 1440×860 at twice the pixels, so it stays sharp on GitHub.
 */
async function readmeBoard() {
  const rec = await api('POST', '/api/teams', { request_id: rid(), template: 'blank', practice: true, name: 'Checkout redesign' });
  const folder = sessionFolder;
  const group = (id, label, col) => ({ id, label, x: 40 + col * 320, y: 40, w: 260, h: 560 });
  const agent = (id, name, col, row, job) => ({
    id, name, job, group_id: ['plan', 'build', 'review', 'ship'][col], kind: 'practice', folder,
    x: 70 + col * 320, y: 110 + row * 260, advanced: { model: null, permission: 'unrestricted', resume: 'resume-if-possible' },
  });
  const arrow = (from, to) => ({ id: `c-${from}-${to}`, from, to, kind: 'hands-work-to' });
  const draft = {
    ...rec.draft,
    folder,
    groups: [group('plan', 'Plan', 0), group('build', 'Build', 1), group('review', 'Review & test', 2), group('ship', 'Ship', 3)],
    agents: [
      agent('product-lead', 'Product lead', 0, 0, 'Turn the checkout brief into a plan: what changes, in what order, and how we will know it works.'),
      agent('designer', 'Designer', 0, 1, 'Design the new one-page checkout: the screens, their states and the copy.'),
      agent('api', 'API builder', 1, 0, 'Add the payment and order endpoints the checkout needs, with tests.'),
      agent('frontend', 'Frontend builder', 1, 1, 'Build the checkout screens from the design.'),
      agent('reviewer', 'Code reviewer', 2, 0, 'Review the API changes against the plan and report problems.'),
      agent('qa', 'QA tester', 2, 1, 'Run the checkout end to end, including failed and declined payments.'),
      agent('release', 'Release manager', 3, 0, 'Prepare the release once review and QA pass.'),
      agent('docs', 'Docs writer', 3, 1, 'Update the help pages for the new checkout once QA passes.'),
    ],
    connections: [
      arrow('product-lead', 'api'), arrow('product-lead', 'designer'), arrow('designer', 'frontend'),
      arrow('api', 'reviewer'), arrow('frontend', 'qa'), arrow('reviewer', 'release'), arrow('qa', 'docs'),
    ],
  };
  const saved = await save(draft, rec.draft.revision);
  await start(saved);
  // The first wave runs through every agent (a Practice agent answers at once); then the next wave.
  await wait(8000);
  const seats = Object.fromEntries((await agents(saved.id)).map((a) => [a.name, a.id]));
  await send(seats['Frontend builder'], 'Build the card form and the order summary next.');
  await send(seats['API builder'], 'Add retries to the payment endpoint.');
  await wait(1500);
  // A Practice agent's turn stays open on "hang": it shows as working.
  await send(seats['Frontend builder'], 'hang');
  await send(seats['API builder'], 'hang');
  await send(seats.Designer, 'ask: Should the checkout keep the order summary on the right, or fold it above the payment form? | Keep it on the right | Fold it above');
  await wait(2500);
  await fresh({ width: 1440, height: 860, deviceScaleFactor: 2 }, async (page) => {
    await open(page, board(saved.id, 'run'));
    await cards(page, 8);
    await shot(page, 'board.png', { dir: readmeImages, settle: 2500 });
  });
}

await main();
