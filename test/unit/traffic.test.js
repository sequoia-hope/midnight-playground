// vehicles/Traffic.js and Collisions.js. Traffic is the civilian cars kept
// around the player: they spawn ahead where a level's rules allow, keep to
// lanes, stay off the grid and past-the-finish stretch, and give way. The
// collisions are the car-to-car contacts: separating the cars, the impulse,
// the spin and the events Race reacts to.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { LEVELS } from '../../src/levels/index.js';
import { Track } from '../../src/track/Track.js';
import { KinematicCar } from '../../src/vehicles/Kinematic.js';
import { resolveCollisions, PhysicsBody } from '../../src/vehicles/Collisions.js';
import { makeVehicle, straightTrack } from './support/sim.js';

const { Traffic } = await import('../../src/vehicles/Traffic.js');

const DT = 1 / 30;
const fakeModel = () => ({ root: { visible: false }, dims: { length: 4.6, width: 1.9, height: 1.5, wheelBase: 2.7, wheelRadius: 0.34 } });
const scene = { add() {} };

// A stand-in for the player: a body driving down the road at `speed`.
function player(track, s, speed) {
  return { s, lat: 0, dir: 1, speedAlong: speed, halfW: 0.95, halfL: 2.3, v: { s, lat: 0 } };
}

for (const L of LEVELS) {
  test(`${L.id}: traffic spawns ahead, keeps to the road and to its rules`, () => {
    const t = new Track(L);
    const traffic = new Traffic(t, scene, fakeModel, { level: L, world: {}, count: 22 });
    // Every kind a rule can ask for has cars in the pool.
    for (const r of L.traffic) for (const [k] of r.mix) assert.ok(traffic.pool[k]?.length, `pool has ${k}`);
    const p = player(t, t.startS, 35);
    let seen = 0, maxActive = 0;
    const spawnedAt = new Map();
    const last = t.loop ? t.length * 0.9 : t.finishS - 50;
    while (p.s < last) {
      p.s += p.speedAlong * DT; p.v.s = p.s;
      const agents = [p, ...traffic.cars.filter((c) => c.active)];
      traffic.update(DT, p.s, agents, 0, p.s);
      const active = traffic.cars.filter((c) => c.active);
      maxActive = Math.max(maxActive, active.filter((c) => !c.opposite).length);
      for (const c of active) {
        if (!spawnedAt.has(c)) { spawnedAt.set(c, c.s); seen++; }
        for (const k of ['x', 'y', 'z', 'yaw', 'speed']) if (!Number.isFinite(k === 'speed' ? c.speed : c.v[k])) assert.fail(`${c.kindName}.${k} not finite`);
        if (c.freeLat) continue;
        const f = t.frame(c.s);
        assert.ok(c.lat + c.halfW <= f.wallR + 0.01 && -c.lat + c.halfW <= f.wallL + 0.01, `${c.kindName} inside the walls at s=${c.s.toFixed(0)} (lat ${c.lat.toFixed(2)})`);
        assert.ok(Math.abs(c.lat) < f.hw, `${c.kindName} on the tarmac at s=${c.s.toFixed(0)}`);
        assert.ok(c.speed >= 0 && c.speed < 45, `${c.kindName} speed ${c.speed}`);
      }
      for (const c of traffic.cars) if (!c.active) spawnedAt.delete(c);
    }
    assert.ok(seen > 10, `${seen} cars came and went`);
    assert.ok(maxActive <= 22, `at most count cars at once (${maxActive})`);
  });
}

test('traffic keeps clear of the grid and the stretch past the finish', () => {
  const L = LEVELS[0];
  const t = new Track(L);
  const traffic = new Traffic(t, scene, fakeModel, { level: L, world: {} });
  for (let s = 0; s < t.startS + 250; s += 10) assert.equal(traffic.spawn(s), false, `no spawn at ${s}`);
  for (let s = t.finishS + 110; s < t.length; s += 10) assert.equal(traffic.spawn(s), false, `no spawn at ${s}`);
  let any = false;
  for (let s = 1000; s < 1400 && !any; s += 7) any = traffic.spawn(s);
  assert.ok(any, 'but spawns out on the route');
});

test('the desert lake bed has no traffic', () => {
  const L = LEVELS.find((l) => l.id === 'desert');
  const t = new Track(L);
  const traffic = new Traffic(t, scene, fakeModel, { level: L, world: {} });
  const lake = t.zones[2];
  for (let s = lake.s0 + 5; s < Math.min(lake.s1, t.finishS); s += 13) assert.equal(traffic.spawn(s), false);
});

test('a traffic car slows for a slower car ahead in its lane', () => {
  const t = straightTrack(3000);
  const L = { traffic: [{ gap: [100, 200], mix: [['sedan', 1]], oncoming: 0, speed: [25, 25] }] };
  const traffic = new Traffic(t, scene, fakeModel, { level: L, world: {} });
  const car = traffic.pool.sedan[0];
  traffic.activate(car, 500, 3, 1);
  car.cruise = 25; car.speed = 25; car.writePos();
  const slow = { s: 530, lat: 3, dir: 1, speedAlong: 8, halfW: 0.95 };
  traffic.maxActive = 1;
  for (let i = 0; i < 150; i++) { slow.s += 8 * DT; traffic.update(DT, 500, [slow, car], 0); }
  assert.ok(car.speed < 12, `slowed to ${car.speed.toFixed(1)} m/s`);
  assert.ok(slow.s - car.s > 5, 'no rear-ending');
  assert.equal(car.v.brakeLight, 1, 'brake lights on');
});

