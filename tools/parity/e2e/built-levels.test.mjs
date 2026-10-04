// Desert Run and Downtown Streets built by the client itself (`?world=gen`,
// no scene download; DECISIONS D720 on): each level is ready behind the
// loading screen with its animators, a race starts from the menu, the
// autopilot drives it while the scenery's animators run (the player's s
// moves on), and the finish brings the results screen, with no page error.
// Then the menu's level tab switches to the other level and back, built
// again each time (the JS builds a new world).
//
//   cargo xtask web --release && node --test tools/parity/e2e/built-levels.test.mjs

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame, startFromMenu, waitRacing, expectScreen, sleep } from './harness.mjs';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

const NAMES = { desert: 'Desert Run', streets: 'Downtown Streets' };

for (const level of ['desert', 'streets']) {
  test(`${level} (world=gen): a race from the menu to the results`, async (t) => {
    // The saved level, not `?level=` (which races at once, D570).
    const game = await openGame(browser, { query: 'world=gen&timescale=2&autodrive=1', storage: { 'mr.level': level } });
    try {
      assert.equal(await game.eval('window.__mr.level'), level);
      assert.equal((await game.ui('#lvl-name')).value, NAMES[level]);
      await startFromMenu(game, { timeout: 60000 });
      await waitRacing(game, 30000);
      const s0 = await game.eval('window.__mr.race.s');
      await sleep(8000);
      const r = await game.eval(() => ({ state: window.__mr.race.state, time: window.__mr.race.time, s: window.__mr.race.s }));
      assert.equal(r.state, 'racing');
      assert.ok(r.time > 5, `the race clock runs (${r.time})`);
      assert.ok(r.s > s0 + 50, `the autopilot drives on (s ${s0} → ${r.s})`);
      await game.eval(() => window.__mr.stage({ cmd: 'finish', rivalWon: false }));
      await game.waitFor(() => window.__mr.race.finished, { timeout: 30000, what: 'the player to cross the line' });
      await expectScreen(game, 'results', 30000);
      // SPEC 6.6: the wasm memory's high-water mark under 512 MB, the race's
      // cars and sound included.
      const mb = await game.eval(() => Math.round(window.__mr.wasmMemoryBytes() / 1048576));
      t.diagnostic(`${level}: wasm memory after the race ${mb} MB`);
      assert.ok(mb < 512, `wasm memory ${mb} MB`);
      assert.deepEqual(game.errors, []);
    } finally { await game.close(); }
  });
}

test('level tabs: Desert Run → Downtown Streets → Desert Run (world=gen)', async () => {
  const game = await openGame(browser, { query: 'world=gen&timescale=2', storage: { 'mr.level': 'desert' } });
  try {
    assert.equal(await game.eval('window.__mr.level'), 'desert');
    for (const id of ['streets', 'desert']) {
      await game.click(`#lvl-tab-${id}`);
      await game.waitFor(`window.__mr.level === ${JSON.stringify(id)} && window.__mr.mode === 'menu' && window.__mr.ready`,
        { timeout: 300000, interval: 250, what: `${id} to build` });
      await game.frames(5);
      assert.equal((await game.ui('#lvl-name')).value, NAMES[id]);
    }
    await startFromMenu(game, { timeout: 60000 });
    await waitRacing(game, 30000);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});
