// Engine Sound Lab: offline renders and numbers, so the model can be checked
// without ears. Renders the new model (worklet on an OfflineAudioContext) and
// the current game engine (a whole GameAudio on one, as tools/audio-test.html
// does) at a fixed rpm and throttle, then measures level, peak and the
// energy in bands: the "rumble" band (40-150 Hz) against the "whine" band
// (1-4 kHz).

import { GameAudio } from '../../src/game/Audio.js';
import { presetParams, boostTarget } from './presets.js';

export const WORKLET_URL = new URL('worklet.js', import.meta.url);

export async function renderModel(params, { rpm, throttle, secs = 2.5, rate = 48000, profile = 'headphones', psycho = 0, speed = 0 }) {
  const oc = new OfflineAudioContext(2, Math.ceil(secs * rate), rate);
  await oc.audioWorklet.addModule(WORKLET_URL);
  const boost = boostTarget(params, rpm, throttle);
  const node = new AudioWorkletNode(oc, 'engine-lab', {
    numberOfInputs: 0, outputChannelCount: [2],
    processorOptions: { params, state: { rpm, throttle, boost, speed }, psycho, seed: 777 },
  });
  const spk = new AudioWorkletNode(oc, 'speaker-sim', { outputChannelCount: [2], processorOptions: { profile } });
  node.connect(spk); spk.connect(oc.destination);
  return oc.startRendering();
}

// The current game engine, driven the way Race.js drives it.
export async function renderGame(car, { rpm, throttle, secs = 2.5, rate = 48000, boost = 0, speed = 0 }) {
  const oc = new OfflineAudioContext(2, Math.ceil(secs * rate), rate);
  const a = new GameAudio();
  await a.init({ context: oc });
  a.setVolume({ master: 1, sfx: 1, music: 0 });
  a.setCar(car);
  const st = { rpm, rpmMax: 7800, throttle, gear: 3, speed, onGround: true, boost };
  const dt = 0.05;
  a.update(dt, st);
  for (let t = dt; t < secs - dt; t += dt) oc.suspend(Math.round(t * rate / 128) * 128 / rate).then(() => { a.update(dt, st); oc.resume(); }).catch(() => {});
  return oc.startRendering();
}

// Radix-2 in-place FFT.
function fft(re, im) {
  const n = re.length;
  for (let i = 1, j = 0; i < n; i++) {
    let bit = n >> 1;
    for (; j & bit; bit >>= 1) j ^= bit;
    j ^= bit;
    if (i < j) { [re[i], re[j]] = [re[j], re[i]]; [im[i], im[j]] = [im[j], im[i]]; }
  }
  for (let len = 2; len <= n; len <<= 1) {
    const ang = (-2 * Math.PI) / len, wr = Math.cos(ang), wi = Math.sin(ang);
    for (let i = 0; i < n; i += len) {
      let cr = 1, ci = 0;
      for (let j = 0; j < len / 2; j++) {
        const a = i + j, b = a + len / 2;
        const tr = re[b] * cr - im[b] * ci, ti = re[b] * ci + im[b] * cr;
        re[b] = re[a] - tr; im[b] = im[a] - ti; re[a] += tr; im[a] += ti;
        const ncr = cr * wr - ci * wi; ci = cr * wi + ci * wr; cr = ncr;
      }
    }
  }
}

