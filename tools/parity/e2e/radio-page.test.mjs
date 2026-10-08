// The Radio page as a music app (radio.md 7, tools/radio.html): the graph
// ends in a media element playing a stream, the Media Session carries the
// station and the song with play, pause and next, the dial steps, and the
// last station is remembered.
//
//   cargo xtask web --release && node --test tools/parity/e2e/radio-page.test.mjs

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame, sleep } from './harness.mjs';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

// The harness opens the game; the Radio page lives at the same origin.
async function openRadio(browser, device = 'desktop') {
  const game = await openGame(browser, { device, wait: false });
  const origin = new URL(game.page.url()).origin;
  await game.page.goto(`${origin}/tools/radio.html`);
  await game.waitFor(() => window.radio && document.querySelectorAll('#dial button').length >= 3, { what: 'the dial' });
  return game;
}

const state = (game) => game.eval(() => ({
  station: window.radio.station,
  ctx: window.radio.ctx?.state ?? null,
  out: window.radio.out ? { paused: window.radio.out.paused, stream: window.radio.out.srcObject instanceof MediaStream } : null,
  level: window.radio.level,
  play: document.getElementById('play').textContent,
  media: 'mediaSession' in navigator ? { state: navigator.mediaSession.playbackState, title: navigator.mediaSession.metadata?.title ?? null, artist: navigator.mediaSession.metadata?.artist ?? null, art: navigator.mediaSession.metadata?.artwork?.length ?? 0 } : null,
  stored: localStorage.getItem('radio.station'),
}));

async function until(game, pred, what) {
  for (let i = 0; i < 60; i++) {
    const s = await state(game);
    if (pred(s)) return s;
    await sleep(250);
  }
  assert.fail(`${what}: ${JSON.stringify(await state(game))}`);
}

test('the page plays through a media element with the lock screen\'s metadata, steps the dial and remembers the station', async () => {
  const game = await openRadio(browser);
  try {
    const before = await state(game);
    assert.equal(before.ctx, null, 'no context before a gesture');
    assert.equal(before.play, '▶');

    // A real click on the play button starts the last station (The Tide).
    await game.page.click('#play');
    let s = await until(game, (x) => x.station === 0 && x.ctx === 'running' && x.out && !x.out.paused, 'playing after play');
    assert.ok(s.out.stream, 'the output element plays a MediaStream');
    assert.equal(s.play, '❚❚');
    s = await until(game, (x) => x.media?.title && x.media.state === 'playing', 'the media session');
    assert.match(s.media.artist, /^The Tide 88\.1/);
    assert.equal(s.media.art, 1, 'artwork for the lock screen');
    assert.equal(s.stored, 'tide');
    await until(game, (x) => x.level > 0.01, 'sound comes out');

    // Next steps the dial; the metadata follows.
    await game.page.click('#next');
    s = await until(game, (x) => x.station === 1 && /^Ridgeline/.test(x.media?.artist ?? ''), 'Ridgeline after next');
    assert.equal(s.stored, 'ridgeline');
    await game.page.click('#prev');
    await until(game, (x) => x.station === 0, 'The Tide after previous');

    // Pause is off: the element pauses, the session says so, play comes back.
    await game.page.click('#play');
    s = await until(game, (x) => x.station === -1 && x.out.paused && x.media?.state === 'paused', 'paused');
    assert.equal(s.play, '▶');
    await game.page.click('#play');
    await until(game, (x) => x.station === 0 && !x.out.paused, 'playing again');

    // Reloaded, the page offers the remembered station.
    await game.page.click('#next');
    await until(game, (x) => x.station === 1, 'Ridgeline');
    await game.page.reload();
    await game.waitFor(() => window.radio && document.querySelectorAll('#dial button').length >= 3, { what: 'the dial again' });
    await game.page.click('#play');
    await until(game, (x) => x.station === 1 && x.ctx === 'running', 'Ridgeline again after a reload');
    assert.deepEqual(game.errors, [], 'no page errors');
  } finally { await game.close(); }
});
