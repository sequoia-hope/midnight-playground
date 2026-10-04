// From the menu to a Seaside race on an emulated iPhone held sideways, by
// touch only (roadmap WP 6.2): tap the Seaside tab, wait for it to build,
// pick a car, tap Race, wait for GO, hold the gas on the pedal slider, and
// check the car moves with the sound running; saves __mr.screenshot()s.
//   cargo xtask web --release
//   node tools/parity/e2e/phone-menu.mjs <out dir>
import fs from 'node:fs';
import path from 'node:path';
import { launch, openGame, sleep } from './harness.mjs';
const dir = path.resolve(process.argv[2] || 'parity/report/phone-menu');
fs.mkdirSync(dir, { recursive: true });
const browser = await launch();
let ok = false;
try {
  const g = await openGame(browser, { device: 'iphone', downloads: dir });
  console.log('menu', JSON.stringify(await g.snapshot()));
  await g.shot('menu-start.png', dir);
  await g.tap('#lvl-tab-seaside');
  await g.waitFor(() => window.__mr.level === 'seaside' && window.__mr.mode === 'menu' && window.__mr.ready, { timeout: 240000, what: 'Seaside to build' });
  await g.frames(10);
  console.log('seaside', JSON.stringify(await g.snapshot()), (await g.ui('#lvl-name')).value);
  await g.tap('#pick-rally');
  await g.shot('menu-seaside.png', dir);
  await g.tap('#btn-start');
  await g.waitFor(() => window.__mr.race?.state === 'racing' && window.__mr.screen === 'none', { timeout: 60000, what: 'racing' });
  const touch = (type, points) => g.cdp.send('Input.dispatchTouchEvent', { type, touchPoints: points });
  await touch('touchStart', [{ x: 736, y: 200, id: 7 }]);
  await sleep(2500);
  const r = await g.eval('window.__mr.race');
  await g.shot('menu-race.png', dir);
  await touch('touchEnd', []);
  console.log('race', JSON.stringify({ state: r.state, speed: r.speed, throttle: r.input.throttle, fullscreen: (await g.snapshot()).fullscreen }));
  const audio = await g.eval('window.__mr.audio?.context');
  console.log('audio', audio);
  ok = r.state === 'racing' && r.speed > 5 && r.input.throttle === 1 && audio === 'running';
  console.log('errors', g.errors);
  await g.close();
} finally { await browser.close(); }
console.log(ok ? 'PASS' : 'FAIL');
process.exit(ok ? 0 : 1);
