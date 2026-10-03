import * as THREE from 'three';
import { clamp, damp, lerp, smoothstep, stopSpeed, wrapAngle } from '../util/math.js';
import { Vehicle } from '../vehicles/Vehicle.js';
import { CarPhysics, CAR_SPECS, MOTOR_MAX } from '../vehicles/CarPhysics.js';
import { AIDriver } from '../vehicles/AIDriver.js';
import { Traffic } from '../vehicles/Traffic.js';
import { resolveCollisions, PhysicsBody } from '../vehicles/Collisions.js';
import { CameraRig } from './CameraRig.js';
import { Effects } from './Effects.js';
import { HUD, fmtTime } from './HUD.js';
import { PursuitView } from './PursuitView.js';
import { ROAD_TYPES, ROAD_KEYS } from '../track/roadTypes.js';

// One session on a level: builds the field, runs countdown → race →
// results, and wires physics, AI, traffic, collisions, camera, effects, HUD
// and audio together. In 'cruise' mode (endless loop) there are no rivals
// and no finish; you score points for speed, near misses and drifts.
// With `pursuit` on (Hot Pursuit, sprint levels with police data) the police
// join in: see PursuitView.js and Pursuit.js.
// A circuit (a loop with `laps`, Seaside Raceway) is raced over laps: every
// racer's progress is counted unwrapped (`prog`, metres from the line), so
// standings, lap counts and the finish all read that.

// After the finish every racer drives on at a relaxed cruise, drifts into a
// lane and rolls to a stop in a parking row short of the end of the road.
const COOL_CRUISE = 24;  // m/s
const COOL_DECEL = 1.8;  // m/s², the final roll to a stop
const PARK_GAP = 240;    // front row, metres short of the road's end
const PARK_ROW = 13;     // spacing of the rows behind it

