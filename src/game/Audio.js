// All game audio, synthesized with WebAudio — no sample files.
//
// Graph:
//   engine chains ─┐
//   effects ───────┴─ sfxBus ─ sfxComp ─ sfxVol ─┐
//                        └─ tunnel reverb ─┘     ├─ master ─ limiter ─ out
//   music ─ musicMix ─ musicComp ─ gate ─ duck ─ musicVol┘
//
// The music itself (songs, sequencer, instruments, drum kit) lives in
// audio/Music.js and audio/tracks.js; it plays into musicIn. Crash, gravel,
// scrape, clunk and thud sounds are pre-rendered once (audio/samples.js).
//
// Music and SFX are separate buses with their own compressors, so a loud
// engine never pumps the music down; the master only has a peak limiter.
//
// The engine is a wavetable: one full 720° four-stroke cycle is drawn with a
// pressure pulse per cylinder — timed and weighted by the engine's bank
// layout, which is where a cross-plane V8's lumpy burble comes from — and
// turned into a band-limited PeriodicWave played at the cycle rate
// (rpm / 120). On- and off-throttle cycles are separate waves crossfaded by
// load, run through exhaust formant filters, and doubled on a second,
// slightly detuned chain for stereo width. A separate rumble layer carries
// the low end (see rumbleCycle). Continuous layers are built once and only
// steered in update(); one-shots create short-lived nodes.

import { Music } from './audio/Music.js';
import { renderSfx } from './audio/samples.js';

const clamp = (v, lo, hi) => (v < lo ? lo : v > hi ? hi : v);
const mtof = (m) => 440 * Math.pow(2, (m - 69) / 12);

function distortionCurve(amount, n = 1024) {
  const c = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    const x = (i * 2) / (n - 1) - 1;
    c[i] = Math.tanh(amount * x) / Math.tanh(amount);
  }
  return c;
}

// Asymmetric soft clip: exhaust pressure pulses compress harder on the
// positive swing, which adds even harmonics (warmth).
function exhaustCurve(amount, n = 2048) {
  const c = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    const x = (i * 2) / (n - 1) - 1;
    const y = x >= 0 ? Math.tanh(amount * x) : Math.tanh(amount * 0.6 * x) / 0.8;
    c[i] = y / Math.tanh(amount);
  }
  return c;
}

