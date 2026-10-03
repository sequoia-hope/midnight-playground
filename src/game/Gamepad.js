import { clamp } from '../util/math.js';

// Gamepads, read once a frame (Pads.poll): what each driving action is
// bound to, the buttons that move around the menus, picking up a new
// button or stick for an action (the Controller screen), and rumble.
//
// A binding is a button ({ button: 7 }) or one direction of an axis
// ({ axis: 0, dir: 1, rest: 0 }; rest is where the axis sits untouched, -1
// for a trigger reported as an axis). Each reads 0..1. Every controller
// starts on the standard layout; a remapped one keeps its own map, keyed
// by its id, so two kinds of pad don't fight over one.

export const ACTIONS = [
  ['left', 'Steer left'], ['right', 'Steer right'],
  ['throttle', 'Throttle'], ['brake', 'Brake / reverse'],
  ['nitro', 'Nitro'], ['handbrake', 'Handbrake'],
  ['lookBack', 'Look back'], ['camera', 'Camera'],
  ['reset', 'Reset car'], ['pause', 'Pause'],
];
export const DEFAULT_MAP = {
  left: [{ axis: 0, dir: -1, rest: 0 }], right: [{ axis: 0, dir: 1, rest: 0 }],
  throttle: [{ button: 7 }], brake: [{ button: 6 }],
  nitro: [{ button: 0 }], handbrake: [{ button: 2 }, { button: 5 }],
  lookBack: [{ button: 1 }], camera: [{ button: 3 }],
  reset: [{ button: 8 }], pause: [{ button: 9 }],
};
// One-shot actions: they fire once per press (Input.consume).
const PRESS = ['camera', 'reset', 'pause'];
// The menus use the standard layout whatever the map says, so a bad map
// can always be put right from the pad.
const NAV = { up: [12], down: [13], left: [14], right: [15], confirm: [0], back: [1], start: [9] };
const NAV_KEYS = Object.keys(NAV);

const STD_BUTTONS = ['A', 'B', 'X', 'Y', 'LB', 'RB', 'LT', 'RT', 'Back', 'Start', 'Left stick press', 'Right stick press', 'D-pad ↑', 'D-pad ↓', 'D-pad ←', 'D-pad →', 'Home'];
const STD_AXES = [['Left stick ←', 'Left stick →'], ['Left stick ↑', 'Left stick ↓'], ['Right stick ←', 'Right stick →'], ['Right stick ↑', 'Right stick ↓']];

export function bindingLabel(b, standard = true) {
  if (!b) return '—';
  if (b.button != null) return (standard && STD_BUTTONS[b.button]) || `Button ${b.button}`;
  return (standard && STD_AXES[b.axis]?.[b.dir > 0 ? 1 : 0]) || `Axis ${b.axis} ${b.dir > 0 ? '+' : '−'}`;
}

export function bindingValue(p, b) {
  if (b.button != null) {
    const x = p.buttons[b.button];
    return x ? Math.max(x.value || 0, x.pressed ? 1 : 0) : 0;
  }
  const rest = b.rest ?? 0;
  return clamp(((p.axes[b.axis] ?? rest) - rest) / (b.dir - rest), 0, 1);
}
const sameBinding = (a, b) => (a.button != null ? a.button === b.button : a.axis === b.axis && a.dir === b.dir);
// Sticks don't centre exactly; pedals and buttons bound to one need a dead zone.
const STICK_DEAD = 0.12;
const dead = (v, b) => (b.axis != null && !b.rest ? Math.max(0, (v - STICK_DEAD) / (1 - STICK_DEAD)) : v);

export function connectedPads() {
  const list = typeof navigator !== 'undefined' && navigator.getGamepads ? navigator.getGamepads() : [];
  return [...list].filter((p) => p && p.connected);
}

export class Pads {
  constructor({ maps = {}, onSave = null } = {}) {
    this.maps = maps;
    this.onSave = onSave;
    this.rumbleOn = true;
    this.active = null; // the pad last touched: the Controller screen edits its map
    this.activeIndex = null;
    this.capture = null;
    this.prev = {};
    this.muted = new Set();
    this.last = new Map(); // pad index → its buttons and axes last frame
    this.kicks = [];
    this.cont = null;
    this.sent = { strong: 0, weak: 0, t: -1e9 };
    this.state = { connected: false, value: {}, held: {}, digital: { left: false, right: false }, steerAxis: 0, edges: [], nav: {} };
  }

