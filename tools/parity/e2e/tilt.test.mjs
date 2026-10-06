// Tilt and the page's visibility against the Rust build, beyond
// test/e2e/tilt.test.js (which runs against it with `npm run test:e2e:rust`,
// WP 6.7): an iPhone asks for motion access inside the Race tap and inside
// the tap that picks Tilt (D843), and leaving the page for another tab (a
// real hidden page that gets no frames) pauses the race (WP 6.6).
//
//   cargo xtask web --release && node --test tools/parity/e2e/tilt.test.mjs

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame } from './harness.mjs';
import { sleep, startRace, choose, pageErrors } from './controls-helpers.mjs';
import { pose } from '../../../test/unit/support/pose.js';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

const Q = 'timescale=2';
const tiltTo = (game, p) => game.cdp.send('DeviceOrientation.setDeviceOrientationOverride', p);
const TILT = { 'mr.steering': 'tilt' };

// iOS Safari's `DeviceOrientationEvent.requestPermission`, faked: it says
// yes inside a user gesture (`navigator.userActivation.isActive`) and
// rejects outside one, as Safari does.
function fakeIosPermission() {
  window.__asks = [];
  DeviceOrientationEvent.requestPermission = () => {
    const tap = navigator.userActivation.isActive;
    window.__asks.push(tap);
    return tap ? Promise.resolve('granted') : Promise.reject(new DOMException('needs a user gesture', 'NotAllowedError'));
  };
}

test('iPhone: motion access is asked for in the Race tap and in the tap that picks Tilt', async () => {
  let game = await openGame(browser, { device: 'iphone', query: Q, storage: TILT, wait: false });
  try {
    await game.page.evaluateOnNewDocument(fakeIosPermission);
    await game.reload();
    // At load (no tap) it can't ask: the menu says what to do.
    await game.waitFor(() => /Tap Race to allow motion access/.test(window.__mp.ui('tilt-note')?.value || ''), { what: 'the ask note' });
    assert.deepEqual(await game.eval('window.__asks'), [false]);
    await startRace(game, { racing: false });
    await game.waitFor(() => ['waiting', 'none', 'live'].includes(window.__mp.race?.touch?.tilt?.state), { what: 'motion access granted in the tap' });
    const asks = await game.eval('window.__asks');
    assert.ok(asks.includes(true), `asked inside the tap (${JSON.stringify(asks)})`);
    await tiltTo(game, pose({ turn: 0 }));
    await game.waitFor(() => window.__mp.race.touch.tilt.state === 'live', { what: 'tilt live' });
    assert.deepEqual(pageErrors(game), []);
  } finally { await game.close(); }

  game = await openGame(browser, { device: 'iphone', query: Q, wait: false });
  try {
    await game.page.evaluateOnNewDocument(fakeIosPermission);
    await game.reload();
    await choose(game, '#opt-steer', 'tilt');
    await game.waitFor(() => window.__asks.includes(true), { what: 'the Tilt tap to ask' });
    await game.waitFor(() => !/Tap Race/.test(window.__mp.ui('tilt-note')?.value || ''), { what: 'no ask note once granted' });
    await tiltTo(game, pose({ turn: 0 }));
    await startRace(game);
    await game.waitFor(() => window.__mp.race.touch.tilt.state === 'live', { what: 'tilt live' });
    assert.deepEqual(pageErrors(game), []);
  } finally { await game.close(); }
});

test('phone: switching to another tab pauses the race, and it stays paused on return', async () => {
  const game = await openGame(browser, { device: 'phone', query: Q });
  try {
    await startRace(game);
    const other = await game.context.newPage();
    await other.bringToFront();
    await sleep(500);
    await game.page.bringToFront();
    await game.waitFor(() => window.__mp.mode === 'paused', { what: 'the race to pause' });
    assert.equal(await game.screen(), 'pause');
    await other.close();
    assert.deepEqual(pageErrors(game), []);
  } finally { await game.close(); }
});
