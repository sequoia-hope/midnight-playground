import { clamp, rrange, smoothstep } from '../util/math.js';
import { ROAD_KEYS } from '../track/roadTypes.js';
import { KinematicCar } from '../vehicles/Kinematic.js';
import { PoliceDriver, UNIT_TYPES } from '../vehicles/PoliceDriver.js';

// Hot Pursuit (docs/hot-pursuit.md). Police patrol a sprint race and chase
// whichever racer is nearest, the player most of all. This module is the
// whole system and doesn't touch three.js, so it runs in plain Node: heat,
// line of sight, the pursuit / cooldown states, busts, the unit pool and
// its tactics, roadblocks, spike strips and breakable barriers. The Race
// feeds it racers and collisions and turns its events into sound, HUD and
// effects.
//
// Getting busted or wrecked doesn't end a race. The car is held on the spot
// for a penalty (the race clock and the rivals keep going), then released:
// the Race resets it onto the road ahead of the police that caught it, and
// they stand down.

// Heat: max active units, the unit mix, and a top-speed factor relative to
// the player's car (at heat 5 the fastest units edge you on a straight).
export const HEAT = [
  null,
  { units: 2, mix: ['patrol'], speed: 0.92 },
  { units: 3, mix: ['patrol', 'patrol', 'interceptor'], speed: 0.97 },
  { units: 4, mix: ['patrol', 'interceptor', 'interceptor'], speed: 1.0 },
  { units: 5, mix: ['patrol', 'interceptor', 'suv'], speed: 1.03 },
  { units: 6, mix: ['interceptor', 'suv', 'suv', 'patrol'], speed: 1.06 },
];
export const EVADE_TIME = [0, 8, 11, 14, 18, 22]; // seconds out of sight to escape, by heat
export const HEAT_GAIN = { second: 0.02, hitCop: 0.15, takedown: 0.35, dodge: 0.2, hitCivilian: 0.05 };
export const LOS_RANGE = 300;                      // metres along the road
export const LOS_TURN = 70 * Math.PI / 180;        // total road turn that hides you
export const BUST = { speed: 6, radius: 9, rate: 0.45, extra: 0.25, drain: 0.8 };
export const bustPenalty = (heat) => 4 + heat * 1.5; // seconds held, 5.5–11.5
export const WRECK_PENALTY = 8;
export const GRACE = 8;                            // seconds after a release before police can bust you again
const SPOT_RANGE = 120;                            // a parked unit notices racers this close
const SPAWN_GAP = 4;                               // seconds between units joining from behind
const RETARGET = 1;                                // seconds between target choices
const MIN_BEHAVIOUR = 1.5;
const POOL = { patrol: 3, interceptor: 2, suv: 2 }; // chasers (at most 6 active)
// Radio callsigns: unit i answers to 10 + 3i, give or take two, so every
// pursuit's numbers differ but never collide. CALLSIGNS is every one there is.
export const callsign = (i, rng) => 10 + i * 3 + Math.floor(rng() * 3);
export const CALLSIGNS = Array.from({ length: Object.values(POOL).reduce((a, b) => a + b) * 3 }, (_, k) => 10 + k);
const BLOCK_POOL = ['patrol', 'patrol', 'patrol', 'suv', 'suv'];
const SAWHORSES = 2;

// Top speed of a player car spec on the flat: where drive = drag.
export function topSpeed(spec) {
  let v = 40;
  for (let i = 0; i < 60; i++) {
    let a = Math.min(spec.launch ?? 9.5, spec.power / v);
    if (spec.vmax) a *= 1 - smoothstep(spec.vmax - 5, spec.vmax, v);
    const drag = 0.00115 * v * v + 0.01 * v;
    v += clamp((a - drag) * 2, -2, 2);
  }
  return v;
}

// A breakable sawhorse barrier (heavy roadblocks fill their gap with two).
class Sawhorse extends KinematicCar {
  constructor(vehicle, track) {
    super(vehicle, track);
    this.active = false;
    this.broken = false;
    this.crashy = true;
    this.h = 0; this.vy = 0; this.age = 0;
    this.yFn = (s, lat) => track.surfaceY(s, lat) + this.h;
  }
  get mass() { return 60; }
}

