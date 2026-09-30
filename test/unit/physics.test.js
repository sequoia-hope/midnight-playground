// vehicles/CarPhysics.js: the player's driving model, run in plain Node on
// a straight, flat test road for every car in CAR_SPECS. It checks the
// launch, top speed, braking into reverse, which way steering turns, nitro,
// the handbrake drift, the countdown lock and the walls, and it checks the
// README's claims about the cars.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { CarPhysics, CAR_SPECS, MOTOR_MAX } from '../../src/vehicles/CarPhysics.js';
import { makeVehicle, straightTrack } from './support/sim.js';

const DT = 1 / 60;
const track = straightTrack(14000);
const input = (o = {}) => ({ throttle: 0, brake: 0, steer: 0, handbrake: false, nitro: false, lookBack: false, ...o });
const speedOf = (v) => Math.hypot(v.vx, v.vz);

function car(kind, { s = 100, lat = 0, speed = 0 } = {}) {
  const spec = CAR_SPECS[kind];
  const v = makeVehicle(kind, spec.mass);
  const phys = new CarPhysics(v, track, spec);
  phys.reset(s, lat);
  v.vx = speed; // the test road runs along +x
  return { v, phys, spec };
}
function drive(c, inp, seconds, each) {
  for (let t = 0; t < seconds; t += DT) { c.phys.update(DT, inp); each?.(t); }
}

// 0–100 km/h time, and top speed after a long run flat out.
const perf = {};
for (const kind of Object.keys(CAR_SPECS)) {
  const c = car(kind);
  let t100 = null, top = 0, t = 0;
  drive(c, input({ throttle: 1 }), 100, () => {
    t += DT;
    const sp = speedOf(c.v);
    if (t100 === null && sp >= 100 / 3.6) t100 = t;
    top = Math.max(top, sp);
  });
  perf[kind] = { t100, top, end: c };
}

for (const kind of Object.keys(CAR_SPECS)) {
  test(`${kind}: accelerates from rest and tops out at a plausible speed`, () => {
    const { t100, top, end } = perf[kind];
    assert.ok(t100 > 1.5 && t100 < 6, `0–100 km/h in ${t100?.toFixed(2)} s`);
    const kmh = top * 3.6;
    assert.ok(kmh > 200 && kmh < 320, `top speed ${kmh.toFixed(0)} km/h`);
    // Held flat out on a straight, it goes straight.
    assert.ok(Math.abs(end.v.lat) < 0.01, `stayed on the centreline (lat ${end.v.lat})`);
    assert.ok(Math.abs(end.v.yaw) < 1e-6);
    if (CAR_SPECS[kind].vmax) assert.ok(top <= CAR_SPECS[kind].vmax + 0.5, `the limiter holds (${top.toFixed(1)} m/s)`);
    // The gearbox worked its way up (the electric car has one gear).
    assert.equal(end.phys.gear, CAR_SPECS[kind].electric ? 1 : 6);
  });

  test(`${kind}: brakes to a stop`, () => {
    const c = car(kind, { speed: 30 });
    const s0 = c.v.s;
    let stoppedAt = null, t = 0;
    drive(c, input({ brake: 1 }), 4, () => { t += DT; if (stoppedAt === null && c.v.speed <= 0.05) stoppedAt = t; });
    assert.ok(stoppedAt !== null && stoppedAt < 3, `stopped from 30 m/s in ${stoppedAt?.toFixed(2)} s`);
    // Braking at ~15 m/s² needs about 30 m.
    assert.ok(c.v.s - s0 < 45, `stopping distance ${(c.v.s - s0).toFixed(1)} m`);
    assert.ok(c.v.brakeLight === 0 || c.v.speed <= 0.5, 'brake lights go off once stopped');
  });

  // README controls: "S / ↓  Brake, then reverse".
  test(`${kind}: holding the brake from a standstill reverses`, () => {
    const c = car(kind);
    drive(c, input({ brake: 1 }), 2);
    assert.ok(c.v.speed < -2, `rolling backwards after 2 s on the brake (${c.v.speed.toFixed(2)} m/s)`);
    assert.equal(c.phys.gear, -1, 'in reverse');
    // Reverse is limited.
    drive(c, input({ brake: 1 }), 6);
    assert.ok(c.v.speed > -14, `reverse speed ${c.v.speed.toFixed(1)} m/s`);
    // Throttle takes it out of reverse.
    drive(c, input({ throttle: 1 }), 4);
    assert.ok(c.v.speed > 0 && c.phys.gear >= 1, 'forwards again');
  });

  test(`${kind}: steering right turns right, left turns left`, () => {
    // Positive yaw turns right; the road's right is +z here.
    const r = car(kind, { speed: 20 });
    drive(r, input({ throttle: 0.4, steer: 1 }), 1);
    assert.ok(r.v.yaw > 0.1, `yaw ${r.v.yaw.toFixed(3)}`);
    assert.ok(r.v.lat > 0.5, `moved right (lat ${r.v.lat.toFixed(2)})`);
    const l = car(kind, { speed: 20 });
    drive(l, input({ throttle: 0.4, steer: -1 }), 1);
    assert.ok(l.v.yaw < -0.1 && l.v.lat < -0.5, 'mirror image');
    assert.ok(Math.abs(l.v.yaw + r.v.yaw) < 1e-6, 'left and right are symmetric');
    // Standing still, the wheel alone doesn't turn the car.
    const s = car(kind);
    drive(s, input({ steer: 1 }), 1);
    assert.equal(s.v.yaw, 0);
  });

  test(`${kind}: nitro adds speed and burns the tank`, () => {
    const a = car(kind, { speed: 25 }), b = car(kind, { speed: 25 });
    const tank = b.phys.nitro;
    let active = false;
    drive(a, input({ throttle: 1 }), 2);
    drive(b, input({ throttle: 1, nitro: true }), 2, () => { active ||= b.phys.nitroActive; });
    assert.ok(active, 'nitroActive while boosting');
    assert.ok(speedOf(b.v) > speedOf(a.v) + 5, `with nitro ${speedOf(b.v).toFixed(1)} vs ${speedOf(a.v).toFixed(1)} m/s`);
    assert.ok(b.phys.nitro < tank, 'the tank drains');
    // An empty tank does nothing.
    const e = car(kind, { speed: 25 });
    e.phys.nitro = 0;
    drive(e, input({ throttle: 1, nitro: true }), 2);
    assert.ok(Math.abs(speedOf(e.v) - speedOf(a.v)) < 1e-6);
    // Nitro needs the throttle.
    const n = car(kind, { speed: 25 });
    drive(n, input({ nitro: true }), 1);
    assert.equal(n.phys.nitroActive, false);
  });
}