export class Race {
  // rngs: { ai, police, pursuit } seeded streams for the Rust port's reference
  // run (src/parity/sim.js); null in the game, which uses Math.random.
  constructor({ world, scene, camera, renderer, input, audio, buildVehicle, carKind = 'sports', onFinish, pursuit = null, rngs = null }) {
    Object.assign(this, { world, scene, camera, renderer, input, audio, buildVehicle, onFinish });
    this.pads = input.pads ?? null;
    const track = (this.track = world.track);
    this.level = world.level;
    this.cruise = this.level.mode === 'cruise';
    this.pursuitOn = !!(pursuit && this.level.police && !this.cruise);
    // Laps (circuits only) and the progress that finishes the race.
    this.laps = track.laps;
    this.finishProg = this.laps ? track.startS + this.laps * track.n : track.finishS;
    this.lap = 1;
    this.lapStart = 0;
    this.lapTimes = [];
    this.spec = CAR_SPECS[carKind];
    this.score = 0; this.mult = 1; this.multTimer = 0; this.topSpeed = 0; this.dist = 0; this.nearMisses = 0;
    this.time = 0;
    this.state = 'countdown';
    this.countdown = 3.999;
    this.lastBeep = 4;
    this.events = [];
    this.bonusCooldown = 0;
    this.resetCooldown = 0;
    this.wrongWay = 0;
    this.finishDelay = 0;
    this.group = new THREE.Group();
    scene.add(this.group);

    // Player.
    const pModel = buildVehicle(carKind, { color: this.spec.color, lod: 'high', seed: 1 });
    this.player = new Vehicle(pModel, { kind: carKind, mass: this.spec.mass, name: 'You', color: this.spec.color });
    this.phys = new CarPhysics(this.player, track, this.spec);
    this.group.add(pModel.root);
    pModel.root.traverse((o) => { if (o.isMesh) o.castShadow = true; });
    this.playerBody = new PhysicsBody(this.player, this.phys);

    // Headlights for the player: one real spotlight.
    const spot = new THREE.SpotLight(0xfff2d6, 0, 140, 0.55, 0.55, 1.2);
    spot.castShadow = false;
    const anchor = pModel.headlightAnchor || pModel.root;
    anchor.add(spot);
    spot.position.set(0, 0, 0);
    const tgt = new THREE.Object3D();
    tgt.position.set(0, -2.2, 30);
    anchor.add(tgt);
    spot.target = tgt;
    this.headlight = spot;

    // Rivals.
    this.ais = (this.level.rivals || []).map((r, i) => {
      const m = buildVehicle(r.kind, { color: r.color, lod: 'high', seed: 10 + i });
      m.root.traverse((o) => { if (o.isMesh) o.castShadow = true; });
      this.group.add(m.root);
      const v = new Vehicle(m, { kind: r.kind, mass: 1400, name: r.name, color: r.color });
      const ai = new AIDriver(v, track, { skill: r.skill, name: r.name, power: r.power, bias: (i % 2 ? 1 : -1) * 0.6, lineFactor: 0.8 + (i % 3) * 0.08, rng: rngs?.ai });
      ai.color = r.color;
      return ai;
    });

    // Grid: rows of two; the player starts fourth (alone for a cruise).
    const order = this.ais.length >= 5 ? [this.ais[0], this.ais[1], this.ais[2], 'player', this.ais[3], this.ais[4]] : ['player', ...this.ais];
    order.forEach((c, k) => {
      const row = Math.floor(k / 2), col = k % 2;
      const s = track.startS - 5 - row * 10 - col * 3;
      const lat = col ? 2.4 : -2.4;
      if (c === 'player') { this.phys.reset(track.wrap(s), lat); this.player.prog = s; }
      else { c.s = track.wrap(s); c.lat = lat; c.speed = 0; c.prog = s; c.writePos(); }
    });
    this.progS = new Map([[this.player, this.player.s], ...this.ais.map((a) => [a, a.s])]);

    this.traffic = new Traffic(track, this.group, buildVehicle, { level: this.level, world, count: this.cruise ? 30 : this.pursuitOn ? 18 : 22, rng: rngs?.traffic });

    this.cam = new CameraRig(camera, track, world.terrain);
    this.effects = new Effects(scene, renderer, camera);
    this.effects.addCar(this.player, { player: true });
    for (const a of this.ais) this.effects.addCar(a.v);
    for (const c of this.traffic.cars) this.effects.addCar(c.v);

    this.hud = new HUD(track, this.level);
    this.hud.setRacers([{ player: true, color: this.spec.color }, ...this.ais.map((a) => ({ color: a.color }))]);
    this.hud.show(true);
    this.phys.locked = true;
    this.cam.snap = true;
    this.introT = 0;
    this.nearMiss = new WeakMap();
    this.passed = new WeakMap();
    this.extras = new Map();
    this.tunnels = track.tags.filter((g) => g.tag === 'tunnel');
    this.inTunnel = false;
    this.lastDrift = 0;
    this.pv = this.pursuitOn ? new PursuitView(this, rngs ? { ...pursuit, rng: rngs.pursuit, policeRng: rngs.police } : pursuit) : null;
  }

  dispose() {
    this.pv?.dispose();
    this.scene.remove(this.group);
    this.hud.show(false);
    for (const c of this.effects.cars) this.scene.remove(c.pool);
    this.scene.remove(this.effects.smoke.points, this.effects.sparks.points, this.effects.skids.mesh);
    this.headlight.parent?.remove(this.headlight);
  }

  standings() {
    // On a circuit, how far round the race: prog. Elsewhere s is the same.
    const at = (c) => (this.laps ? c.prog : c.s);
    const list = [
      { player: true, name: 'You', color: this.spec.color, s: this.player.s, prog: at(this.player), finished: this.playerFinished, time: this.playerTime, v: this.player },
      ...this.ais.map((a) => ({ player: false, name: a.name, color: a.color, s: a.s, prog: at(a), finished: a.finished, time: a.finishTime, v: a.v })),
    ];
    list.sort((a, b) => {
      if (a.finished && b.finished) return a.time - b.time;
      if (a.finished) return -1;
      if (b.finished) return 1;
      return b.prog - a.prog;
    });
    return list;
  }

