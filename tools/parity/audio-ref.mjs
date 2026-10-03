// The audio reference of the JS game (Rust port, roadmap WP 0.7; SPEC 7.5):
// the arrays the audio code generates, a facade log of scripted drives and
// the Web Audio call log they produce, and offline renders measured in
// third-octave bands. Formats: parity/golden/audio/README.md.
//
//   node tools/parity/audio-ref.mjs [stage ...] [--check]
//
// Stages (all, in this order, if none is named):
//   arrays    Node: every buffer, periodic wave and curve the audio builds,
//             for every car (arrays.json, arrays-small.bin; all of it in the
//             cache)
//   radio     Chrome: what each radio clip decodes to (radio-clips.json)
//   drive     Chrome: the game's calls on its audio facade during scripted
//             drives (drive-*.jsonl.gz)
//   renders   Chrome: offline renders of the scenario table (renders.json;
//             WAVs in the cache), after checking Chrome builds the same
//             buffers as arrays.json
//   calllog   Node: the drives played back into GameAudio on a recording
//             Web Audio fake (calllog.json; the logs in the cache)
//
// --check regenerates and compares with parity/golden/audio/ instead of
// writing: exactly for everything except the renders, whose band levels
// must agree within RENDER_TOLERANCE (Chrome's offline renders are not
// bit-reproducible; see the README).
//
// Every capture runs with the parity kernel (Node: installed below; Chrome:
// ?kernel=1 or ?parity=1), and Math.random in the audio code is a seeded
// mulberry32.

import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { KERNEL_WASM, installKernel } from '../../src/parity/kernel.js';
import { ROOT, cacheDir } from './lib/jstree.mjs';
import { RecordingContext, ArrayStore, mulberry32, dec } from './lib/webaudio-fake.mjs';
import { facadeRecorder, replay, TICK } from './lib/audio-facade.mjs';
import { renderScenarios } from './lib/audio-scenarios.mjs';
import { analyseRender, writeWav, CENTRES, N_FFT } from './lib/bands.mjs';

installKernel(fs.readFileSync(KERNEL_WASM));

const GOLDEN = path.join(ROOT, 'parity', 'golden', 'audio');
const SAMPLE_RATE = 48000;
const SEED = 1; // Math.random in the audio code: mulberry32(SEED)
const CARS = ['sports', 'muscle', 'super', 'rally', 'electric'];
const DRIVES = [
  { name: 'race', query: 'parity=1&level=sierra&autostart=sports&autodrive=1', ticks: 45 * TICK },
  { name: 'pursuit', query: 'parity=1&level=sierra&autostart=sports&autodrive=1&pursuit=1&heat=2', ticks: 60 * TICK },
];
const RENDER_TOLERANCE = 1.0; // dB per band, for --check against the golden (Chrome varies by up to ~0.4)
const BAND_FLOOR = -90;       // dB: quieter bands are not compared
const SMALL = 16384;          // arrays up to this many floats are committed whole

const args = process.argv.slice(2);
const CHECK = args.includes('--check');
const STAGES = ['arrays', 'radio', 'drive', 'renders', 'calllog'];
const wanted = args.filter((a) => !a.startsWith('--'));
for (const s of wanted) if (!STAGES.includes(s)) { console.error(`unknown stage '${s}' (${STAGES.join(', ')})`); process.exit(2); }
const run = (s) => wanted.length === 0 || wanted.includes(s);

const sha = (buf) => createHash('sha256').update(buf).digest('hex');
const f32bytes = (a) => Buffer.from(a.buffer, a.byteOffset, a.byteLength);
const json = (v) => JSON.stringify(v, null, 1) + '\n';
const failures = [];
const fail = (msg) => { failures.push(msg); console.error('MISMATCH ' + msg); };

// Write a golden file, or (--check) compare it with what is there.
function golden(name, content) {
  const file = path.join(GOLDEN, name);
  if (CHECK) {
    const old = fs.existsSync(file) ? fs.readFileSync(file) : null;
    const same = old && (name.endsWith('.gz') ? zlib.gunzipSync(old).equals(zlib.gunzipSync(content)) : old.equals(Buffer.from(content)));
    if (!same) fail(`${name} differs from the golden`);
    else console.log(`  ${name}: identical`);
    return;
  }
  fs.mkdirSync(GOLDEN, { recursive: true });
  fs.writeFileSync(file, content);
  console.log(`  wrote ${path.relative(ROOT, file)} (${fs.statSync(file).size} bytes)`);
}
const readGolden = (name) => fs.readFileSync(path.join(GOLDEN, name));

