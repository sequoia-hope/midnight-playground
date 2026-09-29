// On-screen touch controls (src/game/TouchControls.js), driven with real
// CDP touches on an emulated phone: the pads appear only while racing, each
// pad does what it says, fingers can slide between pads and hold several at
// once, and every pad can actually be hit in landscape and portrait.

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame } from './harness.js';
import { sleep, simWait, startRace, car, placeCar, cameraRight, pad, press, lift } from './controls-helpers.js';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

const Q = 'timescale=2';
const touchShown = () => !document.getElementById('touch').classList.contains('hidden');
const HOLD = ['throttle', 'brake', 'left', 'right', 'handbrake', 'nitro'];
const TAP = ['reset', 'camera', 'pause'];

// Heading · camera-right after steering from a straight-ahead start:
// positive means the car turned to the right of the screen.
async function steerTurn(game, names, secs) {
  await placeCar(game, { speed: 20 });
  await game.eval(() => { window.__race.cam.snap = true; });
  await simWait(game, 0.05);
  const right = await cameraRight(game);
  await press(game, ...names);
  await simWait(game, secs / 2);
  const mid = await car(game);
  await simWait(game, secs / 2);
  await lift(game);
  const yaw = await game.eval(() => window.__race.player.yaw);
  return { mid, turn: Math.cos(yaw) * right.x + Math.sin(yaw) * right.z };
}

