// Music Lab: the instruments (docs/vision/sound.md 3.1, milestone S3).
//
//   tb303  the acid bassline: one saw/square oscillator into an 18 dB
//          diode-leaning ladder, a filter envelope whose decay the knob sets,
//          accent (shorter, harder envelope; louder; and the accent
//          capacitor that builds up over consecutive accents: the "wow"),
//          slide (no retrigger, fixed-time portamento).
//   juno   a Juno-style polysynth: DCO saw + PWM pulse + square sub + noise
//          per voice, a high-pass, a saturating 4-pole ladder, ADSRs, then
//          the BBD chorus (I, II, I+II) on the whole instrument. `unison`
//          up to 7 detuned oscillators per voice (the JP-8000 supersaw,
//          with its spread: the outer pairs further apart than the inner),
//          and `vowel` a formant bank after the voices (a choir).
//   mono   a Moog-style monosynth: up to three drifting oscillators, ladder
//          with drive, filter and amp ADSRs, glide, vibrato, bend.
//   fm     a DX-style 4-operator FM voice with a few algorithms (EP, bell,
//          stack, organ), per-operator envelopes, velocity to index, feedback.
//   drums  TR-808 and TR-909 kits synthesised per hit (kick, snare, clap,
//          hats, ride, crash, rim, cowbell, toms, shaker, snap, boom, swell).
//
// Every voice adds into a stereo pair; instruments own their voices and gate
// timing (a note knows its length in samples), so the sequencer only fires.

import { SR, TAU, sin1, clamp, mtof, tanh, rng, OSC_SAW, OSC_PULSE, OSC_TRI, OSC_SUB, Osc, Sine, Drift, OnePole, Ladder, SVF, ADSR, Decay, Chorus } from './dsp.js';

const cents = (c) => Math.pow(2, c / 1200);

// ── TB-303 ───────────────────────────────────────────────────────
export class TB303 {
  constructor(patch, seed = 1) {
    this.p = patch;
    this.osc = new Osc();
    this.f = new Ladder();
    this.fenv = new Decay(0.3);
    this.acc = 0; // the accent capacitor
    this.amp = 0; this.gate = 0; this.left = 0;
    this.hz = 110; this.target = 110;
    this.drift = new Drift(rng(seed), 2);
    this.hpIn = new OnePole(30); this.dc = new OnePole(20);
    this.accent = 0; this.k = 0; this.hzc = null;
  }
  trigger(midis, vel, durS, opt = {}) {
    const p = this.p;
    const m = midis[0] + 12 * (p.oct ?? 0);
    this.target = mtof(m);
    // A slide carries the gate over: no new envelope, the pitch glides.
    const sliding = opt.glideFrom != null && this.gate;
    if (!sliding) {
      this.hz = this.target;
      this.accent = opt.accent ? 1 : 0;
      this.fenv.set(this.accent ? 0.2 : p.decay ?? 0.4);
      this.fenv.hit(1);
      this.ampTarget = 1;
    }
    this.gate = 1;
    this.left = Math.max(1, durS | 0);
    this.vel = vel;
  }
  run(out) {
    const p = this.p;
    if (!this.gate && this.amp < 1e-5) return;
    if (this.left > 0 && --this.left === 0) this.gate = 0;
    const env = this.fenv.run();
    const accAmt = (p.accent ?? 0.6) * this.accent;
    // Control rate (every 16 samples): pitch, the accent capacitor, cutoff.
    if ((this.k = (this.k + 1) & 15) === 0 || this.hzc == null) {
      // Fixed-time slide (about 60 ms on the hardware), exponential.
      this.hz += (this.target - this.hz) * (1 - Math.exp(-16 / ((p.glide ?? 0.06) * SR / 3)));
      this.hzc = this.hz * cents(this.drift.run(16));
      // The accent capacitor charges from the accented envelope and leaks slowly.
      this.acc += (env * accAmt - this.acc) * (env * accAmt > this.acc ? 0.014 : 0.002);
      const cut = (p.cutoff ?? 400) * Math.pow(2, (p.env ?? 0.6) * 5.5 * env + accAmt * 1.2 * env + this.acc * 3);
      this.f.set(Math.min(cut, 16000), clamp((p.res ?? 0.7) * 0.97 + this.acc * 0.1, 0, 1));
    }
    const hz = this.hzc;
    const x = p.wave === 'square' ? (this.osc.run(hz, 0.5, OSC_PULSE), this.osc.pulse) : this.osc.run(hz);
    const y = this.f.run(this.hpIn.hp(x) * 0.7, 1 + (p.drive ?? 0.6), 1);
    // VCA: fast attack, slow droop while the gate is held, 8 ms release.
    const tgt = this.gate ? 1 + accAmt * 0.7 : 0;
    const k = this.gate ? (this.amp < tgt ? 0.02 : 0.00003) : 1 - Math.exp(-1 / (0.008 * SR / 4));
    this.amp += (tgt - this.amp) * k;
    let s = y * this.amp * (p.gain ?? 0.3) * (0.75 + 0.25 * this.vel);
    s = tanh(s * 1.4) / 1.4;
    s -= this.dc.lp(s);
    out[0] += s; out[1] += s;
  }
  get active() { return this.gate || this.amp > 1e-5; }
}

