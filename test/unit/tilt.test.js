// game/TiltSteer.js: the screen's roll from deviceorientation in every way
// a phone can be held, roll to steering, the sensor and permission states
// with a fake window, and tilt merging into Input.

import { test, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { screenRoll, rollToSteer, fullLockFor, TiltSteer } from '../../src/game/TiltSteer.js';
import { Input } from '../../src/game/Input.js';
import { pose, expectedRoll } from './support/pose.js';

const DEG = Math.PI / 180;
const near = (a, b, eps, msg) => assert.ok(Math.abs(a - b) <= eps, `${msg ?? ''} ${a} ≉ ${b}`);

test('roll from beta and gamma: the phone held still and level reads 0', () => {
  for (const angle of [0, 90, -90, 270, 180]) {
    near(screenRoll(0, 0, angle), 0, 1e-12, `flat, angle ${angle}`);
  }
  near(screenRoll(90, 0, 0), 0, 1e-12, 'portrait, upright');
  near(screenRoll(0, -90, 90), 0, 1e-12, 'landscape, upright');
  near(screenRoll(0, 90, -90), 0, 1e-12, 'the other landscape, upright');
});

// The spec's own sign rules, with no pose maths: gamma grows as the phone
// tips to the right, beta as its top edge comes up.
test('roll: the right-hand side of the screen dipping is positive', () => {
  near(screenRoll(0, 20, 0), 20 * DEG, 1e-12, 'portrait, tipped right');
  near(screenRoll(0, -20, 0), -20 * DEG, 1e-12, 'portrait, tipped left');
  // Turned anticlockwise (90), the phone's top is on the left: lifting it
  // drops the right-hand side, which is a right turn.
  near(screenRoll(10, -90, 90), 10 * DEG, 1e-12, 'landscape, top end lifted');
  near(screenRoll(-10, -90, 90), -10 * DEG, 1e-12, 'landscape, top end lowered');
  // Turned clockwise, the top is on the right: the same lift is a left turn.
  near(screenRoll(10, 90, -90), -10 * DEG, 1e-12, 'other landscape, top end lifted');
  near(screenRoll(10, 90, 270), -10 * DEG, 1e-12, 'screen.orientation says 270 for it');
});

test('roll: turning the phone like a wheel steers that way, however it is held', () => {
  for (const angle of [90, -90, 270, 0, 180]) {
    for (const back of [0, 20, 45, 70]) {
      for (const turn of [-35, -12, -3, 0, 3, 12, 35]) {
        const p = pose({ angle, turn, back });
        const roll = screenRoll(p.beta, p.gamma, angle);
        near(roll, expectedRoll({ turn, back }), 1e-9, `angle ${angle} back ${back} turn ${turn}:`);
        if (turn) assert.equal(Math.sign(roll), Math.sign(turn), `angle ${angle} back ${back} turn ${turn} steers the same way`);
      }
    }
  }
  // Lying back further only softens it: 20° of wheel at 45° back is 14°.
  const p = pose({ angle: 90, turn: 20, back: 45 });
  near(screenRoll(p.beta, p.gamma, 90) / DEG, 14.0, 0.05);
});

test('roll to steering: dead zone, a gentle curve, full lock, symmetric', () => {
  const full = fullLockFor(0.5);
  near(full / DEG, 26, 1e-9, 'middle sensitivity: full lock at 26°');
  near(fullLockFor(0) / DEG, 40, 1e-9);
  near(fullLockFor(1) / DEG, 12, 1e-9);
  assert.equal(rollToSteer(0, full), 0);
  assert.equal(rollToSteer(1.5 * DEG, full), 0, 'inside the dead zone');
  assert.equal(rollToSteer(-1.5 * DEG, full), 0);
  assert.equal(rollToSteer(full, full), 1, 'full lock');
  assert.equal(rollToSteer(60 * DEG, full), 1, 'and no further');
  assert.equal(rollToSteer(-full, full), -1);
  let prev = 0;
  for (let d = 2; d <= 26; d += 0.5) {
    const s = rollToSteer(d * DEG, full);
    assert.ok(s >= prev, `rises with the tilt (${d}°)`);
    near(rollToSteer(-d * DEG, full), -s, 1e-12);
    prev = s;
  }
  assert.ok(rollToSteer(8 * DEG, full) < (8 - 2) / (26 - 2), 'softer than linear near the centre');
  assert.ok(rollToSteer(6 * DEG, fullLockFor(1)) > rollToSteer(6 * DEG, fullLockFor(0)), 'sensitivity scales it');
});

// A window with just what TiltSteer uses.
function fakeWindow({ secure = true, orientation = 90, requestPermission } = {}) {
  const w = new EventTarget();
  w.isSecureContext = secure;
  w.orientation = orientation;
  w.DeviceOrientationEvent = class {};
  if (requestPermission) w.DeviceOrientationEvent.requestPermission = requestPermission;
  w.listeners = 0;
  const add = w.addEventListener.bind(w), remove = w.removeEventListener.bind(w);
  w.addEventListener = (t, f) => { if (t === 'deviceorientation') w.listeners++; add(t, f); };
  w.removeEventListener = (t, f) => { if (t === 'deviceorientation') w.listeners = Math.max(0, w.listeners - 1); remove(t, f); };
  w.tilt = (o) => w.dispatchEvent(Object.assign(new Event('deviceorientation'), o));
  return w;
}
const tick = () => new Promise((r) => setImmediate(r));

let steer;
afterEach(() => { steer?.enable(false); steer = null; delete globalThis.window; });

test('the sensor: waiting, live on the first reading, smoothed, off again', () => {
  const w = fakeWindow();
  steer = new TiltSteer(w);
  const seen = [];
  steer.onChange = (s) => seen.push(s);
  assert.equal(steer.live, false);
  steer.enable(true);
  assert.equal(steer.state, 'waiting');
  assert.equal(w.listeners, 1);
  steer.enable(true);
  assert.equal(w.listeners, 1, 'enabling twice listens once');

  w.tilt(pose({ angle: 90, turn: 30, back: 30 }));
  assert.equal(steer.state, 'live');
  assert.equal(steer.live, true);
  assert.ok(steer.update(0.016) > 0 && steer.steer < 0.5, 'eases in');
  for (let i = 0; i < 30; i++) steer.update(0.016);
  near(steer.steer, rollToSteer(expectedRoll({ turn: 30, back: 30 }), steer.fullLock), 1e-4, 'settles on the tilt');

  w.tilt(pose({ angle: 90, turn: 0, back: 30 }));
  for (let i = 0; i < 30; i++) steer.update(0.016);
  near(steer.steer, 0, 1e-4, 'back to centre');

  steer.enable(false);
  assert.equal(steer.state, 'off');
  assert.equal(steer.live, false);
  assert.equal(w.listeners, 0, 'stops listening');
  assert.deepEqual(seen, ['waiting', 'live', 'off']);
});

test('the sensor: no sensor, then one; an http page; the screen turning round', () => {
  let w = fakeWindow();
  steer = new TiltSteer(w);
  steer.enable(true);
  w.tilt({ alpha: null, beta: null, gamma: null }); // Chrome with no sensor
  assert.equal(steer.state, 'none');
  w.tilt(pose({ angle: 90, turn: 10 }));
  assert.equal(steer.state, 'live', 'a sensor turning up later still counts');
  steer.enable(false);

  w = fakeWindow({ secure: false });
  steer = new TiltSteer(w);
  steer.enable(true);
  assert.equal(steer.state, 'insecure');
  w.tilt({ alpha: null, beta: null, gamma: null });
  assert.equal(steer.state, 'insecure', 'the reason stays the useful one');
  w.tilt(pose({ angle: 90, turn: 10 }));
  assert.equal(steer.state, 'live', 'a browser that sends it anyway');
  steer.enable(false);

  // The same physical right turn, whichever way round the phone is.
  w = fakeWindow({ orientation: -90 });
  steer = new TiltSteer(w);
  steer.enable(true);
  w.tilt(pose({ angle: -90, turn: 15 }));
  assert.ok(steer.roll > 0);
  w.orientation = undefined; // no window.orientation: screen.orientation.angle
  w.screen = { orientation: { angle: 270 } };
  w.tilt(pose({ angle: 270, turn: 15 }));
  near(steer.roll, expectedRoll({ turn: 15 }), 1e-9);

  const bare = new EventTarget();
  steer = new TiltSteer(bare);
  steer.enable(true);
  assert.equal(steer.state, 'none', 'no DeviceOrientationEvent at all');
});

test('iPhone: motion access is asked for, remembered, and refusals are reported', async () => {
  let asks = 0, answer = 'granted', gesture = true;
  const requestPermission = () => {
    asks++;
    if (!gesture) return Promise.reject(new DOMException('needs a user gesture', 'NotAllowedError'));
    return Promise.resolve(answer);
  };
  let w = fakeWindow({ requestPermission });
  steer = new TiltSteer(w);

  // At page load (no tap) it can't ask yet.
  gesture = false;
  steer.enable(true);
  await tick();
  assert.equal(steer.state, 'ask');
  assert.equal(w.listeners, 0);

  // The tap: asked, granted, listening.
  gesture = true;
  steer.enable(true);
  await tick();
  assert.equal(asks, 2);
  assert.equal(steer.state, 'waiting');
  assert.equal(w.listeners, 1);
  w.tilt(pose({ turn: -10 }));
  assert.equal(steer.live, true);
  assert.ok(steer.roll < 0);

  // Off and on again doesn't ask twice.
  steer.enable(false);
  steer.enable(true);
  assert.equal(asks, 2);
  assert.equal(w.listeners, 1);
  steer.enable(false);

  // Turned down.
  answer = 'denied';
  w = fakeWindow({ requestPermission });
  steer = new TiltSteer(w);
  steer.enable(true);
  await tick();
  assert.equal(steer.state, 'denied');
  assert.equal(w.listeners, 0);

  // Switched off while the question is up: nothing starts listening.
  answer = 'granted';
  w = fakeWindow({ requestPermission });
  steer = new TiltSteer(w);
  steer.enable(true);
  steer.enable(false);
  await tick();
  assert.equal(steer.state, 'off');
  assert.equal(w.listeners, 0);
});

test('Input: tilt steering goes straight through (no ramp), and a held key wins', () => {
  globalThis.window = new EventTarget();
  const input = new Input();
  const touch = { throttle: 0, brake: 0, steer: 0, held: {}, tilt: 0.4, analogSteer() { return this.tilt; } };
  input.touch = touch;
  near(input.update(0.016).steer, 0.4, 1e-12, 'no ramp');
  touch.tilt = -0.7;
  near(input.update(0.016).steer, -0.7, 1e-12);

  window.dispatchEvent(Object.assign(new Event('keydown'), { code: 'KeyD' }));
  const s = input.update(0.1).steer;
  near(s, -0.7 + 0.9, 1e-9, 'D counter-steers from where the tilt was, at the key rate');
  window.dispatchEvent(Object.assign(new Event('keyup'), { code: 'KeyD' }));
  near(input.update(0.016).steer, -0.7, 1e-12, 'let go: back on the tilt');

  // Steering on the ◂ ▸ pads (analogSteer gives null): ramped as before.
  touch.tilt = null; touch.steer = 1;
  input.steer = 0;
  near(input.update(0.1).steer, 0.36, 1e-9);
});
