// The conformance golden for mp_audio's null backend (roadmap WP 5.1, SPEC
// 7.5): one fixed script of Web Audio calls run on the recording fake
// (tools/parity/lib/webaudio-fake.mjs), covering every operation of the call
// log, AudioParam.value reads through each kind of automation event, and
// each exception the fake throws. crates/mp_audio/tests/null_backend.rs runs
// the same script through the facade on the null backend and requires the
// same log, problems and exceptions line for line, so the null backend
// writes what the fake writes and its strict mode rejects what the fake
// rejects.
//
//   node tools/parity/audio-facade-ref.mjs [--check]
//
// Writes parity/golden/audio/facade-conformance.json:
// { log: [line, ...], problems: [...], throws: [...] }. --check regenerates
// it in memory and fails if it would change. Keep the script and its Rust
// twin (`script` in the test) step for step the same.

import fs from 'node:fs';
import path from 'node:path';
import { ROOT } from './lib/jstree.mjs';
import { RecordingContext, ArrayStore, settle } from './lib/webaudio-fake.mjs';
import { KERNEL_WASM, installKernel } from '../../src/parity/kernel.js';

installKernel(fs.readFileSync(KERNEL_WASM));

const OUT = path.join(ROOT, 'parity/golden/audio/facade-conformance.json');
const CHECK = process.argv.includes('--check');

const clock = { now: 0 };
const sink = [];
const ctx = new RecordingContext({ sampleRate: 48000, clock, sink, store: new ArrayStore(), latencyHint: 'balanced' });
// Three bytes decode to a known clip; anything else fails.
ctx.decoder = (bytes) => (bytes.byteLength === 3 ? { name: 'clip.mp3', channels: 1, length: 1234 } : null);
const attempt = (f) => { try { f(); } catch { /* logged by the fake */ } };

// ── Nodes, attributes, connections ─────────────────────────────────
const g = ctx.createGain();
g.connect(ctx.destination);
const f = ctx.createBiquadFilter();
f.type = 'bandpass';
f.type = 'band';                          // invalid: ignored
f.frequency.value = 1150; f.Q.value = 0.9;
f.connect(g);
const o = ctx.createOscillator();
o.type = 'square';
o.type = 'saw';                           // invalid: ignored
attempt(() => { o.type = 'custom'; });    // InvalidStateError
o.frequency.value = 220;
o.detune.setTargetAtTime(12, 0, 0.05);
o.connect(f);
o.start();
attempt(() => o.start());                 // a second start
o.stop(1.5);

// ── Automation and value reads ─────────────────────────────────────
g.gain.value = 0.5;
g.gain.setValueAtTime(1, 0.1);
g.gain.linearRampToValueAtTime(0.2, 0.3);
g.gain.exponentialRampToValueAtTime(0.8, 0.5);
g.gain.setTargetAtTime(0, 0.6, 0.05);
for (const t of [0.05, 0.2, 1 / 3, 0.4, 0.55, 0.7, 2]) { clock.now = t; void g.gain.value; }
clock.now = 0.65;
g.gain.cancelScheduledValues(0.6);
void g.gain.value;
f.frequency.setTargetAtTime(400, 0.65, 0.1);
f.frequency.linearRampToValueAtTime(900, 0.9);
clock.now = 0.8;
void f.frequency.value;
void o.frequency.value;
attempt(() => g.gain.exponentialRampToValueAtTime(0, 1));
attempt(() => g.gain.setTargetAtTime(0, 1, -1));
attempt(() => { g.gain.value = NaN; });
attempt(() => g.gain.setValueAtTime(1, -1));
attempt(() => g.gain.linearRampToValueAtTime(Infinity, 1));
attempt(() => g.gain.cancelScheduledValues(-0.5));

