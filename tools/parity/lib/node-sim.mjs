// Staging the simulation in plain Node for the module oracle (roadmap
// WP 0.4, SPEC 4.6): the JS game's own modules (CarPhysics, AIDriver,
// Traffic, Collisions, Pursuit, ...) on the real levels, with the vehicle
// dimensions and world data the game uses (parity/golden/sim/world-data.json),
// stepped in the order Race.update calls them, at a fixed 1/120 s.
//
// The parity kernel must be installed before this module loads: run tools
// with NODE_OPTIONS=--import=./tools/parity/kernel/register.mjs.
// The 'three' resolve hook lets Node import modules that import three.js.

import '../../../test/unit/support/three.js';
import fs from 'node:fs';
import path from 'node:path';
import { ROOT } from './jstree.mjs';

if (!Math.__kernel) throw new Error('node-sim: the parity kernel is not installed (NODE_OPTIONS=--import=./tools/parity/kernel/register.mjs)');

const { LEVELS } = await import('../../../test/unit/support/levels.js');
const { Track } = await import('../../../src/track/Track.js');
const { Vehicle } = await import('../../../src/vehicles/Vehicle.js');
const { CarPhysics, CAR_SPECS } = await import('../../../src/vehicles/CarPhysics.js');
const { AIDriver } = await import('../../../src/vehicles/AIDriver.js');
const { Traffic } = await import('../../../src/vehicles/Traffic.js');
const { resolveCollisions, PhysicsBody } = await import('../../../src/vehicles/Collisions.js');
const { Pursuit, topSpeed } = await import('../../../src/game/Pursuit.js');
const { UNIT_TYPES } = await import('../../../src/vehicles/PoliceDriver.js');
const { autopilot } = await import('../../../src/game/autopilot.js');
const { simStreams, quantiseInput } = await import('../../../src/parity/sim.js');

export { CAR_SPECS, Pursuit, resolveCollisions, autopilot, quantiseInput, UNIT_TYPES };

export const DT = 1 / 120;
export const WORLD = JSON.parse(fs.readFileSync(path.join(ROOT, 'parity/golden/sim/world-data.json'), 'utf8'));

// The opposite carriageway's height: city/freeway.js oppY, with its HALF.
// Ported as a formula (SPEC 4.3); duplicated here because freeway.js needs
// a DOM to import.
const HALF = 9.4;
const oppY = (f) => f.y + Math.max(0.04, HALF * f.bank - 0.2);

// A level's Track with what the world's scenery sets on it (runout), and
// the world object Traffic reads (the opposite carriageway).
export function level(id) {
  const L = LEVELS.find((l) => l.id === id);
  if (!L) throw new Error('no level ' + id);
  const t = new Track(L);
  const wd = WORLD.levels[id];
  t.runout = wd.track.runout;
  const world = {};
  if (wd.oppositeCarriageway) {
    const oc = wd.oppositeCarriageway, tmp = {};
    world.oppositeCarriageway = { s0: oc.s0, s1: oc.s1, lanes: oc.lanes.slice(), dir: oc.dir, y: (s) => oppY(t.frame(s, tmp)) };
  }
  return { L, t, world };
}

// A model with only what physics and AI read: the game's dimensions for
// this kind at this detail level ('high' for racers, 'lowFar' for traffic
// and police), a root, and the far-model switch Traffic calls.
export function model(kind, lod = 'high') {
  const dims = WORLD.kinds[kind]?.[lod];
  if (!dims) throw new Error(`no dims for ${kind} ${lod}`);
  return { dims: { ...dims }, root: { visible: true }, setFar() {}, isFar: false };
}

export function vehicle(kind, mass, lod = 'high', extra = {}) {
  return new Vehicle(model(kind, lod), { kind, mass, ...extra });
}

// The player as Race builds it.
export function player(t, car) {
  const spec = CAR_SPECS[car];
  const v = vehicle(car, spec.mass, 'high', { name: 'You', color: spec.color });
  const phys = new CarPhysics(v, t, spec);
  const body = new PhysicsBody(v, phys);
  return { v, phys, body, spec };
}

// The level's rivals as Race builds them (Race.js:80-88), with the ai stream.
export function rivals(L, t, rng) {
  return (L.rivals || []).map((r, i) => {
    const v = vehicle(r.kind, 1400, 'high', { name: r.name, color: r.color });
    const ai = new AIDriver(v, t, { skill: r.skill, name: r.name, power: r.power, bias: (i % 2 ? 1 : -1) * 0.6, lineFactor: 0.8 + (i % 3) * 0.08, rng });
    ai.color = r.color;
    return ai;
  });
}