// ── Juno-style poly voice ────────────────────────────────────────
class JunoVoice {
  constructor(seed) {
    this.r = rng(seed);
    this.osc = Array.from({ length: 7 }, () => new Osc(this.r()));
    this.drift = this.osc.map(() => new Drift(this.r, 3.5));
    this.f = new Ladder(); this.hp = new OnePole(20);
    this.env = new ADSR(); this.fenv = new ADSR();
    this.left = 0; this.age = 0; this.midi = -1; this.on = false;
    this.cutVar = 1 + (this.r() * 2 - 1) * 0.06; // per-voice component spread
    this.inc = new Float64Array(7); this.k = 0; this.pw = 0.5;
  }
  start(p, midi, vel, durS, glideFrom, t) {
    this.midi = midi; this.vel = vel; this.left = Math.max(1, durS | 0); this.age = t; this.on = true;
    this.hz = mtof(midi + 12 * (p.oct ?? 0));
    this.hz0 = glideFrom != null ? mtof(glideFrom + 12 * (p.oct ?? 0)) : p.bend ? this.hz * Math.pow(2, -p.bend / 12) : this.hz;
    this.glideK = 1 - Math.exp(-1 / (((glideFrom != null ? p.glide : p.bendT) ?? 0.06) * SR / 3));
    this.cur = this.hz0;
    this.env.setAll(p.a ?? 0.005, p.d ?? 0.3, p.s ?? 0.7, p.r ?? 0.2); this.env.on();
    this.fenv.setAll(p.fa ?? p.a ?? 0.005, p.fd ?? p.d ?? 0.3, p.fs ?? p.s ?? 0.7, p.fr ?? p.r ?? 0.2); this.fenv.on();
    this.hp.setHz(p.hpf ?? 20);
    this.vibT = 0; this.k = 0;
  }
  get done() { return !this.on; }
  run(p, lfo, vib) {
    if (this.left > 0 && --this.left === 0) { this.env.off(); this.fenv.off(); }
    const a = this.env.run();
    if (this.env.done) { this.on = false; return 0; }
    const n = Math.min(7, p.unison ?? 1);
    const fe = this.fenv.run();
    if ((this.k = (this.k + 1) & 15) === 1) {
      this.cur += (this.hz - this.cur) * (1 - Math.pow(1 - this.glideK, 16));
      this.vibT += 16 / SR;
      const vd = p.vibDelay ?? 0.3;
      const vibAmt = vib && this.vibT > vd ? Math.min(1, (this.vibT - vd) / 0.25) * vib : 0;
      const det = p.detune ?? 0;
      for (let i = 0; i < n; i++) {
        // Spread over ±detune/2, the outer oscillators further apart.
        const u = n > 1 ? (i / (n - 1)) * 2 - 1 : 0;
        const c = Math.sign(u) * Math.pow(Math.abs(u), 1.3) * det * 0.5 + this.drift[i].run(16) + vibAmt;
        this.inc[i] = this.cur * cents(c);
      }
      const kt = Math.pow(2, ((this.midi - 60) / 12) * (p.keytrack ?? 0.5));
      const cut = (p.cutoff ?? 2000) * this.cutVar * kt * Math.pow(2, (p.fenv ?? 0) * fe * (0.5 + 0.5 * this.vel));
      this.f.set(Math.min(cut, 18000), p.res ?? 0.1);
      this.pw = clamp((p.pw ?? 0.5) + (p.pwm ?? 0) * lfo * 0.45, 0.05, 0.95);
    }
    let x = 0;
    const pw = this.pw;
    const m = ((p.saw ?? 1) ? OSC_SAW : 0) | (p.pulse ? OSC_PULSE : 0);
    for (let i = 0; i < n; i++) {
      const o = this.osc[i];
      o.run(this.inc[i], pw, i === 0 && p.sub ? m | OSC_SUB : m);
      x += o.saw * (p.saw ?? 1) + o.pulse * (p.pulse ?? 0) + (i === 0 ? o.sub * (p.sub ?? 0) : 0);
    }
    x /= Math.sqrt(n);
    if (p.noise) x += (this.r() * 2 - 1) * p.noise;
    x = this.hp.hp(x);
    const y = this.f.run(x * 0.5, 1 + (p.drive ?? 0.2));
    return y * a * this.vel;
  }
}

// ── Moog-style mono voice ────────────────────────────────────────
class MonoVoice {
  constructor(seed) {
    this.r = rng(seed);
    this.osc = [new Osc(this.r()), new Osc(this.r()), new Osc(this.r())];
    this.sine = new Sine();
    this.drift = this.osc.map(() => new Drift(this.r, 2.5));
    this.f = new Ladder(); this.env = new ADSR(); this.fenv = new ADSR();
    this.on = false; this.left = 0; this.age = 0; this.cur = 0; this.hz = 0; this.vibT = 0; this.breath = new OnePole(2500);
    this.inc = new Float64Array(3); this.k = 0;
  }
  start(p, midi, vel, durS, glideFrom) {
    const legato = glideFrom != null && this.on && !this.env.done;
    this.midi = midi; this.vel = vel; this.left = Math.max(1, durS | 0); this.on = true;
    this.hz = mtof(midi + 12 * (p.oct ?? 0));
    if (!legato) {
      this.cur = p.bend ? this.hz * Math.pow(2, -p.bend / 12) : this.hz;
      this.glideK = 1 - Math.exp(-1 / ((p.bendT ?? 0.06) * SR / 3));
      this.env.setAll(p.a ?? 0.005, p.d ?? 0.3, p.s ?? 0.8, p.r ?? 0.15); this.env.on();
      this.fenv.setAll(p.fa ?? 0.003, p.fd ?? 0.3, p.fs ?? 0.3, p.fr ?? p.r ?? 0.15); this.fenv.on();
      this.vibT = 0; this.k = 0;
    } else {
      this.glideK = 1 - Math.exp(-1 / ((p.glide || 0.06) * SR / 3));
      if (this.env.st === 4) { this.env.st = 2; this.fenv.st = 2; }
    }
  }
  get done() { return !this.on; }
  run(p) {
    if (this.left > 0 && --this.left === 0) { this.env.off(); this.fenv.off(); }
    const a = this.env.run();
    if (this.env.done) { this.on = false; return 0; }
    const oscs = p.osc ?? [{ w: 'saw' }];
    const fe = this.fenv.run();
    if ((this.k = (this.k + 1) & 15) === 1) {
      this.cur += (this.hz - this.cur) * (1 - Math.pow(1 - this.glideK, 16));
      this.vibT += 16 / SR;
      const vd = p.vibDelay ?? 0.3;
      const vib = p.vib && this.vibT > vd ? Math.min(1, (this.vibT - vd) / 0.3) * p.vib * Math.sin(TAU * (p.vibRate ?? 5.5) * this.vibT) : 0;
      for (let i = 0; i < oscs.length; i++) this.inc[i] = this.cur * cents((oscs[i].det ?? 0) + this.drift[i].run(16) + vib) * Math.pow(2, oscs[i].oct ?? 0);
      const kt = Math.pow(2, ((this.midi - 60) / 12) * (p.keytrack ?? 0.4));
      const cut = (p.cutoff ?? 1500) * kt * Math.pow(2, (p.fenv ?? 1) * fe * (0.6 + 0.4 * this.vel));
      this.f.set(Math.min(cut, 18000), p.res ?? 0.15);
    }
    let x = 0;
    for (let i = 0; i < oscs.length; i++) {
      const O = oscs[i], o = this.osc[i];
      const hz = this.inc[i];
      if (O.w === 'sine') { x += this.sine.run(hz) * (O.lvl ?? 1); continue; }
      o.run(hz, O.pw ?? 0.5, (O.w === 'pulse' ? OSC_PULSE : O.w === 'tri' ? OSC_TRI : OSC_SAW) | (i === 0 && p.sub ? OSC_SUB : 0));
      x += (O.w === 'pulse' ? o.pulse : O.w === 'tri' ? o.tri : o.saw) * (O.lvl ?? 1);
    }
    if (p.sub) x += this.osc[0].sub * p.sub;
    if (p.noise) x += this.breath.lp(this.r() * 2 - 1) * p.noise * (0.4 + a);
    return this.f.run(x * 0.45, 1 + (p.drive ?? 0.5)) * a * this.vel;
  }
}

