// The key large parity outputs are cached under: a hash of everything in the
// JS game that can change a reference capture (SPEC 12: "regenerated on
// demand and cached by the hash of the JS tree").
//
//   import { jsTreeKey, cacheDir } from './lib/jstree.mjs';
//   const dir = cacheDir('scenes');   // parity/cache/<key>/scenes/, created
//
// The key covers the working tree, not just HEAD, so an uncommitted edit to
// the game gets a fresh cache entry.
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');

// What a capture depends on: the game, three.js, the page, and the kernel.
const INPUTS = ['src', 'vendor', 'index.html', 'tools/parity/kernel/mr_kernel.wasm'];

function walk(rel, out) {
  const abs = path.join(ROOT, rel);
  const st = fs.statSync(abs);
  if (st.isDirectory()) for (const name of fs.readdirSync(abs).sort()) walk(path.join(rel, name), out);
  else out.push(rel);
}

let memo = null;
export function jsTreeKey() {
  if (memo) return memo;
  const files = [];
  for (const rel of INPUTS) walk(rel, files);
  const h = createHash('sha256');
  for (const rel of files) {
    h.update(rel.split(path.sep).join('/') + '\0');
    h.update(fs.readFileSync(path.join(ROOT, rel)));
    h.update('\0');
  }
  memo = h.digest('hex').slice(0, 16);
  return memo;
}

// parity/cache/<key>/<sub>/ (git-ignored), created if missing.
export function cacheDir(sub) {
  const dir = path.join(ROOT, 'parity', 'cache', jsTreeKey(), sub);
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) console.log(jsTreeKey());
