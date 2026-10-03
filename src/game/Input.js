import { clamp } from '../util/math.js';
import { Pads } from './Gamepad.js';

// Keyboard, gamepad (Pads, as input.pads: remappable) and on-screen touch
// controls (TouchControls, attached as input.touch: a thumb stick, ◂ ▸ pads
// or tilt). Keyboard and ◂ ▸
// steering are ramped so tapping gives small corrections and holding gives
// full lock, like an analogue stick.

const KEYMAP = {
  throttle: ['KeyW', 'ArrowUp'],
  brake: ['KeyS', 'ArrowDown'],
  left: ['KeyA', 'ArrowLeft'],
  right: ['KeyD', 'ArrowRight'],
  handbrake: ['Space'],
  nitro: ['ShiftLeft', 'ShiftRight', 'KeyN'],
  lookBack: ['KeyB'],
};
const PRESS = { camera: ['KeyC'], reset: ['KeyR'], pause: ['Escape', 'KeyP'], music: ['KeyM'] };

export class Input {
  constructor() {
    this.down = new Set();
    this.pressed = new Set();
    this.steer = 0;
    this.state = { throttle: 0, brake: 0, steer: 0, analog: false, handbrake: false, nitro: false, lookBack: false };
    this.enabled = true;
    this.touch = null;
    window.addEventListener('keydown', (e) => {
      if (e.repeat) { if (this.isGameKey(e.code)) e.preventDefault(); return; }
      this.down.add(e.code);
      for (const [name, codes] of Object.entries(PRESS)) if (codes.includes(e.code)) this.pressed.add(name);
      if (this.isGameKey(e.code)) e.preventDefault();
    });
    window.addEventListener('keyup', (e) => this.down.delete(e.code));
    window.addEventListener('blur', () => this.down.clear());
    this.pads = new Pads();
  }

  isGameKey(code) {
    return Object.values(KEYMAP).some((c) => c.includes(code)) || Object.values(PRESS).some((c) => c.includes(code));
  }

  any(codes) { return codes.some((c) => this.down.has(c)); }

  // One-shot actions since last call.
  consume(name) {
    const had = this.pressed.has(name);
    this.pressed.delete(name);
    return had;
  }

  update(dt) {
    const s = this.state;
    const t = this.touch;
    const g = this.pads.poll();
    for (const name of g.edges) this.pressed.add(name);
    let throttle = this.any(KEYMAP.throttle) ? 1 : t ? t.throttle : 0;
    let brake = this.any(KEYMAP.brake) ? 1 : t ? t.brake : 0;
    // A pad's steering bound to buttons (the D-pad, say) ramps like keys.
    const l = this.any(KEYMAP.left) || g.digital.left, r = this.any(KEYMAP.right) || g.digital.right;
    // The thumb stick and tilt are analogue already, so they aren't
    // ramped; a held key still wins over them.
    const analog = t?.analogSteer?.(dt) ?? null;
    // Analogue steering (the stick, tilt or a gamepad) asks for a share of
    // what the tyres can give; keys and pads ask for a wheel angle.
    let analogSteer = analog !== null && !(l || r);
    if (analogSteer) this.steer = analog;
    else {
      const target = l || r ? (r ? 1 : 0) - (l ? 1 : 0) : t ? t.steer : 0;
      // Ramp toward target; snap back faster than we turn in.
      const rate = target === 0 ? 7 : Math.sign(target) !== Math.sign(this.steer) && this.steer !== 0 ? 9 : 3.6;
      this.steer += clamp(target - this.steer, -rate * dt, rate * dt);
    }
    let steer = this.steer;
    let handbrake = this.any(KEYMAP.handbrake) || !!t?.held.handbrake;
    let nitro = this.any(KEYMAP.nitro) || !!t?.held.nitro;
    let lookBack = this.any(KEYMAP.lookBack);

    // Gamepad sticks and triggers (the standard layout unless remapped).
    const ax = g.steerAxis;
    if (Math.abs(ax) > 0.12) { steer = Math.sign(ax) * Math.pow((Math.abs(ax) - 0.12) / 0.88, 1.4); analogSteer = true; }
    const v = g.value;
    if (v.throttle > 0.05) throttle = Math.max(throttle, v.throttle);
    if (v.brake > 0.05) brake = Math.max(brake, v.brake);
    if (g.held.nitro) nitro = true;
    if (g.held.handbrake) handbrake = true;
    if (g.held.lookBack) lookBack = true;
    if (!this.enabled) { throttle = brake = steer = 0; handbrake = nitro = false; }
    s.throttle = throttle; s.brake = brake; s.steer = clamp(steer, -1, 1); s.analog = analogSteer;
    s.handbrake = handbrake; s.nitro = nitro; s.lookBack = lookBack;
    return s;
  }
}
