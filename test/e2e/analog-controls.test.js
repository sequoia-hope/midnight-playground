// The analogue touch controls (src/game/TouchControls.js), the defaults, on
// an emulated phone with real CDP touches: the thumb stick steering in
// proportion, re-centring past full lock and turning the car the way the
// thumb goes; the pedal slider (brake, coast, gas and N₂O by height, DRIFT
// beside it); and the Steering and Pedals choices on the menu.

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame } from './harness.js';
import { sleep, simWait, startRace, car, placeCar, cameraRight, press, lift, choose, stickDown, stickTo, sliderPoint } from './controls-helpers.js';
import { stickSteer, sliderAt } from '../../src/game/TouchControls.js';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

const Q = 'timescale=2';
const shown = (sel) => { const el = document.querySelector(sel); return !!el && el.getClientRects().length > 0; };
const near = (a, b, eps, msg) => assert.ok(Math.abs(a - b) <= eps, `${msg}: ${a} ≉ ${b}`);
const stick = (game) => game.eval(() => {
  const t = window.__race.input.touch, el = document.querySelector('#touch .t-stick');
  return {
    R: t.stickR, s: t.stick && { ...t.stick }, steer: window.__race.input.state.steer,
    active: el.classList.contains('active'), lock: el.classList.contains('lock'),
    left: el.style.left, knob: document.querySelector('#touch .t-stick-knob').style.transform,
  };
});
const panel = (game) => game.eval(() => {
  const p = document.querySelector('#touch .t-pedal');
  return { classes: [...p.classList].filter((c) => c !== 't-pedal').sort(), gas: p.style.getPropertyValue('--gas'), brk: p.style.getPropertyValue('--brk') };
});
// Slide finger 0 on the slider to height u (or onto DRIFT) and let it land.
async function slideTo(game, u, opts) {
  await game.touch('touchMove', [await sliderPoint(game, u, opts)]);
  await sleep(80);
  return car(game);
}

