// The radio on the Rust build (radio.md 7, DECISIONS D1151): the station
// node comes up once the sound is woken, the level's station is tuned by
// default, T steps round the dial (the stations, then the playlist) and the
// choice is remembered. `__mp.audio.tuned` is the station the node plays
// (an index in mp_music::radio::STATIONS, -1 none), `radioReady` whether
// the node is built and tuned, `station` the setting.
//
//   cargo xtask web --release && node --test tools/parity/e2e/radio.test.mjs

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame, sleep, stored } from './harness.mjs';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

const audio = (game) => game.eval(() => window.__mp.audio || null);

async function waitTuned(game, want, what) {
  for (let i = 0; i < 80; i++) {
    const a = await audio(game);
    if (a && a.radioReady && a.tuned === want) return a;
    await sleep(250);
  }
  assert.fail(`${what}: ${JSON.stringify(await audio(game))}`);
}

test('desktop: the level\'s station plays once the sound is up, and T steps round the dial', async () => {
  const game = await openGame(browser, { device: 'desktop' });
  try {
    const before = await audio(game);
    assert.ok(!before || !before.ready, 'nothing before a gesture');
    // A real click wakes the sound (the gesture rule); Coast tunes The Tide.
    await game.page.mouse.click(640, 720);
    await waitTuned(game, 0, 'The Tide after the first click');
    assert.equal(await stored(game, 'station'), null, 'the default setting is not written');

    await game.page.keyboard.press('KeyT');
    await waitTuned(game, 1, 'Ridgeline after T');
    assert.equal(await stored(game, 'station'), 'ridgeline');
    await game.page.keyboard.press('KeyT');
    await waitTuned(game, 2, 'Radio Pacífico after T T');
    await game.page.keyboard.press('KeyT');
    await waitTuned(game, -1, 'the playlist after T T T');
    assert.equal(await stored(game, 'station'), 'playlist');
    let playing = null;
    for (let i = 0; i < 20 && playing !== 'seabright'; i++) { await sleep(250); playing = (await audio(game)).playing; }
    assert.equal(playing, 'seabright', 'the level\'s own song plays on the playlist');
    await game.page.keyboard.press('KeyT');
    await waitTuned(game, 0, 'round to The Tide');

    // The choice survives a reload.
    await game.page.reload();
    await game.waitReady?.();
    await sleep(500);
    await game.page.mouse.click(640, 720);
    await waitTuned(game, 0, 'The Tide again after a reload');
    assert.deepEqual(game.errors, [], 'no page errors');
  } finally { await game.close(); }
});
