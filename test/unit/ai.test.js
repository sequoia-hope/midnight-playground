// vehicles/AIDriver.js and Kinematic.js: the rivals. Each level's field
// drives its route to the finish (or once round the cruise loop) on the
// tarmac and in a sane time, with collisions resolved as Race does it. Also
// covers overtaking, rubber-banding, holding on the grid, and the track-
// coordinate car underneath (walls, spin, velocity round trips).

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { LEVELS } from '../../src/levels/index.js';
import { Track } from '../../src/track/Track.js';
import { AIDriver } from '../../src/vehicles/AIDriver.js';
import { KinematicCar } from '../../src/vehicles/Kinematic.js';
import { resolveCollisions } from '../../src/vehicles/Collisions.js';
import { makeVehicle, straightTrack, withSeededRandom } from './support/sim.js';

const DT = 1 / 60;

// Rivals as Race.js builds them, on its grid (rows of two, 10 m apart).
function field(track, rivals) {
  return rivals.map((r, i) => {
    const ai = new AIDriver(makeVehicle('rival', 1400), track, { skill: r.skill, name: r.name, power: r.power, bias: (i % 2 ? 1 : -1) * 0.6, lineFactor: 0.8 + (i % 3) * 0.08 });
    const row = Math.floor(i / 2), col = i % 2;
    ai.s = track.startS - 5 - row * 10 - col * 3;
    ai.lat = col ? 2.4 : -2.4;
    ai.writePos();
    return ai;
  });
}

for (const L of LEVELS) {
  test(`${L.id}: the field drives the route on the tarmac`, async () => {
    await withSeededRandom(7, () => {
      const t = new Track(L);
      const rivals = L.rivals.length ? L.rivals : [{ name: 'Solo', skill: 0.95, power: 500 }];
      const ais = field(t, rivals);
      const goal = t.loop ? t.length : Infinity; // once round a loop
      const dist = ais.map(() => 0), prev = ais.map((a) => a.s);
      let time = 0, worstCentre = 0, worstWall = -Infinity, where = '';
      while (time < 600) {
        const ctx = { cars: ais, playerS: ais[0].s, started: true, time };
        for (const a of ais) a.update(DT, ctx);
        resolveCollisions(ais, []);
        for (const a of ais) a.writePos();
        time += DT;
        ais.forEach((a, i) => {
          dist[i] += t.ds(prev[i], a.s); prev[i] = a.s;
          const f = t.frame(a.s);
          // The centre stays on the tarmac (passing may use the shoulder),
          // and the body never goes through a wall.
          if (Math.abs(a.lat) - f.hw > worstCentre) { worstCentre = Math.abs(a.lat) - f.hw; where = `${a.name} at s=${a.s.toFixed(0)}`; }
          worstWall = Math.max(worstWall, a.lat + a.halfW - f.wallR, -a.lat + a.halfW - f.wallL);
          for (const k of ['x', 'y', 'z', 'yaw']) if (!Number.isFinite(a.v[k])) assert.fail(`${a.name}.${k} = ${a.v[k]} at ${time.toFixed(1)} s`);
        });
        if (ais.every((a, i) => a.finished || dist[i] >= goal)) break;
      }
      if (!t.loop) {
        for (const a of ais) {
          assert.ok(a.finished, `${a.name} finished (s ${a.s.toFixed(0)} of ${t.finishS})`);
          const avg = (t.finishS - t.startS) / a.finishTime;
          assert.ok(avg > 25 && avg < 69.5, `${a.name}: average ${avg.toFixed(1)} m/s over ${a.finishTime.toFixed(1)} s`);
        }
      } else {
        dist.forEach((d, i) => assert.ok(d >= goal, `${ais[i].name} went round (${d.toFixed(0)} of ${goal} m)`));
        assert.ok(time < 300, `a lap in ${time.toFixed(0)} s`);
      }
      assert.ok(worstCentre <= 0, `centre ${worstCentre.toFixed(2)} m off the tarmac (${where})`);
      assert.ok(worstWall <= 0.01, `body ${worstWall.toFixed(2)} m into a wall`);
    });
  });
}

test('with a clear road, a more skilful rival finishes sooner', async () => {
  await withSeededRandom(3, () => {
    const t = new Track(LEVELS[0]);
    const times = [0.93, 0.96, 0.99].map((skill) => {
      const [a] = field(t, [{ name: 'X', skill, power: 520 }]);
      let time = 0;
      while (!a.finished && time < 600) { a.update(DT, { cars: [a], playerS: a.s, started: true, time }); time += DT; }
      return a.finishTime;
    });
    assert.ok(times[0] > times[1] && times[1] > times[2], `finish times ${times.map((x) => x.toFixed(1))}`);
  });
});

test('rivals wait on the grid until the start', () => {
  const t = straightTrack(2000);
  const [a] = field(t, [{ name: 'X', skill: 0.95, power: 500 }]);
  const s = a.s;
  for (let i = 0; i < 120; i++) a.update(DT, { cars: [a], playerS: s, started: false, time: 0 });
  assert.equal(a.s, s);
  assert.equal(a.speed, 0);
  a.update(DT, { cars: [a], playerS: s, started: true, time: 0 });
  assert.ok(a.speed > 0, 'goes at GO');
});

