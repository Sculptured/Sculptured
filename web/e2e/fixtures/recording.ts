import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import type { Browser, BrowserContext, Locator, Page, TestInfo } from '@playwright/test';

/**
 * Helpers for the documentation recordings (control-video.spec.ts, run with RECORD_VIDEOS=1). Every
 * frame is the real app; the only things drawn over it are a pointer (headless browsers draw none) and
 * a caption naming the step, both marked `data-recording-overlay` and never taking a click.
 */

export const DOCS = fileURLToPath(new URL('../../../docs/', import.meta.url));
export const SHOTS = `${DOCS}images/control/`;
export const VIDEOS = `${DOCS}videos/`;
export const VIDEO_SIZE = { width: 1280, height: 800 };
/** The size of the desktop guide screenshots. */
export const SHOT_SIZE = { width: 1440, height: 900 };

/** Draws the pointer where the mouse is (bigger while pressed) and a caption box at the bottom. */
function overlay() {
  const add = () => {
    const style = document.createElement('style');
    style.textContent = `
      [data-recording-overlay] { position: fixed; pointer-events: none; z-index: 2147483647; }
      #rec-pointer { width: 18px; height: 18px; margin: -9px 0 0 -9px; border-radius: 50%; border: 2px solid white;
        background: rgba(63, 208, 255, 0.45); box-shadow: 0 0 0 1px black; transition: transform 90ms; left: -40px; top: -40px; }
      #rec-pointer.down { transform: scale(1.6); }
      #rec-caption { left: 50%; bottom: 18px; transform: translateX(-50%); max-width: 80%; padding: 8px 16px; border-radius: 6px;
        background: rgba(4, 12, 18, 0.88); color: white; font: 600 17px/1.35 system-ui, sans-serif; border: 1px solid rgba(255,255,255,.25); }
      #rec-caption:empty { display: none; }`;
    const pointer = Object.assign(document.createElement('div'), { id: 'rec-pointer' });
    const caption = Object.assign(document.createElement('div'), { id: 'rec-caption' });
    for (const el of [pointer, caption]) el.setAttribute('data-recording-overlay', '');
    document.documentElement.append(style, pointer, caption);
    addEventListener('mousemove', (e) => { pointer.style.left = `${e.clientX}px`; pointer.style.top = `${e.clientY}px`; }, true);
    addEventListener('mousedown', () => pointer.classList.add('down'), true);
    addEventListener('mouseup', () => pointer.classList.remove('down'), true);
    const saved = sessionStorage.getItem('rec-caption');
    if (saved) caption.textContent = saved;
  };
  if (document.readyState === 'loading') addEventListener('DOMContentLoaded', add);
  else add();
}

/** A browser context that records a video of its first page at VIDEO_SIZE. */
export async function recorder(browser: Browser, info: TestInfo, baseURL: string) {
  const context = await browser.newContext({ baseURL, viewport: VIDEO_SIZE, recordVideo: { dir: info.outputPath('raw'), size: VIDEO_SIZE } });
  await context.addInitScript(overlay);
  const page = await context.newPage();
  return { context, page };
}

/** A desktop context for guide screenshots at SHOT_SIZE, with the caption box (no video). */
export async function stills(browser: Browser, baseURL: string) {
  const context = await browser.newContext({ baseURL, viewport: SHOT_SIZE });
  await context.addInitScript(overlay);
  const page = await context.newPage();
  return { context, page };
}

/** Says what the next step does, and keeps it over page loads. */
export async function caption(page: Page, text: string) {
  await page.evaluate((t) => {
    sessionStorage.setItem('rec-caption', t);
    const el = document.getElementById('rec-caption');
    if (el) el.textContent = t;
  }, text);
}

/** A pause a viewer can follow. */
export const beat = (page: Page, ms = 1200) => page.waitForTimeout(ms);

/** Moves the pointer to the middle of `l` in visible steps. */
export async function glide(page: Page, l: Locator) {
  const b = (await l.boundingBox())!;
  await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2, { steps: 18 });
}

/** Glides to `l`, waits a moment and clicks it. */
export async function press(page: Page, l: Locator) {
  await l.scrollIntoViewIfNeeded();
  await glide(page, l);
  await page.waitForTimeout(350);
  await l.click();
}

const hasFfmpeg = () => {
  try { execFileSync('ffmpeg', ['-version'], { stdio: 'ignore' }); return true; } catch { return false; }
};

/**
 * Closes the context and keeps its video as `docs/videos/<name>.webm` (VP9) and `<name>.mp4` (H.264)
 * when ffmpeg is installed, else the browser's own WebM. `maxSeconds` cuts the end off (the browser can
 * keep recording a still page while the context closes).
 */
export async function keepVideo(context: BrowserContext, page: Page, info: TestInfo, name: string, maxSeconds?: number) {
  const raw = info.outputPath(`${name}-raw.webm`);
  const saved = page.video()!.saveAs(raw);
  await context.close();
  await saved;
  mkdirSync(VIDEOS, { recursive: true });
  if (!hasFfmpeg()) { copyFileSync(raw, `${VIDEOS}${name}.webm`); return; }
  const quiet = ['-y', '-loglevel', 'error', '-i', raw, '-an', ...(maxSeconds ? ['-t', String(maxSeconds)] : [])];
  execFileSync('ffmpeg', [...quiet, '-c:v', 'libvpx-vp9', '-crf', '40', '-b:v', '0', '-row-mt', '1', `${VIDEOS}${name}.webm`]);
  execFileSync('ffmpeg', [...quiet, '-c:v', 'libx264', '-crf', '30', '-preset', 'slow', '-pix_fmt', 'yuv420p', '-movflags', '+faststart', `${VIDEOS}${name}.mp4`]);
}

/** A screenshot for the guide: the caption stays, the pointer is hidden. */
export async function shot(page: Page, name: string) {
  mkdirSync(SHOTS, { recursive: true });
  await page.mouse.move(0, 0);
  const pointer = (hidden: boolean) => page.evaluate((h) => {
    const el = document.getElementById('rec-pointer');
    if (el) el.style.visibility = h ? 'hidden' : '';
  }, hidden);
  await pointer(true);
  await page.screenshot({ path: `${SHOTS}${name}.png` });
  await pointer(false);
}
