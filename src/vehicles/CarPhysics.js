import { clamp, lerp, smoothstep, damp, wrapAngle } from '../util/math.js';

// Arcade driving model for the player car.
//
// Heading and velocity are separate: steering turns the heading (bicycle
// model, capped by tyre grip), and tyre grip then pulls the velocity round
// to match. Normal grip is strong enough that the car goes where it points;
// the handbrake drops rear grip so the heading outruns the velocity and the
// car drifts. A drift-angle assist keeps slides catchable on a keyboard.
//
// The car is always kept inside the road corridor (track.wallL/wallR), which
// the scenery dresses as rock faces, guardrails, fences or barriers.

// power: m²/s³ per kg (accel = power / speed); launch: the traction limit on
// that accel (m/s²); vmax: a speed limiter (m/s). electric: single-speed
// motor, regenerative braking charges the boost tank. turbo: boost builds
// with revs and throttle (lag), and scales the power. gearScale shortens
// (< 1) or lengthens the gear ratios.
export const CAR_SPECS = {
  sports: { label: 'Vento GT', blurb: 'Balanced all-rounder', power: 480, grip: 13.2, driftGrip: 8.5, mass: 1350, color: 0xd81e36 },
  muscle: { label: 'Brawler 69', blurb: 'Big power, loose tail', power: 545, grip: 12.0, driftGrip: 6.8, mass: 1600, color: 0x1f4fd8 },
  super: { label: 'Stiletto R', blurb: 'Grip and precision', power: 515, grip: 14.4, driftGrip: 9.6, mass: 1300, color: 0xf2b705 },
  rally: { label: 'Kestrel RS', blurb: 'Turbo 4WD: launches hard, slides on demand', power: 470, launch: 10.8, vmax: 71, gearScale: 0.84, turbo: true, grip: 14.0, driftGrip: 10.6, mass: 1250, color: 0xee5a12 },
  electric: { label: 'Ion Arc', blurb: 'Electric: instant torque, braking charges boost', power: 500, launch: 11.8, vmax: 71, electric: true, grip: 13.6, driftGrip: 8.8, mass: 1850, color: 0xdfe7ee },
};

const G = 9.81;
const GEAR_TOP = [0, 15, 26, 38, 51, 64, 84];
const IDLE = 900, REDLINE = 7800;
// The electric motor: rpm at the speed limiter (single reduction gear).
export const MOTOR_MAX = 16000;

export class CarPhysics {
  constructor(vehicle, track, spec) {
    this.v = vehicle;
    this.track = track;
    this.spec = spec;
    this.P = {};
    this.F = {};
    this.gear = 1;
    this.rpm = IDLE;
    this.shiftTimer = 0;
    this.nitro = 0.5;
    this.nitroActive = false;
    this.drifting = false;
    this.driftTime = 0;
    this.slip = 0;         // slip angle (rad)
    this.skid = 0;         // 0..1 for audio/fx
    this.scrape = 0;       // 0..1 wall contact
    this.airTime = 0;
    this.events = [];
    this.locked = false;   // during countdown
    this.wheelBase = vehicle.model.dims.wheelBase || 2.6;
    this.electric = !!spec.electric;
    this.gearTop = GEAR_TOP.map((v) => v * (spec.gearScale ?? 1));
    this.boost = 0;        // turbo boost 0..1
    this.powerOut = 0;     // electric: drive power (+) or regen (−), kW
    this.regen = 0;        // electric: 0..1 regen braking strength
    if (this.electric) this.rpm = 0;
    // Hot Pursuit. damage 0..1: past half, the engine loses power (a wreck
    // at 1 is the race's call). spiked: seconds left on shredded tyres
    // after a spike strip: less grip and a lower top speed. Both are 0
    // outside Hot Pursuit, which leaves the car exactly as it was.
    this.damage = 0;
    this.spiked = 0;
  }

  reset(s, lat = 0) {
    const v = this.v;
    v.place(this.track, s, lat);
    this.gear = 1;
    this.rpm = this.electric ? 0 : IDLE;
    this.drifting = false;
    this.onGround = true;
    v.onGround = true;
  }