test('rubber band: a rival far ahead eases off, one far behind pushes', async () => {
  await withSeededRandom(5, () => {
    const t = straightTrack(4000);
    const run = (gap) => {
      const [a] = field(t, [{ name: 'X', skill: 0.95, power: 500 }]);
      a.s = 500; a.speed = 30; a.writePos();
      a.nitroTimer = 1e9; // no nitro in this comparison
      for (let i = 0; i < 300; i++) a.update(DT, { cars: [a], playerS: a.s - gap, started: true, time: i * DT });
      return a.speed;
    };
    const level = run(0), ahead = run(400), behind = run(-350);
    assert.ok(ahead < level - 2, `400 m ahead of the player: ${ahead.toFixed(1)} vs ${level.toFixed(1)} m/s`);
    assert.ok(behind > level + 2, `350 m behind: ${behind.toFixed(1)} vs ${level.toFixed(1)} m/s`);
  });
});

// A rival coming up on a slower car in its lane, on a long straight.
// Returns how far apart sideways they were while alongside (bodies overlap
// when that is under the sum of their half-widths), and whether it got by.
function overtake({ speed, blockSpeed, gap }) {
  const t = straightTrack(3000);
  const [a] = field(t, [{ name: 'X', skill: 0.95, power: 500 }]);
  a.s = 200; a.lat = 0; a.speed = speed; a.nitroTimer = 1e9; a.writePos();
  const block = new KinematicCar(makeVehicle('rival', 1500), t);
  block.s = 200 + gap; block.lat = 0; block.speed = blockSpeed; block.writePos();
  let closest = Infinity, at = 0;
  for (let i = 0; i < 3000 && a.s < block.s + 50; i++) {
    a.update(DT, { cars: [a, block], playerS: a.s, started: true, time: i * DT });
    block.speed = blockSpeed; block.advance(DT);
    if (Math.abs(a.s - block.s) < a.halfL + block.halfL && Math.abs(a.lat - block.lat) < closest) { closest = Math.abs(a.lat - block.lat); at = a.speed; }
  }
  return { passed: a.s > block.s + 50, closest, need: a.halfW + block.halfW, at };
}

test('a rival steers round a stopped car it sees in time', async () => {
  await withSeededRandom(9, () => {
    const r = overtake({ speed: 30, blockSpeed: 0, gap: 60 });
    assert.ok(r.passed, 'got past');
    assert.ok(r.closest > r.need, `passed ${r.closest.toFixed(2)} m off-centre (needs ${r.need.toFixed(2)})`);
  });
});

// The look-ahead is a fixed 45 m, which at 60+ m/s leaves well under a
// second to move over, and the rival doesn't brake when it can't.
test('at racing speed a rival gets round a slow or stopped car without hitting it', async () => {
  await withSeededRandom(9, () => {
    for (const c of [{ speed: 60, blockSpeed: 25, gap: 400 }, { speed: 60, blockSpeed: 0, gap: 400 }]) {
      const r = overtake(c);
      assert.ok(r.closest > r.need, `rival at ${r.at.toFixed(0)} m/s passing a car doing ${c.blockSpeed} m/s: ${r.closest.toFixed(2)} m apart sideways, bodies overlap under ${r.need.toFixed(2)}`);
    }
  });
});

test('KinematicCar: walls, spin decay and velocity round trips', () => {
  const t = straightTrack(1000);
  const c = new KinematicCar(makeVehicle('rival', 1400), t);
  c.s = 100; c.lat = 0; c.speed = 20; c.frame(); c.writePos();
  // Velocity in world space and back.
  const [vx, vz] = c.velocity();
  assert.ok(Math.abs(vx - 20) < 1e-6 && Math.abs(vz) < 1e-6, 'moving along +x');
  c.setVelocity(15, 2);
  assert.ok(Math.abs(c.speed - 15) < 1e-6 && Math.abs(c.latVel - 2) < 1e-6);
  // Shoved sideways hard: the wall stops it and it bounces back off.
  c.latVel = 30;
  for (let i = 0; i < 60; i++) c.advance(DT);
  const lim = t.wallR[100] - c.halfW - 0.15;
  assert.ok(c.lat <= lim + 1e-6, `lat ${c.lat.toFixed(2)} ≤ ${lim.toFixed(2)}`);
  assert.ok(c.latVel <= 0, 'bounced off the wall');
  // A spin decays back to straight.
  c.addSpin(3);
  assert.ok(c.stunned > 0);
  for (let i = 0; i < 600; i++) c.advance(DT);
  assert.ok(Math.abs(c.spin) < 0.05 && c.stunned === 0, `spin ${c.spin.toFixed(3)}`);
  // translate() moves it in world space.
  c.frame();
  const s = c.s, lat = c.lat;
  c.translate(3, -1);
  assert.ok(Math.abs(c.s - s - 3) < 1e-6 && Math.abs(c.lat - lat + 1) < 1e-6);
  assert.ok(Math.abs(c.v.x - t.pointAt(c.s, c.lat).x) < 1e-3, 'the body follows');
  // Oncoming cars face the other way.
  c.dir = -1; c.latVel = 0; c.writePos();
  assert.ok(Math.abs(Math.abs(c.v.yaw) - Math.PI) < 0.05, `oncoming yaw ${c.v.yaw.toFixed(2)}`);
});

test('KinematicCar: stays on a point-to-point road and wraps round a loop', () => {
  const t = straightTrack(500);
  const c = new KinematicCar(makeVehicle('rival', 1400), t);
  c.s = 480; c.speed = 30;
  for (let i = 0; i < 120; i++) c.advance(DT);
  assert.ok(c.s <= t.roadEnd - 1, `clamped at the end (s ${c.s})`);
  const loop = new Track(LEVELS.find((l) => l.loop));
  const d = new KinematicCar(makeVehicle('rival', 1400), loop);
  d.s = loop.length - 5; d.speed = 30;
  for (let i = 0; i < 60; i++) d.advance(DT);
  assert.ok(d.s >= 0 && d.s < 40, `wrapped to ${d.s.toFixed(1)}`);
});
