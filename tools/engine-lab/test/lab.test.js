// Engine Sound Lab: the exhaust model (worklet.js) run offline under Node,
// the presets, the driver and the analysis. Run with
// `node --test tools/engine-lab/test/` (CI does).

import test from 'node:test';
import assert from 'node:assert/strict';

import { engine, render, send, load } from './harness.mjs';
import { PRESETS, ORDER, TWEAKS, presetParams, boostTarget } from '../presets.js';
import { Driver } from '../drive.js';
import { analyse } from '../measure.js';

const RATE = 48000;

// The driver's state for the engine, frame by frame, as the page sends it.
function driven(p, mode) {
  const d = new Driver();
  d.setPreset(p);
  d.start(mode);
  return () => {
    const s = d.step(1 / 60);
    return { rpm: s.rpm, throttle: s.throttle, boost: s.boost, speed: s.speed };
  };
}

const rms = (x, from = 0) => {
  let s = 0;
  for (let i = from; i < x.length; i++) s += x[i] * x[i];
  return Math.sqrt(s / (x.length - from));
};

// Clicks: samples whose second difference is far above the median of the
// 2048 samples around them.
function clicks(x, k) {
  const e = new Float32Array(x.length);
  for (let i = 2; i < x.length; i++) e[i] = Math.abs(x[i] - 2 * x[i - 1] + x[i - 2]);
  const out = [];
  const W = 2048;
  for (let s = 0; s + W <= x.length; s += W) {
    const w = Array.from(e.subarray(s, s + W)).sort((a, b) => a - b);
    const med = w[W >> 1] + 1e-7;
    for (let i = s; i < s + W; i++) if (e[i] > k * med) out.push(i / RATE);
  }
  return out;
}

test('every preset is listed once and fits the model', () => {
  assert.deepEqual([...ORDER].sort(), Object.keys(PRESETS).sort());
  const cars = new Set(['muscle', 'sports', 'super', 'rally']);
  for (const key of ORDER) {
    const p = PRESETS[key];
    assert.ok(p.name, key);
    assert.ok(p.idle > 300 && p.idle < p.redline && p.redline <= 12000, `${key} rpm range`);
    assert.ok(p.events.length >= 1 && p.events.length <= 16, `${key} cylinders`);
    for (const [bank] of p.events) assert.ok(bank === 0 || bank === 1, `${key} bank`);
    if (p.angles) assert.equal(p.angles.length, p.events.length, `${key} angles`);
    assert.ok((p.sections || []).length <= 6, `${key} muffler sections (the model runs six)`);
    for (const [f, q] of p.sections || []) assert.ok(f > 20 && f < 20000 && q > 0, `${key} section`);
    assert.ok(p.headers.every((h) => h > 0), `${key} headers`);
    if (p.gameCar) assert.ok(cars.has(p.gameCar), `${key} gameCar ${p.gameCar}`);
  }
});

test('the tweak sliders start inside their ranges for every preset', () => {
  for (const key of ORDER) {
    const p = presetParams(key);
    for (const [k, , lo, hi] of TWEAKS) {
      if (p[k] === undefined) continue;
      assert.ok(p[k] >= lo && p[k] <= hi, `${key}.${k} = ${p[k]} outside ${lo}..${hi}`);
    }
  }
});

test('presetParams is a deep copy', () => {
  const a = presetParams('crossV8');
  a.events[0][0] = 9;
  a.sections.push([1, 1, 1]);
  assert.equal(PRESETS.crossV8.events[0][0], 0);
  assert.notEqual(PRESETS.crossV8.sections.length, a.sections.length);
});

test('boost is zero without a turbo and grows with rpm and throttle', () => {
  assert.equal(boostTarget(PRESETS.crossV8, 6000, 1), 0);
  const p = PRESETS.i4turbo;
  assert.ok(p.turbo);
  let last = -1;
  for (let rpm = p.idle; rpm <= p.redline; rpm += 250) {
    const b = boostTarget(p, rpm, 1);
    assert.ok(b >= 0 && b <= 1 && b >= last);
    last = b;
  }
  assert.equal(last, 1);
  assert.ok(boostTarget(p, p.redline, 0.5) < boostTarget(p, p.redline, 1));
  assert.equal(boostTarget(p, p.redline, 0), 0);
});