// ── DX-style 4-op FM voice ───────────────────────────────────────
// Algorithms: mods[i] lists the operators that modulate operator i (ops are
// 0-based here; the DX names are 1-based), car the carriers that are heard.
// Operators run from 3 down to 0, so a modulator's sample is ready for the
// operator below it.
const ALGOS = {
  ep: { mods: [[1], [], [3], []], car: [0, 2] }, // 2→1, 4→3
  pair: { mods: [[1], [], [], []], car: [0] }, // 2→1
  bell: { mods: [[1], [], [3], []], car: [0, 2] },
  stack: { mods: [[1], [2], [3], []], car: [0] }, // 4→3→2→1
  organ: { mods: [[], [], [], []], car: [0, 1, 2, 3] },
};
class FMVoice {
  constructor(seed) {
    this.r = rng(seed);
    this.ph = new Float64Array(4); this.out = new Float64Array(4);
    this.env = [new ADSR(), new ADSR(), new ADSR(), new ADSR()];
    this.on = false; this.left = 0; this.fb = 0; this.age = 0; this.k = 0; this.base = 0; this.drift = new Drift(this.r, 1.5);
  }
  start(p, midi, vel, durS, glideFrom) {
    this.midi = midi; this.vel = vel; this.left = Math.max(1, durS | 0); this.on = true;
    this.hz = mtof(midi + 12 * (p.oct ?? 0));
    this.cur = glideFrom != null ? mtof(glideFrom + 12 * (p.oct ?? 0)) : p.bend ? this.hz * Math.pow(2, -p.bend / 12) : this.hz;
    this.glideK = 1 - Math.exp(-1 / (((glideFrom != null ? p.glide : p.bendT) ?? 0.06) * SR / 3));
    const ops = p.ops;
    this.k = 0;
    for (let i = 0; i < 4; i++) {
      const o = ops[i];
      if (!o) continue;
      // Higher notes decay faster (rate scaling).
      const rs = Math.pow(2, -((midi - 60) / 12) * (o.rs ?? 0.3));
      this.env[i].setAll(o.a ?? 0.002, (o.d ?? 0.5) * rs, o.s ?? 0.2, o.r ?? p.r ?? 0.3);
      this.env[i].on();
      this.ph[i] = 0;
    }
  }
  get done() { return !this.on; }
  run(p) {
    if (this.left > 0 && --this.left === 0) for (const e of this.env) e.off();
    const ops = p.ops, al = ALGOS[p.algo ?? 'ep'];
    if ((this.k = (this.k + 1) & 15) === 1) {
      this.cur += (this.hz - this.cur) * (1 - Math.pow(1 - this.glideK, 16));
      this.base = this.cur * cents(this.drift.run(16));
    }
    const base = this.base;
    let y = 0, live = false;
    for (let i = 3; i >= 0; i--) {
      const o = ops[i];
      if (!o) { this.out[i] = 0; continue; }
      const e = this.env[i].run();
      if (!this.env[i].done) live = true;
      let pm = 0;
      for (const m of al.mods[i]) pm += this.out[m];
      if (i === 3) pm += this.fb * (p.fbk ?? 0);
      this.ph[i] += (base * (o.r ?? 1) * cents(o.det ?? 0) + (o.fix ?? 0)) / SR;
      if (this.ph[i] > 1) this.ph[i] -= Math.floor(this.ph[i]);
      const s = sin1(this.ph[i] + pm / TAU);
      const isCar = al.car.includes(i);
      const lvl = isCar ? o.l ?? 1 : (o.l ?? 1) * (1 - (o.v ?? 0.6) + (o.v ?? 0.6) * this.vel);
      this.out[i] = s * e * lvl;
      if (i === 3) this.fb = this.out[i];
      if (isCar) y += this.out[i];
    }
    if (!live) { this.on = false; return 0; }
    return (y / Math.sqrt(al.car.length)) * this.vel;
  }
}

// Vowel formants (a tenor's first three), for the choir: centre frequencies
// and relative levels of three band-passes on the summed voices.
const VOWELS = {
  a: [[650, 1], [1080, 0.5], [2650, 0.25]],
  e: [[400, 1], [1700, 0.4], [2600, 0.25]],
  i: [[290, 1], [1870, 0.3], [2800, 0.25]],
  o: [[400, 1], [800, 0.6], [2600, 0.15]],
  u: [[350, 1], [600, 0.5], [2700, 0.1]],
};

// A polyphonic instrument around any of the poly voices, with an LFO, the
// chorus and voice stealing.
export class Poly {
  constructor(kind, patch, seed = 1, nVoices = 8) {
    this.p = patch; this.kind = kind;
    const V = kind === 'fm' ? FMVoice : kind === 'mono' ? MonoVoice : JunoVoice;
    this.voices = Array.from({ length: kind === 'mono' ? 1 : nVoices }, (_, i) => new V(seed * 101 + i * 7919));
    this.ch = new Chorus(); this.ch.set(patch.chorus ?? 0);
    this.lfoPh = 0; this.vibPh = 0; this.t = 0;
    this.out = [0, 0];
    this.formant = [new SVF(), new SVF(), new SVF()];
  }
  setPatch(p) { this.p = p; this.ch.set(p.chorus ?? 0); }
  // The vowel bank: the LFO drifts the formants a little, as a mouth does.
  _vowel(x, lfo) {
    const V = VOWELS[this.p.vowel];
    if (!V) return x;
    let y = 0;
    for (let i = 0; i < 3; i++) {
      const f = this.formant[i];
      if ((this.t & 7) === i || f.hz < 0) f.set(V[i][0] * (1 + lfo * 0.04), this.p.vowelQ ?? 9);
      f.run(x);
      y += f.bp * V[i][1];
    }
    const mix = this.p.vowelMix ?? 0.8;
    return x * (1 - mix) + y * mix * 2.2;
  }
  trigger(midis, vel, durS, opt = {}) {
    this.t++;
    if (this.kind === 'mono') { this.voices[0].start(this.p, midis[midis.length - 1], vel, durS, opt.glideFrom); return; }
    const gain = 1 / Math.sqrt(midis.length);
    for (const m of midis) {
      let v = this.voices.find((x) => x.done) || this.voices.reduce((a, b) => (a.age <= b.age ? a : b));
      v.start(this.p, m, vel * gain, durS, opt.glideFrom ?? null, this.t);
      v.age = this.t;
    }
  }
  get active() { return this.voices.some((v) => !v.done); }
  run(out) {
    const p = this.p;
    this.lfoPh += (p.lfoRate ?? 0.6) / SR; if (this.lfoPh > 1) this.lfoPh -= 1;
    const lfo = this.lfoPh < 0.5 ? this.lfoPh * 4 - 1 : 3 - this.lfoPh * 4;
    let x = 0;
    if (this.kind === 'juno') {
      this.vibPh += (p.vibRate ?? 5.5) / SR; if (this.vibPh > 1) this.vibPh -= 1;
      const vib = p.vib ? p.vib * Math.sin(TAU * this.vibPh) : 0;
      for (const v of this.voices) if (!v.done) x += v.run(p, lfo, vib);
      if (p.vowel) { this.t++; x = this._vowel(x, lfo); }
    } else {
      for (const v of this.voices) if (!v.done) x += v.run(p);
    }
    this.ch.run(x * (p.gain ?? 0.2), out, 1);
  }
}

// ── Drums ────────────────────────────────────────────────────────
// 808 metallic oscillator frequencies (six squares), shared by hats, cymbal
// and (two of them) the cowbell.
const METAL = [205.3, 304.4, 369.6, 522.7, 540, 800];

