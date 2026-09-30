// Seaside Raceway (src/levels/seaside.js + src/levels/seaside/): the
// survey data decodes to the real circuit (its length, its 55 m of climb
// and the Corkscrew's drop), the barriers stand off the tarmac, the lidar
// ground meets the road, the run-off surface joins the road without a step
// and slows a car down, and the rivals rubber-band on race progress, not on
// where they are round the lap.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { levelById } from './support/levels.js';
import { Track, RUNOFF_FLAT } from '../../src/track/Track.js';
import { Terrain } from '../../src/world/Terrain.js';
import { CarPhysics, CAR_SPECS } from '../../src/vehicles/CarPhysics.js';
import { AIDriver } from '../../src/vehicles/AIDriver.js';
import { makeVehicle } from './support/sim.js';

const L = levelById('seaside');
const D = L.data;
const t = new Track(L);
const DT = 1 / 60;

test('the lap is the surveyed centreline: 3.6 km, anticlockwise, three laps', () => {
  assert.ok(Math.abs(t.n - 3602) < 15, `lap ${t.n} m (Laguna Seca: 3,602 m)`);
  assert.ok(Math.abs(t.n - D.lap) < 1, 'the track is as long as the survey');
  assert.equal(t.laps, 3);
  assert.equal(t.startS, 0, 'the lap starts on the line');
  assert.equal(t.finishS, Infinity, 'the finish is by laps, not by distance');
  // Anticlockwise seen from above: in x-east / z-south coordinates the
  // signed area is negative, and most of the turning is to the left.
  let area = 0, left = 0, right = 0;
  for (let i = 0; i < t.n; i++) {
    const j = (i + 1) % t.n;
    area += t.px[i] * t.pz[j] - t.px[j] * t.pz[i];
    if (t.kappa[i] < 0) left -= t.kappa[i]; else right += t.kappa[i];
  }
  assert.ok(area < 0, 'anticlockwise');
  assert.ok(Math.abs(left - right - Math.PI * 2) < 0.05, `net turning is one full turn left (${(left - right).toFixed(3)})`);
});

test('the heights are the lidar\'s: 55 m of climb and the Corkscrew\'s drop', () => {
  let lo = Infinity, hi = -Infinity, top = 0;
  for (let s = 0; s < t.n; s++) {
    lo = Math.min(lo, t.py[s]); hi = Math.max(hi, t.py[s]);
    if (t.py[s] > t.py[top]) top = s;
  }
  // 180 ft from the lowest point of the lap to the highest.
  assert.ok(Math.abs(hi - lo - 54.9) < 1, `height range ${(hi - lo).toFixed(2)} m`);
  // The top of the hill is the Corkscrew, and it drops about 18 m in the
  // next 150 m.
  const cork = t.tags.find((g) => g.tag === 'corkscrew');
  assert.ok(top > cork.s0 - 80 && top < cork.s0 + 20, `the top (s ${top}) is at the Corkscrew (${cork.s0})`);
  const drop = t.py[top + 50] - t.py[top + 200];
  assert.ok(drop > 16 && drop < 21, `Corkscrew drop ${drop.toFixed(1)} m in 150 m`);
  // Real camber, but nothing wild.
  for (let s = 0; s < t.n; s++) assert.ok(Math.abs(t.bank[s]) < 0.16, `bank ${t.bank[s]} at ${s}`);
});

test('the barriers stand off the tarmac, as far out as the real walls', () => {
  let wide = 0;
  for (let s = 0; s < t.n; s++) {
    for (const w of [t.wallL[s], t.wallR[s]]) {
      assert.ok(w >= t.hw[s] + 1.5 - 1e-4, `wall ${w.toFixed(1)} m at s ${s} (hw ${t.hw[s].toFixed(1)})`);
      assert.ok(w <= 34.5, `wall ${w.toFixed(1)} m at s ${s}`);
      if (w > t.hw[s] + 10) wide++;
    }
  }
  // Plenty of room to run wide somewhere, as at the real circuit.
  assert.ok(wide > t.n * 0.3, `${wide} of ${2 * t.n} sides have more than 10 m of run-off`);
});