  // Circuits: carry each racer's unwrapped progress on by how far it moved
  // along the loop this frame (a reset back down the road counts too).
  trackProgress() {
    const t = this.track;
    for (const [c, last] of this.progS) {
      c.prog += t.ds(last, c.s);
      this.progS.set(c, c.s);
    }
    for (const a of this.ais) {
      if (!a.finished && a.prog >= this.finishProg) {
        a.finished = true;
        a.finishTime = this.time;
      }
    }
  }

  // Where a racer is round the lap, for the HUD: on a circuit, anyone who
  // hasn't crossed the line yet (the grid) is at its start.
  lapS(c) { return this.laps && c.prog < this.track.startS ? this.track.startS : c.s; }

  // The player's lap: announce each new one and keep the lap times.
  lapCheck() {
    const t = this.track;
    const done = Math.floor((this.player.prog - t.startS) / t.n); // laps completed
    if (done < this.lap || done >= this.laps) return;
    this.lapTimes.push(this.time - this.lapStart);
    this.lapStart = this.time;
    this.lap = done + 1;
    const best = Math.min(...this.lapTimes) === this.lapTimes.at(-1) && this.lapTimes.length > 1;
    this.hud.center(this.lap === this.laps ? 'FINAL LAP' : `LAP ${this.lap}/${this.laps}`, 'pop', 1.4);
    this.hud.toast(`LAP ${fmtTime(this.lapTimes.at(-1))}${best ? '  BEST' : ''}`, 2.2);
  }

