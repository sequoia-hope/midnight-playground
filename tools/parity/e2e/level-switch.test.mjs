// Switching levels from the menu's tabs (roadmap WP 6.2; DECISIONS D574,
// D676, D743): Sierra → Seaside → Sierra, each tab showing that level's
// menu section with no scene loaded, then a race on the last, which loads
// it whole: built by the client itself (`animate`, the default, D678), and
// with `?world=export` (the exported scene downloaded).
//
//   cargo xtask web --release && node --test tools/parity/e2e/level-switch.test.mjs
//   (MR_BACKEND=webgl2 for the WebGL2 build)

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame, startFromMenu, waitRacing } from './harness.mjs';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

async function pick(game, id) {
  await game.click(`#lvl-tab-${id}`);
  await game.waitFor(`window.__mr.level === ${JSON.stringify(id)} && window.__mr.sections.shown === ${JSON.stringify(id)} && window.__mr.mode === 'menu' && window.__mr.ready`,
    { timeout: 300000, interval: 250, what: `${id}'s section` });
  await game.frames(5);
  assert.equal(await game.screen(), 'menu');
  assert.equal((await game.ui('#lvl-name')).value, { sierra: 'Sierra to the City', seaside: 'Seaside Raceway' }[id]);
}

for (const query of ['', 'world=export']) {
  test(`level tabs: Sierra → Seaside → Sierra, then a race${query ? ' (' + query + ')' : ''}`, async () => {
    const backend = process.env.MR_BACKEND ? `backend=${process.env.MR_BACKEND}&` : '';
    const game = await openGame(browser, { query: backend + (query ? query + '&timescale=2' : 'timescale=2'), storage: { 'mr.level': 'sierra' } });
    try {
      assert.equal(await game.eval('window.__mr.level'), 'sierra');
      const scenes0 = await game.eval('window.__mr.scenes');
      await pick(game, 'seaside');
      await pick(game, 'sierra');
      assert.equal(await game.eval('window.__mr.scenes'), scenes0, 'no scene loaded for a tab');
      await startFromMenu(game, { timeout: 60000 });
      await waitRacing(game, 30000);
      assert.equal(await game.eval('window.__mr.level'), 'sierra');
      assert.equal(await game.eval('window.__mr.scenes'), scenes0 + 1, 'Race loaded the level');
      assert.equal(await game.eval('window.__mr.sections.full'), 'sierra');
      assert.deepEqual(game.errors, []);
    } finally { await game.close(); }
  });
}
