// game/Pursuit.js and vehicles/PoliceDriver.js: Hot Pursuit in plain Node.
// Line of sight, heat, the pursuit/cooldown/escape states, busts and the
// penalty hold, spawning, and whole sprint levels driven with police on.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { LEVELS } from '../../src/levels/index.js';
import { Track } from '../../src/track/Track.js';
import { AIDriver } from '../../src/vehicles/AIDriver.js';
import { resolveCollisions } from '../../src/vehicles/Collisions.js';
import { Pursuit, EVADE_TIME, HEAT, bustPenalty, topSpeed } from '../../src/game/Pursuit.js';
import { CAR_SPECS } from '../../src/vehicles/CarPhysics.js';
import { makeVehicle, straightTrack, withSeededRandom } from './support/sim.js';
import { mulberry32 } from '../../src/util/math.js';

const DT = 1 / 60;
const makeUnit = (type) => makeVehicle(type === 'sawhorse' ? 'rally' : 'rival', 1600);

// A racer body for the player: an AIDriver standing in for the physics car
// (the Pursuit reads s, lat, speedAlong, halfW/L, v.x/z/vx/vz and phys).
function playerBody(track, s, lat = 0) {
  const p = new AIDriver(makeVehicle('sports', 1350), track, { skill: 0.95, name: 'You' });
  p.s = s; p.lat = lat; p.writePos();
  p.phys = { spiked: 0 };
  return p;
}

function setup(track, level, opts = {}) {
  const P = playerBody(track, opts.s ?? 400);
  const pu = new Pursuit({ track, level, makeUnit, rng: mulberry32(3), playerTop: 72, ...opts });
  pu.setRacers([{ body: P, player: true, name: 'You' }]);
  return { P, pu };
}

// A 90° corner between two straights.
function cornerTrack() {
  return new Track({
    id: 'test-corner', mode: 'race', startHeight: 0, startHeading: 0, finishRunoff: 180,
    segments: [[800, 0, 0, { zone: 0, road: 'valley' }], [120, 90, 0], [1500, 0, 0]],
    zones: [{ key: 'test', name: 'TEST', landform: 'valley' }],
  });
}

test('topSpeed is where the engine meets the drag', () => {
  const v = topSpeed(CAR_SPECS.sports);
  assert.ok(v > 65 && v < 80, `sports top ${v.toFixed(1)} m/s`);
  assert.ok(topSpeed(CAR_SPECS.rally) <= 71.5);
});

test('line of sight: range, corners and open ground', () => {
  const t = cornerTrack();
  const pu = new Pursuit({ track: t, level: { police: {} }, makeUnit });
  assert.ok(pu.canSee(100, 350), 'straight, 250 m');
  assert.ok(!pu.canSee(100, 450), 'beyond 300 m');
  assert.ok(!pu.canSee(700, 960), 'round the 90° corner');
  assert.ok(pu.canSee(800, 850), 'half way into the corner');
  const open = new Pursuit({ track: t, level: { police: { losOpenGround: [true] } }, makeUnit });
  assert.ok(open.canSee(700, 960), 'open ground sees round it');
});

test('line of sight: a tunnel hides you unless both are in it', () => {
  const sierra = LEVELS.find((l) => l.id === 'sierra');
  const t = new Track(sierra);
  const pu = new Pursuit({ track: t, level: sierra, makeUnit });
  const g = t.tags.find((x) => x.tag === 'tunnel');
  assert.ok(g, 'sierra has a tunnel');
  assert.ok(!pu.canSee(g.s0 - 60, g.s0 + 60), 'outside looking in');
  assert.ok(pu.canSee(g.s0 + 20, g.s0 + 120), 'both inside');
});

test('a parked unit spots a speeding racer and starts a pursuit', () => {
  const t = straightTrack(4000, 'valley');
  const level = { police: {}, traffic: [{ speed: [14, 18] }] };
  const { P, pu } = setup(t, level, { s: 300 });
  P.speed = 40;
  let started = false;
  for (let i = 0; i < 60 * 40 && !started; i++) {
    P.s += P.speed * DT; P.writePos();
    pu.update(DT, { cars: [P, ...pu.bodies()], time: i * DT });
    started = pu.events.some((e) => e.type === 'pursuit');
  }
  assert.ok(started, 'pursuit started');
  assert.equal(pu.state, 'pursuit');
  assert.ok(pu.units.some((u) => u.active && u.mode === 'chase' && u.target === P));
});