export class Pursuit {
  // makeUnit(type, role) → a Vehicle for a police car; type is 'patrol',
  // 'interceptor' or 'suv', role 'unit' | 'block' | 'sawhorse'.
  constructor({ track, level, makeUnit, heat = 1, maxUnits = 6, playerTop = 75, rng = Math.random, policeRng = Math.random, flash = true }) {
    this.track = track;
    this.level = level;
    this.rng = rng;
    this.flash = flash;
    const P = level.police || {};
    this.heatCap = track.zones.map((_, i) => P.heatCap?.[i] ?? 5);
    this.openGround = track.zones.map((_, i) => !!P.losOpenGround?.[i]);
    this.playerTop = playerTop;
    this.maxUnits = clamp(maxUnits, 0, 6);
    this.heat = clamp(Math.round(heat), 1, 5);
    this.maxHeat = this.heat;
    this.heatMeter = 0;
    this.state = 'patrol';
    this.bust = 0;     // the player's bust meter
    this.evade = 0;
    this.events = [];
    this.time = 0;
    this.spawnT = 0;
    this.propT = 12;   // no roadblock in the first seconds of a pursuit
    this.takedowns = 0;
    this.busts = 0;
    this.racers = [];
    this.player = null;

    // Cumulative road turn (radians) per metre: line of sight round a bend
    // is a subtraction.
    const n = track.n, cum = new Float32Array(n + 1);
    for (let i = 0; i < n; i++) cum[i + 1] = cum[i] + Math.abs(track.kSmooth[i]);
    this.cumTurn = cum;
    this.tunnels = track.tags.filter((g) => g.tag === 'tunnel');

    this.units = [];
    for (const [type, k] of Object.entries(POOL)) {
      for (let i = 0; i < k; i++) {
        const u = new PoliceDriver(makeUnit(type, 'unit'), track, type, policeRng);
        u.v.model.root.visible = false;
        u.callsign = callsign(this.units.length, rng);
        this.units.push(u);
      }
    }
    this.blockCars = BLOCK_POOL.map((type) => {
      const u = new PoliceDriver(makeUnit(type, 'block'), track, type, policeRng);
      u.v.model.root.visible = false;
      return u;
    });
    this.sawhorses = [];
    for (let i = 0; i < SAWHORSES; i++) {
      const b = new Sawhorse(makeUnit('sawhorse', 'sawhorse'), track);
      b.v.model.root.visible = false;
      this.sawhorses.push(b);
    }
    this.roadblock = null; // { s, cars, gapLat, heavy, passed }
    this.spikes = null;    // { s, lat0, lat1, car, hit: Set }

    // Parked patrol cars along the route.
    this.spots = P.spots ? P.spots.map((p) => ({ ...p, used: false })) : this.autoSpots();
  }

  autoSpots() {
    const t = this.track, out = [];
    let s = t.startS + 650, side = 1;
    while (s < t.finishS - 350) {
      // Somewhere fairly straight so it's seen before it's passed.
      let best = s, bestK = Infinity;
      for (let d = 0; d < 200; d += 20) {
        const k = Math.abs(t.kSmooth[t.idx(s + d)]);
        if (k < bestK) { bestK = k; best = s + d; }
      }
      out.push({ s: best, side, used: false });
      side = -side;
      s = best + rrange(this.rng, 700, 1100);
    }
    return out;
  }

  // racers: [{ body, player, name, ai }] — body is what collides (the
  // player's PhysicsBody or a rival's AIDriver).
  setRacers(list) {
    this.racers = list.map((r) => ({ ...r, bust: 0, hold: 0, holdTotal: 0, holdReason: null, grace: 0, finished: false }));
    this.player = this.racers.find((r) => r.player);
  }

  racerFor(body) { return this.racers.find((r) => r.body === body); }

  // Everything that collides: active units, the roadblock and barriers.
  bodies() {
    const out = [];
    for (const u of this.units) if (u.active) out.push(u);
    for (const u of this.blockCars) if (u.active) out.push(u);
    for (const b of this.sawhorses) if (b.active && !b.broken) out.push(b);
    return out;
  }

  zoneAt(s) { return this.track.zone[this.track.idx(s)]; }
  capAt(s) { return this.heatCap[this.zoneAt(s)]; }
  inTunnel(s) { return this.tunnels.find((g) => s > g.s0 - 5 && s < g.s1 + 5) || null; }

  // Can a unit at s1 see a car at s2? Along-track range, tunnels, and how
  // far the road turns between them (canyon walls and city blocks hide you
  // round a corner; open ground doesn't).
  canSee(s1, s2) {
    const t = this.track;
    const d = t.ds(s1, s2);
    if (Math.abs(d) > LOS_RANGE) return false;
    const g1 = this.inTunnel(s1), g2 = this.inTunnel(s2);
    if ((g1 || g2) && g1 !== g2) return false;
    if (this.openGround[this.zoneAt(s1)] && this.openGround[this.zoneAt(s2)]) return true;
    const a = Math.min(s1, s1 + d), b = Math.max(s1, s1 + d);
    const ia = clamp(Math.round(a), 0, t.n), ib = clamp(Math.round(b), 0, t.n);
    return this.cumTurn[ib] - this.cumTurn[ia] < LOS_TURN;
  }

  emit(type, data = {}) { this.events.push({ type, ...data }); }

