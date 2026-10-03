import * as THREE from 'three';
import { clamp, smoothstep } from '../util/math.js';
import { Vehicle } from '../vehicles/Vehicle.js';
import { UNIT_TYPES } from '../vehicles/PoliceDriver.js';
import { farLod } from '../vehicles/Traffic.js';
import { Pursuit, WRECK_PENALTY, topSpeed } from './Pursuit.js';
import { RADIO, DIRS, placeName, radioClips } from './audio/radioLines.js';

// Hot Pursuit inside a Race: builds the police cars and props, feeds the
// Pursuit its racers and collisions, and turns what happens into damage,
// penalty holds, sirens, flashing lights, radio chatter and the HUD.
// Race calls in at a handful of points (see Race.js); everything
// pursuit-specific lives here.

const MODELS = { patrol: ['police', {}], interceptor: ['muscle', { livery: 'police' }], suv: ['policeSuv', {}] };
const RADIO_GAP = 4; // seconds between chatter lines

// Player damage (0..1; a wreck at 1): per unit of hit strength, scaled by
// the other car's mass, and per wall impact.
const DAMAGE_CAR = 0.11;
const DAMAGE_WALL = 0.1;

export class PursuitView {
  // rng, policeRng: seeded streams for the Rust port's reference run
  // (src/parity/sim.js); left out, Pursuit draws from Math.random.
  constructor(race, { heat = 1, cops = 6, flash = true, hq = true, rng, policeRng } = {}) {
    this.race = race;
    this.flash = flash;
    const { track, level, buildVehicle, group } = race;
    let seed = 300;
    const makeUnit = (type) => {
      if (type === 'sawhorse') {
        const m = sawhorseModel();
        group.add(m.root);
        return new Vehicle(m, { kind: 'sawhorse', mass: 60 });
      }
      const [kind, o] = MODELS[type];
      let m;
      try { m = buildVehicle(kind, { ...o, lod: 'low', far: true, seed: seed++ }); } catch { m = buildVehicle('sedan', { color: 0x16181c, lod: 'low', far: true, seed: seed++ }); }
      m.root.traverse((c) => { if (c.isMesh) c.castShadow = true; });
      group.add(m.root);
      return new Vehicle(m, { kind, mass: UNIT_TYPES[type].mass, name: 'Police' });
    };
    this.pursuit = new Pursuit({ track, level, makeUnit, heat, maxUnits: cops, playerTop: topSpeed(race.spec), flash, rng, policeRng });
    window.__pursuit = this.pursuit;
    this.pursuit.setRacers([
      { body: race.playerBody, player: true, name: 'You' },
      ...race.ais.map((a) => ({ body: a, player: false, name: a.name, ai: a })),
    ]);
    this.cars = [...this.pursuit.units, ...this.pursuit.blockCars];
    for (const u of this.cars) race.effects.addCar(u.v);
    // Fetch this race's radio lines now, so each is ready when it's said.
    race.audio?.radioVoice?.prefetch(radioClips({
      zones: track.zones.map((z) => z.name), units: this.pursuit.units.map((u) => u.callsign), names: race.ais.map((a) => a.name),
    }).map((c) => c.id));

    this.damage = 0;
    this.lastHit = new WeakMap();
    this.wrecks = 0;
    this.penalty = 0;      // seconds served
    this.radioT = 0;
    this.lastState = 'patrol';
    this.t = 0;
    race.hud.setPursuit?.(true);

    // One shared flashing light (HQ only) near the closest unit: red and
    // blue on the road, your car and the walls for the price of one light.
    if (hq && flash) {
      this.light = new THREE.PointLight(0xff2030, 0, 38, 1.6);
      race.scene.add(this.light);
    }
    this.spikeMesh = null;
    this.spikeFor = null;
  }

  get held() { return (this.pursuit.player?.hold ?? 0) > 0; }
  // R does nothing while you're being busted or held.
  blocksReset() { return this.held || this.pursuit.bust > 0; }
  bodies() { return this.pursuit.bodies(); }

  // While held: brake to a stop, then sit there.
  holdControls() {
    const v = this.race.player, sp = Math.hypot(v.vx, v.vz);
    this.race.phys.locked = sp < 0.8;
    return { throttle: 0, brake: sp > 0.8 ? 1 : 0, steer: 0, handbrake: false, nitro: false };
  }

  update(dt, agents, started) {
    this.t += dt;
    this.pursuit.update(dt, { cars: agents, time: this.race.time, started });
  }

