// Writes parity/golden/exhaust/: the engine lab's model (worklet.js) run
// offline under Node through scripted drives, for the Rust port in
// crates/mp_exhaust to be checked against (its golden test). Run with
// `node tools/engine-lab/test/golden.mjs`; regenerating must give identical
// files.
//
// Each file holds one preset through one driver mode: the driver's state per
// 60 Hz frame (rounded to f32, as sent), and the render's left and right
// channels as RMS per 2400 samples (50 ms) and in full over a few 256-sample
// windows. Numbers that are f32 are stored as base64 of their little-endian
// bytes.

import fs from 'node:fs';
import { engine, render } from './harness.mjs';
import { presetParams, ORDER } from '../presets.js';
import { Driver } from '../drive.js';

const RATE = 48000;
const OUT = new URL('../../../parity/golden/exhaust/', import.meta.url);
const SEED = 777;
// [preset or '*', mode, seconds, window starts (s)]
const RUNS = [
  ['*', 'rev', 8, [0.6, 1.3, 2.45, 4.2, 4.9, 5.4]],
  ['audiV8', 'drive', 12, [1.0, 2.0, 4.0, 9.9, 10.4, 11.6]],
  ['i4turbo', 'drive', 12, [1.0, 2.0, 4.0, 9.9, 10.4, 11.6]],
];
const WIN = 256;
const RMS = 2400;

const b64 = (arr) => Buffer.from(new Float32Array(arr).buffer).toString('base64');

fs.mkdirSync(OUT, { recursive: true });
for (const [which, mode, secs, wins] of RUNS) {
  for (const key of which === '*' ? ORDER : [which]) {
    const p = presetParams(key);
    const d = new Driver();
    d.setPreset(p);
    d.start(mode);
    const frames = [];
    const control = () => {
      const s = d.step(1 / 60);
      const f = Array.from(new Float32Array([s.rpm, s.throttle, s.boost, s.speed]));
      frames.push(...f);
      return { rpm: f[0], throttle: f[1], boost: f[2], speed: f[3] };
    };
    const node = await engine(p, { rpm: p.idle, throttle: 0 }, SEED);
    const { L, R } = render(node, secs, control);
    const rms = (x) => {
      const out = [];
      for (let s = 0; s + RMS <= x.length; s += RMS) {
        let e = 0;
        for (let i = s; i < s + RMS; i++) e += x[i] * x[i];
        out.push(Math.sqrt(e / RMS));
      }
      return b64(out);
    };
    const windows = wins.map((t) => {
      const s = Math.round(t * RATE);
      return { start: s, l: b64(L.subarray(s, s + WIN)), r: b64(R.subarray(s, s + WIN)) };
    });
    const doc = { preset: key, mode, seed: SEED, rate: RATE, samples: L.length, frames: b64(frames), rmsBlock: RMS, rmsL: rms(L), rmsR: rms(R), windows };
    fs.writeFileSync(new URL(`${key}-${mode}.json`, OUT), JSON.stringify(doc) + '\n');
  }
}
console.log('wrote', fs.readdirSync(OUT).length, 'files');
