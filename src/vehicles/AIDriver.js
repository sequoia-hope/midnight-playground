import { clamp, lerp, smoothstep } from '../util/math.js';
import { KinematicCar } from './Kinematic.js';

// Rival driver. Follows a precomputed racing line at a speed profile scaled
// by skill, looks ahead for slower cars and oncoming traffic, and picks a
// side to pass on. Mild rubber-banding keeps the pack close to the player.
// Once finished, the race hands it a parking spot (`park`: target speed and
// lane by s) and it cruises there, queueing behind anything in the way.

export class AIDriver extends KinematicCar {
  constructor(vehicle, track, opts = {}) {
    super(vehicle, track);
    this.skill = opts.skill ?? 0.95;
    this.lineFactor = opts.lineFactor ?? 0.9;
    this.bias = opts.bias ?? 0;          // preferred offset from the line
    this.name = opts.name || 'AI';
    this.power = opts.power ?? 500;
    // Random draws (nitro timing). Math.random in the game; the Rust port's
    // reference run passes a seeded stream (src/parity/sim.js).
    this.rng = opts.rng ?? Math.random;
    this.avoid = 0;
    this.avoidTimer = 0;
    this.nitro = 0;
    this.nitroTimer = 4 + this.rng() * 10;
    this.finished = false;
    this.finishTime = null;
    this.throttle = 0;
    this.park = null;
    this.hold = 0;     // Hot Pursuit: seconds left pulled over after a bust
    this.holdLat = null;
    this.spiked = 0;   // Hot Pursuit: seconds left on shredded tyres
  }

  update(dt, ctx) {
    const t = this.track;
    const F = this.frame();
    if (!ctx.started) { this.writePos(); return; }
    const park = this.park;

    // Target speed from the profile a little ahead (braking points).
    const look = clamp(this.speed * 0.5, 6, 40);
    const iA = t.idx(this.s + look);
    let vT = Math.min(t.speedProfile[t.idx(this.s)], t.speedProfile[iA]) * this.skill;
    if (park) vT = Math.min(vT, park.speed(this.s));
    else {
      // Rubber band against the player (on a circuit, by race progress).
      const gap = ctx.playerProg != null ? this.prog - ctx.playerProg : this.s - ctx.playerS;
      vT *= gap > 120 ? lerp(1, 0.9, smoothstep(120, 400, gap)) : gap < -80 ? lerp(1, 1.1, smoothstep(80, 350, -gap)) : 1;
    }
    if (this.stunned > 0) vT *= 0.6;
    // Circuit run-off slows rivals as it does the player.
    if (t.runL && Math.abs(this.lat) > F.hw + 0.4) vT *= 1 - 0.2 * (t.looseAt ? t.looseAt(F.x + F.rx * this.lat, F.z + F.rz * this.lat) : 1);
    // Hot Pursuit: shredded tyres after a spike strip.
    if (this.spiked > 0) { this.spiked -= dt; vT *= 0.75; }

    // Racing line (or the lane we're parking in).
    let latT = park ? park.lat(this.s) : t.racingLine[t.idx(this.s + 8)] * this.lineFactor + this.bias;

    // Look ahead for cars in the way.
    const myW = this.v.halfW;
    const block = findBlock(this, ctx.cars, latT);
    // Parking: queue behind anything in our lane, but still go round a car
    // we're closing on fast.
    if (park) {
      for (const o of ctx.cars) {
        const ds = o.s - this.s;
        if (o === this || o.dir !== 1 || ds <= 0 || ds > 40 || Math.abs(o.lat - this.lat) > myW + o.halfW + 0.5) continue;
        vT = Math.min(vT, Math.max(0, o.speedAlong + (ds - 10) * 0.4));
      }
    }
    if (block && !(park && this.speed - block.speedAlong < 5)) {
      const choice = passLat(this, block, F);
      if (choice !== null) { this.avoid = choice; this.avoidTimer = 0.9; }
      else if (block.dir === 1) vT = Math.min(vT, block.speedAlong - 0.5);
    }
    if (this.avoidTimer > 0) { this.avoidTimer -= dt; latT = this.avoid; }

    // On a circuit the walls stand past the run-off: race on the tarmac.
    const lim = Math.min(Math.min(F.wallR, F.wallL) - myW - 0.35, t.runL ? F.hw - 0.6 : Infinity);
    // Cars alongside (findBlock only sees cars ahead): don't steer into
    // them. Leaning on a car pinned to a wall would shove it along the wall
    // and spin it, so keep off its side, and when there's no room on ours
    // or we're tucked in behind it, drop back instead. (Not roadblock
    // pieces: we aim for their gap, through the sawhorses.)
    for (const o of ctx.cars) {
      if (o === this || o.dir !== 1 || o.gapLat != null) continue;
      const ds = t.ds(this.s, o.s); // > 0: o is ahead
      if (Math.abs(ds) > this.halfL + o.halfL + 1) continue;
      const dl = o.lat - this.lat, clear = myW + o.halfW + 0.4;
      if (Math.abs(dl) > clear + 1.5) continue;
      const room = dl > 0 ? o.lat - clear : o.lat + clear;
      latT = dl > 0 ? Math.min(latT, room) : Math.max(latT, room);
      if (ds > -1 && (Math.abs(dl) < clear - 0.3 || Math.abs(room) > lim)) vT = Math.min(vT, o.speedAlong - 2);
    }

    // Hot Pursuit: busted. Pull over onto the shoulder and wait out the
    // penalty (the race clock keeps running), then rejoin.
    if (this.hold > 0) {
      this.hold -= dt;
      vT = 0;
      latT = this.holdLat ?? this.lat;
    }

    latT = clamp(latT, -lim, lim);

    // Lateral controller (critically damped-ish, limited accel).
    const latA = clamp((latT - this.lat) * 3.2 - this.latVel * 2.6, -7, 7);
    this.latVel += latA * dt;
    this.latVel = clamp(this.latVel, -6, 6);

    // Speed controller.
    this.nitroTimer -= dt;
    if (!park && this.nitroTimer < 0 && this.nitro <= 0 && Math.abs(F.kappa) < 0.004 && this.speed > 30) { this.nitro = 2.5; this.nitroTimer = 12 + this.rng() * 14; }
    const nitroing = this.nitro > 0;
    if (nitroing) { this.nitro -= dt; vT *= 1.12; }
    const acc = Math.min(9, this.power / Math.max(this.speed, 5)) + (nitroing ? 4 : 0);
    const dv = vT - this.speed;
    this.throttle = dv > 0 ? 1 : 0;
    const brake = park && !block ? 4 : this.hold > 0 ? 9 : 12; // ease off after the finish
    this.speed += dv > 0 ? Math.min(dv, acc * dt) : Math.max(dv, -brake * dt);
    this.speed = Math.max(0, this.speed);
    this.v.brakeLight = dv < -1.5 ? 1 : 0;
    this.nitroActive = nitroing;
    this.v.accelLong = dv > 0 ? acc * 0.6 : -Math.min(8, brake);
    this.v.accelLat = this.speed * this.speed * F.kappa;

    this.advance(dt);
    if (!this.finished && this.s >= t.finishS) {
      this.finished = true;
      this.finishTime = ctx.time;
    }
  }

}

