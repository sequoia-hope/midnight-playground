// game/TouchControls.js's analogue maths: the thumb stick's travel to
// steering, its range on different screens, and what each height on the
// pedal slider asks for.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { stickSteer, stickRange, sliderAt, SLIDER } from '../../src/game/TouchControls.js';

const near = (a, b, eps, msg) => assert.ok(Math.abs(a - b) <= eps, `${msg ?? ''} ${a} ≉ ${b}`);

test('stick: dead zone, a gentle curve, full lock (a little early), symmetric', () => {
  assert.equal(stickSteer(0), 0);
  assert.equal(stickSteer(0.05), 0, 'inside the dead zone');
  assert.equal(stickSteer(1), 1);
  assert.equal(stickSteer(-1), -1);
  assert.equal(stickSteer(3), 1, 'no further than full lock');
  assert.equal(stickSteer(0.97), 1, 'the last few per cent are full lock');
  assert.equal(stickSteer(0.9999999999999992), 1);
  let prev = 0;
  for (let d = 0.07; d <= 0.955; d += 0.01) {
    const s = stickSteer(d);
    assert.ok(s > prev && s < 1, `rises with the travel (${d.toFixed(2)})`);
    near(stickSteer(-d), -s, 1e-12);
    prev = s;
  }
  assert.ok(stickSteer(0.5) < 0.5 && stickSteer(0.5) > 0.3, `half travel is a bit under half lock (${stickSteer(0.5).toFixed(2)})`);
});

test('stick: full lock is about a sixth of the short side, within thumb reach', () => {
  near(stickRange(915, 412), 61.8, 1e-9, 'a phone sideways');
  assert.equal(stickRange(412, 915), stickRange(915, 412), 'the same in portrait');
  assert.equal(stickRange(640, 280), 44, 'never too short to control');
  assert.equal(stickRange(1366, 1024), 84, 'nor too long on a tablet');
});

test('slider: brake at the bottom, a gap to coast in, gas above, N₂O at the top', () => {
  const S = SLIDER;
  assert.deepEqual(sliderAt(0), { throttle: 0, brake: 1, nitro: false }, 'the bottom: full brake');
  assert.equal(sliderAt(S.brakeFull).brake, 1, 'and a little way up');
  near(sliderAt(S.brakeTop).brake, 0.15, 1e-12, 'lightest at the top of the brake band');
  assert.deepEqual(sliderAt((S.brakeTop + S.gasBottom) / 2), { throttle: 0, brake: 0, nitro: false }, 'the gap: coasting');
  near(sliderAt(S.gasBottom).throttle, 0.15, 1e-12, 'the lightest gas');
  assert.equal(sliderAt(S.gasFull).throttle, 1, 'flat out from the mark');
  assert.deepEqual(sliderAt((S.gasFull + S.nitro) / 2), { throttle: 1, brake: 0, nitro: false }, 'flat out, no nitro');
  assert.deepEqual(sliderAt(S.nitro), { throttle: 1, brake: 0, nitro: true }, 'N₂O');
  assert.deepEqual(sliderAt(1), { throttle: 1, brake: 0, nitro: true });
  // Steady all the way: the brake eases off going up, the gas comes on.
  let brake = 2, gas = -1;
  for (let u = 0; u <= 1.0001; u += 0.01) {
    const p = sliderAt(u);
    assert.ok(!(p.brake && p.throttle), `never both (${u.toFixed(2)})`);
    if (u <= S.brakeTop) { assert.ok(p.brake <= brake, `brake eases off (${u.toFixed(2)})`); brake = p.brake; }
    if (u >= S.gasBottom) { assert.ok(p.throttle >= gas, `gas comes on (${u.toFixed(2)})`); gas = p.throttle; }
  }
  // Bands a thumb can find: each at least a sixth of the track, the gap a twentieth.
  assert.ok(S.brakeTop >= 1 / 6 && S.nitro - S.gasBottom >= 1 / 6 && 1 - S.nitro >= 1 / 6);
  assert.ok(S.gasBottom - S.brakeTop >= 1 / 20);
});
