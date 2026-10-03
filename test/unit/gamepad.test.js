// game/Gamepad.js (Pads) with fake controllers: bindings and their values,
// the menu buttons, quieting a held button (hush), picking up a new
// binding (capture), maps per controller, and rumble.

import { test, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { Pads, bindingLabel, bindingValue, DEFAULT_MAP } from '../../src/game/Gamepad.js';
import { Input } from '../../src/game/Input.js';

const makePad = ({ id = 'Pad', index = 0, mapping = 'standard', axes = 4 } = {}) => {
  const pad = {
    id, index, mapping, connected: true,
    axes: Array(axes).fill(0),
    buttons: Array.from({ length: 17 }, () => ({ pressed: false, value: 0 })),
    effects: [],
    vibrationActuator: {
      playEffect: (type, p) => { pad.effects.push({ type, ...p }); return Promise.resolve('complete'); },
      reset: () => { pad.effects.push({ type: 'reset' }); return Promise.resolve('complete'); },
    },
  };
  return pad;
};
const plug = (...pads) => { navigator.getGamepads = () => pads; };
const btn = (p, i, on = true) => { p.buttons[i].pressed = on; p.buttons[i].value = on ? 1 : 0; };
afterEach(() => { delete navigator.getGamepads; delete globalThis.window; });

test('binding values: buttons, stick directions, and a trigger reported as an axis', () => {
  const p = makePad();
  p.buttons[7].value = 0.4;
  assert.equal(bindingValue(p, { button: 7 }), 0.4);
  p.buttons[3].pressed = true; // a digital button with no value
  assert.equal(bindingValue(p, { button: 3 }), 1);
  p.axes[0] = -0.6;
  assert.equal(bindingValue(p, { axis: 0, dir: -1, rest: 0 }), 0.6);
  assert.equal(bindingValue(p, { axis: 0, dir: 1, rest: 0 }), 0);
  // Rests at -1, full at +1.
  for (const [v, want] of [[-1, 0], [0, 0.5], [1, 1]]) {
    p.axes[2] = v;
    assert.equal(bindingValue(p, { axis: 2, dir: 1, rest: -1 }), want);
  }
  assert.equal(bindingValue(p, { button: 30 }), 0, 'a button the pad lacks');
});

test('labels: standard names, or numbers for a pad that isn\'t standard', () => {
  assert.equal(bindingLabel({ button: 7 }), 'RT');
  assert.equal(bindingLabel({ button: 14 }), 'D-pad ←');
  assert.equal(bindingLabel({ axis: 0, dir: 1 }), 'Left stick →');
  assert.equal(bindingLabel({ axis: 3, dir: -1 }), 'Right stick ↑');
  assert.equal(bindingLabel({ button: 7 }, false), 'Button 7');
  assert.equal(bindingLabel({ axis: 5, dir: 1 }, false), 'Axis 5 +');
  assert.equal(bindingLabel(undefined), '—');
  const pads = new Pads();
  assert.equal(pads.label('reset'), 'Back', 'no pad: the standard layout');
});

test('the menu buttons: D-pad, left stick, A, B, Start, from any pad', () => {
  const a = makePad(), b = makePad({ index: 1 });
  plug(a, b);
  const pads = new Pads();
  assert.deepEqual(pads.poll(0).nav, { up: false, down: false, left: false, right: false, confirm: false, back: false, start: false });
  btn(a, 13); b.axes[0] = -0.8; btn(b, 0);
  const nav = pads.poll(16).nav;
  assert.equal(nav.down, true, 'D-pad ↓');
  assert.equal(nav.left, true, 'stick ←, on the other pad');
  assert.equal(nav.confirm, true, 'A');
  b.axes[0] = -0.3;
  assert.equal(pads.poll(32).nav.left, false, 'a little stick isn\'t a push');
});

test('hush: what\'s held stays quiet until let go, then works again', () => {
  const p = makePad();
  plug(p);
  const pads = new Pads();
  btn(p, 0); btn(p, 9); p.buttons[7].value = 0.8;
  let s = pads.poll(0);
  assert.equal(s.held.nitro, true);
  assert.deepEqual(s.edges, ['pause']);
  pads.hush();
  s = pads.poll(16);
  assert.equal(s.held.nitro, false, 'A: quiet');
  assert.equal(s.value.throttle, 0, 'RT: quiet');
  assert.equal(s.nav.confirm, false);
  btn(p, 0, false); p.buttons[7].value = 0;
  pads.poll(32);
  btn(p, 0); p.buttons[7].value = 0.5;
  s = pads.poll(48);
  assert.equal(s.held.nitro, true, 'pressed again');
  assert.equal(s.value.throttle, 0.5);
  btn(p, 9, false); pads.poll(64); btn(p, 9);
  assert.deepEqual(pads.poll(80).edges, ['pause'], 'Start: one press, one pause');
});

test('capture: waits for the button that chose it to be let go, then takes the next', () => {
  const p = makePad({ id: 'Pad A' });
  plug(p);
  const saved = [];
  const pads = new Pads({ onSave: (m) => saved.push(structuredClone(m)) });
  btn(p, 0); // A, still down from picking the row
  pads.poll(0);
  let got;
  pads.startCapture('throttle', (b) => { got = b; });
  pads.poll(16); pads.poll(32);
  assert.equal(got, undefined, 'not A');
  assert.equal(pads.state.held.nitro, false, 'and nothing reaches the game while listening');
  btn(p, 0, false);
  pads.poll(48);
  btn(p, 4); // LB
  pads.poll(64);
  assert.deepEqual(got, { button: 4 });
  assert.equal(pads.capture, null);
  assert.deepEqual(pads.mapFor(p).throttle, [{ button: 4 }]);
  assert.deepEqual(saved.at(-1)['Pad A'].throttle, [{ button: 4 }]);
  // The LB that was just bound doesn't fire the throttle until it's pressed again.
  assert.equal(pads.poll(80).value.throttle, 0);
  btn(p, 4, false); pads.poll(96); btn(p, 4);
  assert.equal(pads.poll(112).value.throttle, 1);
  // Another controller keeps the standard layout.
  const q = makePad({ id: 'Pad B', index: 1 });
  assert.equal(pads.mapFor(q), DEFAULT_MAP);
  assert.equal(pads.isRemapped(p), true);
  assert.equal(pads.isRemapped(q), false);
  pads.resetMap(p);
  assert.equal(pads.mapFor(p), DEFAULT_MAP);
});

test('capture: one button, one action — taking RB for nitro frees it from the handbrake', () => {
  const p = makePad();
  plug(p);
  const pads = new Pads();
  pads.poll(0);
  pads.startCapture('nitro', () => {});
  pads.poll(16);
  btn(p, 5);
  pads.poll(32);
  const m = pads.mapFor(p);
  assert.deepEqual(m.nitro, [{ button: 5 }]);
  assert.deepEqual(m.handbrake, [{ button: 2 }], 'X is still the handbrake');
});

test('capture: sticks and triggers that are axes', () => {
  // Standard: an axis is a stick, centred at 0.
  const p = makePad();
  plug(p);
  const pads = new Pads();
  pads.poll(0);
  let got;
  pads.startCapture('left', (b) => { got = b; });
  pads.poll(16);
  p.axes[2] = -0.9;
  pads.poll(32);
  assert.deepEqual(got, { axis: 2, dir: -1, rest: 0 });
  // Not standard: a resting -1 is a trigger; it reads 0 there and 1 pulled.
  const q = makePad({ id: 'Odd pad', mapping: '', axes: 6 });
  q.axes[5] = -1;
  plug(q);
  const pads2 = new Pads();
  pads2.poll(0);
  pads2.startCapture('throttle', (b) => { got = b; });
  pads2.poll(16);
  q.axes[5] = 0.7;
  pads2.poll(32);
  assert.deepEqual(got, { axis: 5, dir: 1, rest: -1 });
  q.axes[5] = 1; pads2.poll(48);
  q.axes[5] = -1; pads2.poll(64); // let go (it was held through the capture)
  q.axes[5] = 0;
  assert.equal(pads2.poll(80).value.throttle, 0.5, 'half pulled');
  assert.equal(pads2.label('throttle'), 'Axis 5 +');
});

test('capture: gives up after 8 s, or when cancelled', () => {
  const p = makePad();
  plug(p);
  const pads = new Pads();
  pads.poll(0);
  const results = [];
  pads.startCapture('camera', (b) => results.push(b));
  pads.poll(4000);
  pads.poll(8100);
  assert.deepEqual(results, [null]);
  assert.equal(pads.capture, null);
  pads.startCapture('camera', (b) => results.push(b));
  pads.cancelCapture();
  assert.deepEqual(results, [null, null]);
  assert.deepEqual(pads.mapFor(p), DEFAULT_MAP);
});

test('a remapped D-pad steers like keys (ramped, not analogue)', () => {
  globalThis.window = new EventTarget();
  const input = new Input();
  const p = makePad({ id: 'Pad' });
  plug(p);
  input.pads.maps = { Pad: { ...DEFAULT_MAP, left: [{ button: 14 }], right: [{ button: 15 }] } };
  btn(p, 15);
  let s = input.update(0.1);
  assert.ok(Math.abs(s.steer - 0.36) < 1e-9, `ramps in like a key (${s.steer})`);
  assert.equal(s.analog, false);
  for (let i = 0; i < 10; i++) s = input.update(0.1);
  assert.equal(s.steer, 1);
  btn(p, 15, false);
  p.axes[0] = 0.8; // the stick isn't bound any more
  for (let i = 0; i < 10; i++) s = input.update(0.1);
  assert.equal(s.steer, 0);
});

test('a stick bound to a pedal has a dead zone', () => {
  const p = makePad({ id: 'Pad' });
  plug(p);
  const pads = new Pads({ maps: { Pad: { ...DEFAULT_MAP, throttle: [{ axis: 3, dir: -1, rest: 0 }] } } });
  p.axes[3] = -0.1;
  assert.equal(pads.poll(0).value.throttle, 0, 'drift at rest');
  p.axes[3] = -1;
  assert.equal(pads.poll(16).value.throttle, 1);
});

test('rumble: a kick fades, a steady buzz lasts while it\'s fed, and it stops with a reset', () => {
  const p = makePad();
  plug(p);
  const pads = new Pads();
  pads.poll(0);
  pads.kick(0.8, 0.4, 200);
  pads.poll(16);
  let e = p.effects.splice(0);
  assert.equal(e.length, 1);
  assert.equal(e[0].type, 'dual-rumble');
  assert.ok(e[0].strongMagnitude > 0.7 && e[0].strongMagnitude <= 0.8);
  assert.ok(e[0].duration >= 100, 'outlasts the gap to the next');
  pads.poll(32);
  assert.equal(p.effects.length, 0, 'not resent every frame');
  pads.poll(120);
  e = p.effects.splice(0);
  assert.equal(e.length, 1, 'refreshed as it fades');
  assert.ok(e[0].strongMagnitude < 0.6);
  pads.poll(230);
  assert.deepEqual(p.effects.splice(0), [{ type: 'reset' }], 'over: stopped');
  pads.poll(246);
  assert.equal(p.effects.length, 0, 'once');

  // Steady: fed each frame, then not (the race paused).
  for (let t = 300; t < 600; t += 16) { pads.feel(0.3, 0.2); pads.poll(t); }
  assert.ok(p.effects.length >= 3 && p.effects.every((x) => x.strongMagnitude === 0.3));
  p.effects.length = 0;
  pads.poll(620); // within 150 ms of the last feel: still on
  pads.poll(800);
  assert.deepEqual(p.effects.at(-1), { type: 'reset' });
  assert.equal(p.effects.filter((x) => x.type === 'reset').length, 1);

  // Off: nothing.
  p.effects.length = 0;
  pads.rumbleOn = false;
  pads.kick(1, 1, 500); pads.feel(1, 1);
  pads.poll(900);
  assert.deepEqual(p.effects, []);
});

test('rumble goes to the pad last used, and a pad without a motor is fine', () => {
  const a = makePad(), b = makePad({ index: 1 });
  delete a.vibrationActuator;
  plug(a, b);
  const pads = new Pads();
  pads.poll(0);
  pads.kick(1, 1, 300);
  pads.poll(16); // a is in use: no motor, no error
  btn(b, 0); pads.poll(32);
  btn(b, 0, false);
  pads.kick(1, 1, 300);
  pads.poll(200);
  assert.ok(b.effects.length > 0, 'b, once it was used');
  assert.equal(pads.active.index, 1);
});
