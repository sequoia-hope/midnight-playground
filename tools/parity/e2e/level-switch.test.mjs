// Switching levels from the menu's tabs (roadmap WP 6.2; DECISIONS D574):
// Sierra → Seaside → Sierra, each built behind the loading screen, then a
// race on the last; with the scenes downloaded, and with `?world=gen`
// (Sierra built by the client itself, `animate`, with its animators;
// Seaside downloaded).
//
//   cargo xtask web --release && node --test tools/parity/e2e/level-switch.test.mjs

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame, startFromMenu, waitRacing } from './harness.mjs';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

async function pick(game, id) {
  await game.click(`#lvl-tab-${id}`);
  await game.waitFor(`window.__mr.level === ${JSON.stringify(id)} && window.__mr.mode === 'menu' && window.__mr.ready`,
    { timeout: 300000, interval: 250, what: `${id} to build` });
  await game.frames(5);
  assert.equal(await game.screen(), 'menu');
  assert.equal((await game.ui('#lvl-name')).value, { sierra: 'Sierra to the City', seaside: 'Seaside Raceway' }[id]);
}

for (const query of ['', 'world=gen']) {
  test(`level tabs: Sierra → Seaside → Sierra, then a race${query ? ' (' + query + ')' : ''}`, async () => {
    const game = await openGame(browser, { query: query ? query + '&timescale=2' : 'timescale=2', storage: { 'mr.level': 'sierra' } });
    try {
      assert.equal(await game.eval('window.__mr.level'), 'sierra');
      const scenes0 = await game.eval('window.__mr.scenes');
      await pick(game, 'seaside');
      await pick(game, 'sierra');
      assert.equal(await game.eval('window.__mr.scenes'), scenes0 + 2, 'two more scenes built');
      await startFromMenu(game, { timeout: 60000 });
      await waitRacing(game, 30000);
      assert.equal(await game.eval('window.__mr.level'), 'sierra');
      assert.deepEqual(game.errors, []);
    } finally { await game.close(); }
  });
}
