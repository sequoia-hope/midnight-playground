// Music Lab: the DSP, instruments, sequencer, grammars, engine and worklet,
// run offline under Node. Run with `npm run test:lab` (CI does).

import test from 'node:test';
import assert from 'node:assert/strict';

import { setRate, Osc, Ladder, ADSR, rng } from '../dsp.js';
import { BPATCH, KITS, Drums, kitVoice, makeInstrument, voiceTrack, patchName } from '../instruments.js';
import { Seq, compileTrack, DRUM_LANES } from '../seq.js';
import { GENRES } from '../gen.js';
import { Engine } from '../engine.js';
import { TRACKS, PATCHES } from '../../../src/game/audio/tracks.js';

const RATE = 48000;
setRate(RATE);

const rms = (x) => { let s = 0; for (const v of x) s += v * v; return Math.sqrt(s / x.length); };
const peak = (x) => { let p = 0; for (const v of x) p = Math.max(p, Math.abs(v)); return p; };
const finite = (x) => x.every(Number.isFinite);

// Power in a band, by direct DFT over a few bins (small n only).
function bandPower(x, lo, hi, rate = RATE) {
  const n = x.length;
  let p = 0;
  for (let f = lo; f < hi; f += rate / n) {
    let re = 0, im = 0;
    const w = (2 * Math.PI * f) / rate;
    for (let i = 0; i < n; i++) { re += x[i] * Math.cos(w * i); im -= x[i] * Math.sin(w * i); }
    p += re * re + im * im;
  }
  return p;
}

// Zero-crossing frequency of a stretch of a signal.
function zcHz(x, from, to, rate = RATE) {
  let c = 0;
  for (let i = from + 1; i < to; i++) if (x[i - 1] < 0 && x[i] >= 0) c++;
  return (c * rate) / (to - from);
}

function renderInst(lab, notes, secs) {
  const inst = makeInstrument(lab, 7);
  const n = Math.round(secs * RATE), L = new Float32Array(n), R = new Float32Array(n), o = [0, 0];
  let k = 0;
  for (let i = 0; i < n; i++) {
    while (k < notes.length && Math.round(notes[k].t * RATE) === i) { const e = notes[k++]; inst.trigger(e.midis, e.vel ?? 0.9, Math.round(e.dur * RATE), e.opt || {}); }
    o[0] = o[1] = 0;
    inst.run(o);
    L[i] = o[0]; R[i] = o[1];
  }
  return { L, R };
}

function renderEngine(T, { secs = 4, bar = 0, kit, energy = 1, seed = 5, solo = null } = {}) {
  const e = new Engine({ seed, rate: RATE });
  e.setTrack(T, { kit });
  e.setEnergy(energy);
  if (solo) e.seq.solo = solo;
  e.play(bar);
  const n = Math.round(secs * RATE / 128) * 128, L = new Float32Array(n), R = new Float32Array(n);
  for (let i = 0; i < n; i += 128) e.process(L.subarray(i, i + 128), R.subarray(i, i + 128), 128);
  return { L, R, e };
}

// ── DSP ──────────────────────────────────────────────────────────
test('the PolyBLEP saw is mostly free of aliasing', () => {
  // A 3.1 kHz saw: its harmonics above Nyquist fold back between the real ones.
  const o = new Osc(), n = 4800, x = new Float32Array(n);
  for (let i = 0; i < n; i++) x[i] = o.run(3100);
  const real = bandPower(x, 3000, 3200), alias = bandPower(x, 4000, 5900);
  assert.ok(10 * Math.log10(real / alias) > 20, `harmonic over alias ${10 * Math.log10(real / alias)} dB`);
});

test('the ladder stays bounded at full resonance under fast sweeps', () => {
  const f = new Ladder(), r = rng(1);
  let p = 0;
  for (let i = 0; i < RATE; i++) {
    f.set(100 + 8000 * (0.5 + 0.5 * Math.sin(i / 50)), 1.05);
    const y = f.run(r() * 2 - 1, 3);
    assert.ok(Number.isFinite(y));
    p = Math.max(p, Math.abs(y));
  }
  assert.ok(p < 4, `peak ${p}`);
});

