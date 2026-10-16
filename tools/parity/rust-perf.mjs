// Frame time, memory, load time and reloads of the Rust client's web build
// (roadmap WP 2.6, gate G1) in headless Chrome on the dev machine's GPU:
// Playground Kit's js/perf.mjs (D1233) with this game's defaults.
//
//   cargo xtask web --release
//   node tools/parity/rust-perf.mjs --level sierra [--backend webgpu|webgl2]
//       [--hq 1|0] [--secs N] [--reloads 10] [--reload-levels sierra,coast]
//       [--query "t=0.2&cars=0"] [--angle gl|vulkan] [--size WxH --dpr N] [--headed]
//       [--phone] [--throttle N] [--trace N] [--tag name] [--dist next]
//       [--race]   a race instead of the flight: the sports car on the autopilot
//                  (seed 1, no pursuit), timed for --secs (default 30) from race time 3 s
//
// The flight runs the client's own measurement page (`index.html?perf=1`,
// crates/mp_game/web/perf.js) and reads `window.__mp.perf`; the kit's
// perf.mjs documents the CPU timer, `--trace` and the result file
// (parity/report/perf/<level>-<backend>[-run][-tag].json here).
//
// The JS game's flight (`--game js`) is no longer here: it is
// tools/parity/perf-baseline.mjs.

import { ROOT } from './lib/jstree.mjs';

const args = process.argv.slice(2);
const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
if (opt('--game', 'rust') !== 'rust') {
  console.error('rust-perf.mjs measures the Rust client; the JS game is measured by tools/parity/perf-baseline.mjs');
  process.exit(2);
}
process.env.GAME_GLOBAL ??= '__mp';
process.env.PK_PROJECT_ROOT ??= ROOT;
const { run } = await import('../../../game-engine/js/perf.mjs');

const level = opt('--level', 'sierra');
const hq = opt('--hq', '1');
const race = args.includes('--race');
const query = race
  ? `level=${level}&autostart=sports&autodrive=1&seed=1&pursuit=0&hq=${hq}`
  : `level=${level}&hq=${hq}`;
await run({
  args: race ? [...args, '--run'] : args,
  root: ROOT,
  defaults: {
    query, label: level, out: 'parity/report/perf', reloads: race ? '0' : '10',
    // The race is timed from race time 3 s (past the countdown); the
    // position is the player's s along the road.
    wait: '(G?.race?.time ?? -1) >= 3', pos: 'G?.race?.s ?? 0',
  },
});