  // A car-to-car hit (after resolveCollisions): unit health, takedowns,
  // PIT pushes, and damage to the player's car.
  onHit(h) {
    const race = this.race, pb = race.playerBody;
    const pit = this.pursuit.onHit(h);
    if (pit) race.player.yawRate += pit * 2.2;
    if (h.a !== pb && h.b !== pb) return;
    const other = h.a === pb ? h.b : h.a;
    if (other.broken !== undefined) return; // barriers: no damage
    // Police rams are braced, glancing shoves, and rubbing with rivals is
    // racing: both wear the car down more slowly than a crash into traffic.
    // One shunt is one hit: a car can hurt you at most twice a second, not
    // every frame the two stay in contact.
    const last = this.lastHit.get(other) ?? -1;
    if (h.strength > 0.1 && this.t - last > 0.5) this.lastHit.set(other, this.t);
    else return;
    this.hurt(h.strength * DAMAGE_CAR * clamp(other.mass / 1500, 0.5, 2) * (other.police && other.mode !== 'block' ? 0.7 : other.kinematicOnly || other.police ? 1 : 0.5), other.police ? 'police' : other.kinematicOnly ? 'traffic' : 'rival');
  }

  onWallImpact(strength) {
    if (strength > 0.2) this.hurt(strength * DAMAGE_WALL, 'wall');
  }

  hurt(d, why) {
    if (this.log) this.log.push([why, +d.toFixed(3), +this.race.time.toFixed(1)]);
    const race = this.race;
    if (this.held || race.playerFinished || race.state === 'countdown') return;
    this.damage = Math.min(1, this.damage + d);
    race.phys.damage = this.damage;
    if (this.damage >= 1) {
      this.wrecks++;
      this.pursuit.arrest(this.pursuit.player, 'wrecked', WRECK_PENALTY);
    }
  }

  // After collisions: pin the units' models to their track positions.
  writePos() { for (const u of this.pursuit.bodies()) u.writePos(); }

  // Events → HUD text, radio chatter, sound and effects.
  events(dt) {
    const race = this.race, hud = race.hud, audio = race.audio, pu = this.pursuit;
    this.radioT -= dt;
    for (const e of pu.events) {
      switch (e.type) {
        case 'pursuit':
          hud.center('PURSUIT', 'warn pop', 1.4);
          audio?.sirenHorn?.();
          this.say(RADIO.pursuit(this.heading(), this.zoneName()), true);
          break;
        case 'reacquired':
          hud.toast('SPOTTED');
          this.say(RADIO.spotted());
          break;
        case 'cooldown':
          hud.toast('COOLDOWN — STAY OUT OF SIGHT', 2);
          this.say(RADIO.lost());
          break;
        case 'escaped':
          hud.center('ESCAPED', 'pop go', 1.6);
          audio?.escaped?.();
          this.say(RADIO.escaped(), true);
          break;
        case 'heat':
          hud.toast(`HEAT LEVEL ${e.heat}`, 1.8);
          this.say(RADIO.heat(e.heat), true);
          break;
        case 'spotted':
          if (e.player) this.say(RADIO.intercept(e.unit.callsign, this.zoneName()));
          break;
        case 'join':
          if (pu.state === 'pursuit') this.say(RADIO.joining(e.unit.callsign));
          break;
        case 'uturn':
          race.effects.smokeAt(e.x, race.player.y, e.z, 1.4, 0, 0, race.world.sky.night);
          race.effects.smokeAt(e.x, race.player.y, e.z, 1.4, 0, 0, race.world.sky.night);
          break;
        case 'takedown':
          if (e.byPlayer) {
            hud.center('TAKEDOWN', 'pop go', 1.2);
            audio?.takedown?.(1);
            race.cam.bump(1);
            race.pads?.kick(0.8, 0.7, 400);
            this.say(RADIO.unitDown(e.unit.callsign), true);
          }
          break;
        case 'roadblock':
          hud.toast(e.heavy ? 'HEAVY ROADBLOCK AHEAD' : 'ROADBLOCK AHEAD', 2);
          this.say(RADIO.roadblock(e.heavy), true);
          break;
        case 'spikes':
          hud.toast('SPIKE STRIP AHEAD', 2);
          this.say(RADIO.spikes(), true);
          break;
        case 'spiked':
          if (e.player) { hud.center('SPIKED!', 'warn pop', 1.2); audio?.spikePop?.(); race.pads?.kick(0.6, 0.8, 350); this.say(RADIO.spiked(), true); }
          else hud.toast(`${e.name.toUpperCase()} HIT THE SPIKES`);
          break;
        case 'dodge':
          hud.toast(e.what === 'spikes' ? 'SPIKES DODGED' : 'ROADBLOCK DODGED');
          break;
        case 'barrier':
          if (e.player) { race.effects.sparksAt(e.x, race.player.y + 0.8, e.z, 14, race.player.vx, race.player.vz); race.player.vx *= 0.97; race.player.vz *= 0.97; audio?.impact?.(0.35, 0); race.pads?.kick(0.5, 0.6, 250); }
          break;
        case 'busted':
          if (e.player) {
            hud.center('BUSTED', 'warn pop', 2);
            audio?.busted?.();
            race.pads?.kick(0.9, 0.9, 700);
            race.crash();
            this.say(RADIO.busted(), true);
          } else {
            hud.toast(`${e.name.toUpperCase()} BUSTED`, 1.8);
            this.say(RADIO.rivalBusted(e.name));
          }
          break;
        case 'wrecked':
          hud.center('WRECKED', 'warn pop', 2);
          audio?.wrecked?.();
          race.cam.bump(1.2);
          this.say(RADIO.wrecked(), true);
          break;
        case 'release':
          if (e.player) this.release(e);
          break;
      }
    }
    pu.events.length = 0;
  }

