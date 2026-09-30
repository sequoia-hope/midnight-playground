// Tilt steering (src/game/TiltSteer.js) on an emulated phone, fed through
// Chrome's own deviceorientation override (DevTools' sensor emulation), so
// the events arrive the way a phone's do: the Steering choice, the note
// when there's no sensor (the thumb stick steers meanwhile), the car
// turning the way the phone is turned in both landscapes and in portrait,
// the stick giving way to the wheel, and the sensitivity slider.

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame } from './harness.js';
import { sleep, simWait, startRace, car, placeCar, cameraRight, press, lift, choose, stickDown, stickTo, sliderPoint } from './controls-helpers.js';
import { pose } from '../unit/support/pose.js';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

const Q = 'timescale=2';
const shown = (sel) => { const el = document.querySelector(sel); return !!el && el.getClientRects().length > 0; };
const tiltTo = (game, p) => game.cdp.send('DeviceOrientation.setDeviceOrientationOverride', p);
const tiltState = (game) => game.eval(() => window.__race?.input.touch.tilt.state ?? null);
const TILT = { 'mr.steering': 'tilt' };

// Turn the phone by `turn` degrees (held `angle` round) for secs of race
// time from a straight-ahead start. turn > 0 means the car went to the
// right of the screen.
async function tiltTurn(game, turn, secs, angle = 90) {
  // Level first, and let the last turn wash out of the input and the wheels.
  await tiltTo(game, pose({ angle, turn: 0 }));
  await game.waitFor(() => window.__race.input.state.steer === 0 && Math.abs(window.__race.player.steerAngle) < 1e-3, { what: 'the steering to centre' });
  await placeCar(game, { speed: 20 });
  await game.eval(() => { window.__race.cam.snap = true; });
  await simWait(game, 0.05);
  const right = await cameraRight(game);
  await tiltTo(game, pose({ angle, turn }));
  await simWait(game, secs / 2);
  const mid = await car(game);
  await simWait(game, secs / 2);
  await tiltTo(game, pose({ angle, turn: 0 }));
  const yaw = await game.eval(() => window.__race.player.yaw);
  return { mid, turn: Math.cos(yaw) * right.x + Math.sin(yaw) * right.z };
}