class Hit {
  constructor(type, k, vel, r, rate = 1, durS = 0) {
    this.type = type; this.k = k; this.vel = vel; this.r = r; this.t = 0; this.done = false; this.rate = rate;
    this.amp = new Decay(k.decay ?? 0.3); this.amp.hit(1);
    this.pan = k.pan ?? 0;
    switch (type) {
      case 'kick808': case 'kick909': case 'tom': case 'boom':
        this.sine = new Sine(0.25); this.click = new SVF().set(k.clickHz ?? 3000, 0.7); this.nz = new Decay(0.004); this.nz.hit(1); break;
      case 'snare808': case 'snare909':
        this.s1 = new Sine(0.25); this.s2 = new Sine(0.1); this.ndec = new Decay(k.snappy ?? 0.15); this.ndec.hit(1);
        this.nlp = new SVF().set(k.nlp ?? 7000, 0.6); this.nhp = new SVF().set(k.nhp ?? 1200, 0.6); break;
      case 'clap':
        this.bp = new SVF().set(k.bp ?? 1100, 1.6); this.tail = new Decay(k.tail ?? 0.2); break;
      case 'metal': case 'cymbal': case 'cowbell':
        this.ph = new Float64Array(6).map(() => r()); this.bp = new SVF().set(k.bp ?? 10000, k.q ?? 0.9); this.hp = new SVF().set(k.hp ?? 7000, 0.7);
        if (type === 'cymbal') { this.lo = new SVF().set(k.bp2 ?? 4000, 1.2); this.amp2 = new Decay(k.decay2 ?? 0.15); this.amp2.hit(1); }
        if (k.attack) this.att = 0;
        break;
      case 'rim':
        this.a = new SVF().set(k.f1 ?? 455, 12); this.b = new SVF().set(k.f2 ?? 1667, 14); this.hp = new SVF().set(300, 0.7); break;
      case 'shaker': case 'snap':
        this.bp = new SVF().set(k.bp ?? 6000, k.q ?? 1.4); this.att = 0; break;
      case 'swell':
        this.len = Math.max(1, durS); this.bp = new SVF().set(8000, 0.8); this.hp = new SVF().set(2500, 0.7); break;
      default: break;
    }
    this.ex = this.k.gate ? (this.k.gate * SR) | 0 : 0;
  }
  run() {
    const k = this.k, t = this.t++ / SR, r = this.r;
    const noise = r() * 2 - 1;
    let y = 0;
    switch (this.type) {
      case 'kick808': case 'kick909': case 'tom': case 'boom': {
        // A pitch that starts high and falls quickly onto the tuned note.
        const f0 = (k.tune ?? 50) * this.rate;
        const sweep = k.pitch ?? 2;
        const hz = f0 * (1 + (sweep - 1) * Math.exp(-t / (k.pt ?? 0.012)));
        let s = this.sine.run(hz);
        if (this.type === 'kick909') s = tanh(s * 1.8) / 0.95; // the 909's shaped triangle
        y = s * this.amp.run();
        // Click: a short filtered noise burst (909) or a tiny pulse (808).
        const c = this.nz.run();
        if (k.click) { this.click.run(noise); y += this.click.bp * c * k.click * 3; }
        if (k.noise) y += noise * k.noise * this.amp.v;
        y = tanh(y * (k.drive ?? 1)) / Math.min(k.drive ?? 1, 1.5);
        break;
      }
      case 'snare808': case 'snare909': {
        const f = (k.tune ?? 180) * this.rate;
        const bend = this.type === 'snare909' ? 1 + 0.5 * Math.exp(-t / 0.01) : 1;
        const tone = this.s1.run(f * bend) * 0.65 + this.s2.run(f * 1.85 * bend) * 0.35;
        let nenv = this.ndec.run();
        if (this.ex) nenv = this.t < this.ex ? Math.max(nenv, 0.35) : nenv * Math.exp(-(this.t - this.ex) / (0.004 * SR));
        this.nhp.run(this.nlp.run(noise));
        y = tone * this.amp.run() * (1 - (k.tone ?? 0.5)) + this.nhp.hp * nenv * (k.tone ?? 0.5) * 2.2;
        break;
      }
      case 'clap': {
        // Three or four quick bursts, then the diffuse tail.
        const sp = k.spacing ?? 0.011, nb = k.bursts ?? 3;
        let e;
        if (t < sp * nb) { const u = (t % sp) / sp; e = Math.exp(-u * 6); } else e = this.tail.run() * 0.8;
        if (t < sp * nb) this.tail.hit(1);
        y = this.bp.run(noise) * e * 2.2;
        this.amp.v = e;
        break;
      }
      case 'metal': case 'cymbal': case 'cowbell': {
        const tune = (k.tune ?? 1) * this.rate;
        let s = 0;
        const freqs = this.type === 'cowbell' ? [540, 800] : METAL;
        for (let i = 0; i < freqs.length; i++) {
          this.ph[i] += (freqs[i] * tune) / SR; if (this.ph[i] > 1) this.ph[i] -= 1;
          s += this.ph[i] < 0.5 ? 1 : -1;
        }
        s /= freqs.length;
        if (k.noise) s = s * (1 - k.noise) + noise * k.noise;
        let e = this.amp.run();
        if (k.attack) { this.att = Math.min(1, this.att + 1 / (k.attack * SR)); e *= this.att; }
        this.bp.run(s);
        if (this.type === 'cowbell') { y = this.bp.bp * e * 3; break; }
        this.hp.run(this.bp.bp);
        y = this.hp.hp * e * 2.5;
        if (this.type === 'cymbal') y += this.lo.run(s) * this.amp2.run() * 0.6;
        break;
      }
      case 'rim': {
        const ex = this.t < 8 ? 1 : 0;
        this.a.run(ex); this.b.run(ex);
        this.hp.run(this.a.bp + this.b.bp);
        y = this.hp.hp * 4 * this.amp.run();
        break;
      }
      case 'shaker': case 'snap': {
        this.att = Math.min(1, this.att + 1 / ((k.attack ?? 0.008) * SR));
        this.bp.run(noise);
        y = (this.type === 'snap' ? this.bp.hp : this.bp.bp) * this.amp.run() * this.att * 1.6;
        break;
      }
      case 'swell': {
        // A reverse cymbal: noise that swells over its length, then stops dead.
        const u = this.t / this.len;
        if (u >= 1) { this.done = true; return 0; }
        this.bp.run(noise);
        this.hp.run(this.bp.bp + noise * 0.3);
        y = this.hp.hp * u * u * u * 1.4;
        this.amp.v = 1;
        break;
      }
      default: break;
    }
    if (this.amp.v < 1e-4 && this.t > 64) this.done = true;
    return y * this.vel * (k.lvl ?? 1);
  }
}

