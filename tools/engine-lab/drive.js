// Engine Sound Lab: what the engine is doing, frame by frame. One driver
// feeds both the new model and the current game engine (A/B), so they hear
// the same rpm, throttle, gear, speed and boost.
//
// Modes:
//   manual: rpm and throttle follow the sliders (rpm at a believable rate).
//   rev:    in neutral: blips, a hold on the limiter, lift and overrun, stabs.
//   drive:  a pull through the gears (launch, shifts with an ignition cut),
//           a long lift (overrun), braking with blipped downshifts, repeat.

import { boostTarget } from './presets.js';

const clamp = (v, lo, hi) => (v < lo ? lo : v > hi ? hi : v);

// Neutral rev script: [seconds, pedal]. Loops.
const REV_SCRIPT = [
  [0, 0], [1.2, 0.7], [1.36, 0], [2.3, 1], [2.55, 0], [3.5, 1], [4.7, 0],
  [6.6, 0.6], [6.72, 0], [7.0, 0.7], [7.12, 0], [7.4, 1], [7.85, 0], [10, 0],
];
const GEAR_K = [500, 330, 240, 185, 150, 125]; // rpm per m/s at 7000 rpm redline

export class Driver {
  constructor() {
    this.mode = 'manual';
    this.p = { idle: 800, redline: 7000, revUp: 10000, revDown: 5000 };
    this.rpm = 800; this.pedal = 0; this.thr = 0; this.gear = 0; this.speed = 0; this.boost = 0;
    this.manualRpm = 800; this.manualThr = 0;
    this.t = 0; this.phase = 'idle'; this.cut = 0; this.blip = 0;
  }

  setPreset(p) {
    this.p = p;
    this.rpm = Math.max(this.rpm, p.idle);
    this.manualRpm = clamp(this.manualRpm, p.idle, p.redline);
  }

  start(mode) {
    this.mode = mode; this.t = 0; this.gear = 0; this.speed = 0; this.phase = 'idle'; this.cut = 0; this.blip = 0;
  }

  k(g) { return GEAR_K[g - 1] * (this.p.redline / 7000); }

  // Free-revving crank (neutral or clutch in).
  freeRev(dt, pedal) {
    const p = this.p;
    let fuel = pedal;
    if (this.rpm >= p.redline) fuel = 0; // the limiter cuts fuel; it bounces
    const idleHold = this.rpm < p.idle ? clamp((p.idle - this.rpm) / 200, 0, 1) * 0.3 : 0;
    const up = (fuel + idleHold) * p.revUp * (1 - Math.pow(this.rpm / (p.redline * 1.06), 3));
    const down = p.revDown * (0.25 + 0.75 * this.rpm / p.redline);
    this.rpm = Math.max(p.idle * 0.92, this.rpm + (up - down * (1 - Math.min(1, fuel + idleHold))) * dt);
  }

  // Returns the state for this frame; events: 'up' / 'down' shifts.
  step(dt) {
    const p = this.p, events = [];
    this.t += dt;
    if (this.mode === 'manual') {
      const d = this.manualRpm - this.rpm;
      this.rpm += clamp(d, -p.revDown * dt, p.revUp * dt);
      this.pedal = this.manualThr;
      this.thr = this.pedal; this.gear = 0; this.speed = 0;
    } else if (this.mode === 'rev') {
      const T = REV_SCRIPT[REV_SCRIPT.length - 1][0], tt = this.t % T;
      let pedal = 0;
      for (const [at, v] of REV_SCRIPT) if (tt >= at) pedal = v;
      this.pedal = pedal; this.thr = pedal; this.gear = 0; this.speed = 0;
      this.freeRev(dt, pedal);
    } else this.driveStep(dt, events);
    // Turbo spool: slow up (lag), quicker down, dumped when the pedal lifts.
    const bt = boostTarget(p, this.rpm, this.thr);
    const tau = bt > this.boost ? 0.7 : this.thr < 0.2 ? 0.15 : 0.35;
    this.boost += (bt - this.boost) * Math.min(1, dt / tau);
    return { rpm: this.rpm, throttle: this.thr, gear: this.gear, speed: this.speed, boost: this.boost, events };
  }

  driveStep(dt, events) {
    const p = this.p;
    if (this.cut > 0) this.cut -= dt;
    if (this.blip > 0) this.blip -= dt;
    let pedal = 0;
    switch (this.phase) {
      case 'idle':
        pedal = 0; this.gear = 0; this.speed = 0;
        this.freeRev(dt, 0);
        if (this.t > 1.5) { this.phase = 'launch'; this.t = 0; }
        break;
      case 'launch': // clutch slipping: rpm held up while the car gets going
        pedal = 1; this.gear = 1;
        this.speed += 7.5 * dt;
        this.rpm += (Math.max(0.5 * p.redline, this.speed * this.k(1)) - this.rpm) * Math.min(1, dt / 0.12);
        if (this.speed * this.k(1) >= 0.5 * p.redline) this.phase = 'pull';
        break;
      case 'pull': {
        pedal = this.cut > 0 ? 0 : 1;
        const a = pedal * 9.5 * Math.pow(this.k(this.gear) / this.k(1), 0.85) * (0.75 + 0.25 * Math.sin(Math.PI * this.rpm / p.redline)) - 0.4 - 0.0004 * this.speed * this.speed;
        this.speed = Math.max(0, this.speed + a * dt);
        this.rpm += (Math.max(p.idle, this.speed * this.k(this.gear)) - this.rpm) * Math.min(1, dt / (this.cut > 0 ? 0.07 : 0.03));
        if (this.rpm >= 0.965 * p.redline && this.cut <= 0) {
          if (this.gear < 4) { this.gear++; this.cut = 0.15; events.push('up'); }
          else { this.phase = 'lift'; this.t = 0; }
        }
        break;
      }
      case 'lift': // off throttle in gear: engine braking, overrun crackle
        pedal = 0;
        this.speed = Math.max(0, this.speed - (1.2 + 0.0005 * this.speed * this.speed + 1.5 * this.rpm / p.redline) * dt);
        this.rpm += (Math.max(p.idle, this.speed * this.k(this.gear)) - this.rpm) * Math.min(1, dt / 0.05);
        if (this.t > 3.5) { this.phase = 'brake'; this.t = 0; }
        break;
      case 'brake': // braking, blipped downshifts
        pedal = this.blip > 0 ? 0.55 : 0;
        this.speed = Math.max(0, this.speed - 7 * dt);
        if (this.gear > 1 && this.speed * this.k(this.gear) < 0.42 * p.redline) { this.gear--; this.blip = 0.13; events.push('down'); }
        this.rpm += (Math.max(p.idle, this.speed * this.k(this.gear) + (this.blip > 0 ? 0.08 * p.redline : 0)) - this.rpm) * Math.min(1, dt / 0.06);
        if (this.speed < 4) { this.phase = 'idle'; this.t = 0; this.gear = 0; }
        break;
    }
    this.pedal = pedal; this.thr = pedal;
  }
}
