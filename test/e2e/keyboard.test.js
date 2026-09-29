// Keyboard driving on a desktop: every key in the README's controls table
// does what it says to the car, the camera and the race, and keys pressed
// on the menu don't carry over into the next race.

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame } from './harness.js';
import { sleep, simWait, startRace, car, placeCar, cameraRight, holdSim } from './controls-helpers.js';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

const Q = 'timescale=2';
const open = () => openGame(browser, { device: 'desktop', query: Q });

// Heading · camera-right after steering from straight ahead at 20 m/s:
// positive means the car turned toward the right of the screen.
async function steerTurn(game, code) {
  await placeCar(game, { speed: 20 });
  await game.eval(() => { window.__race.cam.snap = true; });
  await simWait(game, 0.05);
  const right = await cameraRight(game);
  const mid = await holdSim(game, code, 0.5, () => car(game));
  const yaw = await game.eval(() => window.__race.player.yaw);
  return { mid, turn: Math.cos(yaw) * right.x + Math.sin(yaw) * right.z };
}

test('keys pressed on the menu do nothing to the next race', async () => {
  const game = await open();
  try {
    await game.key('KeyC');
    await game.key('KeyR');
    await game.key('KeyW', 300);
    await sleep(200);
    assert.equal(await game.screen(), 'menu');
    await startRace(game, { how: 'click' });
    await simWait(game, 0.3);
    const c = await car(game);
    assert.equal(c.camMode, 0, 'the race starts on the chase camera (C on the menu is forgotten)');
    assert.ok(c.speed < 0.5, 'the car sits on the grid');
  } finally { await game.close(); }
});

