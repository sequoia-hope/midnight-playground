// Shared bits for the controls and audio tests: start a race the way a
// player does, read the car, and hold on-screen pads with real touches.

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Wait for secs of race time to pass. Physics checks wait on this, not the
// wall clock: a busy GPU drops the frame rate, and the simulation with it.
export async function simWait(game, secs, { timeout = 30000 } = {}) {
  const t0 = await game.eval(() => window.__race.time);
  await game.waitFor(`window.__race.time >= ${t0 + secs}`, { timeout, interval: 25, what: `${secs} s of race time` });
}

// Tap (phone) or click (desktop) Race, then wait for GO.
export async function startRace(game, { how = 'tap', racing = true } = {}) {
  await (how === 'tap' ? game.tap('#btn-start') : game.click('#btn-start'));
  await game.waitFor(() => window.__race && ['countdown', 'racing'].includes(window.__race.state)
    && document.getElementById('menu').classList.contains('hidden'), { timeout: 30000, what: 'the race to start' });
  if (racing) await game.waitFor(() => window.__race.state === 'racing', { timeout: 30000, what: 'GO' });
}

// The player's car and controls, as plain numbers.
export function car(game) {
  return game.eval(() => {
    const r = window.__race, v = r.player, t = r.track, f = t.frame(v.s);
    const ang = Math.atan2(f.fz, f.fx) - v.yaw;
    return {
      speed: Math.hypot(v.vx, v.vz), along: v.speed, x: v.x, z: v.z, s: v.s, lat: v.lat, hw: f.hw,
      yawToRoad: Math.atan2(Math.sin(ang), Math.cos(ang)), steerAngle: v.steerAngle, gear: r.phys.gear,
      nitro: r.phys.nitro, nitroActive: r.phys.nitroActive, skid: r.phys.skid, camMode: r.cam.mode,
      inp: { ...r.input.state }, held: r.input.touch ? { ...r.input.touch.held } : null,
    };
  });
}

// Put the car on the road at rest, pointing along it, a little past the
// grid (a straight on the default level), so every test starts the same.
export function placeCar(game, { ahead = 40, lat = 0, speed = 0 } = {}) {
  return game.eval((ahead, lat, speed) => {
    const r = window.__race, t = r.track, s = t.startS + ahead;
    r.phys.reset(s, lat);
    const f = t.frame(s);
    r.player.vx = f.fx * speed; r.player.vz = f.fz * speed;
  }, ahead, lat, speed);
}

// Camera-right in the world (x, z): where "right" is on the screen.
export function cameraRight(game) {
  return game.eval(() => { const e = window.__camera.matrixWorld.elements; return { x: e[0], z: e[2] }; });
}

// Centre of a touch pad, e.g. pad('throttle') or pad('pause').
export function pad(game, name) {
  return game.center(`#touch [data-act="${name}"], #touch [data-tap="${name}"]`);
}

// Fingers down on the named pads (all at once, ids 0, 1, …) or at points.
export async function press(game, ...targets) {
  const pts = [];
  for (const [i, tg] of targets.entries()) {
    const p = typeof tg === 'string' ? await pad(game, tg) : tg;
    pts.push({ x: p.x, y: p.y, id: i });
  }
  await game.touch('touchStart', pts);
  return pts;
}

export function lift(game) { return game.touch('touchEnd', []); }

// Hold the named pads for ms, then lift.
export async function hold(game, names, ms) {
  await press(game, ...[].concat(names));
  await sleep(ms);
  await lift(game);
}

// Hold a key for ms.
export async function holdKeys(game, codes, ms) {
  for (const c of [].concat(codes)) await game.page.keyboard.down(c);
  await sleep(ms);
  for (const c of [].concat(codes)) await game.page.keyboard.up(c);
}

// Hold keys for secs of race time.
export async function holdSim(game, codes, secs, mid) {
  for (const c of [].concat(codes)) await game.page.keyboard.down(c);
  await simWait(game, secs / 2);
  const m = mid ? await mid() : undefined;
  await simWait(game, secs / 2);
  for (const c of [].concat(codes)) await game.page.keyboard.up(c);
  return m;
}

// Pick an option in a menu <select> the way a phone's picker does (the
// native picker can't be driven, so set it and send the change).
export function choose(game, selector, value) {
  return game.eval((sel, v) => {
    const el = document.querySelector(sel);
    el.value = v;
    el.dispatchEvent(new Event('change', { bubbles: true }));
    return el.value;
  }, selector, value);
}

// A thumb on the steering stick: down at (x, y), a point on the left of
// the screen (by default a third of the way up from the bottom-left), as
// finger `id`. Returns the point; move it with stickTo.
export async function stickDown(game, { x, y, id = 0 } = {}) {
  const vw = await game.eval('innerWidth'), vh = await game.eval('innerHeight');
  const p = { x: x ?? Math.round(vw * 0.2), y: y ?? Math.round(vh * 0.7), id };
  await game.touch('touchStart', [p]);
  return p;
}

// Slide the stick's finger to dx px right of p (other fingers held too).
export function stickTo(game, p, dx, others = []) {
  return game.touch('touchMove', [{ x: p.x + dx, y: p.y, id: p.id }, ...others]);
}

// A point on the pedal slider at height u (0 bottom, 1 top), or with
// drift, on the DRIFT strip beside it at that height.
export function sliderPoint(game, u, { drift = false, id = 0 } = {}) {
  return game.eval((u, drift, id) => {
    const t = document.querySelector('#touch .t-slider').getBoundingClientRect();
    const d = document.querySelector('#touch .t-drift-strip').getBoundingClientRect();
    return { x: drift ? (d.left + d.right) / 2 : (t.left + t.right) / 2, y: t.bottom - t.height * u, id };
  }, u, drift, id);
}