  // A racer is caught (busted) or wrecked: held for `seconds`.
  arrest(r, reason, seconds) {
    if (r.hold > 0) return;
    r.hold = r.holdTotal = seconds;
    r.holdReason = reason;
    r.bust = 0;
    if (r.ai) { r.ai.hold = seconds; r.ai.holdLat = this.shoulderLat(r.ai.s, r.ai.lat, r.ai.halfW); }
    // The units on its tail stop beside it; the rest drop the chase.
    for (const u of this.units) {
      if (!u.active || u.target !== r.body) continue;
      const near = Math.abs(this.track.ds(u.s, r.body.s)) < 60;
      u.setMode(near ? 'hold' : 'standdown');
      u.target = near ? r.body : null;
    }
    if (r.player) {
      if (reason === 'busted') this.busts++;
      this.bust = 0; this.evade = 0;
      if (this.state !== 'patrol') {
        this.state = 'patrol';
        this.heat = Math.max(1, this.heat - 1);
        this.heatMeter = 0;
      }
      this.clearProps();
    }
    this.emit(reason, { racer: r, seconds, player: !!r.player, name: r.name });
  }

  shoulderLat(s, lat, halfW) {
    const f = this.track.frame(s);
    const side = lat >= 0 ? 1 : -1;
    return side * Math.max(0, (side > 0 ? f.wallR : f.wallL) - halfW - 0.6);
  }

  // Where to put a released car back on the road: just ahead of any unit
  // around it, in the middle of the road.
  releaseSpot(r) {
    const t = this.track, s0 = r.body.s;
    let s = s0;
    for (const u of this.units) if (u.active && Math.abs(t.ds(s0, u.s)) < 40) s = Math.max(s, s0 + t.ds(s0, u.s) + u.halfL + 6);
    if (this.roadblock) for (const c of this.roadblock.cars) if (Math.abs(t.ds(s0, c.s)) < 30) s = Math.max(s, s0 + t.ds(s0, c.s) + 8);
    s = t.loop ? t.wrap(s) : Math.min(s, t.roadEnd - 5);
    const f = t.frame(s);
    return { s, lat: clamp(r.body.lat, -f.hw * 0.4, f.hw * 0.4) };
  }

  onHit(h) {
    // Units take damage from every hit; the player taking one out is a
    // takedown. Returns a PIT push for the player's car, if any.
    let pit = 0;
    const pb = this.player?.body;
    for (const [me, other] of [[h.a, h.b], [h.b, h.a]]) {
      if (me instanceof Sawhorse && !me.broken) this.breakSawhorse(me, other);
      if (!me.police || !me.active || me.mode === 'disabled') continue;
      if (me.mode === 'block') { if (other === pb && h.strength > 0.2) this.heatUp(HEAT_GAIN.hitCop); continue; }
      // Resting contact (pushing, boxing in) doesn't wear a unit down; real
      // hits do, less so the ones it braced for.
      // Taking units out is the player's game: other cars do half damage.
      if (h.strength > 0.08) me.health -= h.strength * (1.4 - me.spec.mass / 3000) * (me.behaviour === 'bump' || me.behaviour === 'pit' ? 0.5 : 1) * (other === pb ? 1 : 0.5);
      if (other === pb) {
        if (h.strength > 0.2) this.heatUp(HEAT_GAIN.hitCop);
        if (me.mode === 'chase' && me.behaviour === 'pit' && me.pitCooldown === 0 && h.strength > 0.02) {
          pit = -Math.sign(pb.lat - me.lat) * me.aggression; // the rear goes away from the unit
          me.pitCooldown = 6;
          me.behaviour = 'chase'; me.behT = 0;
        }
        if (me.target !== pb && me.mode !== 'hold') { me.target = pb; me.setMode('chase'); }
      }
      if (me.health <= 0) {
        me.setMode('disabled');
        me.target = null;
        me.siren = 'flash';
        const byPlayer = other === pb;
        if (byPlayer) { this.takedowns++; this.heatUp(HEAT_GAIN.takedown); }
        this.emit('takedown', { unit: me, byPlayer, x: me.v.x, z: me.v.z });
      }
    }
    // Hitting a civilian while the police are watching.
    if (pb && (h.a === pb || h.b === pb)) {
      const o = h.a === pb ? h.b : h.a;
      if (o.kinematicOnly && this.state === 'pursuit' && h.strength > 0.15) this.heatUp(HEAT_GAIN.hitCivilian);
    }
    return pit;
  }

  breakSawhorse(b, by) {
    b.broken = true;
    b.age = 0;
    b.vy = 4 + this.rng() * 3;
    const [vx, vz] = by.velocity ? by.velocity() : [0, 0];
    b.setVelocity(vx * 1.1, vz * 1.1);
    b.spinRate = (this.rng() - 0.5) * 16;
    const player = by === this.player?.body;
    if (player && this.roadblock) this.roadblock.touched = true;
    this.emit('barrier', { x: b.v.x, z: b.v.z, player });
  }

  heatUp(k) {
    if (this.state === 'patrol') return;
    const cap = this.player ? this.capAt(this.player.body.s) : 5;
    this.heatMeter += k;
    while (this.heatMeter >= 1) {
      if (this.heat >= cap) { this.heatMeter = 0.999; break; }
      this.heatMeter -= 1;
      this.heat++;
      this.maxHeat = Math.max(this.maxHeat, this.heat);
      this.emit('heat', { heat: this.heat });
    }
  }

