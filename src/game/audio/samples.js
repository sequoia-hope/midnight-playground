// One-shot sounds rendered once, with plain sample math, into AudioBuffers:
// the drum kit for the music and the layered crash / gravel / scrape / clunk
// sounds for the SFX. A hit then costs one AudioBufferSourceNode and a gain
// instead of a small graph of oscillators and filters per note, and the
// sounds can use things WebAudio nodes can't do cheaply (modal resonators,
// gated reverb tails, per-grain filtering). Everything renders in ~50 ms.

export function rngFrom(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// RBJ-cookbook biquad for offline use (direct form I).
class Biquad {
  constructor(type, f, q, sr) { this.set(type, f, q, sr); this.x1 = this.x2 = this.y1 = this.y2 = 0; }
  set(type, f, q, sr) {
    const w = (2 * Math.PI * Math.min(f, sr * 0.45)) / sr, c = Math.cos(w), al = Math.sin(w) / (2 * q);
    let b0, b1, b2;
    if (type === 'lp') { b0 = (1 - c) / 2; b1 = 1 - c; b2 = b0; }
    else if (type === 'hp') { b0 = (1 + c) / 2; b1 = -(1 + c); b2 = b0; }
    else { b0 = al; b1 = 0; b2 = -al; } // band-pass, 0 dB peak
    const a0 = 1 + al;
    this.b0 = b0 / a0; this.b1 = b1 / a0; this.b2 = b2 / a0; this.a1 = (-2 * c) / a0; this.a2 = (1 - al) / a0;
    return this;
  }
  p(x) {
    const y = this.b0 * x + this.b1 * this.x1 + this.b2 * this.x2 - this.a1 * this.y1 - this.a2 * this.y2;
    this.x2 = this.x1; this.x1 = x; this.y2 = this.y1; this.y1 = y;
    return y;
  }
}

const TAU = Math.PI * 2;
const sat = (x, d) => Math.tanh(x * d) / Math.tanh(d);

// Short fades at both ends so nothing clicks, then peak-normalise to `peak`.
function finish(chs, sr, peak = 0.9, fadeIn = 0.0005, fadeOut = 0.01) {
  let m = 1e-9;
  for (const d of chs) for (let i = 0; i < d.length; i++) m = Math.max(m, Math.abs(d[i]));
  const fi = Math.max(1, Math.floor(fadeIn * sr)), fo = Math.max(1, Math.floor(fadeOut * sr));
  for (const d of chs) {
    const n = d.length;
    for (let i = 0; i < n; i++) {
      let g = peak / m;
      if (i < fi) g *= i / fi;
      if (i > n - fo) g *= (n - i) / fo;
      d[i] *= g;
    }
  }
  return chs;
}

// ── Drum voices ──────────────────────────────────────────────────

// Sine body with a pitch drop, a click transient and a little saturation.
function kick(sr, { f0 = 170, f1 = 48, pt = 0.035, decay = 0.3, len = 0.6, click = 0.5, drive = 1.6, tone = 0 }, rnd) {
  const n = Math.floor(len * sr), d = new Float32Array(n);
  const hp = new Biquad('hp', 1800, 0.7, sr), lp = new Biquad('lp', 7000, 0.7, sr);
  let ph = 0;
  for (let i = 0; i < n; i++) {
    const t = i / sr;
    const f = f1 + (f0 - f1) * Math.exp(-t / pt);
    ph += (TAU * f) / sr;
    // The body holds a touch longer than a plain exponential, then falls away.
    const env = Math.exp(-t / decay) * (1 - Math.exp(-t / 0.0012));
    let s = Math.sin(ph) * env + tone * Math.sin(ph * 2) * env * Math.exp(-t / 0.05);
    const cl = lp.p(hp.p(rnd() * 2 - 1)) * Math.exp(-t / 0.004) * click;
    s = sat(s * 1.1 + cl, drive);
    d[i] = s;
  }
  return finish([d], sr, 0.95, 0.0003, 0.02);
}

// Tuned body, noise snares, an optional gated "plate" tail (the 80s sound).
function snare(sr, { body = 190, decay = 0.16, noise = 1, bright = 5200, gate = 0, gateLen = 0.26, crack = 0.6, len = 0.5 }, rnd, stereo = true) {
  const n = Math.floor(len * sr);
  const out = [];
  for (let c = 0; c < (stereo ? 2 : 1); c++) {
    const d = new Float32Array(n);
    const hp = new Biquad('hp', 900, 0.7, sr), lp = new Biquad('lp', bright, 0.6, sr);
    const cr = new Biquad('bp', 3800, 1.2, sr);
    const tl = new Biquad('lp', 5200, 0.5, sr), th = new Biquad('hp', 350, 0.5, sr);
    let p1 = 0, p2 = 0;
    for (let i = 0; i < n; i++) {
      const t = i / sr;
      const pd = 1 + 0.25 * Math.exp(-t / 0.012);
      p1 += (TAU * body * pd) / sr; p2 += (TAU * body * 1.72 * pd) / sr;
      const tone = (Math.sin(p1) * 0.8 + Math.sin(p2) * 0.45) * Math.exp(-t / 0.055);
      const w = rnd() * 2 - 1;
      const nz = lp.p(hp.p(w)) * Math.exp(-t / decay) * noise;
      const ck = cr.p(w) * Math.exp(-t / 0.005) * crack * 3;
      let tail = 0;
      if (gate) {
        // A dense reverb tail held flat, then chopped: the gate.
        const g = t < 0.012 ? t / 0.012 : t < gateLen ? 1 - 0.35 * (t / gateLen) : Math.max(0, 1 - (t - gateLen) / 0.03) * 0.65;
        tail = th.p(tl.p(rnd() * 2 - 1)) * g * gate;
      }
      d[i] = sat(tone + nz + ck + tail, 1.3);
    }
    out.push(d);
  }
  return finish(out, sr, 0.9);
}

function clap(sr, { tail = 0.14, freq = 1150, len = 0.45 }, rnd) {
  const n = Math.floor(len * sr);
  const out = [];
  for (let c = 0; c < 2; c++) {
    const d = new Float32Array(n);
    const bp = new Biquad('bp', freq, 1.1, sr), hp = new Biquad('hp', 600, 0.7, sr);
    const hits = [0, 0.0105 + c * 0.0015, 0.021, 0.0325 - c * 0.001];
    for (let i = 0; i < n; i++) {
      const t = i / sr;
      let e = 0;
      for (let k = 0; k < hits.length; k++) { const u = t - hits[k]; if (u >= 0 && u < 0.02) e = Math.max(e, Math.exp(-u / 0.0038) * (k === 3 ? 1 : 0.8)); }
      const last = t - hits[3];
      if (last > 0) e = Math.max(e, Math.exp(-last / tail) * 0.55);
      d[i] = hp.p(bp.p(rnd() * 2 - 1)) * e * 3;
    }
    out.push(d);
  }
  return finish(out, sr, 0.85);
}

// 808-style metal: six detuned square waves, band-limited to the top end,
// plus some noise. Used for hats, the ride and the crash.
const METAL = [205.3, 304.4, 369.6, 522.7, 540, 800];
function metal(sr, { decay = 0.03, len = 0.15, hpF = 7000, mix = 0.45, scale = 1, attack = 0.0005, darken = 0, bell = 0 }, rnd, stereo = true) {
  const n = Math.floor(len * sr);
  const out = [];
  for (let c = 0; c < (stereo ? 2 : 1); c++) {
    const d = new Float32Array(n);
    const hp1 = new Biquad('hp', hpF, 0.7, sr), hp2 = new Biquad('hp', hpF, 0.7, sr);
    const lp = new Biquad('lp', 15000, 0.6, sr);
    const bp = new Biquad('bp', hpF * 1.4, 0.9, sr);
    const ph = METAL.map(() => rnd());
    const fr = METAL.map((f) => f * scale * (1 + (c ? 0.004 : -0.004)));
    for (let i = 0; i < n; i++) {
      const t = i / sr;
      let sq = 0;
      for (let k = 0; k < 6; k++) { ph[k] += fr[k] / sr; sq += (ph[k] % 1) < 0.5 ? 1 : -1; }
      let s = sq / 6 * (1 - mix) + (rnd() * 2 - 1) * mix;
      if (darken) lp.set('lp', 15000 - darken * Math.min(1, t / len) * 10000, 0.6, sr);
      s = lp.p(hp2.p(hp1.p(s)) + bp.p(s) * 0.5);
      let b = 0;
      if (bell) b = (Math.sin(TAU * 1240 * scale * t) + 0.6 * Math.sin(TAU * 3170 * scale * t) + 0.35 * Math.sin(TAU * 5120 * scale * t)) * Math.exp(-t / (decay * 0.6)) * bell;
      const env = (t < attack ? t / attack : 1) * Math.exp(-t / decay);
      d[i] = s * env + b * 0.2;
    }
    out.push(d);
  }
  return finish(out, sr, 0.8);
}

function tom(sr, { f = 110, decay = 0.28, len = 0.7, pan = 0 }, rnd) {
  const n = Math.floor(len * sr);
  const L = new Float32Array(n), R = new Float32Array(n);
  const bp = new Biquad('bp', f * 4, 1, sr);
  let ph = 0;
  for (let i = 0; i < n; i++) {
    const t = i / sr;
    ph += (TAU * f * (1 + 0.6 * Math.exp(-t / 0.03))) / sr;
    const s = sat(Math.sin(ph) * Math.exp(-t / decay) + bp.p(rnd() * 2 - 1) * Math.exp(-t / 0.02) * 0.6, 1.5);
    L[i] = s * (1 - Math.max(0, pan)); R[i] = s * (1 + Math.min(0, pan));
  }
  return finish([L, R], sr, 0.85);
}

function shaker(sr, rnd) {
  const n = Math.floor(0.12 * sr), out = [];
  for (let c = 0; c < 2; c++) {
    const d = new Float32Array(n);
    const bp = new Biquad('bp', 6800 + c * 400, 1.1, sr), hp = new Biquad('hp', 3500, 0.7, sr);
    for (let i = 0; i < n; i++) {
      const t = i / sr;
      const e = (t < 0.012 ? t / 0.012 : Math.exp(-(t - 0.012) / 0.03));
      d[i] = hp.p(bp.p(rnd() * 2 - 1)) * e;
    }
    out.push(d);
  }
  return finish(out, sr, 0.8, 0.001);
}

// Rim-shot / finger snap: a short resonant click.
function rim(sr, { f = 1750, body = 480, len = 0.08, q = 4 }, rnd) {
  const n = Math.floor(len * sr), d = new Float32Array(n);
  const bp = new Biquad('bp', f, q, sr);
  for (let i = 0; i < n; i++) {
    const t = i / sr;
    d[i] = bp.p(rnd() * 2 - 1) * Math.exp(-t / 0.01) * 4 + Math.sin(TAU * body * t) * Math.exp(-t / 0.012);
  }
  return finish([d], sr, 0.8, 0.0002, 0.005);
}

// Sub drop for the downbeat of a drop: a long falling sine and a noise swell.
function boom(sr, rnd) {
  const n = Math.floor(2.4 * sr), L = new Float32Array(n), R = new Float32Array(n);
  const lp = new Biquad('lp', 300, 0.7, sr), lp2 = new Biquad('lp', 300, 0.7, sr);
  let ph = 0;
  for (let i = 0; i < n; i++) {
    const t = i / sr;
    ph += (TAU * (32 + 40 * Math.exp(-t / 0.25))) / sr;
    const s = Math.sin(ph) * Math.exp(-t / 0.7);
    const e = Math.exp(-t / 0.35);
    L[i] = sat(s + lp.p(rnd() * 2 - 1) * e * 1.5, 1.4);
    R[i] = sat(s + lp2.p(rnd() * 2 - 1) * e * 1.5, 1.4);
  }
  return finish([L, R], sr, 0.9);
}

function reversed(chs) {
  return chs.map((d) => { const r = new Float32Array(d.length); for (let i = 0; i < d.length; i++) r[i] = d[d.length - 1 - i]; return r; });
}

function toBuffer(ctx, chs) {
  const b = ctx.createBuffer(chs.length, chs[0].length, ctx.sampleRate);
  chs.forEach((d, c) => b.copyToChannel ? b.copyToChannel(d, c) : b.getChannelData(c).set(d));
  return b;
}

// The whole drum kit, as AudioBuffers keyed by name.
export function renderKit(ctx) {
  const sr = ctx.sampleRate, r = rngFrom(1234);
  const crash = metal(sr, { decay: 0.85, len: 2.6, hpF: 3600, mix: 0.7, scale: 1.9, attack: 0.002, darken: 0.6 }, r);
  const kit = {
    kickPunch: kick(sr, { f0: 175, f1: 49, pt: 0.032, decay: 0.26, len: 0.55, click: 0.55, drive: 1.8 }, r),
    kickBoom: kick(sr, { f0: 150, f1: 42, pt: 0.05, decay: 0.55, len: 1.1, click: 0.35, drive: 2.2, tone: 0.2 }, r),
    kickTight: kick(sr, { f0: 230, f1: 56, pt: 0.022, decay: 0.16, len: 0.4, click: 0.8, drive: 2.0 }, r),
    kickSoft: kick(sr, { f0: 120, f1: 50, pt: 0.04, decay: 0.24, len: 0.5, click: 0.12, drive: 1.1 }, r),
    kickHouse: kick(sr, { f0: 160, f1: 52, pt: 0.028, decay: 0.3, len: 0.6, click: 0.45, drive: 2.4, tone: 0.1 }, r),
    snareGated: snare(sr, { body: 180, decay: 0.12, noise: 0.9, bright: 7000, gate: 0.75, gateLen: 0.27, crack: 0.5, len: 0.42 }, r),
    snareCrisp: snare(sr, { body: 225, decay: 0.09, noise: 1, bright: 9500, crack: 1, len: 0.3 }, r),
    snareFat: snare(sr, { body: 170, decay: 0.2, noise: 1.1, bright: 6000, gate: 0.4, gateLen: 0.18, crack: 0.6, len: 0.5 }, r),
    snareSoft: snare(sr, { body: 200, decay: 0.08, noise: 0.55, bright: 4200, crack: 0.2, len: 0.25 }, r),
    clap: clap(sr, {}, r),
    clapBig: clap(sr, { tail: 0.3, freq: 1000, len: 0.8 }, r),
    hat: metal(sr, { decay: 0.022, len: 0.1, hpF: 7500, mix: 0.4 }, r),
    hatSoft: metal(sr, { decay: 0.018, len: 0.08, hpF: 8500, mix: 0.7 }, r),
    ohat: metal(sr, { decay: 0.2, len: 0.55, hpF: 6500, mix: 0.45 }, r),
    ride: metal(sr, { decay: 0.7, len: 1.8, hpF: 4200, mix: 0.3, scale: 1.4, bell: 0.8 }, r),
    crash,
    revCrash: reversed(crash),
    shaker: shaker(sr, r),
    rim: rim(sr, {}, r),
    snap: rim(sr, { f: 2300, body: 900, q: 2.5, len: 0.07 }, r),
    tomL: tom(sr, { f: 82, decay: 0.32, pan: 0.35 }, r),
    tomM: tom(sr, { f: 116, decay: 0.28, pan: 0 }, r),
    tomH: tom(sr, { f: 158, decay: 0.24, pan: -0.35 }, r),
    boom: boom(sr, r),
  };
  const out = {};
  for (const [k, v] of Object.entries(kit)) out[k] = toBuffer(ctx, v);
  return out;
}

// ── SFX one-shots ────────────────────────────────────────────────

// Crumpled sheet metal: a handful of inharmonic panel modes rung by a noisy
// excitation that keeps crackling (the crumple) for a moment.
function metalHit(sr, seed, { len = 1.0, lo = 160, hi = 4200, crumple = 0.25 } = {}) {
  const r = rngFrom(seed);
  const n = Math.floor(len * sr), out = [];
  const modes = [];
  for (let k = 0; k < 14; k++) {
    const f = lo * Math.pow(hi / lo, r());
    modes.push({ f, d: 0.04 + 0.5 * Math.pow(lo / f, 0.6) * r(), a: 0.4 + r(), bp: null });
  }
  for (let c = 0; c < 2; c++) {
    const d = new Float32Array(n);
    const bps = modes.map((m) => new Biquad('bp', m.f * (1 + (c ? 0.006 : -0.006)), 30 + m.f / 60, sr));
    const grit = new Biquad('bp', 2400, 0.8, sr);
    let crk = 0;
    for (let i = 0; i < n; i++) {
      const t = i / sr;
      // Excitation: a hard hit, then random crackle bursts while it crumples.
      if (t < crumple && r() < 0.0025) crk = 0.6 + r();
      crk *= 0.993;
      const w = r() * 2 - 1;
      const ex = w * (Math.exp(-t / 0.006) * 3 + crk * (1 - t / crumple > 0 ? 1 - t / crumple : 0));
      let s = 0;
      for (let k = 0; k < modes.length; k++) {
        const m = modes[k];
        s += bps[k].p(ex) * m.a * Math.exp(-t / m.d) * 6;
      }
      s += grit.p(w) * (Math.exp(-t / 0.05) + crk * 0.3) * 0.7;
      d[i] = sat(s, 1.6);
    }
    out.push(d);
  }
  return finish(out, sr, 0.9, 0.0002, 0.05);
}

// Glass: a sharp shatter and then pieces landing — dozens of tiny high
// ringing tinkles thinning out over half a second.
function glass(sr, seed, len = 1.0) {
  const r = rngFrom(seed);
  const n = Math.floor(len * sr);
  const L = new Float32Array(n), R = new Float32Array(n);
  const hp = new Biquad('hp', 3500, 0.7, sr);
  for (let i = 0; i < Math.floor(0.08 * sr); i++) {
    const t = i / sr, s = hp.p(r() * 2 - 1) * Math.exp(-t / 0.02);
    L[i] += s; R[i] += s * 0.8;
  }
  for (let k = 0; k < 55; k++) {
    const t0 = Math.pow(r(), 1.8) * (len * 0.7);
    const f = 2800 + r() * 6500, dec = 0.01 + r() * 0.05, a = (0.15 + r() * 0.5) * (1 - t0 / len);
    const pan = r() * 2 - 1;
    const i0 = Math.floor(t0 * sr), m = Math.floor(dec * 6 * sr);
    for (let i = 0; i < m && i0 + i < n; i++) {
      const t = i / sr;
      const s = (Math.sin(TAU * f * t) + 0.5 * Math.sin(TAU * f * 2.37 * t)) * Math.exp(-t / dec) * a;
      L[i0 + i] += s * (1 - pan) * 0.5; R[i0 + i] += s * (1 + pan) * 0.5;
    }
  }
  return finish([L, R], sr, 0.8);
}

// Seamless looping textures: generate twice as long, filter it all so the
// filters are settled, and keep the second half (whose end runs straight
// back into its start).
function loopTexture(sr, secs, seed, gen) {
  const n = Math.floor(secs * sr), r = rngFrom(seed);
  const out = [];
  for (let c = 0; c < 2; c++) {
    const full = gen(n, r, c);
    out.push(full.slice(n, 2 * n));
  }
  // Cross-fade the seam anyway; grains straddling it would otherwise click.
  const f = 1024;
  for (const d of out) for (let i = 0; i < f; i++) { const t = i / f; d[n - f + i] = d[n - f + i] * (1 - t) + d[i] * t; }
  return finish(out, sr, 0.8, 0, 0);
}

// Gravel under the tyres: dense random stone clicks of different sizes over a
// bed of low crunch. Played faster at speed.
function gravel(sr) {
  return loopTexture(sr, 2, 77, (n, r) => {
    const x = new Float32Array(2 * n);
    // Grains land in the kept (second) half and wrap around its end.
    const grains = Math.floor(n / sr * 900);
    for (let k = 0; k < grains; k++) {
      const at = Math.floor(r() * n), size = r();
      const f = 700 + (1 - size) * 4200, dec = 0.001 + size * 0.004, a = 0.2 + size * 0.8;
      const bp = new Biquad('bp', f, 2.5, sr);
      const m = Math.floor(dec * 5 * sr);
      for (let i = 0; i < m; i++) x[n + ((at + i) % n)] += bp.p(r() * 2 - 1) * Math.exp(-i / sr / dec) * a;
    }
    const lp = new Biquad('lp', 380, 0.7, sr);
    let br = 0;
    for (let i = 0; i < 2 * n; i++) { br = br * 0.97 + (r() * 2 - 1) * 0.03; x[i] = x[i] * 1.4 + lp.p(br) * 5; }
    return x;
  });
}

// Metal on concrete: a grinding excitation through a few ringing panel modes.
function scrape(sr) {
  return loopTexture(sr, 1.5, 91, (n, r, c) => {
    const x = new Float32Array(2 * n);
    const modes = [1450, 2330, 3170, 4480, 6100].map((f) => new Biquad('bp', f * (1 + c * 0.01), 25, sr));
    const grit = new Biquad('bp', 2800, 0.7, sr), lpA = new Biquad('lp', 30, 0.7, sr);
    for (let i = 0; i < 2 * n; i++) {
      // Grind intensity wobbles quickly and randomly (the car bounces along the wall).
      const a = Math.max(0, 0.6 + lpA.p((r() * 2 - 1) * 40));
      const w = (r() * 2 - 1) * a;
      let s = grit.p(w) * 0.9;
      for (const m of modes) s += m.p(w) * 3;
      x[i] = sat(s, 1.8);
    }
    return x;
  });
}

// Gearbox clunk: the dog rings engaging — a woody "thock" with a metal tick.
function clunk(sr) {
  const r = rngFrom(5), n = Math.floor(0.16 * sr), d = new Float32Array(n);
  const tick = new Biquad('bp', 3400, 3, sr);
  for (let i = 0; i < n; i++) {
    const t = i / sr;
    const s = Math.sin(TAU * 165 * t) * Math.exp(-t / 0.03) + 0.6 * Math.sin(TAU * 430 * t) * Math.exp(-t / 0.018)
      + 0.35 * Math.sin(TAU * 960 * t) * Math.exp(-t / 0.012) + tick.p(r() * 2 - 1) * Math.exp(-t / 0.003) * 4;
    d[i] = sat(s, 1.3);
  }
  return finish([d], sr, 0.9, 0.0002, 0.01);
}

// Suspension bottoming out: a deep thump with a spring's metallic ring.
function thud(sr) {
  const r = rngFrom(9), n = Math.floor(0.5 * sr), d = new Float32Array(n);
  const lp = new Biquad('lp', 240, 0.8, sr), ring = new Biquad('bp', 780, 18, sr);
  let ph = 0;
  for (let i = 0; i < n; i++) {
    const t = i / sr;
    ph += (TAU * (46 + 70 * Math.exp(-t / 0.03))) / sr;
    const w = r() * 2 - 1;
    d[i] = sat(Math.sin(ph) * Math.exp(-t / 0.16) + lp.p(w) * Math.exp(-t / 0.05) * 2 + ring.p(w) * Math.exp(-t / 0.12) * 0.5, 1.4);
  }
  return finish([d], sr, 0.9, 0.0005, 0.03);
}

export function renderSfx(ctx) {
  const sr = ctx.sampleRate;
  const s = {
    metal1: metalHit(sr, 3), metal2: metalHit(sr, 8, { lo: 120, hi: 3000, crumple: 0.35 }), metal3: metalHit(sr, 13, { lo: 260, hi: 5200, crumple: 0.15, len: 0.8 }),
    glass1: glass(sr, 4), glass2: glass(sr, 17, 0.8),
    gravel: gravel(sr), scrape: scrape(sr), clunk: clunk(sr), thud: thud(sr),
  };
  const out = {};
  for (const [k, v] of Object.entries(s)) out[k] = toBuffer(ctx, v);
  return out;
}
