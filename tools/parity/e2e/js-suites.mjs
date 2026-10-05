// The JS game's e2e suites (test/e2e/*.test.js) against the Rust build
// (roadmap WP 6.7, M6's exit; SPEC 8.5): each suite runs with the
// harness's `target: 'rust'` (MR_TARGET=rust), one Chrome at a time, and
// the result is a table of suite × device: passed, failed, skipped, with
// the reason for every skip.
//
//   cargo xtask web --release && npm run test:e2e:rust
//   node tools/parity/e2e/js-suites.mjs menu race-flow     (some suites)
//   node tools/parity/e2e/js-suites.mjs --target js        (the JS game)
//   node tools/parity/e2e/js-suites.mjs --only "Esc pauses"  (a name pattern)
//
// The device is the test's own: a test named for a phone ("phone…",
// "phonePortrait…", "iPhone…") is a phone-emulation test, the rest run on
// the desktop viewport. Before each suite the GPU's memory must be under
// MR_GPU_MB (default 9000 MB): the machine is shared and the suites are
// GPU-heavy.

import { spawn, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { ROOT } from '../lib/jstree.mjs';

// What does not run against the Rust build, and why. Matched against the
// test's name. (`m8` marked what waited for Hot Pursuit, roadmap M8; the
// pursuit suite runs since WP 8.3.)
export const SKIPS = [
  // Deviations (DEVIATIONS.md).
  { suite: 'audio', name: /^phone: pausing suspends the audio, taps on the pause screen leave it/, why: 'deviation: a tap on no control of the pause screen resumes (DEVIATIONS.md, D578); suspend on pause and resume are race-flow\'s Esc test' },
  // JS-only hooks.
  { suite: 'race-button', name: /^phone: a second tap while the race is starting/, why: 'replaces three\'s `renderer.compileAsync` and traps `window.__race` assignments; the Rust check counts `__mr.races` (tools/parity/e2e/race-button.test.mjs)' },
  { suite: 'traffic-lod', name: /./, why: 'drives three.js internals (CarModel.setFar and its geometry groups, Traffic.activate and .lod); the Rust switch (play::FAR_OUT 95 m, FAR_IN 85 m, the same hysteresis) is inside the draw system, with no bridge' },
];

const args = process.argv.slice(2);
let target = 'rust';
const only = [];
const suites = [];
for (let i = 0; i < args.length; i++) {
  if (args[i] === '--target') target = args[++i];
  else if (args[i] === '--only') only.push(args[++i]);
  else suites.push(args[i].replace(/\.test\.js$/, ''));
}
const DIR = path.join(ROOT, 'test', 'e2e');
const all = fs.readdirSync(DIR).filter((f) => f.endsWith('.test.js')).map((f) => f.replace(/\.test\.js$/, '')).sort();
const run = suites.length ? suites : all;

const GPU_MB = Number(process.env.MR_GPU_MB || 9000);
function gpuUsedMb() {
  try {
    const out = execFileSync('nvidia-smi', ['--query-gpu=memory.used', '--format=csv,noheader,nounits'], { encoding: 'utf8' });
    return Math.max(...out.trim().split('\n').map(Number));
  } catch {
    return 0;
  }
}
async function waitGpu() {
  for (let n = 0; gpuUsedMb() >= GPU_MB; n++) {
    if (n % 6 === 0) console.log(`  (GPU memory ${gpuUsedMb()} MB ≥ ${GPU_MB}: waiting)`);
    await new Promise((r) => setTimeout(r, 5000));
  }
}

// The device a test opens (its name says, but for these).
const device = (name) => (/^(phone|phonePortrait|iPhone)\b|^rotate hint|^leaving the page/i.test(name) ? 'phone' : 'desktop');

// node --test leaves the skipped tests out of its report: name them from
// the suite's source (their names are plain strings) and add them.
function skippedOf(suite, skip, tests) {
  const src = fs.readFileSync(path.join(DIR, suite + '.test.js'), 'utf8');
  for (const m of src.matchAll(/\btest\(\s*(['"])((?:\\.|(?!\1).)*)\1/g)) {
    const name = m[2].replace(/\\(.)/g, '$1');
    const s = skip.find((k) => k.name.test(name));
    if (s && !tests.some((t) => t.name === name)) tests.push({ name, status: 'skip', device: device(name), why: s.why, m8: !!s.m8, detail: '' });
  }
  return tests;
}

// One suite: node --test with the TAP reporter, one test at a time.
function runSuite(suite) {
  const skip = target === 'rust' ? SKIPS.filter((s) => s.suite === suite) : [];
  const nodeArgs = ['--test', '--test-reporter=tap', '--test-concurrency=1'];
  for (const s of skip) nodeArgs.push('--test-skip-pattern=' + s.name.source);
  for (const o of only) nodeArgs.push('--test-name-pattern=' + o);
  nodeArgs.push(path.join(DIR, suite + '.test.js'));
  // A suite skipped whole is not started: its `before` would launch a
  // Chrome that no `after` closes.
  if (skip.some((s) => s.name.source === '.')) return Promise.resolve({ suite, code: 0, tests: skippedOf(suite, skip, []), out: '' });
  return new Promise((resolve) => {
    const child = spawn('nice', ['-n', '10', process.execPath, ...nodeArgs], {
      cwd: ROOT, env: { ...process.env, MR_TARGET: target }, stdio: ['ignore', 'pipe', 'pipe'],
    });
    let out = '';
    child.stdout.on('data', (d) => { out += d; if (process.env.MR_VERBOSE) process.stdout.write(d); });
    child.stderr.on('data', (d) => { out += d; if (process.env.MR_VERBOSE) process.stderr.write(d); });
    const timer = setTimeout(() => child.kill('SIGKILL'), Number(process.env.MR_SUITE_TIMEOUT || 1800000));
    child.on('close', (code) => {
      clearTimeout(timer);
      const tests = [];
      const lines = out.split('\n');
      for (let i = 0; i < lines.length; i++) {
        const m = lines[i].match(/^(not )?ok \d+ - (.+?)(?: # (SKIP|TODO).*)?$/);
        if (!m || m[2].endsWith('.test.js')) continue;
        const name = m[2];
        const status = m[3] === 'SKIP' ? 'skip' : m[1] ? 'fail' : 'pass';
        let detail = '';
        if (status === 'fail') {
          const err = [];
          for (let j = i + 1; j < lines.length && !/^(not )?ok \d+/.test(lines[j]) && err.length < 40; j++) err.push(lines[j]);
          detail = err.join('\n');
        }
        const s = skip.find((k) => k.name.test(name));
        tests.push({ name, status, device: device(name), why: status === 'skip' ? (s?.why || 'skipped') : '', m8: !!s?.m8, detail });
      }
      resolve({ suite, code, tests: skippedOf(suite, skip, tests), out });
    });
  });
}

const results = [];
for (const suite of run) {
  await waitGpu();
  const t0 = Date.now();
  console.log(`▶ ${suite} (${target})`);
  const r = await runSuite(suite);
  results.push(r);
  for (const t of r.tests) {
    console.log(`  ${t.status === 'pass' ? '✓' : t.status === 'skip' ? '–' : '✗'} [${t.device}] ${t.name}${t.why ? '  (' + t.why + ')' : ''}`);
    if (t.status === 'fail') console.log(t.detail.split('\n').slice(0, 12).map((l) => '      ' + l).join('\n'));
  }
  if (!r.tests.length) console.log(r.out.split('\n').slice(-30).join('\n'));
  console.log(`  ${((Date.now() - t0) / 1000).toFixed(0)} s`);
}

// The table: suite × device.
const rows = [];
for (const r of results) {
  for (const dev of ['desktop', 'phone']) {
    const ts = r.tests.filter((t) => t.device === dev);
    if (!ts.length) continue;
    const n = (s) => ts.filter((t) => t.status === s).length;
    const why = [...new Set(ts.filter((t) => t.status !== 'pass').map((t) => (t.status === 'fail' ? 'FAIL: ' + t.name : t.why)))].join('; ');
    rows.push(`| ${r.suite} | ${target} | ${dev} | ${n('pass')} | ${n('fail')} | ${n('skip')} | ${why} |`);
  }
}
console.log('\n| Suite | Target | Device | Pass | Fail | Skip | Notes |\n|---|---|---|---|---|---|---|\n' + rows.join('\n'));
const failed = results.some((r) => r.code !== 0 && r.tests.some((t) => t.status === 'fail')) || results.some((r) => !r.tests.length);
if (process.env.MR_RESULTS) fs.writeFileSync(process.env.MR_RESULTS, JSON.stringify(results.map(({ out, ...r }) => r), null, 1));
process.exit(failed ? 1 : 0);