  update(dt, { cars, time = 0, started = true }) {
    const t = this.track;
    this.time = time;
    if (!started) { for (const u of this.units) if (u.active) u.writePos(); return; }
    for (const r of this.racers) {
      if (!r.finished && (r.player ? r.body.s >= t.finishS : r.ai?.finished)) this.onFinished(r);
      r.grace = Math.max(0, r.grace - dt);
      if (r.hold > 0) {
        r.hold -= dt;
        if (r.hold <= 0) this.release(r);
      }
    }

    this.activateSpots();
    this.pickTargets(dt);
    this.updateState(dt);
    this.spawn(dt);
    this.assignBehaviours();
    this.bustMeters(dt);

    const cap = this.playerTop * HEAT[this.heat].speed;
    for (const u of this.units) {
      if (!u.active) continue;
      u.cap = cap * u.spec.top;
      u.update(dt, { cars });
      u.siren = u.mode === 'parked' ? 'off' : u.mode === 'disabled' ? (u.disabledT > 3 ? 'disabled' : 'flash') : 'flash';
      if (u.uturned) { u.uturned = false; this.emit('uturn', { unit: u, x: u.v.x, z: u.v.z }); }
    }
    for (const u of this.blockCars) if (u.active) u.update(dt, { cars });
    for (const b of this.sawhorses) if (b.active) this.updateSawhorse(b, dt);
    this.props(dt);
    this.recycle();
  }

  onFinished(r) {
    // The finish line is a safe zone: everyone on its tail breaks off.
    r.finished = true;
    r.bust = 0;
    for (const u of this.units) if (u.active && u.target === r.body && u.mode !== 'hold') { u.setMode('standdown'); u.target = null; }
    if (r.player) { this.state = 'patrol'; this.bust = 0; this.evade = 0; this.clearProps(); }
  }

  release(r) {
    r.hold = 0;
    r.grace = GRACE;
    for (const u of this.units) if (u.active && u.target === r.body && u.mode === 'hold') { u.setMode('standdown'); u.target = null; }
    this.emit('release', { racer: r, player: !!r.player, spot: r.player ? this.releaseSpot(r) : null });
  }

  // Parked patrol cars appear ahead of the player and notice any racer who
  // blasts past them.
  activateSpots() {
    const t = this.track, P = this.player;
    if (!P) return;
    for (const sp of this.spots) {
      if (sp.used) continue;
      const d = t.ds(P.body.s, sp.s);
      if (d > 900 || d < 150) continue;
      const u = this.freeUnit(['patrol', 'interceptor']);
      if (!u) return;
      sp.used = true;
      const f = t.frame(sp.s);
      const lat = sp.lat ?? sp.side * Math.max(0, (sp.side > 0 ? f.wallR : f.wallL) - u.halfW - 0.5);
      this.activate(u, sp.s, lat, 0, 'parked');
      u.parkLat = lat;
    }
  }

  freeUnit(types) {
    for (const ty of types) { const u = this.units.find((x) => !x.active && x.type === ty); if (u) return u; }
    return null;
  }

  activate(u, s, lat, speed, mode, dir = 1) {
    u.active = true;
    u.s = s; u.lat = lat; u.speed = speed; u.dir = dir;
    u.latVel = 0; u.spin = 0; u.spinRate = 0; u.stunned = 0;
    u.health = 1; u.disabledT = 0; u.pitCooldown = 3; u.avoidTimer = 0;
    u.target = null;
    u.mode = null;
    u.setMode(mode);
    u.v.model.root.visible = true;
    u.writePos();
  }

  deactivate(u) {
    u.active = false;
    u.target = null;
    u.v.model.root.visible = false;
  }

  // Who each unit chases: the nearest racer by distance along the road,
  // weighted toward the player so it gets most of the attention.
  pickTargets(dt) {
    const t = this.track;
    const full = this.activeCount() >= Math.min(HEAT[this.heat].units, this.maxUnits);
    for (const u of this.units) {
      if (!u.active) continue;
      if (u.mode === 'parked') {
        if (full) continue; // the heat's quota is out already
        const P = this.player;
        for (const r of this.racers) {
          if (r.finished || r.hold > 0 || r.grace > 0) continue;
          // A rival only sets it off when the player isn't coming too: it
          // waits for the one it's really after.
          if (!r.player && P && !P.finished && t.ds(P.body.s, u.s) < 500 && t.ds(P.body.s, u.s) > -20) continue;
          const d = Math.abs(t.ds(u.s, r.body.s));
          if (d < SPOT_RANGE && Math.abs(r.body.speedAlong) > this.spotSpeed(u.s)) {
            u.target = r.body;
            u.setMode('chase');
            this.emit('spotted', { unit: u, racer: r, player: !!r.player });
            break;
          }
        }
        continue;
      }
      if (u.mode !== 'chase') continue;
      u.retarget = (u.retarget ?? 0) - dt;
      const cur = u.target && this.racerFor(u.target);
      if (cur && (cur.hold > 0 || cur.finished || cur.grace > 0)) u.target = null;
      if (u.retarget > 0 && u.target) continue;
      u.retarget = RETARGET;
      let best = null, bestD = Infinity;
      for (const r of this.racers) {
        if (r.finished || r.hold > 0 || r.grace > 0) continue;
        if (!this.canSee(u.s, r.body.s)) continue;
        const d = Math.abs(t.ds(u.s, r.body.s)) * (r.player ? 0.45 : 1);
        if (d < bestD) { bestD = d; best = r; }
      }
      if (best) u.target = best.body;
      else if (!u.target) { u.lastSeenS = u.s; u.setMode('standdown'); }
    }
  }