// Stats over buf from `skip` seconds on (past the fade-in). With rpm given,
// also cycleCorr: how alike one 720° cycle is to the next (below 1 kHz,
// normalised correlation at a lag of one cycle). A wavetable engine sits near
// 1; a real one wanders.
export function analyse(buf, skip = 0.6, rpm = 0) {
  const rate = buf.sampleRate, L = buf.getChannelData(0), R = buf.getChannelData(1);
  const i0 = Math.floor(skip * rate), n = buf.length - i0;
  let finite = true, peak = 0, sum = 0;
  const mono = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    const l = L[i + i0], r = R[i + i0];
    if (!Number.isFinite(l) || !Number.isFinite(r)) { finite = false; continue; }
    peak = Math.max(peak, Math.abs(l), Math.abs(r));
    sum += (l * l + r * r) / 2;
    mono[i] = (l + r) / 2;
  }
  const F = 8192, hop = 4096, bands = { sub: [20, 40], low: [40, 150], lowmid: [150, 600], mid: [600, 1000], high: [1000, 4000], air: [4000, 12000] };
  const pw = Object.fromEntries(Object.keys(bands).map((k) => [k, 0]));
  const win = new Float32Array(F).map((_, i) => 0.5 - 0.5 * Math.cos((2 * Math.PI * i) / (F - 1)));
  let frames = 0;
  for (let s = 0; s + F <= n; s += hop) {
    const re = new Float32Array(F), im = new Float32Array(F);
    for (let i = 0; i < F; i++) re[i] = mono[s + i] * win[i];
    fft(re, im);
    for (let k = 1; k < F / 2; k++) {
      const f = (k * rate) / F, e = re[k] * re[k] + im[k] * im[k];
      for (const [b, [lo, hi]] of Object.entries(bands)) if (f >= lo && f < hi) pw[b] += e;
    }
    frames++;
  }
  const db = (x) => +(10 * Math.log10(x + 1e-20)).toFixed(1);
  const total = Object.values(pw).reduce((a, b) => a + b, 0);
  let cycleCorr = null;
  if (rpm > 0) {
    const T = Math.round((120 / rpm) * rate), a = 1 - Math.exp((-2 * Math.PI * 1000) / rate);
    const lp = new Float32Array(n);
    let y1 = 0, y2 = 0;
    for (let i = 0; i < n; i++) { y1 += a * (mono[i] - y1); y2 += a * (y1 - y2); lp[i] = y2; }
    let xy = 0, xx = 0, yy = 0;
    for (let i = 0; i + T < n; i++) { xy += lp[i] * lp[i + T]; xx += lp[i] * lp[i]; yy += lp[i + T] * lp[i + T]; }
    cycleCorr = +(xy / Math.sqrt(xx * yy + 1e-30)).toFixed(3);
  }
  return {
    finite, peak: +peak.toFixed(3), rmsDb: db(sum / n), cycleCorr,
    bandsDb: Object.fromEntries(Object.entries(pw).map(([k, v]) => [k, +(db(v) - db(pw.low)).toFixed(1)])),
    lowVsHighDb: +(db(pw.low) - db(pw.high)).toFixed(1),
    lowmidVsHighDb: +(db(pw.lowmid) - db(pw.high)).toFixed(1),
    share: Object.fromEntries(Object.entries(pw).map(([k, v]) => [k, +((100 * v) / (total || 1)).toFixed(1)])),
    frames,
  };
}

export const POINTS = [
  { label: 'idle', rpm: (p) => p.idle, throttle: 0 },
  { label: '2500 / 30%', rpm: () => 2500, throttle: 0.3 },
  { label: '4000 / full', rpm: () => 4000, throttle: 1 },
  { label: '6000 / full', rpm: (p) => Math.min(6000, p.redline * 0.95), throttle: 1 },
  { label: '5000 / overrun', rpm: (p) => Math.min(5000, p.redline * 0.8), throttle: 0 },
];

// Every preset at every point, new model vs the game's comparable car.
export async function measureAll(keys, onRow = () => {}) {
  const rows = [];
  for (const key of keys) {
    const p = presetParams(key);
    for (const pt of POINTS) {
      const rpm = pt.rpm(p), throttle = pt.throttle;
      const nm = analyse(await renderModel(p, { rpm, throttle }), 0.6, rpm);
      const gm = analyse(await renderGame(p.gameCar, { rpm, throttle, boost: boostTarget(p, rpm, throttle) }), 0.6, rpm);
      const row = { preset: key, point: pt.label, rpm: Math.round(rpm), throttle, model: nm, game: gm };
      rows.push(row); onRow(row);
    }
  }
  return rows;
}

// What a phone does to the rumble band, with and without psychoacoustic bass:
// energy reaching 150-600 Hz (where a phone can play) vs 1-4 kHz.
export async function measurePhone(key, rpm, throttle) {
  const p = presetParams(key);
  const head = analyse(await renderModel(p, { rpm, throttle }));
  const off = analyse(await renderModel(p, { rpm, throttle, profile: 'phone', psycho: 0 }));
  const on = analyse(await renderModel(p, { rpm, throttle, profile: 'phone', psycho: 1 }));
  return { preset: key, rpm, throttle, headphones: head, phone: off, phonePsycho: on };
}
