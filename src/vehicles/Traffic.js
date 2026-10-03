import { clamp, mulberry32, rpick, rrange, stopSpeed } from '../util/math.js';
import { ROAD_KEYS } from '../track/roadTypes.js';
import { KinematicCar } from './Kinematic.js';
import { Vehicle } from './Vehicle.js';

// Civilian traffic. A fixed pool of cars is kept around the player: cars
// that fall far behind are recycled to a spot ahead that suits the zone —
// oncoming cars on two-lane roads (plus the odd tractor), both directions on
// the beach boulevard, dense same-direction traffic on freeways, and
// headlights streaming the other way on the far carriageway where a level
// has one. Rules per zone come from the level (level.traffic).

const PAINT = [0xc9ccd1, 0x2b2f36, 0x8a1c1c, 0x1d3f75, 0xe8e6df, 0x4d5a3a, 0x6b4a2e, 0x9aa3ad, 0x2e6d8e, 0xb5a27a];
const MASS = { sedan: 1500, hatch: 1200, van: 2200, pickup: 2100, boxtruck: 6500, tractor: 3500 };
// Past FAR_OUT metres from the camera (x, z) a car switches to its cheap far
// model (CarModel's far LOD), and back inside FAR_IN, so one pacing you at
// the boundary doesn't flicker between the two.
export const FAR_OUT = 95, FAR_IN = 85;
export function farLod(v, x, z) {
  const m = v.model;
  if (!m.setFar) return;
  const d2 = Math.pow(v.x - x, 2) + Math.pow(v.z - z, 2);
  m.setFar(d2 > Math.pow(m.isFar ? FAR_IN : FAR_OUT, 2));
}

export class TrafficCar extends KinematicCar {
  constructor(vehicle, track) {
    super(vehicle, track);
    this.active = false;
    this.cruise = 20;
    this.laneLat = 0;
    this.crashed = 0;
    this.kinematicOnly = true;
    this.opposite = false;
  }
}

export class Traffic {
  // rng: the Rust port's reference run passes the same seed-99 stream with
  // a draw counter (src/parity/sim.js); left out, it is made here.
  constructor(track, scene, buildVehicle, { level, world, count = 22, seed = 99, rng = null } = {}) {
    this.track = track;
    this.scene = scene;
    this.rules = level.traffic;
    this.world = world;
    this.rng = rng ?? mulberry32(seed);
    this.cars = [];
    this.pool = {};
    const hasTractors = this.rules.some((r) => r.mix.some(([k]) => k === 'tractor'));
    const per = { sedan: 8, hatch: 6, van: 4, pickup: 5, boxtruck: 5, tractor: hasTractors ? 2 : 0 };
    if (this.rules.some((r) => r.gap[0] < 40)) for (const k of ['sedan', 'hatch', 'van', 'boxtruck']) per[k] += 3;
    const paint = level.trafficPaint || PAINT;
    const make = (k, i) => {
      const color = k === 'tractor' ? rpick(this.rng, [0x9c2a1c, 0x3d6b2a]) : rpick(this.rng, paint);
      const model = buildVehicle(k, { color, seed: i * 17 + k.length, lod: 'low', far: true });
      const veh = new Vehicle(model, { kind: k, mass: MASS[k] });
      model.root.visible = false;
      scene.add(model.root);
      const car = new TrafficCar(veh, track);
      car.kindName = k;
      this.cars.push(car);
      return car;
    };
    for (const [k, n] of Object.entries(per)) {
      this.pool[k] = [];
      for (let i = 0; i < n; i++) this.pool[k].push(make(k, i));
    }
    // Oncoming traffic on the far carriageway (visual only, behind the median).
    this.oppPool = [];
    if (this.rules.some((r) => r.opposite)) {
      for (let i = 0; i < 14; i++) {
        const car = make(rpick(this.rng, ['sedan', 'sedan', 'hatch', 'van', 'boxtruck', 'pickup']), 100 + i);
        car.opposite = true;
        car.freeLat = true;
        this.oppPool.push(car);
      }
    }
    this.maxActive = count;
    this.nextSpawnS = 0;
    this.nextOppS = 0;
  }

  laneFor(dir, f, s) {
    const t = this.track;
    const type = ROAD_KEYS[t.roadType[t.idx(s)]];
    if (type === 'freeway' && f.hw > 8.5) {
      const left = -f.hw + 1.2, right = f.hw - 2.0;
      const lw = (right - left) / 4;
      const lane = Math.floor(this.rng() * 4);
      return { lat: left + lw * (lane + 0.5), lane };
    }
    if (type === 'street') {
      // Two lanes each way between the kerbs.
      const lw = f.hw / 2;
      const lane = this.rng() < 0.5 ? 0 : 1;
      return { lat: dir * lw * (lane + 0.5), lane: dir > 0 ? 2 + lane : 1 - lane };
    }
    if (type === 'boulevard') {
      const lw = (f.hw - 0.45) / 2;
      const lane = this.rng() < 0.5 ? 0 : 1;
      return { lat: dir * (lw * (lane + 0.5) + 0.1), lane: dir > 0 ? 2 + lane : 1 - lane };
    }
    const off = f.hw * 0.5;
    return { lat: dir > 0 ? off : -off, lane: dir > 0 ? 1 : 0 };
  }