  // The penalty is served: back on the road ahead of the police, repaired
  // if it was wrecked.
  release(e) {
    const race = this.race, r = e.racer;
    this.penalty += r.holdTotal;
    race.phys.locked = false;
    race.phys.reset(e.spot.s, e.spot.lat);
    if (r.holdReason === 'wrecked') { this.damage = 0; race.phys.damage = 0; }
    race.phys.spiked = 0;
    race.cam.snap = true;
    race.hud.toast('BACK IN THE RACE', 1.4);
  }

  // A line from RADIO: its text on the HUD (up as long as it takes to say)
  // and its words over the radio.
  say(line, force = false) {
    if (this.radioT > 0 && !force) return;
    this.radioT = RADIO_GAP;
    this.race.hud.radio?.(line.text, Math.max(3, line.text.length / 14));
    this.race.audio?.radioLine?.(line.parts);
  }

  zoneName() {
    const t = this.race.track;
    return placeName(t.zones[t.zone[t.idx(this.race.player.s)]].name);
  }

  heading() {
    // Compass heading from the road direction (+x east, +z south).
    const f = this.race.track.frame(this.race.player.s);
    const a = Math.atan2(f.fz, f.fx);
    return DIRS[((Math.round(a / (Math.PI / 4)) % 8) + 8) % 8];
  }

  // Per-frame visuals: models, sirens, the flash light, smoke and sparks
  // from damage and spiked tyres, the spike strip.
  sync(dt, night, lightsOn) {
    const race = this.race, t = race.track, pu = this.pursuit, v = race.player;
    this.events(dt);
    const mode = (u) => (u.siren === 'flash' && !this.flash ? 'steady' : u.siren);
    let near = null, nearD = 40;
    const cam = race.camera.position;
    for (const u of this.cars) {
      if (!u.active) continue;
      u.v.sync(t, dt);
      farLod(u.v, cam.x, cam.z);
      u.v.model.setHeadlights?.(Math.max(0.15, lightsOn));
      u.v.model.setSiren?.(mode(u), this.t);
      // The additive glow reads from 300 m at night but is a big halo up
      // close by day: dim it in daylight.
      const gu = u.v.model.sirenGlow?.material.uniforms;
      if (gu) { const k = 0.3 + 0.7 * night; gu.uRed.value.multiplyScalar(k); gu.uBlue.value.multiplyScalar(k); }
      if (u.siren === 'flash') {
        const d = Math.hypot(u.v.x - v.x, u.v.z - v.z);
        if (d < nearD) { nearD = d; near = u; }
      }
    }
    for (const b of pu.sawhorses) if (b.active) b.v.sync(t, dt);
    // Disabled units smoke.
    for (const u of pu.units) if (u.active && u.mode === 'disabled' && Math.random() < 0.3) race.effects.smokeAt(u.v.x, u.v.y + 0.6, u.v.z, 0.6, 0, 0, night);

    if (this.light) {
      if (near) {
        const c = near.v.model.sirenColor?.() ?? this.fallbackColor();
        const k = (c.r + c.b) || 0.001;
        this.light.color.setRGB(c.r / k, 0.08, c.b / k);
        this.light.intensity = (c.r + c.b) * 70 * (0.25 + 0.75 * night) * (1 - smoothstep(25, 40, nearD));
        const a = near.v.model.sirenAnchor;
        if (a) a.getWorldPosition(this.light.position); else this.light.position.set(near.v.x, near.v.y + 1.8, near.v.z);
        this.light.position.y += 0.6;
      } else this.light.intensity = 0;
    }

    // Damage: smoke from under the bonnet, heavier and darker as it goes.
    const fx = Math.cos(v.yaw), fz = Math.sin(v.yaw);
    if (this.damage > 0.45 && Math.random() < (this.damage - 0.35) * 1.2) {
      race.effects.smokeAt(v.x + fx * 1.5, v.y + 0.7, v.z + fz * 1.5, this.damage, v.vx, v.vz, Math.min(1, night + (this.damage - 0.6) * 2));
    }
    // Spiked: sparks off the rims.
    if (race.phys.spiked > 0 && Math.hypot(v.vx, v.vz) > 5 && Math.random() < 0.7) {
      for (const side of [-1, 1]) {
        const w = race.effects.wheelWorld(v, side, { x: 0, y: 0, z: 0 });
        race.effects.sparksAt(w.x, w.y + 0.15, w.z, 2, v.vx, v.vz);
      }
    }
    this.syncSpikes();
  }