test('phone: the thumb stick is the default, steers in proportion, and re-centres past full lock', async () => {
  const game = await openGame(browser, { device: 'phone', query: Q });
  try {
    assert.equal(await game.eval(() => document.getElementById('opt-steer').value), 'stick');
    await startRace(game);
    assert.equal(await game.eval(shown, '#touch .t-stick'), true, 'the stick waits bottom-left');
    assert.equal(await game.eval(shown, '#touch [data-act="left"]'), false, 'no ◂ ▸ pads');

    await placeCar(game, { speed: 15 });
    const p = await stickDown(game);
    await sleep(80);
    let st = await stick(game);
    const R = st.R;
    assert.ok(R >= 44 && R <= 84, `full lock at ${R} px`);
    assert.equal(st.steer, 0, 'putting the thumb down steers nowhere');
    assert.equal(st.active, true);
    assert.equal(st.left, `${p.x}px`, 'the stick centres under the thumb');

    await stickTo(game, p, R * 0.5);
    await sleep(80);
    st = await stick(game);
    near(st.steer, stickSteer(0.5), 0.02, 'half travel right');
    await stickTo(game, p, -R * 0.25);
    await sleep(80);
    near((await stick(game)).steer, stickSteer(-0.25), 0.02, 'a quarter left');

    // Past full lock the centre follows the thumb...
    await stickTo(game, p, R * 2);
    await sleep(80);
    st = await stick(game);
    assert.equal(st.steer, 1, 'full lock');
    assert.equal(st.lock, true);
    assert.equal(st.knob, `translateX(${R.toFixed(1)}px)`.replace('.0px', 'px'), 'the knob stops at the end');
    // ...so coming back steers the other way at once.
    await stickTo(game, p, R);
    await sleep(80);
    assert.equal((await stick(game)).steer, 0, 'back by one lock: straight');
    await stickTo(game, p, 0);
    await sleep(80);
    assert.equal((await stick(game)).steer, -1, 'back by two: full left');

    await lift(game);
    await sleep(80);
    st = await stick(game);
    assert.equal(st.steer, 0, 'lifting centres the steering');
    assert.equal(st.active, false);
    assert.equal(st.left, '', 'and the stick goes back to its corner');

    // A thumb right by the left edge still has room for full left lock.
    const e = await stickDown(game, { x: Math.round(R * 0.6) });
    await sleep(80);
    st = await stick(game);
    assert.equal(st.s.x0, R + 4, 'the centre keeps a full lock from the edge');
    assert.ok(st.steer < 0 && st.steer > -0.8, `a little left to start with (${st.steer.toFixed(2)})`);
    await stickTo(game, e, -e.x);
    await sleep(80);
    assert.equal((await stick(game)).steer, -1, 'full left at the edge');
    await lift(game);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('phone: the stick turns the car the way the thumb goes, with the gas under the other thumb', async () => {
  const game = await openGame(browser, { device: 'phone', query: Q });
  try {
    await startRace(game);
    for (const dir of [1, -1]) {
      await placeCar(game, { speed: 20 });
      await game.eval(() => { window.__race.cam.snap = true; });
      await simWait(game, 0.05);
      const right = await cameraRight(game);
      const R = (await stick(game)).R;
      const vw = await game.eval('innerWidth'), vh = await game.eval('innerHeight');
      const p = { x: Math.round(vw * 0.2), y: Math.round(vh * 0.7), id: 0 }, q = await sliderPoint(game, 0.72, { id: 1 });
      await game.touch('touchStart', [p, q]);
      await stickTo(game, p, dir * R * 0.7, [q]);
      await simWait(game, 0.3);
      const mid = await car(game);
      await simWait(game, 0.3);
      await lift(game);
      const yaw = await game.eval(() => window.__race.player.yaw);
      const turn = Math.cos(yaw) * right.x + Math.sin(yaw) * right.z;
      assert.equal(mid.inp.throttle, 1, 'gas held by the other thumb');
      assert.ok(mid.inp.steer * dir > 0.4 && mid.steerAngle * dir > 0, `steering ${dir > 0 ? 'right' : 'left'} (${mid.inp.steer.toFixed(2)})`);
      assert.ok(turn * dir > 0.1, `the car turns to screen-${dir > 0 ? 'right' : 'left'} (${turn.toFixed(2)})`);
    }

    // The stick's thumb keeps steering even over the pedals, and presses
    // none of them; a second thumb on the left doesn't take the stick over.
    const gas = await sliderPoint(game, 0.72);
    const p = await stickDown(game);
    await game.touch('touchMove', [{ x: gas.x, y: gas.y, id: 0 }]);
    await sleep(80);
    let c = await car(game);
    assert.equal(c.held.throttle, false, 'the stick thumb presses no pedal');
    assert.equal(c.inp.steer, 1);
    const first = (await stick(game)).s.id; // Chrome's pointerId, not the CDP touch id
    const q = { x: p.x, y: p.y - 40, id: 1 };
    await game.touch('touchStart', [{ x: gas.x, y: gas.y, id: 0 }, q]);
    await sleep(80);
    const st = await stick(game);
    assert.equal(st.s.id, first, 'still the first thumb');
    assert.equal(st.steer, 1);
    c = await car(game);
    assert.ok(Object.values(c.held).every((h) => !h), 'the second thumb holds nothing');
    await lift(game);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('phone: the pedal slider: brake, coast, gas and N₂O by height, with the fill and knob following', async () => {
  const game = await openGame(browser, { device: 'phone', query: Q });
  try {
    assert.equal(await game.eval(() => document.getElementById('opt-pedals').value), 'slider', 'the slider by default');
    await startRace(game);
    assert.equal(await game.eval(shown, '#touch .t-pedal'), true);
    assert.equal(await game.eval(shown, '#touch [data-act="throttle"]'), false, 'no pedal buttons');
    assert.deepEqual((await panel(game)).classes, [], 'at rest');

    await placeCar(game, { speed: 15 });
    await press(game, await sliderPoint(game, 0.72));
    await sleep(80);
    let c = await car(game), p = await panel(game);
    assert.equal(c.inp.throttle, 1, 'flat out above the mark');
    assert.deepEqual(p.classes, ['active', 'gas']);
    assert.ok(parseFloat(p.gas) > 30, `the gas fill reaches the thumb (${p.gas})`);

    for (const [u, what] of [[0.45, 'light gas'], [0.18, 'light brake']]) {
      c = await slideTo(game, u);
      const want = sliderAt(u);
      near(c.inp.throttle, want.throttle, 0.05, `${what}: gas`);
      near(c.inp.brake, want.brake, 0.05, `${what}: brake`);
      assert.ok(c.inp.throttle < 0.7 && c.inp.brake < 0.7, `${what} is part way`);
    }
    p = await panel(game);
    assert.deepEqual(p.classes, ['active', 'brake']);
    assert.ok(parseFloat(p.brk) > 5 && p.gas === '0%', `the brake fill runs down to the thumb (${p.brk})`);
    c = await slideTo(game, 0.33);
    assert.equal(c.inp.throttle, 0, 'the gap: off the gas');
    assert.equal(c.inp.brake, 0, 'and off the brake');
    c = await slideTo(game, 0.02);
    assert.equal(c.inp.brake, 1, 'the bottom: full brake');

    // The top: N₂O, with the gas flat.
    await placeCar(game, { speed: 20 });
    await game.eval(() => { window.__race.phys.nitro = 1; });
    c = await slideTo(game, 0.92);
    assert.equal(c.inp.throttle, 1);
    assert.equal(c.inp.nitro, true);
    assert.ok((await panel(game)).classes.includes('nitro'));
    await game.waitFor(() => window.__race.phys.nitroActive, { timeout: 5000, what: 'the nitro to fire' });

    await lift(game);
    await sleep(80);
    c = await car(game);
    assert.ok(c.inp.throttle === 0 && c.inp.brake === 0 && !c.inp.nitro, 'lifting lets go of everything');
    assert.deepEqual((await panel(game)).classes, []);

    // Part gas really is part gas: it pulls away slower than flat out.
    const pull = async (u) => {
      await placeCar(game);
      await press(game, await sliderPoint(game, u));
      await simWait(game, 1);
      const v = (await car(game)).speed;
      await lift(game);
      return v;
    };
    const light = await pull(0.42), flat = await pull(0.72);
    assert.ok(light < flat * 0.7, `light gas pulls away slower (${light.toFixed(1)} vs ${flat.toFixed(1)} m/s)`);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('phone: slide right onto DRIFT for the handbrake, still on the pedals; the thumb keeps the slider', async () => {
  const game = await openGame(browser, { device: 'phone', query: Q });
  try {
    await startRace(game);
    await placeCar(game, { speed: 20 });
    await press(game, await sliderPoint(game, 0.72));
    let c = await slideTo(game, 0.72, { drift: true });
    assert.equal(c.held.handbrake, true, 'DRIFT');
    assert.equal(c.inp.handbrake, true);
    assert.equal(c.inp.throttle, 1, 'still flat out');
    assert.ok((await panel(game)).classes.includes('drift'));
    c = await slideTo(game, 0.02, { drift: true });
    assert.ok(c.inp.handbrake && c.inp.brake === 1, 'DRIFT low down: handbrake and brake');
    c = await slideTo(game, 0.72);
    assert.equal(c.inp.handbrake, false, 'back on the slider: no handbrake');

    // Wandering off to the left, or off the top, it's still the pedal thumb.
    const q = await sliderPoint(game, 0.72);
    await game.touch('touchMove', [{ x: q.x - 70, y: q.y, id: 0 }]);
    await sleep(80);
    c = await car(game);
    assert.equal(c.inp.throttle, 1, 'off to the left: still on the gas');
    assert.equal(c.inp.handbrake, false);
    const top = await sliderPoint(game, 1);
    await game.touch('touchMove', [{ x: top.x, y: top.y - 60, id: 0 }]);
    await sleep(80);
    assert.equal((await car(game)).inp.nitro, true, 'off the top: N₂O');
    await lift(game);

    // Auto gas drives until a thumb is on the slider, which then decides.
    await game.eval(() => { window.__race.input.touch.autoGas = true; });
    await sleep(80);
    assert.equal((await car(game)).inp.throttle, 1, 'auto gas with no thumb down');
    await press(game, await sliderPoint(game, 0.33));
    await sleep(80);
    assert.equal((await car(game)).inp.throttle, 0, 'a thumb in the gap coasts');
    await lift(game);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('phone: the menu choices: ◂ ▸ buttons instead of the stick, pedal buttons instead of the slider', async () => {
  const game = await openGame(browser, { device: 'phone', query: Q });
  try {
    await choose(game, '#opt-steer', 'buttons');
    await choose(game, '#opt-pedals', 'buttons');
    assert.equal(await game.eval(() => localStorage.getItem('mr.steering')), '"buttons"');
    assert.equal(await game.eval(() => localStorage.getItem('mr.pedals')), '"buttons"');
    await startRace(game);
    assert.equal(await game.eval(shown, '#touch [data-act="left"]'), true, 'the ◂ ▸ pads');
    assert.equal(await game.eval(shown, '#touch .t-stick'), false, 'no stick');
    assert.equal(await game.eval(shown, '#touch [data-act="throttle"]'), true, 'the pedal buttons');
    assert.equal(await game.eval(shown, '#touch .t-pedal'), false, 'no slider');
    await placeCar(game, { speed: 15 });
    await press(game, 'right', 'throttle');
    await sleep(150);
    const c = await car(game);
    assert.ok(c.inp.steer > 0, 'the pads steer');
    assert.equal(c.inp.throttle, 1, 'GAS is on or off');
    await lift(game);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});