test('ADSR attacks, sustains and releases to silence', () => {
  const e = new ADSR(0.01, 0.1, 0.5, 0.1);
  e.on();
  let v = 0;
  for (let i = 0; i < 0.01 * RATE + 10; i++) v = e.run();
  assert.ok(v > 0.9);
  for (let i = 0; i < 0.5 * RATE; i++) v = e.run();
  assert.ok(Math.abs(v - 0.5) < 0.01, `sustain ${v}`);
  e.off();
  for (let i = 0; i < 0.5 * RATE; i++) e.run();
  assert.ok(e.done);
});

// ── Instruments ──────────────────────────────────────────────────
test('every patch plays a finite, audible, bounded note and then stops', () => {
  for (const [name, lab] of Object.entries(BPATCH)) {
    const { L, R } = renderInst(lab, [{ t: 0.01, midis: [lab.kind === 'tb303' || name.includes('ass') ? 45 : 60], dur: 0.4 }], 3);
    assert.ok(finite(L) && finite(R), name + ' finite');
    const head = L.subarray(0, 0.5 * RATE);
    assert.ok(rms(head) > 1e-3, `${name} audible (${rms(head)})`);
    assert.ok(peak(L) < 1.5, `${name} peak ${peak(L)}`);
    assert.ok(rms(L.subarray(2.6 * RATE)) < 1e-3, `${name} released`);
  }
});

test('the 303 slides: no new envelope, and the pitch glides', () => {
  const lab = { ...BPATCH.acid, res: 0, env: 0, drive: 0 };
  // A2 for one step, sliding into A3.
  const step = 0.12;
  const { L } = renderInst(lab, [{ t: 0, midis: [45], dur: step * 1.05 }, { t: step, midis: [57], dur: step, opt: { glideFrom: 45 } }], 0.5);
  const before = zcHz(L, 0.03 * RATE, step * RATE);
  const late = zcHz(L, (step + 0.07) * RATE, (step + 0.115) * RATE);
  assert.ok(Math.abs(before - 110) < 6, `before ${before}`);
  assert.ok(Math.abs(late - 220) < 10, `after ${late}`);
});

test('kits: every lane sounds, decays, and kicks sit low and hats high', () => {
  for (const [kit, lanes] of Object.entries(KITS)) {
    for (const lane of Object.keys(lanes)) {
      const d = new Drums(1);
      const h = d.hit(lane, kitVoice(kit, lane), 1, 1, lane === 'revCrash' ? RATE : 0);
      const x = new Float32Array(5 * RATE);
      let i = 0;
      while (i < x.length) { let y = 0; d.run((_, v) => { y += v; }); x[i++] = y; }
      assert.ok(finite(x), `${kit} ${lane} finite`);
      assert.ok(peak(x) > 0.02 && peak(x) < 3, `${kit} ${lane} peak ${peak(x)}`);
      assert.ok(h.done, `${kit} ${lane} done`);
    }
    const one = (lane) => { const d = new Drums(1); d.hit(lane, kitVoice(kit, lane), 1); const x = new Float32Array(4096); for (let i = 0; i < x.length; i++) { let y = 0; d.run((_, v) => { y += v; }); x[i] = y; } return x; };
    const k = one('kick'), h = one('hat');
    assert.ok(bandPower(k, 30, 120) > 10 * bandPower(k, 2000, 4000), `${kit} kick is low`);
    assert.ok(bandPower(h, 6000, 12000) > 10 * bandPower(h, 100, 1000), `${kit} hat is high`);
  }
});

test('the game\'s sample names tweak the lab voices', () => {
  assert.ok(kitVoice('tr808', 'kick', 'kickBoom').decay > KITS.tr808.kick.decay);
  assert.equal(kitVoice('tr909', 'snare', 'snareGated').gate, 0.2);
  assert.equal(kitVoice('tr909', 'hat', 'nope'), KITS.tr909.hat);
});

