// Music Lab: the DSP building blocks, per sample. A prototype of
// docs/vision/sound.md 3.1 and 3.3 for listening only: nothing in the game
// uses it, and it is free to use Math.*. Every class takes plain numbers and
// keeps its own state, so the same code runs in the AudioWorklet and under
// Node for the tests.
//
// What is here, and why it sounds less "browser" than OscillatorNode +
// BiquadFilterNode:
// - Oscillators are PolyBLEP band-limited and carry their own slow random
//   drift, so two voices are never exactly in tune (analogue character).
// - Filters are zero-delay-feedback (topology-preserving transform) designs:
//   a 4-pole ladder with saturation in the loop (Moog / Juno / 303 family)
//   and a state-variable filter. Resonance stays stable when the cutoff is
//   swept fast, and pushing them saturates instead of clipping.
// - The reverb is a feedback delay network with damping and slow modulation,
//   the delay a tempo-synced ping-pong with a darkening feedback path, the
//   bus has a glue compressor and tape saturation.

export const TAU = Math.PI * 2;
export let SR = 48000;
export function setRate(r) { SR = r; }

export const clamp = (v, lo, hi) => (v < lo ? lo : v > hi ? hi : v);
export const mtof = (m) => 440 * Math.pow(2, (m - 69) / 12);
export const dbToGain = (db) => Math.pow(10, db / 20);

// A cheap tanh (Padé 3/2, clamped): smooth, odd, and 1 at the rails.
export function tanh(x) {
  if (x < -3) return -1;
  if (x > 3) return 1;
  const x2 = x * x;
  return (x * (27 + x2)) / (27 + 9 * x2);
}

// sin(2π·x) from a 4096-point table with linear interpolation (≈ -100 dB
// error): the FM operators call it millions of times a second.
const SIN_N = 4096, SIN = new Float64Array(SIN_N + 1);
for (let i = 0; i <= SIN_N; i++) SIN[i] = Math.sin((TAU * i) / SIN_N);
export function sin1(x) {
  x -= Math.floor(x);
  const f = x * SIN_N, i = f | 0;
  return SIN[i] + (SIN[i + 1] - SIN[i]) * (f - i);
}

