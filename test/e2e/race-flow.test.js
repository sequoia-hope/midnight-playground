// A race from start to finish: countdown, pause (Esc, P, the touch pause
// button, leaving the page), resume, restart, back to the menu, the finish
// and results screen with best times, Race again, and ending a cruise.
//
// Most tests run at ?timescale=2 so countdowns and finish delays pass faster.

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame } from './harness.js';
import { fmtTime } from '../../src/game/HUD.js';
import { isShown, stored, startFromMenu, markRace, newRaceStarted, waitRacing, expectScreen, sleep } from './flow-helpers.js';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

const FAST = 'timescale=2';

test('desktop: the countdown turns into racing, the clock runs and the car is released', async () => {
  const game = await openGame(browser, { query: FAST });
  try {
    await startFromMenu(game);
    const c0 = await game.eval(() => ({ state: __race.state, locked: __race.phys.locked, time: __race.time, hud: !document.getElementById('hud').classList.contains('hidden') }));
    assert.equal(c0.state, 'countdown');
    assert.equal(c0.locked, true, 'the car is held during the countdown');
    assert.equal(c0.time, 0, 'the clock waits for GO');
    assert.equal(c0.hud, true, 'the HUD is up');
    await waitRacing(game, 15000);
    assert.equal(await game.eval('__race.phys.locked'), false);
    const t1 = await game.eval('__race.time');
    await sleep(400);
    assert.ok(await game.eval('__race.time') > t1, 'the race clock runs');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('desktop: Esc pauses and freezes the race; Resume carries on', async () => {
  const game = await openGame(browser, { query: FAST });
  try {
    await startFromMenu(game);
    await waitRacing(game);
    await game.key('Escape');
    await expectScreen(game, 'pause');
    const s = await game.snapshot();
    assert.equal(s.mode, 'paused');
    assert.equal(s.audio, 'suspended', 'the sound stops while paused');
    const t0 = await game.eval('__race.time');
    await sleep(500);
    assert.equal(await game.eval('__race.time'), t0, 'the clock is frozen');
    assert.equal(await game.eval(isShown, '#btn-end'), false, 'no End run outside a cruise');

    await game.click('#btn-resume');
    await expectScreen(game, 'none');
    await game.waitFor(() => window.__audio.ctx.state === 'running', { timeout: 5000, what: 'the sound to resume' });
    assert.equal((await game.snapshot()).mode, 'race');
    await game.waitFor(`__race.time > ${t0}`, { timeout: 5000, what: 'the clock to run again' });
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('desktop: P pauses too, and Esc on the pause screen resumes', async () => {
  const game = await openGame(browser, { query: FAST });
  try {
    await startFromMenu(game);
    await game.key('KeyP');
    await expectScreen(game, 'pause');
    await game.key('Escape');
    await expectScreen(game, 'none');
    assert.equal((await game.snapshot()).mode, 'race');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('phone: the touch pause button pauses, hides the pads, and Resume brings them back', async () => {
  const game = await openGame(browser, { device: 'phone', query: FAST });
  try {
    await startFromMenu(game, { touch: true });
    await waitRacing(game);
    assert.equal(await game.eval(isShown, '#touch'), true);
    await game.tap('#touch [data-tap="pause"]');
    await expectScreen(game, 'pause');
    assert.equal(await game.eval(isShown, '#touch'), false, 'no pads over the pause menu');
    await game.tap('#btn-resume');
    await expectScreen(game, 'none');
    assert.equal(await game.eval(isShown, '#touch'), true, 'pads back');
    assert.equal((await game.snapshot()).mode, 'race');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('leaving the page (visibilitychange to hidden) pauses the race', async () => {
  const game = await openGame(browser, { device: 'phone', query: FAST });
  try {
    await startFromMenu(game, { touch: true });
    await game.eval(() => {
      Object.defineProperty(document, 'hidden', { configurable: true, get: () => true });
      Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'hidden' });
      document.dispatchEvent(new Event('visibilitychange'));
    });
    await expectScreen(game, 'pause');
    assert.equal((await game.snapshot()).mode, 'paused');
    // Coming back doesn't resume by itself; the player taps Resume.
    await game.eval(() => {
      delete document.hidden; delete document.visibilityState;
      document.dispatchEvent(new Event('visibilitychange'));
    });
    await sleep(300);
    assert.equal(await game.screen(), 'pause');
    await game.tap('#btn-resume');
    await expectScreen(game, 'none');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('desktop: Restart from the pause menu starts a fresh race', async () => {
  const game = await openGame(browser, { query: FAST });
  try {
    await startFromMenu(game);
    await waitRacing(game);
    await markRace(game);
    await game.key('Escape');
    await expectScreen(game, 'pause');
    await game.click('#btn-restart');
    await game.waitFor(newRaceStarted, { timeout: 20000, what: 'a new race' });
    await expectScreen(game, 'none');
    const r = await game.eval(() => ({ state: __race.state, time: __race.time, s: __race.player.s, startS: __race.track.startS }));
    assert.equal(r.state, 'countdown');
    assert.equal(r.time, 0);
    assert.ok(r.s < r.startS, 'back on the grid');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('desktop: Restart from the pause menu brings the sound back', async () => {
  const game = await openGame(browser, { query: FAST });
  try {
    await startFromMenu(game);
    await game.key('Escape');
    await expectScreen(game, 'pause');
    assert.equal((await game.snapshot()).audio, 'suspended');
    await markRace(game);
    await game.click('#btn-restart');
    await game.waitFor(newRaceStarted, { timeout: 20000, what: 'a new race' });
    await game.waitFor(() => window.__audio.ctx.state === 'running', { timeout: 5000, what: 'the sound to come back after Restart' });
  } finally { await game.close(); }
});

test('phone: Main menu from pause returns to the menu with the pads hidden; Race works again', async () => {
  const game = await openGame(browser, { device: 'phone', query: FAST });
  try {
    await startFromMenu(game, { touch: true });
    await game.tap('#touch [data-tap="pause"]');
    await expectScreen(game, 'pause');
    await game.tap('#btn-quit');
    await expectScreen(game, 'menu');
    const s = await game.snapshot();
    assert.equal(s.mode, 'menu');
    assert.equal(await game.eval(isShown, '#touch'), false, 'no pads on the menu');
    assert.equal(await game.eval(isShown, '#hud'), false, 'no HUD on the menu');
    await game.waitFor(() => window.__audio.ctx.state === 'running', { timeout: 5000, what: 'the sound to unpause on the menu' });
    await markRace(game);
    await game.tap('#btn-start');
    await game.waitFor(newRaceStarted, { timeout: 20000, what: 'a new race from the menu' });
    assert.equal(await game.eval(isShown, '#touch'), true);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

// Put the player 250 m from the line (the autopilot drives it home) and
// the rivals back near the start, or give one of them a finish time.
function teleportToFinish(game, { rivalWon = false } = {}) {
  return game.eval((won) => {
    const r = window.__race, t = r.track;
    const s = t.finishS - 250;
    r.phys.reset(s, 0);
    const F = t.frame(s);
    r.player.vx = F.fx * 45; r.player.vz = F.fz * 45;
    r.lastS = s; r.odo = s;
    r.ais.forEach((a, i) => { a.s = t.startS + 40 - i * 12; a.lat = i % 2 ? 2 : -2; a.speed = 0; a.writePos(); });
    if (won) { r.ais[0].finished = true; r.ais[0].finishTime = 1; }
  }, rivalWon);
}

test('desktop: winning shows the results, saves the best time, and Race again restarts', async () => {
  const game = await openGame(browser, { query: FAST + '&autodrive=1' });
  try {
    await startFromMenu(game);
    await waitRacing(game);
    await teleportToFinish(game);
    await game.waitFor(() => window.__race.state === 'finished', { timeout: 20000, what: 'the player to cross the line' });
    await expectScreen(game, 'results', 15000);
    const res = await game.eval(() => ({
      title: document.getElementById('res-title').textContent,
      rows: document.querySelectorAll('#res-table tr').length,
      me: document.querySelector('#res-table tr.me')?.rowIndex,
      best: document.getElementById('res-best').textContent,
      time: __race.playerTime, mode: __game.mode,
    }));
    assert.equal(res.mode, 'results');
    assert.equal(res.title, 'You win!');
    assert.equal(res.rows, 6, 'you and five rivals');
    assert.equal(res.me, 0, 'you are first');
    const best = await stored(game, 'best.sierra');
    assert.ok(Math.abs(best - res.time) < 1e-6, `best time saved (${best} vs ${res.time})`);
    assert.equal(res.best, `Best winning time: ${fmtTime(best)}`);

    await markRace(game);
    await game.click('#btn-again');
    await game.waitFor(newRaceStarted, { timeout: 20000, what: 'Race again' });
    await expectScreen(game, 'none');

    // The menu card shows the best time.
    await game.key('Escape');
    await expectScreen(game, 'pause');
    await game.click('#btn-quit');
    await expectScreen(game, 'menu');
    assert.equal(await game.eval(() => document.getElementById('lvl-best').textContent), `Best winning time ${fmtTime(best)}`);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('desktop: finishing second saves no best time; Main menu from the results', async () => {
  const game = await openGame(browser, { query: FAST + '&autodrive=1' });
  try {
    await startFromMenu(game);
    await waitRacing(game);
    await teleportToFinish(game, { rivalWon: true });
    await expectScreen(game, 'results', 25000);
    assert.equal(await game.eval(() => document.getElementById('res-title').textContent), '2nd place');
    assert.equal(await game.eval(() => document.querySelector('#res-table tr.me')?.rowIndex), 1);
    assert.equal(await stored(game, 'best.sierra'), null);
    assert.equal(await game.eval(() => document.getElementById('res-best').textContent), 'Win the race to set a best time');
    await game.click('#btn-menu');
    await expectScreen(game, 'menu');
    assert.equal((await game.snapshot()).mode, 'menu');
    assert.equal(await game.eval(() => document.getElementById('lvl-best').textContent), '');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('cruise: End run in the pause menu shows the score and saves the best', async () => {
  const game = await openGame(browser, { query: FAST, storage: { 'mr.level': 'cruise' } });
  try {
    assert.equal(await game.eval(() => document.getElementById('btn-start').textContent), 'Cruise');
    await startFromMenu(game);
    assert.equal(await game.eval('__race.cruise'), true);
    await waitRacing(game);
    await game.eval(() => { __race.score = 12345.4; __race.nearMisses = 3; });
    await game.key('Escape');
    await expectScreen(game, 'pause');
    assert.equal(await game.eval(isShown, '#btn-end'), true, 'End run is offered in a cruise');
    await game.click('#btn-end');
    await expectScreen(game, 'results');
    const res = await game.eval(() => ({
      title: document.getElementById('res-title').textContent,
      rows: [...document.querySelectorAll('#res-table tr')].map((tr) => [...tr.cells].slice(1).map((c) => c.textContent)),
      best: document.getElementById('res-best').textContent,
      audio: __audio.ctx.state,
    }));
    assert.equal(res.title, 'New best!');
    assert.deepEqual(res.rows.map((r) => r[0]), ['Score', 'Distance', 'Top speed', 'Near misses', 'Time']);
    assert.deepEqual(res.rows[0], ['Score', '12,345']);
    assert.deepEqual(res.rows[3], ['Near misses', '3']);
    assert.equal(res.best, 'Best score: 12,345');
    assert.equal(await stored(game, 'bestScore.cruise'), 12345);
    await game.waitFor(() => window.__audio.ctx.state === 'running', { timeout: 5000, what: 'the sound to unpause on the results' });

    await game.click('#btn-menu');
    await expectScreen(game, 'menu');
    assert.equal(await game.eval(() => document.getElementById('lvl-best').textContent), 'Best score 12,345');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});
