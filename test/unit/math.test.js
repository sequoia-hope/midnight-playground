// util/math.js: the small helpers every module leans on, the seeded random
// numbers that make the world identical on every load, and the noise the
// terrain is built from.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  clamp, lerp, invLerp, smoothstep, damp, wrapAngle, DEG, stopSpeed,
  mulberry32, rrange, rpick, hash2, makeNoise2D, fbm, ridged,
} from '../../src/util/math.js';

const near = (a, b, eps = 1e-9, msg) => assert.ok(Math.abs(a - b) <= eps, msg ?? `${a} ≉ ${b}`);

test('clamp keeps values inside [lo, hi]', () => {
  assert.equal(clamp(5, 0, 10), 5);
  assert.equal(clamp(-1, 0, 10), 0);
  assert.equal(clamp(11, 0, 10), 10);
  assert.equal(clamp(0, 0, 10), 0);
  assert.equal(clamp(10, 0, 10), 10);
  assert.equal(clamp(-Infinity, -1, 1), -1);
  assert.equal(clamp(Infinity, -1, 1), 1);
});

test('lerp and invLerp are inverses, and invLerp clamps', () => {
  assert.equal(lerp(2, 6, 0), 2);
  assert.equal(lerp(2, 6, 1), 6);
  assert.equal(lerp(2, 6, 0.25), 3);
  assert.equal(lerp(2, 6, 1.5), 8, 'lerp extrapolates');
  for (const t of [0, 0.1, 0.5, 0.9, 1]) near(invLerp(2, 6, lerp(2, 6, t)), t);
  assert.equal(invLerp(2, 6, 0), 0);
  assert.equal(invLerp(2, 6, 99), 1);
  assert.equal(invLerp(6, 2, 3), 0.75, 'works with a reversed range');
});

test('smoothstep eases from 0 to 1 with flat ends', () => {
  assert.equal(smoothstep(0, 10, -5), 0);
  assert.equal(smoothstep(0, 10, 0), 0);
  assert.equal(smoothstep(0, 10, 5), 0.5);
  assert.equal(smoothstep(0, 10, 10), 1);
  assert.equal(smoothstep(0, 10, 50), 1);
  let prev = -1;
  for (let x = 0; x <= 10; x += 0.5) { const y = smoothstep(0, 10, x); assert.ok(y >= prev); prev = y; }
  // Flat at the ends: small steps near 0 and 1 change it far less than a linear ramp.
  assert.ok(smoothstep(0, 10, 0.1) < 0.01 * 0.1 * 10);
  assert.ok(1 - smoothstep(0, 10, 9.9) < 0.001);
});

test('damp approaches the target frame-rate independently', () => {
  assert.equal(damp(0, 10, 5, 0), 0);
  near(damp(0, 10, 5, 1e6), 10);
  // One 0.1 s step lands where ten 0.01 s steps do.
  let a = 0;
  for (let i = 0; i < 10; i++) a = damp(a, 10, 3, 0.01);
  near(a, damp(0, 10, 3, 0.1), 1e-9);
  // Never overshoots.
  assert.ok(damp(0, 10, 50, 0.5) <= 10);
});

test('wrapAngle maps any angle into [-π, π]', () => {
  const PI = Math.PI;
  for (const a of [0, 1, -1, PI, -PI, 3 * PI, -3 * PI, 7.5, -7.5, 100, -100, 1e4]) {
    const w = wrapAngle(a);
    assert.ok(w >= -PI && w <= PI, `wrapAngle(${a}) = ${w}`);
    near(Math.cos(w), Math.cos(a), 1e-9, `same direction for ${a}`);
    near(Math.sin(w), Math.sin(a), 1e-9, `same direction for ${a}`);
  }
  near(wrapAngle(2 * PI + 0.25), 0.25, 1e-12);
  near(wrapAngle(-2 * PI - 0.25), -0.25, 1e-12);
  near(DEG * 180, PI);
});

test('stopSpeed is the speed that stops in exactly that room', () => {
  assert.equal(stopSpeed(0, 10), 0);
  assert.equal(stopSpeed(-50, 10), 0, 'no room: stand still');
  // v² = 2 a d
  near(stopSpeed(45, 10), 30);
  const v = stopSpeed(120, 7.5);
  near((v * v) / (2 * 7.5), 120, 1e-9);
});

test('mulberry32 is deterministic, seeded and in [0, 1)', () => {
  const a = mulberry32(42), b = mulberry32(42), c = mulberry32(43);
  const sa = Array.from({ length: 1000 }, a), sb = Array.from({ length: 1000 }, b), sc = Array.from({ length: 1000 }, c);
  assert.deepEqual(sa, sb, 'same seed, same sequence');
  assert.notDeepEqual(sa, sc, 'different seed, different sequence');
  for (const x of sa) assert.ok(x >= 0 && x < 1);
  const mean = sa.reduce((s, x) => s + x, 0) / sa.length;
  assert.ok(Math.abs(mean - 0.5) < 0.05, `roughly uniform (mean ${mean})`);
  // Spread over the whole range.
  const bins = new Array(10).fill(0);
  for (const x of sa) bins[Math.floor(x * 10)]++;
  for (const n of bins) assert.ok(n > 60 && n < 140, `bins ${bins}`);
});

test('rrange and rpick draw from their range and list', () => {
  const r = mulberry32(7);
  for (let i = 0; i < 500; i++) {
    const x = rrange(r, -3, 5);
    assert.ok(x >= -3 && x < 5);
  }
  const list = ['a', 'b', 'c'];
  const seen = new Set();
  for (let i = 0; i < 200; i++) seen.add(rpick(r, list));
  assert.deepEqual([...seen].sort(), list);
});

test('hash2 is deterministic and in [0, 1)', () => {
  assert.equal(hash2(3, 4, 1), hash2(3, 4, 1));
  assert.notEqual(hash2(3, 4, 1), hash2(4, 3, 1));
  assert.notEqual(hash2(3, 4, 1), hash2(3, 4, 2));
  for (let i = -20; i < 20; i++) for (let j = -20; j < 20; j++) {
    const h = hash2(i, j);
    assert.ok(h >= 0 && h < 1);
  }
});

test('noise, fbm and ridged are finite, bounded and deterministic', () => {
  const n1 = makeNoise2D(5), n2 = makeNoise2D(5), n3 = makeNoise2D(6);
  let differs = false, min = Infinity, max = -Infinity;
  for (let i = 0; i < 400; i++) {
    const x = (i * 7.31) % 97 - 40, y = (i * 3.77) % 53 - 20;
    const v = n1(x, y);
    assert.ok(Number.isFinite(v));
    assert.equal(v, n2(x, y), 'same seed, same noise');
    if (v !== n3(x, y)) differs = true;
    min = Math.min(min, v); max = Math.max(max, v);
    const f = fbm(n1, x * 0.1, y * 0.1);
    assert.ok(Number.isFinite(f) && Math.abs(f) <= 1.05, `fbm ${f}`);
    const r = ridged(n1, x * 0.1, y * 0.1);
    assert.ok(Number.isFinite(r) && r >= 0 && r <= 1.0001, `ridged ${r}`);
  }
  assert.ok(differs, 'a different seed gives different noise');
  assert.ok(min >= -1.05 && max <= 1.05, `noise range ${min}..${max}`);
  assert.ok(max - min > 0.8, 'noise actually varies');
  // Continuous: nearby points give nearby values.
  assert.ok(Math.abs(n1(10, 10) - n1(10.001, 10)) < 0.01);
});