test('the driver keeps rpm, throttle and gear believable in every mode', () => {
  for (const key of ORDER) {
    const p = presetParams(key);
    for (const mode of ['rev', 'drive']) {
      const d = new Driver();
      d.setPreset(p);
      d.start(mode);
      const ups = [];
      let maxGear = 0;
      for (let f = 0; f < 60 * 30; f++) {
        const s = d.step(1 / 60);
        assert.ok(s.rpm >= p.idle * 0.9 && s.rpm <= p.redline * 1.06, `${key} ${mode} rpm ${s.rpm}`);
        assert.ok(s.throttle >= 0 && s.throttle <= 1);
        assert.ok(s.boost >= 0 && s.boost <= 1);
        assert.ok(s.speed >= 0);
        maxGear = Math.max(maxGear, s.gear);
        ups.push(...s.events.filter((e) => e === 'up'));
      }
      if (mode === 'drive') {
        assert.equal(maxGear, 4, `${key} pulls through four gears`);
        assert.ok(ups.length >= 3, `${key} shifts up`);
      } else assert.equal(maxGear, 0);
    }
  }
});

test('manual mode moves rpm towards the slider at the preset rates', () => {
  const p = presetParams('v10');
  const d = new Driver();
  d.setPreset(p);
  d.start('manual');
  d.manualRpm = p.redline;
  const s = d.step(0.1);
  assert.ok(Math.abs(s.rpm - (p.idle + p.revUp * 0.1)) < 1e-6);
});

test('every preset sounds at idle, mid and full throttle, finite and under the ceiling', async () => {
  for (const key of ORDER) {
    const p = presetParams(key);
    for (const [rpm, throttle] of [[p.idle, 0], [p.redline * 0.55, 0.5], [p.redline * 0.95, 1]]) {
      const node = await engine(p, { rpm, throttle, boost: boostTarget(p, rpm, throttle) });
      const { L, R } = render(node, 1.0);
      for (const ch of [L, R]) {
        for (const v of ch) assert.ok(Number.isFinite(v) && Math.abs(v) <= 1, `${key} @${rpm}: ${v}`);
      }
      const level = rms(L, RATE / 2);
      assert.ok(level > 0.01, `${key} @${rpm} is near silent (${level})`);
    }
  }
});

// The crackle the owner heard: the old peak limiter had an instant attack,
// so every peak over the ceiling was flattened onto it (a short clip). With
// the look-ahead, a peak only touches the ceiling.
test('the limiter never holds the output flat on its ceiling', async () => {
  for (const key of ORDER) {
    const p = presetParams(key);
    for (const mode of ['drive', 'rev']) {
      const node = await engine(p, { rpm: p.idle, throttle: 0 });
      const { L, R } = render(node, 12, driven(p, mode));
      for (const ch of [L, R]) {
        let run = 0, worst = 0;
        for (const v of ch) {
          run = Math.abs(v) >= 0.8399 ? run + 1 : 0;
          worst = Math.max(worst, run);
        }
        assert.ok(worst <= 2, `${key} ${mode}: ${worst} samples in a row on the ceiling`);
      }
    }
  }
});

test('without pops, a drive cycle has no clicks', async () => {
  for (const key of ORDER) {
    const p = { ...presetParams(key), pops: 0, antiLag: 0 };
    const node = await engine(p, { rpm: p.idle, throttle: 0 });
    const { L } = render(node, 12, driven(p, 'drive'));
    const c = clicks(L.subarray(RATE), 60);
    assert.equal(c.length, 0, `${key}: clicks at ${c.slice(0, 5).map((t) => (t + 1).toFixed(3))}`);
  }
});

