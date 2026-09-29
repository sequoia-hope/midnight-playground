// Level data (src/levels/*.js): what the menu lists and the world builder,
// terrain, traffic and rivals read from each level. Also checks that the
// README's lengths and rival counts match the data.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { LEVELS, levelById } from '../../src/levels/index.js';
import { ROAD_TYPES } from '../../src/track/roadTypes.js';
import { LANDFORMS } from '../../src/world/Terrain.js';
import { CAR_SPECS } from '../../src/vehicles/CarPhysics.js';

const WORLD = new URL('../../src/world/', import.meta.url);
const README = fs.readFileSync(new URL('../../README.md', import.meta.url), 'utf8');

test('the menu lists five levels with unique ids, in order', () => {
  assert.deepEqual(LEVELS.map((l) => l.id), ['sierra', 'coast', 'streets', 'desert', 'cruise']);
  assert.equal(new Set(LEVELS.map((l) => l.num)).size, LEVELS.length, 'unique level numbers');
  assert.deepEqual(LEVELS.map((l) => l.num), ['LEVEL 1', 'LEVEL 2', 'LEVEL 3', 'LEVEL 4', 'ENDLESS']);
});

test('levelById finds each level and falls back to the first', () => {
  for (const l of LEVELS) assert.equal(levelById(l.id), l);
  // A stale saved id, a bad ?level= or nothing at all: Level 1.
  for (const bad of ['nope', '', undefined, null, 'SIERRA']) assert.equal(levelById(bad), LEVELS[0]);
});