// The kits: per lane name, a voice type and its parameters.
export const KITS = {
  tr808: {
    kick: { t: 'kick808', tune: 49, pitch: 1.7, pt: 0.014, decay: 0.75, click: 0.25, clickHz: 2500, drive: 1.3, lvl: 1 },
    snare: { t: 'snare808', tune: 185, tone: 0.45, snappy: 0.13, decay: 0.11, nhp: 1800, nlp: 9000, lvl: 0.8 },
    clap: { t: 'clap', bp: 1050, tail: 0.24, bursts: 3, spacing: 0.012, lvl: 0.75 },
    hat: { t: 'metal', tune: 1, decay: 0.045, bp: 10500, hp: 7500, lvl: 0.45 },
    ohat: { t: 'metal', tune: 1, decay: 0.38, bp: 10000, hp: 7000, lvl: 0.35 },
    ride: { t: 'metal', tune: 1.3, decay: 0.9, bp: 8000, hp: 5200, noise: 0.15, lvl: 0.28 },
    crash: { t: 'cymbal', tune: 1.1, decay: 1.7, bp: 7500, hp: 4200, bp2: 3500, decay2: 0.25, noise: 0.35, lvl: 0.35 },
    revCrash: { t: 'swell', lvl: 0.32 },
    rim: { t: 'rim', f1: 455, f2: 1667, decay: 0.03, lvl: 0.5 },
    cowbell: { t: 'cowbell', bp: 800, q: 2.5, decay: 0.28, lvl: 0.35 },
    tomL: { t: 'tom', tune: 82, pitch: 1.25, pt: 0.05, decay: 0.45, noise: 0.03, lvl: 0.7, pan: -0.3 },
    tomM: { t: 'tom', tune: 118, pitch: 1.25, pt: 0.05, decay: 0.38, noise: 0.03, lvl: 0.65 },
    tomH: { t: 'tom', tune: 165, pitch: 1.25, pt: 0.05, decay: 0.32, noise: 0.03, lvl: 0.6, pan: 0.3 },
    shaker: { t: 'shaker', bp: 6500, decay: 0.06, attack: 0.006, lvl: 0.35 },
    snap: { t: 'snap', bp: 2200, q: 0.8, decay: 0.05, attack: 0.0005, lvl: 0.55 },
    boom: { t: 'boom', tune: 38, pitch: 2.2, pt: 0.05, decay: 1.6, drive: 1.6, lvl: 0.8 },
  },
  tr909: {
    kick: { t: 'kick909', tune: 54, pitch: 3.6, pt: 0.009, decay: 0.42, click: 0.7, clickHz: 4500, drive: 1.6, lvl: 1 },
    snare: { t: 'snare909', tune: 195, tone: 0.55, snappy: 0.17, decay: 0.09, nhp: 2200, nlp: 11000, lvl: 0.8 },
    clap: { t: 'clap', bp: 1250, tail: 0.18, bursts: 4, spacing: 0.009, lvl: 0.75 },
    hat: { t: 'metal', tune: 1.45, decay: 0.05, bp: 12000, hp: 8500, noise: 0.35, lvl: 0.45 },
    ohat: { t: 'metal', tune: 1.45, decay: 0.3, bp: 11000, hp: 8000, noise: 0.35, lvl: 0.35 },
    ride: { t: 'metal', tune: 1.9, decay: 1.1, bp: 9000, hp: 5500, noise: 0.25, lvl: 0.27 },
    crash: { t: 'cymbal', tune: 1.25, decay: 1.5, bp: 8500, hp: 4800, bp2: 4200, decay2: 0.2, noise: 0.45, lvl: 0.35 },
    revCrash: { t: 'swell', lvl: 0.32 },
    rim: { t: 'rim', f1: 500, f2: 1900, decay: 0.025, lvl: 0.5 },
    cowbell: { t: 'cowbell', bp: 800, q: 2.5, decay: 0.22, lvl: 0.35 },
    tomL: { t: 'tom', tune: 90, pitch: 1.5, pt: 0.03, decay: 0.35, noise: 0.08, lvl: 0.7, pan: -0.3 },
    tomM: { t: 'tom', tune: 130, pitch: 1.5, pt: 0.03, decay: 0.3, noise: 0.08, lvl: 0.65 },
    tomH: { t: 'tom', tune: 180, pitch: 1.5, pt: 0.03, decay: 0.26, noise: 0.08, lvl: 0.6, pan: 0.3 },
    shaker: { t: 'shaker', bp: 7500, decay: 0.05, attack: 0.005, lvl: 0.35 },
    snap: { t: 'snap', bp: 2600, q: 0.8, decay: 0.045, attack: 0.0005, lvl: 0.55 },
    boom: { t: 'boom', tune: 40, pitch: 2.5, pt: 0.05, decay: 1.5, drive: 1.8, lvl: 0.8 },
  },
};

// The game's kit sample names (samples.js) as tweaks of the lab's voices.
export const SAMPLE_TWEAK = {
  kickSoft: { decay: 0.6, click: 0.3, lvl: 0.85 },
  kickBoom: { decay: 1.5, tune: 0.92 },
  kickTight: { decay: 0.55, tune: 1.12 },
  snareGated: { gate: 0.2, snappy: 1.5 },
  snareSoft: { tone: 0.7, lvl: 0.8 },
  snareCrisp: { snappy: 1.3, tune: 1.08 },
  snareFat: { tune: 0.85, decay: 1.5 },
  clapBig: { tail: 1.9 },
  hatSoft: { decay: 0.8, hp: 0.8, lvl: 0.8 },
};
const ABSOLUTE = new Set(['gate']);

export function kitVoice(kitName, lane, sample) {
  const base = KITS[kitName]?.[lane];
  if (!base) return null;
  const tw = SAMPLE_TWEAK[sample];
  if (!tw) return base;
  const out = { ...base };
  for (const [k, v] of Object.entries(tw)) out[k] = ABSOLUTE.has(k) ? v : (out[k] ?? (k === 'lvl' ? 1 : 0)) * v;
  return out;
}

// A drum machine: any number of overlapping hits; the open hat is choked
// by the closed one, as on both machines.
export class Drums {
  constructor(seed = 3) { this.r = rng(seed); this.hits = []; }
  hit(lane, k, vel, rate = 1, durS = 0) {
    if (!k) return;
    if (lane === 'hat') for (const h of this.hits) if (h.lane === 'ohat') h.amp.k = Math.exp(-1 / (0.01 * SR / 4.6));
    const h = new Hit(k.t, k, vel, this.r, rate, durS);
    h.lane = lane;
    this.hits.push(h);
    return h;
  }
  // Mixes every hit into out; `each(lane, y)` lets the engine route lanes.
  run(each) {
    let alive = 0;
    for (let i = 0; i < this.hits.length; i++) {
      const h = this.hits[i];
      if (h.done) continue;
      each(h, h.run());
      alive++;
    }
    if (this.hits.length > 32 && alive < this.hits.length / 2) this.hits = this.hits.filter((h) => !h.done);
  }
}