  update(dt, inp) {
    const t = this.track;
    const night = this.world.sky.night;

    // ── State machine ──────────────────────────────────────────
    if (this.state === 'countdown') {
      this.countdown -= dt;
      // Perfect start = get on the throttle in the last moment before GO.
      if (inp.throttle > 0.5) { if (this.throttleAt == null) this.throttleAt = this.countdown; }
      else this.throttleAt = null;
      const n = Math.ceil(this.countdown);
      if (n < this.lastBeep && n >= 1) { this.hud.center(String(n)); this.audio?.beep(false); this.pads?.kick(0, 0.25, 90); this.lastBeep = n; }
      if (this.countdown <= 0) {
        this.state = 'racing';
        this.phys.locked = false;
        this.hud.center('GO!', 'pop go');
        this.audio?.beep(true);
        this.pads?.kick(0.3, 0.6, 200);
        if (this.throttleAt != null && this.throttleAt < 0.75) {
          const fx = Math.cos(this.player.yaw), fz = Math.sin(this.player.yaw);
          this.player.vx += fx * 6; this.player.vz += fz * 6;
          this.hud.toast('PERFECT START');
        }
      }
    }
    const started = this.state !== 'countdown';
    if (started) this.time += dt;
    // Scenery that follows the start (Seaside Raceway's start lights).
    this.world.onCountdown?.(started ? -1 : this.countdown);

    // ── Player ─────────────────────────────────────────────────
    this.resetCooldown = Math.max(0, this.resetCooldown - dt);
    if (this.input.consume('reset') && started && this.resetCooldown === 0 && !this.pv?.blocksReset()) this.resetPlayer();
    // ── Agents list for AI/traffic awareness ───────────────────
    const agents = [this.playerBody, ...this.ais, ...this.traffic.cars.filter((c) => c.active), ...(this.pv ? this.pv.bodies() : [])];
    const ctrl = this.playerFinished ? this.coolDown(agents, dt) : this.pv?.held ? this.pv.holdControls() : inp;
    this.phys.update(dt, ctrl);

    const ctx = { cars: agents, playerS: this.player.s, playerProg: this.laps ? this.player.prog : null, started, time: this.time };
    for (const a of this.ais) {
      a.update(dt, ctx);
      if (a.finished && !a.park) a.park = this.parkSpot(a.s, a.lat);
    }
    if (this.laps) this.trackProgress();
    const dS = t.ds(this.lastS ?? this.player.s, this.player.s);
    this.lastS = this.player.s;
    this.dist += Math.abs(dS);
    this.odo = (this.odo ?? this.player.s) + dS; // unwrapped position (loops)
    this.traffic.update(dt, this.player.s, agents, night, t.loop ? this.odo : this.player.s);
    if (this.pv) {
      this.pv.update(dt, agents, started);
      // Units that joined this frame collide from now on.
      for (const b of this.pv.bodies()) if (!agents.includes(b)) agents.push(b);
    }

    // ── Collisions ─────────────────────────────────────────────
    const hits = [];
    resolveCollisions(agents, hits);
    for (const a of this.ais) a.writePos();
    for (const c of this.traffic.cars) if (c.active) c.writePos();
    this.pv?.writePos();
    for (const h of hits) {
      this.pv?.onHit(h);
      const involvesPlayer = h.a === this.playerBody || h.b === this.playerBody;
      const other = h.a === this.playerBody ? h.b : h.a;
      if (other.crashed !== undefined && h.strength > 0.15) other.crashed = Math.max(other.crashed, 0.01);
      if (involvesPlayer) {
        const cr = this.camera.matrixWorld.elements; // camera right = column 0
        this.audio?.impact(h.strength, clamp(((h.x - this.player.x) * cr[0] + (h.z - this.player.z) * cr[2]) / 2, -1, 1));
        this.jolt(h.strength);
        this.cam.bump(h.strength * 1.2);
        this.effects.sparksAt(h.x, h.y, h.z, Math.round(8 + h.strength * 30), this.player.vx, this.player.vz);
        if (other.crashed !== undefined) this.nearMiss.set(other, 'hit');
        if (h.strength > 0.2) this.crash();
      }
    }
    for (const e of this.phys.events) {
      if (e.type === 'impact') {
        if (e.strength > 0.35) this.crash();
        this.pv?.onWallImpact(e.strength);
        this.audio?.impact(e.strength, (e.side || 0) * 0.6);
        this.jolt(e.strength);
        this.cam.bump(e.strength);
        this.effects.sparksAt(e.x, e.y, e.z, Math.round(6 + e.strength * 40), this.player.vx, this.player.vz);
      } else if (e.type === 'shift') { this.audio?.shift(e.up); this.pads?.kick(0, 0.2, 70); }
      else if (e.type === 'land') {
        this.audio?.landing(e.strength);
        this.pads?.kick(0.7 * e.strength, 0.5 * e.strength, 180);
        this.cam.bump(e.strength * 0.8);
        if (e.air > 0.55) { this.bonus(`AIR ${e.air.toFixed(1)}s`, 0.12); }
      }
    }
    this.phys.events.length = 0;
    if (this.phys.scrape > 0.3 && Math.random() < 0.6) {
      const v = this.player, F = t.frame(v.s);
      const side = this.phys.scrapeSide || 1;
      const x = v.x + F.rx * side * v.halfW, z = v.z + F.rz * side * v.halfW;
      this.effects.sparksAt(x, v.y + 0.4, z, 2, v.vx, v.vz);
    }

    // ── Bonuses: drift, near miss, overtakes ───────────────────
    this.bonusCooldown = Math.max(0, this.bonusCooldown - dt);
    if (this.phys.drifting) this.lastDrift = this.phys.driftTime;
    else if (this.lastDrift > 1.2) { this.bonus(`DRIFT ${this.lastDrift.toFixed(1)}s`, 0, Math.round(this.lastDrift * 150)); this.lastDrift = 0; }
    else this.lastDrift = 0;
    const psp = Math.hypot(this.player.vx, this.player.vz);
    for (const c of this.traffic.cars) {
      if (!c.active) continue;
      const ds = t.ds(this.player.s, c.s);
      const prev = this.passed.get(c);
      this.passed.set(c, ds);
      if (prev !== undefined && prev > 0 && ds <= 0) {
        const gap = Math.abs(c.lat - this.player.lat) - c.halfW - this.player.halfW;
        const rel = c.dir === 1 ? psp - c.speed : psp + c.speed;
        if (this.nearMiss.get(c) !== 'hit' && gap < 1.4 && rel > 12) {
          this.nearMisses++;
          this.bonus('NEAR MISS', 0.08, 250, true);
          this.audio?.whoosh(clamp((c.lat - this.player.lat) / 3, -1, 1), clamp(rel / 50, 0.3, 1));
        } else if (rel > 20 && gap < 4) this.audio?.whoosh(clamp((c.lat - this.player.lat) / 3, -1, 1), 0.4);
      }
    }

    // Wrong way / finish.
    const F = t.frame(this.player.s);
    const along = Math.cos(this.player.yaw) * F.fx + Math.sin(this.player.yaw) * F.fz;
    const spd = this.player.speed;
    if (started && !this.playerFinished && along < -0.3 && spd > 4) this.wrongWay += dt; else this.wrongWay = 0;
    if (this.wrongWay > 1.5 && this.hud.centerTimer <= 0) this.hud.center('WRONG WAY', 'warn pop', 1);
    // Stuck? Offer the reset key.
    this.stuck = started && !this.playerFinished && spd < 1.5 ? (this.stuck || 0) + dt : 0;
    if (this.stuck > 3 && this.hud.toastTimer <= 0 && !this.pv?.held && !(this.pv?.pursuit.bust > 0)) this.hud.toast(this.input.touch?.visible ? 'STUCK? TAP ↺ TO RESET' : `STUCK? PRESS ${this.pads?.state.connected ? this.pads.label('reset').toUpperCase() : 'R'} TO RESET`, 2);
    this.hud.centerTimer = Math.max(0, this.hud.centerTimer - dt);

    if (this.cruise && started) this.cruiseScore(dt, psp);
    if (this.laps && started && !this.playerFinished) this.lapCheck();
    if (!this.cruise && !this.playerFinished && (this.laps ? this.player.prog : this.player.s) >= this.finishProg && started) {
      this.playerFinished = true;
      this.playerTime = this.time;
      if (this.laps) this.lapTimes.push(this.time - this.lapStart);
      this.park = this.parkSpot(this.player.s, this.player.lat);
      const place = this.standings().findIndex((r) => r.player) + 1;
      this.hud.center(place === 1 ? 'WINNER!' : `${place}${['st', 'nd', 'rd'][place - 1] || 'th'} PLACE`, 'pop go', 2);
      this.audio?.finishFanfare();
      this.state = 'finished';
      this.finishDelay = 3.2;
    }
    if (this.state === 'finished') {
      this.finishDelay -= dt;
      if (this.finishDelay <= 0 && !this.reported) {
        this.reported = true;
        this.onFinish?.(this.results());
      }
    }

    // ── Visual sync ────────────────────────────────────────────
    this.player.sync(t, dt);
    for (const a of this.ais) a.v.sync(t, dt);
    for (const c of this.traffic.cars) if (c.active) c.v.sync(t, dt);
    this.traffic.lod(this.camera.position.x, this.camera.position.z);
    const lightsOn = smoothstep(0.25, 0.6, night);
    this.player.model.setHeadlights?.(Math.max(0.15, lightsOn));
    for (const a of this.ais) a.v.model.setHeadlights?.(Math.max(0.15, lightsOn));
    for (const c of this.traffic.cars) if (c.active) c.v.model.setHeadlights?.(Math.max(0.1, lightsOn));
    this.player.model.setReverse?.(this.phys.gear === -1);
    this.player.model.setBoost?.(this.phys.nitroActive ? 1 : 0);
    for (const a of this.ais) a.v.model.setBoost?.(a.nitroActive ? 1 : 0);
    this.headlight.intensity = lightsOn * 140;
    this.pv?.sync(dt, night, lightsOn);

    // Effects.
    this.extras.clear();
    this.extras.set(this.player, { nitro: this.phys.nitroActive, skid: this.phys.skid, launch: this.state === 'countdown' && inp.throttle > 0.5 });
    for (const a of this.ais) this.extras.set(a.v, { nitro: a.nitroActive, skid: 0 });
    this.effects.update(dt, night, this.extras);

    // Camera.
    let camInp = inp;
    if (this.input.consume('camera')) this.cam.cycle();
    this.cam.update(dt, this.player, { lookBack: inp.lookBack, nitro: this.phys.nitroActive, speed: psp });
    if (this.state === 'countdown') this.introCamera(dt);

    // Tyres past the edge of the tarmac: gravel instead of squeal (and a
    // rumble in the pad). Freeway shoulders, boulevards and kerbed streets
    // are paved to the wall.
    const edge = ROAD_TYPES[ROAD_KEYS[t.roadType[t.idx(this.player.s)]]]?.edge;
    const paved = edge === 'jersey' || edge === 'rail' || edge === 'curb';
    let offroad = paved ? 0 : clamp((Math.abs(this.player.lat) + this.player.halfW * 0.6 - F.hw) / 1.2, 0, 1);
    if (offroad > 0 && t.looseAt) offroad *= t.looseAt(this.player.x, this.player.z); // paved run-off
    this.rumble(psp, offroad);

    // Audio.
    if (this.audio?.ready) {
      const ph = this.phys;
      this.audio.update(dt, {
        rpm: ph.rpm, rpmMax: 7800, throttle: ph.locked ? inp.throttle : ctrl.throttle, gear: ph.gear,
        speed: psp, skid: ph.skid, nitro: ph.nitroActive, onGround: this.player.onGround, scrape: ph.scrape,
        boost: ph.boost, slip: ph.slip, offroad, scrapeSide: ph.scrapeSide,
        ...(ph.electric ? { motor: ph.rpm / MOTOR_MAX, power: ph.powerOut, regen: ph.regen } : {}),
      });
      if (this.phys.nitroActive && !this.wasNitro) this.audio.nitroBurst();
      const camRight = new THREE.Vector3().setFromMatrixColumn(this.camera.matrixWorld, 0);
      const near = this.ais.map((a) => {
        const dx = a.v.x - this.player.x, dz = a.v.z - this.player.z;
        const d = Math.hypot(dx, dz);
        return { dist: d, pan: clamp((dx * camRight.x + dz * camRight.z) / Math.max(d, 1), -1, 1), rpmNorm: clamp(a.speed / 70, 0.2, 1), electric: a.v.kind === 'electric' };
      }).sort((a, b) => a.dist - b.dist).slice(0, 3);
      this.audio.setRivalEngines(near);
      this.pv?.audio(camRight);
    }
    if (this.phys.nitroActive && !this.wasNitro) this.pads?.kick(0.45, 0.6, 260);
    this.wasNitro = this.phys.nitroActive;
    // Tunnels get a concrete echo.
    const inTunnel = this.tunnels.some((g) => this.player.s > g.s0 - 10 && this.player.s < g.s1 + 10);
    if (inTunnel !== this.inTunnel) { this.inTunnel = inTunnel; this.audio?.setEnvironment?.(inTunnel ? 'tunnel' : 'open'); }

    // HUD.
    const standings = this.standings();
    this.hud.update(dt, {
      position: standings.findIndex((r) => r.player) + 1,
      time: this.playerFinished ? this.playerTime : this.time,
      speed: Math.hypot(this.player.vx, this.player.vz),
      gear: this.phys.gear, rpm: this.phys.rpm, nitro: this.phys.nitro, nitroActive: this.phys.nitroActive,
      electric: this.phys.electric, power: this.phys.powerOut,
      // (On a circuit's grid, behind the line, you're at the start of lap 1.)
      s: this.lapS(this.player), started,
      racers: [{ s: this.lapS(this.player) }, ...this.ais.map((a) => ({ s: this.lapS(a) }))],
      laps: this.laps ? { lap: this.lap, of: this.laps, time: this.playerFinished ? null : this.time - this.lapStart, best: this.lapTimes.length ? Math.min(...this.lapTimes) : null } : null,
      player: this.player,
      traffic: this.traffic.cars.filter((c) => c.active).map((c) => c.v),
      racersFull: standings,
      cruise: this.cruise ? { score: this.score, mult: this.mult, multTimer: this.multTimer, dist: this.dist, top: this.topSpeed } : null,
      pursuit: this.pv ? this.pv.hudState() : null,
    });
    return standings;
  }