// A seeded generator (mulberry32), so a render is the same every time.
export function rng(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// ── Oscillators ──────────────────────────────────────────────────
// PolyBLEP: subtracts the band-limited step's residual around each
// discontinuity, which removes most of the aliasing of a naive saw or pulse.
function blep(t, dt) {
  if (t < dt) { t /= dt; return t + t - t * t - 1; }
  if (t > 1 - dt) { t = (t - 1) / dt; return t * t + t + t + 1; }
  return 0;
}

// One analogue-style oscillator: saw, pulse (with width) and triangle, all
// from one phase, plus a sub an octave down (the Juno's square sub). The
// caller mixes the outputs.
export class Osc {
  constructor(phase = 0) { this.ph = phase; this.subPh = 0; this.tri = 0; this.saw = 0; this.pulse = 0; this.sub = 0; }
  // mask: which outputs to compute (OSC_SAW | OSC_PULSE | OSC_TRI | OSC_SUB).
  run(hz, width = 0.5, mask = 1) {
    const dt = clamp(hz / SR, 0, 0.45);
    let ph = this.ph;
    if (mask & 1) this.saw = 2 * ph - 1 - blep(ph, dt);
    if (mask & 2) {
      let p = ph < width ? 1 : -1;
      p += blep(ph, dt);
      let q = ph - width; if (q < 0) q += 1;
      this.pulse = p - blep(q, dt);
    }
    if (mask & 4) {
      // Triangle: the integrated square (leaky, so it can't drift off).
      const sq = (ph < 0.5 ? 1 : -1) + blep(ph, dt) - blep((ph + 0.5) % 1, dt);
      this.tri = this.tri * 0.9995 + sq * 4 * dt;
    }
    ph += dt;
    if (ph >= 1) { ph -= 1; this.subPh ^= 1; }
    this.ph = ph;
    if (mask & 8) {
      // The sub flips on every wrap: a rising edge when it turns 1. Just after
      // a wrap the edge was the one that set subPh; just before, the next one.
      const s = this.subPh ? 1 : -1;
      const rising = ph < 0.5 ? s > 0 : s < 0;
      this.sub = s + (rising ? 1 : -1) * blep(ph, dt);
    }
    return this.saw;
  }
}
export const OSC_SAW = 1, OSC_PULSE = 2, OSC_TRI = 4, OSC_SUB = 8;

export class Sine {
  constructor(phase = 0) { this.ph = phase; }
  run(hz) {
    const y = Math.sin(TAU * this.ph);
    this.ph += hz / SR;
    if (this.ph >= 1) this.ph -= Math.floor(this.ph);
    return y;
  }
}

// Slow analogue drift: a smoothed random walk, in cents. Each voice owns one.
export class Drift {
  constructor(r, cents = 4, hz = 0.6) { this.r = r; this.c = cents; this.v = (r() * 2 - 1) * cents; this.t = this.v; this.a = 1 - Math.exp((-TAU * hz) / SR); this.n = 0; }
  // Called every `n` samples (control rate).
  run(n = 1) {
    if ((this.n -= n) <= 0) { this.t = (this.r() * 2 - 1) * this.c; this.n = (SR * (0.2 + this.r() * 0.6)) | 0; }
    this.v += this.a * n * (this.t - this.v);
    return this.v;
  }
}

// ── Filters ──────────────────────────────────────────────────────
export class OnePole {
  constructor(hz = 1000) { this.y = 0; this.setHz(hz); }
  setHz(hz) { this.a = 1 - Math.exp((-TAU * clamp(hz, 1, SR * 0.49)) / SR); return this; }
  lp(x) { return (this.y += this.a * (x - this.y)); }
  hp(x) { return x - this.lp(x); }
}

// ZDF 4-pole ladder (Zavalishin's TPT form) with a tanh on the input of the
// feedback sum. res 0..1 (self-oscillates near 1); drive pushes the input.
// `mode` 0 = 24 dB low-pass (Moog / Juno IR3109), 1 = an 18 dB tap mix that
// leans toward the 303's diode ladder.
export class Ladder {
  constructor() { this.s = new Float64Array(4); this.out = new Float64Array(4); this.g = 0; this.G = 0; this.k = 0; this.hz = -1; this.res = -1; }
  set(hz, res) {
    if (hz !== this.hz) {
      this.hz = hz;
      const g = Math.tan((Math.PI * clamp(hz, 10, SR * 0.45)) / SR);
      this.g = g; this.G = g / (1 + g);
    }
    if (res !== this.res) { this.res = res; this.k = 4 * clamp(res, 0, 1.05); }
  }
  run(x, drive = 1, mode = 0) {
    const s = this.s, G = this.G, k = this.k;
    const G2 = G * G, G3 = G2 * G, G4 = G3 * G;
    // Estimate of the loop output without the input (the "zero-delay" solve).
    const S = (G3 * s[0] + G2 * s[1] + G * s[2] + s[3]) * (1 - G);
    const u = tanh((x * drive - k * S) / (1 + k * G4));
    let y = u;
    const out = this.out;
    for (let i = 0; i < 4; i++) {
      const v = (y - s[i]) * G;
      const lp = v + s[i];
      s[i] = lp + v;
      out[i] = lp;
      y = lp;
    }
    if (mode === 1) return out[2] * 1.25 - out[3] * 0.3;
    // Make up some of the pass-band loss resonance costs (as the hardware does).
    return out[3] * (1 + k * 0.25);
  }
  reset() { this.s.fill(0); }
}

// ZDF state-variable filter: low, band and high at once.
export class SVF {
  constructor() { this.ic1 = 0; this.ic2 = 0; this.lp = 0; this.bp = 0; this.hp = 0; this.hz = -1; this.q = -1; }
  set(hz, q = 0.707) {
    if (hz === this.hz && q === this.q) return this;
    this.hz = hz; this.q = q;
    const g = Math.tan((Math.PI * clamp(hz, 10, SR * 0.45)) / SR), k = 1 / q;
    this.a1 = 1 / (1 + g * (g + k)); this.a2 = g * this.a1; this.a3 = g * this.a2; this.k = k;
    return this;
  }
  run(x) {
    const v3 = x - this.ic2;
    const v1 = this.a1 * this.ic1 + this.a2 * v3;
    const v2 = this.ic2 + this.a2 * this.ic1 + this.a3 * v3;
    this.ic1 = 2 * v1 - this.ic1; this.ic2 = 2 * v2 - this.ic2;
    this.lp = v2; this.bp = v1; this.hp = x - this.k * v1 - v2;
    return v2;
  }
}

// ── Envelopes ────────────────────────────────────────────────────
// Analogue-style ADSR: exponential segments (RC curves), retriggerable from
// wherever it is (no click), times in seconds.
export class ADSR {
  constructor(a = 0.005, d = 0.2, s = 0.7, r = 0.2) { this.v = 0; this.st = 0; this.setAll(a, d, s, r); }
  setAll(a, d, s, r) {
    const c = (t) => 1 - Math.exp(-1 / (Math.max(t, 0.0005) * SR / 4));
    this.ka = 1 - Math.exp(-1 / (Math.max(a, 0.0005) * SR / 1.6)); this.kd = c(d); this.s = s; this.kr = c(r);
    return this;
  }
  on() { this.st = 1; }
  off() { if (this.st && this.st < 4) this.st = 4; }
  get done() { return this.st === 0 || (this.st === 4 && this.v < 5e-4); }
  run() {
    switch (this.st) {
      case 1: this.v += this.ka * (1.25 - this.v); if (this.v >= 1) { this.v = 1; this.st = 2; } break;
      case 2: case 3: this.v += this.kd * (this.s - this.v); break;
      case 4: this.v += this.kr * (0 - this.v); if (this.v < 5e-4) { this.v = 0; this.st = 0; } break;
      default: break;
    }
    return this.v;
  }
}

// A one-shot decay (drums, the 303's filter envelope): jumps to 1, decays.
export class Decay {
  constructor(t = 0.2) { this.v = 0; this.set(t); }
  set(t) { this.k = Math.exp(-1 / (Math.max(t, 0.0005) * SR / 4.6)); return this; }
  hit(v = 1) { this.v = v; }
  run() { return (this.v *= this.k); }
}

// ── Effects ──────────────────────────────────────────────────────
export class Delay {
  constructor(maxSecs) { this.b = new Float32Array(Math.ceil(maxSecs * SR) + 4); this.w = 0; }
  write(x) { this.b[this.w] = x; this.w = (this.w + 1) % this.b.length; }
  // Read `d` samples back (fractional, linear interpolation).
  read(d) {
    const n = this.b.length;
    let p = this.w - d - 1;
    while (p < 0) p += n;
    const i = p | 0, f = p - i;
    return this.b[i] * (1 - f) + this.b[(i + 1) % n] * f;
  }
}

// Stereo ping-pong delay with high-pass in, low-pass in the loop.
export class PingPong {
  constructor() { this.l = new Delay(2.1); this.r = new Delay(2.1); this.hp = new OnePole(280); this.lpL = new OnePole(3200); this.lpR = new OnePole(3200); this.time = 0.3; this.fb = 0.38; }
  run(x, out) {
    const d = this.time * SR;
    const yl = this.l.read(d), yr = this.r.read(d);
    this.l.write(this.hp.hp(x) + this.lpR.lp(yr) * this.fb);
    this.r.write(this.lpL.lp(yl));
    out[0] += yl * 0.5; out[1] += yr * 0.5;
  }
}

// An 8-line feedback delay network (Householder mix), damped and slowly
// modulated: a smooth hall with no metallic ring, cheap enough for phones.
const FDN_LEN = [1031, 1327, 1523, 1801, 2053, 2311, 2633, 2971];
export class Reverb {
  constructor(seed = 7) {
    const r = rng(seed);
    this.n = FDN_LEN.map((l) => new Delay((l * (SR / 48000) + 32) / SR));
    this.len = FDN_LEN.map((l) => l * (SR / 48000));
    this.damp = FDN_LEN.map(() => new OnePole(5200));
    this.ph = FDN_LEN.map(() => r());
    this.pre = new Delay(0.1);
    this.inLp = new OnePole(9000); this.inHp = new OnePole(120);
    this.y = new Float64Array(8);
    this.k = 0; this.mod = Float64Array.from(this.len);
    this.set(2.4, 0.5);
  }
  // size: decay time (RT60, s); tone 0 (dark) .. 1 (bright).
  set(rt60, tone = 0.5) {
    this.rt = rt60;
    this.g = this.len.map((l) => Math.pow(10, (-3 * l) / (rt60 * SR)));
    for (const d of this.damp) d.setHz(2500 + tone * 7000);
  }
  run(inL, inR, out) {
    this.pre.write((inL + inR) * 0.5);
    let x = this.pre.read(0.018 * SR);
    x = this.inHp.hp(this.inLp.lp(x));
    const y = this.y;
    let sum = 0;
    if ((this.k = (this.k + 1) & 31) === 0) {
      for (let i = 0; i < 8; i++) {
        this.ph[i] += (32 * (0.07 + i * 0.013)) / SR;
        if (this.ph[i] > 1) this.ph[i] -= 1;
        this.mod[i] = this.len[i] + Math.sin(TAU * this.ph[i]) * 6;
      }
    }
    for (let i = 0; i < 8; i++) {
      y[i] = this.damp[i].lp(this.n[i].read(this.mod[i])) * this.g[i];
      sum += y[i];
    }
    const h = sum * 0.25; // Householder: y - 2/N * sum
    for (let i = 0; i < 8; i++) this.n[i].write(y[i] - h + x * (i & 1 ? -0.35 : 0.35));
    out[0] += (y[0] - y[2] + y[4] - y[6]) * 0.6;
    out[1] += (y[1] - y[3] + y[5] - y[7]) * 0.6;
  }
}

// Stereo-linked feed-forward compressor (RMS-ish detector), for glue.
export class Compressor {
  constructor() { this.env = 0; this.gr = 1; this.set(-14, 2.5, 0.01, 0.15); }
  set(threshDb, ratio, att, rel) {
    this.th = threshDb; this.ratio = ratio;
    this.ka = Math.exp(-1 / (att * SR)); this.kr = Math.exp(-1 / (rel * SR));
    return this;
  }
  run(l, r) {
    const x = Math.max(Math.abs(l), Math.abs(r));
    const k = x > this.env ? this.ka : this.kr;
    this.env = k * this.env + (1 - k) * x;
    const db = 20 * Math.log10(this.env + 1e-9);
    const over = db - this.th;
    this.gr = over > 0 ? Math.pow(10, (-over * (1 - 1 / this.ratio)) / 20) : 1;
    return this.gr;
  }
}

// Tape-ish saturation: asymmetric soft clip with a little head bump and a
// high-frequency roll-off that grows with drive.
export class Tape {
  constructor() { this.lp = new OnePole(16000); this.bump = new SVF().set(90, 0.9); this.dc = new OnePole(12); this.drive = 0; }
  set(drive) { this.drive = drive; this.lp.setHz(18000 - drive * 6000); return this; }
  run(x) {
    if (this.drive <= 0) return x;
    // Unity gain for small signals; drive sets how early the curve bends.
    const d = 1 + this.drive * 1.5;
    this.bump.run(x);
    const y = tanh((x + this.bump.bp * 0.15 * this.drive) * d + 0.05 * this.drive) / d;
    return this.lp.lp(y - this.dc.lp(y));
  }
}

// A brickwall-ish peak limiter at the very end, so nothing ever clips.
export class Limiter {
  constructor(ceil = 0.95) { this.ceil = ceil; this.g = 1; this.kr = Math.exp(-1 / (0.12 * SR)); }
  run(l, r, out) {
    const p = Math.max(Math.abs(l), Math.abs(r)) * this.g;
    if (p > this.ceil) this.g *= this.ceil / p; else this.g = this.kr * this.g + (1 - this.kr);
    out[0] = l * this.g; out[1] = r * this.g;
  }
}

// Juno-style BBD chorus: two delay taps swept by one triangle LFO in
// opposite directions, wet only on the sides (mode I 0.51 Hz, II 0.86 Hz).
export class Chorus {
  constructor() { this.d = new Delay(0.03); this.ph = 0; this.rate = 0.513; this.depth = 1; this.lp = new OnePole(9000); }
  set(mode) { this.rate = mode === 2 ? 0.863 : mode === 3 ? 9.75 : 0.513; this.depth = mode === 3 ? 0.25 : 1; this.on = mode > 0; }
  run(x, out, gain = 1) {
    if (!this.on) { out[0] += x * gain; out[1] += x * gain; return; }
    this.d.write(this.lp.lp(x));
    this.ph += this.rate / SR; if (this.ph > 1) this.ph -= 1;
    const tri = this.ph < 0.5 ? this.ph * 4 - 1 : 3 - this.ph * 4;
    const base = 0.0035 * SR, sw = 0.0017 * SR * this.depth;
    const a = this.d.read(base + sw * tri), b = this.d.read(base - sw * tri);
    out[0] += (x * 0.7 + a * 0.7) * gain;
    out[1] += (x * 0.7 + b * 0.7) * gain;
  }
}
