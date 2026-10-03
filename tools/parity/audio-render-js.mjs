// Chrome's renders of chosen audio scenarios as WAVs, for looking at a
// difference the band comparison (tools/parity/audio-bands.mjs) reports:
// the same page and scenario table as the reference renders
// (tools/parity/audio-ref.html, parity/golden/audio/renders.json), without
// touching the golden. Headless Chrome through the e2e harness (request
// interception, no server).
//
//   node tools/parity/audio-render-js.mjs <id-prefix> [...]
//
// Writes parity/cache/<js-tree-key>/audio/js-renders/<id>.wav.

import fs from 'node:fs';
import path from 'node:path';
import { ROOT, cacheDir } from './lib/jstree.mjs';
import { writeWav } from './lib/bands.mjs';

const prefixes = process.argv.slice(2);
if (!prefixes.length) { console.error('usage: node tools/parity/audio-render-js.mjs <id-prefix> [...]'); process.exit(2); }
const golden = JSON.parse(fs.readFileSync(path.join(ROOT, 'parity/golden/audio/renders.json'), 'utf8'));
const wanted = golden.renders.filter((r) => prefixes.some((p) => r.id.startsWith(p)));
const dir = cacheDir('audio/js-renders');

const { launch, openGame } = await import('../../test/e2e/harness.js');
const browser = await launch();
try {
  const game = await openGame(browser, { path: 'tools/parity/audio-ref.html', query: 'kernel=1' });
  for (const r of wanted) {
    const info = await game.eval((s) => window.renderScenario(s), r.scenario);
    const channels = [];
    for (let c = 0; c < info.channels; c++) {
      const parts = [];
      for (let i = 0; i < info.frames; i += 1 << 20) parts.push(Buffer.from(await game.eval((c, i) => window.renderChunk(c, i, 1 << 20), c, i), 'base64'));
      const b = Buffer.concat(parts);
      channels.push(new Float32Array(b.buffer, b.byteOffset, b.length / 4));
    }
    writeWav(path.join(dir, r.id + '.wav'), channels, info.sampleRate);
    console.log(`${r.id}: ${info.frames} frames`);
  }
  if (game.errors.length) console.error('page errors:\n  ' + game.errors.join('\n  '));
  await game.close();
} finally {
  await browser.close();
}
console.log(`WAVs in ${path.relative(ROOT, dir)}`);
process.exit(0);
