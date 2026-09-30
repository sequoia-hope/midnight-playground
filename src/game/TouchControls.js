import { clamp } from '../util/math.js';

// On-screen controls for touch screens: steering on the left, the pedals
// bottom-right, reset, camera and pause at the top. Held controls feed
// Input.update(); tap buttons post one-shot actions the same way the
// keyboard does.
//
// Steering (mode) is one of:
// - 'stick': an analogue thumb stick. A thumb put down anywhere on the
//   left of the screen sets the centre there (never so near the edge that
//   full left lock is out of reach), and sliding it left or right steers in
//   proportion. Slide past full lock and the centre follows, so coming
//   back steers the other way at once.
// - 'buttons': ◂ ▸ pads, which Input ramps like keys.
// - 'tilt': a TiltSteer (as .tilt), with a wheel showing the lock; the
//   stick stands in until the sensor answers.
//
// Pedals (pedals) are one of:
// - 'slider': one vertical slider for the right thumb. From the bottom:
//   BRAKE (full at the bottom, lighter going up), a gap to coast in, GAS
//   (light just above the gap, flat out from about two thirds up) and N₂O
//   at the top. DRIFT is a strip beside it in the same touch space: slide
//   the thumb right onto it for the handbrake, still on the gas. A thumb
//   that starts on the slider keeps working it until it lifts.
// - 'buttons': GAS, BRAKE, DRIFT and N₂O pads.
//
// Fingers on the pads are hit-tested against every pad on each move, so a
// thumb can slide from gas to brake (or ◂ to ▸) without lifting.

const HOLD = ['throttle', 'brake', 'left', 'right', 'handbrake', 'nitro'];
const SLOP = 14; // px of forgiveness round each button
const STICK_ZONE = 0.45; // of the screen's width, from the left
const STICK_DEAD = 0.06; // of full travel, round the centre
const STICK_END = 0.04; // and at the ends, so full lock is easy to hold

// The slider's bands, as fractions of its height from the bottom.
export const SLIDER = { brakeFull: 0.06, brakeTop: 0.3, gasBottom: 0.36, gasFull: 0.62, nitro: 0.82 };
const LIGHT = 0.15; // the lightest brake or gas, at the gap

// Stick travel as a fraction of full lock (−1…1) to steering: a small dead
// zone, then a gentle curve for fine corrections near the centre, and the
// last few per cent are full lock.
export function stickSteer(d) {
  const a = Math.abs(d);
  if (a <= STICK_DEAD) return 0;
  return Math.sign(d) * Math.pow(Math.min(1, (a - STICK_DEAD) / (1 - STICK_DEAD - STICK_END)), 1.25);
}

// Full lock on the stick, in CSS px: about a sixth of the screen's short side.
export const stickRange = (w, h) => clamp(Math.min(w, h) * 0.15, 44, 84);

// What a thumb at height u on the slider (0 bottom, 1 top) asks for.
export function sliderAt(u) {
  const S = SLIDER, lerp = (a, b, t) => a + (b - a) * clamp(t, 0, 1);
  if (u >= S.nitro) return { throttle: 1, brake: 0, nitro: true };
  if (u >= S.gasBottom) return { throttle: lerp(LIGHT, 1, (u - S.gasBottom) / (S.gasFull - S.gasBottom)), brake: 0, nitro: false };
  if (u > S.brakeTop) return { throttle: 0, brake: 0, nitro: false };
  return { throttle: 0, brake: lerp(1, LIGHT, (u - S.brakeFull) / (S.brakeTop - S.brakeFull)), nitro: false };
}

const pct = (f) => (f * 100).toFixed(2) + '%';

export class TouchControls {
  constructor(input, root = document.getElementById('touch')) {
    this.input = input;
    this.root = root;
    this.visible = false;
    this.autoGas = false;
    this.mode = 'stick';
    this.pedals = 'slider';
    this.tilt = null;
    this.steering = ''; // what steers right now: 'stick', 'buttons' or 'tilt'
    this.stick = null; // the thumb on the stick: { id, x0, y0, x }
    this.stickR = 60;
    this.held = Object.fromEntries(HOLD.map((k) => [k, false]));
    this.amount = { throttle: 0, brake: 0 };
    this.pointers = new Map(); // pointerId → [x, y] for fingers on the pads
    this.sliding = new Map(); // pointerId → [x, y] for thumbs on the slider
    this.slide = null; // what the slider reads, while a thumb is on it
    this.holdEls = [...root.querySelectorAll('[data-act]')];
    this.tapEls = [...root.querySelectorAll('[data-tap]')];
    this.wheel = root.querySelector('.t-wheel');
    this.stickEl = root.querySelector('.t-stick');
    this.knob = root.querySelector('.t-stick-knob');
    this.panel = root.querySelector('.t-pedal');
    this.track = root.querySelector('.t-slider');
    this.placeBands();
    const opt = { passive: false };
    root.addEventListener('pointerdown', (e) => this.onDown(e), opt);
    root.addEventListener('pointermove', (e) => this.onMove(e), opt);
    for (const t of ['pointerup', 'pointercancel']) root.addEventListener(t, (e) => this.onUp(e), opt);
    // No long-press menus, text selection or double-tap zoom on the pads.
    root.addEventListener('contextmenu', (e) => e.preventDefault());
    root.addEventListener('touchstart', (e) => { if (e.cancelable) e.preventDefault(); }, opt);
    window.addEventListener('blur', () => this.release());
    window.addEventListener('resize', () => this.measure());
    this.measure();
    this.layout();
  }