test('the same seed renders the same sound, another seed a different one', async () => {
  const p = presetParams('audiV8');
  const a = render(await engine(p, { rpm: 3000, throttle: 0.6 }, 7), 0.5).L;
  const b = render(await engine(p, { rpm: 3000, throttle: 0.6 }, 7), 0.5).L;
  const c = render(await engine(p, { rpm: 3000, throttle: 0.6 }, 8), 0.5).L;
  assert.deepEqual(a, b);
  assert.notDeepEqual(a, c);
});

test('stopping the engine fades it to silence, and starting brings it back', async () => {
  const p = presetParams('flatV8');
  const node = await engine(p, { rpm: 4000, throttle: 0.5 });
  render(node, 0.5);
  send(node, { type: 'run', on: false });
  const off = render(node, 1.0).L;
  assert.ok(rms(off, off.length - 4800) < 1e-3, 'silent after a second');
  send(node, { type: 'run', on: true });
  const on = render(node, 1.0).L;
  assert.ok(rms(on, on.length - 4800) > 0.01, 'sounding again');
});

test('changing parameters mid-run keeps the sound finite and going', async () => {
  const p = presetParams('v12');
  const node = await engine(p, { rpm: 5000, throttle: 0.7 });
  render(node, 0.3);
  for (const change of [{ headerScale: 2 }, { pipeLen: 0.5 }, { events: [[0], [1], [0], [1]] }, { sections: [] }, { tailLen: 1.5 }]) {
    send(node, { type: 'params', params: { ...p, ...change } });
    const { L } = render(node, 0.3);
    assert.ok(L.every(Number.isFinite), JSON.stringify(change));
    assert.ok(rms(L, L.length / 2) > 0.005, JSON.stringify(change));
  }
});

test('the phone speaker is mono and cuts the lows; headphones pass the sound through', async () => {
  const P = await load();
  const sine = (hz, phase = 0) => Float32Array.from({ length: 128 * 200 }, (_, i) => Math.sin((2 * Math.PI * hz * i) / RATE + phase) * 0.5);
  const run = (profile, L, R) => {
    const s = new P['speaker-sim']({ processorOptions: { profile } });
    const oL = new Float32Array(L.length), oR = new Float32Array(L.length);
    for (let i = 0; i < L.length; i += 128) {
      s.process([[L.subarray(i, i + 128), R.subarray(i, i + 128)]], [[oL.subarray(i, i + 128), oR.subarray(i, i + 128)]]);
    }
    return { oL, oR };
  };
  const a = sine(440), b = sine(440, 1);
  const hp = run('headphones', a, b);
  assert.deepEqual(hp.oL, a);
  assert.deepEqual(hp.oR, b);
  const ph = run('phone', a, b);
  assert.deepEqual(ph.oL, ph.oR);
  const low = run('phone', sine(80), sine(80));
  const mid = run('phone', sine(1000), sine(1000));
  assert.ok(rms(low.oL, 128 * 100) < rms(mid.oL, 128 * 100) / 10, 'phone cuts 80 Hz');
  // An unknown profile falls back to headphones.
  assert.deepEqual(run('gramophone', a, b).oL, a);
});

test('the analysis finds levels, bands and a repeating cycle', () => {
  const n = RATE * 2;
  const L = new Float32Array(n);
  for (let i = 0; i < n; i++) L[i] = 0.5 * Math.sin((2 * Math.PI * 100 * i) / RATE);
  const buf = { sampleRate: RATE, length: n, getChannelData: () => L };
  const a = analyse(buf, 0.5, 3000);
  assert.equal(a.finite, true);
  assert.equal(a.peak, 0.5);
  assert.ok(Math.abs(a.rmsDb - 10 * Math.log10(0.125)) < 0.1);
  assert.ok(a.share.low > 99, JSON.stringify(a.share));
  assert.ok(a.cycleCorr > 0.99);
  L[RATE] = NaN;
  assert.equal(analyse(buf, 0.5).finite, false);
});