// ── Chrome ─────────────────────────────────────────────────────────
let browser = null;
async function chrome() {
  if (!browser) {
    const { launch } = await import('../../test/e2e/harness.js');
    browser = await launch();
  }
  return browser;
}
// Close Chrome; one that does not go within 20 s is killed.
async function closeChrome() {
  if (!browser) return;
  const b = browser;
  browser = null;
  const t0 = Date.now();
  const gone = await Promise.race([b.close().then(() => true), new Promise((r) => setTimeout(r, 20000, false))]);
  if (!gone) { console.log('  Chrome did not close in 20 s: killed'); b.process()?.kill('SIGKILL'); }
  else if (Date.now() - t0 > 2000) console.log(`  Chrome took ${((Date.now() - t0) / 1000).toFixed(0)} s to close`);
}
async function refPage() {
  const { openGame } = await import('../../test/e2e/harness.js');
  return openGame(await chrome(), { path: 'tools/parity/audio-ref.html', query: 'kernel=1' });
}
function pageErrors(game) {
  if (game.errors.length) throw new Error('page errors:\n  ' + game.errors.join('\n  '));
}

// ── radio ──────────────────────────────────────────────────────────
async function stageRadio() {
  console.log('radio: decoding the clips in Chrome');
  const dir = path.join(ROOT, 'audio', 'radio');
  const files = fs.readdirSync(dir).filter((f) => f.endsWith('.mp3')).sort();
  const game = await refPage();
  const decoded = await game.eval((f) => window.decodeClips(f), files);
  pageErrors(game);
  await game.close();
  const clips = decoded.map((d) => ({ file: d.file, sha256: sha(fs.readFileSync(path.join(dir, d.file))), channels: d.channels, length: d.length }));
  golden('radio-clips.json', json({ sampleRate: SAMPLE_RATE, clips }));
}

// ── drive ──────────────────────────────────────────────────────────
async function captureDrive(d) {
  const { openGame } = await import('../../test/e2e/harness.js');
  const game = await openGame(await chrome(), { query: d.query, init: facadeRecorder });
  const t0 = Date.now();
  await game.waitFor(`window.__mrFacade.tick >= ${d.ticks} || window.__parity.errors.length > 0`, { timeout: d.ticks * 100, interval: 250, what: `${d.ticks} ticks` });
  const errors = await game.eval('window.__parity.errors.map(String)');
  if (errors.length) throw new Error(`drive ${d.name}: errors in the game loop:\n  ${errors.join('\n  ')}`);
  const lines = await game.eval(`window.__mrFacade.log.filter((l) => JSON.parse(l)[0] < ${d.ticks})`);
  pageErrors(game);
  await game.close();
  console.log(`  ${d.name}: ${lines.length} calls in ${d.ticks} ticks (${((Date.now() - t0) / 1000).toFixed(0)} s)`);
  return lines.join('\n') + '\n';
}

async function stageDrive() {
  console.log('drive: recording the audio facade in Chrome');
  const texts = await Promise.all(DRIVES.map(captureDrive));
  const index = [];
  DRIVES.forEach((d, i) => {
    const text = texts[i];
    golden(`drive-${d.name}.jsonl.gz`, zlib.gzipSync(text, { level: 9 }));
    const calls = {};
    for (const l of text.split('\n')) if (l) { const m = dec(l)[1]; calls[m] = (calls[m] || 0) + 1; }
    index.push({ name: d.name, query: d.query, ticks: d.ticks, sha256: sha(text), calls });
  });
  golden('drives.json', json({ tick: 1 / TICK, drives: index }));
}

// ── renders ────────────────────────────────────────────────────────
async function renderOnce(game, sc) {
  const info = await game.eval((s) => window.renderScenario(s), sc);
  const channels = [];
  for (let c = 0; c < info.channels; c++) {
    const parts = [];
    for (let i = 0; i < info.frames; i += 1 << 20) parts.push(Buffer.from(await game.eval((c, i) => window.renderChunk(c, i, 1 << 20), c, i), 'base64'));
    const b = Buffer.concat(parts);
    channels.push(new Float32Array(b.buffer, b.byteOffset, b.length / 4));
  }
  return channels;
}
const round2 = (x) => Math.round(x * 100) / 100;
function bandDiff(a, b) {
  let max = 0;
  for (let c = 0; c < a.length; c++) {
    for (let j = 0; j < a[c].bands.length; j++) {
      const x = a[c].bands[j], y = b[c].bands[j];
      if (Math.max(x, y) > BAND_FLOOR) max = Math.max(max, Math.abs(x - y));
    }
  }
  return max;
}