  // Put a car somewhere ahead of the player that suits the zone.
  spawn(sRaw) {
    const t = this.track;
    const s = t.wrap(sRaw);
    const f = t.frame(s);
    if (!t.loop && (s < t.startS + 250 || s > t.finishS + 100)) return false;
    const rule = this.rules[f.zone];
    if (!rule || !rule.mix.length) return false; // an empty mix: no traffic in this zone
    let r = this.rng(), kind = rule.mix[0][0];
    for (const [k, w] of rule.mix) { if ((r -= w) <= 0) { kind = k; break; } }
    const car = this.pool[kind]?.find((c) => !c.active);
    if (!car) return false;
    let dir = this.rng() < rule.oncoming ? -1 : 1;
    if (kind === 'tractor') dir = 1;
    const { lat, lane } = this.laneFor(dir, f, s);
    for (const o of this.cars) if (o.active && Math.abs(t.ds(o.s, s)) < 25 && Math.abs(o.lat - lat) < 3) return false;
    this.activate(car, s, lat, dir);
    car.lane = lane;
    const [a, b] = rule.speed;
    car.cruise = kind === 'tractor' ? 8 : kind === 'boxtruck' ? a * 0.9 : rrange(this.rng, a, b);
    if (ROAD_KEYS[t.roadType[t.idx(s)]] === 'freeway') car.cruise += (3 - lane) * 1.6; // left lanes faster
    car.speed = car.cruise;
    car.writePos();
    return true;
  }

  activate(car, s, lat, dir) {
    car.active = true;
    car.s = s; car.lat = lat; car.laneLat = lat; car.dir = dir;
    car.latVel = 0; car.spin = 0; car.spinRate = 0; car.crashed = 0; car.stunned = 0;
    car.v.model.root.visible = true;
  }

  spawnOpposite(sRaw) {
    const oc = this.world?.oppositeCarriageway;
    const t = this.track;
    const s = t.wrap(sRaw);
    if (!oc || !oc.lanes?.length) return false;
    if (!t.loop && (s < oc.s0 + 20 || s > oc.s1 - 20)) return false;
    const rule = this.rules[t.zone[t.idx(s)]];
    if (!rule?.opposite || this.rng() > rule.opposite) return false;
    const car = this.oppPool.find((c) => !c.active);
    if (!car) return false;
    const lat = rpick(this.rng, oc.lanes);
    for (const o of this.oppPool) if (o.active && Math.abs(t.ds(o.s, s)) < 30 && Math.abs(o.lat - lat) < 2) return false;
    this.activate(car, s, lat, -1);
    car.yFn = (ss) => oc.y(ss);
    car.cruise = rrange(this.rng, 22, 31);
    car.speed = car.cruise;
    car.writePos();
    return true;
  }

  // Far models for the cars a long way from the camera (x, z).
  lod(x, z) {
    for (const c of this.cars) if (c.active) farLod(c.v, x, z);
  }

  despawn(car) {
    car.active = false;
    car.v.model.root.visible = false;
  }

  active() { return this.cars.filter((c) => c.active); }

  // playerS: position on the track; playerDist: unwrapped distance driven
  // (same as playerS on point-to-point tracks).
  update(dt, playerS, agents, night, playerDist = playerS) {
    const t = this.track;
    let n = 0;
    for (const c of this.cars) {
      if (!c.active) continue;
      const ds = t.ds(playerS, c.s);
      const oc = this.world?.oppositeCarriageway;
      const offDeck = c.opposite && oc && !t.loop && (c.s < oc.s0 - 5 || c.s > oc.s1);
      if (ds < (c.opposite ? -120 : -260) || ds > 1400 || c.crashed > 25 || offDeck) this.despawn(c);
      else if (!c.opposite) n++;
    }
    // Keep the road ahead populated.
    if (this.nextSpawnS < playerDist + 350) this.nextSpawnS = playerDist + 350 + this.rng() * 200;
    let tries = 0;
    while (n < this.maxActive && this.nextSpawnS < playerDist + 1100 && tries++ < 6) {
      const rule = this.rules[t.zone[t.idx(this.nextSpawnS)]];
      if (this.spawn(this.nextSpawnS)) n++;
      this.nextSpawnS += rrange(this.rng, rule.gap[0], rule.gap[1]);
    }
    if (this.oppPool.length) {
      if (this.nextOppS < playerDist + 500) this.nextOppS = playerDist + 500 + this.rng() * 200;
      for (let k = 0; k < 4 && this.nextOppS < playerDist + 1200; k++) {
        this.spawnOpposite(this.nextOppS);
        this.nextOppS += rrange(this.rng, 40, 110);
      }
    }

    for (const c of this.cars) {
      if (!c.active) continue;
      c.frame();
      if (c.crashed > 0) {
        c.crashed += dt;
        c.speed = Math.max(0, c.speed - 9 * dt);
        c.latVel *= 1 - 2 * dt;
      } else if (!c.opposite) {
        // Car-following: slow for anything ahead in the lane.
        let vT = c.cruise;
        for (const o of agents) {
          if (o === c) continue;
          const ds = t.ds(c.s, o.s) * c.dir;
          if (ds > 0 && ds < 35 && Math.abs(o.lat - c.lat) < 2.4) {
            const ov = o.dir === c.dir ? o.speedAlong : 0;
            vT = Math.min(vT, ov + (ds - 10) * 0.4);
          }
        }
        // The road runs out past the finish: pull up short of its end,
        // ahead of where the racers park.
        if (!t.loop && c.dir > 0) vT = Math.min(vT, stopSpeed(t.roadEnd - 200 - c.s, 2.5));
        vT = Math.max(0, vT);
        c.speed += clamp(vT - c.speed, -8 * dt, 3 * dt);
        // Return to lane after a shove.
        const la = clamp((c.laneLat - c.lat) * 1.5 - c.latVel * 2, -3, 3);
        c.latVel += la * dt;
        if (c.stunned > 0.6) c.crashed = 0.01;
      }
      c.v.brakeLight = c.speed < c.cruise - 2 ? 1 : 0;
      c.advance(dt);
    }
  }
}
