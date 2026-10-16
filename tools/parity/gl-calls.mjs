// WebGL2 calls per frame of a race in the Rust client's WebGL2 build, and
// the GPU's time by pass and shader program: Playground Kit's
// js/gl-calls.mjs (D1233) with this game's defaults.
//
//   node tools/parity/gl-calls.mjs [--level sierra] [--hq 1|0] [--secs 5] [--size WxH]
//       [--dist next] [--query "cars=0"] [--dump-shaders <dir>] [--by-program --passes 30]
//       [--angle gl|vulkan] [--headed] [--sub 'regex=>to' | --sub-vert | --sub-frag]
//
// The race is the sports car on the autopilot (seed 1, no pursuit),
// counted for `--secs` from race time 4 s. The JS game's counts
// (`--game js`) are no longer here: the kit's tool counts the Rust build;
// the three.js figures are in docs/rust-port/BASELINE.md.

import { ROOT } from './lib/jstree.mjs';

const args = process.argv.slice(2);
const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
if (opt('--game', 'rust') !== 'rust') {
  console.error('gl-calls.mjs counts the Rust client; the JS game\'s counts are recorded in docs/rust-port/BASELINE.md');
  process.exit(2);
}
process.env.GAME_GLOBAL ??= '__mp';
process.env.PK_PROJECT_ROOT ??= ROOT;
const { run } = await import('../../../game-engine/js/gl-calls.mjs');

const level = opt('--level', 'sierra');
const hq = opt('--hq', '1');
await run({
  args,
  root: ROOT,
  defaults: {
    query: `level=${level}&autostart=sports&autodrive=1&seed=1&pursuit=0&hq=${hq}`,
    label: `rust ${level} hq ${hq}`,
    wait: '(G?.race?.time ?? -1) >= 4',
  },
});
