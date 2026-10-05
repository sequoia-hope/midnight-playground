// test/e2e/controls-helpers.js for the Rust build (roadmap WP 6.5, 6.6):
// start a race the way a player does, read the car, and hold on-screen
// pads with real CDP touches. `__race` is `__mr.race` (the car, the
// camera's right as `camRight`, `input.touch` as `touch`), the staging
// (`phys.reset`, `phys.nitro`, `touch.autoGas`) is `__mr.stage`, a pad is
// found by its id (`touch-throttle`, `touch-reset`, `touch-slider`, …).

import { sleep } from './harness.mjs';

export { sleep };

// Wait for secs of race time to pass (a busy GPU slows the simulation).
export async function simWait(game, secs, { timeout = 30000 } = {}) {
  const t0 = await game.eval(() => window.__mr.race.time);
  await game.waitFor(`window.__mr.race.time >= ${t0 + secs}`, { timeout, interval: 25, what: `${secs} s of race time` });
}

// Tap (phone) or click (desktop) Race, then wait for GO.
export async function startRace(game, { how = 'tap', racing = true } = {}) {
  await (how === 'tap' ? game.tap('#btn-start') : game.click('#btn-start'));
  await game.waitFor(() => !!window.__mr.race && ['countdown', 'racing'].includes(window.__mr.race.state)
    && window.__mr.screen === 'none' && window.__mr.mode === 'race', { timeout: 60000, what: 'the race to start' });
  if (racing) await game.waitFor(() => window.__mr.race.state === 'racing', { timeout: 30000, what: 'GO' });
}

// The player's car and controls, as plain numbers.
export function car(game) {
  return game.eval(() => {
    const r = window.__mr.race;
    return {
      speed: r.speed, s: r.s, lat: r.lat, yaw: r.yaw, steerAngle: r.steerAngle, gear: r.gear,
      nitro: r.nitro, nitroActive: r.nitroActive, camMode: r.camMode,
      inp: { ...r.input }, held: r.touch ? { ...r.touch.held } : null,
    };
  });
}

// Put the car on the road at rest, pointing along it, a little past the
// grid, so every test starts the same.
// Also `{ fromFinish: m }` for a place short of the line, `{ latFrac }`
// for a share of the road's half width, `{ yaw }` to turn it.
export function placeCar(game, { ahead = 40, lat = 0, speed = 0, ...more } = {}) {
  return game.eval((c) => window.__mr.stage({ cmd: 'place', ...c }), { ahead, lat, speed, ...more })
    .then(() => game.frames(2));
}

// The page's errors, less Chrome's note that a touchstart could not be
// cancelled: winit cancels every touchstart on the canvas, and Chrome
// sends a touch as not cancelable when the main thread is busy with a
// frame. The canvas is `touch-action: none`, so nothing scrolls or zooms
// either way (DECISIONS D846).
export const pageErrors = (game) => game.errors.filter((e) => !/Ignored attempt to cancel a touchstart event/.test(e));

// Camera-right in the world (x, z): where "right" is on the screen.
export function cameraRight(game) {
  return game.eval(() => window.__mr.race.camRight);
}

// Heading · camera-right: positive when the car points to the right of the
// screen (the JS suites' `Math.cos(yaw) * right.x + Math.sin(yaw) * right.z`).
export async function turnFrom(game, right) {
  const yaw = await game.eval(() => window.__mr.race.yaw);
  return Math.cos(yaw) * right.x + Math.sin(yaw) * right.z;
}

// Centre of a touch pad, e.g. pad('throttle') or pad('pause').
export function pad(game, name) {
  return game.center('#touch-' + name);
}

// Real touches (CDP), then a couple of frames for the client to read them.
export async function touch(game, type, points) {
  await game.cdp.send('Input.dispatchTouchEvent', { type, touchPoints: points.map(({ x, y, id }) => ({ x, y, id })) });
  await game.frames(2);
}

// Fingers down on the named pads (all at once, ids 0, 1, …) or at points.
export async function press(game, ...targets) {
  const pts = [];
  for (const [i, tg] of targets.entries()) {
    const p = typeof tg === 'string' ? await pad(game, tg) : tg;
    pts.push({ x: p.x, y: p.y, id: p.id ?? i });
  }
  await touch(game, 'touchStart', pts);
  return pts;
}

export function lift(game) { return touch(game, 'touchEnd', []); }

// Pick an option in a menu drop-down: open it, tap the option (a tap, so
// an iPhone could take it as the gesture to ask for motion access in).
export async function choose(game, selector, value) {
  await game.tap(selector);
  await game.tap('#option-' + value);
  await game.frames(3);
}

// A thumb on the steering stick: down at (x, y), by default a fifth of the
// way across and 70 % down, as finger `id`.
export async function stickDown(game, { x, y, id = 0 } = {}) {
  const vw = await game.eval('innerWidth'), vh = await game.eval('innerHeight');
  const p = { x: x ?? Math.round(vw * 0.2), y: y ?? Math.round(vh * 0.7), id };
  await touch(game, 'touchStart', [p]);
  return p;
}

// Slide the stick's finger to dx px right of p (other fingers held too).
export function stickTo(game, p, dx, others = []) {
  return touch(game, 'touchMove', [{ x: p.x + dx, y: p.y, id: p.id }, ...others]);
}

// A point on the pedal slider at height u (0 bottom, 1 top), or with
// drift, on the DRIFT strip beside it at that height.
export function sliderPoint(game, u, { drift = false, id = 0 } = {}) {
  return game.eval((u, drift, id) => {
    const t = window.__mr.ui('touch-slider'), d = window.__mr.ui('touch-drift');
    return { x: drift ? d.x + d.w / 2 : t.x + t.w / 2, y: t.y + t.h - t.h * u, id };
  }, u, drift, id);
}

// Whether a control is shown (`el.getClientRects().length > 0`).
export const shown = (game, id) => game.eval((i) => {
  const u = window.__mr.ui(i);
  return !!u && u.visible && u.w > 0 && u.h > 0;
}, id);
