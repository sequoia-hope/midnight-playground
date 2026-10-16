// The Rust client's page console, for a build that does not come up:
// Playground Kit's js/console.mjs (D1233) opening Sierra's frozen station.
//
//   node tools/parity/web-console.mjs [--backend webgpu|webgl2] [--dist next] [--secs 60]

import { ROOT } from './lib/jstree.mjs';

process.env.GAME_GLOBAL ??= '__mp';
process.env.PK_PROJECT_ROOT ??= ROOT;
const { run } = await import('../../../game-engine/js/console.mjs');
await run({
  args: process.argv.slice(2),
  defaults: { backend: 'webgl2', query: 'level=sierra&s=300&h=2.2&back=7&freeze=1&t=0.3&hq=1' },
});