test('stopping with two units beside you is a bust within 5 s, then a hold and a release', () => {
  const t = straightTrack(3000, 'freeway');
  const { P, pu } = setup(t, { police: {} }, { s: 800, heat: 2 });
  P.speed = 0;
  pu.state = 'pursuit';
  const [a, b] = pu.units;
  pu.activate(a, 793, 0, 0, 'chase'); a.target = P;
  pu.activate(b, 800, 3, 0, 'chase'); b.target = P;
  let bustAt = null, released = null;
  for (let i = 0; i < 60 * 30; i++) {
    const time = i * DT;
    P.writePos();
    pu.update(DT, { cars: [P, ...pu.bodies()], time });
    resolveCollisions([P, ...pu.bodies()], []);
    for (const e of pu.events) {
      if (e.type === 'busted') bustAt = time;
      if (e.type === 'release') released = { time, spot: e.spot };
    }
    pu.events.length = 0;
    if (released) break;
  }
  assert.ok(bustAt !== null && bustAt < 5, `busted at ${bustAt}`);
  assert.equal(pu.busts, 1);
  assert.ok(released, 'released');
  const held = released.time - bustAt;
  assert.ok(Math.abs(held - bustPenalty(2)) < 0.1, `held ${held.toFixed(2)} s`);
  // The release spot is clear of the units that made the arrest.
  for (const u of [a, b]) if (u.active) assert.ok(released.spot.s > u.s + u.halfL, `release ahead of unit at ${u.s.toFixed(1)}`);
  assert.equal(pu.state, 'patrol', 'the pursuit ends with the bust');
  assert.ok(pu.player.grace > 0, 'grace period after release');
});

test('out of sight round a corner, you escape in evadeTime(heat) ± 1 s', () => {
  const t = cornerTrack();
  for (const heat of [1, 3]) {
    const { P, pu } = setup(t, { police: {} }, { s: 1050, heat });
    pu.state = 'pursuit';
    const u = pu.units[0];
    pu.activate(u, 700, 0, 0, 'chase'); u.target = P;
    let cooldownAt = null, escapedAt = null;
    for (let i = 0; i < 60 * 40 && escapedAt === null; i++) {
      const time = i * DT;
      // Hold 300+ m of road and the corner between you.
      P.s = u.s + 350; P.writePos();
      u.speed = Math.min(u.speed, 5);
      pu.update(DT, { cars: [P, ...pu.bodies()], time });
      for (const e of pu.events) {
        if (e.type === 'cooldown') cooldownAt = time;
        if (e.type === 'escaped') escapedAt = time;
      }
      pu.events.length = 0;
    }
    assert.ok(cooldownAt !== null, 'cooldown');
    assert.ok(escapedAt !== null, 'escaped');
    const took = escapedAt - cooldownAt;
    assert.ok(Math.abs(took - EVADE_TIME[pu.heat]) < 1, `heat ${heat}: escaped after ${took.toFixed(2)} s`);
  }
});

test('heat rises with the meter and stops at the zone cap', () => {
  const t = straightTrack(3000, 'freeway');
  const { pu } = setup(t, { police: { heatCap: [3] } }, { s: 500 });
  pu.state = 'pursuit';
  pu.heatUp(1.05);
  assert.equal(pu.heat, 2);
  pu.heatUp(5);
  assert.equal(pu.heat, 3, 'capped');
  assert.ok(pu.heatMeter < 1);
  pu.state = 'patrol';
  const before = pu.heatMeter;
  pu.heatUp(0.5);
  assert.equal(pu.heatMeter, before, 'no heat out of a pursuit');
});

test('reinforcements: never more active units than the heat allows', () => {
  const t = straightTrack(6000, 'freeway');
  for (const heat of [1, 3, 5]) {
    const { P, pu } = setup(t, { police: {} }, { s: 800, heat });
    pu.state = 'pursuit';
    const u = pu.units[0];
    pu.activate(u, 760, 0, 30, 'chase'); u.target = P;
    P.speed = 30;
    let most = 0;
    for (let i = 0; i < 60 * 60; i++) {
      P.s += P.speed * DT; P.writePos();
      pu.update(DT, { cars: [P, ...pu.bodies()], time: i * DT });
      most = Math.max(most, pu.activeCount());
    }
    assert.ok(most <= HEAT[pu.heat].units, `heat ${heat}: ${most} units`);
    assert.ok(most >= Math.min(2, HEAT[heat].units), `heat ${heat}: reinforcements came (${most})`);
  }
});