// ── Patches: the game's instruments re-voiced (B) ────────────────
// Keys are the patch names in src/game/audio/tracks.js. `kind` picks the
// instrument; the rest are its parameters (see the classes above). Times in
// seconds, cutoffs in Hz, fenv in octaves, detune and vib in cents.
export const BPATCH = {
  sawBass: { kind: 'mono', osc: [{ w: 'saw' }, { w: 'saw', det: 8 }], sub: 0.6, cutoff: 260, res: 0.35, fenv: 2.6, fd: 0.2, fs: 0.15, drive: 1.2, a: 0.002, d: 0.3, s: 0.85, r: 0.05, gain: 0.1094 },
  pluckBass: { kind: 'tb303', wave: 'square', cutoff: 320, res: 0.55, env: 0.45, decay: 0.18, accent: 0.5, drive: 0.4, gain: 0.1997 },
  reese: { kind: 'mono', osc: [{ w: 'saw', det: -22 }, { w: 'saw', det: 22 }, { w: 'saw', oct: -1, lvl: 0.6 }], cutoff: 520, res: 0.2, fenv: 0.4, fd: 0.4, fs: 0.6, drive: 2.2, a: 0.01, s: 1, r: 0.08, gain: 0.1739, keytrack: 0.2 },
  sub: { kind: 'mono', osc: [{ w: 'sine' }], cutoff: 900, res: 0, fenv: 0, a: 0.004, d: 0.5, s: 0.9, r: 0.08, glide: 0.08, gain: 0.1148 },
  darkBass: { kind: 'mono', osc: [{ w: 'saw' }, { w: 'pulse', det: -6, pw: 0.4 }], sub: 0.5, cutoff: 420, res: 0.3, fenv: 2, fd: 0.14, fs: 0.2, drive: 2.4, a: 0.002, d: 0.3, s: 0.8, r: 0.04, gain: 0.127 },
  acid: { kind: 'tb303', wave: 'saw', cutoff: 300, res: 0.82, env: 0.62, decay: 0.32, accent: 0.75, drive: 0.9, glide: 0.06, gain: 0.127 },
  roundBass: { kind: 'mono', osc: [{ w: 'tri' }], sub: 0.7, cutoff: 800, res: 0.05, fenv: 1, fd: 0.12, fs: 0.3, a: 0.003, d: 0.3, s: 0.85, r: 0.08, gain: 0.1892 },
  superPad: { kind: 'juno', saw: 0.9, pulse: 0.5, pwm: 0.7, sub: 0.25, unison: 2, detune: 14, cutoff: 1900, res: 0.12, fenv: 0.6, fa: 1.2, fd: 1.5, fs: 0.7, a: 0.35, d: 1, s: 0.85, r: 1, hpf: 140, chorus: 2, lfoRate: 0.45, gain: 0.1973 },
  warmPad: { kind: 'juno', saw: 0, pulse: 0.9, pwm: 0.6, sub: 0.35, cutoff: 1300, res: 0.1, fenv: 0.3, a: 0.8, d: 1, s: 1, r: 1.6, hpf: 120, chorus: 1, lfoRate: 0.3, gain: 0.1177 },
  darkPad: { kind: 'juno', saw: 1, pulse: 0.3, pwm: 0.5, sub: 0.2, unison: 2, detune: 10, cutoff: 750, res: 0.25, fenv: 0.4, fa: 1.5, fd: 2, fs: 0.6, a: 0.7, s: 1, r: 1.1, hpf: 110, chorus: 2, lfoRate: 0.25, gain: 0.1654 },
  stab: { kind: 'juno', saw: 1, pulse: 0.4, sub: 0.15, unison: 2, detune: 12, cutoff: 1300, res: 0.2, fenv: 1.6, fa: 0.001, fd: 0.18, fs: 0.15, a: 0.002, d: 0.25, s: 0.3, r: 0.15, chorus: 1, gain: 0.5114 },
  hit: { kind: 'juno', saw: 1, pulse: 0.2, sub: 0.45, unison: 3, detune: 22, cutoff: 2200, res: 0.15, fenv: 1.2, fa: 0.001, fd: 0.35, fs: 0.3, a: 0.004, d: 0.6, s: 0.25, r: 0.5, chorus: 2, gain: 0.3034 },
  ep: { kind: 'fm', algo: 'ep', ops: [{ r: 1, l: 1, a: 0.002, d: 1.6, s: 0.15, r: 0.5 }, { r: 1, l: 1.6, d: 0.9, s: 0.12, v: 0.8 }, { r: 1, det: 7, l: 0.6, a: 0.002, d: 1.1, s: 0.1, r: 0.4 }, { r: 14, l: 0.9, d: 0.05, s: 0, v: 0.9 }], r: 0.45, chorus: 1, gain: 0.2335 },
  bell: { kind: 'fm', algo: 'bell', ops: [{ r: 1, l: 1, d: 1.6, s: 0.05, r: 0.8 }, { r: 3.5, l: 2.4, d: 0.8, s: 0.05 }, { r: 2, det: 4, l: 0.5, d: 2.4, s: 0, r: 0.9 }, { r: 7.07, l: 1.2, d: 0.4, s: 0 }], r: 0.7, chorus: 1, gain: 0.0914 },
  glass: { kind: 'fm', algo: 'pair', ops: [{ r: 1, l: 1, d: 0.5, s: 0.1, r: 0.35 }, { r: 2, l: 1.3, d: 0.3, s: 0.1 }], r: 0.3, chorus: 1, gain: 0.069 },
  pluck: { kind: 'juno', saw: 1, pulse: 0, unison: 2, detune: 8, cutoff: 500, res: 0.3, fenv: 3.2, fa: 0.001, fd: 0.11, fs: 0, a: 0.002, d: 0.2, s: 0, r: 0.15, chorus: 1, gain: 0.2148 },
  sqArp: { kind: 'juno', saw: 0, pulse: 1, pw: 0.25, cutoff: 2400, res: 0.15, fenv: 0.8, fa: 0.001, fd: 0.09, fs: 0.3, a: 0.002, d: 0.14, s: 0.5, r: 0.1, chorus: 1, gain: 0.0932 },
  twang: { kind: 'fm', algo: 'stack', ops: [{ r: 1, l: 1, d: 0.8, s: 0.2, r: 0.3 }, { r: 1, l: 2.4, d: 0.25, s: 0.15 }, { r: 3, l: 0.9, d: 0.08, s: 0 }, { r: 1, l: 0.3, d: 0.1, s: 0 }], bend: 0.7, bendT: 0.07, r: 0.3, chorus: 0, gain: 0.081 },
  brassLead: { kind: 'mono', osc: [{ w: 'saw' }, { w: 'saw', det: 9 }], cutoff: 1300, res: 0.2, fenv: 1.5, fa: 0.04, fd: 0.35, fs: 0.55, a: 0.02, d: 0.35, s: 0.85, r: 0.2, vib: 14, vibRate: 5.3, vibDelay: 0.25, drive: 0.8, gain: 0.0693 },
  sawLead: { kind: 'mono', osc: [{ w: 'saw', det: -12 }, { w: 'saw', det: 12 }, { w: 'saw', oct: 1, lvl: 0.35 }], cutoff: 3200, res: 0.1, fenv: 0.6, fd: 0.3, fs: 0.6, a: 0.008, s: 0.9, r: 0.14, vib: 10, vibDelay: 0.3, glide: 0.05, drive: 0.6, gain: 0.0682 },
  sqLead: { kind: 'mono', osc: [{ w: 'pulse', pw: 0.35 }, { w: 'pulse', det: 7, pw: 0.5, lvl: 0.7 }], cutoff: 2800, res: 0.25, fenv: 0.8, fd: 0.2, fs: 0.5, a: 0.006, s: 0.85, r: 0.12, vib: 12, vibDelay: 0.2, glide: 0.04, gain: 0.0838 },
  whistle: { kind: 'mono', osc: [{ w: 'sine' }], noise: 0.04, cutoff: 6000, res: 0, fenv: 0, a: 0.03, s: 0.9, r: 0.25, vib: 24, vibRate: 5.8, vibDelay: 0.16, bend: 0.5, bendT: 0.08, glide: 0.06, gain: 0.0931 },
  flute: { kind: 'mono', osc: [{ w: 'tri' }, { w: 'sine', oct: 1, lvl: 0.15 }], noise: 0.12, cutoff: 2600, res: 0.05, fenv: 0.5, fa: 0.06, fd: 0.3, fs: 0.6, a: 0.05, s: 0.85, r: 0.22, vib: 16, vibDelay: 0.25, glide: 0.05, gain: 0.1238 },
  // Lab-only voices for the generated tracks.
  organ: { kind: 'fm', algo: 'organ', ops: [{ r: 1, l: 1, a: 0.002, d: 0.3, s: 0.7, r: 0.08 }, { r: 2, l: 0.6, a: 0.002, d: 0.2, s: 0.5, r: 0.08 }, { r: 3, l: 0.35, a: 0.001, d: 0.08, s: 0.2, r: 0.06 }, { r: 4.02, l: 0.25, a: 0.001, d: 0.05, s: 0, r: 0.05 }], r: 0.08, chorus: 1, gain: 0.19 },
  dubChord: { kind: 'juno', saw: 1, pulse: 0.5, pw: 0.4, unison: 2, detune: 9, cutoff: 900, res: 0.35, fenv: 1.8, fa: 0.001, fd: 0.16, fs: 0.05, a: 0.002, d: 0.22, s: 0.15, r: 0.2, hpf: 220, chorus: 1, gain: 0.45 },
  rumble: { kind: 'mono', osc: [{ w: 'sine' }, { w: 'tri', lvl: 0.4 }], cutoff: 260, res: 0.1, fenv: 0.8, fd: 0.12, fs: 0, drive: 2.5, a: 0.002, d: 0.22, s: 0.2, r: 0.1, gain: 0.12 },
  // The supersaw (JP-8000): seven detuned saws, high-passed so the bass
  // keeps the low end, wide chorus. As a lead, and slower as a pad.
  supersaw: { kind: 'juno', saw: 1, pulse: 0, unison: 7, detune: 36, cutoff: 5000, res: 0.05, fenv: 0.8, fa: 0.01, fd: 0.4, fs: 0.6, a: 0.01, d: 0.3, s: 0.85, r: 0.25, hpf: 260, chorus: 2, lfoRate: 0.4, gain: 0.11 },
  superPadWide: { kind: 'juno', saw: 1, pulse: 0.2, pwm: 0.4, unison: 5, detune: 30, cutoff: 1600, res: 0.1, fenv: 0.9, fa: 1.5, fd: 2, fs: 0.7, a: 0.6, d: 1, s: 0.9, r: 1.4, hpf: 200, chorus: 2, lfoRate: 0.3, gain: 0.1 },
  // Trance bass: a short saw with sub, the filter closing fast (the
  // "rolling" off-beat bass is this played on the 16ths between the kicks).
  tranceBass: { kind: 'mono', osc: [{ w: 'saw' }, { w: 'pulse', det: 4, pw: 0.5, lvl: 0.5 }], sub: 0.5, cutoff: 180, res: 0.3, fenv: 2.4, fd: 0.1, fs: 0.05, drive: 1.4, a: 0.002, d: 0.14, s: 0.5, r: 0.04, gain: 0.13 },
  // Psy bass: saw plus square an octave down, very short, a hard filter
  // snap, driven; one note per step and always the root.
  psyBass: { kind: 'mono', osc: [{ w: 'saw' }, { w: 'pulse', oct: -1, pw: 0.5, lvl: 0.7 }], cutoff: 150, res: 0.42, fenv: 2.8, fd: 0.07, fs: 0, drive: 2.2, a: 0.001, d: 0.1, s: 0.3, r: 0.02, keytrack: 0.2, gain: 0.15 },
  // Eurobeat lead: bright, three saws, fast vibrato after a moment, glide.
  euroLead: { kind: 'mono', osc: [{ w: 'saw', det: -14 }, { w: 'saw', det: 14 }, { w: 'pulse', oct: 1, pw: 0.3, lvl: 0.3 }], cutoff: 4200, res: 0.12, fenv: 0.8, fa: 0.005, fd: 0.25, fs: 0.7, a: 0.004, d: 0.2, s: 0.9, r: 0.12, vib: 18, vibRate: 6, vibDelay: 0.18, glide: 0.03, drive: 0.7, gain: 0.07 },
  // A piano for house: two FM pairs, a bright hammer that fades at once.
  piano: { kind: 'fm', algo: 'ep', ops: [{ r: 1, l: 1, a: 0.001, d: 1.4, s: 0.1, r: 0.3 }, { r: 1, l: 2.0, d: 0.22, s: 0.08, v: 0.8 }, { r: 2, det: 3, l: 0.45, a: 0.001, d: 0.9, s: 0.05, r: 0.3 }, { r: 7, l: 1.1, d: 0.06, s: 0, v: 0.9 }], r: 0.3, chorus: 0, gain: 0.2 },
  // A choir: pulse with PWM through the vowel bank, slow and wide.
  choir: { kind: 'juno', saw: 0.4, pulse: 0.8, pwm: 0.5, sub: 0, unison: 3, detune: 12, cutoff: 3500, res: 0.05, fenv: 0, a: 0.5, d: 1, s: 1, r: 1.2, hpf: 150, chorus: 2, lfoRate: 0.35, vowel: 'a', vowelMix: 0.85, vowelQ: 8, gain: 0.07 },
  // A trance pluck: the arp's voice, bright attack, no sustain.
  pluckTrance: { kind: 'juno', saw: 1, pulse: 0.3, pw: 0.3, unison: 3, detune: 14, cutoff: 900, res: 0.25, fenv: 3, fa: 0.001, fd: 0.13, fs: 0, a: 0.001, d: 0.22, s: 0, r: 0.15, hpf: 200, chorus: 2, gain: 0.16 },
  // A psy lead: FM zap, a fast falling index, short.
  zap: { kind: 'fm', algo: 'stack', ops: [{ r: 1, l: 1, a: 0.001, d: 0.3, s: 0.2, r: 0.1 }, { r: 2, l: 2.6, d: 0.07, s: 0.1, v: 0.9 }, { r: 3, l: 1.2, d: 0.04, s: 0 }, { r: 1, l: 0.4, d: 0.05, s: 0 }], fbk: 0.3, r: 0.1, chorus: 1, gain: 0.1 },
};