  bonus(text, nitro, points = 0, chain = false) {
    this.phys.nitro = Math.min(1, this.phys.nitro + nitro);
    if (this.cruise && points) {
      const gained = Math.round(points * this.mult);
      this.score += gained;
      if (chain) { this.mult = Math.min(10, this.mult + 1); this.multTimer = 6; }
      this.hud.toast(`${text}  +${gained.toLocaleString()}`);
      return;
    }
    this.hud.toast(nitro > 0 ? `${text}  +N₂O` : text);
  }

  // Cruise scoring: points for sustained speed, a multiplier from chained
  // near misses that decays if you stop taking risks, lost on a crash.
  cruiseScore(dt, speed) {
    this.topSpeed = Math.max(this.topSpeed, speed);
    const kmh = speed * 3.6;
    if (kmh > 120) this.score += (kmh - 120) * 0.9 * this.mult * dt;
    this.multTimer -= dt;
    if (this.multTimer <= 0 && this.mult > 1) { this.mult -= 1; this.multTimer = 2.5; }
  }

  // Gamepad rumble (Pads: kick is a jolt, feel the steady buzz this frame).
  jolt(strength) {
    if (strength > 0.03) this.pads?.kick(0.25 + 0.75 * strength, 0.5 + 0.5 * strength, 120 + 280 * strength);
  }
  rumble(speed, offroad) {
    const pads = this.pads, ph = this.phys;
    if (!pads?.state.connected) return;
    const sp = clamp(speed / 30, 0, 1);
    const spiked = ph.spiked > 0 ? 0.3 * sp : 0;
    pads.feel(
      Math.max(ph.scrape * 0.55, offroad * 0.35 * sp, spiked),
      Math.max(ph.scrape * 0.45, offroad * 0.45 * sp, ph.skid * 0.15, ph.nitroActive ? 0.15 : 0, spiked),
    );
  }