function rngFrom(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// ── Engine characters ────────────────────────────────────────────
// banks: which exhaust bank each firing event in the cycle belongs to (firing
// order mapped to banks). Cross-plane V8 (1-8-4-3-6-5-7-2) fires L R R L R L
// L R — two same-bank pulses in a row — which is the burble.
const CARS = {
  sports: { // flat-plane V8: banks alternate evenly, crisp and howly
    cyl: 8, banks: [0, 1, 0, 1, 0, 1, 0, 1],
    bankAmp: [1, 0.9], bankDelay: [0, 0.004], jitter: 0.03, ampVar: 0.12,
    pulseW: 0.34, sharp: 1.8, lowBoost: 1.0, rumble: 1.0,
    formants: [[190, 6, 1.3], [640, 4, 1.8], [1850, 4, 2.4]],
    lpBase: 520, lpRange: 5200, drive: 1.5, trim: 1.1,
    pops: 1.0, popFreq: [650, 1700], intake: 0.9, rasp: 0.9, whine: 1,
  },
  muscle: { // big cross-plane V8: deep, lumpy, loud pops
    cyl: 8, banks: [0, 1, 1, 0, 1, 0, 0, 1],
    bankAmp: [1, 0.72], bankDelay: [0, 0.014], jitter: 0.07, ampVar: 0.22,
    pulseW: 0.46, sharp: 1.4, lowBoost: 1.7, rumble: 1.1,
    formants: [[105, 7, 1.1], [360, 5, 1.6], [920, 3, 2.2]],
    lpBase: 360, lpRange: 3300, drive: 2.1, trim: 0.9,
    pops: 1.7, popFreq: [380, 1100], intake: 1.1, rasp: 0.7, whine: 1.3,
  },
  super: { // V10: high, hard-edged scream
    cyl: 10, banks: [0, 1, 0, 1, 0, 1, 0, 1, 0, 1],
    bankAmp: [1, 0.94], bankDelay: [0, 0.003], jitter: 0.025, ampVar: 0.09,
    pulseW: 0.28, sharp: 2.2, lowBoost: 0.8, rumble: 0.75,
    formants: [[300, 4, 1.4], [1150, 5, 2.0], [3100, 5, 2.6]],
    lpBase: 900, lpRange: 7800, drive: 1.35, trim: 1.2,
    pops: 0.6, popFreq: [1100, 2600], intake: 1.2, rasp: 1.2, whine: 0.8,
  },
  rally: { // turbo inline-4: even, buzzy and raspy, anti-lag crackle, turbo whistle
    cyl: 4, banks: [0, 0, 0, 0],
    bankAmp: [1, 1], bankDelay: [0, 0], jitter: 0.045, ampVar: 0.16,
    pulseW: 0.4, sharp: 1.7, lowBoost: 1.15, rumble: 0.7,
    formants: [[170, 5, 1.2], [560, 5, 1.7], [1650, 5, 2.3]],
    lpBase: 640, lpRange: 5600, drive: 2.0, trim: 1.05,
    pops: 2.2, popFreq: [800, 2300], intake: 1.3, rasp: 1.35, whine: 0.9, turbo: 1,
  },
  // No combustion: the engine chains fall silent and the motor layers
  // (_buildElectric) take over. The fields below only feed the pops code.
  electric: { electric: true, cyl: 8, pops: 0, popFreq: [800, 2000] },
};

const WAVE_SAMPLES = 2048;
const WAVE_HARMONICS = 220;
let COS = null, SIN = null;

// Draw one engine cycle and return its Fourier series.
function engineCycle(prof, seed, off) {
  const M = WAVE_SAMPLES;
  if (!COS) {
    COS = new Float32Array(M); SIN = new Float32Array(M);
    for (let i = 0; i < M; i++) { COS[i] = Math.cos((2 * Math.PI * i) / M); SIN[i] = Math.sin((2 * Math.PI * i) / M); }
  }
  const rng = rngFrom(seed);
  const x = new Float32Array(M);
  const n = prof.cyl;
  const w = (prof.pulseW / n) * (off ? 1.7 : 1);
  const sharp = off ? 0.9 : prof.sharp;
  const ampVar = prof.ampVar * (off ? 2.2 : 1);
  for (let k = 0; k < n; k++) {
    const bank = prof.banks[k];
    const t0 = k / n + prof.bankDelay[bank] + (rng() - 0.5) * prof.jitter / n;
    const amp = prof.bankAmp[bank] * (1 + (rng() - 0.5) * ampVar) * (off ? 0.6 : 1);
    for (let i = 0; i < M; i++) {
      let tau = i / M - t0;
      tau -= Math.floor(tau);
      const u = tau / w;
      if (u > 7) continue;
      // Gamma-like blowdown pulse, then a small reflected rarefaction.
      const p = Math.pow(u * Math.exp(1 - u), sharp);
      const refl = u > 1.2 ? -0.22 * Math.exp(-(u - 2.6) * (u - 2.6)) : 0;
      x[i] += amp * (p + refl);
    }
  }
  if (off) {
    // Overrun: irregular soft burbles between pulses.
    for (let b = 0; b < n; b++) {
      const t0 = rng(), a = 0.15 + rng() * 0.25, bw = w * (0.6 + rng());
      for (let i = 0; i < M; i++) {
        let tau = i / M - t0; tau -= Math.floor(tau);
        const u = tau / bw;
        if (u < 5) x[i] += a * u * Math.exp(1 - u);
      }
    }
  }
  const H = WAVE_HARMONICS;
  const real = new Float32Array(H + 1), imag = new Float32Array(H + 1);
  for (let h = 1; h <= H; h++) {
    let re = 0, im = 0, idx = 0;
    for (let i = 0; i < M; i++) {
      re += x[i] * COS[idx];
      im += x[i] * SIN[idx];
      idx += h; if (idx >= M) idx -= M;
    }
    let g = 2 / M;
    // Orders below the firing frequency carry the burble; the very lowest
    // (below half-order) are mostly sub-bass mud, so taper them off.
    if (h < n) g *= prof.lowBoost;
    if (h < n / 2) g *= Math.pow(h / (n / 2), 1.5);
    if (off) g *= Math.exp(-h / 70);
    real[h] = re * g;
    imag[h] = im * g;
  }
  return { real, imag };
}

// The low end, drawn separately: every order up to twice the firing order,
// tilted toward the bottom, with extra weight on the half and full firing
// orders, and the engine's own phases so it lines up with the exhaust
// pulses. Through a fixed low-pass that leaves the sub-firing lope at racing
// revs and the firing thump near idle.
function rumbleCycle(prof, { real, imag }) {
  const n = prof.cyl, H = 2 * n;
  const re = new Float32Array(H + 1), im = new Float32Array(H + 1);
  for (let h = 1; h <= H; h++) {
    let w = h === 1 ? 0.9 : 1 / Math.sqrt(h); // order 1: the deepest, felt more than heard
    if (h === n / 2 || h === n) w *= 1.6;
    if (h < n / 2) w *= prof.lowBoost; // cross-plane lope
    const ph = Math.atan2(imag[h], real[h]);
    re[h] = w * Math.cos(ph);
    im[h] = w * Math.sin(ph);
  }
  return { real: re, imag: im };
}

// Internal trims (calibrated with the analyser measurements in the bench).
const MASTER_TRIM = 0.64;
const SFX_TRIM = 1.25;
const MUSIC_TRIM = 2.4;
const RUMBLE = 4; // engine rumble layer, relative to the exhaust chains

export class GameAudio {
  constructor() {
    this.ctx = null;
    this._initPromise = null;
    this._vol = { master: 1, sfx: 0.85, music: 0.7 };
    this._car = 'sports';
    this._env = 'open';
    this._prevThrottle = 0;
    this._popCooldown = 0;
    this._musicOn = false;
    this._musicTimer = null;
    this._paused = false;
    this._limiting = false;
    this.onTrackChange = null; // (info) => void when a music track starts
    this._track = null; // requested track id (null = keep / level default)
    this._impactT = -1;
  }

  get ready() { return !!this.ctx && this._built === true; }
  get car() { return this._car; }
  get environment() { return this._env; }

  // opts.context: run on a given context (tests render an OfflineAudioContext).
  //
  // Never waits for the context to start. One made outside a user gesture
  // (on a phone, a tap's pointerdown doesn't count) starts suspended, and a
  // resume() made there doesn't settle until a later one succeeds. The graph
  // builds fine on a suspended context; unlock() starts it from a gesture.
  async init(opts = {}) {
    if (this._initPromise) return this._initPromise;
    this._initPromise = (async () => {
      const AC = window.AudioContext || window.webkitAudioContext;
      if (!AC && !opts.context) return;
      const ctx = opts.context || new AC({ latencyHint: 'interactive' });
      this.ctx = ctx;
      this._build();
      this._built = true;
      this.setCar(this._car);
      this.setEnvironment(this._env, true);
      if (this._track) this.music.play(this._track);
      if (this._musicWanted) this.setMusic(true);
      if (!opts.context) this.unlock();
    })();
    return this._initPromise;
  }

  // Start the context if it isn't running (it can also be interrupted later,
  // e.g. by a phone call on iOS). Call it from inside a user gesture: a tap's
  // pointerup, touchend or click, or a key. While paused it stays suspended.
  unlock() {
    const ctx = this.ctx;
    if (!ctx || this._paused || ctx.state === 'running' || ctx.state === 'closed') return;
    ctx.resume().catch(() => {});
  }

  // Independent 0..1 volumes. sfx covers the engine and every effect.
  setVolume({ master, sfx, music } = {}) {
    if (master !== undefined) this._vol.master = clamp(master, 0, 1);
    if (sfx !== undefined) this._vol.sfx = clamp(sfx, 0, 1);
    if (music !== undefined) this._vol.music = clamp(music, 0, 1);
    if (this.ready) this._applyVolumes(0.03);
  }

  _applyVolumes(tc) {
    const t = this.ctx.currentTime;
    this.master.gain.setTargetAtTime(MASTER_TRIM * this._vol.master, t, tc);
    this.sfxVol.gain.setTargetAtTime(SFX_TRIM * this._vol.sfx, t, tc);
    this.musicVol.gain.setTargetAtTime(MUSIC_TRIM * this._vol.music, t, tc);
  }

  // Engine character: 'sports' (flat-plane V8), 'muscle' (cross-plane V8),
  // 'super' (V10), 'rally' (turbo four), 'electric' (motor, no engine).
  // Can be switched live.
  setCar(kind = 'sports') {
    if (!CARS[kind]) kind = 'sports';
    this._car = kind;
    if (!this.ready) return;
    const prof = CARS[kind];
    this.prof = prof;
    this.electric = !!prof.electric;
    // Rivals are all combustion cars, whatever the player drives.
    this._rivalWave ||= (() => { const { real, imag } = engineCycle(CARS.sports, 21, false); return this.ctx.createPeriodicWave(real, imag); })();
    for (const r of this.rivals) if (!r.ev) r.osc.setPeriodicWave(this._rivalWave);
    if (this.electric) return;
    const ctx = this.ctx, t = ctx.currentTime;
    const key = kind;
    this._waves = this._waves || {};
    if (!this._waves[key]) {
      const mk = (seed, off) => {
        const { real, imag } = engineCycle(prof, seed, off);
        return ctx.createPeriodicWave(real, imag);
      };
      const rum = rumbleCycle(prof, engineCycle(prof, 11, false));
      this._waves[key] = { onL: mk(11, false), offL: mk(12, true), onR: mk(21, false), offR: mk(22, true), rumble: ctx.createPeriodicWave(rum.real, rum.imag) };
    }
    const w = this._waves[key];
    const e = this.eng;
    e.L.on.setPeriodicWave(w.onL); e.L.off.setPeriodicWave(w.offL);
    e.R.on.setPeriodicWave(w.onR); e.R.off.setPeriodicWave(w.offR);
    e.rum.setPeriodicWave(w.rumble);
    for (const side of [e.L, e.R]) {
      side.f.forEach((f, i) => {
        const [freq, gain, q] = prof.formants[i];
        const spread = side === e.R ? 1.04 : 1;
        f.frequency.setTargetAtTime(freq * spread, t, 0.05);
        f.gain.setTargetAtTime(gain, t, 0.05);
        f.Q.setTargetAtTime(q, t, 0.05);
      });
    }
  }

  // 'tunnel' blends in a short, dense concrete-box reverb on all SFX.
  setEnvironment(env = 'open', immediate = false) {
    this._env = env === 'tunnel' ? 'tunnel' : 'open';
    if (!this.ready) return;
    const t = this.ctx.currentTime, tc = immediate ? 0.001 : 0.25;
    const on = this._env === 'tunnel';
    this.envSend.gain.setTargetAtTime(on ? 0.75 : 0, t, tc);
    this.envLow.gain.setTargetAtTime(on ? 3 : 0, t, tc);
  }

  // ── Graph construction ───────────────────────────────────────────
  _build() {
    const ctx = this.ctx;
    const limiter = ctx.createDynamicsCompressor();
    limiter.threshold.value = -1.5;
    limiter.knee.value = 0;
    limiter.ratio.value = 20;
    limiter.attack.value = 0.002;
    limiter.release.value = 0.12;
    limiter.connect(ctx.destination);
    this.limiter = limiter;
    this.master = ctx.createGain();
    this.master.connect(limiter);

    // SFX bus: its own glue compressor, then the user volume.
    this.sfxBus = ctx.createGain();
    this.sfxComp = ctx.createDynamicsCompressor();
    this.sfxComp.threshold.value = -13; this.sfxComp.knee.value = 8;
    this.sfxComp.ratio.value = 3; this.sfxComp.attack.value = 0.005; this.sfxComp.release.value = 0.18;
    this.envLow = ctx.createBiquadFilter(); this.envLow.type = 'lowshelf'; this.envLow.frequency.value = 180; this.envLow.gain.value = 0;
    this.sfxVol = ctx.createGain();
    this.sfxBus.connect(this.envLow); this.envLow.connect(this.sfxComp);
    this.sfxComp.connect(this.sfxVol); this.sfxVol.connect(this.master);

    // Music bus: mix → gentle compressor → on/off gate → user volume.
    this.musicIn = ctx.createBiquadFilter(); this.musicIn.type = 'highpass'; this.musicIn.frequency.value = 36;
    this.musicMix = ctx.createGain();
    this.musicIn.connect(this.musicMix);
    this.musicComp = ctx.createDynamicsCompressor();
    this.musicComp.threshold.value = -14; this.musicComp.knee.value = 12;
    this.musicComp.ratio.value = 2; this.musicComp.attack.value = 0.004; this.musicComp.release.value = 0.25;
    this.musicGate = ctx.createGain(); this.musicGate.gain.value = 0;
    this.musicDuck = ctx.createGain(); // dips under the finish fanfare
    this.musicVol = ctx.createGain();
    this.musicMix.connect(this.musicComp); this.musicComp.connect(this.musicGate);
    this.musicGate.connect(this.musicDuck); this.musicDuck.connect(this.musicVol); this.musicVol.connect(this.master);

    this._makeNoise();
    this.sfxBuf = renderSfx(ctx);
    this._buildTunnel();
    this._buildEngine();
    this._buildTurbo();
    this._buildElectric();
    this._buildEnvironment();
    this._buildRivals();
    this.music = new Music(ctx, this.musicIn);
    this.music.build();
    this.music.onTrack = (info) => this.onTrackChange?.(info);
    this._applyVolumes(0.001);
  }

  _makeNoise() {
    const ctx = this.ctx;
    const len = Math.floor(ctx.sampleRate * 2);
    const white = ctx.createBuffer(1, len, ctx.sampleRate);
    const pink = ctx.createBuffer(1, len, ctx.sampleRate);
    const brown = ctx.createBuffer(1, len, ctx.sampleRate);
    const w = white.getChannelData(0), p = pink.getChannelData(0), b = brown.getChannelData(0);
    let b0 = 0, b1 = 0, b2 = 0, b3 = 0, b4 = 0, b5 = 0, b6 = 0, last = 0;
    for (let i = 0; i < len; i++) {
      const r = Math.random() * 2 - 1;
      w[i] = r;
      b0 = 0.99886 * b0 + r * 0.0555179; b1 = 0.99332 * b1 + r * 0.0750759;
      b2 = 0.969 * b2 + r * 0.153852; b3 = 0.8665 * b3 + r * 0.3104856;
      b4 = 0.55 * b4 + r * 0.5329522; b5 = -0.7616 * b5 - r * 0.016898;
      p[i] = (b0 + b1 + b2 + b3 + b4 + b5 + b6 + r * 0.5362) * 0.11;
      b6 = r * 0.115926;
      last = (last + 0.02 * r) / 1.02;
      b[i] = last * 3.5;
    }
    // Crossfade the loop seam so looping noise doesn't click.
    const fade = 2048;
    for (const d of [w, p, b]) {
      for (let i = 0; i < fade; i++) {
        const t = i / fade;
        d[len - fade + i] = d[len - fade + i] * (1 - t) + d[i] * t;
      }
    }
    this.noise = { white, pink, brown };
  }

  _loop(buffer, rate = 1) {
    const src = this.ctx.createBufferSource();
    src.buffer = buffer;
    src.loop = true;
    src.playbackRate.value = rate;
    src.start(0, Math.random() * buffer.duration);
    return src;
  }

  _buildTunnel() {
    const ctx = this.ctx;
    // Concrete tube: dense early reflections, ~1.3 s dark tail.
    const sr = ctx.sampleRate, len = Math.floor(sr * 1.4);
    const ir = ctx.createBuffer(2, len, sr);
    for (let c = 0; c < 2; c++) {
      const d = ir.getChannelData(c);
      let lp = 0;
      for (let i = 0; i < len; i++) {
        const t = i / sr;
        lp += 0.35 * ((Math.random() * 2 - 1) - lp); // darken
        d[i] = lp * Math.exp(-t * 5.2) * (t < 0.008 ? t / 0.008 : 1);
      }
      for (let k = 0; k < 14; k++) {
        const at = Math.floor(sr * (0.006 + Math.random() * 0.07));
        d[at] += (Math.random() < 0.5 ? -1 : 1) * (0.5 - k * 0.025);
      }
    }
    this.tunnel = ctx.createConvolver();
    this.tunnel.buffer = ir;
    this.envSend = ctx.createGain(); this.envSend.gain.value = 0;
    const out = ctx.createGain(); out.gain.value = 0.55;
    this.sfxBus.connect(this.envSend); this.envSend.connect(this.tunnel);
    this.tunnel.connect(out); out.connect(this.sfxComp);
  }

  _buildEngine() {
    const ctx = this.ctx;
    const e = {};
    e.sum = ctx.createGain(); e.sum.gain.value = 0.105;
    e.hp = ctx.createBiquadFilter(); e.hp.type = 'highpass'; e.hp.frequency.value = 28; e.hp.Q.value = 0.6;
    e.shiftG = ctx.createGain(); e.shiftG.gain.value = 1;
    e.limG = ctx.createGain(); e.limG.gain.value = 1;
    e.out = ctx.createGain(); e.out.gain.value = 0;
    // Weight under the whole engine, exhaust chains and rumble alike.
    e.body = ctx.createBiquadFilter(); e.body.type = 'lowshelf'; e.body.frequency.value = 140; e.body.gain.value = 6;
    e.sum.connect(e.hp); e.hp.connect(e.body); e.body.connect(e.shiftG); e.shiftG.connect(e.limG);
    e.limG.connect(e.out); e.out.connect(this.sfxBus);

    // Slow combustion unsteadiness: low-passed noise wobbles amplitude and
    // pitch a touch, so the cycle never repeats exactly.
    const jitSrc = this._loop(this.noise.brown, 0.7);
    const jitLp = ctx.createBiquadFilter(); jitLp.type = 'lowpass'; jitLp.frequency.value = 16;
    jitSrc.connect(jitLp);
    e.jitAmp = ctx.createGain(); e.jitAmp.gain.value = 0.35;
    e.jitPitch = ctx.createGain(); e.jitPitch.gain.value = 14; // cents
    jitLp.connect(e.jitAmp); jitLp.connect(e.jitPitch);

    const chain = (pan, detune) => {
      const c = {};
      c.on = ctx.createOscillator(); c.off = ctx.createOscillator();
      c.gOn = ctx.createGain(); c.gOff = ctx.createGain();
      c.gOn.gain.value = 0.5; c.gOff.gain.value = 0.5;
      c.am = ctx.createGain(); c.am.gain.value = 1;
      e.jitAmp.connect(c.am.gain);
      c.on.connect(c.gOn); c.off.connect(c.gOff); c.gOn.connect(c.am); c.gOff.connect(c.am);
      for (const o of [c.on, c.off]) { o.detune.value = detune; e.jitPitch.connect(o.detune); }
      c.drive = ctx.createGain(); c.drive.gain.value = 1.4;
      c.shaper = ctx.createWaveShaper(); c.shaper.curve = exhaustCurve(2.2); c.shaper.oversample = '2x';
      c.am.connect(c.drive); c.drive.connect(c.shaper);
      // The asymmetric shaper leaves a DC offset that wanders with the
      // envelope; block it before it turns into sub-bass rumble.
      c.dc = ctx.createBiquadFilter(); c.dc.type = 'highpass'; c.dc.frequency.value = 28; c.dc.Q.value = 0.6;
      c.shaper.connect(c.dc);
      c.f = [0, 1, 2].map(() => { const f = ctx.createBiquadFilter(); f.type = 'peaking'; return f; });
      c.lp = ctx.createBiquadFilter(); c.lp.type = 'lowpass'; c.lp.Q.value = 0.9; c.lp.frequency.value = 800;
      c.dc.connect(c.f[0]); c.f[0].connect(c.f[1]); c.f[1].connect(c.f[2]); c.f[2].connect(c.lp);
      let out = c.lp;
      if (ctx.createStereoPanner) { c.pan = ctx.createStereoPanner(); c.pan.pan.value = pan; c.lp.connect(c.pan); out = c.pan; }
      out.connect(e.sum);
      c.on.frequency.value = 7; c.off.frequency.value = 7;
      c.on.start(); c.off.start();
      return c;
    };
    e.L = chain(-0.38, -4);
    e.R = chain(0.38, 5);

    // Rumble: the low end on its own oscillator, low-passed, then squashed a
    // little so its overtones still carry on small speakers.
    e.rum = ctx.createOscillator(); e.rum.frequency.value = 7;
    e.jitPitch.connect(e.rum.detune);
    const rumHp = ctx.createBiquadFilter(); rumHp.type = 'highpass'; rumHp.frequency.value = 26; rumHp.Q.value = 0.7;
    const rumLp = ctx.createBiquadFilter(); rumLp.type = 'lowpass'; rumLp.frequency.value = 170; rumLp.Q.value = 0.8;
    const rumAm = ctx.createGain(); rumAm.gain.value = 1;
    e.jitAmp.connect(rumAm.gain);
    const rumSh = ctx.createWaveShaper(); rumSh.curve = distortionCurve(2.5); rumSh.oversample = '2x';
    const rumLp2 = ctx.createBiquadFilter(); rumLp2.type = 'lowpass'; rumLp2.frequency.value = 520; rumLp2.Q.value = 0.6;
    e.rumG = ctx.createGain(); e.rumG.gain.value = 0;
    e.rum.connect(rumHp); rumHp.connect(rumLp); rumLp.connect(rumAm); rumAm.connect(rumSh); rumSh.connect(rumLp2); rumLp2.connect(e.rumG);
    e.rumG.connect(e.sum);
    e.rum.start();

    // Intake roar: noise band-passed at the firing rate and its double.
    const intakeSrc = this._loop(this.noise.pink);
    e.in1 = ctx.createBiquadFilter(); e.in1.type = 'bandpass'; e.in1.Q.value = 1.6;
    e.in2 = ctx.createBiquadFilter(); e.in2.type = 'bandpass'; e.in2.Q.value = 2.4;
    e.inG = ctx.createGain(); e.inG.gain.value = 0;
    const in2G = ctx.createGain(); in2G.gain.value = 0.6;
    intakeSrc.connect(e.in1); intakeSrc.connect(e.in2);
    e.in1.connect(e.inG); e.in2.connect(in2G); in2G.connect(e.inG);
    e.inG.connect(e.sum);

    // Exhaust rasp: high noise gated by the engine's own pulse wave.
    const raspSrc = this._loop(this.noise.white);
    e.raspBp = ctx.createBiquadFilter(); e.raspBp.type = 'bandpass'; e.raspBp.frequency.value = 2600; e.raspBp.Q.value = 0.8;
    e.raspG = ctx.createGain(); e.raspG.gain.value = 0;
    e.raspDepth = ctx.createGain(); e.raspDepth.gain.value = 0;
    raspSrc.connect(e.raspBp); e.raspBp.connect(e.raspG);
    e.L.on.connect(e.raspDepth); e.raspDepth.connect(e.raspG.gain);
    e.raspG.connect(e.sum);

    // Rev limiter: square LFO chops the engine when bouncing off redline.
    e.limLfo = ctx.createOscillator(); e.limLfo.type = 'square'; e.limLfo.frequency.value = 14;
    e.limDepth = ctx.createGain(); e.limDepth.gain.value = 0;
    e.limLfo.connect(e.limDepth); e.limDepth.connect(e.limG.gain);
    e.limLfo.start();

    // Transmission / diff whine (rises with road speed) — straight into sfx.
    e.whine = ctx.createOscillator(); e.whine.type = 'sine';
    e.whine2 = ctx.createOscillator(); e.whine2.type = 'triangle';
    e.whineG = ctx.createGain(); e.whineG.gain.value = 0;
    const w2g = ctx.createGain(); w2g.gain.value = 0.35;
    e.whine.connect(e.whineG); e.whine2.connect(w2g); w2g.connect(e.whineG);
    e.whineG.connect(this.sfxBus);
    e.whine.frequency.value = 200; e.whine2.frequency.value = 300;
    e.whine.start(); e.whine2.start();

    this.eng = e;
  }

  // Turbo: a whistle that climbs with shaft speed and a breathy intake
  // hiss, both scaled by boost. The blow-off valve is a one-shot.
  _buildTurbo() {
    const ctx = this.ctx;
    const tb = {};
    tb.out = ctx.createGain(); tb.out.gain.value = 0;
    tb.out.connect(this.sfxBus);
    tb.w1 = ctx.createOscillator(); tb.w1.type = 'sine';
    tb.w2 = ctx.createOscillator(); tb.w2.type = 'sine';
    tb.wg = ctx.createGain(); tb.wg.gain.value = 0;
    const w2g = ctx.createGain(); w2g.gain.value = 0.35;
    tb.w1.connect(tb.wg); tb.w2.connect(w2g); w2g.connect(tb.wg);
    tb.wg.connect(tb.out);
    tb.hissBp = ctx.createBiquadFilter(); tb.hissBp.type = 'bandpass'; tb.hissBp.Q.value = 2.2;
    tb.hg = ctx.createGain(); tb.hg.gain.value = 0;
    this._loop(this.noise.white).connect(tb.hissBp); tb.hissBp.connect(tb.hg); tb.hg.connect(tb.out);
    tb.w1.frequency.value = 2000; tb.w2.frequency.value = 3000;
    tb.w1.start(); tb.w2.start();
    this.turbo = tb;
    this._boost = 0;
  }

  // Electric drive: the motor's whine (a fundamental that follows motor
  // speed plus a detuned upper partial), a resonant sawtooth "jet" layer, a
  // gear-mesh whine from the reduction gear, fan/air noise, a separate
  // higher whirr under regen and a growl under boost. A faint hum when
  // stopped says the car is on.
  _buildElectric() {
    const ctx = this.ctx;
    const ev = {};
    ev.out = ctx.createGain(); ev.out.gain.value = 0;
    ev.hp = ctx.createBiquadFilter(); ev.hp.type = 'highpass'; ev.hp.frequency.value = 45; ev.hp.Q.value = 0.7;
    ev.sum = ctx.createGain(); ev.sum.gain.value = 1;
    ev.sum.connect(ev.hp); ev.hp.connect(ev.out); ev.out.connect(this.sfxBus);
    // A slow shared vibrato so the tones never sound perfectly static.
    const vib = ctx.createOscillator(); vib.frequency.value = 4.3;
    const vibG = ctx.createGain(); vibG.gain.value = 4; // cents
    vib.connect(vibG); vib.start();
    const tone = (type, gain = 0) => {
      const o = ctx.createOscillator(); o.type = type;
      const g = ctx.createGain(); g.gain.value = gain;
      vibG.connect(o.detune);
      o.connect(g); o.start();
      return { o, g };
    };
    ev.f1 = tone('sine'); ev.f2 = tone('sine'); ev.f3 = tone('triangle');
    ev.mesh = tone('sine'); ev.regen = tone('triangle');
    for (const k of ['f1', 'f2', 'f3', 'mesh']) ev[k].g.connect(ev.sum);
    ev.regenLp = ctx.createBiquadFilter(); ev.regenLp.type = 'lowpass'; ev.regenLp.frequency.value = 3000;
    ev.regen.g.connect(ev.regenLp); ev.regenLp.connect(ev.sum);
    ev.saw = tone('sawtooth');
    ev.sawLp = ctx.createBiquadFilter(); ev.sawLp.type = 'lowpass'; ev.sawLp.Q.value = 7; ev.sawLp.frequency.value = 400;
    ev.saw.g.connect(ev.sawLp); ev.sawLp.connect(ev.sum);
    ev.growl = tone('square');
    ev.growlBp = ctx.createBiquadFilter(); ev.growlBp.type = 'bandpass'; ev.growlBp.Q.value = 1.8;
    ev.growl.g.connect(ev.growlBp); ev.growlBp.connect(ev.sum);
    ev.hum = tone('sine'); ev.hum.o.frequency.value = 100; ev.hum.g.connect(ev.sum);
    ev.hum2 = tone('sine'); ev.hum2.o.frequency.value = 200; ev.hum2.g.connect(ev.sum);
    ev.airBp = ctx.createBiquadFilter(); ev.airBp.type = 'bandpass'; ev.airBp.Q.value = 2.5; ev.airBp.frequency.value = 800;
    ev.airG = ctx.createGain(); ev.airG.gain.value = 0;
    this._loop(this.noise.pink).connect(ev.airBp); ev.airBp.connect(ev.airG); ev.airG.connect(ev.sum);
    this.ev = ev;
  }

  _buildEnvironment() {
    const ctx = this.ctx;
    const pan = (v) => { if (!ctx.createStereoPanner) return ctx.createGain(); const p = ctx.createStereoPanner(); p.pan.value = v; return p; };
    const mk = (buf, type, freq, q, dest = this.sfxBus) => {
      const src = this._loop(buf);
      const f = ctx.createBiquadFilter(); f.type = type; f.frequency.value = freq; if (q !== undefined) f.Q.value = q;
      const g = ctx.createGain(); g.gain.value = 0;
      src.connect(f); f.connect(g); g.connect(dest);
      return { src, f, g };
    };
    // Slow random wobble (brown noise, low-passed) for gusts and chatter.
    const wobble = (hz, depth) => {
      const lp = ctx.createBiquadFilter(); lp.type = 'lowpass'; lp.frequency.value = hz;
      const g = ctx.createGain(); g.gain.value = depth;
      this._loop(this.noise.brown, 0.5 + Math.random()).connect(lp); lp.connect(g);
      return g;
    };

    // Wind: a low roar in the middle plus two gusting upper bands, one each
    // side, so the air moves around you at speed.
    this.wind = mk(this.noise.pink, 'lowpass', 500, 0.5);
    this.windHi = [-0.6, 0.6].map((pv) => {
      const p = pan(pv); p.connect(this.sfxBus);
      const w = mk(this.noise.pink, 'bandpass', 1400, 0.9, p);
      w.gust = ctx.createGain(); w.gust.gain.value = 1;
      w.g.disconnect(); w.g.connect(w.gust); w.gust.connect(p);
      wobble(0.45, 0.45).connect(w.gust.gain);
      return w;
    });
    this.rumble = mk(this.noise.brown, 'lowpass', 130, 0.7);

    // Tyre squeal: a jittery tone (the rubber stick-slip) plus a narrow noise
    // band at the same pitch, chattering in level. Pitch climbs with slip.
    const sq = {};
    sq.out = ctx.createGain(); sq.out.gain.value = 0;
    sq.am = ctx.createGain(); sq.am.gain.value = 0.75;
    wobble(28, 0.5).connect(sq.am.gain);
    sq.am.connect(sq.out); sq.out.connect(this.sfxBus);
    sq.o1 = ctx.createOscillator(); sq.o1.type = 'triangle'; sq.o1.frequency.value = 950;
    sq.o2 = ctx.createOscillator(); sq.o2.type = 'sine'; sq.o2.frequency.value = 1930;
    const jit = wobble(45, 70); // cents
    jit.connect(sq.o1.detune); jit.connect(sq.o2.detune);
    sq.tone = ctx.createGain(); sq.tone.gain.value = 0.5;
    const o2g = ctx.createGain(); o2g.gain.value = 0.35;
    sq.tbp = ctx.createBiquadFilter(); sq.tbp.type = 'bandpass'; sq.tbp.Q.value = 2; sq.tbp.frequency.value = 1100;
    sq.o1.connect(sq.tbp); sq.o2.connect(o2g); o2g.connect(sq.tbp); sq.tbp.connect(sq.tone); sq.tone.connect(sq.am);
    sq.o1.start(); sq.o2.start();
    const nz = this._loop(this.noise.white);
    sq.nbp = ctx.createBiquadFilter(); sq.nbp.type = 'bandpass'; sq.nbp.Q.value = 9; sq.nbp.frequency.value = 1000;
    sq.nbp2 = ctx.createBiquadFilter(); sq.nbp2.type = 'bandpass'; sq.nbp2.Q.value = 2.5; sq.nbp2.frequency.value = 2100;
    sq.noise = ctx.createGain(); sq.noise.gain.value = 1.4;
    nz.connect(sq.nbp); nz.connect(sq.nbp2); sq.nbp.connect(sq.noise); sq.nbp2.connect(sq.noise); sq.noise.connect(sq.am);
    this.squeal = sq;

    // Gravel off the tarmac: a looped bed of stone clicks, played faster
    // with speed, over extra low rumble.
    const gr = this.sfxBuf.gravel;
    this.gravel = { src: this._loop(gr), g: ctx.createGain(), f: ctx.createBiquadFilter() };
    this.gravel.g.gain.value = 0;
    this.gravel.f.type = 'highshelf'; this.gravel.f.frequency.value = 2500; this.gravel.f.gain.value = -4;
    this.gravel.src.connect(this.gravel.f); this.gravel.f.connect(this.gravel.g); this.gravel.g.connect(this.sfxBus);

    // Wall scrape: grinding metal (pre-rendered loop) plus a gritty band, panned to the wall side.
    const sc = {};
    sc.pan = pan(0); sc.pan.connect(this.sfxBus);
    sc.g = ctx.createGain(); sc.g.gain.value = 0; sc.g.connect(sc.pan);
    sc.src = this._loop(this.sfxBuf.scrape);
    sc.src.connect(sc.g);
    sc.grit = mk(this.noise.white, 'bandpass', 2600, 0.8, sc.pan);
    this.scrape = sc;

    // Nitro: hiss, a low rumble and a fluttering flame roar.
    this.nitroHiss = mk(this.noise.white, 'highpass', 3200, 0.7);
    this.nitroRumble = mk(this.noise.brown, 'lowpass', 90, 1.2);
    this.nitroFlame = mk(this.noise.pink, 'bandpass', 420, 0.8);
    const fl = ctx.createOscillator(); fl.frequency.value = 23;
    const flg = ctx.createGain(); flg.gain.value = 0;
    this.nitroFlame.flutter = flg;
    fl.connect(flg); flg.connect(this.nitroFlame.g.gain); fl.start();
  }

  _buildRivals() {
    const ctx = this.ctx;
    this.rivals = [];
    for (let i = 0; i < 3; i++) {
      const osc = ctx.createOscillator();
      const lp = ctx.createBiquadFilter(); lp.type = 'lowpass'; lp.frequency.value = 800; lp.Q.value = 1;
      const g = ctx.createGain(); g.gain.value = 0;
      osc.connect(lp); lp.connect(g);
      let p = null, out = g;
      if (ctx.createStereoPanner) { p = ctx.createStereoPanner(); g.connect(p); out = p; }
      out.connect(this.sfxBus);
      osc.frequency.value = 30;
      osc.detune.value = (i - 1) * 13;
      osc.start();
      this.rivals.push({ osc, lp, g, p });
    }
  }

  // ── Per-frame update ─────────────────────────────────────────────
  update(dt, s = {}) {
    if (!this.ready || this._paused) return;
    const ctx = this.ctx, t = ctx.currentTime;
    const prof = this.prof || CARS.sports;
    const rpmMax = s.rpmMax || 7800;
    const rawRpm = Number.isFinite(s.rpm) ? s.rpm : 800;
    const engineOff = rawRpm < 100;
    const rpm = clamp(rawRpm, 500, rpmMax * 1.05);
    const rn = clamp((rpm - 800) / (rpmMax - 800), 0, 1);
    const thr = clamp(s.throttle || 0, 0, 1);
    const speed = Math.abs(s.speed || 0);
    const onGround = s.onGround !== false;
    const gear = s.gear ?? 1;
    const e = this.eng;
    this._updateTurbo(dt, s, thr, prof);
    if (this.electric) {
      this._updateElectric(dt, s);
      e.out.gain.setTargetAtTime(0, t, 0.05);
      e.whineG.gain.setTargetAtTime(0, t, 0.05);
      this._prevThrottle = thr;
      this._updateEnvironment(s, speed, onGround);
      return;
    }
    this.ev.out.gain.setTargetAtTime(0, t, 0.05);

    // Pitch: the wave holds one 720° cycle, so it plays at rpm / 120.
    const fc = rpm / 120;
    const k = 0.022;
    for (const c of [e.L, e.R]) {
      c.on.frequency.setTargetAtTime(fc, t, k);
      c.off.frequency.setTargetAtTime(fc, t, k);
    }
    e.rum.frequency.setTargetAtTime(fc, t, k);
    // Load crossfade: sharp on-throttle pulses vs soft overrun burble.
    const load = Math.pow(thr, 0.7);
    const gOn = 0.12 + 0.88 * load, gOff = 0.75 * (1 - load) + 0.08;
    const drive = prof.drive * (0.65 + 0.7 * load + 0.35 * rn);
    const lp = prof.lpBase + prof.lpRange * (0.25 * rn + 0.45 * load * (0.4 + 0.6 * rn));
    for (const c of [e.L, e.R]) {
      c.gOn.gain.setTargetAtTime(gOn, t, 0.04);
      c.gOff.gain.setTargetAtTime(gOff, t, 0.06);
      c.drive.gain.setTargetAtTime(drive, t, 0.05);
      c.lp.frequency.setTargetAtTime(c === e.R ? lp * 1.06 : lp, t, 0.04);
    }
    const ff = fc * prof.cyl; // firing frequency
    e.in1.frequency.setTargetAtTime(clamp(ff * 1.1, 60, 8000), t, 0.03);
    e.in2.frequency.setTargetAtTime(clamp(ff * 2.3, 120, 10000), t, 0.03);
    e.inG.gain.setTargetAtTime(prof.intake * (0.04 + 0.5 * load * (0.3 + 0.7 * rn)), t, 0.05);
    e.raspBp.frequency.setTargetAtTime(1800 + rn * 2600, t, 0.05);
    e.raspDepth.gain.setTargetAtTime(prof.rasp * 0.1 * load * (0.3 + 0.7 * rn), t, 0.05);
    // Near idle almost the whole rumble wave gets through the low-pass, so ease it back there.
    e.rumG.gain.setTargetAtTime(prof.rumble * RUMBLE * (0.55 + 0.45 * load) * (0.65 + 0.35 * Math.min(1, rn * 4)), t, 0.05);

    // Level: idle burble is modest, full-load high rpm is loud.
    const vol = engineOff ? 0 : prof.trim * (0.45 + 0.27 * load + 0.2 * rn);
    e.out.gain.setTargetAtTime(vol, t, engineOff ? 0.15 : 0.05);

    // Rev limiter bounce.
    const limiting = !engineOff && rpm >= rpmMax * 0.975 && thr > 0.6 && onGround;
    if (limiting !== this._limiting) {
      this._limiting = limiting;
      e.limG.gain.setTargetAtTime(limiting ? 0.7 : 1, t, 0.01);
      e.limDepth.gain.setTargetAtTime(limiting ? 0.3 : 0, t, 0.01);
    }

    // Transmission whine with road speed (and a louder reverse whine).
    const rev = gear === -1;
    e.whine.frequency.setTargetAtTime(rev ? 250 + speed * 90 : 90 + speed * 26, t, 0.05);
    e.whine2.frequency.setTargetAtTime(rev ? 375 + speed * 135 : 140 + speed * 41, t, 0.05);
    const whine = engineOff ? 0 : rev ? 0.05 * clamp(speed / 6, 0, 1)
      : prof.whine * 0.016 * clamp(speed / 35, 0, 1) * (0.35 + 0.65 * thr);
    e.whineG.gain.setTargetAtTime(whine, t, 0.08);

    // Decel pops/crackle on lift-off at high revs.
    this._popCooldown -= dt;
    if (!engineOff) {
      if (this._prevThrottle > 0.5 && thr < 0.15 && rn > 0.5) this._popBurst(Math.round((3 + Math.random() * 4) * prof.pops), rn);
      else if (thr < 0.1 && rn > 0.42 && this._popCooldown <= 0 && Math.random() < dt * 1.6 * prof.pops) {
        this._pop(t + Math.random() * 0.05, rn * 0.7);
        this._popCooldown = 0.12;
      }
    }
    this._prevThrottle = thr;
    this._updateEnvironment(s, speed, onGround);
  }

  _updateTurbo(dt, s, thr, prof) {
    const t = this.ctx.currentTime, tb = this.turbo;
    const on = !!prof.turbo && !this.electric;
    const boost = on ? clamp(s.boost || 0, 0, 1) : 0;
    const rn = clamp(((s.rpm || 800) - 800) / 7000, 0, 1);
    tb.out.gain.setTargetAtTime(on ? 1 : 0, t, 0.1);
    const wf = 1900 + boost * 3600 + rn * 900;
    tb.w1.frequency.setTargetAtTime(wf, t, 0.08);
    tb.w2.frequency.setTargetAtTime(wf * 1.505, t, 0.08);
    tb.wg.gain.setTargetAtTime(0.045 * boost * boost * (0.4 + 0.6 * thr), t, 0.06);
    tb.hissBp.frequency.setTargetAtTime(1400 + boost * 2600, t, 0.06);
    tb.hg.gain.setTargetAtTime(0.1 * boost * thr, t, 0.06);
    // Blow-off: lifting with boost up vents it.
    if (on && this._prevThrottle > 0.5 && thr < 0.2 && this._boost > 0.35) this._blowOff(this._boost);
    this._boost = boost;
  }

  _blowOff(strength) {
    const t = this.ctx.currentTime;
    this._noiseBurst({ time: t, dur: 0.42, type: 'bandpass', freq: 3600, q: 1.4, gain: 0.2 * strength, sweepTo: 1300 });
    this._noiseBurst({ time: t, dur: 0.3, type: 'highpass', freq: 5200, q: 0.7, gain: 0.07 * strength });
    // Compressor surge flutter: "chu-tu-tu".
    for (let k = 0; k < 3; k++) this._noiseBurst({ time: t + 0.05 + k * 0.055, dur: 0.05, type: 'bandpass', freq: 520, q: 2, gain: 0.12 * strength * (1 - k * 0.25) });
  }

  _updateElectric(dt, s) {
    const t = this.ctx.currentTime, ev = this.ev;
    const motor = clamp(s.motor ?? 0, 0, 1.05);
    const speed = Math.abs(s.speed || 0);
    const load = clamp((s.power || 0) / 600, 0, 1);
    const regen = clamp(s.regen || 0, 0, 1);
    const boost = s.nitro ? 1 : 0;
    const moving = clamp(motor * 25, 0, 1);
    const f1 = 45 + motor * 1450;
    const k = 0.03;
    ev.f1.o.frequency.setTargetAtTime(f1, t, k);
    ev.f2.o.frequency.setTargetAtTime(f1 * 2.003, t, k);
    ev.f3.o.frequency.setTargetAtTime(f1 * 3.02, t, k);
    ev.mesh.o.frequency.setTargetAtTime(f1 * 4.37, t, k);
    ev.regen.o.frequency.setTargetAtTime(f1 * 1.5, t, k);
    ev.saw.o.frequency.setTargetAtTime(f1 * 0.5, t, k);
    ev.sawLp.frequency.setTargetAtTime(Math.min(9000, 120 + f1 * (2.2 + 1.4 * load)), t, k);
    ev.growl.o.frequency.setTargetAtTime(f1 * 0.25, t, k);
    ev.growlBp.frequency.setTargetAtTime(200 + f1 * 0.9, t, k);
    ev.airBp.frequency.setTargetAtTime(500 + f1 * 1.7, t, 0.05);
    const tc = 0.05;
    ev.f1.g.gain.setTargetAtTime(0.06 * moving * (0.3 + 0.7 * load) * (1 - 0.4 * regen), t, tc);
    ev.f2.g.gain.setTargetAtTime(0.028 * moving * (0.2 + 0.8 * load), t, tc);
    ev.f3.g.gain.setTargetAtTime(0.012 * moving * load, t, tc);
    ev.mesh.g.gain.setTargetAtTime(0.006 * moving * (0.3 + load + regen), t, tc);
    ev.regen.g.gain.setTargetAtTime(0.03 * regen * moving, t, tc);
    ev.saw.g.gain.setTargetAtTime(0.03 * moving * (0.25 + 0.75 * load), t, tc);
    ev.growl.g.gain.setTargetAtTime(0.06 * boost * moving, t, boost ? 0.04 : 0.15);
    ev.airG.gain.setTargetAtTime(0.05 * clamp(speed / 50, 0, 1) * (0.4 + 0.6 * load), t, 0.08);
    const idle = 1 - clamp(motor * 40, 0, 1);
    ev.hum.g.gain.setTargetAtTime(0.01 * idle, t, 0.2);
    ev.hum2.g.gain.setTargetAtTime(0.004 * idle, t, 0.2);
    // Menu / results pass no motor state: switched off.
    ev.out.gain.setTargetAtTime(s.motor === undefined ? 0 : 3.4, t, 0.1);
  }

  // s.offroad (0..1, how far onto the verge), s.slip (slip angle, rad) and
  // s.scrapeSide (-1 left / 1 right) are optional; older callers still work.
  _updateEnvironment(s, speed, onGround) {
    const t = this.ctx.currentTime;
    const off = onGround ? clamp(s.offroad || 0, 0, 1) : 0;
    // Wind: builds with the square of speed; louder in the air.
    const sp = clamp(speed / 80, 0, 1.3);
    const air = onGround ? 1 : 1.35;
    this.wind.g.gain.setTargetAtTime(0.2 * sp * sp * air, t, 0.1);
    this.wind.f.frequency.setTargetAtTime(350 + speed * 22, t, 0.1);
    for (const w of this.windHi) {
      w.g.gain.setTargetAtTime(0.06 * Math.pow(clamp((speed - 12) / 70, 0, 1.3), 2.2) * air, t, 0.12);
      w.f.frequency.setTargetAtTime(900 + speed * 28, t, 0.15);
    }
    this.rumble.g.gain.setTargetAtTime(onGround ? (0.13 + 0.22 * off) * clamp(speed / 50, 0, 1) : 0, t, 0.05);
    this.rumble.f.frequency.setTargetAtTime(80 + speed * 1.5 + off * 60, t, 0.1);

    // Squeal follows the skid amount; the slip angle and speed raise its pitch.
    // Off the tarmac tyres don't squeal, they plough.
    const skid = onGround ? clamp(s.skid || 0, 0, 1) * (1 - off) : 0;
    const slip = clamp(Math.abs(s.slip || 0), 0, 0.9);
    const sq = this.squeal;
    const f = 780 + slip * 520 + clamp(speed, 0, 70) * 3.5 + skid * 120;
    sq.out.gain.setTargetAtTime(0.17 * Math.pow(skid, 1.3) * clamp(speed / 6, 0, 1), t, skid > 0.05 ? 0.035 : 0.07);
    sq.o1.frequency.setTargetAtTime(f, t, 0.08);
    sq.o2.frequency.setTargetAtTime(f * 2.03, t, 0.08);
    sq.tbp.frequency.setTargetAtTime(f * 1.15, t, 0.08);
    sq.nbp.frequency.setTargetAtTime(f * 1.05, t, 0.08);
    sq.nbp2.frequency.setTargetAtTime(f * 2.2, t, 0.08);
    // A light, cornering squeal is mostly tone; a big slide is mostly scrub noise.
    sq.tone.gain.setTargetAtTime(0.75 - 0.35 * skid, t, 0.1);

    // Gravel.
    const gv = off * clamp(speed / 14, 0, 1);
    this.gravel.g.gain.setTargetAtTime(0.3 * gv, t, 0.06);
    this.gravel.src.playbackRate.setTargetAtTime(clamp(0.55 + speed / 45, 0.5, 1.9), t, 0.1);

    // Wall scrape, from the side that's touching.
    const scrape = clamp(s.scrape || 0, 0, 1);
    const sc = this.scrape;
    sc.g.gain.setTargetAtTime(0.32 * scrape, t, 0.03);
    sc.src.playbackRate.setTargetAtTime(clamp(0.7 + speed / 60, 0.6, 1.8), t, 0.05);
    sc.grit.g.gain.setTargetAtTime(0.1 * scrape, t, 0.03);
    sc.grit.f.frequency.setTargetAtTime(1800 + clamp(speed, 0, 60) * 25, t, 0.05);
    if (sc.pan.pan && s.scrapeSide) sc.pan.pan.setTargetAtTime(0.55 * Math.sign(s.scrapeSide), t, 0.05);

    // Nitro hiss, rumble and flame (the electric car's boost has its own growl).
    const nitro = s.nitro ? 1 : 0;
    const comb = this.electric ? 0 : nitro;
    this.nitroHiss.g.gain.setTargetAtTime((this.electric ? 0.035 : 0.07) * nitro, t, nitro ? 0.04 : 0.15);
    this.nitroRumble.g.gain.setTargetAtTime(0.24 * comb, t, nitro ? 0.05 : 0.2);
    this.nitroFlame.g.gain.setTargetAtTime(0.12 * comb, t, nitro ? 0.05 : 0.2);
    this.nitroFlame.flutter.gain.setTargetAtTime(0.06 * comb, t, 0.05);
    this.nitroFlame.f.frequency.setTargetAtTime(300 + speed * 5, t, 0.1);
  }

  _popBurst(n, strength) {
    const t0 = this.ctx.currentTime;
    let at = t0 + 0.03;
    for (let i = 0; i < n; i++) {
      this._pop(at, strength * (1 - i / (n + 2)));
      at += 0.035 + Math.random() * 0.11;
    }
    this._popCooldown = at - t0;
  }

  _pop(time, strength) {
    const ctx = this.ctx;
    const prof = this.prof || CARS.sports;
    const [f0, f1] = prof.popFreq;
    const src = ctx.createBufferSource();
    src.buffer = this.noise.white;
    src.playbackRate.value = 0.6 + Math.random() * 0.8;
    const bp = ctx.createBiquadFilter(); bp.type = 'bandpass';
    bp.frequency.value = f0 + Math.random() * (f1 - f0); bp.Q.value = 1.4;
    const sh = ctx.createWaveShaper(); sh.curve = this._popCurve || (this._popCurve = distortionCurve(6));
    const g = ctx.createGain();
    const peak = 0.5 * strength * Math.min(1.3, prof.pops);
    g.gain.setValueAtTime(0, time);
    g.gain.linearRampToValueAtTime(peak, time + 0.002);
    g.gain.exponentialRampToValueAtTime(0.0005, time + 0.05 + Math.random() * 0.03);
    src.connect(bp); bp.connect(sh); sh.connect(g); g.connect(this.sfxBus);
    src.start(time, Math.random() * 1.5, 0.12);
    // Low thud under the crack.
    const o = ctx.createOscillator(); o.type = 'sine';
    o.frequency.setValueAtTime(f0 * 0.18 + 30, time); o.frequency.exponentialRampToValueAtTime(40, time + 0.06);
    const og = ctx.createGain();
    og.gain.setValueAtTime(0.28 * strength, time);
    og.gain.exponentialRampToValueAtTime(0.0005, time + 0.07);
    o.connect(og); og.connect(this.sfxBus);
    o.start(time); o.stop(time + 0.09);
  }

  // ── One-shots ─────────────────────────────────────────────────────
  shift(up = true) {
    if (!this.ready) return;
    const t = this.ctx.currentTime;
    const g = this.eng.shiftG.gain;
    g.cancelScheduledValues(t);
    g.setValueAtTime(g.value, t);
    g.linearRampToValueAtTime(up ? 0.3 : 0.55, t + 0.025);
    g.setTargetAtTime(1, t + (up ? 0.11 : 0.06), 0.05);
    if (this.electric) return;
    // The gearbox: a dog-ring clunk, softer on the way down.
    this._play(this.sfxBuf.clunk, { time: t + (up ? 0.03 : 0.015), gain: up ? 0.32 : 0.24, rate: 0.9 + Math.random() * 0.2 });
    if (!up) this._pop(t + 0.02, 0.35); // rev-match blip
    else {
      // Exhaust bark as the ignition cuts and comes back under load.
      if (this._prevThrottle > 0.6) { this._pop(t + 0.015, 0.5); this._pop(t + 0.06 + Math.random() * 0.03, 0.3); }
      if (this.prof?.turbo && this._boost > 0.5) this._blowOff(this._boost * 0.6);
    }
  }

  // Play a pre-rendered buffer once.
  _play(buf, { time = this.ctx.currentTime, gain = 0.5, rate = 1, pan = 0, dest = this.sfxBus, offset = 0 } = {}) {
    if (!buf) return null;
    const ctx = this.ctx;
    const src = ctx.createBufferSource(); src.buffer = buf; src.playbackRate.value = rate;
    const g = ctx.createGain(); g.gain.value = gain;
    src.connect(g);
    if (pan && ctx.createStereoPanner) { const p = ctx.createStereoPanner(); p.pan.value = clamp(pan, -1, 1); g.connect(p); p.connect(dest); }
    else g.connect(dest);
    src.start(time, offset);
    return { src, g };
  }

  _noiseBurst({ time, dur, type = 'bandpass', freq = 1000, q = 1, gain = 0.3, rate = 1, dest = this.sfxBus, sweepTo = null, pan = 0 }) {
    const ctx = this.ctx;
    const src = ctx.createBufferSource();
    src.buffer = this.noise.white;
    src.playbackRate.value = rate;
    const f = ctx.createBiquadFilter(); f.type = type; f.frequency.value = freq; f.Q.value = q;
    if (sweepTo) {
      f.frequency.setValueAtTime(freq, time);
      f.frequency.exponentialRampToValueAtTime(sweepTo, time + dur);
    }
    const g = ctx.createGain();
    g.gain.setValueAtTime(0, time);
    g.gain.linearRampToValueAtTime(gain, time + Math.min(0.01, dur * 0.2));
    g.gain.exponentialRampToValueAtTime(0.0005, time + dur);
    src.connect(f); f.connect(g);
    if (pan && ctx.createStereoPanner) { const p = ctx.createStereoPanner(); p.pan.value = pan; g.connect(p); p.connect(dest); }
    else g.connect(dest);
    src.start(time, Math.random() * 1.5, dur + 0.05);
    return { src, f, g };
  }

  // Layered crash: a body thud, crumpling sheet metal, a noise crunch for the
  // attack and, on a hard hit, glass. pan: -1 left .. 1 right (optional).
  impact(strength = 0.5, pan = 0) {
    if (!this.ready) return;
    const ctx = this.ctx, t = ctx.currentTime;
    const s = clamp(strength, 0, 1);
    // Grinding along a wall fires a stream of small hits: thin them out.
    if (t - this._impactT < 0.07 && s < (this._impactS ?? 0) + 0.2) return;
    this._impactT = t; this._impactS = s;
    const B = this.sfxBuf, r = Math.random;
    this._play(B.thud, { gain: 0.55 * (0.3 + 0.7 * s), rate: 0.85 + r() * 0.25, pan: pan * 0.4 });
    const metal = [B.metal1, B.metal2, B.metal3][Math.floor(r() * 3)];
    // Harder hits ring lower and longer (a bigger panel moves).
    this._play(metal, { gain: 0.5 * (0.2 + 0.8 * s), rate: 1.2 - 0.4 * s + r() * 0.15, pan: pan * 0.6 });
    if (s > 0.45) this._play(r() < 0.5 ? B.metal1 : B.metal2, { time: t + 0.015, gain: 0.3 * s, rate: 0.7 + r() * 0.15, pan: -pan * 0.3 });
    this._noiseBurst({ time: t, dur: 0.12 + s * 0.15, freq: 900 + s * 700, q: 0.8, gain: 0.3 * (0.25 + 0.75 * s), rate: 0.7 });
    if (s > 0.55) this._play(r() < 0.5 ? B.glass1 : B.glass2, { time: t + 0.02, gain: 0.45 * (s - 0.35), rate: 0.9 + r() * 0.25, pan: pan * 0.5 });
  }

  landing(strength = 0.5) {
    if (!this.ready) return;
    const ctx = this.ctx, t = ctx.currentTime;
    const s = clamp(strength, 0, 1);
    const o = ctx.createOscillator(); o.type = 'sine';
    o.frequency.setValueAtTime(75, t); o.frequency.exponentialRampToValueAtTime(32, t + 0.2);
    const og = ctx.createGain();
    og.gain.setValueAtTime(0, t);
    og.gain.linearRampToValueAtTime(0.45 * (0.3 + 0.7 * s), t + 0.006);
    og.gain.exponentialRampToValueAtTime(0.0005, t + 0.28);
    o.connect(og); og.connect(this.sfxBus);
    o.start(t); o.stop(t + 0.3);
    // Suspension bottoming out, then the tyres chirp as they bite again.
    this._play(this.sfxBuf.thud, { time: t + 0.01, gain: 0.5 * (0.2 + 0.8 * s), rate: 0.95 + Math.random() * 0.15 });
    this._noiseBurst({ time: t + 0.015, dur: 0.07, freq: 420, q: 2.5, gain: 0.18 * s, rate: 0.5 });
    if (s > 0.25) {
      this._noiseBurst({ time: t + 0.03, dur: 0.12, freq: 1300, q: 7, gain: 0.25 * s, sweepTo: 1000 });
      if (s > 0.6) this._play(this.sfxBuf.metal3, { time: t + 0.02, gain: 0.18 * s, rate: 0.8 });
    }
  }

  // Countdown: a rounded square-wave pip; GO is an octave up with a chord.
  beep(final = false) {
    if (!this.ready) return;
    const ctx = this.ctx, t = ctx.currentTime;
    const dur = final ? 0.7 : 0.22;
    const f0 = final ? 880 : 440;
    const notes = final ? [[f0, 1], [f0 * 1.26, 0.55], [f0 * 1.5, 0.5], [f0 / 2, 0.6]] : [[f0, 1], [f0 / 2, 0.5]];
    const lp = ctx.createBiquadFilter(); lp.type = 'lowpass'; lp.Q.value = 0.7;
    lp.frequency.setValueAtTime(final ? 5200 : 3200, t); lp.frequency.setTargetAtTime(final ? 2600 : 1600, t, dur / 2);
    const g = ctx.createGain();
    const lvl = final ? 0.075 : 0.085;
    g.gain.setValueAtTime(0, t);
    g.gain.linearRampToValueAtTime(lvl, t + 0.006);
    g.gain.setTargetAtTime(lvl * 0.7, t + 0.01, 0.1);
    g.gain.setTargetAtTime(0, t + dur - 0.06, 0.025);
    lp.connect(g); g.connect(this.sfxBus);
    for (const [f, a] of notes) {
      for (const [type, m, det] of [['square', 0.6, 0], ['sine', 1, 4]]) {
        const o = ctx.createOscillator(); o.type = type; o.frequency.value = f; o.detune.value = det;
        const og = ctx.createGain(); og.gain.value = a * m;
        o.connect(og); og.connect(lp);
        o.start(t); o.stop(t + dur + 0.1);
      }
    }
    if (final) this._noiseBurst({ time: t, dur: 0.6, type: 'bandpass', freq: 900, q: 0.7, gain: 0.08, sweepTo: 5000 });
  }

  // A car flashing past: an air rush that sweeps across the stereo field
  // plus its engine note dropping in pitch (doppler).
  whoosh(pan = 0, strength = 0.5) {
    if (!this.ready) return;
    const ctx = this.ctx, t = ctx.currentTime;
    const s = clamp(strength, 0, 1);
    const p = clamp(pan, -1, 1);
    const b = this._noiseBurst({ time: t, dur: 0.6, freq: 350, q: 1.4, gain: 0.35 * s });
    b.f.frequency.cancelScheduledValues(t);
    b.f.frequency.setValueAtTime(380, t);
    b.f.frequency.exponentialRampToValueAtTime(1700, t + 0.15);
    b.f.frequency.exponentialRampToValueAtTime(280, t + 0.6);
    b.g.gain.cancelScheduledValues(t);
    b.g.gain.setValueAtTime(0, t);
    b.g.gain.linearRampToValueAtTime(0.35 * s, t + 0.14);
    b.g.gain.exponentialRampToValueAtTime(0.0005, t + 0.6);
    let dest = this.sfxBus;
    if (ctx.createStereoPanner) {
      const pn = ctx.createStereoPanner();
      pn.pan.setValueAtTime(p * 0.5, t);
      pn.pan.linearRampToValueAtTime(p, t + 0.14);
      pn.pan.linearRampToValueAtTime(p * 0.3, t + 0.6);
      pn.connect(this.sfxBus); dest = pn;
      b.g.disconnect(); b.g.connect(pn);
    }
    const o = ctx.createOscillator(); o.type = 'sawtooth';
    const f = 70 + Math.random() * 40;
    o.frequency.setValueAtTime(f * 1.25, t); o.frequency.linearRampToValueAtTime(f * 1.2, t + 0.12);
    o.frequency.exponentialRampToValueAtTime(f * 0.8, t + 0.3);
    const lp = ctx.createBiquadFilter(); lp.type = 'lowpass'; lp.frequency.value = 700;
    const og = ctx.createGain();
    og.gain.setValueAtTime(0, t); og.gain.linearRampToValueAtTime(0.09 * s, t + 0.12); og.gain.exponentialRampToValueAtTime(0.0005, t + 0.5);
    o.connect(lp); lp.connect(og); og.connect(dest);
    o.start(t); o.stop(t + 0.55);
  }

  nitroBurst() {
    if (!this.ready) return;
    const ctx = this.ctx, t = ctx.currentTime;
    if (this.electric) {
      // Overboost: a rising electric zap and a crackle of discharge.
      const o = ctx.createOscillator(); o.type = 'sawtooth';
      o.frequency.setValueAtTime(180, t); o.frequency.exponentialRampToValueAtTime(2600, t + 0.28);
      const bp = ctx.createBiquadFilter(); bp.type = 'bandpass'; bp.Q.value = 3;
      bp.frequency.setValueAtTime(600, t); bp.frequency.exponentialRampToValueAtTime(5200, t + 0.3);
      const g = ctx.createGain();
      g.gain.setValueAtTime(0, t);
      g.gain.linearRampToValueAtTime(0.16, t + 0.02);
      g.gain.exponentialRampToValueAtTime(0.0005, t + 0.45);
      o.connect(bp); bp.connect(g); g.connect(this.sfxBus);
      o.start(t); o.stop(t + 0.5);
      for (let k = 0; k < 6; k++) this._noiseBurst({ time: t + k * 0.03 + Math.random() * 0.02, dur: 0.025, type: 'highpass', freq: 3500, q: 0.8, gain: 0.08 });
      return;
    }
    // The solenoid's "pssh", the flame lighting ("whump") and a rising rush.
    this._noiseBurst({ time: t, dur: 0.18, type: 'highpass', freq: 4500, q: 0.7, gain: 0.16 });
    this._noiseBurst({ time: t + 0.04, dur: 0.65, type: 'bandpass', freq: 600, q: 0.9, gain: 0.26, sweepTo: 4500 });
    const o = ctx.createOscillator(); o.type = 'sine';
    o.frequency.setValueAtTime(95, t + 0.04); o.frequency.exponentialRampToValueAtTime(38, t + 0.35);
    const g = ctx.createGain();
    g.gain.setValueAtTime(0, t + 0.04);
    g.gain.linearRampToValueAtTime(0.4, t + 0.05);
    g.gain.exponentialRampToValueAtTime(0.0005, t + 0.4);
    o.connect(g); g.connect(this.sfxBus);
    o.start(t + 0.04); o.stop(t + 0.45);
    const o2 = ctx.createOscillator(); o2.type = 'sawtooth';
    o2.frequency.setValueAtTime(55, t); o2.frequency.exponentialRampToValueAtTime(110, t + 0.4);
    const lp = ctx.createBiquadFilter(); lp.type = 'lowpass'; lp.frequency.value = 300;
    const g2 = ctx.createGain();
    g2.gain.setValueAtTime(0, t);
    g2.gain.linearRampToValueAtTime(0.14, t + 0.03);
    g2.gain.exponentialRampToValueAtTime(0.0005, t + 0.5);
    o2.connect(lp); lp.connect(g2); g2.connect(this.sfxBus);
    o2.start(t); o2.stop(t + 0.55);
    this._pop(t + 0.03, 0.6);
  }

  // Short UI sounds for the menus: 'click' (buttons, tabs), 'start' (go racing).
  uiClick(kind = 'click') {
    if (!this.ready) return;
    const ctx = this.ctx, t = ctx.currentTime;
    const blips = kind === 'start' ? [[880, 0], [1318.5, 0.06], [1760, 0.12]] : [[1500, 0]];
    for (const [f, dt] of blips) {
      const o = ctx.createOscillator(); o.type = 'triangle';
      o.frequency.setValueAtTime(f * 1.3, t + dt); o.frequency.exponentialRampToValueAtTime(f, t + dt + 0.02);
      const g = ctx.createGain();
      g.gain.setValueAtTime(0, t + dt);
      g.gain.linearRampToValueAtTime(0.09, t + dt + 0.002);
      g.gain.exponentialRampToValueAtTime(0.0005, t + dt + (kind === 'start' ? 0.16 : 0.06));
      o.connect(g); g.connect(this.sfxBus);
      o.start(t + dt); o.stop(t + dt + 0.2);
    }
    this._noiseBurst({ time: t, dur: 0.015, type: 'highpass', freq: 6000, q: 0.7, gain: 0.05 });
  }

  // A brass-and-bells lift over a crash; the music dips under it.
  finishFanfare() {
    if (!this.ready) return;
    const ctx = this.ctx, t0 = ctx.currentTime + 0.05;
    const d = this.musicDuck.gain;
    d.cancelScheduledValues(t0);
    d.setTargetAtTime(0.35, t0, 0.08);
    d.setTargetAtTime(1, t0 + 2.4, 0.6);
    const brass = { type: 'saw', voices: 3, detune: 14, width: 0.5, cutoff: 2600, q: 1.2, fenv: 1.2, fdec: 0.2, a: 0.015, d: 0.3, s: 0.8, r: 0.25, vib: 10, vibDelay: 0.2, gain: 0.11 };
    const bell = { type: 'fm', mods: [{ ratio: 3.5, index: 2, dec: 0.6, sus: 0.05 }], a: 0.002, d: 1, s: 0.1, r: 0.8, gain: 0.07 };
    const out = ctx.createGain(); out.gain.value = 1; out.connect(this.sfxBus);
    const m = this.music;
    for (const [n, dt] of [[69, 0], [72, 0.14], [76, 0.28], [81, 0.42], [76, 0.62], [81, 0.76]]) m.note(out, t0 + dt, [n], 0.18, brass, 0.9);
    m.note(out, t0 + 0.95, [57, 64, 69, 73, 76], 1.6, { ...brass, a: 0.04, gain: 0.16 }, 1);
    for (const [n, dt] of [[81, 0.95], [85, 1.05], [88, 1.15], [93, 1.25]]) m.note(out, t0 + dt, [n], 0.3, bell, 0.9);
    this._play(m.kit.crash, { time: t0 + 0.95, gain: 0.3 });
    this._play(m.kit.kickPunch, { time: t0 + 0.95, gain: 0.5 });
  }

  setRivalEngines(list = []) {
    if (!this.ready) return;
    const t = this.ctx.currentTime;
    for (let i = 0; i < this.rivals.length; i++) {
      const v = this.rivals[i], r = list[i];
      if (!r) { v.g.gain.setTargetAtTime(0, t, 0.1); continue; }
      const rn = clamp(r.rpmNorm ?? 0.5, 0, 1);
      // Electric rivals whine instead of burbling.
      const ev = !!r.electric;
      if (ev !== !!v.ev) {
        v.ev = ev;
        if (ev) v.osc.type = 'sawtooth';
        else if (this._rivalWave) v.osc.setPeriodicWave(this._rivalWave);
      }
      v.osc.frequency.setTargetAtTime(ev ? 40 + rn * 700 : (1200 + rn * 6000) / 120, t, 0.05);
      v.lp.frequency.setTargetAtTime(ev ? 900 + rn * 2600 : 500 + rn * 1600, t, 0.05);
      const near = clamp(1 - (r.dist ?? 100) / 70, 0, 1);
      v.g.gain.setTargetAtTime((ev ? 0.035 : 0.09) * near * near, t, 0.08);
      if (v.p) v.p.pan.setTargetAtTime(clamp(r.pan ?? 0, -1, 1), t, 0.05);
    }
  }

  setPaused(paused) {
    this._paused = !!paused;
    if (!this.ctx) return;
    if (paused) this.ctx.suspend().catch(() => {});
    else {
      this.ctx.resume().catch(() => {});
      this.music?.onResume();
    }
  }

  // ── Music ─────────────────────────────────────────────────────────
  // The songs and the playlist: [{ id, title, style, bpm }].
  static get tracks() { return Music.tracks; }
  // The track that suits a level (its default when nothing else is picked).
  static levelTrack(levelId) { return Music.levelTrack(levelId); }

  setMusic(on) {
    if (!this.ready) { this._musicWanted = on; return; }
    const t = this.ctx.currentTime;
    if (on && !this._musicOn) {
      this._musicOn = true;
      this.musicGate.gain.cancelScheduledValues(t);
      this.musicGate.gain.setTargetAtTime(1, t, 0.3);
      this.music.start();
    } else if (!on && this._musicOn) {
      this._musicOn = false;
      this.musicGate.gain.cancelScheduledValues(t);
      this.musicGate.gain.setTargetAtTime(0, t, 0.25);
      this.music.stop();
    }
  }

  // Play a track by id (a quick fade if another is playing). If it is
  // already the one playing, it just carries on.
  playTrack(id) {
    this._track = id;
    if (this.ready) this.music.play(id);
  }

  // Skip to the next track in the playlist; returns its info.
  nextTrack() {
    if (!this.ready) return null;
    const info = this.music.next();
    this._track = info?.id ?? this._track;
    return info;
  }

  // { id, title, style, bpm } of the playing (or queued) track.
  get trackInfo() { return this.ready ? this.music.info : null; }
}
