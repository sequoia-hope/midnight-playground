// game/HUD.js fmtTime: the race clock, lap and results times, and the best
// times on the menu card. Then the HUD itself on a fake DOM: the Hot Pursuit
// furniture (heat stars, bust/evade bar, damage, the penalty hold card,
// radio chatter) and that none of it shows without st.pursuit.

import { test, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { fmtTime, HUD } from '../../src/game/HUD.js';

test('fmtTime shows m:ss.hh', () => {
  assert.equal(fmtTime(0), '0:00.00');
  assert.equal(fmtTime(5.2), '0:05.20');
  assert.equal(fmtTime(59.5), '0:59.50');
  assert.equal(fmtTime(60), '1:00.00');
  assert.equal(fmtTime(65.432), '1:05.43');
  assert.equal(fmtTime(185.123), '3:05.12');
  assert.equal(fmtTime(3600), '60:00.00');
});

test('fmtTime shows dashes for no time', () => {
  for (const t of [null, undefined, NaN, Infinity, -Infinity]) assert.equal(fmtTime(t), '--:--.--', String(t));
});

// The last few milliseconds of a minute round up to the next minute, not
// to ":60.00".
test('fmtTime rounds across the minute', () => {
  assert.equal(fmtTime(59.996), '1:00.00');
  assert.equal(fmtTime(119.999), '2:00.00');
  assert.equal(fmtTime(0.004), '0:00.00');
  assert.equal(fmtTime(9.996), '0:10.00');
});

// ── A fake DOM: just enough of Element for HUD. ──
class FakeEl {
  constructor(id = '') {
    this.id = id; this.style = {}; this.textContent = ''; this.children = []; this.parentElement = null; this.offsetWidth = 0;
    this.cls = new Set(); this.sel = {};
    this.classList = {
      add: (c) => this.cls.add(c), remove: (c) => this.cls.delete(c), contains: (c) => this.cls.has(c),
      toggle: (c, on = !this.cls.has(c)) => (on ? this.cls.add(c) : this.cls.delete(c), on),
    };
  }
  get className() { return [...this.cls].join(' '); }
  set className(v) { this.cls = new Set(String(v).split(/\s+/).filter(Boolean)); }
  set innerHTML(v) { this.children = []; }
  appendChild(c) { c.parentElement = this; this.children.push(c); return c; }
  insertBefore(c) { return this.appendChild(c); }
  querySelector(s) { return (this.sel[s] ||= new FakeEl()); }
  querySelectorAll() { return []; }
  getContext() {
    const noop = () => ({ addColorStop() {} });
    return new Proxy({}, { get: (o, k) => (k in o ? o[k] : noop), set: (o, k, v) => ((o[k] = v), true) });
  }
}

let els, hud;
const el = (id) => (els[id] ||= new FakeEl(id));
const hidden = (id) => el(id).classList.contains('hidden');

function makeTrack() {
  const n = 1000;
  return {
    n, loop: false, runout: 0, finishS: 900, roadEnd: 999,
    zones: [{ s0: 0, s1: n, name: 'SIERRA PASS', sub: 'x', color: '#888' }],
    zone: new Array(n).fill(0), px: new Float32Array(n), pz: new Float32Array(n),
    idx: (s) => Math.max(0, Math.min(n - 1, Math.floor(s))), frame: () => ({ x: 0, z: 0 }),
  };
}

const pursuit = (o = {}) => ({
  heat: 1, heatMeter: 0, state: 'patrol', bust: 0, evade: 0, damage: 0, hold: 0, holdReason: 'busted', holdTotal: 6,
  penalties: 0, units: [], roadblocks: [], spikes: [], flash: true, ...o,
});
const state = (pu) => ({
  position: 1, time: 12.3, speed: 30, gear: 3, nitro: 0.5, nitroActive: false, rpm: 4000, s: 10, started: true,
  racers: [{ s: 10 }], racersFull: [{ player: true, v: { x: 0, z: 0 }, color: 0xffffff }], traffic: [],
  player: { x: 0, z: 0, yaw: 0 }, pursuit: pu,
});
const starWidths = () => el('pz-stars').children.map((s) => parseFloat(s.children[0].style.width));

beforeEach(() => {
  els = {};
  // The speedo box: #hud-nitro's parent is .nitro, whose parent is .hud-br.
  el('hud-nitro').parentElement = el('.nitro');
  el('.nitro').parentElement = el('.hud-br');
  const docEls = {};
  globalThis.document = {
    getElementById: el,
    querySelector: (s) => (docEls[s] ||= new FakeEl()),
    createElement: () => new FakeEl(),
  };
  hud = new HUD(makeTrack(), { mode: 'race' });
});
afterEach(() => { delete globalThis.document; });

test('no pursuit furniture without st.pursuit', () => {
  hud.update(1 / 60, state(undefined));
  for (const id of ['hud-pz', 'hud-dmg', 'hud-pen', 'hud-hold']) assert.ok(hidden(id), id);
  assert.ok(!el('hud-radio').classList.contains('show'));
  assert.ok(!el('.hud-br').classList.contains('pz-on'));
  assert.equal(el('hud-time').textContent, '0:12.30');
});

test('st.pursuit shows the stars and damage, and dropping it hides them again', () => {
  hud.update(1 / 60, state(pursuit()));
  assert.ok(!hidden('hud-pz'));
  assert.ok(!hidden('hud-dmg'));
  assert.ok(el('.hud-br').classList.contains('pz-on'));
  hud.update(1 / 60, state(null));
  assert.ok(hidden('hud-pz'));
  assert.ok(hidden('hud-dmg'));
});

test('setPursuit shows and hides the furniture', () => {
  hud.setPursuit(true);
  assert.ok(!hidden('hud-pz'));
  hud.setPursuit(false);
  assert.ok(hidden('hud-pz'));
});

test('five stars: full up to the heat, the next one filling with the meter', () => {
  hud.update(1 / 60, state(pursuit({ heat: 2, heatMeter: 0.4 })));
  assert.deepEqual(starWidths(), [100, 100, 40, 0, 0]);
  assert.ok(!el('pz-stars').classList.contains('max'));
  hud.update(1 / 60, state(pursuit({ heat: 5, heatMeter: 0.7 })));
  assert.deepEqual(starWidths(), [100, 100, 100, 100, 100]);
  assert.ok(el('pz-stars').classList.contains('max'));
});

test('the bar: hidden in patrol, BUST while the bust meter fills, EVADE in cooldown', () => {
  hud.update(1 / 60, state(pursuit({ state: 'patrol' })));
  assert.ok(hidden('pz-bar'));
  assert.ok(el('hud-pz').classList.contains('patrol'));

  hud.update(1 / 60, state(pursuit({ state: 'pursuit', bust: 0.6 })));
  assert.ok(!hidden('pz-bar'));
  assert.ok(el('pz-bar').classList.contains('bust'));
  assert.equal(el('pz-label').textContent, 'BUST');
  assert.equal(el('pz-fill').style.width, '60.0%');

  // In pursuit with no bust there is nothing to show.
  hud.update(1 / 60, state(pursuit({ state: 'pursuit', bust: 0, evade: 0.3 })));
  assert.ok(hidden('pz-bar'));

  hud.update(1 / 60, state(pursuit({ state: 'cooldown', evade: 0.25 })));
  assert.ok(el('pz-bar').classList.contains('evade'));
  assert.ok(!el('pz-bar').classList.contains('bust'));
  assert.equal(el('pz-label').textContent, 'EVADE');
  assert.equal(el('pz-fill').style.width, '25.0%');

  // Bust wins over evade, even in cooldown.
  hud.update(1 / 60, state(pursuit({ state: 'cooldown', evade: 0.5, bust: 0.1 })));
  assert.equal(el('pz-label').textContent, 'BUST');
});

test('damage bar: width, critical above 75 %', () => {
  hud.update(1 / 60, state(pursuit({ damage: 0.5 })));
  assert.equal(el('hud-dmg-fill').style.width, '50.0%');
  assert.ok(!el('hud-dmg').classList.contains('crit'));
  hud.update(1 / 60, state(pursuit({ damage: 0.9 })));
  assert.ok(el('hud-dmg').classList.contains('crit'));
});

test('the hold card counts down the penalty', () => {
  hud.update(1 / 60, state(pursuit()));
  assert.ok(hidden('hud-hold'));
  hud.update(1 / 60, state(pursuit({ hold: 4.26, holdTotal: 6, holdReason: 'busted' })));
  assert.ok(!hidden('hud-hold'));
  assert.equal(el('hold-title').textContent, 'BUSTED');
  assert.equal(el('hold-sub').textContent, '+4.3 s PENALTY');
  assert.equal(el('hold-fill').style.width, '71.0%');
  assert.ok(!el('hud-hold').classList.contains('wrecked'));
  hud.update(1 / 60, state(pursuit({ hold: 2, holdTotal: 5, holdReason: 'wrecked' })));
  assert.equal(el('hold-title').textContent, 'WRECKED');
  assert.ok(el('hud-hold').classList.contains('wrecked'));
  hud.update(1 / 60, state(pursuit({ hold: 0 })));
  assert.ok(hidden('hud-hold'));
});

test('penalty served shows under the race time once there is some', () => {
  hud.update(1 / 60, state(pursuit({ penalties: 0 })));
  assert.ok(hidden('hud-pen'));
  hud.update(1 / 60, state(pursuit({ penalties: 12.5 })));
  assert.ok(!hidden('hud-pen'));
  assert.equal(el('hud-pen').textContent, '+12.5 s');
});

test('flash off turns the blinking off', () => {
  hud.update(1 / 60, state(pursuit({ flash: true })));
  assert.ok(el('hud-pz').classList.contains('flash'));
  hud.update(1 / 60, state(pursuit({ flash: false, damage: 0.9 })));
  assert.ok(!el('hud-pz').classList.contains('flash'));
  assert.ok(!el('hud-dmg').classList.contains('flash'));
});

test('radio chatter shows for its duration', () => {
  hud.update(1 / 60, state(pursuit()));
  hud.radio('Unit 12 in pursuit', 2);
  assert.equal(el('hud-radio-text').textContent, 'Unit 12 in pursuit');
  assert.ok(el('hud-radio').classList.contains('show'));
  hud.update(1.5, state(pursuit()));
  assert.ok(el('hud-radio').classList.contains('show'));
  hud.update(0.6, state(pursuit()));
  assert.ok(!el('hud-radio').classList.contains('show'));
});

test('the minimap draws police without throwing', () => {
  hud.update(1 / 60, state(pursuit({
    units: [{ x: 5, z: 5, disabled: false }, { x: -5, z: 9, disabled: true }],
    roadblocks: [{ x: 0, z: 100, yaw: 0, width: 16 }], spikes: [{ x: 0, z: 60, yaw: 1, width: 10 }],
  })));
});