  mapFor(p) { return this.maps[p.id] ?? DEFAULT_MAP; }
  // What an action is on the pad in hand, for prompts ("PRESS BACK").
  label(action) {
    const p = this.active;
    return bindingLabel((p ? this.mapFor(p) : DEFAULT_MAP)[action]?.[0], !p || p.mapping === 'standard');
  }
  isRemapped(p) { return !!p && !!this.maps[p.id]; }

  poll(now = performance.now()) {
    this.now = now;
    const pads = connectedPads();
    const s = this.state;
    const value = {}, held = {}, nav = {};
    let steerAxis = 0, dl = false, dr = false;
    for (const [a] of ACTIONS) { value[a] = 0; held[a] = false; }
    for (const k of NAV_KEYS) nav[k] = false;
    for (const p of pads) this.noticeActivity(p);
    if (!pads.some((p) => p.index === this.activeIndex)) this.activeIndex = pads[0]?.index ?? null;
    this.active = pads.find((p) => p.index === this.activeIndex) ?? null;
    for (const p of pads) {
      const map = this.mapFor(p);
      for (const [a] of ACTIONS) {
        for (const b of map[a] ?? []) {
          const v = bindingValue(p, b);
          value[a] = Math.max(value[a], dead(v, b));
          if (v > 0.5) held[a] = true;
          if (a === 'left' || a === 'right') {
            if (b.axis != null) steerAxis += (a === 'right' ? v : -v);
            else if (v > 0.5) { if (a === 'left') dl = true; else dr = true; }
          }
        }
      }
      for (const k of NAV_KEYS) if (NAV[k].some((i) => p.buttons[i]?.pressed)) nav[k] = true;
      const x = p.axes[0] ?? 0, y = p.axes[1] ?? 0;
      if (x < -0.5) nav.left = true; else if (x > 0.5) nav.right = true;
      if (y < -0.5) nav.up = true; else if (y > 0.5) nav.down = true;
    }
    // Held through a capture, or through the press that left a menu: quiet
    // until let go, so the A that picked Resume doesn't fire the nitro.
    const capturing = !!this.capture;
    // This frame was read with the old map, so the new binding is kept
    // quiet by hand until it's seen let go.
    const bound = capturing ? this.listen(pads) : null;
    if (capturing) { for (const k of Object.keys(held)) this.muted.add(k); for (const k of NAV_KEYS) this.muted.add('nav.' + k); }
    for (const k of [...this.muted]) {
      const nk = k.startsWith('nav.') ? k.slice(4) : null;
      if (k !== bound && (nk ? !nav[nk] : !held[k] && value[k] < 0.05)) { this.muted.delete(k); continue; }
      if (nk) nav[nk] = false;
      else {
        value[k] = 0; held[k] = false;
        if (k === 'left') dl = false;
        if (k === 'right') dr = false;
      }
    }
    if (this.muted.has('left') || this.muted.has('right')) steerAxis = 0;
    const edges = [];
    for (const a of PRESS) { if (held[a] && !this.prev[a]) edges.push(a); this.prev[a] = held[a]; }
    Object.assign(s, { connected: pads.length > 0, value, held, nav, steerAxis: clamp(steerAxis, -1, 1), digital: { left: dl, right: dr }, edges });
    this.flushRumble(now, pads);
    return s;
  }

  // Quiet whatever is held now until it's let go.
  hush() {
    const s = this.state;
    for (const [a] of ACTIONS) if (s.held[a] || s.value[a] > 0.05) this.muted.add(a);
    for (const k of NAV_KEYS) if (s.nav[k]) this.muted.add('nav.' + k);
  }

  // Browsers hand out a fresh snapshot each frame, so the pad is kept by index.
  noticeActivity(p) {
    const was = this.last.get(p.index);
    const now = { buttons: p.buttons.map((b) => b.pressed), axes: [...p.axes] };
    this.last.set(p.index, now);
    if (was && (now.buttons.some((b, i) => b && !was.buttons[i]) || now.axes.some((v, i) => Math.abs(v - (was.axes[i] ?? v)) > 0.3))) this.activeIndex = p.index;
  }