  crash() {
    if (!this.cruise || this.state === 'countdown') return;
    if (this.mult > 1) this.hud.toast('CRASH — MULTIPLIER LOST', 1.6);
    this.mult = 1;
    this.multTimer = 0;
  }

  // Cruise summary for the results screen.
  cruiseResults() {
    const best = this.score;
    return { cruise: true, score: Math.round(best), dist: this.dist, top: this.topSpeed, time: this.time, nearMisses: this.nearMisses };
  }

  // A finisher's parking spot: the lane nearest where it crossed the line,
  // unless that lane is already filling up, in the row behind anyone parked
  // there. Returns its target speed and lane as functions of s.
  parkSpot(s0, lat0) {
    const t = this.track;
    // A circuit has no end to park at: a slow lap on the right-hand side,
    // easing off for the corners, and the racers still going pass on the left.
    if (this.laps) {
      const cool = (s) => Math.min(20, ...[0, 25, 50].map((d) => t.speedProfile[t.idx(s + d)] * 0.7));
      return { speed: cool, lat: (s) => t.hw[t.idx(s)] - 2.2, kind: 'circuit' };
    }
    const front = Math.max(t.finishS + 60, t.roadEnd - PARK_GAP);
    const f = t.frame(front);
    const n = Math.max(1, Math.floor(f.hw / 2.1)); // four lanes on a freeway
    const w = (2 * f.hw) / n, laneLat = (i) => -f.hw + w * (i + 0.5);
    const rows = (this.parkRows ??= new Array(n).fill(0));
    let lane = 0, best = Infinity;
    for (let i = 0; i < n; i++) {
      const cost = rows[i] * 6 + Math.abs(laneLat(i) - lat0);
      if (cost < best) { best = cost; lane = i; }
    }
    const stopAt = front - rows[lane]++ * PARK_ROW, lat = laneLat(lane);
    return {
      speed: (s) => Math.min(COOL_CRUISE, stopSpeed(stopAt - s, COOL_DECEL)),
      lat: (s) => lerp(lat0, lat, smoothstep(s0, s0 + 300, s)),
      // What the closures hold, as data (read by the Rust port's parity trace).
      kind: 'lane', stopAt, laneLat: lat, s0, lat0,
    };
  }

