import * as THREE from 'three';
import { clamp, damp } from '../util/math.js';

// Shared state for every car on the road — player, rivals and traffic.
// Heading convention: yaw θ, forward = (cos θ, sin θ) in the XZ plane,
// right = (−sin θ, cos θ); a positive yaw rate turns right.

const _m = new THREE.Matrix4();
const _X = new THREE.Vector3(), _Y = new THREE.Vector3(), _Z = new THREE.Vector3();
const _A = new THREE.Vector3(), _T = new THREE.Vector3();

export class Vehicle {
  constructor(model, { kind = 'sports', mass = 1400, name = '', color = 0xffffff } = {}) {
    this.model = model;
    this.kind = kind;
    this.name = name;
    this.color = color;
    this.mass = mass;
    const d = model.dims;
    this.halfW = d.width / 2;
    this.halfL = d.length / 2;
    this.radius = d.width / 2;
    this.x = 0; this.y = 0; this.z = 0;
    this.yaw = 0;
    this.vx = 0; this.vz = 0; this.vy = 0;
    this.yawRate = 0;
    this.s = 0; this.lat = 0;
    this.speed = 0;          // signed forward speed
    this.steerAngle = 0;
    this.accelLong = 0;
    this.accelLat = 0;
    this.pitchV = 0; this.rollV = 0;
    this.pitch = 0; this.roll = 0;
    this.onGround = true;
    this.airborne = 0;
    this.brakeLight = 0;
    this.visualYaw = 0;      // extra yaw for spins (kinematic cars)
    this.alive = true;
  }

  get fx() { return Math.cos(this.yaw); }
  get fz() { return Math.sin(this.yaw); }

  place(track, s, lat, yawOffset = 0) {
    const p = track.pointAt(s, lat);
    const f = track.frame(s);
    this.x = p.x; this.y = p.y; this.z = p.z;
    this.s = s; this.lat = lat;
    this.yaw = Math.atan2(f.fz, f.fx) + yawOffset;
    this.vx = this.vz = this.vy = 0;
    this.yawRate = 0;
    this.speed = 0;
  }

  // Push position/orientation into the three.js model.
  sync(track, dt) {
    const root = this.model.root;
    const f = track.frame(this.s);
    // Surface normal from the road frame (grade along, bank across).
    _T.set(f.fx, f.grade, f.fz).normalize();
    _A.set(f.rx, -f.bank, f.rz).normalize();
    _Y.crossVectors(_A, _T).normalize();
    const yaw = this.yaw + this.visualYaw;
    _Z.set(Math.cos(yaw), 0, Math.sin(yaw));
    // In the air, pitch the nose along the flight path a little.
    if (!this.onGround) {
      const sp = Math.hypot(this.vx, this.vz) + 0.01;
      const tilt = clamp(this.vy / sp, -0.35, 0.35) * 0.6;
      _Z.y += tilt;
    }
    _Z.addScaledVector(_Y, -_Z.dot(_Y) * (this.onGround ? 1 : 0.4)).normalize();
    _X.crossVectors(_Y, _Z).normalize();
    _Y.crossVectors(_Z, _X).normalize();
    _m.makeBasis(_X, _Y, _Z);
    root.quaternion.setFromRotationMatrix(_m);
    root.position.set(this.x, this.visY ?? this.y, this.z);

    // Suspension feel: body pitch/roll springs driven by accelerations.
    const tp = clamp(-this.accelLong * 0.0045, -0.055, 0.055);
    const tr = clamp(-this.accelLat * 0.0042, -0.065, 0.065);
    const k = 90, c = 11;
    this.pitchV += ((tp - this.pitch) * k - this.pitchV * c) * dt;
    this.rollV += ((tr - this.roll) * k - this.rollV * c) * dt;
    this.pitch += this.pitchV * dt;
    this.roll += this.rollV * dt;
    const body = this.model.body;
    body.rotation.x = this.pitch;
    body.rotation.z = this.roll;

    const wr = this.model.dims.wheelRadius || 0.34;
    for (const w of this.model.wheels) w.rotation.x += (this.speed / (w.userData.radius || wr)) * dt;
    for (const p of this.model.steerPivots || []) p.rotation.y = -this.steerAngle;
    this.model.setBrake?.(this.brakeLight);
  }
}
