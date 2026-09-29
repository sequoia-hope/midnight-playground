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
    this.avoid = 0;
    this.avoidTimer = 0;
    this.nitro = 0;
    this.nitroTimer = 4 + Math.random() * 10;
    this.finished = false;
    this.finishTime = null;
    this.throttle = 0;
    this.park = null;
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
      // Rubber band against the player.
      const gap = this.s - ctx.playerS;
      vT *= gap > 120 ? lerp(1, 0.9, smoothstep(120, 400, gap)) : gap < -80 ? lerp(1, 1.1, smoothstep(80, 350, -gap)) : 1;
    }
    if (this.stunned > 0) vT *= 0.6;

    // Racing line (or the lane we're parking in).
    let latT = park ? park.lat(this.s) : t.racingLine[t.idx(this.s + 8)] * this.lineFactor + this.bias;

    // Look ahead for cars in the way.
    let block = null, blockGap = Infinity;
    const myW = this.v.halfW;
    for (const o of ctx.cars) {
      if (o === this) continue;
      const ds = o.s - this.s;
      const oncoming = o.dir === -1;
      const range = oncoming ? 110 : 45;
      if (ds < 2 || ds > range) continue;
      const closing = this.speed - (oncoming ? -o.speedAlong : o.speedAlong);
      if (closing < 0.5 && !oncoming) continue;
      const tHit = ds / Math.max(closing, 1);
      if (tHit > 3.2) continue;
      if (Math.abs(o.lat - this.lat) < myW + o.halfW + 0.7 || Math.abs(o.lat - latT) < myW + o.halfW + 0.7) {
        if (ds < blockGap) { blockGap = ds; block = o; }
      }
    }
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
      const need = myW + block.halfW + 1.1;
      const wallR = F.wallR - myW - 0.4, wallL = -(F.wallL - myW - 0.4);
      const right = block.lat + need, left = block.lat - need;
      const okR = right < wallR, okL = left > wallL;
      let choice = null;
      if (okR && okL) choice = Math.abs(right - this.lat) < Math.abs(left - this.lat) ? right : left;
      else if (okR) choice = right;
      else if (okL) choice = left;
      if (choice !== null) { this.avoid = choice; this.avoidTimer = 0.9; }
      else if (block.dir === 1) vT = Math.min(vT, block.speedAlong - 0.5);
    }
    if (this.avoidTimer > 0) { this.avoidTimer -= dt; latT = this.avoid; }

    const lim = Math.min(F.wallR, F.wallL) - myW - 0.35;
    latT = clamp(latT, -lim, lim);

    // Lateral controller (critically damped-ish, limited accel).
    const latA = clamp((latT - this.lat) * 3.2 - this.latVel * 2.6, -7, 7);
    this.latVel += latA * dt;
    this.latVel = clamp(this.latVel, -6, 6);

    // Speed controller.
    this.nitroTimer -= dt;
    if (!park && this.nitroTimer < 0 && this.nitro <= 0 && Math.abs(F.kappa) < 0.004 && this.speed > 30) { this.nitro = 2.5; this.nitroTimer = 12 + Math.random() * 14; }
    const nitroing = this.nitro > 0;
    if (nitroing) { this.nitro -= dt; vT *= 1.12; }
    const acc = Math.min(9, this.power / Math.max(this.speed, 5)) + (nitroing ? 4 : 0);
    const dv = vT - this.speed;
    this.throttle = dv > 0 ? 1 : 0;
    const brake = park && !block ? 4 : 12; // ease off after the finish
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
