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

// Skipping and rating (radio.md 8). The harness serves files without a
// server, so favourites.json comes back read-only (no X-Favourites header)
// and the rating controls stay hidden, as on Pages; this test stands in for
// tools/serve.py with a fetch that answers the file writable and records
// the POST.
test('skips ahead on its own clock, rates the song where the file is writable, and comes back live', async () => {
  const game = await openGame(browser, { wait: false });
  try {
    await game.page.evaluateOnNewDocument(() => {
      window.__posted = [];
      const real = window.fetch.bind(window);
      let songs = [];
      window.fetch = async (url, init = {}) => {
        if (!String(url).endsWith('/crates/mp_music/favourites.json')) return real(url, init);
        if (init.method === 'POST') {
          const song = JSON.parse(init.body);
          window.__posted.push(song);
          songs = songs.filter((s) => s.genre !== song.genre || s.seed !== song.seed).concat(song);
        }
        return new Response(JSON.stringify({ songs }), { status: 200, headers: { 'Content-Type': 'application/json', 'X-Favourites': 'writable' } });
      };
    });
    const origin = new URL(game.page.url()).origin;
    await game.page.goto(`${origin}/tools/radio.html`);
    await game.waitFor(() => window.radio && document.querySelectorAll('#dial button').length >= 3, { what: 'the dial' });
    assert.equal(await game.eval(() => document.getElementById('rate').hidden), false, 'the rating controls show where the file is writable');

    await game.page.click('#play');
    await until(game, (x) => x.station === 0 && x.ctx === 'running', 'playing');
    const before = await game.eval(() => ({ block: window.radio.schedule.block, slot: window.radio.schedule.slot, skew: window.radio.skew, live: document.getElementById('live').hidden }));
    assert.equal(before.skew, 0);
    assert.equal(before.live, true, 'Live is hidden while live');

    // Skip: the next song, on a clock now ahead of the station's.
    await game.page.click('#skip');
    await sleep(300);
    const after = await game.eval(() => ({ block: window.radio.schedule.block, slot: window.radio.schedule.slot, into: window.radio.schedule.into - window.radio.schedule.slots[window.radio.schedule.slot].start, skew: window.radio.skew, live: document.getElementById('live').hidden }));
    assert.ok(after.skew > 0, 'the clock is ahead');
    assert.ok(after.block > before.block || after.slot === before.slot + 1, `the next song: ${JSON.stringify({ before, after })}`);
    assert.ok(after.into < 5, 'at the song\'s start');
    assert.equal(after.live, false, 'Live offered');

    // Keep it, with a note: the POST carries the pair and what the page knows.
    await game.page.type('#note', 'the bass walks');
    await game.page.click('#keep');
    await game.waitFor(() => window.__posted.length === 1 && document.getElementById('saved').textContent.includes('saved'), { what: 'the verdict saved' });
    const posted = await game.eval(() => window.__posted[0]);
    const cur = await game.eval(() => window.radio.schedule.slots[window.radio.schedule.slot]);
    assert.equal(posted.verdict, 'keep');
    assert.equal(posted.note, 'the bass walks');
    assert.equal(posted.genre, cur.genre);
    assert.equal(posted.seed, cur.seed);
    assert.equal(posted.title, cur.title);
    assert.equal(posted.station, 'tide');
    assert.ok(posted.wall > 1767225600, 'the song\'s moment on the station clock');
    assert.equal(await game.eval(() => document.getElementById('keep').classList.contains('on')), true);
    assert.equal(await game.eval(() => [...document.querySelectorAll('#prog-table td.v.keep')].length), 1, 'the programme marks it');

    // Reject replaces the verdict for the same pair.
    await game.page.click('#reject');
    await game.waitFor(() => window.__posted.length === 2 && document.getElementById('reject').classList.contains('on'), { what: 'the second verdict shown' });
    assert.equal(await game.eval(() => document.getElementById('keep').classList.contains('on')), false);

    // Live: the clock is the station's again.
    await game.page.click('#live');
    await sleep(300);
    const back = await game.eval(() => ({ skew: window.radio.skew, live: document.getElementById('live').hidden }));
    assert.equal(back.skew, 0);
    assert.equal(back.live, true);
    assert.deepEqual(game.errors, [], 'no page errors');
  } finally { await game.close(); }
});

// The page's energy grows with the song, and a DJ is cut off by a station
// change.
test('energy follows the song until the slider takes over, and the DJ stops when the dial moves', async () => {
  const game = await openRadio(browser);
  try {
    await game.page.click('#play');
    await until(game, (x) => x.station === 0 && x.ctx === 'running', 'playing');
    await sleep(1200);
    const e = await game.eval(() => {
      const sc = window.radio.schedule;
      const s = sc.slots[sc.slot];
      return { frac: (sc.into - s.start) / s.secs, slider: Number(document.getElementById('energy').value), auto: document.getElementById('energyAuto').checked };
    });
    assert.equal(e.auto, true);
    const want = 0.5 + 0.5 * Math.min(1, e.frac / 0.67);
    assert.ok(Math.abs(e.slider - want) < 0.02, `the slider follows the song: ${JSON.stringify({ e, want })}`);
    // A hand on the slider takes over.
    await game.eval(() => { const el = document.getElementById('energy'); el.value = 0.2; el.dispatchEvent(new Event('input')); });
    await sleep(1200);
    const h = await game.eval(() => ({ slider: Number(document.getElementById('energy').value), auto: document.getElementById('energyAuto').checked }));
    assert.equal(h.auto, false);
    assert.equal(h.slider, 0.2);

    // The DJ speaks on The Tide; stepping to Ridgeline cuts her off and the
    // station comes back up.
    await game.eval(() => window.radio.sayDj());
    await game.waitFor(() => window.radio.djPlaying && document.getElementById('dj').textContent.startsWith('marisol'), { what: 'the DJ talking' });
    await game.page.click('#next');
    await until(game, (x) => x.station === 1, 'Ridgeline');
    await sleep(400);
    const d = await game.eval(() => ({ playing: window.radio.djPlaying, text: document.getElementById('dj').textContent, gain: window.radio.stationGain }));
    assert.equal(d.playing, false, 'the clip stopped');
    assert.equal(d.text, '');
    assert.ok(d.gain > 0.95, `the station is back up: ${d.gain}`);
    assert.deepEqual(game.errors, [], 'no page errors');
  } finally { await game.close(); }
});