// ── Buffers, sources, convolver ────────────────────────────────────
clock.now = 1;
const b = ctx.createBuffer(2, 4, 48000);
b.getChannelData(0).set([0, 0.25, -0.5, 1]);
b.copyToChannel(Float32Array.from([1e-7, -0, 3.4e38, -1]), 1);
const s = ctx.createBufferSource();
s.buffer = b;
s.loop = true; s.loopStart = 0.25; s.loopEnd = 1;
s.playbackRate.value = 0.7;
s.connect(g);
s.start(1, 0.5, 1);
attempt(() => { s.buffer = b; });         // a second buffer
const s2 = ctx.createBufferSource();
s2.buffer = b;                            // already logged: no second data line
attempt(() => s2.stop());                 // stop before start
attempt(() => s2.start(-1));              // negative time
s2.start(2, 0.125);
const ir = ctx.createBuffer(2, 8, 48000);
ir.getChannelData(0)[0] = 1; ir.getChannelData(1)[3] = -0.5;
const cv = ctx.createConvolver();
cv.buffer = ir;
cv.normalize = false;
cv.connect(g);
attempt(() => ctx.createBuffer(0, 1, 48000));

// ── Periodic waves, shapers, the other node types ─────────────────
const w = ctx.createPeriodicWave(Float32Array.from([0, 1, 0.5]), new Float32Array(3), { disableNormalization: true });
const w2 = ctx.createPeriodicWave(new Float32Array(2), Float32Array.from([0, 1]));
attempt(() => ctx.createPeriodicWave(new Float32Array(2), new Float32Array(3)));
const o2 = ctx.createOscillator();
o2.setPeriodicWave(w);
o2.setPeriodicWave(w2);
o2.frequency.value = 7;
const sh = ctx.createWaveShaper();
sh.curve = Float32Array.from([-1, 0, 1]);
sh.oversample = '2x';
sh.oversample = '8x';                     // invalid: ignored
o2.connect(sh);
const d = ctx.createDelay(2);
d.delayTime.value = 0.375;
const m = ctx.createChannelMerger(2);
const p = ctx.createStereoPanner();
p.pan.value = -0.38;
const c = ctx.createDynamicsCompressor();
c.threshold.value = -13; c.knee.value = 8; c.ratio.value = 3; c.attack.value = 0.005; c.release.value = 0.18;
sh.connect(d); d.connect(m, 0, 1); sh.connect(m, 0, 0); m.connect(p); p.connect(c); c.connect(g);
const lfo = ctx.createOscillator();
lfo.type = 'triangle'; lfo.frequency.value = 1 / 3.4;
lfo.connect(o2.detune);
lfo.connect(g.gain);
lfo.disconnect(g.gain);
lfo.start(0);
attempt(() => lfo.disconnect(g.gain));    // not connected any more
attempt(() => sh.disconnect(g));          // never connected
sh.disconnect();
o2.start(0.25);

// ── Context state and decoding ─────────────────────────────────────
clock.now = 2.5;
const states = [];
ctx.resume().then(() => states.push(ctx.state));
await settle();
const ok = ctx.decodeAudioData(new Uint8Array([1, 2, 3]).buffer).then((buf) => states.push(`decoded ${buf._id} ${buf.length}`));
const bad = ctx.decodeAudioData(new Uint8Array([9]).buffer).catch((e) => states.push(`failed ${e.name}`));
await ok; await bad;
await settle();
ctx.suspend().then(() => states.push(ctx.state));
await settle();
ctx.close().then(() => states.push(ctx.state));
await settle();
sink.push(JSON.stringify(['states', states]));

const out = JSON.stringify({ log: sink, problems: ctx.problems, throws: ctx.throws }, null, 1) + '\n';
if (CHECK) {
  const old = fs.existsSync(OUT) ? fs.readFileSync(OUT, 'utf8') : null;
  if (old !== out) { console.error(`MISMATCH ${path.relative(ROOT, OUT)} differs from what the fake writes now`); process.exit(1); }
  console.log(`${path.relative(ROOT, OUT)}: identical (${sink.length} lines)`);
} else {
  fs.writeFileSync(OUT, out);
  console.log(`wrote ${path.relative(ROOT, OUT)} (${sink.length} lines, ${ctx.throws.length} exceptions, ${ctx.problems.length} problems)`);
}