  // Equal steps of about 1/120 s that add up to exactly the frame's time. A
  // fixed step with a carried-over remainder took 1, 2 or 3 steps per 60 Hz
  // frame (and 0 or 1 at 144 Hz) with nothing drawn in between, so at speed
  // the car lurched against the camera, the traffic and the rivals, which all
  // move by the frame's own time. A step may run 10 % long, so a 60 Hz frame
  // a hair over 1/60 s is still two steps, not three.
  update(frameDt, inp) {
    const H = 1 / 120;
    if (!(frameDt > 0)) return;
    const n = Math.min(12, Math.ceil(frameDt / (H * 1.1)));
    const h = Math.min(H * 1.1, frameDt / n);
    for (let i = 0; i < n; i++) this.step(h, inp);
  }

  engineAccel(v) {
    const spec = this.spec;
    let a = Math.min(spec.launch ?? 9.5, spec.power / Math.max(v, 5));
    if (spec.turbo) a *= 0.8 + 0.2 * this.boost;
    if (spec.vmax) a *= 1 - smoothstep(spec.vmax - 5, spec.vmax, v);
    if (this.damage > 0.5) a *= 1 - 0.35 * smoothstep(0.5, 1, this.damage);
    if (this.spiked > 0) { const vm = (spec.vmax ?? 76) * 0.75; a *= 1 - smoothstep(vm - 5, vm, v); }
    return a;
  }