  fallbackColor() {
    const ph = (this.t * 2.5) % 1;
    return ph < 0.5 ? { r: 1, b: 0 } : { r: 0, b: 1 };
  }

  syncSpikes() {
    const S = this.pursuit.spikes;
    if (S === this.spikeFor) return;
    this.spikeFor = S;
    if (this.spikeMesh) { this.race.group.remove(this.spikeMesh); this.spikeMesh.geometry.dispose(); this.spikeMesh = null; }
    if (!S) return;
    this.spikeMesh = spikeStrip(this.race.track, S.s, S.lat0, S.lat1);
    this.race.group.add(this.spikeMesh);
  }

  audio(camRight) {
    const race = this.race, audio = race.audio, v = race.player;
    if (!audio?.ready) return;
    const list = [];
    for (const u of this.pursuit.units) {
      if (!u.active || u.siren !== 'flash' || u.mode === 'disabled' || u.mode === 'hold' || u.mode === 'standdown') continue;
      const dx = u.v.x - v.x, dz = u.v.z - v.z, d = Math.hypot(dx, dz);
      if (d > 360) continue;
      // Closing speed: how fast the gap between us shrinks.
      const rel = -((dx * (u.v.vx - v.vx) + dz * (u.v.vz - v.vz)) / Math.max(d, 1));
      list.push({ id: u.callsign, dist: d, pan: clamp((dx * camRight.x + dz * camRight.z) / Math.max(d, 1), -1, 1), relSpeed: rel, mode: u.mode === 'search' ? 'hilo' : d < 60 ? 'yelp' : 'wail' });
    }
    list.sort((a, b) => a.dist - b.dist);
    audio.setSirens?.(list.slice(0, 3));
    const st = this.pursuit.state;
    audio.setPursuitMood?.(this.held || race.playerFinished ? 'off' : st === 'patrol' ? 'off' : st);
    audio.setDamage?.(this.damage);
    audio.setSpikedTyres?.(race.phys.spiked > 0, Math.hypot(v.vx, v.vz));
  }

  hudState() {
    const s = this.pursuit.hud(this.damage);
    s.penalties = this.penalty + (this.held ? this.pursuit.player.holdTotal - this.pursuit.player.hold : 0);
    return s;
  }

  stats() {
    return { busts: this.pursuit.busts, wrecks: this.wrecks, takedowns: this.pursuit.takedowns, penalty: this.penalty, heat: this.pursuit.maxHeat };
  }

  dispose() {
    const race = this.race;
    if (this.light) race.scene.remove(this.light);
    if (this.spikeMesh) this.spikeMesh.geometry.dispose();
    race.hud.setPursuit?.(false);
    const a = race.audio;
    a?.setSirens?.([]); a?.setPursuitMood?.('off'); a?.setDamage?.(0); a?.setSpikedTyres?.(false, 0);
    if (window.__pursuit === this.pursuit) window.__pursuit = null;
  }
}

// ── Props ──────────────────────────────────────────────────────────

