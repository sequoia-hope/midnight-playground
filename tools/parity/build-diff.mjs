// Pixel comparison of two builds of the Rust client on WebGL2: the same
// frozen station of each level from dist/<a>/ and dist/<b>/, compared
// pixel for pixel; Playground Kit's js/build-diff.mjs (D1233) with this
// game's defaults. `dist/prof/` (`cargo xtask prof`, a copy of the
// previous dist/next/) is the usual "before". Writes the shots and a red
// overlay of the differing pixels to parity/report/build-diff/.
//
//   node tools/parity/build-diff.mjs [--a prof] [--b next] [--levels sierra,coast,seaside,desert]
//       [--query "s=300&h=2.2&back=7&pitch=-0.04&freeze=1&t=0.3&hq=1"]

import { ROOT } from './lib/jstree.mjs';

const args = process.argv.slice(2);
const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
process.env.GAME_GLOBAL ??= '__mp';
process.env.PK_PROJECT_ROOT ??= ROOT;
const { run } = await import('../../../game-engine/js/build-diff.mjs');

await run({
  args: [...args, '--cases', opt('--levels', 'sierra,coast,seaside,desert')],
  root: ROOT,
  defaults: {
    param: 'level',
    query: 's=300&h=2.2&back=7&pitch=-0.04&freeze=1&t=0.3&hq=1',
    out: 'parity/report/build-diff',
    wait: 'G && G.ready',
  },
});
