import * as THREE from 'three';
import { clamp, damp, lerp, smoothstep } from '../util/math.js';

// Chase camera. Trails the car along a blend of its heading and its actual
// direction of travel (so drifts read as drifts), widens FOV with speed and
// nitro, shakes on impacts, and never dips below the ground.

const MODES = [
  { name: 'chase', back: 6.4, up: 2.05, look: 1.05, ahead: 3.5, fov: 60 },
  { name: 'far', back: 9.5, up: 3.1, look: 1.2, ahead: 4, fov: 58 },
  { name: 'bumper', back: -1.2, up: 0.72, look: 0.7, ahead: 20, fov: 72 },
];

export class CameraRig {
  constructor(camera, track, terrain) {
    this.camera = camera;
    this.track = track;
    this.terrain = terrain;
    this.mode = 0;
    this.pos = new THREE.Vector3();
    this.lookAt = new THREE.Vector3();
    this.dirX = 1; this.dirZ = 0;
    this.shake = 0;
    this.fov = 60;
    this.t = 0;
    this.snap = true;
  }

  cycle() { this.mode = (this.mode + 1) % MODES.length; this.snap = true; }

  bump(amount) { this.shake = Math.min(1.2, this.shake + amount); }

  update(dt, car, { lookBack = false, nitro = false, speed = 0 } = {}) {
    const M = MODES[this.mode];
    this.t += dt;
    const v = car;
    const fx = Math.cos(v.yaw), fz = Math.sin(v.yaw);
    const sp = Math.hypot(v.vx, v.vz);
    // Direction the camera trails along.
    let tx = fx, tz = fz;
    if (sp > 3 && M.name !== 'bumper') {
      const vxn = v.vx / sp, vzn = v.vz / sp;
      const w = 0.45 * smoothstep(3, 12, sp);
      tx = lerp(fx, vxn, w); tz = lerp(fz, vzn, w);
      if (v.vx * fx + v.vz * fz < -1) { tx = fx; tz = fz; } // reversing
    }
    const l = Math.hypot(tx, tz) || 1;
    tx /= l; tz /= l;
    const k = this.snap ? 1 : 1 - Math.exp(-(M.name === 'bumper' ? 30 : 7) * dt);
    this.dirX += (tx - this.dirX) * k;
    this.dirZ += (tz - this.dirZ) * k;
    const dl = Math.hypot(this.dirX, this.dirZ) || 1;
    let dx = this.dirX / dl, dz = this.dirZ / dl;
    if (lookBack) { dx = -dx; dz = -dz; }

    const back = M.back + (M.name === 'bumper' ? 0 : clamp(sp * 0.018, 0, 1.3));
    const desired = new THREE.Vector3(v.x - dx * back, v.y + M.up + (M.name === 'bumper' ? 0 : sp * 0.004), v.z - dz * back);
    if (M.name === 'bumper') { desired.set(v.x + fx * 1.2, v.y + M.up, v.z + fz * 1.2); if (lookBack) desired.set(v.x - fx * 2.3, v.y + M.up + 0.3, v.z - fz * 2.3); }

    if (this.snap) { this.pos.copy(desired); this.snap = false; }
    else {
      const kp = M.name === 'bumper' ? 1 : 1 - Math.exp(-14 * dt);
      this.pos.x += (desired.x - this.pos.x) * kp;
      this.pos.z += (desired.z - this.pos.z) * kp;
      const ky = M.name === 'bumper' ? 1 : 1 - Math.exp(-9 * dt);
      this.pos.y += (desired.y - this.pos.y) * ky;
    }
    // Keep above the ground and the road.
    const g = Math.max(this.terrain.heightAt(this.pos.x, this.pos.z), this.track.surfaceY(v.s, v.lat)) + (M.name === 'bumper' ? 0.3 : 0.9);
    if (this.pos.y < g) this.pos.y = g;

    const aheadDir = lookBack ? -1 : 1;
    this.lookAt.set(v.x + fx * M.ahead * aheadDir, v.y + M.look, v.z + fz * M.ahead * aheadDir);
    if (M.name === 'bumper') this.lookAt.set(v.x + dx * 30, v.y + 0.9 + (v.vy || 0) * 0.3, v.z + dz * 30);

    const cam = this.camera;
    cam.position.copy(this.pos);
    // Shake.
    this.shake = damp(this.shake, 0, 4, dt);
    const rumble = smoothstep(40, 85, sp) * 0.035 + (nitro ? 0.05 : 0);
    const sh = this.shake * 0.35 + rumble;
    if (sh > 0.001) {
      cam.position.x += (Math.sin(this.t * 53) + Math.sin(this.t * 31)) * sh * 0.5;
      cam.position.y += (Math.sin(this.t * 47) + Math.sin(this.t * 23)) * sh * 0.35;
    }
    cam.lookAt(this.lookAt);
    let fovT = M.fov + smoothstep(10, 85, sp) * 16 + (nitro ? 7 : 0);
    // Narrow (portrait) screens: widen the vertical angle so the horizontal
    // view stays close to a landscape screen's, short of fisheye.
    if (cam.aspect < 1.2) fovT = Math.min(96, (2 * Math.atan((Math.tan((fovT * Math.PI) / 360) * 1.2) / cam.aspect) * 180) / Math.PI);
    this.fov = damp(this.fov, fovT, 3, dt);
    if (Math.abs(cam.fov - this.fov) > 0.01) { cam.fov = this.fov; cam.updateProjectionMatrix(); }
  }
}