// Race's grid (Race.js:90-98): rows of two; the player fourth with five
// rivals, else first.
export function grid(t, P, ais) {
  const order = ais.length >= 5 ? [ais[0], ais[1], ais[2], 'player', ais[3], ais[4]] : ['player', ...ais];
  order.forEach((c, k) => {
    const row = Math.floor(k / 2), col = k % 2;
    const s = t.startS - 5 - row * 10 - col * 3;
    const lat = col ? 2.4 : -2.4;
    if (c === 'player') { P.phys.reset(t.wrap(s), lat); P.v.prog = s; }
    else { c.s = t.wrap(s); c.lat = lat; c.speed = 0; c.prog = s; c.writePos(); }
  });
}

// Traffic as Race builds it, with models that carry only dimensions.
export function traffic(L, t, world, count, rng) {
  const scene = { add() {} };
  const build = (kind) => model(kind, 'lowFar');
  return new Traffic(t, scene, build, { level: L, world, count, rng });
}

// The police as PursuitView builds them (models by unit type).
const UNIT_KIND = { patrol: 'police', interceptor: 'muscle', suv: 'policeSuv' };
export function pursuit(L, t, spec, { heat = 1, cops = 6, rng, policeRng }) {
  const makeUnit = (type) => {
    if (type === 'sawhorse') {
      const saw = WORLD.levels.sierra.bodies.sawhorses[0];
      return new Vehicle({ dims: { length: saw.length, width: saw.width, height: saw.height, wheelRadius: saw.wheelRadius, wheelBase: saw.wheelBase }, root: { visible: false } }, { kind: 'sawhorse', mass: 60 });
    }
    return vehicle(UNIT_KIND[type], UNIT_TYPES[type].mass, 'lowFar', { name: 'Police' });
  };
  return new Pursuit({ track: t, level: L, makeUnit, heat, maxUnits: cops, playerTop: topSpeed(spec), flash: true, rng, policeRng });
}

export { simStreams };

// A staged world stepped as Race.update steps it (SPEC 4.3 tick order),
// without Race's own rules (laps, bonuses, the finish) and without
// PursuitView: physics, rivals, traffic, the pursuit, collisions and
// writePos, the crash flag per hit, and the pursuit's own hit rules.
export class Sim {
  constructor({ t, P = null, ais = [], traf = null, pu = null, streams = null }) {
    Object.assign(this, { t, P, ais, traf, pu, streams });
    this.time = 0;
    this.tick = 0;
    this.started = true;
    this.odo = null;
    this.lastS = null;
  }

  agents() {
    return [...(this.P ? [this.P.body] : []), ...this.ais, ...(this.traf ? this.traf.cars.filter((c) => c.active) : []), ...(this.pu ? this.pu.bodies() : [])];
  }

  // inp: the player's input this tick (already quantised), or null.
  step(inp) {
    const t = this.t, dt = DT, P = this.P;
    this.tick++;
    this.time += dt;
    const agents = this.agents();
    if (P) P.phys.update(dt, inp);
    const ps = P ? P.v.s : this.ais[0]?.s ?? 0;
    const ctx = { cars: agents, playerS: ps, playerProg: null, started: this.started, time: this.time };
    for (const a of this.ais) a.update(dt, ctx);
    if (this.traf) {
      const dS = t.ds(this.lastS ?? ps, ps);
      this.lastS = ps;
      this.odo = (this.odo ?? ps) + dS;
      this.traf.update(dt, ps, agents, 0, t.loop ? this.odo : ps);
    }
    if (this.pu) {
      this.pu.update(dt, { cars: agents, time: this.time, started: this.started });
      for (const b of this.pu.bodies()) if (!agents.includes(b)) agents.push(b);
    }
    const hits = [];
    resolveCollisions(agents, hits);
    for (const a of this.ais) a.writePos();
    if (this.traf) for (const c of this.traf.cars) if (c.active) c.writePos();
    if (this.pu) for (const u of this.pu.bodies()) u.writePos();
    for (const h of hits) {
      if (this.pu) {
        const pit = this.pu.onHit(h);
        if (pit && P) P.v.yawRate += pit * 2.2;
      }
      const pb = P?.body;
      const other = h.a === pb ? h.b : h.a;
      if (other.crashed !== undefined && h.strength > 0.15) other.crashed = Math.max(other.crashed, 0.01);
    }
    if (P) P.phys.events.length = 0;
    if (this.pu) this.pu.events.length = 0;
  }

  view(input) {
    return {
      tick: this.tick, race: null,
      players: this.P ? [{ v: this.P.v, phys: this.P.phys, rule: null, pv: null, body: this.P.body, input }] : [],
      rivals: this.ais, traffic: this.traf, pursuit: this.pu, pv: null,
      passed: null, nearMiss: null, streams: this.streams,
    };
  }
}
