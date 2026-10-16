// A CPU profile of the Rust client racing in headless Chrome, by function
// and by Bevy system (DECISIONS D1180's method): Playground Kit's
// js/prof.mjs (D1233) with this game's defaults.
//
//   cargo xtask web --release && cargo xtask prof
//   node tools/parity/rust-prof.mjs --level sierra [--backend webgpu|webgl2]
//       [--phone | --size WxH] [--throttle N] [--hq 1|0] [--secs N]
//       [--dist prof] [--query "cars=0"] [--tag name]
//   node tools/parity/rust-prof.mjs --from parity/report/perf/prof-sierra-webgl2.cpuprofile
//       [--callers <regex>] [--depth N] [--top N]
//
// The profile runs for `--secs` (default 10) from race time 4 s of the
// sports car on the autopilot (seed 1, no pursuit). `--phone` is a phone
// in landscape with the CPU throttled 4x and hq off.

import { ROOT } from './lib/jstree.mjs';

const args = process.argv.slice(2);
const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
process.env.GAME_GLOBAL ??= '__mp';
process.env.PK_PROJECT_ROOT ??= ROOT;
const { run } = await import('../../../game-engine/js/prof.mjs');

const level = opt('--level', 'sierra');
const hq = opt('--hq', args.includes('--phone') ? '0' : '1');
await run({
  args,
  root: ROOT,
  defaults: {
    query: `level=${level}&autostart=sports&autodrive=1&seed=1&pursuit=0&hq=${hq}`,
    label: level, out: 'parity/report/perf',
    wait: '(G?.race?.time ?? -1) >= 4', pos: 'G?.race?.s ?? 0',
  },
});