for (const L of LEVELS) {
  test(`${L.id}: required fields`, () => {
    for (const k of ['id', 'num', 'title', 'desc']) assert.ok(typeof L[k] === 'string' && L[k].length, `${k} is a non-empty string`);
    assert.ok(['race', 'cruise'].includes(L.mode), `mode ${L.mode}`);
    assert.ok(Array.isArray(L.zones) && L.zones.length > 0, 'zones');
    assert.ok(Array.isArray(L.sky) && L.sky.length >= 2, 'sky keyframes');
    assert.ok(Array.isArray(L.rivals), 'rivals');
    assert.ok(Array.isArray(L.traffic), 'traffic');
    assert.ok(Number.isFinite(L.sunAzimuth), 'sunAzimuth');
    // Exactly one kind of route.
    assert.ok(!!L.segments !== !!L.loop, 'either segments or a loop');
    if (L.mode === 'cruise') {
      assert.ok(L.loop && typeof L.loop.path === 'function', 'a cruise is a loop');
      assert.equal(L.rivals.length, 0, 'a cruise has no rivals');
    } else {
      assert.ok(Array.isArray(L.segments) && L.segments.length > 0, 'a race has segments');
      assert.ok(L.rivals.length > 0, 'a race has rivals');
      assert.ok(Number.isFinite(L.startHeight), 'startHeight');
    }
  });

  test(`${L.id}: zones name a known landform and existing scenery`, () => {
    assert.equal(new Set(L.zones.map((z) => z.key)).size, L.zones.length, 'unique zone keys');
    for (const z of L.zones) {
      for (const k of ['key', 'name', 'sub']) assert.ok(typeof z[k] === 'string' && z[k], `${z.key}.${k}`);
      assert.ok(LANDFORMS[z.landform], `${z.key}: landform '${z.landform}' is one of ${Object.keys(LANDFORMS)}`);
      // World.loadScenery imports ./<scenery>.js; a missing file only warns.
      assert.ok(z.scenery, `${z.key}: scenery`);
      assert.ok(fs.existsSync(new URL(z.scenery + '.js', WORLD)), `${z.key}: src/world/${z.scenery}.js exists`);
      assert.match(z.color, /^#[0-9a-f]{6}$/i, `${z.key}: color`);
    }
  });

  test(`${L.id}: sky keyframes run from 0 to 1 with finite values`, () => {
    assert.equal(L.sky[0].s, 0);
    assert.equal(L.sky.at(-1).s, 1);
    const keys = Object.keys(L.sky[0]);
    for (let i = 0; i < L.sky.length; i++) {
      const k = L.sky[i];
      if (i) assert.ok(k.s > L.sky[i - 1].s, `s increases at keyframe ${i}`);
      assert.deepEqual(Object.keys(k).sort(), [...keys].sort(), `keyframe ${i} has the same fields`);
      for (const [f, v] of Object.entries(k)) assert.ok(Number.isFinite(v), `keyframe ${i}.${f} = ${v}`);
      assert.ok(k.night >= 0 && k.night <= 1, `night ${k.night}`);
      assert.ok(k.fogD > 0 && k.exp > 0 && k.sunI >= 0 && k.hemiI >= 0);
    }
  });

  if (L.segments) {
    test(`${L.id}: segments are well formed`, () => {
      const first = L.segments[0][3] || {};
      assert.equal(first.zone, 0, 'the first segment starts zone 0');
      assert.ok(first.road, 'the first segment names its road type');
      let zone = 0;
      for (const [i, seg] of L.segments.entries()) {
        const [len, turn, rise, extra = {}] = seg;
        assert.ok(Number.isInteger(len) && len > 0, `segment ${i}: length ${len}`);
        assert.ok(Number.isFinite(turn) && Math.abs(turn) <= 270, `segment ${i}: turn ${turn}`);
        assert.ok(Number.isFinite(rise), `segment ${i}: rise ${rise}`);
        assert.ok(Math.abs(rise / len) < 0.3, `segment ${i}: grade ${rise / len}`);
        if (extra.road) assert.ok(ROAD_TYPES[extra.road], `segment ${i}: road '${extra.road}'`);
        if (extra.zone !== undefined) {
          assert.ok(extra.zone >= zone && extra.zone < L.zones.length, `segment ${i}: zone ${extra.zone} (zones only move forward)`);
          zone = extra.zone;
        }
      }
      assert.equal(zone, L.zones.length - 1, 'the route reaches the last zone');
    });
  }

  test(`${L.id}: rivals drive real cars`, () => {
    assert.equal(new Set(L.rivals.map((r) => r.name)).size, L.rivals.length, 'unique names');
    for (const r of L.rivals) {
      assert.ok(CAR_SPECS[r.kind], `${r.name}: kind '${r.kind}'`);
      assert.ok(r.skill > 0.8 && r.skill <= 1, `${r.name}: skill ${r.skill}`);
      assert.ok(r.power > 300 && r.power < 700, `${r.name}: power ${r.power}`);
      assert.ok(Number.isInteger(r.color) && r.color >= 0 && r.color <= 0xffffff, `${r.name}: color`);
    }
  });

  test(`${L.id}: one traffic rule per zone, with sane numbers`, () => {
    // Traffic.update reads rules[zone].gap for every spawn point.
    assert.equal(L.traffic.length, L.zones.length);
    for (const [i, r] of L.traffic.entries()) {
      assert.ok(r.gap[0] > 0 && r.gap[0] <= r.gap[1], `rule ${i}: gap ${r.gap}`);
      assert.ok(r.oncoming >= 0 && r.oncoming <= 1, `rule ${i}: oncoming ${r.oncoming}`);
      if (r.opposite !== undefined) assert.ok(r.opposite >= 0 && r.opposite <= 1, `rule ${i}: opposite`);
      // An empty mix is a zone with no traffic (the desert's lake bed).
      if (r.mix.length) {
        assert.ok(r.speed[0] > 0 && r.speed[0] <= r.speed[1] && r.speed[1] < 45, `rule ${i}: speed ${r.speed}`);
        const sum = r.mix.reduce((a, [, w]) => a + w, 0);
        assert.ok(Math.abs(sum - 1) < 1e-6, `rule ${i}: mix weights sum to ${sum}`);
        for (const [k, w] of r.mix) assert.ok(typeof k === 'string' && w > 0, `rule ${i}: ${k} ${w}`);
      }
    }
  });
}

// The menu card and the README both quote each race's length (the route
// minus the run-off past the finish, as main.js's levelStats works it out).
test('README: level lengths and rival counts match the data', () => {
  const km = (l) => (l.segments.reduce((a, s) => a + s[0], 0) - (l.finishRunoff ?? 180)) / 1000;
  const claims = [
    ['sierra', /Level 1: Sierra to the City\.\*\* A ([\d.]+) km sprint against (\w+) rivals/],
    ['coast', /Level 2: Coast Highway\.\*\* A ([\d.]+) km sprint/],
    ['streets', /Level 3: Downtown Streets\.\*\* A ([\d.]+) km street race[^.]*?against (\w+) rivals/s],
    ['desert', /Level 4: Desert Run\.\*\* A ([\d.]+) km sprint[^.]*?against\s+(\w+) rivals/s],
  ];
  const WORDS = { three: 3, four: 4, five: 5, six: 6 };
  for (const [id, re] of claims) {
    const m = re.exec(README);
    assert.ok(m, `README describes ${id}`);
    const l = levelById(id);
    assert.equal(km(l).toFixed(1), m[1], `${id}: README says ${m[1]} km, the route is ${km(l).toFixed(2)} km`);
    if (m[2]) assert.equal(l.rivals.length, WORDS[m[2]], `${id}: README says ${m[2]} rivals`);
  }
  const loop = /endless ([\d.]+) km freeway loop/.exec(README);
  assert.ok(loop, 'README describes the cruise loop');
});