// The nearest car ahead that we'd hit on our current lane or on the lane we
// want (latT) within a few seconds: slower cars, stopped cars and anything
// oncoming. Shared by the rivals and the police. skip(o) → true ignores o.
export function findBlock(self, cars, latT, skip = null) {
  const myW = self.v.halfW;
  let block = null, blockGap = Infinity;
  for (const o of cars) {
    if (o === self || (skip && skip(o))) continue;
    const ds = self.track.ds(self.s, o.s);
    const oncoming = o.dir === -1;
    const closing = self.speed - (oncoming ? -o.speedAlong : o.speedAlong);
    // Far enough ahead to get round it at speed (a stopped car at 60 m/s
    // needs ~3 s of warning, not 45 m).
    const range = Math.max(oncoming ? 110 : 45, closing * 3.2);
    if (ds < 2 || ds > range) continue;
    if (closing < 0.5 && !oncoming) continue;
    const tHit = ds / Math.max(closing, 1);
    if (tHit > 3.2) continue;
    if (Math.abs(o.lat - self.lat) < myW + o.halfW + 0.7 || Math.abs(o.lat - latT) < myW + o.halfW + 0.7) {
      if (ds < blockGap) { blockGap = ds; block = o; }
    }
  }
  return block;
}

// A lane that clears `block` on the nearer side, inside the walls, or null
// when neither side fits. A roadblock car points at its gap instead.
export function passLat(self, block, F) {
  if (block.gapLat != null) return block.gapLat;
  const myW = self.v.halfW;
  const need = myW + block.halfW + 1.1;
  const wallR = F.wallR - myW - 0.4, wallL = -(F.wallL - myW - 0.4);
  const right = block.lat + need, left = block.lat - need;
  const okR = right < wallR, okL = left > wallL;
  if (okR && okL) return Math.abs(right - self.lat) < Math.abs(left - self.lat) ? right : left;
  if (okR) return right;
  if (okL) return left;
  return null;
}
