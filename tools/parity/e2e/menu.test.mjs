// The menu against the Rust build, beyond test/e2e/menu.test.js (which runs
// against it with `npm run test:e2e:rust`, WP 6.7): the volume sliders and
// the track picker reach the sound, and M on the pause slider.
//
//   cargo xtask web --release && node --test tools/parity/e2e/menu.test.mjs

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame, stored } from './harness.mjs';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

const TRACKS = ['midnight-run', 'seabright', 'neon-rush', 'mirage', 'interstate', 'chrome-heart', 'afterburner'];

test('desktop: the volume sliders and the track picker reach the sound', async () => {
  const game = await openGame(browser);
  try {
    // A first click wakes the sound (`wakeAudio`), as on the JS menu.
    await game.click('#car-pick .pick:nth-child(1)');
    await game.waitFor(() => window.__mr.audio?.context === 'running', { timeout: 5000, what: 'the sound to start' });
    // Music to 0: the slider's left end.
    await game.center('#vol-music'); // scrolled into view
    const u = await game.ui('#vol-music');
    await game.page.mouse.click(u.x + 2, u.y + u.h / 2);
    await game.frames(4);
    assert.equal(await stored(game, 'musicVol'), 0);
    assert.equal(await game.eval('window.__mr.audio.music'), 0, 'the sound has the new volume');
    await game.center('#vol-sfx');
    const s = await game.ui('#vol-sfx');
    await game.page.mouse.click(s.x + s.w / 2, s.y + s.h / 2);
    await game.frames(4);
    const sfx = await stored(game, 'sfxVol');
    assert.ok(sfx > 0.4 && sfx < 0.6, `SFX about half (${sfx})`);
    assert.equal(await game.eval('window.__mr.audio.sfx'), sfx);
    // A track: the sound switches to it.
    await game.click('#opt-track');
    await game.click(`#option-${TRACKS[3]}`);
    assert.equal(await stored(game, 'track'), TRACKS[3]);
    await game.waitFor(`window.__mr.audio.track === ${JSON.stringify(TRACKS[3])} && window.__mr.audio.playing === ${JSON.stringify(TRACKS[3])}`, { timeout: 5000, what: 'the chosen track to play' });
    // M (the sound's music key) shows on the pause screen's slider.
    await game.click('#btn-start');
    await game.waitFor(() => window.__mr.mode === 'race', { timeout: 30000, what: 'the race' });
    await game.key('KeyM');
    await game.waitFor(() => window.__mr.audio.music === 0.7, { timeout: 5000, what: 'M to turn the music back on' });
    await game.key('Escape');
    await game.waitFor(() => window.__mr.screen === 'pause', { timeout: 5000, what: 'pause' });
    await game.frames(3);
    assert.equal((await game.ui('#vol-music')).value, 70, 'the pause slider follows');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});
