// The registered dev server's address and Chrome's flags: Playground Kit's
// js/server.mjs (D1233), bound to this checkout. `--url <base>`, else
// http://127.0.0.1 on `--port`, else `$PORT`, else `proj port`; there is
// no default port. A worktree is reached under its path in the main
// checkout.

import { ROOT } from './jstree.mjs';
import { baseUrl as kitBaseUrl, chromeFlags as kitChromeFlags } from '../../../../game-engine/js/server.mjs';

export function baseUrl(opt) {
  return kitBaseUrl(opt, ROOT);
}

/// `rust: false` (the JS game) forces no backend in Chrome.
export function chromeFlags({ backend = 'webgpu', capped = false, race = false, rust = true, angle = 'vulkan' } = {}) {
  return kitChromeFlags({ backend: rust ? backend : null, capped, gesture: race, angle });
}