  // Drives the player's car after the finish, toward its parking spot.
  coolDown(agents, dt) {
    const t = this.track, v = this.player, park = this.park;
    const sp = Math.hypot(v.vx, v.vz);
    const look = 8 + sp * 0.6;
    let target = park.speed(v.s), lat = park.lat(v.s + look);
    // Anything ahead in our path: go round it while we're closing fast
    // (like the rivals do), otherwise queue behind it.
    let block = null, gap = Infinity;
    for (const o of agents) {
      if (o === this.playerBody || o.dir !== 1) continue;
      const ds = t.ds(v.s, o.s);
      const inPath = Math.abs(o.lat - v.lat) < v.halfW + o.halfW + 0.7 || Math.abs(o.lat - lat) < v.halfW + o.halfW + 0.7;
      if (ds > 2 && ds < 60 && ds < gap && inPath) { block = o; gap = ds; }
    }
    this.passTimer = Math.max(0, (this.passTimer || 0) - dt);
    if (block && sp - block.speedAlong > 5) {
      const f = t.frame(v.s), lim = f.hw - v.halfW - 0.4, need = v.halfW + block.halfW + 1.2;
      const sides = [block.lat + need, block.lat - need].filter((l) => Math.abs(l) < lim);
      if (sides.length) { this.passLat = sides.sort((a, b) => Math.abs(a - lat) - Math.abs(b - lat))[0]; this.passTimer = 0.9; }
      else target = Math.min(target, block.speedAlong);
    } else if (block) target = Math.min(target, Math.max(0, block.speedAlong + (gap - 10) * 0.4));
    if (this.passTimer > 0) lat = this.passLat;
    const p = t.pointAt(v.s + look, lat);
    const err = target - sp;
    return {
      throttle: target > 0.5 && err > 0 ? clamp(0.08 + err * 0.15, 0, 0.6) : 0,
      brake: err < -0.3 && sp > 1 ? clamp(-err * 0.12, 0, 0.4) : 0,
      steer: clamp(wrapAngle(Math.atan2(p.z - v.z, p.x - v.x) - v.yaw) * 2, -1, 1),
      handbrake: false, nitro: false, cruise: true,
    };
  }