  spotSpeed(s) {
    const rule = this.level.traffic?.[this.zoneAt(s)];
    return 1.25 * (rule?.speed?.[1] ?? 20);
  }

  // The player's pursuit state: pursuit while a unit can see you, cooldown
  // (the evade meter fills) while none can.
  updateState(dt) {
    const P = this.player;
    if (!P || P.finished) return;
    const ps = P.body.s;
    let seen = false;
    for (const u of this.units) {
      if (!u.active || u.mode === 'disabled' || u.mode === 'parked' || u.mode === 'standdown' || u.mode === 'hold') continue;
      if (u.target !== P.body && u.mode !== 'search') continue;
      if (this.canSee(u.s, ps)) {
        seen = true;
        if (u.mode === 'search') { u.target = P.body; u.setMode('chase'); }
        u.lastSeenS = ps;
      }
    }
    if (P.hold > 0 || P.grace > 0) seen = false;
    const was = this.state;
    if (seen) {
      if (this.state !== 'pursuit') {
        this.state = 'pursuit';
        this.emit(was === 'cooldown' ? 'reacquired' : 'pursuit', { heat: this.heat });
        if (was === 'patrol') { this.propT = 12; this.spawnT = 2; }
      }
      this.evade = Math.max(0, this.evade - 0.5 * dt);
      this.heatUp(HEAT_GAIN.second * dt);
    } else if (this.state === 'pursuit') {
      this.state = 'cooldown';
      this.emit('cooldown');
      for (const u of this.units) if (u.active && u.target === P.body && u.mode === 'chase') { u.setMode('search'); u.lastSeenS = ps; }
    } else if (this.state === 'cooldown') {
      this.evade += dt / EVADE_TIME[this.heat];
      if (this.evade >= 1) {
        this.evade = 0;
        this.state = 'patrol';
        this.emit('escaped', { heat: this.heat });
        for (const u of this.units) if (u.active && (u.mode === 'search' || u.target === P.body)) { u.setMode('standdown'); u.target = null; }
        this.clearProps();
      }
    }
  }

  activeCount() { return this.units.filter((u) => u.chasing).length; }

  // Reinforcements while the player is in sight: from behind, out of view
  // of the chase camera, arriving fast with the siren on; or ahead in the
  // oncoming lane on two-way roads, to U-turn as you pass.
  spawn(dt) {
    const t = this.track, P = this.player;
    this.spawnT -= dt;
    // Between pursuits a patrol car on the same road clocks you now and
    // then and comes up from behind.
    if (P && this.state === 'patrol' && !P.finished && P.hold <= 0 && P.grace <= 0) {
      this.patrolT = (this.patrolT ?? rrange(this.rng, 20, 30)) - dt;
      if (this.patrolT <= 0 && this.activeCount() < Math.min(HEAT[this.heat].units, this.maxUnits)) {
        this.patrolT = rrange(this.rng, 30, 50);
        const u = this.freeUnit(['patrol', 'interceptor']);
        const s = P.body.s - rrange(this.rng, 150, 220);
        if (u && s > t.startS && t.ds(s, P.body.s) > 0) {
          const f = t.frame(s);
          this.activate(u, s, clamp(P.body.lat, -f.hw * 0.6, f.hw * 0.6), Math.max(20, Math.abs(P.body.speedAlong) + 8), 'chase');
          u.target = P.body;
          this.emit('join', { unit: u });
        }
      }
    }
    if (!P || this.state !== 'pursuit' || this.spawnT > 0 || P.hold > 0 || P.finished) return;
    const limit = Math.min(HEAT[this.heat].units, this.maxUnits);
    if (this.activeCount() >= limit) return;
    const mix = HEAT[this.heat].mix;
    const want = mix[Math.floor(this.rng() * mix.length)];
    const u = this.freeUnit([want, 'patrol', 'interceptor', 'suv']);
    if (!u) return;
    this.spawnT = SPAWN_GAP;
    const ps = P.body.s;
    const rule = this.level.traffic?.[this.zoneAt(ps)];
    const twoWay = (rule?.oncoming ?? 0) > 0.3 && !['freeway', 'playa'].includes(ROAD_KEYS[t.roadType[t.idx(ps)]]);
    if (twoWay && this.rng() < 0.35) {
      const s = ps + rrange(this.rng, 400, 600);
      if (!t.loop && s > t.finishS - 50) return;
      const f = t.frame(s);
      // Along the far edge of the oncoming lane, out of the racers' way.
      const lat = -Math.max(f.hw * 0.5, f.wallL - u.halfW - 0.5);
      this.activate(u, s, lat, 22, 'oncoming', -1);
      u.laneLat = lat;
      u.target = P.body;
    } else {
      const s = ps - rrange(this.rng, 180, 260);
      if (!t.loop && s < 5) return;
      const f = t.frame(s);
      this.activate(u, s, clamp(P.body.lat, -f.hw * 0.6, f.hw * 0.6), Math.max(20, Math.abs(P.body.speedAlong) + 8), 'chase');
      u.target = P.body;
    }
    this.emit('join', { unit: u });
  }

