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