  resetPlayer() {
    const t = this.track, v = this.player;
    const s = t.loop ? t.wrap(v.s - 5) : Math.max(t.startS, v.s - 5);
    this.crash();
    const f = t.frame(s);
    this.phys.reset(s, clamp(v.lat, -f.hw * 0.5, f.hw * 0.5));
    this.resetCooldown = 2;
    this.cam.snap = true;
  }

  introCamera(dt) {
    // Swing from a front three-quarter view round to the chase position.
    this.introT += dt;
    const k = smoothstep(0, 3.2, this.introT);
    const v = this.player;
    const fx = Math.cos(v.yaw), fz = Math.sin(v.yaw);
    const rx = -fz, rz = fx;
    const ang = lerp(Math.PI * 0.8, 0, k);
    const dist = lerp(7.5, 6.6, k);
    const bx = -fx * Math.cos(ang) + rx * Math.sin(ang), bz = -fz * Math.cos(ang) + rz * Math.sin(ang);
    const pos = new THREE.Vector3(v.x + bx * dist, v.y + lerp(1.3, 2.1, k), v.z + bz * dist);
    this.camera.position.lerp(pos, 1 - k * k);
    this.camera.lookAt(v.x + fx * 1.5 * k, v.y + 0.9, v.z + fz * 1.5 * k);
    this.cam.pos.copy(this.camera.position);
  }

  results() {
    const st = this.standings();
    // Estimate times for anyone still driving.
    const res = st.map((r, i) => {
      let time = r.time;
      if (!r.finished) {
        const rem = this.finishProg - r.prog;
        time = this.time + rem / 45;
      }
      return { place: i + 1, name: r.name, player: r.player, color: r.color, time, estimated: !r.finished };
    });
    if (this.pv) res.pursuit = this.pv.stats();
    if (this.laps) res.laps = { times: this.lapTimes.slice(), best: this.lapTimes.length ? Math.min(...this.lapTimes) : null };
    return res;
  }
}
