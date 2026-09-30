// On-screen controls for touch screens: steering bottom-left; gas, brake,
// nitro and handbrake bottom-right; reset, camera and pause at the top.
//
// The overlay takes every pointer, and each finger is hit-tested against
// every button on each move, so a thumb can slide from gas to brake or from
// left to right without lifting. Held buttons feed Input.update(); tap
// buttons post one-shot actions the same way the keyboard does.
//
// With tilt steering on (a TiltSteer as .tilt) and the sensor answering,
// the steering pads give way to a wheel that shows how much lock is on.

const HOLD = ['throttle', 'brake', 'left', 'right', 'handbrake', 'nitro'];
const SLOP = 14; // px of forgiveness round each button

export class TouchControls {
  constructor(input, root = document.getElementById('touch')) {
    this.input = input;
    this.root = root;
    this.visible = false;
    this.autoGas = false;
    this.tilt = null;
    this.tilting = false;
    this.wheel = root.querySelector('.t-wheel');
    this.held = Object.fromEntries(HOLD.map((k) => [k, false]));
    this.pointers = new Map(); // pointerId → [x, y]
    this.holdEls = [...root.querySelectorAll('[data-act]')];
    this.tapEls = [...root.querySelectorAll('[data-tap]')];
    const opt = { passive: false };
    root.addEventListener('pointerdown', (e) => this.onDown(e), opt);
    root.addEventListener('pointermove', (e) => this.onMove(e), opt);
    for (const t of ['pointerup', 'pointercancel']) root.addEventListener(t, (e) => this.onUp(e), opt);
    // No long-press menus, text selection or double-tap zoom on the pads.
    root.addEventListener('contextmenu', (e) => e.preventDefault());
    root.addEventListener('touchstart', (e) => { if (e.cancelable) e.preventDefault(); }, opt);
    window.addEventListener('blur', () => this.release());
  }

  show(on) {
    if (on === this.visible) return;
    this.visible = on;
    this.root.classList.toggle('hidden', !on);
    if (!on) this.release();
  }

  release() {
    this.pointers.clear();
    this.refresh();
  }

  onDown(e) {
    e.preventDefault();
    try { this.root.setPointerCapture(e.pointerId); } catch { /* not all browsers */ }
    this.pointers.set(e.pointerId, [e.clientX, e.clientY]);
    const tap = this.pick(this.tapEls, e.clientX, e.clientY);
    if (tap) {
      this.input.pressed.add(tap.dataset.tap);
      tap.classList.add('on');
      setTimeout(() => tap.classList.remove('on'), 140);
      navigator.vibrate?.(8);
    }
    this.refresh();
  }

  onMove(e) {
    if (!this.pointers.has(e.pointerId)) return;
    e.preventDefault();
    this.pointers.set(e.pointerId, [e.clientX, e.clientY]);
    this.refresh();
  }

  onUp(e) {
    if (!this.pointers.delete(e.pointerId)) return;
    e.preventDefault();
    this.refresh();
  }

  // The button under (x, y): the nearest centre among those whose rect
  // (grown by SLOP) contains the point, so neighbours don't both light.
  pick(els, x, y) {
    let best = null, bd = Infinity;
    for (const el of els) {
      const r = el.getBoundingClientRect();
      if (!r.width) continue; // hidden (the steering pads while tilting)
      if (x < r.left - SLOP || x > r.right + SLOP || y < r.top - SLOP || y > r.bottom + SLOP) continue;
      const d = Math.hypot(x - (r.left + r.right) / 2, y - (r.top + r.bottom) / 2);
      if (d < bd) { bd = d; best = el; }
    }
    return best;
  }

  refresh() {
    for (const k of HOLD) this.held[k] = false;
    for (const [x, y] of this.pointers.values()) {
      const el = this.pick(this.holdEls, x, y);
      if (el) this.held[el.dataset.act] = true;
    }
    for (const el of this.holdEls) el.classList.toggle('on', this.held[el.dataset.act]);
  }

  // Called by Input.update(): the tilt steering for this frame, or null
  // while steering is on the pads.
  tiltSteer(dt) {
    const on = !!this.tilt?.live && this.visible;
    if (on !== this.tilting) {
      this.tilting = on;
      this.root.classList.toggle('tilt', on);
      this.refresh(); // a thumb resting on a pad that just hid lets go
    }
    if (!on) return null;
    const s = this.tilt.update(dt);
    if (this.wheel) this.wheel.style.transform = `rotate(${(s * 90).toFixed(1)}deg)`;
    return s;
  }

  // Merged into Input.update(): gas (or auto gas unless braking), brake,
  // handbrake, nitro and a steering target of −1, 0 or +1. Nitro sits
  // above the gas pedal and needs throttle, so it holds the gas down too.
  get throttle() { return this.held.throttle || this.held.nitro || (this.autoGas && this.visible && !this.held.brake) ? 1 : 0; }
  get brake() { return this.held.brake ? 1 : 0; }
  get steer() { return (this.held.right ? 1 : 0) - (this.held.left ? 1 : 0); }
}

// Touch-first device, or forced with ?touch=1 (and off with ?touch=0).
export function isTouchDevice(params) {
  if (params.get('touch') === '1') return true;
  if (params.get('touch') === '0') return false;
  return !!window.matchMedia?.('(pointer: coarse)').matches && (navigator.maxTouchPoints || 0) > 0;
}