test('handbrake at speed starts a drift, and drifting fills the nitro tank', () => {
  const c = car('sports', { speed: 30 });
  c.phys.nitro = 0.2;
  let drifted = false;
  drive(c, input({ throttle: 1, steer: 1, handbrake: true }), 0.6, () => { drifted ||= c.phys.drifting; });
  assert.ok(drifted, 'drifting');
  assert.ok(Math.abs(c.phys.slip) > 0.18, `slip angle ${c.phys.slip.toFixed(2)}`);
  assert.ok(c.phys.skid > 0.3, 'tyres skid');
  const before = c.phys.nitro;
  drive(c, input({ throttle: 1, steer: 1 }), 0.5);
  assert.ok(c.phys.nitro > before, `nitro ${before.toFixed(3)} → ${c.phys.nitro.toFixed(3)}`);
});

test('the car is held still during the countdown', () => {
  const c = car('sports');
  c.phys.locked = true;
  const x = c.v.x;
  drive(c, input({ throttle: 1 }), 2);
  assert.equal(c.v.x, x);
  assert.equal(speedOf(c.v), 0);
  assert.ok(c.phys.rpm > 5000, `revving on the line (${c.phys.rpm.toFixed(0)} rpm)`);
  c.phys.locked = false;
  drive(c, input({ throttle: 1 }), 1);
  assert.ok(speedOf(c.v) > 5, 'goes at GO');
});

test('lifting off coasts down, and it rolls to a stop', () => {
  const c = car('sports', { speed: 20 });
  drive(c, input(), 3);
  const sp = speedOf(c.v);
  assert.ok(sp < 18 && sp > 5, `coasting ${sp.toFixed(1)} m/s after 3 s`);
  drive(c, input(), 30);
  assert.ok(c.v.speed >= 0 && c.v.speed < 1e-3, `stopped, not creeping backwards (${c.v.speed})`);
});

test('the walls keep the car on the road, with an impact event', () => {
  for (const kind of Object.keys(CAR_SPECS)) {
    const c = car(kind, { speed: 35 });
    let maxLat = 0;
    drive(c, input({ throttle: 1, steer: 1 }), 4, () => { maxLat = Math.max(maxLat, c.v.lat); });
    const wall = track.wallR[100];
    assert.ok(maxLat <= wall - c.v.halfW + 0.01, `${kind}: lat ${maxLat.toFixed(2)} with the wall at ${wall.toFixed(2)}`);
    assert.ok(c.phys.events.some((e) => e.type === 'impact'), `${kind}: hit the wall`);
    for (const k of ['x', 'z', 'vx', 'vz', 'yaw']) assert.ok(Number.isFinite(c.v[k]), `${kind}: ${k} finite`);
  }
});

// Pressed into the right-hand wall at `rel` to the road, spinning at
// `yawRate`, for one frame.
function wallFrame(rel, yawRate, steer) {
  const c = car('sports');
  const ext = c.v.halfW * Math.cos(rel) + c.v.halfL * Math.abs(Math.sin(rel));
  c.phys.reset(100, track.wallR[100] - ext);
  c.v.yaw = rel; c.v.vx = 25; c.v.vz = 3; c.v.yawRate = yawRate;
  c.phys.update(DT, input({ throttle: 1, steer }));
  assert.ok(c.phys.scrape > 0, 'on the wall');
  return c.v.yawRate;
}

