// test/e2e/tilt.test.js against the Rust build (roadmap WP 6.6): tilt
// steering on an emulated phone, fed through Chrome's own deviceorientation
// override (DevTools' sensor emulation), so the events arrive the way a
// phone's do: the Steering choice, the note when there's no sensor (the
// thumb stick steers meanwhile), the car turning the way the phone is
// turned in both landscapes and in portrait, the stick giving way to the
// wheel, and the sensitivity slider. The page is https (the harness's
// origin), a secure context, as tilt needs.
//
// `#tilt-sens-row` is the slider `opt-tilt-sens`, `#tilt-note` is
// `__mr.ui('tilt-note').value`, `touch.tilt.state` and `touch.steering`
// are `__mr.race.touch`'s, the wheel's `rotate(…deg)` is `touch.wheel`.
// Also here: leaving the page for another tab pauses the race (WP 6.6's
// visibility pause, with a real hidden page that gets no frames).
//
//   cargo xtask web --release && node --test tools/parity/e2e/tilt.test.mjs

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame } from './harness.mjs';
import {
  sleep, simWait, startRace, car, placeCar, cameraRight, turnFrom, press, lift, choose, stickDown, stickTo, sliderPoint, shown,
} from './controls-helpers.mjs';
import { pose } from '../../../test/unit/support/pose.js';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

const Q = 'timescale=2';
const tiltTo = (game, p) => game.cdp.send('DeviceOrientation.setDeviceOrientationOverride', p);
const tiltState = (game) => game.eval(() => window.__mr.race?.touch?.tilt?.state ?? null);
const note = (game) => game.eval(() => window.__mr.ui('tilt-note')?.value ?? '');
const TILT = { 'mr.steering': 'tilt' };

// Turn the phone by `turn` degrees (held `angle` round) for secs of race
// time from a straight-ahead start. turn > 0 means the car went to the
// right of the screen.
async function tiltTurn(game, turn, secs, angle = 90) {
  await tiltTo(game, pose({ angle, turn: 0 }));
  await game.waitFor(() => window.__mr.race.input.steer === 0 && Math.abs(window.__mr.race.steerAngle) < 1e-3, { what: 'the steering to centre' });
  await placeCar(game, { speed: 20 });
  await simWait(game, 0.05);
  const right = await cameraRight(game);
  await tiltTo(game, pose({ angle, turn }));
  await simWait(game, secs / 2);
  const mid = await car(game);
  await simWait(game, secs / 2);
  await tiltTo(game, pose({ angle, turn: 0 }));
  return { mid, turn: await turnFrom(game, right) };
}