  setMode(mode) {
    this.mode = mode;
    this.layout();
  }

  setPedals(kind) {
    this.pedals = kind;
    this.sliding.clear();
    this.pointers.clear();
    this.layout(true);
  }

  show(on) {
    if (on === this.visible) return;
    this.visible = on;
    this.root.classList.toggle('hidden', !on);
    if (!on) this.release();
  }

  release() {
    this.pointers.clear();
    this.sliding.clear();
    this.stick = null;
    this.drawStick();
    this.refresh();
  }

  measure() {
    this.stickR = stickRange(window.innerWidth, window.innerHeight);
    this.root.style.setProperty('--stick-r', this.stickR + 'px');
  }

  // The slider's bands and labels, from SLIDER, so the picture and the
  // reading can't disagree.
  placeBands() {
    if (!this.track) return;
    const S = SLIDER;
    const place = (sel, from, to) => {
      const el = this.track.querySelector(sel);
      if (el) Object.assign(el.style, { bottom: pct(from), height: pct(to - from) });
    };
    place('.t-band-brake', 0, S.brakeTop);
    place('.t-band-gas', S.gasBottom, S.nitro);
    place('.t-band-nitro', S.nitro, 1);
    const mark = this.track.querySelector('.t-mark');
    if (mark) mark.style.bottom = pct(S.gasFull);
    this.track.querySelector('.t-fill-gas').style.bottom = pct(S.gasBottom);
    this.track.querySelector('.t-fill-brake').style.top = pct(1 - S.brakeTop);
  }

  // Show the controls for whatever steers now (tilt goes live when the
  // sensor first answers) and for the pedals chosen.
  layout(force = false) {
    const s = this.mode === 'tilt' && this.tilt?.live ? 'tilt' : this.mode === 'buttons' ? 'buttons' : 'stick';
    if (s === this.steering && !force) return;
    this.steering = s;
    for (const k of ['stick', 'buttons', 'tilt']) this.root.classList.toggle(k, k === s);
    this.root.classList.toggle('slider', this.pedals === 'slider');
    if (s !== 'stick') { this.stick = null; this.drawStick(); }
    this.refresh(); // a thumb resting on a pad that just hid lets go
  }

  onDown(e) {
    e.preventDefault();
    try { this.root.setPointerCapture(e.pointerId); } catch { /* not all browsers */ }
    const x = e.clientX, y = e.clientY;
    const tap = this.pick(this.tapEls, x, y);
    if (tap) {
      this.input.pressed.add(tap.dataset.tap);
      tap.classList.add('on');
      setTimeout(() => tap.classList.remove('on'), 140);
      navigator.vibrate?.(8);
    } else if (this.steering === 'stick' && !this.stick && x < window.innerWidth * STICK_ZONE) {
      this.stick = { id: e.pointerId, x0: Math.max(x, this.stickR + 4), y0: y, x };
      this.drawStick();
      return;
    } else if (this.pedals === 'slider' && this.onPanel(x, y)) {
      this.sliding.set(e.pointerId, [x, y]);
      this.refresh();
      return;
    }
    this.pointers.set(e.pointerId, [x, y]);
    this.refresh();
  }

  onMove(e) {
    if (this.stick?.id === e.pointerId) {
      e.preventDefault();
      const s = this.stick, R = this.stickR;
      s.x = e.clientX;
      s.x0 = clamp(s.x0, s.x - R, s.x + R); // past full lock, the centre follows
      this.drawStick();
      return;
    }
    const map = this.sliding.has(e.pointerId) ? this.sliding : this.pointers.has(e.pointerId) ? this.pointers : null;
    if (!map) return;
    e.preventDefault();
    map.set(e.pointerId, [e.clientX, e.clientY]);
    this.refresh();
  }

  onUp(e) {
    if (this.stick?.id === e.pointerId) {
      e.preventDefault();
      this.stick = null;
      this.drawStick();
      return;
    }
    if (!this.sliding.delete(e.pointerId) && !this.pointers.delete(e.pointerId)) return;
    e.preventDefault();
    this.refresh();
  }

  onPanel(x, y) {
    const r = this.panel?.getBoundingClientRect();
    return !!r?.width && x >= r.left - SLOP && x <= r.right + SLOP && y >= r.top - SLOP && y <= r.bottom + SLOP;
  }

