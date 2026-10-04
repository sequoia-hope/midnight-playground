// The L4 audio comparison of the Rust port (SPEC 7.5, roadmap M5): the
// native backend renders every scenario of parity/golden/audio/renders.json
// (crates/mr_audio/examples/render_scenarios.rs, the way
// tools/parity/audio-ref.html renders it in Chrome), and each render's
// third-octave band levels (tools/parity/lib/bands.mjs, the one analyser for
// both sides) are compared with Chrome's: within TOLERANCE dB per band,
// ignoring bands quieter than the golden's compareAboveDb in both.
//
//   node tools/parity/audio-bands.mjs [--no-render] [id-prefix ...]
//
// The WAVs and a JSON report go to parity/cache/<js-tree-key>/audio/
// rust-renders/. Prints one line per scenario (worst band difference, the
// band, how many bands compared) and a summary; exits 1 if a scenario is
// outside the tolerance. Every scenario is rendered, the seven songs'
// first thirty seconds included (WP 5.6); an id prefix narrows the run.

import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { ROOT, cacheDir } from './lib/jstree.mjs';
import { readWav, analyseRender, CENTRES } from './lib/bands.mjs';

const TOLERANCE = 1.5; // dB per band (SPEC 7.5)

const args = process.argv.slice(2);
const render = !args.includes('--no-render');
const prefixes = args.filter((a) => !a.startsWith('--'));
const golden = JSON.parse(fs.readFileSync(path.join(ROOT, 'parity/golden/audio/renders.json'), 'utf8'));
const floor = golden.analyser.compareAboveDb;
const wanted = golden.renders.filter((r) => !prefixes.length || prefixes.some((p) => r.id.startsWith(p)));
const dir = cacheDir('audio/rust-renders');

if (render) {
  const ids = prefixes.length ? prefixes : [...new Set(wanted.map((r) => r.id.split('-')[0] + '-'))];
  const r = spawnSync('cargo', ['run', '--release', '-q', '-p', 'mr_audio', '--features', 'native', '--example', 'render_scenarios', '--', dir, ...ids], { cwd: ROOT, stdio: ['ignore', 'inherit', 'inherit'] });
  if (r.status !== 0) { console.error('render_scenarios failed'); process.exit(2); }
}

const fmt = (x) => x.toFixed(2).padStart(6);
const report = [];
let fails = 0;
for (const ref of wanted) {
  const file = path.join(dir, ref.id + '.wav');
  if (!fs.existsSync(file)) { console.log(`${ref.id.padEnd(32)} no render`); continue; }
  const { channels, sampleRate } = readWav(file);
  const res = analyseRender(channels, sampleRate, ref.scenario.analyse);
  let worst = 0, at = null, compared = 0;
  const bands = [];
  for (let c = 0; c < res.length; c++) {
    for (let j = 0; j < CENTRES.length; j++) {
      const x = Math.round(res[c].bands[j] * 100) / 100, y = ref.channels[c].bands[j];
      if (Math.max(x, y) <= floor) continue;
      compared++;
      const d = Math.abs(x - y);
      bands.push({ c, hz: Math.round(CENTRES[j]), rust: x, chrome: y });
      if (d > worst) { worst = d; at = { c, hz: Math.round(CENTRES[j]), rust: x, chrome: y }; }
    }
  }
  const rms = res.map((r) => Math.round(r.rms * 100) / 100);
  const ok = worst <= TOLERANCE;
  if (!ok) fails++;
  report.push({ id: ref.id, ok, worst: Math.round(worst * 100) / 100, at, compared, jitter: ref.jitter, rms, chromeRms: ref.channels.map((c) => c.rms), bands });
  console.log(`${ref.id.padEnd(32)} ${ok ? 'ok  ' : 'FAIL'} worst ${fmt(worst)} dB${at ? ` at ${String(at.hz).padStart(5)} Hz ch${at.c} (rust ${fmt(at.rust)}, chrome ${fmt(at.chrome)})` : ''}; rms ${rms.map(fmt).join('/')} vs ${ref.channels.map((c) => fmt(c.rms)).join('/')}; ${compared} bands`);
}
fs.writeFileSync(path.join(dir, 'report.json'), JSON.stringify({ tolerance: TOLERANCE, floor, renders: report }, null, 1) + '\n');
console.log(`\n${report.length - fails} of ${report.length} scenarios within ${TOLERANCE} dB; report in ${path.relative(ROOT, path.join(dir, 'report.json'))}`);
process.exit(fails ? 1 : 0);