  // Tactics, by heat. Each unit keeps a behaviour at least 1.5 s.
  assignBehaviours() {
    const t = this.track;
    const byTarget = new Map();
    for (const u of this.units) {
      if (!u.active || u.mode !== 'chase' || !u.target) continue;
      (byTarget.get(u.target) || byTarget.set(u.target, []).get(u.target)).push(u);
    }
    for (const [T, list] of byTarget) {
      const isPlayer = T === this.player?.body;
      const near = list.filter((u) => Math.abs(t.ds(u.s, T.s)) < 40);
      // Box (heat 4+): three units close by hold slots round the target.
      if (isPlayer && this.heat >= 4 && near.length >= 3) {
        const f = t.frame(T.s);
        const wide = f.hw > 5;
        const order = [...near].sort((a, b) => t.ds(T.s, b.s) - t.ds(T.s, a.s)); // furthest ahead first
        const slots = wide ? ['ahead', 'left', 'right', 'behind'] : ['ahead', 'behind', 'behind', 'behind'];
        const sides = order.slice(1, 3).sort((a, b) => a.lat - b.lat);
        for (const u of near) {
          let slot = u === order[0] ? 'ahead' : wide && sides.includes(u) ? (u === sides[0] ? 'left' : 'right') : 'behind';
          if (!slots.includes(slot)) slot = 'behind';
          if (u.behaviour !== 'box' && u.behT < MIN_BEHAVIOUR) continue;
          if (u.behaviour !== 'box') u.behT = 0;
          u.behaviour = 'box'; u.slot = slot;
        }
        continue;
      }
      let rolling = list.some((u) => u.behaviour === 'roll');
      for (const u of list) {
        if (u.behaviour === 'box' && near.length < 3) { u.behaviour = 'chase'; u.behT = 0; }
        if (u.behT < MIN_BEHAVIOUR) continue;
        const gap = t.ds(u.s, T.s);
        const r = this.rng();
        let b = 'chase';
        if (this.heat >= 3 && !rolling && isPlayer && gap < 20 && gap > -30 && u.type !== 'patrol' && r < 0.35 * u.aggression + 0.15) { b = 'roll'; rolling = true; }
        else if (this.heat >= 2 && u.pitCooldown === 0 && gap > 0 && gap < 18 && r < 0.5 * u.aggression + 0.1) { b = 'pit'; u.pitSide = this.rng() < 0.5 ? -1 : 1; }
        else if (gap > 0 && gap < 25 && r < 0.4 + u.aggression * 0.5) b = 'bump';
        if (u.behaviour === 'roll' && b !== 'roll') rolling = list.some((o) => o !== u && o.behaviour === 'roll');
        if (b !== u.behaviour) { u.behaviour = b; u.behT = 0; }
        else u.behT = MIN_BEHAVIOUR * 0.5; // re-roll soon
      }
    }
  }

  // Bust meters: fill while a racer crawls with a unit right beside it.
  bustMeters(dt) {
    for (const r of this.racers) {
      if (r.finished || r.hold > 0) continue;
      const b = r.body, sp = Math.hypot(b.v.vx, b.v.vz);
      let n = 0;
      if (r.grace <= 0 && sp < BUST.speed && (!r.player || this.state === 'pursuit')) {
        for (const u of this.units) {
          if (!u.active || u.mode !== 'chase' || u.target !== b) continue;
          if (Math.hypot(u.v.x - b.v.x, u.v.z - b.v.z) < BUST.radius) n++;
        }
        for (const c of this.roadblock?.cars || []) if (Math.hypot(c.v.x - b.v.x, c.v.z - b.v.z) < BUST.radius && this.unitNear(b, 80)) n++;
      }
      if (n > 0) r.bust += (BUST.rate + BUST.extra * (n - 1)) * dt;
      else r.bust = Math.max(0, r.bust - BUST.drain * dt);
      if (r.player) this.bust = Math.min(1, r.bust);
      if (r.bust >= 1) this.arrest(r, 'busted', bustPenalty(this.heat));
    }
  }

  unitNear(b, d) {
    return this.units.some((u) => u.active && u.mode === 'chase' && u.target === b && Math.abs(this.track.ds(u.s, b.s)) < d);
  }

