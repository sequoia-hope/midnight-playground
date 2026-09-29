// Midnight Racer desktop shell.
//
// Serves the project folder through a private app:// scheme instead of a
// local HTTP server, so the desktop build needs no port and no web server.
// app://game/<path> maps to <project root>/<path>.
//
// Flags:
//   --dev              allow F12 devtools
//   --smoke-test       load the game, wait for window.__ready, report console
//                      errors, optionally screenshot (--shot <png>), then exit
//   --wait <ms>        smoke test: settle time after ready (default 2500)
//   --url-query <qs>   extra query string for index.html (e.g. "stats=1")

const { app, BrowserWindow, Menu, protocol, net, globalShortcut } = require('electron');
const path = require('node:path');
const fs = require('node:fs');
const { pathToFileURL } = require('node:url');

const ROOT = path.resolve(__dirname, '..');
const argv = process.argv.slice(1);
const flag = (name) => argv.includes(name);
const opt = (name) => { const i = argv.indexOf(name); return i >= 0 ? argv[i + 1] : null; };
const DEV = flag('--dev');
const SMOKE = flag('--smoke-test');
const SHOT = opt('--shot');
const QUERY = opt('--url-query');

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.cjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.webmanifest': 'application/manifest+json',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.webp': 'image/webp',
  '.gif': 'image/gif',
  '.ico': 'image/x-icon',
  '.wasm': 'application/wasm',
  '.mp3': 'audio/mpeg',
  '.ogg': 'audio/ogg',
  '.wav': 'audio/wav',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
  '.ttf': 'font/ttf',
  '.txt': 'text/plain; charset=utf-8',
  '.md': 'text/markdown; charset=utf-8',
};

// Local files only, plus Google Fonts for the HUD typeface (optional: the
// game falls back to system fonts offline). Inline script is the importmap.
const CSP = [
  "default-src 'self'",
  "script-src 'self' 'unsafe-inline'",
  "style-src 'self' 'unsafe-inline' https://fonts.googleapis.com",
  "font-src 'self' data: https://fonts.gstatic.com",
  "img-src 'self' data: blob:",
  "media-src 'self' data: blob:",
  "connect-src 'self' data: blob:",
  "worker-src 'self' blob:",
].join('; ');

protocol.registerSchemesAsPrivileged([{
  scheme: 'app',
  privileges: { standard: true, secure: true, supportFetchAPI: true, corsEnabled: true, stream: true },
}]);

// Game and audio keep running when the window is behind others.
app.commandLine.appendSwitch('autoplay-policy', 'no-user-gesture-required');
app.commandLine.appendSwitch('disable-renderer-backgrounding');
app.commandLine.appendSwitch('disable-background-timer-throttling');
app.commandLine.appendSwitch('ignore-gpu-blocklist');
app.setName('Midnight Racer');
// Lets the desktop entry (tools/install-desktop-entry.sh) claim the window.
process.env.CHROME_DESKTOP = process.env.CHROME_DESKTOP || 'midnight-racer.desktop';

// ── Window state ─────────────────────────────────────────────────
const stateFile = () => path.join(app.getPath('userData'), 'window-state.json');
function loadState() {
  try { return JSON.parse(fs.readFileSync(stateFile(), 'utf8')); } catch { return {}; }
}
function saveState(win) {
  try {
    const b = win.isFullScreen() || win.isMaximized() ? (win._lastBounds || win.getNormalBounds()) : win.getBounds();
    const s = { ...b, fullscreen: win.isFullScreen(), maximized: win.isMaximized() };
    fs.mkdirSync(path.dirname(stateFile()), { recursive: true });
    fs.writeFileSync(stateFile(), JSON.stringify(s));
  } catch { /* not worth failing over */ }
}

// ── app:// handler ───────────────────────────────────────────────
function serveApp(request) {
  let rel;
  try {
    const url = new URL(request.url);
    rel = decodeURIComponent(url.pathname);
  } catch {
    return new Response('bad request', { status: 400 });
  }
  if (rel === '/' || rel === '') rel = '/index.html';
  const file = path.resolve(ROOT, '.' + rel);
  // Refuse anything that escapes the project folder.
  if (file !== ROOT && !file.startsWith(ROOT + path.sep)) return new Response('forbidden', { status: 403 });
  let stat;
  try { stat = fs.statSync(file); } catch { return new Response('not found', { status: 404 }); }
  if (!stat.isFile()) return new Response('not found', { status: 404 });
  const type = MIME[path.extname(file).toLowerCase()] || 'application/octet-stream';
  return net.fetch(pathToFileURL(file).toString()).then(async (res) => {
    // Re-wrap so the MIME type is always right (module scripts need it).
    const headers = { 'content-type': type, 'cache-control': 'no-cache' };
    if (type.startsWith('text/html')) headers['content-security-policy'] = CSP;
    return new Response(res.body, { status: 200, headers });
  });
}