async function stageRenders() {
  console.log('renders: offline renders in Chrome');
  const scenarios = renderScenarios();
  const game = await refPage();
  // The same buffers in Chrome as in Node: the kernel and the seeded
  // Math.random make the browser's GameAudio build what the fake recorded.
  const inChrome = await game.eval(() => window.bufferHashes());
  const inNode = new Map(JSON.parse(readGolden('arrays.json')).arrays.map((a) => [a.name, a.hashes.join()]));
  let agree = 0;
  for (const [name, hashes] of Object.entries(inChrome)) {
    if (inNode.get(name) === hashes.join()) agree++;
    else fail(`renders: Chrome builds ${name} with other contents than arrays.json (${hashes.join()} vs ${inNode.get(name)})`);
  }
  console.log(`  ${agree} of ${Object.keys(inChrome).length} buffers identical in Chrome and in arrays.json`);
  const dir = CHECK ? null : cacheDir('audio/renders');
  const old = CHECK ? JSON.parse(readGolden('renders.json')) : null;
  const out = [];
  let worst = 0, worstId = null;
  for (const sc of scenarios) {
    const ch = await renderOnce(game, sc);
    const res = analyseRender(ch, SAMPLE_RATE, sc.analyse).map((r) => ({ rms: round2(r.rms), peak: r.peak, bands: r.bands.map(round2) }));
    if (CHECK) {
      const ref = old.renders.find((r) => r.id === sc.id);
      if (!ref) { fail(`renders: ${sc.id} is not in the golden`); continue; }
      const d = bandDiff(ref.channels, res);
      if (d > worst) { worst = d; worstId = sc.id; }
      if (d > RENDER_TOLERANCE) fail(`renders: ${sc.id} bands differ by ${d.toFixed(2)} dB`);
      continue;
    }
    // A second render measures Chrome's run-to-run variation.
    const again = analyseRender(await renderOnce(game, sc), SAMPLE_RATE, sc.analyse).map((r) => ({ bands: r.bands.map(round2) }));
    const jitter = round2(bandDiff(res, again));
    if (jitter > worst) { worst = jitter; worstId = sc.id; }
    writeWav(path.join(dir, sc.id + '.wav'), ch, SAMPLE_RATE);
    out.push({ id: sc.id, scenario: sc, frames: ch[0].length, sha256: sha(Buffer.concat(ch.map(f32bytes))), jitter, channels: res });
    process.stdout.write(`\r  ${out.length}/${scenarios.length} ${sc.id}${' '.repeat(20)}`);
  }
  pageErrors(game);
  await game.close();
  if (CHECK) { console.log(`  renders: largest band difference from the golden ${worst.toFixed(2)} dB, ${worstId} (tolerance ${RENDER_TOLERANCE})`); return; }
  console.log(`\n  renders: ${out.length}, largest run-to-run band difference ${worst.toFixed(2)} dB (${worstId}); WAVs in ${path.relative(ROOT, dir)}`);
  golden('renders.json', json({
    sampleRate: SAMPLE_RATE, frame: 384, seed: SEED,
    analyser: { nfft: N_FFT, window: 'hann', overlap: 0.5, centres: CENTRES.map((f) => +f.toFixed(2)), floorDb: -120, compareAboveDb: BAND_FLOOR },
    renders: out,
  }));
}

// ── arrays ─────────────────────────────────────────────────────────
// Names: the shortest property path from the GameAudio to each node,
// buffer and wave; the creating function for waves no property holds.
function namePaths(audio) {
  const names = new Map();
  const seen = new Set([audio.ctx]);
  let level = [[audio, 'audio']];
  while (level.length) {
    const next = [];
    for (const [o, p] of level) {
      for (const k of Object.keys(o)) {
        if (k === 'ctx' || k.startsWith('_ctx')) continue;
        const v = o[k];
        if (!v || typeof v !== 'object' || seen.has(v) || ArrayBuffer.isView(v)) continue;
        seen.add(v);
        const q = Array.isArray(o) ? `${p}[${k}]` : `${p}.${k}`;
        if (typeof v._id === 'string' && !names.has(v._id)) names.set(v._id, q);
        next.push([v, q]);
      }
    }
    level = next;
  }
  return names;
}

