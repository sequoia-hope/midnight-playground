// The soundtrack player (music.html), reached from the menu's Music player
// link: play and pause, every track, next/previous, jumping by section and
// along the timeline, soloing parts, repeat, volume, the level meter, the
// keyboard, and the way back to the game.

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame } from './harness.js';
import { TRACKS, PLAYLIST } from '../../src/game/audio/tracks.js';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const state = () => ({
  track: window.__player.track,
  playing: window.__player.playing,
  audio: window.__audio.ctx?.state ?? 'none',
  pos: window.__player.position(),
  engine: window.__audio.music?.song?.T.id ?? null,
});
const openPlayer = (opts = {}) => openGame(browser, { path: 'music.html', ...opts });
const bars = (id) => TRACKS.find((t) => t.id === id).sections.reduce((a, s) => a + s.bars, 0);

test('phone: the menu links to the player, which lists every track', async () => {
  const game = await openGame(browser, { device: 'phone' });
  try {
    await game.tap('#link-music');
    await game.waitReady();
    assert.match(await game.eval('location.pathname'), /music\.html$/);
    const rows = await game.eval(() => [...document.querySelectorAll('#tracks .row')].map((b) => b.dataset.track));
    assert.deepEqual(rows, PLAYLIST, 'all seven, in playlist order');
    assert.equal(await game.eval(() => document.getElementById('np-title').textContent), TRACKS.find((t) => t.id === PLAYLIST[0]).title);
    assert.equal((await game.eval(state)).audio, 'none', 'no audio before a tap');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('phone: Play starts the sound, Pause holds the place, Play carries on', async () => {
  const game = await openPlayer({ device: 'phone' });
  try {
    await game.tap('#play');
    await game.waitFor(() => window.__player.playing && window.__audio.ctx.state === 'running' && window.__player.position()?.time > 1.5,
      { what: 'the song to play' });
    await game.tap('#play');
    await game.waitFor(() => !window.__player.playing && window.__audio.ctx.state === 'suspended', { what: 'pause' });
    const held = (await game.eval(state)).pos.time;
    await sleep(800);
    assert.equal((await game.eval(state)).pos.time, held, 'the position stands still while paused');
    await game.tap('#play');
    await game.waitFor(() => window.__audio.ctx.state === 'running', { what: 'resume' });
    await game.waitFor(`window.__player.position().time > ${held + 0.5}`, { what: 'the song to carry on' });
    const s = await game.eval(state);
    assert.ok(s.pos.time < held + 5, 'it carried on from where it paused, not from the top');
    assert.deepEqual(game.errors, []);
    assert.deepEqual(game.warnings, []);
  } finally { await game.close(); }
});

test('phone: every track in the list plays when tapped', async () => {
  const game = await openPlayer({ device: 'phone' });
  try {
    for (const id of PLAYLIST) {
      await game.tap(`#tracks .row[data-track="${id}"]`);
      await game.waitFor(`window.__audio.music.song?.T.id === ${JSON.stringify(id)} && window.__player.position()?.bar === 0`, { what: 'track ' + id });
      const s = await game.eval(state);
      assert.equal(s.track, id);
      assert.equal(s.audio, 'running');
      assert.equal(await game.eval(() => document.querySelector('#tracks .row.cur')?.dataset.track), id, 'the list marks it');
    }
    assert.deepEqual(game.errors, []);
    assert.deepEqual(game.warnings, []);
  } finally { await game.close(); }
});

test('phone: next and previous go through the playlist and wrap round', async () => {
  const game = await openPlayer({ device: 'phone' });
  try {
    await game.tap('#next');
    await game.waitFor(`window.__audio.music.song?.T.id === ${JSON.stringify(PLAYLIST[1])}`, { what: 'next' });
    await game.tap('#prev');
    await game.tap('#prev');
    await game.waitFor(`window.__audio.music.song?.T.id === ${JSON.stringify(PLAYLIST.at(-1))}`, { what: 'wrap to the last' });
    await game.tap('#next');
    await game.waitFor(`window.__audio.music.song?.T.id === ${JSON.stringify(PLAYLIST[0])}`, { what: 'wrap to the first' });
  } finally { await game.close(); }
});

test('phone: tapping a section or the timeline jumps there', async () => {
  const game = await openPlayer({ device: 'phone' });
  try {
    const T = TRACKS.find((t) => t.id === PLAYLIST[0]);
    const k = 3, start = T.sections.slice(0, k).reduce((a, s) => a + s.bars, 0);
    await game.tap(`#sections .row[data-sec="${k}"]`);
    await game.waitFor(`window.__player.position()?.sec === ${k}`, { what: 'section ' + (k + 1) });
    const p = (await game.eval(state)).pos;
    assert.ok(p.bar >= start && p.bar <= start + 1, `starts at its first bar (${start}), got ${p.bar}`);
    assert.equal(await game.eval(() => document.querySelector('#sections .row.cur')?.dataset.sec), String(k), 'the list marks it');

    // The middle of the timeline is the middle bar.
    await game.tap('#timeline');
    const mid = Math.floor(bars(T.id) / 2);
    await game.waitFor(`Math.abs(window.__player.position()?.bar - ${mid}) <= 1`, { what: 'the middle bar' });
  } finally { await game.close(); }
});

test('phone: soloing a part plays it alone, and Full mix brings everything back', async () => {
  const game = await openPlayer({ device: 'phone' });
  try {
    await game.tap('#play');
    await game.waitFor(() => window.__player.playing);
    const part = Object.keys(TRACKS.find((t) => t.id === PLAYLIST[0]).parts)[0];
    await game.tap(`#parts .chip[data-part="${part}"]`);
    assert.equal(await game.eval(() => window.__audio.music.solo), part);
    assert.equal(await game.eval((p) => document.querySelector(`#parts .chip[data-part="${p}"]`).getAttribute('aria-pressed'), part), 'true');
    await game.tap('#parts .chip[data-part="drums"]');
    assert.equal(await game.eval(() => window.__audio.music.solo), 'drums');
    await game.tap('#parts .chip:not([data-part])');
    assert.equal(await game.eval(() => window.__audio.music.solo), null);

    // A solo on a part the next song doesn't have is dropped.
    const other = PLAYLIST.find((id) => !TRACKS.find((t) => t.id === id).parts.acid);
    const withAcid = PLAYLIST.find((id) => TRACKS.find((t) => t.id === id).parts.acid);
    if (other && withAcid) {
      await game.tap(`#tracks .row[data-track="${withAcid}"]`);
      await game.tap('#parts .chip[data-part="acid"]');
      await game.tap(`#tracks .row[data-track="${other}"]`);
      assert.equal(await game.eval(() => window.__audio.music.solo), null);
    }
  } finally { await game.close(); }
});

test('desktop: at the end of a song, repeat plays it again, otherwise the playlist moves on', async () => {
  const game = await openPlayer({ device: 'desktop' });
  try {
    // Neon Rush is the fastest (172 BPM), so its last bar is short.
    const id = 'neon-rush', next = PLAYLIST[(PLAYLIST.indexOf(id) + 1) % PLAYLIST.length];
    const lastBar = async () => {
      const r = await game.eval(() => { const b = document.getElementById('timeline').getBoundingClientRect(); return { x: b.right - 2, y: b.top + b.height / 2 }; });
      await game.page.mouse.click(r.x, r.y);
      await game.waitFor(`window.__player.position()?.bar === ${bars(id) - 1}`, { what: 'the last bar' });
    };

    await game.click(`#tracks .row[data-track="${id}"]`);
    await game.click('#repeat');
    assert.equal(await game.eval(() => window.__player.repeat), true);
    assert.deepEqual(await game.eval(() => window.__audio.music.playlist), [id]);
    await lastBar();
    await game.waitFor(`window.__audio.music.song?.T.id === ${JSON.stringify(id)} && window.__player.position()?.bar < 2`, { timeout: 10000, what: 'the song to start again' });

    await game.click('#repeat');
    assert.equal(await game.eval(() => window.__player.repeat), false);
    await lastBar();
    await game.waitFor(`window.__audio.music.song?.T.id === ${JSON.stringify(next)}`, { timeout: 10000, what: 'the next song' });
    await game.waitFor(`window.__player.track === ${JSON.stringify(next)}`, { timeout: 5000, what: 'the page to follow it' });
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('desktop: volume, repeat and the last track are remembered', async () => {
  const game = await openPlayer({ device: 'desktop' });
  try {
    await game.click(`#tracks .row[data-track="${PLAYLIST[2]}"]`);
    await game.click('#repeat');
    const r = await game.eval(() => { const b = document.getElementById('vol').getBoundingClientRect(); return { x: b.left + b.width * 0.25, y: b.top + b.height / 2 }; });
    await game.page.mouse.click(r.x, r.y);
    const v = Number(await game.eval(() => document.getElementById('vol').value));
    assert.ok(v > 10 && v < 40, `the slider moved to about a quarter (${v})`);
    assert.ok(Math.abs(await game.eval(() => window.__audio._vol.music) - v / 100) < 1e-6, 'and applies to the music');
    await game.reload();
    assert.equal(await game.eval(() => document.getElementById('vol').value), String(v));
    assert.equal(await game.eval(() => window.__player.track), PLAYLIST[2]);
    assert.equal(await game.eval(() => window.__player.repeat), true);
  } finally { await game.close(); }
});

test('desktop: the meter reads the output while a song plays', async () => {
  const game = await openPlayer({ device: 'desktop' });
  try {
    assert.equal(await game.eval(() => document.getElementById('m-avg').textContent), '—');
    await game.click('#play');
    await game.waitFor(() => /dB/.test(document.getElementById('m-avg').textContent) && window.__player.position()?.time > 2, { what: 'the meter' });
    const avg = parseFloat(await game.eval(() => document.getElementById('m-avg').textContent.replace('−', '-')));
    assert.ok(avg > -60 && avg < 0, `a sane average level (${avg} dB)`);
  } finally { await game.close(); }
});

test('desktop: Space plays and pauses, arrows jump sections, N and P change track', async () => {
  const game = await openPlayer({ device: 'desktop' });
  try {
    await game.key('Space');
    await game.waitFor(() => window.__player.playing && window.__audio.ctx.state === 'running', { what: 'Space to play' });
    await game.key('ArrowRight');
    await game.waitFor(() => window.__player.position()?.sec === 1, { what: 'the next section' });
    await game.key('ArrowLeft');
    await game.waitFor(() => window.__player.position()?.sec === 0, { what: 'the previous section' });
    await game.key('KeyN');
    await game.waitFor(`window.__audio.music.song?.T.id === ${JSON.stringify(PLAYLIST[1])}`, { what: 'N' });
    await game.key('KeyP');
    await game.waitFor(`window.__audio.music.song?.T.id === ${JSON.stringify(PLAYLIST[0])}`, { what: 'P' });
    await game.key('Space');
    await game.waitFor(() => !window.__player.playing, { what: 'Space to pause' });
  } finally { await game.close(); }
});

test('phone: Back to the game returns to the menu', async () => {
  const game = await openPlayer({ device: 'phone' });
  try {
    await game.tap('#back');
    await game.waitReady();
    assert.match(await game.eval('location.pathname'), /\/(index\.html)?$/);
    assert.equal(await game.screen(), 'menu');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});
