// The driving aids on an emulated iPhone (Rust only, DECISIONS D1080–D1084):
// a race with the touch screen's defaults (guide line Full, steering assist
// Light), the gas held on the pedal slider, pictures of the line as it
// turns from green to red into a corner, in each camera mode, on a day and
// a night level, sideways and upright; then `line=brake` and `line=off`
// with `assist=0`. Checks `__mr.aids` says what the settings say and that
// the line shows (or not). Pictures into the given directory.
//
//   cargo xtask web --release
//   node tools/parity/e2e/guide-line.mjs <out dir> [--only sierra,streets] [--devices iphone,iphonePortrait]
import fs from 'node:fs';
import path from 'node:path';
import { launch, openGame, sleep, DEVICES } from './harness.mjs';

const args = process.argv.slice(2);
const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
const dir = path.resolve(args[0] && !args[0].startsWith('--') ? args[0] : 'parity/report/guide-line');
const levels = opt('--only', 'sierra,streets').split(',');
const devices = opt('--devices', 'iphone,iphonePortrait').split(',');
fs.mkdirSync(dir, { recursive: true });

DEVICES.iphonePortrait = {
  userAgent: DEVICES.iphone.userAgent,
  viewport: { width: 390, height: 844, deviceScaleFactor: 3, isMobile: true, hasTouch: true, isLandscape: false },
};

const fails = [];
const check = (ok, what) => {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}`);
  if (!ok) fails.push(what);
};

async function race(browser, device, level, query) {
  const g = await openGame(browser, { device, query: `level=${level}&autostart=sports&seed=1${query}`, downloads: dir });
  await g.waitFor(() => window.__mr.race?.state === 'racing' && window.__mr.screen === 'none', { timeout: 120000, what: 'racing' });
  return g;
}

// The gas on the pedal slider (its band at 72 % up), held by finger 1.
async function gas(g, on) {
  const s = await g.ui('touch-slider');
  const p = { x: s.x + s.w / 2, y: s.y + s.h * (1 - 0.72), id: 1 };
  await g.cdp.send('Input.dispatchTouchEvent', { type: on ? 'touchStart' : 'touchEnd', touchPoints: on ? [p] : [] });
}

const browser = await launch();
try {
  for (const device of devices) {
    for (const level of levels) {
      const tag = `${level}-${device}`;
      const g = await race(browser, device, level, '');
      const aids = await g.eval('window.__mr.aids');
      check(aids?.guide === 'full' && aids?.assist === 'light', `${tag}: touch defaults ${JSON.stringify(aids)}`);
      await gas(g, true);
      // Shots through the first corners: the line turns as the car closes in.
      let worst = 0;
      for (let k = 0; k < 6; k++) {
        await sleep(1500);
        const a = await g.eval('window.__mr.aids');
        worst = Math.max(worst, a.maxUrgency);
        await g.shot(`${tag}-full-${k}.png`, dir);
      }
      const a = await g.eval('window.__mr.aids');
      check(a.shown > 10, `${tag}: chevrons shown (${a.shown})`);
      console.log(`${tag}: highest urgency seen ${worst.toFixed(2)}`);
      // Every camera mode (the ⟳ camera button).
      for (let m = 0; m < 3; m++) {
        const cam = await g.eval('window.__mr.race.camMode');
        await g.shot(`${tag}-cam${cam}.png`, dir);
        await g.tap('touch-camera');
        await sleep(600);
      }
      await gas(g, false);
      check(g.errors.length === 0, `${tag}: no page errors ${JSON.stringify(g.errors.slice(0, 3))}`);
      await g.close();
    }
  }
  // Braking only, and off with no assist (the query overrides the setting).
  const device = devices[0];
  const level = levels[0];
  {
    const g = await race(browser, device, level, '&line=brake');
    await gas(g, true);
    let shownMax = 0;
    let shownMin = 1e9;
    for (let k = 0; k < 8; k++) {
      await sleep(1000);
      const a = await g.eval('window.__mr.aids');
      shownMax = Math.max(shownMax, a.shown);
      shownMin = Math.min(shownMin, a.shown);
      if (k % 2) await g.shot(`${level}-${device}-brake-${k}.png`, dir);
    }
    const a = await g.eval('window.__mr.aids');
    check(a.guide === 'brake', `line=brake: ${JSON.stringify(a)}`);
    console.log(`line=brake: chevrons shown from ${shownMin} to ${shownMax}`);
    await gas(g, false);
    await g.close();
  }
  {
    const g = await race(browser, device, level, '&line=off&assist=0');
    await gas(g, true);
    await sleep(2000);
    const a = await g.eval('window.__mr.aids');
    check(a.guide === 'off' && a.assist === 'off' && a.shown === 0, `line=off&assist=0: ${JSON.stringify(a)}`);
    await g.shot(`${level}-${device}-off.png`, dir);
    await gas(g, false);
    await g.close();
  }
} finally {
  await browser.close();
}
console.log(fails.length ? `FAIL (${fails.length})` : 'PASS');
process.exit(fails.length ? 1 : 0);
