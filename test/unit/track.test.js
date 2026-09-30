// track/Track.js on every level: the route sampled every metre, its zones,
// the frame and projection queries physics and AI use, the racing line and
// speed profile, and that no two parts of a road run into each other. Also
// the terrain check (tools/check-terrain.js): the ground never pokes up
// through the paved road.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { LEVELS } from './support/levels.js';
import { Track } from '../../src/track/Track.js';
import { Terrain } from '../../src/world/Terrain.js';
import { mulberry32 } from '../../src/util/math.js';

const tracks = new Map(LEVELS.map((l) => [l.id, new Track(l)]));
const near = (a, b, eps, msg) => assert.ok(Math.abs(a - b) <= eps, `${msg}: ${a} vs ${b}`);

for (const L of LEVELS) {
  const t = tracks.get(L.id);

  test(`${L.id}: length, start and finish`, () => {
    assert.equal(t.loop, !!L.loop);
    if (t.loop) {
      assert.equal(t.length, t.n, 'a loop is as long as its samples');
      assert.equal(t.finishS, Infinity, 'a loop has no finish');
      assert.equal(t.roadEnd, Infinity);
    } else {
      const total = L.segments.reduce((a, s) => a + s[0], 0);
      assert.equal(t.length, total, 'one sample per metre');
      assert.equal(t.n, total + 1);
      assert.equal(t.finishS, t.length - (L.finishRunoff ?? 180));
      assert.ok(t.finishS > 3000, `a real race (finish at ${t.finishS} m)`);
    }
    // (A circuit starts its lap on the line: startS 0.)
    assert.ok(t.laps ? t.startS >= 0 && t.startS < 200 : t.startS > 0 && t.startS < 200, `startS ${t.startS}`);
  });

  test(`${L.id}: samples are finite`, () => {
    for (const name of ['px', 'py', 'pz', 'hw', 'fx', 'fz', 'rx', 'rz', 'kappa', 'kSmooth', 'grade', 'bank', 'wallL', 'wallR', 'speedProfile', 'racingLine']) {
      const a = t[name];
      assert.equal(a.length, t.n, `${name} has a value per sample`);
      for (let i = 0; i < a.length; i++) if (!Number.isFinite(a[i])) assert.fail(`${name}[${i}] = ${a[i]}`);
    }
    for (let i = 0; i < t.n; i++) {
      assert.ok(t.hw[i] > 3 && t.hw[i] < 12, `hw[${i}] = ${t.hw[i]}`);
      if (t.wallL[i] < t.hw[i] || t.wallR[i] < t.hw[i]) assert.fail(`wall inside the tarmac at ${i}`);
      near(Math.hypot(t.fx[i], t.fz[i]), 1, 1e-5, `forward is a unit vector at ${i}`);
    }
  });

  test(`${L.id}: consecutive samples are a metre apart`, () => {
    const last = t.loop ? t.n : t.n - 1;
    for (let i = 0; i < last; i++) {
      const j = (i + 1) % t.n;
      const d = Math.hypot(t.px[j] - t.px[i], t.pz[j] - t.pz[i]);
      if (Math.abs(d - 1) > 0.02) assert.fail(`samples ${i}→${j} are ${d.toFixed(3)} m apart`);
    }
  });

  test(`${L.id}: zones are contiguous and cover the route`, () => {
    assert.equal(t.zones.length, L.zones.length);
    assert.equal(t.zones[0].s0, 0);
    assert.equal(t.zones.at(-1).s1, t.length);
    for (let z = 0; z < t.zones.length; z++) {
      const Z = t.zones[z];
      assert.ok(Z.s1 > Z.s0, `zone ${Z.key} has length`);
      if (z) assert.equal(Z.s0, t.zones[z - 1].s1, `zone ${Z.key} starts where ${t.zones[z - 1].key} ends`);
      assert.equal(Z.key, L.zones[z].key);
      for (let s = Z.s0; s < Z.s1; s += 97) assert.equal(t.zone[s], z, `sample ${s} is in zone ${Z.key}`);
    }
    // Blend weights always sum to one.
    for (let s = 0; s < t.length; s += 211) {
      const w = t.zoneBlend(s);
      near(w.reduce((a, b) => a + b, 0), 1, 1e-9, `zoneBlend(${s})`);
      for (const x of w) assert.ok(x >= -1e-9 && x <= 1 + 1e-9);
    }
  });

  test(`${L.id}: frame and pointAt describe the road`, () => {
    const f = t.frame(0);
    near(f.x, t.px[0], 1e-4, 'frame(0).x'); near(f.z, t.pz[0], 1e-4, 'frame(0).z'); near(f.y, t.py[0], 1e-4, 'frame(0).y');
    for (let s = 0.5; s < t.length; s += 173.3) {
      const F = t.frame(s);
      near(Math.hypot(F.fx, F.fz), 1, 1e-6, `|forward| at ${s}`);
      near(F.fx * F.rx + F.fz * F.rz, 0, 1e-6, `right ⟂ forward at ${s}`);
      // Right is forward turned clockwise seen from above: (−fz, fx).
      near(F.rx, -F.fz, 1e-9, 'rx'); near(F.rz, F.fx, 1e-9, 'rz');
      // Interpolates between its two samples.
      const i = Math.floor(s);
      assert.ok(Math.min(t.px[i], t.px[i + 1]) - 1e-3 <= F.x && F.x <= Math.max(t.px[i], t.px[i + 1]) + 1e-3);
      for (const lat of [-4, 0, 3]) {
        const p = t.pointAt(s, lat);
        near(Math.hypot(p.x - F.x, p.z - F.z), Math.abs(lat), 1e-4, `pointAt(${s}, ${lat}) is ${lat} m across`);
        near(p.y, t.surfaceY(s, lat), 1e-4, 'pointAt sits on the surface');
      }
    }
  });

  test(`${L.id}: project() inverts pointAt()`, () => {
    const r = mulberry32(L.id.length);
    for (let k = 0; k < 300; k++) {
      const s = 5 + r() * (t.length - 10);
      const lat = (r() * 2 - 1) * (t.hw[t.idx(s)] - 0.5);
      const p = t.pointAt(s, lat);
      // A nearby hint, as the physics passes last frame's s.
      const q = t.project(p.x, p.z, t.wrap(s + (r() * 2 - 1) * 8));
      const ds = t.loop ? t.ds(s, q.s) : q.s - s;
      assert.ok(Math.abs(ds) < 0.15, `s ${s.toFixed(2)} → ${q.s.toFixed(2)}`);
      assert.ok(Math.abs(q.lat - lat) < 0.15, `lat ${lat.toFixed(2)} → ${q.lat.toFixed(2)} at s ${s.toFixed(1)}`);
    }
  });

  test(`${L.id}: idx, wrap and ds`, () => {
    const n = t.n;
    if (t.loop) {
      assert.equal(t.idx(-1), n - 1);
      assert.equal(t.idx(n), 0);
      assert.equal(t.idx(n + 5.2), 5);
      assert.equal(t.wrap(n + 5), 5);
      assert.equal(t.wrap(-5), n - 5);
      assert.equal(t.wrap(3 * n + 1), 1);
      assert.equal(t.ds(n - 10, 10), 20, 'shortest way round, forwards');
      assert.equal(t.ds(10, n - 10), -20, 'shortest way round, backwards');
      // The loop closes: the last sample runs into the first.
      assert.ok(Math.hypot(t.px[n - 1] - t.px[0], t.pz[n - 1] - t.pz[0]) < 1.1);
      const a = t.frame(n - 0.5), b = t.frame(-0.5);
      near(a.x, b.x, 1e-3, 'frame wraps'); near(a.z, b.z, 1e-3, 'frame wraps');
    } else {
      assert.equal(t.idx(-5), 0);
      assert.equal(t.idx(1e9), n - 1);
      assert.equal(t.idx(10.4), 10);
      assert.equal(t.wrap(-5), -5, 'point-to-point: wrap is the identity');
      assert.equal(t.wrap(n + 50), n + 50);
      assert.equal(t.ds(100, 40), -60);
      // Past the last sample the road carries on straight (the runout).
      const end = t.frame(t.length), past = t.frame(t.length + 20);
      near(Math.hypot(past.x - end.x, past.z - end.z), 20, 0.05, 'runout carries on');
    }
  });

  test(`${L.id}: racing line stays on the tarmac`, () => {
    for (let i = 0; i < t.n; i++) {
      if (Math.abs(t.racingLine[i]) > t.hw[i] - 1.6 + 1e-3) assert.fail(`racingLine[${i}] = ${t.racingLine[i]} with hw ${t.hw[i]}`);
    }
  });

  test(`${L.id}: speed profile is positive, capped and brakeable`, () => {
    const v = t.speedProfile;
    for (let i = 0; i < t.n; i++) {
      if (!(v[i] > 10 && v[i] <= 69.5 + 1e-4)) assert.fail(`speedProfile[${i}] = ${v[i]}`);
    }
    // No corner needs more than the 11 m/s² the profile assumes to brake for.
    const last = t.loop ? t.n : t.n - 1;
    for (let i = 0; i < last; i++) {
      const j = (i + 1) % t.n;
      if (v[i] * v[i] > v[j] * v[j] + 2 * 11 + 1e-2) assert.fail(`can't brake from ${v[i]} to ${v[j]} at ${i}`);
    }
  });

  // tools/check-track.js: two stretches of road further apart along the
  // route than 300 m must not come within their widths (+18 m) of each
  // other, unless one passes over the other with real clearance.
  test(`${L.id}: no two parts of the road overlap`, () => {
    const step = 4, gap = 300, clearance = 6, hits = [];
    const far = (i, j) => (t.loop ? Math.min(j - i, t.n - (j - i)) : j - i) >= gap;
    for (let i = 0; i < t.n; i += step) {
      for (let j = i + gap; j < t.n; j += step) {
        if (!far(i, j)) continue;
        const d = Math.hypot(t.px[i] - t.px[j], t.pz[i] - t.pz[j]);
        if (d < t.hw[i] + t.hw[j] + 18 && Math.abs(t.py[j] - t.py[i]) < clearance) hits.push(`s=${i} and s=${j} are ${d.toFixed(1)} m apart`);
      }
    }
    assert.deepEqual(hits.slice(0, 5), []);
  });

  test(`${L.id}: nearest() and distanceToRoad() find the road`, () => {
    for (let s = 20; s < t.length - 20; s += 331) {
      const p = t.pointAt(s, 2);
      const k = t.nearest(p.x, p.z);
      assert.ok(k >= 0 && Math.abs(t.ds(k, s)) < 3, `nearest to s=${s} is ${k}`);
      const d = t.distanceToRoad(p.x, p.z);
      assert.ok(Math.abs(d.d - 2) < 0.2, `2 m off the centreline at ${s}: ${d.d}`);
    }
    assert.equal(t.nearest(t.bounds.maxX + 5000, t.bounds.maxZ + 5000), -1, 'nothing out in the wilds');
    assert.equal(t.distanceToRoad(t.bounds.maxX + 5000, 0).d, Infinity);
  });

  test(`${L.id}: the terrain never covers the road`, () => {
    const T = new Terrain(t, L);
    T.buildFields();
    T.resolveFlattens();
    const bad = [];
    const f = {};
    for (let s = 0; s < t.length; s += 3) {
      t.frame(s, f);
      if (T.isElevated(t.idx(s))) continue;
      for (const u of [-1, -0.66, -0.33, 0, 0.33, 0.66, 1]) {
        const lat = u * f.hw;
        const g = T.heightAt(f.x + f.rx * lat, f.z + f.rz * lat);
        const road = f.y - lat * f.bank;
        if (g > road + 0.03) bad.push(`s=${s} lat=${lat.toFixed(1)} ground ${(g - road).toFixed(2)} m above`);
      }
    }
    assert.deepEqual(bad.slice(0, 5), [], `${bad.length} samples where the ground is above the road`);
  });
}

test('tags mark named features inside the route', () => {
  for (const [id, t] of tracks) {
    assert.ok(t.tags.length > 0, `${id} has tags`);
    for (const g of t.tags) {
      assert.ok(typeof g.tag === 'string' && g.s1 > g.s0, `${id}: ${g.tag} ${g.s0}-${g.s1}`);
      if (!t.loop) assert.ok(g.s0 >= 0 && g.s1 <= t.length, `${id}: ${g.tag} inside the route`);
    }
    if (!t.loop) assert.equal(t.tag('start').length, 1, `${id} has one start`);
  }
});