test('phone: tilt is a Steering choice; picked, it saves, shows the slider, and says when there is no sensor', async () => {
  const game = await openGame(browser, { device: 'phone', query: Q });
  try {
    assert.equal(await game.eval(() => document.getElementById('opt-steer').value), 'stick', 'the thumb stick by default');
    assert.equal(await game.eval(shown, '#tilt-sens-row'), false, 'no slider without tilt');
    assert.equal(await game.eval(shown, '#tilt-note'), false);

    await choose(game, '#opt-steer', 'tilt');
    assert.equal(await game.eval(() => localStorage.getItem('mr.steering')), '"tilt"', 'the choice is saved');
    assert.equal(await game.eval(shown, '#tilt-sens-row'), true, 'the slider shows');
    // Headless Chrome has no sensor: it sends one event of nulls.
    await game.waitFor(() => /No tilt sensor/.test(document.getElementById('tilt-note').textContent), { what: 'the no-sensor note' });
    assert.equal(await game.eval(shown, '#tilt-note'), true);

    // The race falls back to the thumb stick.
    await startRace(game);
    assert.equal(await tiltState(game), 'none');
    assert.equal(await game.eval(shown, '#touch .t-stick'), true, 'the stick');
    assert.equal(await game.eval(shown, '#touch .t-wheel'), false, 'no wheel');
    await placeCar(game, { speed: 15 });
    const p = await stickDown(game);
    await stickTo(game, p, 40);
    await sleep(200);
    assert.ok((await car(game)).inp.steer > 0, 'the stick steers');
    await lift(game);

    // A sensor that answers later takes over.
    await tiltTo(game, pose({ turn: 0 }));
    await game.waitFor(() => window.__race.input.touch.steering === 'tilt', { what: 'tilt steering to take over' });
    assert.equal(await game.eval(shown, '#touch .t-stick'), false, 'stick hidden');
    assert.equal(await game.eval(shown, '#touch .t-wheel'), true, 'wheel shown');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('phone: turning the phone steers the car that way; gas still works; the wheel shows the lock', async () => {
  // Saved by the old Tilt steer checkbox: carries over as the tilt choice.
  const game = await openGame(browser, { device: 'phone', query: Q, storage: { 'mr.tilt': true } });
  try {
    assert.equal(await game.eval(() => document.getElementById('opt-steer').value), 'tilt', 'the old setting carries over');
    await tiltTo(game, pose({ turn: 0 }));
    await game.waitFor(() => document.getElementById('tilt-note').textContent === '', { what: 'no note once the sensor answers' });
    await startRace(game);
    assert.equal(await tiltState(game), 'live');
    await game.waitFor(() => window.__race.input.touch.steering === 'tilt', { what: 'tilt steering' });

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
    await game.waitFor(() => window.__race.input.state.steer > 0.99, { what: 'full lock' });
    const wheel = await game.eval(() => document.querySelector('#touch .t-wheel').style.transform);
    assert.equal(wheel, 'rotate(90deg)');

    // The pedals are unchanged: tilt and gas together.
    await placeCar(game);
    await tiltTo(game, pose({ turn: -15 }));
    await press(game, await sliderPoint(game, 0.72));
    await simWait(game, 0.4);
    const c = await car(game);
    assert.equal(c.inp.throttle, 1);
    assert.ok(c.inp.steer < 0, 'steering by tilt while on the gas');
    await lift(game);

    // The hidden ◂ ▸ pads can't be hit (a zero-sized box at 0, 0), and a
    // thumb on the left doesn't start the (hidden) stick.
    await tiltTo(game, pose({ turn: 0 }));
    await press(game, { x: 4, y: 4 });
    await sleep(60);
    const held = (await car(game)).held;
    await lift(game);
    assert.ok(!held.left && !held.right, 'a touch in the corner holds no pad');
    await stickDown(game);
    assert.equal(await game.eval(() => window.__race.input.touch.stick), null, 'no stick while tilting');
    await lift(game);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('phone: the sensitivity slider changes how much lock a tilt gives', async () => {
  const game = await openGame(browser, { device: 'phone', query: Q, storage: TILT });
  try {
    await tiltTo(game, pose({ turn: 0 }));
    const steerAt = async (value) => {
      await game.eval((v) => {
        const el = document.getElementById('opt-tilt-sens');
        el.value = v;
        el.dispatchEvent(new Event('input'));
      }, value);
      await startRace(game);
      await tiltTo(game, pose({ turn: 10 }));
      await simWait(game, 0.5);
      const s = (await car(game)).inp.steer;
      await tiltTo(game, pose({ turn: 0 }));
      await game.tap('#touch [data-tap="pause"]');
      await game.waitFor(() => window.__game.mode === 'paused', { what: 'pause' });
      await game.tap('#btn-quit');
      await game.waitFor(() => window.__game.mode === 'menu', { what: 'the menu' });
      return s;
    };
    const low = await steerAt(0), high = await steerAt(100);
    assert.ok(low > 0.05 && high > low * 2, `10° of tilt: ${low.toFixed(2)} at the lowest, ${high.toFixed(2)} at the highest`);
    assert.equal(await game.eval(() => localStorage.getItem('mr.tiltSens')), '1', 'saved');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

// The same physical right turn, with the picture turned the other way.
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
    assert.equal(await game.eval(() => document.getElementById('opt-steer').closest('label').getClientRects().length), 0);
    assert.equal(await game.eval(shown, '#tilt-sens-row'), false);
    assert.equal(await game.eval(shown, '#tilt-note'), false);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});