  // ── Remapping ─────────────────────────────────────────────────────
  // Waits for every button to be let go (the A that chose the action is
  // still down), then takes the first button pressed or axis pushed past
  // half travel. done(binding) or done(null) after 8 s.
  startCapture(action, done) {
    this.capture = { action, done, t0: this.now ?? performance.now(), armed: false, base: null };
  }
  cancelCapture() {
    const c = this.capture;
    this.capture = null;
    c?.done(null);
  }
  listen(pads) {
    const c = this.capture;
    if (this.now - c.t0 > 8000) { this.cancelCapture(); return null; }
    if (!c.armed) {
      const still = pads.every((p) => p.buttons.every((b) => !b.pressed && (b.value || 0) < 0.3)
        && p.axes.every((v) => Math.abs(v) < 0.3 || Math.abs(v) > 0.95));
      if (!still) return null;
      c.armed = true;
      c.base = new Map(pads.map((p) => [p.index, [...p.axes]]));
      return null;
    }
    for (const p of pads) {
      let b = null;
      const i = p.buttons.findIndex((x) => x.pressed || (x.value || 0) > 0.6);
      if (i >= 0) b = { button: i };
      else {
        const base = c.base.get(p.index) ?? p.axes.map(() => 0);
        const j = p.axes.findIndex((v, k) => Math.abs(v - (base[k] ?? 0)) > 0.6);
        if (j >= 0) {
          // A standard pad's axes are sticks, centred at 0; otherwise a
          // resting -1 or +1 is a trigger reported as an axis.
          const b0 = base[j] ?? 0;
          const rest = p.mapping !== 'standard' && Math.abs(b0) > 0.8 ? Math.sign(b0) : 0;
          b = { axis: j, dir: p.axes[j] > rest ? 1 : -1, rest };
        }
      }
      if (!b) continue;
      this.bind(p, c.action, b);
      this.muted.add(c.action); // what it does now waits for it to be let go
      this.activeIndex = p.index;
      this.capture = null;
      c.done(b);
      return c.action;
    }
    return null;
  }
  // One button does one thing: taking it for this action frees it elsewhere.
  bind(p, action, b) {
    const map = structuredClone(this.mapFor(p));
    for (const a of Object.keys(map)) map[a] = map[a].filter((x) => !sameBinding(x, b));
    map[action] = [b];
    this.maps[p.id] = map;
    this.onSave?.(this.maps);
  }
  resetMap(p) {
    if (!p) return;
    delete this.maps[p.id];
    this.onSave?.(this.maps);
  }

  // ── Rumble ────────────────────────────────────────────────────────
  // kick: a jolt that fades out over ms. feel: the steady buzz this frame
  // (gravel, a wall scrape); it stops when the race stops calling it.
  kick(strong, weak, ms) {
    if (!this.rumbleOn) return;
    this.kicks.push({ strong, weak, ms, t0: this.now ?? performance.now() });
  }
  feel(strong, weak) {
    this.cont = this.rumbleOn ? { strong, weak, t: this.now ?? performance.now() } : null;
  }
  rumbleLevel(now) {
    this.kicks = this.kicks.filter((k) => now - k.t0 < k.ms);
    let strong = 0, weak = 0;
    for (const k of this.kicks) {
      const f = 1 - (now - k.t0) / k.ms;
      strong = Math.max(strong, k.strong * f); weak = Math.max(weak, k.weak * f);
    }
    if (this.cont && now - this.cont.t < 150) { strong = Math.max(strong, this.cont.strong); weak = Math.max(weak, this.cont.weak); }
    return { strong: clamp(strong, 0, 1), weak: clamp(weak, 0, 1) };
  }
  flushRumble(now, pads) {
    const { strong, weak } = this.rumbleOn ? this.rumbleLevel(now) : { strong: 0, weak: 0 };
    const sent = this.sent;
    const p = this.active;
    const act = p?.vibrationActuator;
    if (strong < 0.02 && weak < 0.02) {
      if (sent.strong || sent.weak) { act?.reset?.()?.catch?.(() => {}); Object.assign(sent, { strong: 0, weak: 0, t: -1e9 }); }
      return;
    }
    // Each effect runs a little longer than the gap to the next, so a steady
    // buzz doesn't stutter; a new one replaces the last.
    if (now - sent.t < 80 && Math.abs(strong - sent.strong) < 0.08 && Math.abs(weak - sent.weak) < 0.08) return;
    Object.assign(sent, { strong, weak, t: now });
    if (act?.playEffect) act.playEffect('dual-rumble', { duration: 140, strongMagnitude: strong, weakMagnitude: weak }).catch?.(() => {});
    else p?.hapticActuators?.[0]?.pulse?.(Math.max(strong, weak), 140);
  }
}