test('phone: tilt is a Steering choice; picked, it saves, shows the slider, and says when there is no sensor', async () => {
  const game = await openGame(browser, { device: 'phone', query: Q });
  try {
    assert.equal((await game.ui('#opt-steer')).value, 'stick', 'the thumb stick by default');
    assert.equal(await shown(game, 'opt-tilt-sens'), false, 'no slider without tilt');
    assert.equal(await shown(game, 'tilt-note'), false);

    await choose(game, '#opt-steer', 'tilt');
    assert.equal(await game.eval(() => localStorage.getItem('mr.steering')), '"tilt"', 'the choice is saved');
    assert.equal(await shown(game, 'opt-tilt-sens'), true, 'the slider shows');
    // Headless Chrome has no sensor: it sends one event of nulls.
    await game.waitFor(() => /No tilt sensor/.test(window.__mr.ui('tilt-note')?.value || ''), { what: 'the no-sensor note' });
    assert.equal(await shown(game, 'tilt-note'), true);

    await startRace(game);
    assert.equal(await tiltState(game), 'none');
    assert.equal(await shown(game, 'touch-stick'), true, 'the stick');
    assert.equal(await shown(game, 'touch-wheel'), false, 'no wheel');
    await placeCar(game, { speed: 15 });
    const p = await stickDown(game);
    await stickTo(game, p, 40);
    await sleep(200);
    assert.ok((await car(game)).inp.steer > 0, 'the stick steers');
    await lift(game);

    // A sensor that answers later takes over.
    await tiltTo(game, pose({ turn: 0 }));
    await game.waitFor(() => window.__mr.race.touch.steering === 'tilt', { what: 'tilt steering to take over' });
    await game.frames(2);
    assert.equal(await shown(game, 'touch-stick'), false, 'stick hidden');
    assert.equal(await shown(game, 'touch-wheel'), true, 'wheel shown');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('phone: turning the phone steers the car that way; gas still works; the wheel shows the lock', async () => {
  // Saved by the old Tilt steer checkbox: carries over as the tilt choice.
  const game = await openGame(browser, { device: 'phone', query: Q, storage: { 'mr.tilt': true } });
  try {
    assert.equal((await game.ui('#opt-steer')).value, 'tilt', 'the old setting carries over');
    await tiltTo(game, pose({ turn: 0 }));
    await game.waitFor(() => (window.__mr.ui('tilt-note')?.value ?? '') === '', { what: 'no note once the sensor answers' });
    await startRace(game);
    assert.equal(await tiltState(game), 'live');
    await game.waitFor(() => window.__mr.race.touch.steering === 'tilt', { what: 'tilt steering' });

    const r = await tiltTurn(game, 20, 0.6);
    assert.ok(r.mid.inp.steer > 0.4 && r.mid.steerAngle > 0, `turned right: steers right (steer ${r.mid.inp.steer.toFixed(2)})`);
    assert.ok(r.turn > 0.1, `the car turns to screen-right (${r.turn.toFixed(2)})`);
    const l = await tiltTurn(game, -20, 0.6);
    assert.ok(l.mid.inp.steer < -0.4 && l.mid.steerAngle < 0, `turned left: steers left (steer ${l.mid.inp.steer.toFixed(2)})`);
    assert.ok(l.turn < -0.1, `the car turns to screen-left (${l.turn.toFixed(2)})`);
    const s = await tiltTurn(game, 1, 0.6);
    assert.equal(s.mid.inp.steer, 0, 'a steady hand near level is straight ahead');
    assert.ok(Math.abs(s.turn) < 0.02, `and the car goes straight (${s.turn.toFixed(3)})`);

    // Full lock turns the wheel a quarter turn.
    await tiltTo(game, pose({ turn: 40 }));
    await game.waitFor(() => window.__mr.race.input.steer > 0.99, { what: 'full lock' });
    await game.waitFor(() => window.__mr.race.touch.wheel === 90, { what: 'the wheel at a quarter turn' });

    // The pedals are unchanged: tilt and gas together.
    await placeCar(game);
    await tiltTo(game, pose({ turn: -15 }));
    await press(game, await sliderPoint(game, 0.72));
    await simWait(game, 0.4);
    const c = await car(game);
    assert.equal(c.inp.throttle, 1);
    assert.ok(c.inp.steer < 0, 'steering by tilt while on the gas');
    await lift(game);

    // The hidden ◂ ▸ pads can't be hit, and a thumb on the left doesn't
    // start the (hidden) stick.
    await tiltTo(game, pose({ turn: 0 }));
    await press(game, { x: 4, y: 4 });
    await sleep(60);
    const held = (await car(game)).held;
    await lift(game);
    assert.ok(!held.left && !held.right, 'a touch in the corner holds no pad');
    await stickDown(game);
    assert.equal(await game.eval(() => window.__mr.race.touch.stick), null, 'no stick while tilting');
    await lift(game);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('phone: the sensitivity slider changes how much lock a tilt gives', async () => {
  const game = await openGame(browser, { device: 'phone', query: Q, storage: TILT });
  try {
    await tiltTo(game, pose({ turn: 0 }));
    // The slider set by a drag to either end (`oninput`).
    const setSens = async (end) => {
      const u = await game.ui('#opt-tilt-sens');
      const y = u.y + u.h / 2, x = end ? u.x + u.w + 30 : u.x - 30;
      const from = { x: u.x + u.w / 2, y, id: 0 };
      await game.cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [from] });
      await game.frames(2);
      await game.cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x, y, id: 0 }] });
      await game.frames(2);
      await game.cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
      await game.frames(2);
    };
    const steerAt = async (end) => {
      await game.eval(() => window.__mr.reveal('opt-tilt-sens'));
      await game.frames(3);
      await setSens(end);
      await startRace(game);
      await tiltTo(game, pose({ turn: 10 }));
      await simWait(game, 0.5);
      const s = (await car(game)).inp.steer;
      await tiltTo(game, pose({ turn: 0 }));
      await game.tap('#touch [data-tap="pause"]');
      await game.waitFor(() => window.__mr.mode === 'paused', { what: 'pause' });
      await game.tap('#btn-quit');
      await game.waitFor(() => window.__mr.mode === 'menu', { what: 'the menu' });
      return s;
    };
    const low = await steerAt(false), high = await steerAt(true);
    assert.ok(low > 0.05 && high > low * 2, `10° of tilt: ${low.toFixed(2)} at the lowest, ${high.toFixed(2)} at the highest`);
    assert.equal(await game.eval(() => localStorage.getItem('mr.tiltSens')), '1', 'saved');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('phone: the other landscape and portrait steer the right way too', async () => {
  let game = await openGame(browser, { device: 'phone', query: Q, storage: TILT });
  try {
    await game.cdp.send('Emulation.setDeviceMetricsOverride', {
      width: 915, height: 412, deviceScaleFactor: 2, mobile: true, screenOrientation: { type: 'landscapeSecondary', angle: 270 },
    });
    assert.equal(await game.eval('window.orientation'), -90);
    await tiltTo(game, pose({ angle: -90, turn: 0 }));
    await startRace(game);
    const r = await tiltTurn(game, 20, 0.6, -90);
    assert.ok(r.mid.inp.steer > 0.4, `steers right (${r.mid.inp.steer.toFixed(2)})`);
    assert.ok(r.turn > 0.1, `the car turns to screen-right (${r.turn.toFixed(2)})`);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }

  game = await openGame(browser, { device: 'phonePortrait', query: Q, storage: TILT });
  try {
    assert.equal(await game.eval('window.orientation'), 0);
    await tiltTo(game, pose({ angle: 0, turn: 0 }));
    await startRace(game);
    const r = await tiltTurn(game, 20, 0.6, 0);
    assert.ok(r.mid.inp.steer > 0.4, `steers right (${r.mid.inp.steer.toFixed(2)})`);
    assert.ok(r.turn > 0.1, `the car turns to screen-right (${r.turn.toFixed(2)})`);
    const l = await tiltTurn(game, -20, 0.6, 0);
    assert.ok(l.turn < -0.1, `and left (${l.turn.toFixed(2)})`);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('desktop: no steering choice or tilt option', async () => {
  const game = await openGame(browser, { device: 'desktop', query: Q });
  try {
    assert.equal(await shown(game, 'opt-steer'), false);
    assert.equal(await shown(game, 'opt-tilt-sens'), false);
    assert.equal(await shown(game, 'tilt-note'), false);
    assert.deepEqual(game.errors, []);
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
    await game.waitFor(() => window.__mr.mode === 'paused', { what: 'the race to pause' });
    assert.equal(await game.screen(), 'pause');
    await other.close();
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});