function createWindow() {
  const st = SMOKE ? {} : loadState();
  const iconPath = path.join(__dirname, 'icon.png');
  const win = new BrowserWindow({
    width: st.width || 1600,
    height: st.height || 900,
    x: st.x,
    y: st.y,
    minWidth: 800,
    minHeight: 450,
    title: 'Midnight Racer',
    backgroundColor: '#05060a',
    autoHideMenuBar: true,
    show: false,
    icon: fs.existsSync(iconPath) ? iconPath : undefined,
    webPreferences: {
      backgroundThrottling: false,
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      devTools: DEV || SMOKE,
    },
  });
  win.setMenu(null);
  if (st.maximized) win.maximize();
  if (st.fullscreen) win.setFullScreen(true);

  win.on('resize', () => { if (!win.isFullScreen() && !win.isMaximized()) win._lastBounds = win.getBounds(); });
  win.on('move', () => { if (!win.isFullScreen() && !win.isMaximized()) win._lastBounds = win.getBounds(); });
  if (!SMOKE) win.on('close', () => saveState(win));

  win.webContents.on('before-input-event', (event, input) => {
    if (input.type !== 'keyDown') return;
    if (input.key === 'F11') { win.setFullScreen(!win.isFullScreen()); event.preventDefault(); }
    else if ((input.control || input.meta) && input.key.toLowerCase() === 'q') { app.quit(); event.preventDefault(); }
    else if (input.key === 'F12' && DEV) { win.webContents.toggleDevTools(); event.preventDefault(); }
  });
  // Never navigate away from the game or open popups inside the shell.
  win.webContents.on('will-navigate', (e, url) => { if (!url.startsWith('app://')) e.preventDefault(); });
  win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));

  win.once('ready-to-show', () => win.show());
  const q = QUERY ? '?' + QUERY.replace(/^\?/, '') : '';
  win.loadURL('app://game/index.html' + q);
  return win;
}

async function smokeTest(win) {
  const errors = [];
  win.webContents.on('console-message', (...args) => {
    // Electron ≥ 35 passes a single event object with level/message.
    const e = args[0];
    const level = e && e.level !== undefined ? e.level : args[1];
    const message = e && e.message !== undefined ? e.message : args[2];
    const isErr = level === 'error' || level === 3;
    const isWarn = level === 'warning' || level === 2;
    if (isErr || isWarn) errors.push(`[${isErr ? 'error' : 'warn'}] ${message}`);
  });
  win.webContents.on('render-process-gone', (_e, d) => errors.push(`[crash] renderer gone: ${d.reason}`));
  const t0 = Date.now();
  let ready = false;
  while (Date.now() - t0 < 60000) {
    try { ready = await win.webContents.executeJavaScript('window.__ready === true'); } catch { /* page loading */ }
    if (ready) break;
    await new Promise((r) => setTimeout(r, 250));
  }
  const loadMs = Date.now() - t0;
  let info = {};
  if (ready) {
    await new Promise((r) => setTimeout(r, Number(opt('--wait')) || 2500));
    try {
      info = await win.webContents.executeJavaScript(`(() => {
        const c = document.createElement('canvas').getContext('webgl2');
        const d = c && c.getExtension('WEBGL_debug_renderer_info');
        return { renderer: d ? c.getParameter(d.UNMASKED_RENDERER_WEBGL) : 'unknown', url: location.href, title: document.title };
      })()`);
    } catch (e) { info = { error: String(e) }; }
  }
  if (SHOT) {
    try {
      const img = await win.webContents.capturePage();
      fs.writeFileSync(SHOT, img.toPNG());
    } catch (e) { errors.push('[smoke] screenshot failed: ' + e); }
  }
  const real = errors.filter((e) => !/favicon|404 \(File not found\)|Autofill|fonts\.g/i.test(e));
  console.log(JSON.stringify({ ready, loadMs, ...info, messages: real }, null, 2));
  app.exit(ready && !real.some((e) => e.startsWith('[error]') || e.startsWith('[crash]')) ? 0 : 1);
}

app.whenReady().then(() => {
  protocol.handle('app', serveApp);
  const win = createWindow();
  if (SMOKE) smokeTest(win);
  app.on('activate', () => { if (BrowserWindow.getAllWindows().length === 0) createWindow(); });
});

app.on('window-all-closed', () => app.quit());
app.on('will-quit', () => globalShortcut.unregisterAll());