async function buildArrays() {
  const { GameAudio } = await import('../../src/game/Audio.js');
  const clock = { now: 0 }, sink = [], store = new ArrayStore();
  const saved = { random: Math.random, window: globalThis.window };
  let ctx = null;
  // Creation sites, for the arrays no property holds (pulseWave, ...).
  const sites = new Map();
  globalThis.window = {
    AudioContext: class extends RecordingContext {
      constructor(o = {}) { super({ sampleRate: SAMPLE_RATE, clock, sink, store, latencyHint: o.latencyHint }); ctx = this; this.sites = sites; }
    },
  };
  Math.random = mulberry32(SEED);
  try {
    const audio = new GameAudio();
    await audio.init();
    for (const car of CARS) audio.setCar(car);
    const names = namePaths(audio);
    const entries = [];
    for (const b of ctx._buffers) {
      entries.push({ name: names.get(b._id) ?? b._id, kind: 'buffer', sampleRate: b.sampleRate, data: b._data });
    }
    for (const line of sink) {
      const r = dec(line);
      if (r[1] === 'wave') {
        const id = r[2];
        const name = names.get(id) ?? `${sites.get(id)}()`;
        entries.push({ name, kind: 'wave', parts: ['real', 'imag'], hashes: [r[3], r[4]], options: r[5] });
      }
      if (r[1] === 'set' && r[3] === 'curve' && r[4]) {
        const name = names.has(r[2]) ? names.get(r[2]) + '.curve' : `${sites.get(r[2] + '.curve')}(): ${r[2]}.curve`;
        entries.push({ name, kind: 'curve', hashes: [r[4]] });
      }
    }
    return { entries, store, problems: ctx.problems, throws: ctx.throws };
  } finally {
    Math.random = saved.random;
    globalThis.window = saved.window;
  }
}

const stats = (a) => {
  let ms = 0, peak = 0;
  for (let i = 0; i < a.length; i++) { ms += a[i] * a[i]; peak = Math.max(peak, Math.abs(a[i])); }
  return { rms: Math.sqrt(ms / a.length), peak };
};

async function stageArrays() {
  console.log('arrays: building GameAudio on the recording fake, every car');
  const { entries, store, problems, throws } = await buildArrays();
  if (problems.length || throws.length) throw new Error(`arrays: the fake flagged ${[...problems, ...throws].join('; ')}`);
  // One record per distinct name; a wave or curve shared by several names
  // keeps the first.
  const index = [], small = [], full = [];
  let smallOff = 0, fullOff = 0;
  const seenWave = new Map();
  for (const e of entries) {
    const arrays = e.kind === 'buffer' ? e.data : e.hashes.map((h) => store.byHash.get(h));
    const hashes = arrays.map((a) => ArrayStore.hash(a));
    const key = hashes.join();
    if (e.kind !== 'buffer' && seenWave.has(key)) { seenWave.get(key).aliases.push(e.name); continue; }
    const rec = { name: e.name, kind: e.kind };
    if (e.kind === 'buffer') Object.assign(rec, { sampleRate: e.sampleRate, channels: arrays.length });
    if (e.kind === 'wave') Object.assign(rec, { parts: e.parts, options: e.options });
    rec.length = arrays[0].length;
    rec.hashes = hashes;
    rec.stats = arrays.map(stats);
    rec.cache = arrays.map((a) => { const o = fullOff; full.push(a); fullOff += a.length; return o; });
    if (arrays.every((a) => a.length <= SMALL)) rec.small = arrays.map((a) => { const o = smallOff; small.push(a); smallOff += a.length; return o; });
    if (e.kind !== 'buffer') { rec.aliases = []; seenWave.set(key, rec); }
    index.push(rec);
  }
  const head = { sampleRate: SAMPLE_RATE, seed: SEED, cars: CARS, smallFile: 'arrays-small.bin', cacheFile: 'arrays.bin' };
  const smallBin = Buffer.concat(small.map(f32bytes));
  golden('arrays.json', json({ ...head, arrays: index }));
  golden('arrays-small.bin', smallBin);
  if (!CHECK) {
    const dir = cacheDir('audio');
    fs.writeFileSync(path.join(dir, 'arrays.bin'), Buffer.concat(full.map(f32bytes)));
    console.log(`  ${index.length} arrays (${fullOff * 4} bytes) in ${path.relative(ROOT, path.join(dir, 'arrays.bin'))}`);
  }
}

