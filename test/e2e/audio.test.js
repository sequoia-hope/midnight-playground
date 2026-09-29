// Sound: the Race tap starts audio on a phone, pause suspends it and resume
// brings it back, and an interrupted context (iOS does this on a call) comes
// back on the next touch or key, which is the unlock() path. Also the music
// controls (M, T, Next track, the track picker, the volume sliders) and a
// race that logs no console warnings.

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame } from './harness.js';
import { LEVEL_TRACK, TRACKS } from '../../src/game/audio/tracks.js';
import { sleep, simWait, startRace, holdKeys } from './controls-helpers.js';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

const Q = 'timescale=2';
const ctxState = () => window.__audio.ctx?.state ?? 'none';
const trackId = () => window.__audio.trackInfo?.id ?? null;
const sliders = () => ({
  music: [...document.querySelectorAll('.vol-music')].map((e) => e.value),
  sfx: [...document.querySelectorAll('.vol-sfx')].map((e) => e.value),
});

// Click a slider at a fraction of its width (a real mouse click on the track).
async function setSlider(game, selector, frac) {
  await game.center(selector);
  const r = await game.eval((sel) => { const b = document.querySelector(sel).getBoundingClientRect(); return { l: b.left, w: b.width, y: b.top + b.height / 2 }; }, selector);
  await game.page.mouse.click(r.l + r.w * frac, r.y);
  return Number(await game.eval((sel) => document.querySelector(sel).value, selector));
}