// The game's patch for a part: by identity, else the closest by fields
// (parts like `{ ...P.flute, oct: 1 }` are copies with an override).
export function patchName(inst, patches) {
  for (const [k, v] of Object.entries(patches)) if (v === inst) return k;
  let best = null, score = -1;
  for (const [k, v] of Object.entries(patches)) {
    let s = 0;
    for (const f of Object.keys(v)) if (JSON.stringify(v[f]) === JSON.stringify(inst[f])) s++;
    if (v.type !== inst.type) s -= 100;
    if (s > score) { score = s; best = k; }
  }
  return best;
}

// A game track (tracks.js) with every part's `lab` patch set: the B voicing.
export function voiceTrack(T, patches) {
  const parts = {};
  for (const [name, p] of Object.entries(T.parts)) {
    const pn = patchName(p.inst, patches);
    const base = BPATCH[pn] ?? BPATCH.superPad;
    const lab = { ...base, name: pn };
    if (p.inst.oct) lab.oct = (lab.oct ?? 0) + p.inst.oct;
    const ref = patches[pn];
    if (ref && p.inst.gain && ref.gain) lab.gain = base.gain * (p.inst.gain / ref.gain);
    parts[name] = { ...p, inst: p.inst, lab };
  }
  return { ...T, parts };
}