test('every game part finds its patch, and B has one for it', () => {
  for (const T of TRACKS) {
    const V = voiceTrack(T, PATCHES);
    for (const [name, p] of Object.entries(V.parts)) {
      assert.ok(BPATCH[p.lab.name], `${T.id}.${name}: ${p.lab.name}`);
      assert.equal(patchName(p.inst, PATCHES), p.lab.name);
    }
  }
  // Copies with an override (`{ ...P.flute, oct: 1 }`) keep the override.
  const sea = voiceTrack(TRACKS.find((t) => t.id === 'seabright'), PATCHES);
  assert.equal(sea.parts.lead.lab.name, 'flute');
  assert.equal(sea.parts.lead.lab.oct, 1);
});

// ── Sequencer ────────────────────────────────────────────────────
test('the sequencer plays the game\'s patterns as written', () => {
  const T = TRACKS.find((t) => t.id === 'interstate');
  const s = new Seq(voiceTrack(T, PATCHES));
  s.seekBar(16); // 'full': four-on-the-floor, claps on 2 and 4
  const bar = [];
  for (let i = 0; i < 16; i++) bar.push(s.step());
  const steps = (lane) => bar.map((ev, i) => (ev.some((e) => e.k === 'drum' && e.lane === lane) ? i : -1)).filter((i) => i >= 0);
  assert.deepEqual(steps('kick'), [0, 4, 8, 12]);
  assert.deepEqual(steps('clap'), [4, 12]);
  assert.ok(bar[0].some((e) => e.k === 'section'));
  // Bass 'a': '..r...r...r...r.' on Gm7 from G2.
  const bass = bar.flatMap((ev) => ev.filter((e) => e.k === 'note' && e.part === 'bass'));
  assert.equal(bass.length, 4);
  assert.equal(bass[0].midis[0], 43);
});

test('swing delays the off 16ths; the song loops with an end event', () => {
  const T = voiceTrack(TRACKS.find((t) => t.id === 'seabright'), PATCHES);
  const s = new Seq(T);
  s.seekBar(8);
  const a = s.step(), b = s.step();
  assert.ok(a.filter((e) => e.k === 'drum').every((e) => e.dt === 0));
  assert.ok(b.filter((e) => e.k === 'drum').every((e) => Math.abs(e.dt - 0.1 * s.stepDur) < 1e-12));
  s.seekBar(s.bars - 1);
  let end = false;
  for (let i = 0; i < 16; i++) if (s.step().some((e) => e.k === 'end')) end = true;
  assert.ok(end);
  assert.equal(s.barIndex, 0);
});

test('mute, solo and energy layers drop what they should', () => {
  const T = GENRES.house(3);
  const count = (setup) => {
    const s = new Seq(T);
    setup(s);
    s.seekBar(24); // 'g1': the full groove
    const lanes = new Set();
    for (let i = 0; i < 64; i++) for (const e of s.step()) if (e.k === 'drum') lanes.add(e.lane); else if (e.k === 'note') lanes.add(e.part);
    return lanes;
  };
  const all = count(() => {});
  assert.ok(all.has('kick') && all.has('shaker') && all.has('bass'));
  const low = count((s) => { s.energy = 0.1; });
  assert.ok(low.has('kick') && !low.has('shaker') && !low.has('hat'));
  const solo = count((s) => { s.solo = 'drums'; });
  assert.ok(solo.has('kick') && !solo.has('bass'));
  const muted = count((s) => { s.mute = new Set(['kick', 'bass']); });
  assert.ok(!muted.has('kick') && !muted.has('bass') && muted.has('clap'));
  assert.ok(DRUM_LANES.includes('revCrash'));
});

