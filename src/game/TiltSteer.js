import { clamp } from '../util/math.js';

// Tilt steering for phones: hold the phone like a steering wheel and turn
// it. The steering follows the screen's roll, how far its left-right axis
// leans off level, taken from the gravity direction in deviceorientation's
// beta and gamma. Level is straight ahead, so there is nothing to
// calibrate, and it reads the same with the phone upright or lying back in
// your hands, in either landscape and in portrait.
//
// Browsers only send the sensor to secure pages (https), and iPhones ask
// the player first: requestPermission() has to run inside a tap, so
// enable() is called from the menu toggle and again from the Race tap.
// Chrome sends one event of nulls when there is no sensor, and after that
// only sends when the reading changes, so a phone held still goes quiet.

const DEG = Math.PI / 180;
const DEAD = 2 * DEG; // a steady hand's wobble round level
const SMOOTH = 0.05; // s: settles sensor jitter, still quick on the wheel

// The screen's roll in radians, positive when its right-hand edge dips,
// from beta and gamma (degrees) and the angle the picture is turned from
// the phone's natural orientation (window.orientation: 90 with the phone
// turned anticlockwise, -90 or 270 clockwise).
export function screenRoll(beta, gamma, angle = 0) {
  const b = beta * DEG, g = gamma * DEG, a = angle * DEG;
  // "Up" in the phone's own axes (x to the right, y to the top, z out of
  // the glass), and the screen's right-hand edge in the same axes.
  const ux = -Math.cos(b) * Math.sin(g), uy = Math.sin(b);
  const rx = Math.cos(a), ry = -Math.sin(a);
  return Math.asin(clamp(-(ux * rx + uy * ry), -1, 1));
}

// Roll to steering, −1…+1: a small dead zone round level, then a gentle
// curve (fine corrections near centre) up to full lock at `full` radians.
export function rollToSteer(roll, full) {
  const a = Math.abs(roll);
  if (a <= DEAD) return 0;
  return Math.sign(roll) * Math.pow(Math.min(1, (a - DEAD) / (full - DEAD)), 1.3);
}

// Sensitivity 0…1 to the roll that gives full lock: 40° down to 12°.
export const fullLockFor = (k) => (40 - 28 * clamp(k, 0, 1)) * DEG;

export class TiltSteer {
  constructor(win = window) {
    this.win = win;
    this.on = false; // the player's choice
    // off · waiting (listening, nothing yet) · live · none (no sensor) ·
    // insecure (http page) · ask (iPhone: needs a tap) · denied
    this.state = 'off';
    this.granted = false;
    this.roll = 0;
    this.steer = 0;
    this.fullLock = fullLockFor(0.5);
    this.onChange = null;
    this.handler = (e) => this.onOrientation(e);
  }

  get live() { return this.on && this.state === 'live'; }

  setSensitivity(k) { this.fullLock = fullLockFor(k); }

  set(state) {
    if (state === this.state) return;
    this.state = state;
    this.onChange?.(state);
  }

  // Turn tilt steering on or off. On an iPhone, call it inside a tap.
  enable(on = true) {
    this.on = on;
    const w = this.win, DOE = w.DeviceOrientationEvent;
    if (!on) {
      w.removeEventListener('deviceorientation', this.handler);
      clearTimeout(this.timer);
      this.steer = 0;
      return this.set('off');
    }
    if (this.state === 'live') return;
    if (!DOE) return this.set('none');
    if (typeof DOE.requestPermission === 'function' && !this.granted) {
      let asked;
      try { asked = DOE.requestPermission(); } catch { return this.set('ask'); }
      Promise.resolve(asked).then((r) => {
        if (r !== 'granted') return this.set('denied');
        this.granted = true;
        if (this.on) this.listen();
      }, () => { if (this.on) this.set('ask'); }); // not inside a tap
      return;
    }
    this.listen();
  }

  listen() {
    const w = this.win;
    w.removeEventListener('deviceorientation', this.handler);
    w.addEventListener('deviceorientation', this.handler);
    // Plain-http pages get no sensor in most browsers; say so rather than
    // wait. If one answers anyway, onOrientation goes live.
    this.set(w.isSecureContext === false ? 'insecure' : 'waiting');
    clearTimeout(this.timer);
    this.timer = setTimeout(() => { if (this.on && this.state === 'waiting') this.set('none'); }, 2000);
  }

  onOrientation(e) {
    if (!this.on) return;
    if (e.beta == null || e.gamma == null) {
      if (this.state === 'waiting') this.set('none');
      return;
    }
    const w = this.win;
    const angle = typeof w.orientation === 'number' ? w.orientation : w.screen?.orientation?.angle ?? 0;
    this.roll = screenRoll(e.beta, e.gamma, angle);
    this.set('live');
  }

  // Smoothed steering for this frame.
  update(dt) {
    const target = rollToSteer(this.roll, this.fullLock);
    this.steer += (target - this.steer) * (1 - Math.exp(-dt / SMOOTH));
    if (Math.abs(target - this.steer) < 1e-3) this.steer = target; // level is dead straight
    return this.steer;
  }
}