  // ── Roadblocks and spike strips ────────────────────────────────
  props(dt) {
    const t = this.track, P = this.player;
    if (!P) return;
    const ps = P.body.s;
    this.propT -= dt;
    // Place one when the chase is on and the zone allows it.
    if (this.state === 'pursuit' && !this.roadblock && !this.spikes && this.propT <= 0 && !P.finished) {
      const spikes = this.heat >= 4 && this.rng() < 0.45;
      if (this.heat >= 3) {
        const s = this.findStraight(ps + (spikes ? 380 : 470), ps + (spikes ? 600 : 750), spikes ? 80 : 150, spikes ? 0.004 : 0.002);
        if (s != null && this.capAt(s) >= (spikes ? 4 : 3)) {
          if (spikes) this.placeSpikes(s); else this.placeRoadblock(s);
        }
      }
      this.propT = 6; // look again soon if nothing fitted
    }
    // Spike strips shred any racer whose wheels cross them.
    const S = this.spikes;
    if (S) {
      for (const r of this.racers) {
        const b = r.body, prev = r.prevS ?? b.s;
        if (prev < S.s && b.s >= S.s && !S.hit.has(r) && b.lat + b.halfW > S.lat0 && b.lat - b.halfW < S.lat1) {
          S.hit.add(r);
          if (r.player) P.body.phys.spiked = 10; else if (r.ai) r.ai.spiked = 10;
          this.emit('spiked', { racer: r, player: !!r.player, name: r.name });
        }
      }
    }
    for (const r of this.racers) r.prevS = r.body.s;
    // Passed: count the dodge, clear up once it's well behind.
    for (const [key, prop] of [['roadblock', this.roadblock], ['spikes', this.spikes]]) {
      if (!prop) continue;
      const d = t.ds(prop.s, ps);
      if (d > 5 && !prop.passed) {
        prop.passed = true;
        const hitIt = key === 'spikes' ? prop.hit.has(P) : prop.touched;
        if (!hitIt && this.state !== 'patrol') { this.heatUp(HEAT_GAIN.dodge); this.emit('dodge', { what: key }); }
      }
      if (d > 250 || d < -1200) this[key === 'roadblock' ? 'clearRoadblock' : 'clearSpikes']();
    }
    if (this.roadblock && !this.roadblock.touched) {
      const pb = P.body;
      for (const c of this.roadblock.cars) if (Math.hypot(c.v.x - pb.v.x, c.v.z - pb.v.z) < c.halfL + pb.halfL + 0.5) this.roadblock.touched = true;
    }
  }

  // The first s in [a, b] with a straight run `len` long around it.
  findStraight(a, b, len, kmax) {
    const t = this.track;
    for (let s = a; s <= b; s += 10) {
      if (!t.loop && (s > t.finishS - 120 || s < t.startS + 200)) continue;
      if (this.inTunnel(s)) continue;
      let ok = true;
      for (let d = -len / 2; d <= len / 2; d += 5) if (Math.abs(t.kSmooth[t.idx(s + d)]) > kmax) { ok = false; break; }
      if (ok) return s;
    }
    return null;
  }

  placeRoadblock(s) {
    const t = this.track, f = t.frame(s);
    const heavy = this.heat >= 5;
    const L = -f.wallL + 0.3, R = f.wallR - 0.3, span = R - L;
    const angle = 1.05; // ~60° to the road
    const pool = heavy ? [...this.blockCars].sort((a, b) => (b.type === 'suv') - (a.type === 'suv')) : this.blockCars;
    const car0 = pool[0];
    const ext = car0.halfL * 2 * Math.sin(angle) + car0.halfW * 2 * Math.cos(angle);
    const gapW = 1.6 * 2 * 1.0; // 1.6 car widths
    const n = clamp(Math.ceil((span - gapW) / ext), 2, pool.length);
    const step = (span - gapW) / n;
    // The gap sits between two cars, away from the walls.
    const k = 1 + Math.floor(this.rng() * Math.max(1, n - 1));
    const cars = [];
    let lat = L;
    for (let i = 0; i < n; i++) {
      if (i === k) lat += gapW;
      const c = pool[i];
      this.activateBlock(c, s + (i % 2 ? 1.5 : -1.5), lat + step / 2, (i % 2 ? 1 : -1) * angle);
      cars.push(c);
      lat += step;
    }
    const gapLat = L + k * step + gapW / 2;
    for (const c of cars) c.gapLat = gapLat;
    if (heavy) {
      // Heavy roadblock: the gap is closed by barriers you can smash.
      this.sawhorses.forEach((b, i) => {
        this.activateBarrier(b, s + (i ? 2 : -2), gapLat + (i ? 0.7 : -0.7));
        b.gapLat = gapLat;
      });
    }
    this.roadblock = { s, cars, gapLat, heavy, passed: false, touched: false };
    this.emit('roadblock', { s, heavy });
  }