test('phone: the pads show only while racing, not on the menu, pause or results', async () => {
  const game = await openGame(browser, { device: 'phone', query: Q });
  try {
    assert.equal(await game.eval(touchShown), false, 'hidden on the menu');
    await startRace(game);
    assert.equal(await game.eval(touchShown), true, 'shown while racing');

    await game.tap('#touch [data-tap="pause"]');
    await game.waitFor(() => window.__game.mode === 'paused', { what: 'the pause pad to pause' });
    assert.equal(await game.screen(), 'pause');
    assert.equal(await game.eval(touchShown), false, 'hidden while paused');

    await game.tap('#btn-resume');
    await game.waitFor(() => window.__game.mode === 'race', { what: 'Resume' });
    assert.equal(await game.eval(touchShown), true, 'back after Resume');

    // Jump to just short of the finish line at speed.
    await game.eval(() => {
      const r = window.__race, t = r.track, s = t.finishS - 30;
      r.phys.reset(s, 0);
      const f = t.frame(s);
      r.player.vx = f.fx * 40; r.player.vz = f.fz * 40;
    });
    await game.waitFor(() => window.__game.mode === 'results', { timeout: 30000, what: 'the results screen' });
    assert.equal(await game.eval(touchShown), false, 'hidden on the results');

    await game.tap('#btn-menu');
    await game.waitFor(() => window.__game.mode === 'menu', { what: 'the menu' });
    assert.equal(await game.eval(touchShown), false, 'hidden back on the menu');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('phone: GAS, BRAKE, steering and N₂O drive the car', async () => {
  const game = await openGame(browser, { device: 'phone', query: Q });
  try {
    await startRace(game);

    // GAS.
    await placeCar(game);
    await press(game, 'throttle');
    await sleep(150);
    let c = await car(game);
    assert.equal(c.held.throttle, true, 'GAS is held');
    assert.equal(c.inp.throttle, 1, 'GAS reaches the input');
    await simWait(game, 1.5);
    await lift(game);
    c = await car(game);
    assert.ok(c.speed > 8, `GAS speeds the car up (speed ${c.speed.toFixed(1)} m/s)`);
    assert.ok(Object.values(c.held).every((h) => !h), 'lifting releases every pad');

    // BRAKE: from speed, much harder than just coasting.
    await placeCar(game, { speed: 25 });
    const v0 = (await car(game)).speed;
    await press(game, 'brake');
    await sleep(100);
    c = await car(game);
    assert.equal(c.held.brake, true);
    assert.equal(c.inp.brake, 1);
    await simWait(game, 0.5);
    await lift(game);
    c = await car(game);
    assert.ok(c.speed < v0 - 6, `BRAKE slows the car (${v0.toFixed(1)} → ${c.speed.toFixed(1)} m/s)`);

    // Steering: right turns toward the right of the screen, left to the left.
    const r = await steerTurn(game, ['right'], 0.5);
    assert.equal(r.mid.held.right, true);
    assert.ok(r.mid.inp.steer > 0 && r.mid.steerAngle > 0, `right steers right (steer ${r.mid.inp.steer})`);
    assert.ok(r.turn > 0.1, `the car turns to screen-right (${r.turn.toFixed(2)})`);
    const l = await steerTurn(game, ['left'], 0.5);
    assert.equal(l.mid.held.left, true);
    assert.ok(l.mid.inp.steer < 0 && l.mid.steerAngle < 0, `left steers left (steer ${l.mid.inp.steer})`);
    assert.ok(l.turn < -0.1, `the car turns to screen-left (${l.turn.toFixed(2)})`);

    // N₂O on its own holds the gas down, and fires the nitro once moving.
    await placeCar(game);
    await game.eval(() => { window.__race.phys.nitro = 1; });
    await press(game, 'nitro');
    await sleep(150);
    c = await car(game);
    assert.equal(c.held.nitro, true);
    assert.equal(c.held.throttle, false);
    assert.equal(c.inp.throttle, 1, 'N₂O holds the gas');
    assert.equal(c.inp.nitro, true);
    await game.waitFor(() => window.__race.phys.nitroActive, { timeout: 5000, what: 'the nitro to fire' });
    await lift(game);
    c = await car(game);
    assert.ok(c.nitro < 1, 'the nitro tank drains');
    assert.ok(c.speed > 4);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('phone: a finger slides from GAS to BRAKE; steer and gas together', async () => {
  const game = await openGame(browser, { device: 'phone', query: Q });
  try {
    await startRace(game);
    await placeCar(game);

    // One finger: down on GAS, slide onto BRAKE without lifting.
    const [p] = await press(game, 'throttle');
    await sleep(100);
    assert.equal((await car(game)).held.throttle, true);
    const brake = await pad(game, 'brake');
    await game.touch('touchMove', [{ x: (p.x + brake.x) / 2, y: (p.y + brake.y) / 2, id: 0 }]);
    await game.touch('touchMove', [{ x: brake.x, y: brake.y, id: 0 }]);
    await sleep(100);
    let c = await car(game);
    assert.equal(c.held.throttle, false, 'GAS lets go when the finger leaves it');
    assert.equal(c.held.brake, true, 'BRAKE picks the finger up');
    assert.equal(c.inp.brake, 1);
    await lift(game);

    // Left to right on the steering pads, same finger.
    const [q] = await press(game, 'left');
    await sleep(100);
    assert.equal((await car(game)).held.left, true);
    const right = await pad(game, 'right');
    await game.touch('touchMove', [{ x: right.x, y: q.y, id: 0 }]);
    await sleep(100);
    c = await car(game);
    assert.equal(c.held.left, false);
    assert.equal(c.held.right, true);
    await lift(game);

    // Two thumbs: steer left and hold the gas at the same time.
    await placeCar(game);
    await press(game, 'left', 'throttle');
    await simWait(game, 0.3);
    c = await car(game);
    assert.equal(c.held.left, true, 'left thumb held');
    assert.equal(c.held.throttle, true, 'right thumb held');
    assert.equal(c.inp.throttle, 1);
    assert.ok(c.inp.steer < 0, 'steering while on the gas');
    await lift(game);
    c = await car(game);
    assert.ok(Object.values(c.held).every((h) => !h));
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('phone: Auto gas drives with no finger down, and BRAKE overrides it', async () => {
  const game = await openGame(browser, { device: 'phone', query: Q });
  try {
    assert.equal(await game.eval(() => document.getElementById('opt-autogas').checked), false, 'off by default');
    await game.tap('#opt-autogas');
    assert.equal(await game.eval(() => document.getElementById('opt-autogas').checked), true);
    assert.equal(await game.eval(() => localStorage.getItem('mr.autogas')), 'true', 'the choice is saved');
    await startRace(game);
    await placeCar(game);
    await simWait(game, 1);
    let c = await car(game);
    assert.equal(c.inp.throttle, 1, 'gas on with no finger down');
    assert.ok(c.speed > 5, `the car drives itself forward (${c.speed.toFixed(1)} m/s)`);
    await press(game, 'brake');
    await sleep(200);
    c = await car(game);
    assert.equal(c.inp.throttle, 0, 'BRAKE lifts the auto gas');
    assert.equal(c.inp.brake, 1);
    await lift(game);
    await sleep(100);
    assert.equal((await car(game)).inp.throttle, 1, 'auto gas back after BRAKE');
  } finally { await game.close(); }
});

test('phone: the reset, camera and pause pads', async () => {
  const game = await openGame(browser, { device: 'phone', query: Q });
  try {
    await startRace(game);

    assert.equal((await car(game)).camMode, 0);
    await game.tap('#touch [data-tap="camera"]');
    await game.waitFor(() => window.__race.cam.mode === 1, { what: 'the camera pad to change camera' });
    await game.tap('#touch [data-tap="camera"]');
    await game.waitFor(() => window.__race.cam.mode === 2, { what: 'the next camera' });

    // Knock the car against the edge, sideways, then reset it.
    await placeCar(game, { ahead: 60 });
    const before = await game.eval(() => {
      const r = window.__race, f = r.track.frame(r.player.s);
      r.phys.reset(r.player.s, f.hw * 0.9);
      r.player.yaw += 1.3;
      return { s: r.player.s };
    });
    await sleep(100);
    await game.tap('#touch [data-tap="reset"]');
    await game.waitFor(() => {
      const v = window.__race.player, f = window.__race.track.frame(v.s);
      return Math.abs(v.lat) <= f.hw * 0.5 + 0.05;
    }, { what: 'the reset pad to put the car back on the road' });
    const c = await car(game);
    assert.ok(Math.abs(c.yawToRoad) < 0.05, `pointing along the road (${c.yawToRoad.toFixed(3)} rad off)`);
    assert.ok(c.speed < 1, 'at rest');
    assert.ok(c.s < before.s && c.s > before.s - 10, 'a few metres back');

    await game.tap('#touch [data-tap="pause"]');
    await game.waitFor(() => window.__game.mode === 'paused', { what: 'the pause pad to pause' });
    assert.equal(await game.screen(), 'pause');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

for (const device of ['phone', 'phonePortrait']) {
  test(`${device}: every pad is on screen, uncovered, and answers at its centre`, async () => {
    const game = await openGame(browser, { device, query: Q });
    try {
      await startRace(game);
      const rects = await game.eval((names) => names.map((n) => {
        const el = document.querySelector(`#touch [data-act="${n}"], #touch [data-tap="${n}"]`);
        const b = el.getBoundingClientRect();
        return { n, l: b.left, t: b.top, r: b.right, b: b.bottom };
      }), [...HOLD, ...TAP]);
      const vw = await game.eval('innerWidth'), vh = await game.eval('innerHeight');
      for (const r of rects) {
        assert.ok(r.l >= 0 && r.t >= 0 && r.r <= vw && r.b <= vh, `${r.n} is fully on screen`);
        assert.ok(r.r - r.l >= 40 && r.b - r.t >= 40, `${r.n} is big enough for a thumb`);
      }
      for (const a of rects) for (const b of rects) {
        if (a.n < b.n) assert.ok(a.r <= b.l || b.r <= a.l || a.b <= b.t || b.b <= a.t, `${a.n} and ${b.n} don't overlap`);
      }
      // game.center() throws if anything else sits on the pad's centre.
      for (const n of TAP) await pad(game, n);
      for (const n of HOLD) {
        await press(game, n);
        await sleep(60);
        const held = (await car(game)).held;
        await lift(game);
        await sleep(30);
        assert.deepEqual(Object.keys(held).filter((k) => held[k]), [n], `a touch on ${n} holds ${n} and nothing else`);
      }
      assert.deepEqual(game.errors, []);
    } finally { await game.close(); }
  });
}

test('desktop: no touch overlay; ?touch=1 and ?touch=0 force it on and off', async () => {
  let game = await openGame(browser, { device: 'desktop', query: Q });
  try {
    await startRace(game, { how: 'click' });
    assert.equal((await game.snapshot()).touchUI, false);
    assert.equal(await game.eval(touchShown), false, 'no pads on a desktop');
    assert.equal(await game.eval(() => window.__race.input.touch), null);
  } finally { await game.close(); }

  game = await openGame(browser, { device: 'desktop', query: Q + '&touch=1' });
  try {
    assert.equal((await game.snapshot()).touchUI, true, '?touch=1 turns the touch UI on');
    await startRace(game, { how: 'click' });
    assert.equal(await game.eval(touchShown), true);
  } finally { await game.close(); }

  game = await openGame(browser, { device: 'phone', query: Q + '&touch=0' });
  try {
    assert.equal((await game.snapshot()).touchUI, false, '?touch=0 turns it off on a phone');
    await startRace(game);
    assert.equal(await game.eval(touchShown), false);
  } finally { await game.close(); }
});
