// The registered dev server's address, for the tools that load the full
// builds through it (the exports are too big for request interception,
// DECISIONS D106): `--url <base>`, else http://127.0.0.1 on `--port`, else
// `$PORT`, else `proj port`; there is no default port. The server serves
// the main checkout, so a worktree is reached under its path there.

import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { ROOT } from './jstree.mjs';

export function baseUrl(opt) {
  const given = opt('--url', null);
  if (given) return given.endsWith('/') ? given : given + '/';
  let port = opt('--port', null) || process.env.PORT;
  if (!port) {
    try {
      port = execFileSync('proj', ['port'], { cwd: ROOT, encoding: 'utf8' }).trim();
    } catch (e) {
      throw new Error(`no server address: pass --url or --port, set $PORT, or register the project (proj port failed: ${e.message})`);
    }
  }
  if (!/^\d+$/.test(port)) throw new Error(`proj port gave "${port}"`);
  const common = path.resolve(ROOT, execFileSync('git', ['rev-parse', '--git-common-dir'], { cwd: ROOT, encoding: 'utf8' }).trim());
  const served = path.dirname(common);
  const rel = path.relative(served, ROOT);
  if (rel.startsWith('..')) throw new Error(`${ROOT} is not below the served checkout ${served}`);
  return `http://127.0.0.1:${port}/${rel ? rel.split(path.sep).map(encodeURIComponent).join('/') + '/' : ''}`;
}

/// Chrome's flags for a measurement: the real GPU, no sound, and the
/// backend forced (`--backend webgl2` turns WebGPU off so the page falls
/// back by itself).
export function chromeFlags({ backend = 'webgpu', capped = false, race = false, rust = true, angle = 'vulkan' } = {}) {
  // `angle`: what ANGLE runs WebGL on, vulkan (the measurements so far) or
  // gl (the native driver, as Chrome on Linux picks for Intel graphics).
  const flags = ['--ignore-gpu-blocklist', '--mute-audio', '--enable-gpu', `--use-angle=${angle}`, '--enable-precise-memory-info'];
  if (!capped) flags.push('--disable-gpu-vsync', '--disable-frame-rate-limit');
  if (race) flags.push('--autoplay-policy=no-user-gesture-required');
  if (rust && backend === 'webgpu') flags.push('--enable-unsafe-webgpu', '--enable-features=Vulkan');
  if (rust && backend === 'webgl2') flags.push('--disable-blink-features=WebGPU', '--disable-features=WebGPU');
  return flags;
}