test('phone: the Race tap starts the audio and brings the music up', async () => {
  const game = await openGame(browser, { device: 'phone', query: Q });
  try {
    assert.equal(await game.eval(ctxState), 'none', 'no audio before the first touch');
    await startRace(game, { racing: false });
    await game.waitFor(() => window.__audio.ctx.state === 'running', { what: 'audio to run' });
    await game.waitFor(() => window.__audio._musicOn && window.__audio.musicGate.gain.value > 0.5, { timeout: 10000, what: 'the music to fade in' });
    assert.equal(await game.eval(() => window.__audio.ready), true);
    assert.ok(Math.abs(await game.eval(() => window.__audio._vol.music) - 0.7) < 1e-6, 'default music volume');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('phone: pausing suspends the audio, taps on the pause screen leave it, Resume restarts it', async () => {
  const game = await openGame(browser, { device: 'phone', query: Q });
  try {
    await startRace(game);
    await game.waitFor(() => window.__audio.ctx.state === 'running', { what: 'audio to run' });
    await game.tap('#touch [data-tap="pause"]');
    await game.waitFor(() => window.__audio.ctx.state === 'suspended', { what: 'pause to suspend the audio' });
    // A touch that isn't Resume must not wake the audio while paused.
    await game.tap('#pause .title');
    await sleep(400);
    assert.equal(await game.eval(ctxState), 'suspended', 'still silent while paused');
    await game.tap('#btn-resume');
    await game.waitFor(() => window.__audio.ctx.state === 'running', { what: 'Resume to restart the audio' });
  } finally { await game.close(); }
});

test('phone: an interrupted audio context comes back on the next touch', async () => {
  const game = await openGame(browser, { device: 'phone', query: Q });
  try {
    await startRace(game);
    await game.waitFor(() => window.__audio.ctx.state === 'running', { what: 'audio to run' });
    // What iOS does on a phone call or a notification.
    await game.eval(() => window.__audio.ctx.suspend());
    await game.waitFor(() => window.__audio.ctx.state === 'suspended', { what: 'the interruption' });
    await sleep(300);
    assert.equal(await game.eval(ctxState), 'suspended', 'nothing restarts it on its own');
    // A touch on an empty part of the screen, over the race.
    const { w, h } = await game.eval(() => ({ w: innerWidth, h: innerHeight }));
    await game.page.touchscreen.tap(w / 2, h * 0.4);
    await game.waitFor(() => window.__audio.ctx.state === 'running', { what: 'the touch to restart the audio' });
    assert.equal(await game.eval(() => window.__game.mode), 'race', 'and the race carries on');
  } finally { await game.close(); }
});

test('desktop: an interrupted audio context comes back on the next key', async () => {
  const game = await openGame(browser, { device: 'desktop', query: Q });
  try {
    await startRace(game, { how: 'click' });
    await game.waitFor(() => window.__audio.ctx.state === 'running', { what: 'audio to run' });
    await game.eval(() => window.__audio.ctx.suspend());
    await game.waitFor(() => window.__audio.ctx.state === 'suspended', { what: 'the interruption' });
    await game.key('KeyW', 100);
    await game.waitFor(() => window.__audio.ctx.state === 'running', { what: 'the key to restart the audio' });
  } finally { await game.close(); }
});

test('desktop: M mutes and unmutes the music, and mute survives a reload', async () => {
  const game = await openGame(browser, { device: 'desktop', query: Q });
  try {
    await startRace(game, { how: 'click' });
    await game.waitFor(() => window.__audio._musicOn, { what: 'the music to start' });
    await game.key('KeyM');
    await game.waitFor(() => localStorage.getItem('mr.musicVol') === '0' && !window.__audio._musicOn, { what: 'M to mute' });
    assert.deepEqual((await game.eval(sliders)).music, ['0', '0'], 'both music sliders drop to 0');
    assert.equal(await game.eval(() => window.__audio._vol.music), 0);

    await game.key('KeyM');
    await game.waitFor(() => localStorage.getItem('mr.musicVol') === '0.7' && window.__audio._musicOn, { what: 'M to unmute' });
    assert.deepEqual((await game.eval(sliders)).music, ['70', '70']);

    await game.key('KeyM');
    await game.waitFor(() => localStorage.getItem('mr.musicVol') === '0', { what: 'M to mute again' });
    await game.reload();
    assert.deepEqual((await game.eval(sliders)).music, ['0', '0'], 'still muted after a reload');
    await startRace(game, { how: 'click', racing: false });
    await sleep(500);
    assert.equal(await game.eval(() => window.__audio._musicOn), false, 'a muted race starts without music');
  } finally { await game.close(); }
});

test('desktop: T and the pause screen\'s Next track change the track', async () => {
  const game = await openGame(browser, { device: 'desktop', query: Q });
  try {
    await startRace(game, { how: 'click' });
    const id0 = await game.waitFor(trackId, { what: 'a track to play' });
    assert.equal(id0, LEVEL_TRACK.sierra, 'the level starts on its own track');
    await game.key('KeyT');
    const id1 = await game.waitFor(`(${trackId})() !== ${JSON.stringify(id0)} && (${trackId})()`, { what: 'T to change the track' });
    await game.key('Escape');
    await game.waitFor(() => window.__game.mode === 'paused', { what: 'pause' });
    await game.click('#btn-next-track');
    const id2 = await game.waitFor(`(${trackId})() !== ${JSON.stringify(id1)} && (${trackId})()`, { what: 'Next track to change the track' });
    const title = TRACKS.find((t) => t.id === id2)?.title;
    assert.ok(title, `${id2} is a known track`);
    assert.match(await game.eval(() => document.getElementById('np-pause').textContent), new RegExp(title), 'the pause screen names it');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('desktop: the music and SFX sliders stay in sync between menu and pause, and persist', async () => {
  const game = await openGame(browser, { device: 'desktop', query: Q });
  try {
    const m = await setSlider(game, '#menu .vol-music', 0.2);
    const s = await setSlider(game, '#menu .vol-sfx', 0.45); // the default is 85
    assert.ok(m > 5 && m < 40 && s > 30 && s < 60, `sliders moved (music ${m}, sfx ${s})`);
    assert.deepEqual(await game.eval(sliders), { music: [String(m), String(m)], sfx: [String(s), String(s)] }, 'the pause sliders follow the menu');
    assert.equal(Number(await game.eval(() => localStorage.getItem('mr.musicVol'))), m / 100);
    assert.equal(Number(await game.eval(() => localStorage.getItem('mr.sfxVol'))), s / 100);

    await startRace(game, { how: 'click' });
    await game.waitFor(() => window.__audio.ready, { what: 'audio' });
    let vol = await game.eval(() => ({ ...window.__audio._vol }));
    assert.ok(Math.abs(vol.music - m / 100) < 1e-6 && Math.abs(vol.sfx - s / 100) < 1e-6, 'the race uses the chosen volumes');

    await game.key('Escape');
    await game.waitFor(() => window.__game.mode === 'paused', { what: 'pause' });
    const m2 = await setSlider(game, '#pause .vol-music', 0.6);
    assert.ok(Math.abs(m2 - m) > 10, `pause slider moved (${m} → ${m2})`);
    assert.deepEqual((await game.eval(sliders)).music, [String(m2), String(m2)], 'the menu slider follows the pause one');
    vol = await game.eval(() => ({ ...window.__audio._vol }));
    assert.ok(Math.abs(vol.music - m2 / 100) < 1e-6, 'applied live');

    await game.reload();
    assert.deepEqual(await game.eval(sliders), { music: [String(m2), String(m2)], sfx: [String(s), String(s)] }, 'both survive a reload');
  } finally { await game.close(); }
});

test('desktop: the track picker\'s choice is what plays, and it is remembered', async () => {
  const game = await openGame(browser, { device: 'desktop', query: Q });
  try {
    const own = LEVEL_TRACK.sierra;
    const pick = TRACKS.find((t) => t.id !== own).id;
    const options = await game.eval(() => [...document.getElementById('opt-track').options].map((o) => o.value));
    assert.deepEqual(options, ['auto', ...TRACKS.map((t) => t.id)], 'the level\'s own track, then every song');
    await game.eval((id) => { const s = document.getElementById('opt-track'); s.value = id; s.dispatchEvent(new Event('change')); }, pick);
    assert.equal(await game.eval(() => localStorage.getItem('mr.track')), JSON.stringify(pick));
    await startRace(game, { how: 'click', racing: false });
    await game.waitFor(`(${trackId})() === ${JSON.stringify(pick)}`, { what: `${pick} to play` });

    await game.reload();
    assert.equal(await game.eval(() => document.getElementById('opt-track').value), pick, 'remembered');
    // Back to the level's own track, picked while the audio is running.
    await startRace(game, { how: 'click', racing: false });
    await game.waitFor(`(${trackId})() === ${JSON.stringify(pick)}`, { what: `${pick} to play again` });
    await game.key('Escape');
    await game.waitFor(() => window.__game.mode === 'paused', { what: 'pause' });
    await game.click('#btn-quit');
    await game.waitFor(() => window.__game.mode === 'menu', { what: 'the menu' });
    await game.eval(() => { const s = document.getElementById('opt-track'); s.value = 'auto'; s.dispatchEvent(new Event('change')); });
    await game.waitFor(`(${trackId})() === ${JSON.stringify(own)}`, { what: 'the level\'s own track' });
  } finally { await game.close(); }
});

test('desktop: a few seconds of hard driving log no warnings or errors', async () => {
  const game = await openGame(browser, { device: 'desktop', query: Q });
  try {
    await startRace(game, { how: 'click' });
    await holdKeys(game, 'KeyW', 1500);
    await holdKeys(game, ['KeyW', 'ShiftLeft'], 800);
    await holdKeys(game, ['KeyW', 'KeyD', 'Space'], 600);
    await holdKeys(game, 'KeyS', 800);
    await simWait(game, 0.5);
    assert.deepEqual(game.warnings, [], 'no console warnings');
    assert.deepEqual(game.errors, [], 'no console errors');
  } finally { await game.close(); }
});
