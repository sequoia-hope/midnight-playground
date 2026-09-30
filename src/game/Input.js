import { clamp } from '../util/math.js';

// Keyboard, gamepad and on-screen touch controls (TouchControls, attached
// as input.touch: a thumb stick, ◂ ▸ pads or tilt). Keyboard and ◂ ▸
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
    this.state = { throttle: 0, brake: 0, steer: 0, handbrake: false, nitro: false, lookBack: false };
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
    this.padPrev = {};
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
    let throttle = this.any(KEYMAP.throttle) ? 1 : t ? t.throttle : 0;
    let brake = this.any(KEYMAP.brake) ? 1 : t ? t.brake : 0;
    const l = this.any(KEYMAP.left), r = this.any(KEYMAP.right);
    // The thumb stick and tilt are analogue already, so they aren't
    // ramped; a held key still wins over them.
    const analog = t?.analogSteer?.(dt) ?? null;
    if (analog !== null && !(l || r)) this.steer = analog;
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

    // Gamepad (standard mapping).
    const pads = navigator.getGamepads ? navigator.getGamepads() : [];
    for (const p of pads) {
      if (!p || !p.connected) continue;
      const ax = p.axes[0] || 0;
      if (Math.abs(ax) > 0.12) steer = Math.sign(ax) * Math.pow((Math.abs(ax) - 0.12) / 0.88, 1.4);
      const rt = p.buttons[7]?.value || 0, lt = p.buttons[6]?.value || 0;
      if (rt > 0.05) throttle = Math.max(throttle, rt);
      if (lt > 0.05) brake = Math.max(brake, lt);
      if (p.buttons[0]?.pressed) nitro = true;
      if (p.buttons[2]?.pressed || p.buttons[5]?.pressed) handbrake = true;
      if (p.buttons[1]?.pressed) lookBack = true;
      const edge = (i, name) => { if (p.buttons[i]?.pressed && !this.padPrev[i]) this.pressed.add(name); this.padPrev[i] = p.buttons[i]?.pressed; };
      edge(3, 'camera'); edge(9, 'pause'); edge(8, 'reset');
    }
    if (!this.enabled) { throttle = brake = steer = 0; handbrake = nitro = false; }
    s.throttle = throttle; s.brake = brake; s.steer = clamp(steer, -1, 1);
    s.handbrake = handbrake; s.nitro = nitro; s.lookBack = lookBack;
    return s;
  }
}