  step(dt, inp) {
    const v = this.v, t = this.track;
    let spec = this.spec;
    if (this.spiked > 0) {
      this.spiked = Math.max(0, this.spiked - dt);
      spec = this.spikedSpec ??= { ...spec, grip: spec.grip * 0.7, driftGrip: spec.driftGrip * 0.7 };
    }
    const P = t.project(v.x, v.z, v.s, this.P);
    v.s = P.s; v.lat = P.lat;
    const F = t.frame(v.s, this.F);

    let fx = Math.cos(v.yaw), fz = Math.sin(v.yaw);
    let rx = -fz, rz = fx;
    let vLong = v.vx * fx + v.vz * fz;
    let vLat = v.vx * rx + v.vz * rz;
    const speed = Math.hypot(v.vx, v.vz);

    // Steering: less lock at speed.
    const steerMax = lerp(0.6, 0.15, smoothstep(0, 65, speed));
    const target = inp.steer * steerMax;
    v.steerAngle += clamp(target - v.steerAngle, -3.2 * dt, 3.2 * dt);

    // Gearbox (automatic). Mostly for sound and the shift cut. inp.cruise
    // short-shifts and holds taller gears, like an automatic at part throttle.
    this.shiftTimer = Math.max(0, this.shiftTimer - dt);
    const upRpm = inp.cruise ? 3800 : 7350, downK = inp.cruise ? 0.45 : 0.7;
    const GT = this.gearTop;
    if (vLong < -0.5 && inp.brake > 0) this.gear = -1;
    else if (this.gear === -1 && vLong > -0.2) this.gear = 1;
    if (this.electric) {
      // One fixed reduction: motor speed follows the wheels, no shifts.
      if (this.gear !== -1) this.gear = 1;
      this.rpm = damp(this.rpm, this.locked ? 0 : Math.min(MOTOR_MAX * 1.02, (Math.abs(vLong) / spec.vmax) * MOTOR_MAX), 30, dt);
    } else if (this.gear >= 1) {
      const ratio = Math.abs(vLong) / GT[this.gear];
      let rpm = IDLE + ratio * (REDLINE - IDLE);
      rpm = Math.max(rpm, IDLE + inp.throttle * 3800 * (1 - smoothstep(4, 14, speed)));
      if (this.locked) rpm = IDLE + inp.throttle * 6200;
      this.rpm = damp(this.rpm, Math.min(rpm, REDLINE + 150), 18, dt);
      if (!this.locked && this.rpm > upRpm && this.gear < 6 && this.shiftTimer === 0 && Math.abs(vLong) > GT[this.gear] * (downK + 0.08)) {
        this.gear++; this.shiftTimer = 0.2; this.events.push({ type: 'shift', up: true });
      } else if (this.gear > 1 && Math.abs(vLong) < GT[this.gear - 1] * downK && this.shiftTimer === 0) {
        this.gear--; this.shiftTimer = 0.12; this.events.push({ type: 'shift', up: false });
      }
    } else {
      this.rpm = damp(this.rpm, IDLE + Math.abs(vLong) / 12 * 4000, 10, dt);
    }

    // Turbo: boost follows revs under throttle, with lag; lifting dumps it
    // (the audio plays the blow-off from the drop).
    if (spec.turbo) {
      const want = inp.throttle * smoothstep(2600, 5200, this.rpm) * (this.shiftTimer > 0.08 ? 0.3 : 1);
      this.boost = damp(this.boost, want, want > this.boost ? 2.2 : 9, dt);
    }

    if (this.locked) { v.vx = v.vz = 0; v.yawRate = 0; this.nitroActive = false; this.skid = 0; this.powerOut = 0; return; }

    const onGround = v.onGround;
    this.nitroActive = false;
    if (onGround) {
      // ── Longitudinal ─────────────────────────────────────────
      let a = 0;
      const throttle = this.shiftTimer > 0.08 ? inp.throttle * 0.2 : inp.throttle;
      if (vLong > -0.5) {
        if (inp.brake > 0 && vLong > 0.8) a -= 15 * inp.brake;
        else if (inp.brake > 0 && vLong > -12) a -= 7 * inp.brake;
        a += throttle * this.engineAccel(Math.abs(vLong));
      } else {
        if (inp.throttle > 0) a += 14 * inp.throttle;
        else if (inp.brake > 0 && vLong > -13) a -= 6 * inp.brake;
      }
      if (inp.nitro && this.nitro > 0 && vLong > 4 && inp.throttle > 0.1) {
        a += 6;
        this.nitro = Math.max(0, this.nitro - dt / 6);
        this.nitroActive = true;
      }
      if (this.electric) {
        // Regenerative braking: the motor does some of the slowing and
        // tops up the boost; lifting off regens gently too.
        const moving = smoothstep(3, 18, vLong);
        const coast = inp.throttle < 0.05 && inp.brake < 0.05 ? 0.25 : 0;
        this.regen = damp(this.regen, vLong > 3 ? Math.max(inp.brake, coast) * moving : 0, 12, dt);
        this.nitro = Math.min(1, this.nitro + dt * 0.075 * this.regen);
        const drive = vLong > -0.5 ? throttle * this.engineAccel(Math.abs(vLong)) + (this.nitroActive ? 6 : 0) : 0;
        this.powerOut = damp(this.powerOut, (spec.mass * (drive - this.regen * 7) * Math.max(0, vLong)) / 1000, 14, dt);
      }
      a -= 0.00115 * vLong * Math.abs(vLong) + 0.01 * vLong;
      // Circuit run-off (surveyed tracks carry run-off grades): off the
      // tarmac the tyres plough through dirt and dry grass.
      const off = t.runL ? clamp((Math.abs(v.lat) - F.hw - 0.4) / 1.5, 0, 1) : 0;
      this.offTrack = off;
      if (off > 0) a -= off * Math.sign(vLong) * Math.min(Math.abs(vLong) * 4, 1.2 + 0.0025 * vLong * vLong);
      if (inp.handbrake) a -= Math.sign(vLong) * 3;
      if (inp.throttle < 0.05 && inp.brake < 0.05) a -= Math.sign(vLong) * Math.min(0.9, Math.abs(vLong));
      const gradeAlong = F.grade * (fx * F.fx + fz * F.fz);
      a -= G * gradeAlong / Math.sqrt(1 + gradeAlong * gradeAlong);
      const before = vLong;
      vLong += a * dt;
      // Braking or coasting through zero stops the car rather than rolling
      // it the other way; holding the brake from a standstill reverses.
      const reversing = inp.brake > 0.05 && Math.abs(before) < 0.05;
      if (inp.throttle < 0.05 && !reversing && Math.sign(before) !== Math.sign(vLong) && Math.abs(before) < 1) vLong = 0;
      v.accelLong = a;
      v.brakeLight = inp.brake > 0 && vLong > 0.5 ? 1 : 0;

      // ── Yaw ──────────────────────────────────────────────────
      const slip = Math.atan2(vLat, Math.max(Math.abs(vLong), 0.5));
      // Nearly stopped (e.g. nose in a wall): wheelspin still lets you pivot.
      const vYaw = Math.abs(vLong) < 2.5 && (inp.throttle > 0.3 || inp.brake > 0.3) ? Math.sign(vLong || 1) * 2.5 * (inp.brake > 0.3 && vLong <= 0.5 ? -1 : 1) : vLong;
      let yawTarget = vYaw * Math.tan(v.steerAngle) / this.wheelBase;
      const hb = inp.handbrake && speed > 9;
      if (hb) yawTarget = yawTarget * 1.6 + Math.sign(v.steerAngle) * 0.35 * smoothstep(9, 20, speed);
      // Arcade grip: ~2 g of cornering before the tyres give up.
      const gripLim = (this.drifting || hb ? spec.grip * 1.8 : spec.grip * 1.5) / Math.max(speed, 4);
      yawTarget = clamp(yawTarget, -gripLim, gripLim);
      // Drift assist: stop the slide angle running away past ~35°.
      if (this.drifting || hb) {
        const maxSlip = 0.62;
        const room = maxSlip - Math.abs(slip);
        if (Math.sign(yawTarget) === -Math.sign(slip) && slip !== 0) {
          const omegaV = spec.driftGrip / Math.max(speed, 5);
          const cap = Math.max(0, omegaV + 3 * room);
          yawTarget = Math.sign(yawTarget) * Math.min(Math.abs(yawTarget), cap);
        }
      }
      const resp = this.drifting ? 5 : 10;
      v.yawRate = damp(v.yawRate, yawTarget, resp, dt);
      // Rotate heading, keep the world velocity, re-express it.
      const wvx = vLong * fx + vLat * rx, wvz = vLong * fz + vLat * rz;
      v.yaw += v.yawRate * dt;
      fx = Math.cos(v.yaw); fz = Math.sin(v.yaw); rx = -fz; rz = fx;
      vLong = wvx * fx + wvz * fz;
      vLat = wvx * rx + wvz * rz;

      // ── Lateral grip ─────────────────────────────────────────
      const gripA = (hb ? 4.5 : this.drifting ? spec.driftGrip * 1.15 : spec.grip * 2.2) * (1 - 0.3 * off);
      const dv = Math.min(Math.abs(vLat), gripA * dt);
      vLat -= Math.sign(vLat) * dv;
      if (hb || this.drifting) vLong += Math.sign(vLong) * dv * 0.3; // keep some momentum
      v.accelLat = v.yawRate * speed;

      this.slip = Math.atan2(vLat, Math.max(Math.abs(vLong), 0.5));
      if (!this.drifting && speed > 12 && Math.abs(this.slip) > 0.18) this.drifting = true;
      if (this.drifting && (Math.abs(this.slip) < 0.06 || speed < 7)) this.drifting = false;
      if (this.drifting) {
        this.driftTime += dt;
        // (The electric car tops up mostly from regen, so drifting pays half.)
        this.nitro = Math.min(1, this.nitro + dt * 0.1 * (this.electric ? 0.5 : 1) * clamp(Math.abs(this.slip) / 0.4, 0.3, 1.2) * smoothstep(12, 30, speed));
      } else this.driftTime = 0;
      this.skid = clamp(Math.max(Math.abs(vLat) / 5, hb ? speed / 25 : 0, inp.brake > 0.5 && speed > 18 ? 0.35 : 0), 0, 1) * smoothstep(3, 8, speed);

      v.vx = vLong * fx + vLat * rx;
      v.vz = vLong * fz + vLat * rz;
    } else {
      // Airborne: ballistic, keep spin, a hint of air control.
      v.yawRate *= 1 - 0.8 * dt;
      v.yaw += (v.yawRate + inp.steer * 0.25) * dt;
      v.vx *= 1 - 0.02 * dt; v.vz *= 1 - 0.02 * dt;
      this.skid = 0;
      this.airTime += dt;
      v.accelLong = 0; v.accelLat = 0;
    }
    v.speed = v.vx * Math.cos(v.yaw) + v.vz * Math.sin(v.yaw);

    // ── Integrate position ─────────────────────────────────────
    const prevGround = t.surfaceY(v.s, v.lat);
    v.x += v.vx * dt;
    v.z += v.vz * dt;
    t.project(v.x, v.z, v.s, P);
    v.s = P.s; v.lat = P.lat;
    this.collideWalls(dt, P);

    // Vertical: follow the road unless it falls away faster than gravity.
    const ground = t.surfaceY(v.s, v.lat);
    const groundVy = (ground - prevGround) / dt;
    v.vy -= G * dt;
    const yNew = v.y + v.vy * dt;
    if (yNew <= ground + 0.001) {
      if (!v.onGround) {
        const impact = groundVy - v.vy;
        if (impact > 2.5) this.events.push({ type: 'land', strength: clamp(impact / 12, 0, 1), air: this.airTime });
        v.pitchV -= impact * 0.02;
        this.airTime = 0;
      }
      v.y = ground;
      v.vy = groundVy;
      v.onGround = true;
    } else {
      // Road falling away faster than gravity. Suspension travel keeps the
      // tyres on the road for the first 30 cm; past that we're flying.
      v.y = yNew;
      if (v.onGround && yNew - ground > 0.3) v.onGround = false;
    }
    v.visY = v.onGround ? ground : v.y;
  }