// ── calllog ────────────────────────────────────────────────────────
function clipTable() {
  const { clips } = JSON.parse(readGolden('radio-clips.json'));
  const byHash = new Map(clips.map((c) => [c.sha256, c]));
  return {
    byHash(bytes) {
      const b = Buffer.from(bytes);
      return byHash.get(sha(b)) ?? null;
    },
  };
}

function digest(sink) {
  const text = sink.join('\n') + '\n';
  const ops = {};
  const seconds = [];
  let cur = -1, h = null, n = 0;
  const close = () => { if (h) seconds.push({ second: cur, lines: n, sha256: h.digest('hex').slice(0, 16) }); };
  for (const l of sink) {
    const r = dec(l);
    ops[r[1]] = (ops[r[1]] || 0) + 1;
    const s = Math.floor(r[0]);
    if (s !== cur) { close(); cur = s; h = createHash('sha256'); n = 0; }
    h.update(l + '\n'); n++;
  }
  close();
  return { text, summary: { lines: sink.length, bytes: Buffer.byteLength(text), sha256: sha(text), ops, seconds } };
}

async function stageCalllog() {
  console.log('calllog: playing the drives back into GameAudio on the recording fake');
  const { GameAudio } = await import('../../src/game/Audio.js');
  const clips = clipTable();
  const readFile = (url) => {
    const file = fileURLToPath(url);
    return file.startsWith(ROOT + path.sep) && fs.existsSync(file) ? fs.readFileSync(file) : null;
  };
  const out = [];
  for (const d of DRIVES) {
    const lines = zlib.gunzipSync(readGolden(`drive-${d.name}.jsonl.gz`)).toString('utf8').split('\n').filter(Boolean);
    const r = await replay(lines, { GameAudio, sampleRate: SAMPLE_RATE, seed: SEED, clips, readFile });
    if (r.errors.length || r.problems.length || r.mutated.length) {
      throw new Error(`calllog ${d.name}: ${[...r.errors, ...r.problems, ...r.mutated.map((b) => `buffer ${b} changed after use`)].join('\n  ')}`);
    }
    const { text, summary } = digest(r.sink);
    const arrays = [...r.store.byHash.entries()];
    out.push({ name: d.name, end: r.end, throws: r.throws, arrays: arrays.length, ...summary });
    console.log(`  ${d.name}: ${summary.lines} calls, ${arrays.length} arrays, ${r.throws.length} browser exceptions`);
    if (!CHECK) {
      const dir = cacheDir('audio');
      fs.writeFileSync(path.join(dir, `calllog-${d.name}.jsonl`), text);
      let off = 0;
      const idx = arrays.map(([h, a]) => { const e = { hash: h, offset: off, length: a.length }; off += a.length; return e; });
      fs.writeFileSync(path.join(dir, `calllog-${d.name}.arrays.json`), json(idx));
      fs.writeFileSync(path.join(dir, `calllog-${d.name}.arrays.bin`), Buffer.concat(arrays.map(([, a]) => f32bytes(a))));
    }
  }
  golden('calllog.json', json({ sampleRate: SAMPLE_RATE, seed: SEED, tick: 1 / TICK, drives: out }));
}

// ── main ───────────────────────────────────────────────────────────
try {
  if (run('arrays')) await stageArrays();
  if (run('radio')) await stageRadio();
  if (run('drive')) await stageDrive();
  if (run('renders')) await stageRenders();
  await closeChrome();
  if (run('calllog')) await stageCalllog();
} finally {
  await closeChrome();
}
if (CHECK) {
  if (failures.length) { console.error(`\n${failures.length} mismatch(es)`); process.exit(1); }
  console.log(`\nall identical to the golden${run('renders') ? ' (renders: band levels within tolerance)' : ''}`);
}
console.log(`done in ${process.uptime().toFixed(0)} s`);
// Puppeteer can leave a handle open after Chrome has gone; nothing is pending.
process.exit(0);
