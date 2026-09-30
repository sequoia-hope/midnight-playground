import { clamp, smoothstep } from '../util/math.js';

// Car-to-car contacts. Each car is two circles (front and rear axle) — cheap,
// and good enough for door-to-door racing, rear-enders and T-bones.
// Bodies expose: x, z, yaw, mass, halfL, halfW and velocity()/setVelocity()/
// translate()/addSpin(), implemented for both the physics car and the
// track-coordinate (kinematic) cars.

function circles(b, out) {
  const v = b.v;
  const yaw = v.yaw + (v.visualYaw || 0);
  const fx = Math.cos(yaw), fz = Math.sin(yaw);
  const off = Math.max(0, v.halfL - v.halfW * 1.05);
  const r = v.halfW * 1.08;
  out[0] = v.x + fx * off; out[1] = v.z + fz * off;
  out[2] = v.x - fx * off; out[3] = v.z - fz * off;
  out[4] = r;
  return out;
}

const ca = new Float32Array(5), cb = new Float32Array(5);

export function resolveCollisions(bodies, events) {
  const n = bodies.length;
  for (let i = 0; i < n; i++) {
    const A = bodies[i];
    for (let j = i + 1; j < n; j++) {
      const B = bodies[j];
      if (A.kinematicOnly && B.kinematicOnly && !A.crashy && !B.crashy) continue;
      const dx0 = B.v.x - A.v.x, dz0 = B.v.z - A.v.z;
      const reach = A.v.halfL + B.v.halfL + 0.5;
      if (dx0 * dx0 + dz0 * dz0 > reach * reach) continue;
      if (Math.abs(A.v.y - B.v.y) > 2.5) continue; // one flying over the other
      circles(A, ca); circles(B, cb);
      let best = 0, nx = 0, nz = 0, px = 0, pz = 0;
      for (let a = 0; a < 2; a++) for (let b = 0; b < 2; b++) {
        const ax = ca[a * 2], az = ca[a * 2 + 1], bx = cb[b * 2], bz = cb[b * 2 + 1];
        const dx = bx - ax, dz = bz - az;
        const d = Math.hypot(dx, dz) || 0.001;
        const pen = ca[4] + cb[4] - d;
        if (pen > best) { best = pen; nx = dx / d; nz = dz / d; px = (ax + bx) / 2; pz = (az + bz) / 2; }
      }
      if (best <= 0) continue;
      const ma = A.mass, mb = B.mass;
      const ia = 1 / ma, ib = 1 / mb;
      // Separate.
      const sa = best * ia / (ia + ib), sb = best * ib / (ia + ib);
      A.translate(-nx * sa, -nz * sa);
      B.translate(nx * sb, nz * sb);
      // Impulse.
      const [avx, avz] = A.velocity();
      const [bvx, bvz] = B.velocity();
      const vr = (bvx - avx) * nx + (bvz - avz) * nz;
      if (vr >= 0) continue;
      const e = 0.3;
      const jn = -(1 + e) * vr / (ia + ib);
      // Tangential friction so side-swipes drag a little.
      const tx = -nz, tz = nx;
      const vt = (bvx - avx) * tx + (bvz - avz) * tz;
      const jt = clamp(-vt / (ia + ib), -jn * 0.3, jn * 0.3);
      A.setVelocity(avx - (nx * jn + tx * jt) * ia, avz - (nz * jn + tz * jt) * ia);
      B.setVelocity(bvx + (nx * jn + tx * jt) * ib, bvz + (nz * jn + tz * jt) * ib);
      // Spin from off-centre hits (2D cross product of lever arm × impulse).
      // Only real hits spin: two cars leaning on each other (door to door,
      // or one pinned against a wall) touch every frame at a crawl, and
      // spinning them each time would override the steering.
      const spinK = 0.35 * smoothstep(1, 4, -vr);
      const raX = px - A.v.x, raZ = pz - A.v.z, rbX = px - B.v.x, rbZ = pz - B.v.z;
      A.addSpin(-(raX * nz - raZ * nx) * jn * ia * spinK);
      B.addSpin((rbX * nz - rbZ * nx) * jn * ib * spinK);
      events.push({ type: 'carhit', a: A, b: B, strength: clamp(-vr / 20, 0, 1), x: px, y: (A.v.y + B.v.y) / 2 + 0.6, z: pz });
    }
  }
}

// Adapter so the player's free-physics car looks like any other body.
export class PhysicsBody {
  constructor(vehicle, physics) {
    this.v = vehicle;
    this.phys = physics;
    this.mass = vehicle.mass;
    this.dir = 1;
  }
  get s() { return this.v.s; }
  get lat() { return this.v.lat; }
  get halfW() { return this.v.halfW; }
  get halfL() { return this.v.halfL; }
  get speedAlong() { return this.v.speed; }
  velocity() { return [this.v.vx, this.v.vz]; }
  setVelocity(vx, vz) { this.v.vx = vx; this.v.vz = vz; }
  translate(dx, dz) { this.v.x += dx; this.v.z += dz; }
  addSpin(w) { this.v.yawRate += w; }
}