// ── Grammars ─────────────────────────────────────────────────────
test('the grammars are seeded: same seed, same track; new seed, new track', () => {
  for (const g of Object.values(GENRES)) {
    assert.deepEqual(g(42), g(42));
    assert.notDeepEqual(g(42), g(43));
    for (let seed = 1; seed <= 25; seed++) {
      const T = g(seed);
      compileTrack(T);
      for (const s of T.sections) {
        assert.ok(T.drums[s.drums], `${T.id} drums ${s.drums}`);
        for (const [part, pat] of Object.entries(s.p || {})) assert.ok(T.parts[part].pat[pat], `${T.id} ${part}.${pat}`);
        for (const target of Object.keys(s.auto || {})) assert.ok(T.parts[target.split('.')[0]], target);
      }
    }
  }
});

test('déjà vu: 1 repeats every block, 0 keeps changing', () => {
  const locked = GENRES.techno(7, { dejavu: 1 }), free = GENRES.techno(7, { dejavu: 0 });
  const acid = (T) => Object.values(T.parts.acid.pat);
  assert.equal(new Set(acid(locked)).size, 1);
  assert.ok(new Set(acid(free)).size >= 6);
});

// ── Engine ───────────────────────────────────────────────────────
test('every song renders finite, audible and under the ceiling, the same every time', () => {
  const songs = [...TRACKS.map((T) => voiceTrack(T, PATCHES)), GENRES.house(1), GENRES.techno(2)];
  for (const T of songs) {
    const bar = T.sections[0].bars;
    const a = renderEngine(T, { secs: 3, bar });
    assert.ok(finite(a.L) && finite(a.R), T.id);
    assert.ok(rms(a.L) > 0.01, `${T.id} rms ${rms(a.L)}`);
    assert.ok(peak(a.L) <= 0.951 && peak(a.R) <= 0.951, `${T.id} peak`);
    if (T === songs[0] || T === songs.at(-1)) {
      const b = renderEngine(T, { secs: 3, bar });
      assert.deepEqual(a.L, b.L, `${T.id} deterministic`);
    }
  }
});

test('energy closes the mix down', () => {
  const T = GENRES.house(5);
  const full = renderEngine(T, { secs: 3, bar: 48 }).L, low = renderEngine(T, { secs: 3, bar: 48, energy: 0.15 }).L;
  const hi = (x) => bandPower(x.subarray(RATE, RATE + 8192), 3000, 8000);
  assert.ok(hi(low) < hi(full) / 30, `high band ${10 * Math.log10(hi(full) / hi(low))} dB down`);
});

test('section automation moves the patch', () => {
  const T = GENRES.techno(4);
  const sec = T.sections.findIndex((s) => s.auto?.['acid.cutoff']);
  let bar = 0;
  for (let i = 0; i < sec; i++) bar += T.sections[i].bars;
  const { e } = renderEngine(T, { secs: 4, bar });
  const [from, to] = T.sections[sec].auto['acid.cutoff'];
  const c = e.parts.acid.lab.cutoff;
  assert.ok(c !== from && (c - from) * (to - from) > 0, `cutoff ${c} on the way from ${from} to ${to}`);
});

test('the worklet loads and plays under a minimal AudioWorklet scope', async () => {
  const procs = {};
  globalThis.sampleRate = RATE;
  globalThis.AudioWorkletProcessor = class { constructor() { this.port = { onmessage: null, posted: [], postMessage(m) { this.posted.push(m); } }; } };
  globalThis.registerProcessor = (name, cls) => { procs[name] = cls; };
  await import('../worklet.js');
  const T = voiceTrack(TRACKS[0], PATCHES);
  const p = new procs['music-lab']({ processorOptions: { seed: 1, track: T, kit: 'tr808', play: 8 } });
  const L = new Float32Array(128), R = new Float32Array(128);
  let s = 0;
  for (let i = 0; i < 400; i++) { p.process([], [[L, R]]); s += rms(L); }
  assert.ok(s / 400 > 0.005);
  assert.ok(p.port.posted.some((m) => m.type === 'pos' && m.pos.barIndex >= 8));
  p.port.onmessage({ data: { type: 'stop' } });
  p.port.onmessage({ data: { type: 'audition', lab: BPATCH.acid } });
  p.port.onmessage({ data: { type: 'on', midi: 45, vel: 1 } });
  p.process([], [[L, R]]);
  assert.ok(peak(L) > 0);
});