test('the run-off leaves the road without a step, and the lidar ground meets it', () => {
  for (let s = 0.5; s < t.n; s += 37.3) {
    const f = t.frame(s);
    for (const side of [-1, 1]) {
      // Continuous across the tarmac edge and where the grade takes over.
      for (const lat of [f.hw, f.hw + RUNOFF_FLAT]) {
        const a = t.surfaceY(s, side * (lat - 0.01)), b = t.surfaceY(s, side * (lat + 0.01));
        assert.ok(Math.abs(a - b) < 0.01, `step of ${(b - a).toFixed(3)} m at s ${s.toFixed(1)}, lat ${side * lat}`);
      }
      // pointAt agrees with surfaceY out on the run-off.
      const w = side < 0 ? f.wallL : f.wallR;
      const p = t.pointAt(s, side * (w - 0.5));
      assert.ok(Math.abs(p.y - t.surfaceY(s, side * (w - 0.5))) < 1e-4);
    }
  }
  // The surveyed road sits on the surveyed ground (both from the lidar).
  let worst = 0;
  for (let s = 0; s < t.n; s += 11) worst = Math.max(worst, Math.abs(L.ground(t.px[s], t.pz[s]) - t.py[s]));
  assert.ok(worst < 0.6, `road vs ground on the centreline: worst ${worst.toFixed(2)} m`);
});

test('inside the barriers the terrain is the run-off the car drives on', () => {
  const T = new Terrain(t, L);
  T.buildFields();
  T.resolveFlattens();
  let worst = 0, where = '';
  for (let s = 0; s < t.n; s += 13) {
    const f = t.frame(s);
    for (const side of [-1, 1]) {
      const w = side < 0 ? f.wallL : f.wallR;
      for (let lat = f.hw + 3; lat < Math.min(w - 2, f.hw + 12); lat += 3) {
        const p = t.pointAt(s, side * lat);
        // What the car drives on there: physics projects onto the nearest
        // bit of centreline (inside a tight corner that can be another
        // part of it).
        const q = t.project(p.x, p.z, s);
        const d = t.surfaceY(q.s, q.lat) - T.heightAt(p.x, p.z);
        if (Math.abs(d) > Math.abs(worst)) { worst = d; where = `s ${s} lat ${side * lat} (projects to s ${q.s.toFixed(0)} lat ${q.lat.toFixed(1)})`; }
      }
    }
  }
  // Within half a metre everywhere (a car would visibly float or sink).
  assert.ok(Math.abs(worst) < 0.5, `terrain vs run-off surface: ${worst.toFixed(2)} m at ${where}`);
});

test('the run-off slows the car; the tarmac does not', () => {
  const spec = CAR_SPECS.sports;
  const run = (lat) => {
    const v = makeVehicle('sports', spec.mass);
    const phys = new CarPhysics(v, t, spec);
    // The long climb: straight enough to coast along at a fixed offset.
    const s = 2150;
    phys.reset(s, lat);
    const f = t.frame(s);
    v.vx = f.fx * 40; v.vz = f.fz * 40;
    for (let k = 0; k < 60; k++) phys.update(DT, { throttle: 0, brake: 0, steer: 0, handbrake: false, nitro: false });
    return { speed: Math.hypot(v.vx, v.vz), off: phys.offTrack };
  };
  const on = run(0), off = run(t.hw[2150] + 5);
  assert.equal(on.off, 0, 'on the tarmac');
  assert.equal(off.off, 1, 'out on the run-off');
  assert.ok(on.speed - off.speed > 4, `after a second: ${on.speed.toFixed(1)} m/s on the tarmac, ${off.speed.toFixed(1)} on the run-off`);
});

test('rivals rubber-band on race progress, not on lap position', () => {
  // A rival a lap ahead but just behind the player on the road: it's
  // leading by miles, so it must not get the catch-up boost.
  const mk = () => {
    const a = new AIDriver(makeVehicle('rival', 1400), t, { skill: 0.97, power: 520 });
    a.s = 1000; a.lat = 0; a.speed = 30; a.writePos();
    return a;
  };
  const ahead = mk(), level = mk();
  ahead.prog = 1000 + t.n; level.prog = 1000;
  const ctx = (a) => ({ cars: [a], playerS: 1300, playerProg: 1300, started: true, time: 10 });
  for (let k = 0; k < 240; k++) { ahead.update(DT, ctx(ahead)); level.update(DT, ctx(level)); }
  // Same road, same start: the one far ahead in the race eases off.
  assert.ok(ahead.prog === 1000 + t.n, 'progress is the race\'s to count');
  assert.ok(level.s - ahead.s > 5, `leader eased off: ${ahead.s.toFixed(0)} vs ${level.s.toFixed(0)}`);
});