test('a spike strip shreds the tyres of a racer that crosses it', () => {
  const t = straightTrack(3000, 'freeway');
  const { P, pu } = setup(t, { police: {} }, { s: 500, heat: 4 });
  pu.state = 'pursuit';
  pu.placeSpikes(600);
  P.lat = (pu.spikes.lat0 + pu.spikes.lat1) / 2;
  P.speed = 40;
  for (let i = 0; i < 60 * 4; i++) { P.s += P.speed * DT; P.writePos(); pu.update(DT, { cars: [P, ...pu.bodies()], time: i * DT }); }
  assert.equal(P.phys.spiked > 0, true);
  assert.ok(pu.events.some((e) => e.type === 'spiked' && e.player));
});

test('a roadblock spans the road with a gap you fit through', () => {
  for (const heat of [3, 5]) {
    const t = straightTrack(3000, 'freeway');
    const { pu } = setup(t, { police: {} }, { s: 500, heat });
    pu.state = 'pursuit';
    pu.placeRoadblock(1000);
    const rb = pu.roadblock;
    assert.ok(rb.cars.length >= 2);
    const f = t.frame(1000);
    assert.ok(rb.gapLat > -f.wallL && rb.gapLat < f.wallR, 'gap on the road');
    for (const c of rb.cars) {
      assert.ok(Math.abs(c.lat - rb.gapLat) > c.halfW + 1, `car at ${c.lat.toFixed(2)} clear of the gap ${rb.gapLat.toFixed(2)}`);
      assert.ok(Math.abs(c.lat) < Math.max(f.wallL, f.wallR), 'inside the walls');
    }
    assert.equal(pu.sawhorses.every((b) => b.active), heat >= 5, 'heavy roadblocks close the gap with barriers');
  }
});

// Whole sprint levels with the police on: rivals as the pack, a stand-in
// player driving the racing line, collisions resolved as the Race does.
for (const L of LEVELS.filter((l) => l.police)) {
  test(`${L.id}: a race with the police on runs clean`, async () => {
    await withSeededRandom(11, () => {
      const t = new Track(L);
      const ais = L.rivals.map((r, i) => {
        const ai = new AIDriver(makeVehicle('rival', 1400), t, { skill: r.skill, name: r.name, power: r.power, bias: (i % 2 ? 1 : -1) * 0.6 });
        ai.s = t.startS - 5 - Math.floor(i / 2) * 10; ai.lat = i % 2 ? 2.4 : -2.4; ai.writePos();
        return ai;
      });
      const P = playerBody(t, t.startS - 25, -2.4);
      P.skill = 0.97;
      const pu = new Pursuit({ track: t, level: L, makeUnit, rng: mulberry32(5), playerTop: 72, heat: 1 });
      pu.setRacers([{ body: P, player: true, name: 'You' }, ...ais.map((a) => ({ body: a, player: false, name: a.name, ai: a }))]);
      const seen = new Set();
      let time = 0, maxHeat = 1, worstWall = -Infinity;
      while (time < 420 && !P.finished) {
        const cars = [P, ...ais, ...pu.bodies()];
        const ctx = { cars, playerS: P.s, started: true, time };
        P.update(DT, ctx);
        for (const a of ais) a.update(DT, ctx);
        pu.update(DT, { cars, time });
        const hits = [];
        resolveCollisions([P, ...ais, ...pu.bodies()], hits);
        for (const h of hits) pu.onHit(h);
        for (const e of pu.events) seen.add(e.type);
        pu.events.length = 0;
        maxHeat = Math.max(maxHeat, pu.heat);
        for (const u of pu.bodies()) {
          for (const k of ['x', 'y', 'z', 'yaw']) if (!Number.isFinite(u.v[k])) assert.fail(`unit ${k} = ${u.v[k]} at ${time.toFixed(1)} s`);
          if (u.police && u.mode !== 'block') { const f = t.frame(u.s); worstWall = Math.max(worstWall, u.lat + u.halfW - f.wallR, -u.lat + u.halfW - f.wallL); }
        }
        time += DT;
      }
      assert.ok(P.finished, `the player finished (s ${P.s.toFixed(0)} of ${t.finishS.toFixed(0)}, ${time.toFixed(0)} s)`);
      assert.ok(seen.has('pursuit'), `a pursuit started (${[...seen].join(', ')})`);
      assert.ok(worstWall < 0.3, `units stay inside the walls (worst ${worstWall.toFixed(2)} m)`);
      assert.ok(maxHeat <= Math.max(...pu.heatCap));
      assert.ok(pu.units.filter((u) => u.active && u.mode === 'chase').length === 0 || P.finished);
    });
  });
}
