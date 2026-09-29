import { clamp, damp } from '../util/math.js';

// Cars that live in track coordinates (s along the road, lat across it):
// rivals and traffic. Much cheaper and more robust than full physics, and
// they can still be shoved around — collisions feed back into speed,
// lateral velocity and a decaying spin.

export class KinematicCar {
  constructor(vehicle, track) {
    this.v = vehicle;
    this.track = track;
    this.s = 0;
    this.lat = 0;
    this.speed = 0;     // along the road in direction `dir`
    this.latVel = 0;
    this.dir = 1;       // +1 with the race, −1 oncoming
    this.spin = 0;      // extra yaw from being hit
    this.spinRate = 0;
    this.F = {};
    this.stunned = 0;   // seconds of reduced control after a hit
    this.freeLat = false; // true: not bound by our road's walls (other carriageway)
    this.yFn = null;      // optional surface height override (s, lat) → y
  }

  frame() { return this.track.frame(this.s, this.F); }
  get halfW() { return this.v.halfW; }
  get halfL() { return this.v.halfL; }
  get speedAlong() { return this.speed; }
  get mass() { return this.v.mass; }

  // World-space velocity (for collisions).
  velocity() {
    const F = this.F;
    return [F.fx * this.speed * this.dir + F.rx * this.latVel, F.fz * this.speed * this.dir + F.rz * this.latVel];
  }

  setVelocity(vx, vz) {
    const F = this.F;
    this.speed = (vx * F.fx + vz * F.fz) * this.dir;
    this.latVel = vx * F.rx + vz * F.rz;
  }

  translate(dx, dz) {
    const F = this.F;
    this.s += dx * F.fx + dz * F.fz;
    this.lat += dx * F.rx + dz * F.rz;
    this.writePos();
  }

  addSpin(w) { this.spinRate += w; this.stunned = Math.max(this.stunned, Math.min(1.5, Math.abs(w) * 0.8)); }

  // Advance along the road; curvature makes inside lines shorter.
  advance(dt) {
    const F = this.frame();
    const k = F.kappa;
    const scale = 1 / Math.max(0.4, 1 - k * this.lat);
    this.s += this.speed * this.dir * dt * scale;
    this.lat += this.latVel * dt;
    // Walls.
    const t = this.track;
    const F2 = this.frame();
    const lim = this.v.halfW + 0.15;
    // Cars on the other carriageway (freeLat) aren't bound by our walls.
    if (!this.freeLat) {
      if (this.lat > F2.wallR - lim) { this.lat = F2.wallR - lim; if (this.latVel > 0) this.latVel *= -0.3; }
      if (this.lat < -(F2.wallL - lim)) { this.lat = -(F2.wallL - lim); if (this.latVel < 0) this.latVel *= -0.3; }
    }
    this.s = t.loop ? t.wrap(this.s) : clamp(this.s, 0, t.roadEnd - 1);
    // Spin decays back to straight.
    this.spin += this.spinRate * dt;
    this.spinRate = damp(this.spinRate, 0, 2.5, dt);
    this.spin = damp(this.spin, 0, this.stunned > 0 ? 0.6 : 2.2, dt);
    this.stunned = Math.max(0, this.stunned - dt);
    this.writePos();
  }

  writePos() {
    const v = this.v, t = this.track;
    const F = this.frame();
    v.s = this.s; v.lat = this.lat;
    v.x = F.x + F.rx * this.lat;
    v.z = F.z + F.rz * this.lat;
    v.y = this.yFn ? this.yFn(this.s, this.lat) : t.surfaceY(this.s, this.lat);
    const base = Math.atan2(F.fz, F.fx) + (this.dir < 0 ? Math.PI : 0);
    const crab = Math.atan2(this.latVel * this.dir, Math.max(4, this.speed));
    v.yaw = base + crab * 0.8;
    v.visualYaw = this.spin;
    v.speed = this.speed;
    const [vx, vz] = this.velocity();
    v.vx = vx; v.vz = vz;
  }
}