  collideWalls(dt, P) {
    const v = this.v, t = this.track;
    const F = t.frame(v.s, this.F);
    const trackYaw = Math.atan2(F.fz, F.fx);
    const rel = wrapAngle(v.yaw - trackYaw);
    const ext = v.halfW * Math.abs(Math.cos(rel)) + v.halfL * Math.abs(Math.sin(rel));
    const limR = F.wallR - ext, limL = -(F.wallL - ext);
    let side = 0, pen = 0;
    if (v.lat > limR) { side = 1; pen = v.lat - limR; }
    else if (v.lat < limL) { side = -1; pen = v.lat - limL; }
    this.scrape = Math.max(0, this.scrape - dt * 4);
    if (side !== 0) {
      v.x -= F.rx * pen; v.z -= F.rz * pen;
      v.lat -= pen;
      const vn = (v.vx * F.rx + v.vz * F.rz) * side; // into the wall
      if (vn > 0) {
        const e = 0.25;
        v.vx -= F.rx * side * vn * (1 + e);
        v.vz -= F.rz * side * vn * (1 + e);
        // Scrub speed along the wall and swing the nose parallel.
        const scrub = 1 - clamp(0.05 + vn * 0.025, 0, 0.35);
        v.vx *= scrub; v.vz *= scrub;
        const pointsIn = Math.sign(rel) === side && Math.abs(rel) < Math.PI / 2;
        if (pointsIn) v.yaw -= rel * clamp(vn * 0.06, 0.05, 0.5);
        // The wall stops the touching corner swinging further into it (a
        // tail slap kills the spin), but never the rotation back toward
        // parallel: steering a nose-in car off the wall.
        if (v.yawRate * rel > 0) v.yawRate *= 0.5;
        if (vn > 1.5) {
          const px = v.x + F.rx * side * ext, pz = v.z + F.rz * side * ext;
          this.events.push({ type: 'impact', strength: clamp(vn / 18, 0, 1), x: px, y: v.y + 0.5, z: pz, side });
        }
      }
      // Continuous scraping friction.
      const sp = Math.hypot(v.vx, v.vz);
      if (sp > 1) {
        const k = Math.max(0, 1 - 4 * dt / sp);
        v.vx *= k; v.vz *= k;
        this.scrape = Math.min(1, 0.4 + sp / 60);
        this.scrapeSide = side;
      }
    }
    // Ends of the road (loops have none).
    if (t.loop) return;
    if (v.s < 1) { v.x += F.fx * (1 - v.s); v.z += F.fz * (1 - v.s); const vf = v.vx * F.fx + v.vz * F.fz; if (vf < 0) { v.vx -= F.fx * vf; v.vz -= F.fz * vf; } }
    if (v.s > t.roadEnd - 3) { const vf = v.vx * F.fx + v.vz * F.fz; if (vf > 0) { v.vx -= F.fx * vf; v.vz -= F.fz * vf; } }
  }

  get speedKmh() { return Math.abs(this.v.speed) * 3.6; }
}