test('the wall never fights steering off it, but a tail slap still kills the spin', () => {
  // Nose in, turning back out at full lock: the turn carries on.
  const out = wallFrame(0.3, -0.6, -1);
  assert.ok(out < -0.5, `steering off the wall: yaw rate ${out.toFixed(2)}`);
  // Tail swinging into the wall: the wall stops it.
  const slap = wallFrame(-0.4, -2, 0);
  assert.ok(slap > -1, `tail slap: yaw rate ${slap.toFixed(2)}`);
});

test('the ends of a point-to-point road stop the car', () => {
  const c = car('sports', { s: 20, speed: -15 });
  drive(c, input(), 3);
  assert.ok(c.v.s >= 0.5, `didn't back off the start (s ${c.v.s.toFixed(2)})`);
});

test('the electric motor has one gear, a power meter rev counter and regen', () => {
  const c = car('electric', { speed: 30 });
  drive(c, input({ throttle: 1 }), 1);
  assert.equal(c.phys.gear, 1);
  assert.ok(c.phys.rpm > 0 && c.phys.rpm <= MOTOR_MAX * 1.02);
  assert.ok(c.phys.powerOut > 0, 'drawing power');
  const tank = (c.phys.nitro = 0.2);
  drive(c, input({ brake: 0.5 }), 1);
  assert.ok(c.phys.regen > 0.2 && c.phys.powerOut < 0, 'braking regenerates');
  assert.ok(c.phys.nitro > tank, 'regen charges the boost tank');
});

test('the turbo builds boost under throttle and dumps it on a lift', () => {
  const c = car('rally', { speed: 20 });
  drive(c, input({ throttle: 1 }), 2);
  assert.ok(c.phys.boost > 0.5, `boost ${c.phys.boost.toFixed(2)}`);
  drive(c, input(), 0.5);
  assert.ok(c.phys.boost < 0.1, `after lifting ${c.phys.boost.toFixed(2)}`);
});

test('the same inputs give the same drive (deterministic)', () => {
  const run = () => {
    const c = car('muscle', { speed: 10 });
    for (let i = 0; i < 600; i++) c.phys.update(DT, input({ throttle: 1, steer: Math.sin(i / 40), handbrake: i % 200 < 30 }));
    return [c.v.x, c.v.z, c.v.yaw, c.phys.nitro];
  };
  assert.deepEqual(run(), run());
});

// At a steady 70 m/s every frame should move the car speed × frame time, at
// any refresh rate and with the timing jitter a browser adds. The camera and
// every other car move by the frame's time; a car that moves 0, 1 or 3 physics
// steps' worth instead lurches against them.
test('the car moves by each frame’s own time at any refresh rate', () => {
  let seed = 7;
  const rand = () => ((seed = (seed * 16807) % 2147483647) / 2147483647);
  for (const hz of [60, 90, 120, 144]) {
    const c = car('super', { speed: 70 });
    let worst = 0;
    for (let i = 0; i < 400; i++) {
      const dt = (1 + (rand() - 0.5) * 0.06) / hz; // ±3 % jitter
      const x0 = c.v.x, v0 = c.v.vx;
      c.phys.update(dt, input({ throttle: 0.4 }));
      const expect = ((v0 + c.v.vx) / 2) * dt;
      worst = Math.max(worst, Math.abs(c.v.x - x0 - expect) / expect);
    }
    assert.ok(worst < 0.01, `${hz} Hz: a frame's move was off by ${(worst * 100).toFixed(1)} %`);
  }
});

// README "Cars": Kestrel RS "launches hard … but its top speed is lower";
// Ion Arc has "the quickest launch" and "a lower top speed".
test('README: the car table matches CAR_SPECS and its claims hold', () => {
  const readme = fs.readFileSync(new URL('../../README.md', import.meta.url), 'utf8');
  const table = readme.slice(readme.indexOf('## Cars'), readme.indexOf('## Run it'));
  for (const spec of Object.values(CAR_SPECS)) assert.ok(table.includes(`| ${spec.label} |`), `README lists ${spec.label}`);
  const fastest = Object.entries(perf).sort((a, b) => a[1].t100 - b[1].t100);
  assert.equal(fastest[0][0], 'electric', `quickest 0–100: ${fastest.map(([k, p]) => `${k} ${p.t100.toFixed(2)}s`).join(', ')}`);
  for (const k of ['sports', 'muscle', 'super']) {
    assert.ok(perf.rally.t100 < perf[k].t100, `Kestrel launches harder than ${k}`);
    assert.ok(perf.rally.top < perf[k].top, `Kestrel's top speed is below ${k}'s`);
    assert.ok(perf.electric.top < perf[k].top, `Ion Arc's top speed is below ${k}'s`);
  }
});
