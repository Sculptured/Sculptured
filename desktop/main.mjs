// Sculptured desktop. Two modes, chosen on first launch and remembered (menu: Switch mode…):
// - On this computer: a window on the local workspace. It starts the bundled `sculptured serve` (which
//   starts the agent runner) unless one already answers on the port. Agents keep running when the
//   window closes; the runner owns them. Community stays off until it is turned on in the app.
// - Sculptured account: the account's site, with this computer connected by the bundled runner
//   (`sculptured connect`), approved in this window, and stopped when the app quits.
// Teams and settings are not synced between the two (later).
import { app, BrowserWindow, dialog, ipcMain, Menu, shell } from 'electron';
import { spawn } from 'node:child_process';
import { existsSync, readFileSync, rmSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { allowedOrigins, defaultAccountOrigin, loadMode, mayStay, saveMode, validOrigin } from './src/mode.mjs';
import { startRunner } from './src/runner.mjs';

// Read by the chooser's preload, which starts after this.
process.env.SCULPTURED_ACCOUNT_ORIGIN = defaultAccountOrigin(process.env, JSON.parse(readFileSync(path.join(app.getAppPath(), 'package.json'), 'utf8')));
const PORT = Number(process.env.SCULPTURED_PORT || 4410);
const ORIGIN = `http://127.0.0.1:${PORT}`;
const DEV = process.env.SCULPTURED_DEV === '1';
let server = null;
let runner = null;
let window = null;

/** The bundled binary and web build when packaged; the repository's release build in development. */
function resources() {
  const root = app.isPackaged ? process.resourcesPath : path.join(app.getAppPath(), '..');
  const exe = process.platform === 'win32' ? 'sculptured.exe' : 'sculptured';
  return {
    bin: process.env.SCULPTURED_BIN || (app.isPackaged ? path.join(root, 'bin', exe) : path.join(root, 'target', 'release', exe)),
    web: app.isPackaged ? path.join(root, 'web') : path.join(root, 'web', 'dist'),
  };
}

/** True when a Sculptured workspace already answers on the port. */
async function answering() {
  try {
    const res = await fetch(`${ORIGIN}/api/teams`, { signal: AbortSignal.timeout(1500) });
    return res.ok;
  } catch {
    return false;
  }
}

function startServer() {
  const { bin, web } = resources();
  if (!existsSync(bin)) throw new Error(`The Sculptured program is missing: ${bin}`);
  const args = ['serve', '--port', String(PORT), '--web-dir', web];
  if (process.env.SCULPTURED_DATA_DIR) args.push('--data-dir', process.env.SCULPTURED_DATA_DIR);
  server = spawn(bin, args, { stdio: ['ignore', 'inherit', 'pipe'] });
  let tail = '';
  server.stderr.on('data', (d) => { tail = (tail + d.toString()).slice(-4000); });
  server.on('exit', (code) => {
    server = null;
    if (!app.isQuitting) fail(`Sculptured stopped (${code}).\n${tail}`);
  });
}

async function waitReady() {
  for (let i = 0; i < 60; i++) {
    if (await answering()) return;
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error('Sculptured did not become ready within 30 seconds.');
}

function fail(message) {
  dialog.showErrorBox('Sculptured', message);
  app.quit();
}

/** The allowed origins (and sign-in providers' pages) stay in the window; other web links (such as a CLI sign-in page) open in the system browser. */
function openWindow(allowed, start) {
  window = new BrowserWindow({
    width: 1280, height: 850, minWidth: 640, minHeight: 480, show: false, title: 'Sculptured',
    backgroundColor: '#10141c', autoHideMenuBar: true,
    icon: path.join(app.getAppPath(), 'icon.png'),
    webPreferences: { nodeIntegration: false, contextIsolation: true, sandbox: true, spellcheck: false },
  });
  const external = (url) => {
    if (/^https:\/\//.test(url)) void shell.openExternal(url);
  };
  window.webContents.setWindowOpenHandler(({ url }) => { external(url); return { action: 'deny' }; });
  // Server redirects (such as sign-in) don't fire will-navigate, so both are checked.
  const guard = (e, url) => { if (!mayStay(url, allowed)) { e.preventDefault(); external(url); } };
  window.webContents.on('will-redirect', guard);
  window.webContents.on('will-navigate', guard);
  window.on('close', (e) => { if (!confirmStopAgents()) e.preventDefault(); });
  window.webContents.session.setPermissionRequestHandler((_c, _p, cb) => cb(false));
  window.once('ready-to-show', () => window.show());
  window.loadURL(start).catch((e) => fail(e.message));
}

/** First launch: ask how to use Sculptured; resolves with the saved choice. */
function choose() {
  return new Promise((resolve) => {
    const w = new BrowserWindow({
      width: 640, height: 600, title: 'Sculptured', backgroundColor: '#10141c', autoHideMenuBar: true,
      webPreferences: { nodeIntegration: false, contextIsolation: true, sandbox: true, preload: path.join(app.getAppPath(), 'src', 'chooser-preload.cjs') },
    });
    let chosen = false;
    w.on('closed', () => { if (!chosen) app.quit(); });
    w.webContents.on('will-navigate', (e) => e.preventDefault());
    w.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
    ipcMain.handle('sculptured:choose', (_e, { mode, origin }) => {
      const o = origin ? validOrigin(origin, { dev: DEV }) : null;
      if (mode === 'account' && !o) return 'Enter your account address, starting with https://';
      if (mode === 'local' && origin && !o) return 'That address must start with https:// (or leave it empty)';
      const m = mode === 'account' ? { mode, origin: o } : o ? { mode: 'local', origin: o } : { mode: 'local' };
      saveMode(app.getPath('userData'), m);
      ipcMain.removeHandler('sculptured:choose');
      chosen = true;
      resolve(m);
      w.close();
      return '';
    });
    w.loadFile(path.join(app.getAppPath(), 'src', 'chooser.html'));
  });
}

/** Account mode: quitting stops the agents running here for the account, so ask once first. */
function confirmStopAgents() {
  if (!runner || app.agentsStopConfirmed) return true;
  const choice = dialog.showMessageBoxSync(window ?? undefined, {
    type: 'question', buttons: ['Quit and stop agents', 'Cancel'], defaultId: 1, cancelId: 1,
    message: 'Quit Sculptured?', detail: 'Agents running on this computer for your account will stop.',
  });
  app.agentsStopConfirmed = choice === 0;
  return app.agentsStopConfirmed;
}

/** Account mode: connect this computer with the bundled runner, approving the code in the window. */
function connectThisComputer(origin) {
  const { bin } = resources();
  if (!existsSync(bin)) throw new Error(`The Sculptured program is missing: ${bin}`);
  runner = startRunner({ bin, origin, dataDir: path.join(app.getPath('userData'), 'runner'), name: os.hostname() });
  runner.onCode((code) => window?.loadURL(`${origin}/connect?code=${encodeURIComponent(code)}`));
  runner.onConnected(() => { if (window && !window.webContents.getURL().startsWith(origin + '/')) window.loadURL(origin); });
  runner.child.on('exit', () => { runner = null; });
}

function menu() {
  Menu.setApplicationMenu(Menu.buildFromTemplate([
    {
      label: 'Sculptured',
      submenu: [
        { label: 'Switch mode…', click: () => { rmSync(path.join(app.getPath('userData'), 'mode.json'), { force: true }); app.relaunch(); app.quit(); } },
        { type: 'separator' },
        { role: 'quit' },
      ],
    },
    { role: 'editMenu' },
    { role: 'viewMenu' },
  ]));
}

if (!app.requestSingleInstanceLock()) app.quit();
else {
  app.on('second-instance', () => { if (window) { window.restore(); window.focus(); } });
  app.on('before-quit', (e) => {
    if (!confirmStopAgents()) { e.preventDefault(); return; }
    app.isQuitting = true; server?.kill('SIGTERM'); runner?.stop();
  });
  app.on('window-all-closed', () => { if (app.isQuitting || window) app.quit(); });
  app.whenReady().then(async () => {
    menu();
    const mode = loadMode(app.getPath('userData'), { dev: DEV }) ?? (await choose());
    if (mode.mode === 'account') {
      openWindow(allowedOrigins({ ...mode, local: ORIGIN }), mode.origin);
      connectThisComputer(mode.origin);
      return;
    }
    if (!(await answering())) startServer();
    await waitReady();
    openWindow(allowedOrigins({ ...mode, local: ORIGIN }), ORIGIN);
  }).catch((e) => fail(e.message));
}