test('W and ↑ accelerate; S and ↓ brake, then reverse', async () => {
  const game = await open();
  try {
    await startRace(game, { how: 'click' });
    for (const code of ['KeyW', 'ArrowUp']) {
      await placeCar(game);
      const mid = await holdSim(game, code, 1.5, () => car(game));
      assert.equal(mid.inp.throttle, 1, `${code} is the throttle`);
      const c = await car(game);
      assert.ok(c.speed > 8, `${code} speeds the car up (${c.speed.toFixed(1)} m/s)`);
    }
    for (const code of ['KeyS', 'ArrowDown']) {
      await placeCar(game, { speed: 25 });
      const v0 = (await car(game)).speed;
      const mid = await holdSim(game, code, 0.5, () => car(game));
      assert.equal(mid.inp.brake, 1, `${code} is the brake`);
      const c = await car(game);
      assert.ok(c.speed < v0 - 5, `${code} brakes (${v0.toFixed(1)} → ${c.speed.toFixed(1)} m/s)`);
      // From rest, holding it backs the car up.
      await placeCar(game);
      await holdSim(game, code, 1.5);
      const r = await car(game);
      assert.equal(r.gear, -1, `${code} from rest selects reverse`);
      assert.ok(r.along < -1, `${code} reverses (${r.along.toFixed(1)} m/s)`);
    }
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('A / D and ← / → steer toward that side of the screen', async () => {
  const game = await open();
  try {
    await startRace(game, { how: 'click' });
    for (const [code, dir] of [['KeyD', 1], ['ArrowRight', 1], ['KeyA', -1], ['ArrowLeft', -1]]) {
      const { mid, turn } = await steerTurn(game, code);
      assert.equal(Math.sign(mid.inp.steer), dir, `${code} steers ${dir > 0 ? 'right' : 'left'} (steer ${mid.inp.steer.toFixed(2)})`);
      assert.equal(Math.sign(mid.steerAngle), dir, `${code} turns the wheels`);
      assert.ok(turn * dir > 0.1, `${code} turns the car toward screen-${dir > 0 ? 'right' : 'left'} (${turn.toFixed(2)})`);
    }
  } finally { await game.close(); }
});

test('Space is the handbrake; Shift and N fire the nitro when the tank has some', async () => {
  const game = await open();
  try {
    await startRace(game, { how: 'click' });

    await placeCar(game, { speed: 25 });
    const hb = await holdSim(game, 'Space', 0.4, () => car(game));
    assert.equal(hb.inp.handbrake, true, 'Space is the handbrake');
    assert.ok(hb.skid > 0.3, `the tyres skid (${hb.skid.toFixed(2)})`);
    assert.ok((await car(game)).speed < 25, 'and it slows the car');

    for (const code of ['ShiftLeft', 'ShiftRight', 'KeyN']) {
      await placeCar(game, { speed: 15 });
      await game.eval(() => { window.__race.phys.nitro = 1; });
      const mid = await holdSim(game, ['KeyW', code], 0.4, () => car(game));
      assert.equal(mid.inp.nitro, true, `${code} is the nitro`);
      assert.equal(mid.nitroActive, true, `${code} fires the nitro`);
      assert.ok((await car(game)).nitro < 1, 'the tank drains');
    }

    await placeCar(game, { speed: 15 });
    await game.eval(() => { window.__race.phys.nitro = 0; });
    const empty = await holdSim(game, ['KeyW', 'KeyN'], 0.4, () => car(game));
    assert.equal(empty.nitroActive, false, 'an empty tank gives nothing');
  } finally { await game.close(); }
});

test('C cycles the camera, B looks back, R puts the car back on the road', async () => {
  const game = await open();
  try {
    await startRace(game, { how: 'click' });

    for (const want of [1, 2, 0]) {
      await game.key('KeyC');
      await game.waitFor(`window.__race.cam.mode === ${want}`, { what: `C to select camera ${want}` });
    }

    // Where the camera sits along the car's heading: behind is negative.
    const camAlong = () => game.eval(() => {
      const v = window.__race.player, p = window.__camera.position;
      return (p.x - v.x) * Math.cos(v.yaw) + (p.z - v.z) * Math.sin(v.yaw);
    });
    await placeCar(game);
    await simWait(game, 0.5);
    assert.ok(await camAlong() < -2, 'the chase camera sits behind the car');
    await game.page.keyboard.down('KeyB');
    await simWait(game, 0.6);
    assert.equal((await car(game)).inp.lookBack, true);
    assert.ok(await camAlong() > 1, 'holding B looks back from in front of the car');
    await game.page.keyboard.up('KeyB');
    await simWait(game, 0.6);
    assert.ok(await camAlong() < -2, 'letting go of B looks forward again');

    // Against the edge and sideways, then R.
    await placeCar(game, { ahead: 60 });
    const s0 = await game.eval(() => {
      const r = window.__race, f = r.track.frame(r.player.s);
      r.phys.reset(r.player.s, f.hw * 0.9);
      r.player.yaw += 1.3;
      return r.player.s;
    });
    await simWait(game, 0.1);
    await game.key('KeyR');
    await game.waitFor(() => {
      const v = window.__race.player, f = window.__race.track.frame(v.s);
      return Math.abs(v.lat) <= f.hw * 0.5 + 0.05;
    }, { what: 'R to put the car back on the road' });
    const c = await car(game);
    assert.ok(Math.abs(c.yawToRoad) < 0.05, `pointing along the road (${c.yawToRoad.toFixed(3)} rad off)`);
    assert.ok(c.speed < 1, 'at rest');
    assert.ok(c.s < s0 && c.s > s0 - 10, 'a few metres back');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('Esc and P pause and resume; nothing moves while paused', async () => {
  const game = await open();
  try {
    await startRace(game, { how: 'click' });
    await placeCar(game, { speed: 20 });
    for (const code of ['Escape', 'KeyP']) {
      await game.key(code);
      await game.waitFor(() => window.__game.mode === 'paused', { what: `${code} to pause` });
      assert.equal(await game.screen(), 'pause');
      await game.waitFor(() => window.__audio.ctx.state === 'suspended', { what: 'the audio to stop' });
      const a = await game.eval(() => ({ s: window.__race.player.s, t: window.__race.time }));
      await game.key('KeyW', 400);
      await sleep(200);
      const b = await game.eval(() => ({ s: window.__race.player.s, t: window.__race.time }));
      assert.deepEqual(b, a, 'the car and the clock stand still while paused');
      await game.key(code);
      await game.waitFor(() => window.__game.mode === 'race', { what: `${code} to resume` });
      assert.equal(await game.screen(), 'none');
      await game.waitFor(() => window.__audio.ctx.state === 'running', { what: 'the audio to restart' });
    }
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});
