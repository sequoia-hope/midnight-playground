// The driving aids on an emulated iPhone (Rust only, DECISIONS D1080–D1084):
// a race with the touch screen's defaults (guide line Full, steering assist
// Light), checked on `__mp.aids`; then the car is put on the road at speed
// 200 m before a hairpin with the gas held on the pedal slider, and the
// line is pictured as it turns from green to orange to red; then every
// camera mode on the same approach; on a day and a night level, sideways
// and upright. Last, `line=brake` (only the braking part shows) and
// `line=off&assist=0` (nothing, and no assist). Pictures into the given
// directory.
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

// A slow corner after a fast stretch (the track's speed profile: about
// 18–19 m/s at the apex, 65+ m/s 200 m before), metres past the start.
const HAIRPIN = { sierra: 974 - 60, streets: 495 - 60 };

const fails = [];
const check = (ok, what) => {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}`);
  if (!ok) fails.push(what);
};

async function race(browser, device, level, query) {
  const g = await openGame(browser, { device, query: `level=${level}&autostart=sports&seed=1${query}`, downloads: dir });
  await g.waitFor(() => window.__mp.race?.state === 'racing' && window.__mp.screen === 'none', { timeout: 120000, what: 'racing' });
  return g;
}

// The gas on the pedal slider (its band at 72 % up), held by finger 1.
async function gas(g, on) {
  const s = await g.ui('touch-slider');
  const p = { x: s.x + s.w / 2, y: s.y + s.h * (1 - 0.72), id: 1 };
  await g.cdp.send('Input.dispatchTouchEvent', { type: on ? 'touchStart' : 'touchEnd', touchPoints: on ? [p] : [] });
}

// 200 m short of the hairpin at 42 m/s, on the line's side of the road.
async function approach(g, level) {
  await g.eval((c) => window.__mp.stage({ cmd: 'place', ...c }), { ahead: HAIRPIN[level] - 200, speed: 42, lat: 0 });
  await g.frames(3);
}

const browser = await launch();
try {
  for (const device of devices) {
    for (const level of levels) {
      const tag = `${level}-${device}`;
      const g = await race(browser, device, level, '');
      const aids = await g.eval('window.__mp.aids');
      check(aids?.guide === 'full' && aids?.assist === 'light', `${tag}: touch defaults ${JSON.stringify(aids)}`);
      await gas(g, true);
      await sleep(1500);
      const a0 = await g.eval('window.__mp.aids');
      check(a0.shown > 10, `${tag}: chevrons shown (${a0.shown})`);
      // Into the hairpin flat out: green, then orange, then red.
      await approach(g, level);
      const seen = [];
      for (let k = 0; k < 6; k++) {
        const a = await g.eval('window.__mp.aids');
        seen.push(+a.maxUrgency.toFixed(2));
        await g.shot(`${tag}-approach-${k}.png`, dir);
        await sleep(450);
      }
      console.log(`${tag}: highest urgency on the approach ${seen.join(' ')}`);
      check(Math.max(...seen) >= 1.5, `${tag}: the line turns red into the hairpin`);
      // Every camera mode (the camera button), on the same approach.
      for (let m = 0; m < 3; m++) {
        await approach(g, level);
        await sleep(900);
        const cam = await g.eval('window.__mp.race.camMode');
        await g.shot(`${tag}-cam${cam}.png`, dir);
        await g.tap('touch-camera');
      }
      await gas(g, false);
      // The haptic tick before any real tap is refused; nothing else may err.
      const errors = g.errors.filter((e) => !/navigator\.vibrate/.test(e));
      check(errors.length === 0, `${tag}: no page errors ${JSON.stringify(errors.slice(0, 3))}`);
      await g.close();
    }
  }
  // Braking only, and off with no assist (the query overrides the setting).
  const device = devices[0];
  const level = levels[0];
  {
    const g = await race(browser, device, level, '&line=brake');
    await gas(g, true);
    await sleep(1000);
    const calm = await g.eval('window.__mp.aids');
    await approach(g, level);
    let shown = 0;
    for (let k = 0; k < 6; k++) {
      const a = await g.eval('window.__mp.aids');
      shown = Math.max(shown, a.shown);
      await g.shot(`${level}-${device}-brake-${k}.png`, dir);
      await sleep(450);
    }
    check(calm.guide === 'brake', `line=brake: ${JSON.stringify(calm)}`);
    check(shown > 0, `line=brake: the braking part shows into the hairpin (${shown} chevrons at most)`);
    await gas(g, false);
    await g.close();
  }
  {
    const g = await race(browser, device, level, '&line=off&assist=0');
    await gas(g, true);
    await approach(g, level);
    await sleep(1200);
    const a = await g.eval('window.__mp.aids');
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