test('collisions push overlapping cars apart and trade momentum', () => {
  const t = straightTrack(1000);
  const a = new KinematicCar(makeVehicle('rival', 1400), t);
  const b = new KinematicCar(makeVehicle('rival', 1400), t);
  a.s = 100; a.lat = 0; a.speed = 30; a.frame(); a.writePos();
  b.s = 103.5; b.lat = 0; b.speed = 10; b.frame(); b.writePos();
  const p0 = a.speed + b.speed;
  const events = [];
  resolveCollisions([a, b], events);
  assert.equal(events.length, 1);
  const e = events[0];
  assert.equal(e.type, 'carhit');
  assert.ok(e.strength > 0.3 && e.strength <= 1, `strength ${e.strength}`);
  assert.ok(a.speed < 30 && b.speed > 10, `rear-ended: ${a.speed.toFixed(1)}, ${b.speed.toFixed(1)}`);
  assert.ok(Math.abs(a.speed + b.speed - p0) < 1e-6, 'equal masses: momentum is conserved');
  assert.ok(b.s - a.s > 3.5, 'pushed apart');
  // Separated: a second pass finds no contact.
  a.writePos(); b.writePos();
  const again = [];
  resolveCollisions([a, b], again);
  assert.equal(again.length, 0);
});

// A PIT: nudging the car ahead on one rear corner shoves its tail aside,
// so its nose swings the other way (and the pusher's nose with it).
test('an off-centre hit spins the cars, mirror-symmetrically', () => {
  const t = straightTrack(1000);
  const hit = (side) => {
    const a = new KinematicCar(makeVehicle('rival', 1400), t);
    const b = new KinematicCar(makeVehicle('rival', 1400), t);
    a.s = 100; a.lat = -0.9 * side; a.speed = 30; a.frame(); a.writePos();
    b.s = 103; b.lat = 0.9 * side; b.speed = 5; b.frame(); b.writePos();
    const ev = [];
    resolveCollisions([a, b], ev);
    assert.equal(ev.length, 1, 'they touch');
    return { a, b };
  };
  const r = hit(1), l = hit(-1);
  // b is to the right and hit on its left rear: shoved right, nose left.
  assert.ok(r.b.latVel > 1, `shoved right (${r.b.latVel.toFixed(2)})`);
  assert.ok(r.b.spinRate < -0.1, `nose swings left (${r.b.spinRate.toFixed(2)})`);
  assert.ok(r.b.stunned > 0, 'stunned');
  assert.ok(Math.abs(r.b.spinRate + l.b.spinRate) < 1e-9 && Math.abs(r.a.spinRate + l.a.spinRate) < 1e-9, 'mirror image');
});

// Two cars leaning on each other (door to door, or one pinned to a wall)
// touch every frame at a crawl. That pushes them apart but mustn't spin
// them, or it drowns out the steering.
test('leaning on another car pushes it but does not spin it', () => {
  const t = straightTrack(1000);
  const a = new KinematicCar(makeVehicle('rival', 1400), t);
  const b = new KinematicCar(makeVehicle('rival', 1400), t);
  a.s = 100; a.lat = 0; a.speed = 30; a.latVel = 0.6; a.frame(); a.writePos();
  b.s = 102; b.lat = 1.9; b.speed = 30; b.frame(); b.writePos();
  const ev = [];
  resolveCollisions([a, b], ev);
  assert.equal(ev.length, 1, 'they touch');
  assert.ok(b.latVel > 0.1, `pushed aside (${b.latVel.toFixed(2)})`);
  assert.equal(a.spinRate, 0);
  assert.equal(b.spinRate, 0);
});

test('collisions skip far-apart cars, a car flying over, and traffic among itself', () => {
  const t = straightTrack(1000);
  const mk = (s, lat) => { const c = new KinematicCar(makeVehicle('rival', 1400), t); c.s = s; c.lat = lat; c.speed = 20; c.frame(); c.writePos(); return c; };
  const ev = [];
  resolveCollisions([mk(100, 0), mk(120, 0)], ev);
  assert.equal(ev.length, 0, 'far apart');
  const low = mk(200, 0), high = mk(201, 0);
  high.v.y += 3;
  resolveCollisions([low, high], ev);
  assert.equal(ev.length, 0, 'one over the other');
  const t1 = mk(300, 0), t2 = mk(301, 0);
  t1.kinematicOnly = t2.kinematicOnly = true;
  t1.speed = 30; // closing on t2
  resolveCollisions([t1, t2], ev);
  assert.equal(ev.length, 0, 'traffic cars pass through each other');
  assert.equal(t1.s, 300, 'untouched');
});

test('the player body (PhysicsBody) collides like any other car', () => {
  const t = straightTrack(1000);
  const v = makeVehicle('sports', 1350);
  v.place(t, 100, 0);
  v.vx = 30;
  const phys = { };
  const body = new PhysicsBody(v, phys);
  assert.equal(body.s, 100);
  assert.equal(body.speedAlong, 0, 'speed comes from the vehicle');
  const k = new KinematicCar(makeVehicle('rival', 1400), t);
  k.s = 103.6; k.lat = 0.3; k.speed = 15; k.frame(); k.writePos();
  const ev = [];
  resolveCollisions([body, k], ev);
  assert.equal(ev.length, 1);
  assert.ok(v.vx < 30, `the player lost speed (${v.vx.toFixed(1)})`);
  assert.ok(k.speed > 15, 'and shoved the other car');
  assert.ok(v.yawRate !== 0, 'and got a spin');
});
