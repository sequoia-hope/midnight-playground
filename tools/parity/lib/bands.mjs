// Third-octave band energies of a render: what SPEC 7.5 compares between
// the JS reference (Chrome's offline renders) and the Rust port's native
// backend (within 1.5 dB per band). Both sides go through this one
// analyser, so the comparison measures the audio, not two analysers.
//
//   node tools/parity/lib/bands.mjs render.wav [from_s to_s]
//
// Method: Welch's average of Hann-windowed 16384-point periodograms (50 %
// overlap; one zero-padded segment if the window is shorter), scaled so the
// one-sided spectrum sums to the signal's mean square. Each band sums the
// bins whose frequency lies in [fc·10^-0.05, fc·10^0.05), for the 31 base-10
// third-octave centres fc = 1000·10^(k/10), k = -17..13 (20 Hz to 20 kHz).
// Levels are 10·log10(power), floored at -120 dB (digital silence).

import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

export const N_FFT = 16384;
export const CENTRES = Array.from({ length: 31 }, (_, i) => 1000 * 10 ** ((i - 17) / 10));
const FLOOR = 1e-12;
const db = (p) => 10 * Math.log10(Math.max(p, FLOOR));

function fft(re, im) {
  const n = re.length;
  for (let i = 1, j = 0; i < n; i++) {
    let bit = n >> 1;
    for (; j & bit; bit >>= 1) j ^= bit;
    j ^= bit;
    if (i < j) { [re[i], re[j]] = [re[j], re[i]]; [im[i], im[j]] = [im[j], im[i]]; }
  }
  for (let len = 2; len <= n; len <<= 1) {
    const ang = (-2 * Math.PI) / len;
    for (let i = 0; i < n; i += len) {
      for (let k = 0; k < len / 2; k++) {
        const wr = Math.cos(ang * k), wi = Math.sin(ang * k);
        const a = i + k, b = a + len / 2;
        const xr = re[b] * wr - im[b] * wi, xi = re[b] * wi + im[b] * wr;
        re[b] = re[a] - xr; im[b] = im[a] - xi;
        re[a] += xr; im[a] += xi;
      }
    }
  }
}

// x: one channel (Float32Array); returns { bands: dB[31], rms: dB, peak }.
export function analyse(x, sampleRate) {
  const N = N_FFT, hop = N / 2;
  const w = new Float64Array(N);
  let w2 = 0;
  for (let i = 0; i < N; i++) { w[i] = 0.5 - 0.5 * Math.cos((2 * Math.PI * i) / N); w2 += w[i] * w[i]; }
  const starts = [];
  if (x.length <= N) starts.push(0);
  else for (let s = 0; s + N <= x.length; s += hop) starts.push(s);
  const psd = new Float64Array(N / 2 + 1);
  const re = new Float64Array(N), im = new Float64Array(N);
  for (const s of starts) {
    re.fill(0); im.fill(0);
    for (let i = 0; i < N && s + i < x.length; i++) re[i] = x[s + i] * w[i];
    fft(re, im);
    for (let k = 0; k <= N / 2; k++) {
      const p = (re[k] * re[k] + im[k] * im[k]) / (N * w2);
      psd[k] += (k === 0 || k === N / 2 ? p : 2 * p) / starts.length;
    }
  }
  const bands = CENTRES.map((fc) => {
    const lo = fc * 10 ** -0.05, hi = fc * 10 ** 0.05;
    let p = 0;
    for (let k = 0; k <= N / 2; k++) { const f = (k * sampleRate) / N; if (f >= lo && f < hi) p += psd[k]; }
    return db(p);
  });
  let ms = 0, peak = 0;
  for (let i = 0; i < x.length; i++) { ms += x[i] * x[i]; peak = Math.max(peak, Math.abs(x[i])); }
  return { bands, rms: db(ms / Math.max(1, x.length)), peak };
}

// ── WAV (IEEE float, 32-bit, interleaved) ──────────────────────────
export function writeWav(file, channels, sampleRate) {
  const n = channels[0].length, nc = channels.length;
  const buf = Buffer.alloc(44 + n * nc * 4);
  buf.write('RIFF', 0); buf.writeUInt32LE(36 + n * nc * 4, 4); buf.write('WAVE', 8);
  buf.write('fmt ', 12); buf.writeUInt32LE(16, 16); buf.writeUInt16LE(3, 20); buf.writeUInt16LE(nc, 22);
  buf.writeUInt32LE(sampleRate, 24); buf.writeUInt32LE(sampleRate * nc * 4, 28); buf.writeUInt16LE(nc * 4, 32); buf.writeUInt16LE(32, 34);
  buf.write('data', 36); buf.writeUInt32LE(n * nc * 4, 40);
  for (let i = 0, o = 44; i < n; i++) for (let c = 0; c < nc; c++, o += 4) buf.writeFloatLE(channels[c][i], o);
  fs.writeFileSync(file, buf);
}

export function readWav(file) {
  const buf = fs.readFileSync(file);
  if (buf.toString('ascii', 0, 4) !== 'RIFF' || buf.toString('ascii', 8, 12) !== 'WAVE') throw new Error(`${file}: not a WAV`);
  let o = 12, fmt = null;
  while (o + 8 <= buf.length) {
    const id = buf.toString('ascii', o, o + 4), size = buf.readUInt32LE(o + 4);
    if (id === 'fmt ') fmt = { format: buf.readUInt16LE(o + 8), channels: buf.readUInt16LE(o + 10), sampleRate: buf.readUInt32LE(o + 12), bits: buf.readUInt16LE(o + 22) };
    if (id === 'data') {
      if (!fmt) throw new Error(`${file}: data before fmt`);
      const { format, channels: nc, bits } = fmt;
      const bps = bits / 8, n = Math.floor(size / (bps * nc));
      const ch = Array.from({ length: nc }, () => new Float32Array(n));
      for (let i = 0, p = o + 8; i < n; i++) {
        for (let c = 0; c < nc; c++, p += bps) {
          ch[c][i] = format === 3 && bits === 32 ? buf.readFloatLE(p) : bits === 16 ? buf.readInt16LE(p) / 32768 : NaN;
        }
      }
      return { channels: ch, sampleRate: fmt.sampleRate };
    }
    o += 8 + size + (size & 1);
  }
  throw new Error(`${file}: no data chunk`);
}

// One render's analysis over [from, to) seconds, per channel.
export function analyseRender(channels, sampleRate, [from = 0, to = Infinity] = []) {
  const a = Math.max(0, Math.round(from * sampleRate)), b = Math.min(channels[0].length, Math.round(to * sampleRate));
  return channels.map((d) => analyse(d.subarray(a, b), sampleRate));
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [file, from, to] = process.argv.slice(2);
  if (!file) { console.error('usage: node tools/parity/lib/bands.mjs render.wav [from_s to_s]'); process.exit(2); }
  const { channels, sampleRate } = readWav(file);
  const res = analyseRender(channels, sampleRate, [Number(from ?? 0), to === undefined ? Infinity : Number(to)]);
  console.log(JSON.stringify({ centres: CENTRES.map((f) => +f.toFixed(1)), channels: res }, null, 1));
}
