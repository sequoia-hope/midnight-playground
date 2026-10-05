// The Race button against the Rust build, beyond test/e2e/race-button.test.js
// (which runs against it with `npm run test:e2e:rust`, WP 6.7): a second tap
// while the race is starting starts one race. The JS suite's version holds
// three's compileAsync and traps `window.__race`; here the client counts
// its races (`__mr.races`).
//
//   cargo xtask web --release && node --test tools/parity/e2e/race-button.test.mjs

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame, raceStarted, sleep } from './harness.mjs';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

async function expectRaceStarts(game) {
  await game.waitFor(raceStarted, { timeout: 30000, what: 'the race to start' });
  const s = await game.snapshot();
  assert.equal(s.screen, 'none', 'no menu or other screen over the race');
  assert.equal(s.mode, 'race');
  return s;
}

test('phone: a second tap while the race is starting starts exactly one race', async () => {
  const game = await openGame(browser, { device: 'phone' });
  try {
    const { x, y } = await game.center('#btn-start');
    await game.page.touchscreen.tap(x, y);
    await sleep(30);
    await game.page.touchscreen.tap(x, y);
    await expectRaceStarts(game);
    await sleep(1500);
    assert.equal(await game.eval('window.__mr.races'), 1);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});
