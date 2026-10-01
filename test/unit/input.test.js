// game/Input.js with a fake window and navigator: the keymap, one-shot
// actions (consume), the steering ramp, merging the on-screen touch pads,
// a gamepad, which steering counts as analogue, and turning input off
// (enabled = false) during the countdown and menus.

import { test, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { Input } from '../../src/game/Input.js';

let input;
beforeEach(() => {
  globalThis.window = new EventTarget();
  input = new Input();
});
afterEach(() => {
  delete globalThis.window;
  delete navigator.getGamepads;
});

const key = (type, code, { repeat = false } = {}) => {
  const e = Object.assign(new Event(type, { cancelable: true }), { code, repeat });
  window.dispatchEvent(e);
  return e;
};
const down = (code, o) => key('keydown', code, o);
const up = (code) => key('keyup', code);
const near = (a, b, eps = 1e-9) => assert.ok(Math.abs(a - b) <= eps, `${a} ≉ ${b}`);

test('the keymap: pedals, handbrake, nitro and look back', () => {
  const cases = [
    ['KeyW', 'throttle', 1], ['ArrowUp', 'throttle', 1],
    ['KeyS', 'brake', 1], ['ArrowDown', 'brake', 1],
    ['Space', 'handbrake', true],
    ['ShiftLeft', 'nitro', true], ['ShiftRight', 'nitro', true], ['KeyN', 'nitro', true],
    ['KeyB', 'lookBack', true],
  ];
  for (const [code, field, on] of cases) {
    down(code);
    assert.equal(input.update(0.016)[field], on, `${code} → ${field}`);
    up(code);
    assert.equal(input.update(0.016)[field], typeof on === 'boolean' ? false : 0, `${code} released`);
  }
});

test('game keys are kept from the page (no scrolling), other keys are not', () => {
  for (const code of ['ArrowUp', 'ArrowDown', 'Space', 'KeyW', 'Escape']) assert.equal(down(code).defaultPrevented, true, code);
  assert.equal(down('KeyQ').defaultPrevented, false);
  assert.equal(down('Tab').defaultPrevented, false, 'Tab still moves focus');
  assert.equal(down('ArrowUp', { repeat: true }).defaultPrevented, true, 'auto-repeat too');
});

test('one-shot actions: camera, reset, pause and music, consumed once', () => {
  for (const [code, name] of [['KeyC', 'camera'], ['KeyR', 'reset'], ['Escape', 'pause'], ['KeyP', 'pause'], ['KeyM', 'music']]) {
    down(code);
    assert.equal(input.consume(name), true, `${code} → ${name}`);
    assert.equal(input.consume(name), false, 'only once');
    up(code);
  }
  // Holding a key down (auto-repeat) doesn't fire it again.
  down('KeyC');
  input.consume('camera');
  down('KeyC', { repeat: true });
  assert.equal(input.consume('camera'), false);
  assert.equal(input.consume('nothing'), false);
});

test('steering ramps in, snaps back faster, and full lock is ±1', () => {
  down('KeyD');
  input.update(0.1);
  near(input.state.steer, 0.36, 1e-9); // 3.6 per second turning in
  for (let i = 0; i < 10; i++) input.update(0.1);
  assert.equal(input.state.steer, 1, 'full lock');
  up('KeyD');
  input.update(0.1);
  near(input.state.steer, 0.3, 1e-9); // 7 per second back to centre
  input.update(0.1);
  input.update(0.1);
  assert.equal(input.state.steer, 0, 'centred, no overshoot');
  // Flicking from full right to left counter-steers at 9 per second.
  down('KeyD'); for (let i = 0; i < 10; i++) input.update(0.1); up('KeyD');
  down('KeyA');
  input.update(0.1);
  near(input.state.steer, 0.1, 1e-9);
  // Both at once cancel out.
  down('KeyD');
  for (let i = 0; i < 20; i++) input.update(0.1);
  assert.equal(input.state.steer, 0);
});

test('losing focus lets go of every key', () => {
  down('KeyW'); down('KeyD');
  window.dispatchEvent(new Event('blur'));
  const s = input.update(0.016);
  assert.equal(s.throttle, 0);
  input.update(1);
  assert.equal(input.state.steer, 0);
});

test('the touch pads merge in, and the keyboard wins where both are used', () => {
  input.touch = { throttle: 1, brake: 0, steer: -1, held: { handbrake: true, nitro: true } };
  let s = input.update(0.1);
  assert.equal(s.throttle, 1);
  assert.equal(s.handbrake, true);
  assert.equal(s.nitro, true);
  near(s.steer, -0.36, 1e-9, 'touch steering ramps like a key');
  input.touch.throttle = 0; input.touch.brake = 1;
  s = input.update(0.1);
  assert.equal(s.throttle, 0); assert.equal(s.brake, 1);
  down('KeyD');
  for (let i = 0; i < 20; i++) s = input.update(0.1);
  assert.equal(s.steer, 1, 'a held key overrides the steering pad');
  down('KeyW');
  assert.equal(input.update(0.1).throttle, 1);
});

test('a gamepad: analogue steering with a dead zone, triggers and buttons', () => {
  const buttons = Array.from({ length: 17 }, () => ({ pressed: false, value: 0 }));
  const pad = { connected: true, axes: [0, 0], buttons };
  navigator.getGamepads = () => [null, pad];
  assert.equal(input.update(0.016).steer, 0);
  pad.axes[0] = 0.1;
  assert.equal(input.update(0.016).steer, 0, 'inside the dead zone');
  pad.axes[0] = 0.56;
  near(input.update(0.016).steer, Math.pow((0.56 - 0.12) / 0.88, 1.4), 1e-9);
  pad.axes[0] = -1;
  assert.equal(input.update(0.016).steer, -1);
  pad.axes[0] = 0;
  buttons[7].value = 0.6; buttons[6].value = 0.3;
  let s = input.update(0.016);
  near(s.throttle, 0.6); near(s.brake, 0.3);
  buttons[0].pressed = true; buttons[2].pressed = true; buttons[1].pressed = true;
  s = input.update(0.016);
  assert.equal(s.nitro, true, 'A: nitro');
  assert.equal(s.handbrake, true, 'X: handbrake');
  assert.equal(s.lookBack, true, 'B: look back');
  buttons[2].pressed = false; buttons[5].pressed = true;
  assert.equal(input.update(0.016).handbrake, true, 'RB: handbrake');
  // Y, Start and Back fire once per press.
  for (const [i, name] of [[3, 'camera'], [9, 'pause'], [8, 'reset']]) {
    buttons[i].pressed = true;
    input.update(0.016);
    assert.equal(input.consume(name), true, `button ${i} → ${name}`);
    input.update(0.016);
    assert.equal(input.consume(name), false, 'held: no repeat');
    buttons[i].pressed = false;
    input.update(0.016);
  }
  // A disconnected pad is ignored.
  pad.connected = false;
  buttons[7].value = 1;
  assert.equal(input.update(0.016).throttle, 0);
});

// Physics scales analogue steering to the grip (CarPhysics: ANALOG_LOCK), so
// Input says which kind this frame's steering is.
test('steering is flagged analogue from the stick, tilt or a gamepad, not keys or ◂ ▸ pads', () => {
  let stick = 0.4;
  input.touch = { throttle: 0, brake: 0, steer: 0, held: {}, analogSteer: () => stick };
  let s = input.update(0.016);
  assert.equal(s.analog, true, 'the stick');
  near(s.steer, 0.4);
  down('KeyA');
  s = input.update(0.016);
  assert.equal(s.analog, false, 'a held key wins, and is a key');
  up('KeyA');
  stick = null; // ◂ ▸ pads: no analogue reading
  input.touch.steer = 1;
  assert.equal(input.update(0.016).analog, false, 'the pads');
  input.touch = null;
  const pad = { connected: true, axes: [0.6, 0], buttons: Array.from({ length: 17 }, () => ({ pressed: false, value: 0 })) };
  navigator.getGamepads = () => [pad];
  assert.equal(input.update(0.016).analog, true, 'a gamepad stick');
  pad.axes[0] = 0.05;
  assert.equal(input.update(0.016).analog, false, 'a gamepad at rest leaves it to the keys');
});

test('enabled = false zeroes everything', () => {
  input.touch = { throttle: 1, brake: 1, steer: 1, held: { handbrake: true, nitro: true } };
  down('KeyW'); down('Space'); down('ShiftLeft');
  for (let i = 0; i < 5; i++) input.update(0.1);
  input.enabled = false;
  const s = input.update(0.1);
  assert.deepEqual({ ...s, lookBack: false }, { throttle: 0, brake: 0, steer: 0, analog: false, handbrake: false, nitro: false, lookBack: false });
  // One-shot actions still get through (pause works on the menu).
  down('Escape');
  assert.equal(input.consume('pause'), true);
});