  // The stick sits under the thumb while it's down, and back in its corner
  // when it isn't.
  drawStick() {
    const el = this.stickEl, s = this.stick;
    if (!el) return;
    el.classList.toggle('active', !!s);
    el.style.left = s ? s.x0 + 'px' : '';
    el.style.top = s ? s.y0 + 'px' : '';
    const d = s ? clamp(s.x - s.x0, -this.stickR, this.stickR) : 0;
    this.knob.style.transform = d ? `translateX(${d.toFixed(1)}px)` : '';
    el.classList.toggle('lock', Math.abs(d) >= this.stickR - 0.5);
  }

  // The button under (x, y): the nearest centre among those whose rect
  // (grown by SLOP) contains the point, so neighbours don't both light.
  pick(els, x, y) {
    let best = null, bd = Infinity;
    for (const el of els) {
      const r = el.getBoundingClientRect();
      if (!r.width) continue; // hidden (pads for the steering or pedals not in use)
      if (x < r.left - SLOP || x > r.right + SLOP || y < r.top - SLOP || y > r.bottom + SLOP) continue;
      const d = Math.hypot(x - (r.left + r.right) / 2, y - (r.top + r.bottom) / 2);
      if (d < bd) { bd = d; best = el; }
    }
    return best;
  }

  refresh() {
    const h = this.held;
    for (const k of HOLD) h[k] = false;
    for (const [x, y] of this.pointers.values()) {
      const el = this.pick(this.holdEls, x, y);
      if (el) h[el.dataset.act] = true;
    }
    for (const el of this.holdEls) el.classList.toggle('on', h[el.dataset.act]);
    this.amount.throttle = h.throttle ? 1 : 0;
    this.amount.brake = h.brake ? 1 : 0;
    this.slide = this.sliding.size ? this.readSlider() : null;
    if (this.slide) {
      const s = this.slide;
      h.throttle ||= s.throttle > 0; h.brake ||= s.brake > 0; h.nitro ||= s.nitro; h.handbrake ||= s.drift;
      this.amount.throttle = Math.max(this.amount.throttle, s.throttle);
      this.amount.brake = Math.max(this.amount.brake, s.brake);
    }
    this.drawSlider();
  }

  // Every thumb on the slider: its height sets the pedals (the hardest
  // wins if there are two), and past the slider's right edge is DRIFT.
  readSlider() {
    const r = this.track.getBoundingClientRect();
    const out = { throttle: 0, brake: 0, nitro: false, drift: false, u: null };
    for (const [x, y] of this.sliding.values()) {
      const u = clamp((r.bottom - y) / r.height, 0, 1);
      const p = sliderAt(u);
      out.throttle = Math.max(out.throttle, p.throttle);
      out.brake = Math.max(out.brake, p.brake);
      out.nitro ||= p.nitro;
      out.drift ||= x > r.right + 3;
      out.u ??= u;
    }
    return out;
  }

  // The knob at the thumb, and a fill from the gap up (gas) or down (brake)
  // to it, so how hard you're pressing is plain to see.
  drawSlider() {
    const p = this.panel;
    if (!p) return;
    const s = this.slide, S = SLIDER;
    p.classList.toggle('active', !!s);
    for (const k of ['gas', 'brake', 'nitro', 'drift']) {
      p.classList.toggle(k, !!s && (k === 'gas' ? s.throttle > 0 : k === 'brake' ? s.brake > 0 : s[k]));
    }
    p.style.setProperty('--u', s ? pct(s.u) : '0%');
    p.style.setProperty('--gas', s && s.u > S.gasBottom ? pct(s.u - S.gasBottom) : '0%');
    p.style.setProperty('--brk', s && s.u < S.brakeTop ? pct(S.brakeTop - s.u) : '0%');
  }

  // Called by Input.update(): this frame's analogue steering (the stick,
  // or tilt), or null when steering is on the ◂ ▸ pads.
  analogSteer(dt) {
    this.layout();
    if (!this.visible || this.steering === 'buttons') return null;
    if (this.steering === 'stick') return this.stick ? stickSteer((this.stick.x - this.stick.x0) / this.stickR) : 0;
    const s = this.tilt.update(dt);
    if (this.wheel) this.wheel.style.transform = `rotate(${(s * 90).toFixed(1)}deg)`;
    return s;
  }

  // Merged into Input.update(): gas (or auto gas unless braking), brake,
  // handbrake, nitro and, for the ◂ ▸ pads, a steering target of −1, 0 or
  // +1. Nitro needs throttle, so it holds the gas flat too. A thumb on the
  // slider sets the gas itself, so auto gas waits while it's there.
  get throttle() {
    if (this.held.nitro) return 1;
    if (this.held.throttle) return this.amount.throttle;
    return this.autoGas && this.visible && !this.held.brake && !this.slide ? 1 : 0;
  }
  get brake() { return this.held.brake ? this.amount.brake : 0; }
  get steer() { return (this.held.right ? 1 : 0) - (this.held.left ? 1 : 0); }
}

// Touch-first device, or forced with ?touch=1 (and off with ?touch=0).
export function isTouchDevice(params) {
  if (params.get('touch') === '1') return true;
  if (params.get('touch') === '0') return false;
  return !!window.matchMedia?.('(pointer: coarse)').matches && (navigator.maxTouchPoints || 0) > 0;
}
