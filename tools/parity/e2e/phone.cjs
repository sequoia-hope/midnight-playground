// The phone check of roadmap M4 (a race straight from the address, D432),
// kept for the screens of WP 6.2: headless Chrome on the GPU, an iPhone held
// sideways (844 x 390 at 3x, touch), the Rust web build from the working
// tree by request interception (no server). Drives the race with CDP touch
// events on the stick and the pedal slider (gas, drift, brake), pauses with
// the touch pause button and resumes with the pause screen's Resume (the
// M4 card resumed on a tap anywhere; that spot is Main menu on the JS's
// screen, D578), or lets the autopilot run to the results; saves
// __mr.screenshot()s.
//   cargo xtask web --release
//   node tools/parity/e2e/phone.cjs <repo root> <out dir> [touch|auto|desktop] [query]
// The level is $LEVEL, else Seaside.
const puppeteer = require('puppeteer-core');
const fs = require('fs');
const path = require('path');

const [root, out, kind = 'touch', extra = ''] = process.argv.slice(2);
const ORIGIN = 'https://midnight-racer.test';
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.json': 'application/json', '.wasm': 'application/wasm', '.bin': 'application/octet-stream', '.mrscene': 'application/octet-stream' };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

(async () => {
  fs.mkdirSync(out, { recursive: true });
  const browser = await puppeteer.launch({
    executablePath: '/usr/bin/google-chrome', headless: 'new',
    args: ['--enable-unsafe-webgpu', '--enable-features=Vulkan', '--use-angle=vulkan', '--ignore-gpu-blocklist'],
  });
  const errors = [];
  try {
    const page = await browser.newPage();
    if (kind === 'desktop') await page.setViewport({ width: 1280, height: 800, deviceScaleFactor: 1 });
    else await page.emulate({
      userAgent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1',
      viewport: { width: 844, height: 390, deviceScaleFactor: 3, isMobile: true, hasTouch: true, isLandscape: true },
    });
    const cdp = await page.createCDPSession();
    await cdp.send('Browser.setDownloadBehavior', { behavior: 'allow', downloadPath: out });
    page.on('console', (m) => { const t = m.text(); if (m.type() === 'error' && !/Failed to load resource/.test(t)) errors.push(t); if (process.env.MR_VERBOSE) console.log('[page]', t); });
    page.on('pageerror', (e) => errors.push(String(e)));
    await page.setRequestInterception(true);
    page.on('request', (req) => {
      const u = new URL(req.url());
      if (u.origin !== ORIGIN) return req.continue();
      let p = path.join(root, decodeURIComponent(u.pathname));
      if (p.endsWith('/')) p = path.join(p, 'index.html');
      if (!fs.existsSync(p)) return req.respond({ status: 404, body: 'not found' });
      req.respond({ status: 200, contentType: TYPES[path.extname(p)] || 'application/octet-stream', body: fs.readFileSync(p) });
    });
    const q = `level=${process.env.LEVEL || 'seaside'}${kind !== 'touch' ? '&autodrive=1&timescale=4&seed=1' : ''}${extra ? '&' + extra : ''}`;
    await page.goto(`${ORIGIN}/dist/next/index.html?${q}`);
    const ev = (f) => cdp.send('Runtime.evaluate', { expression: f, returnByValue: true, userGesture: false }).then((r) => r.result.value);
    const t0 = Date.now();
    for (;;) {
      const s = await ev('({state: __mr.state, ready: __mr.ready, error: __mr.error, race: __mr.race})');
      if (s.error || s.state === 'failed') throw new Error('client failed: ' + s.error);
      if (s.ready && s.race) break;
      if (Date.now() - t0 > 240000) throw new Error('not ready: ' + JSON.stringify(s));
      await sleep(300);
    }
    const shot = async (name) => {
      const f = path.join(out, name);
      fs.rmSync(f, { force: true });
      await ev(`__mr.screenshot(${JSON.stringify(name)})`);
      for (let i = 0; i < 100 && !fs.existsSync(f); i++) await sleep(100);
      console.log('shot', f, fs.existsSync(f));
    };
    const info = await ev('({touch: __mr.touch, insets: __mr.insets, canvas: [document.getElementById("game").width, document.getElementById("game").height, document.getElementById("game").clientWidth], race: __mr.race})');
    console.log('ready', JSON.stringify(info));
    await sleep(1200);
    await shot(`${kind}-countdown.png`);
    for (;;) {
      const r = await ev('__mr.race');
      if (r.state === 'racing') break;
      await sleep(100);
    }
    if (kind === 'touch') {
      // Phone 844 × 390 (CSS px): the slider's GAS band (flat out) near x 736, y 207.
      const touch = (type, points) => cdp.send('Input.dispatchTouchEvent', { type, touchPoints: points });
      const stick = { x: 200, y: 300, id: 1 }, gas = { x: 736, y: 200, id: 2 };
      await touch('touchStart', [stick]);
      await touch('touchStart', [stick, gas]);
      for (let i = 0; i < 40; i++) {
        await touch('touchMove', [{ ...stick, x: 200 + Math.min(25, i) }, gas]);
        await sleep(50);
      }
      const r1 = await ev('__mr.race');
      console.log('driving', JSON.stringify(r1));
      if (!(r1.input.throttle === 1 && r1.input.analog && r1.input.steer > 0 && r1.speed > 8)) errors.push('touch driving failed: ' + JSON.stringify(r1.input) + ' speed ' + r1.speed);
      await shot('touch-race.png');
      // Slide right onto DRIFT, then down to the brake.
      await touch('touchMove', [stick, { ...gas, x: 800 }]);
      await sleep(200);
      const r2 = await ev('__mr.race.input');
      console.log('drift', JSON.stringify(r2));
      if (!r2.handbrake) errors.push('drift strip failed');
      await touch('touchMove', [stick, { ...gas, y: 370 }]);
      await sleep(200);
      const r3 = await ev('__mr.race.input');
      console.log('brake', JSON.stringify(r3));
      if (!(r3.brake > 0.9 && r3.throttle === 0)) errors.push('brake failed');
      await touch('touchEnd', []);
      await sleep(200);
      // Pause button, then a tap resumes.
      await touch('touchStart', [{ x: 16 + 108 + 22, y: 96 + 8 + 22, id: 3 }]);
      await touch('touchEnd', []);
      await sleep(300);
      const m1 = await ev('__mr.race.mode');
      await shot('touch-paused.png');
      const rb = await ev("__mr.ui('btn-resume')"); await touch('touchStart', [{ x: rb.x + rb.w / 2, y: rb.y + rb.h / 2, id: 4 }]);
      await touch('touchEnd', []);
      await sleep(300);
      const m2 = await ev('__mr.race.mode');
      console.log('pause', m1, '→', m2);
      if (m1 !== 'paused' || m2 !== 'race') errors.push('pause/resume failed');
    } else {
      let raced = false; // desktop and auto
      for (;;) {
        const r = await ev('__mr.race');
        if (r.time > 20 && !raced) { raced = true; await shot(`${kind}-race.png`); }
        if (r.mode === 'results') break;
        if (Date.now() - t0 > 600000) throw new Error('no results');
        await sleep(500);
      }
      await sleep(1500);
      await shot(`${kind}-results.png`);
      console.log('results', JSON.stringify(await ev('__mr.race.results')));
    }
    console.log('fps-ish frames', await ev('__mr.frames'));
  } catch (e) {
    errors.push(String(e.message || e));
  } finally {
    await browser.close();
  }
  if (errors.length) { console.error('errors:\n  ' + errors.join('\n  ')); process.exit(1); }
})();