  activateBlock(c, s, lat, yaw) {
    this.activate(c, s, lat, 0, 'block');
    c.blockYaw = yaw;
    c.spin = yaw;
    c.siren = 'flash';
    c.writePos();
  }

  activateBarrier(b, s, lat) {
    b.active = true; b.broken = false;
    b.s = s; b.lat = lat; b.speed = 0; b.latVel = 0; b.dir = 1;
    b.h = 0; b.vy = 0; b.age = 0;
    b.spin = Math.PI / 2; b.spinRate = 0;
    b.v.model.root.visible = true;
    b.writePos();
  }

  updateSawhorse(b, dt) {
    if (!b.broken) { b.spin = Math.PI / 2; b.speed = 0; b.latVel = 0; b.writePos(); return; }
    // Flying debris: a tumble up and back down, then gone.
    b.age += dt;
    b.vy -= 9.8 * dt;
    b.h = Math.max(0, b.h + b.vy * dt);
    if (b.h === 0) { b.vy = 0; b.speed *= 1 - 3 * dt; b.latVel *= 1 - 3 * dt; }
    b.s += b.speed * dt; b.lat += b.latVel * dt;
    b.spin += b.spinRate * dt;
    b.writePos();
    if (b.age > 3) { b.active = false; b.v.model.root.visible = false; }
  }

  placeSpikes(s) {
    const t = this.track, f = t.frame(s), P = this.player;
    // Across the half of the road the player's on, with a unit parked on
    // the far shoulder.
    const L = -f.wallL + 0.4, R = f.wallR - 0.4, span = R - L;
    const w = Math.min(span * 0.6, 9);
    const c = clamp(P.body.lat, L + w / 2, R - w / 2);
    const car = this.blockCars[0];
    const side = c > 0 ? -1 : 1;
    this.activateBlock(car, s - 6, side * Math.max(0, (side > 0 ? f.wallR : f.wallL) - car.halfW - 0.4), side * 0.25);
    car.gapLat = null;
    this.spikes = { s, lat0: c - w / 2, lat1: c + w / 2, car, hit: new Set(), passed: false };
    this.emit('spikes', { s });
  }

  clearRoadblock() {
    if (!this.roadblock) return;
    for (const c of this.roadblock.cars) this.deactivate(c);
    for (const b of this.sawhorses) { b.active = false; b.v.model.root.visible = false; }
    this.roadblock = null;
    this.propT = 25;
  }

  clearSpikes() {
    if (!this.spikes) return;
    this.deactivate(this.spikes.car);
    this.spikes = null;
    this.propT = 20;
  }

  clearProps() {
    // Only clear props still ahead (out of view); ones in sight stay.
    const t = this.track, ps = this.player?.body.s ?? 0;
    if (this.roadblock && t.ds(ps, this.roadblock.s) > 250) this.clearRoadblock();
    if (this.spikes && t.ds(ps, this.spikes.s) > 250) this.clearSpikes();
  }

  // Units well behind the player and out of sight go back to the pool.
  recycle() {
    const t = this.track, P = this.player;
    if (!P) return;
    const ps = P.body.s;
    for (const u of this.units) {
      if (!u.active) continue;
      const d = t.ds(ps, u.s);
      const tgt = u.target && this.racerFor(u.target);
      const busy = u.mode === 'chase' && tgt && !tgt.player && Math.abs(t.ds(u.s, tgt.body.s)) < 400;
      if (busy) continue;
      if ((d < -600 && !this.canSee(u.s, ps)) || d > 1500) this.deactivate(u);
      else if ((u.mode === 'standdown' || u.mode === 'disabled' || u.mode === 'parked' || u.mode === 'hold') && d < -350) this.deactivate(u);
    }
  }

  // What the HUD shows.
  hud(damage = 0) {
    const P = this.player;
    return {
      heat: this.heat, heatMeter: this.heatMeter, state: this.state,
      bust: this.bust, evade: this.evade, damage,
      hold: P?.hold > 0 ? P.hold : 0, holdReason: P?.holdReason, holdTotal: P?.holdTotal || 0,
      units: this.units.filter((u) => u.active && u.mode !== 'parked').map((u) => ({ x: u.v.x, z: u.v.z, disabled: u.mode === 'disabled' }))
        .concat(this.units.filter((u) => u.active && u.mode === 'parked').map((u) => ({ x: u.v.x, z: u.v.z, disabled: true }))),
      roadblocks: this.roadblock ? [this.propMark(this.roadblock.s, -this.track.frame(this.roadblock.s).wallL, this.track.frame(this.roadblock.s).wallR)] : [],
      spikes: this.spikes ? [this.propMark(this.spikes.s, this.spikes.lat0, this.spikes.lat1)] : [],
      flash: this.flash,
    };
  }

  propMark(s, lat0, lat1) {
    const f = this.track.frame(s), c = (lat0 + lat1) / 2;
    return { x: f.x + f.rx * c, z: f.z + f.rz * c, yaw: Math.atan2(f.fz, f.fx), width: lat1 - lat0 };
  }
}

export { UNIT_TYPES };