let sawhorseParts = null;
// A sawhorse barrier: a striped board on two A-frame legs. Built as a
// minimal vehicle model (+Z along the board) so it moves like one.
// Exported (with spikeStrip) for the Rust port's scene export.
export function sawhorseModel() {
  if (!sawhorseParts) {
    const c = document.createElement('canvas');
    c.width = 128; c.height = 16;
    const g = c.getContext('2d');
    for (let i = 0; i < 8; i++) { g.fillStyle = i % 2 ? '#f4f4f0' : '#e2261c'; g.beginPath(); g.moveTo(i * 16, 16); g.lineTo(i * 16 + 16, 0); g.lineTo(i * 16 + 32, 0); g.lineTo(i * 16 + 16, 16); g.fill(); }
    g.fillStyle = '#e2261c'; g.beginPath(); g.moveTo(-16, 16); g.lineTo(0, 0); g.lineTo(16, 0); g.lineTo(0, 16); g.fill();
    const tex = new THREE.CanvasTexture(c);
    tex.colorSpace = THREE.SRGBColorSpace;
    sawhorseParts = {
      board: new THREE.BoxGeometry(0.06, 0.28, 2.4),
      leg: new THREE.BoxGeometry(0.05, 1.0, 0.06),
      boardMat: new THREE.MeshStandardMaterial({ map: tex, roughness: 0.5, emissive: 0xffffff, emissiveMap: tex, emissiveIntensity: 0.18 }),
      legMat: new THREE.MeshStandardMaterial({ color: 0xd8d8d0, roughness: 0.7 }),
    };
  }
  const P = sawhorseParts;
  const root = new THREE.Group(), body = new THREE.Group();
  root.add(body);
  const board = new THREE.Mesh(P.board, P.boardMat);
  board.position.y = 0.9;
  body.add(board);
  for (const z of [-0.95, 0.95]) for (const x of [-1, 1]) {
    const leg = new THREE.Mesh(P.leg, P.legMat);
    leg.position.set(x * 0.2, 0.48, z);
    leg.rotation.z = x * 0.38;
    body.add(leg);
  }
  return { root, body, wheels: [], steerPivots: [], exhausts: [], dims: { length: 2.4, width: 0.5, height: 1.0, wheelRadius: 0.3, wheelBase: 1 } };
}

let spikeMat = null;
// A spike strip across the road from lat0 to lat1 at s: a dark base with
// rows of small steel pyramids.
export function spikeStrip(track, s, lat0, lat1) {
  spikeMat ??= new THREE.MeshStandardMaterial({ color: 0x3a3d42, metalness: 0.7, roughness: 0.35 });
  const f = track.frame(s);
  const w = lat1 - lat0, parts = [];
  const base = new THREE.BoxGeometry(w, 0.03, 0.34);
  base.translate(0, 0.02, 0);
  parts.push(base);
  const n = Math.round(w / 0.16);
  for (let i = 0; i < n; i++) {
    for (const z of [-0.09, 0.09]) {
      const sp = new THREE.ConeGeometry(0.03, 0.09, 4);
      sp.translate(-w / 2 + (i + 0.5) * (w / n) + (z > 0 ? 0.04 : 0), 0.08, z);
      parts.push(sp);
    }
  }
  const geo = mergeAll(parts);
  const mesh = new THREE.Mesh(geo, spikeMat);
  const c = (lat0 + lat1) / 2;
  const p = track.pointAt(s, c);
  mesh.position.set(p.x, p.y + 0.02, p.z);
  // Local X across the road (the frame's right vector), Z along it.
  mesh.rotation.y = -Math.atan2(f.rz, f.rx);
  mesh.receiveShadow = true;
  return mesh;
}

function mergeAll(geos) {
  // Small non-indexed merge (the strips are rebuilt per placement).
  let n = 0;
  const flat = geos.map((g) => { const x = g.index ? g.toNonIndexed() : g; n += x.attributes.position.count; return x; });
  const pos = new Float32Array(n * 3), nor = new Float32Array(n * 3);
  let o = 0;
  for (const g of flat) {
    pos.set(g.attributes.position.array, o * 3);
    nor.set(g.attributes.normal.array, o * 3);
    o += g.attributes.position.count;
    g.dispose();
  }
  const out = new THREE.BufferGeometry();
  out.setAttribute('position', new THREE.BufferAttribute(pos, 3));
  out.setAttribute('normal', new THREE.BufferAttribute(nor, 3));
  return out;
}