// Builds the instrument for a lab patch.
export function makeInstrument(lab, seed) {
  return lab.kind === 'tb303' ? new TB303(lab, seed) : new Poly(lab.kind, lab, seed);
}

// Knobs for the page, per kind: [key, label, min, max, step].
export const KNOBS = {
  tb303: [['cutoff', 'Cutoff (Hz)', 60, 3000, 1], ['res', 'Resonance', 0, 1, 0.01], ['env', 'Env mod', 0, 1, 0.01], ['decay', 'Decay (s)', 0.05, 2, 0.01], ['accent', 'Accent', 0, 1, 0.01], ['drive', 'Drive', 0, 3, 0.01], ['glide', 'Slide time (s)', 0.01, 0.2, 0.001], ['gain', 'Level', 0, 1.5, 0.01]],
  juno: [['saw', 'Saw', 0, 1, 0.01], ['pulse', 'Pulse', 0, 1, 0.01], ['pw', 'Pulse width', 0.05, 0.95, 0.01], ['pwm', 'PWM (LFO)', 0, 1, 0.01], ['sub', 'Sub', 0, 1, 0.01], ['noise', 'Noise', 0, 0.5, 0.01], ['unison', 'Unison', 1, 7, 1], ['detune', 'Detune (c)', 0, 40, 0.5], ['hpf', 'HPF (Hz)', 20, 1000, 1], ['cutoff', 'Cutoff (Hz)', 80, 12000, 1], ['res', 'Resonance', 0, 1, 0.01], ['fenv', 'Env (oct)', 0, 5, 0.01], ['keytrack', 'Keytrack', 0, 1, 0.01], ['fa', 'F attack', 0.001, 3, 0.001], ['fd', 'F decay', 0.01, 3, 0.01], ['fs', 'F sustain', 0, 1, 0.01], ['a', 'Attack', 0.001, 3, 0.001], ['d', 'Decay', 0.01, 3, 0.01], ['s', 'Sustain', 0, 1, 0.01], ['r', 'Release', 0.01, 4, 0.01], ['lfoRate', 'LFO (Hz)', 0.05, 8, 0.01], ['chorus', 'Chorus (0, I, II, I+II)', 0, 3, 1], ['drive', 'Drive', 0, 3, 0.01], ['gain', 'Level', 0, 1.5, 0.01]],
  mono: [['cutoff', 'Cutoff (Hz)', 40, 12000, 1], ['res', 'Resonance', 0, 1, 0.01], ['fenv', 'Env (oct)', 0, 5, 0.01], ['keytrack', 'Keytrack', 0, 1, 0.01], ['drive', 'Drive', 0, 4, 0.01], ['sub', 'Sub', 0, 1, 0.01], ['noise', 'Noise', 0, 0.5, 0.01], ['fa', 'F attack', 0.001, 2, 0.001], ['fd', 'F decay', 0.01, 2, 0.01], ['fs', 'F sustain', 0, 1, 0.01], ['a', 'Attack', 0.001, 2, 0.001], ['d', 'Decay', 0.01, 2, 0.01], ['s', 'Sustain', 0, 1, 0.01], ['r', 'Release', 0.01, 3, 0.01], ['glide', 'Glide (s)', 0, 0.4, 0.001], ['vib', 'Vibrato (c)', 0, 40, 0.5], ['gain', 'Level', 0, 1.5, 0.01]],
  fm: [['fbk', 'Feedback', 0, 1.5, 0.01], ['r', 'Release', 0.01, 3, 0.01], ['chorus', 'Chorus (0, I, II, I+II)', 0, 3, 1], ['gain', 'Level', 0, 1.5, 0.01]],
};
// FM operators get their own knobs: index/level, ratio and decay per op.
export const FM_OP_KNOBS = [['l', 'level', 0, 4, 0.01], ['r', 'ratio', 0.5, 16, 0.01], ['d', 'decay', 0.01, 4, 0.01], ['s', 'sustain', 0, 1, 0.01]];
// Drum knobs, per voice type.
export const DRUM_KNOBS = {
  kick808: [['tune', 'Tune (Hz)', 30, 90, 0.5], ['decay', 'Decay', 0.1, 2.5, 0.01], ['pitch', 'Pitch sweep', 1, 5, 0.01], ['click', 'Click', 0, 1.5, 0.01], ['drive', 'Drive', 0.5, 4, 0.01], ['lvl', 'Level', 0, 2, 0.01]],
  snare808: [['tune', 'Tune (Hz)', 120, 300, 1], ['tone', 'Snappy mix', 0, 1, 0.01], ['snappy', 'Snappy decay', 0.03, 0.5, 0.005], ['decay', 'Tone decay', 0.03, 0.4, 0.005], ['lvl', 'Level', 0, 2, 0.01]],
  clap: [['bp', 'Tone (Hz)', 600, 2500, 1], ['tail', 'Tail', 0.05, 0.6, 0.01], ['bursts', 'Bursts', 1, 6, 1], ['spacing', 'Spacing', 0.004, 0.025, 0.0005], ['lvl', 'Level', 0, 2, 0.01]],
  metal: [['tune', 'Tune', 0.5, 2.5, 0.01], ['decay', 'Decay', 0.01, 2, 0.005], ['hp', 'HPF (Hz)', 2000, 12000, 10], ['noise', 'Noise', 0, 1, 0.01], ['lvl', 'Level', 0, 2, 0.01]],
  cymbal: [['tune', 'Tune', 0.5, 2.5, 0.01], ['decay', 'Decay', 0.2, 4, 0.01], ['noise', 'Noise', 0, 1, 0.01], ['lvl', 'Level', 0, 2, 0.01]],
  tom: [['tune', 'Tune (Hz)', 50, 300, 1], ['decay', 'Decay', 0.05, 1.5, 0.01], ['pitch', 'Pitch sweep', 1, 3, 0.01], ['noise', 'Noise', 0, 0.5, 0.01], ['lvl', 'Level', 0, 2, 0.01]],
  default: [['decay', 'Decay', 0.01, 2, 0.005], ['lvl', 'Level', 0, 2, 0.01]],
};
DRUM_KNOBS.kick909 = DRUM_KNOBS.kick808;
DRUM_KNOBS.snare909 = DRUM_KNOBS.snare808;
DRUM_KNOBS.boom = DRUM_KNOBS.tom;
