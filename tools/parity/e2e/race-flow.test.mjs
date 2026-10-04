// test/e2e/race-flow.test.js against the Rust build (roadmap WP 6.2): a
// race from start to finish, countdown, pause (Esc, P, the touch pause
// button, leaving the page), resume, restart, back to the menu, the finish
// and results screen with best times, Race again, and ending a cruise.
//
// The DOM reads become `__mr.ui(id)`, `__race` is `__mr.race`, the staging
// (`teleportToFinish`, the cruise score) is `__mr.stage`; the AudioContext's
// state is `__mr.audio.context`.
//
//   cargo xtask web && node --test tools/parity/e2e/race-flow.test.mjs

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import {
  launch, openGame, stored, startFromMenu, markRace, newRaceStarted, waitRacing, expectScreen, isShown, sleep,
} from './harness.mjs';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

const FAST = 'timescale=2';

// fmtTime (src/game/HUD.js).
function fmtTime(t) {
  if (t == null || !isFinite(t)) return '--:--.--';
  const cs = Math.round(t * 100);
  const m = Math.floor(cs / 6000);
  const s = (cs - m * 6000) / 100;
  return `${m}:${s.toFixed(2).padStart(5, '0')}`;
}

test('desktop: the countdown turns into racing, the clock runs and the car is released', async () => {
  const game = await openGame(browser, { query: FAST });
  try {
    await startFromMenu(game);
    const c0 = await game.eval(() => ({ state: __mr.race.state, locked: __mr.race.locked, time: __mr.race.time }));
    assert.equal(c0.state, 'countdown');
    assert.equal(c0.locked, true, 'the car is held during the countdown');
    assert.equal(c0.time, 0, 'the clock waits for GO');
    await waitRacing(game, 15000);
    assert.equal(await game.eval('__mr.race.locked'), false);
    const t1 = await game.eval('__mr.race.time');
    await sleep(400);
    assert.ok(await game.eval('__mr.race.time') > t1, 'the race clock runs');
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
    const t0 = await game.eval('__mr.race.time');
    await sleep(500);
    assert.equal(await game.eval('__mr.race.time'), t0, 'the clock is frozen');
    assert.equal(await isShown(game, '#btn-end'), false, 'no End run outside a cruise');

    await game.click('#btn-resume');
    await expectScreen(game, 'none');
    await game.waitFor(() => window.__mr.audio?.context === 'running', { timeout: 5000, what: 'the sound to resume' });
    assert.equal((await game.snapshot()).mode, 'race');
    await game.waitFor(`__mr.race.time > ${t0}`, { timeout: 5000, what: 'the clock to run again' });
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
    assert.equal(await isShown(game, '#touch [data-tap="pause"]'), true);
    await game.tap('#touch [data-tap="pause"]');
    await expectScreen(game, 'pause');
    assert.equal(await isShown(game, '#touch [data-tap="pause"]'), false, 'no pads over the pause menu');
    await game.tap('#btn-resume');
    await expectScreen(game, 'none');
    await game.frames(3);
    assert.equal(await isShown(game, '#touch [data-tap="pause"]'), true, 'pads back');
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
    const r = await game.eval(() => ({ state: __mr.race.state, time: __mr.race.time, s: __mr.race.s }));
    assert.equal(r.state, 'countdown');
    assert.equal(r.time, 0);
    assert.ok(r.s < 200, 'back on the grid');
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
    await game.waitFor(() => window.__mr.audio?.context === 'running', { timeout: 5000, what: 'the sound to come back after Restart' });
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
    assert.equal((await game.snapshot()).mode, 'menu');
    assert.equal(await isShown(game, '#touch [data-tap="pause"]'), false, 'no pads on the menu');
    assert.equal(await game.eval('window.__mr.race === undefined'), true, 'no race (and no HUD) on the menu');
    await game.waitFor(() => window.__mr.audio?.context === 'running', { timeout: 5000, what: 'the sound to unpause on the menu' });
    await markRace(game);
    await game.tap('#btn-start');
    await game.waitFor(newRaceStarted, { timeout: 30000, what: 'a new race from the menu' });
    await game.frames(3);
    assert.equal(await isShown(game, '#touch [data-tap="pause"]'), true);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

const teleportToFinish = (game, { rivalWon = false } = {}) => game.eval((w) => window.__mr.stage({ cmd: 'finish', rivalWon: w }), rivalWon);

test('desktop: winning shows the results, saves the best time, and Race again restarts', async () => {
  const game = await openGame(browser, { query: FAST + '&autodrive=1' });
  try {
    await startFromMenu(game);
    await waitRacing(game);
    await teleportToFinish(game);
    await game.waitFor(() => window.__mr.race.finished, { timeout: 30000, what: 'the player to cross the line' });
    await expectScreen(game, 'results', 15000);
    const res = await game.eval(() => {
      const ui = window.__mr.ui;
      const rows = Object.keys(window.__mr.uiNodes).filter((k) => k.startsWith('res-row-')).length;
      return {
        title: ui('res-title').value, rows, me: [0, 1, 2, 3, 4, 5].findIndex((i) => ui('res-row-' + i)?.sel),
        best: ui('res-best').value, time: __mr.race.results.find((r) => r.player).time, mode: __mr.mode,
      };
    });
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

    await game.key('Escape');
    await expectScreen(game, 'pause');
    await game.click('#btn-quit');
    await expectScreen(game, 'menu');
    assert.equal(await game.eval(() => window.__mr.ui('lvl-best').value), `Best winning time ${fmtTime(best)}`);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('desktop: finishing second saves no best time; Main menu from the results', async () => {
  const game = await openGame(browser, { query: FAST + '&autodrive=1' });
  try {
    await startFromMenu(game);
    await waitRacing(game);
    await teleportToFinish(game, { rivalWon: true });
    await expectScreen(game, 'results', 30000);
    assert.equal(await game.eval(() => window.__mr.ui('res-title').value), '2nd place');
    assert.equal(await game.eval(() => [0, 1, 2, 3, 4, 5].findIndex((i) => window.__mr.ui('res-row-' + i)?.sel)), 1);
    assert.equal(await stored(game, 'best.sierra'), null);
    assert.equal(await game.eval(() => window.__mr.ui('res-best').value), 'Win the race to set a best time');
    await game.click('#btn-menu');
    await expectScreen(game, 'menu');
    assert.equal((await game.snapshot()).mode, 'menu');
    assert.equal(await game.eval(() => window.__mr.ui('lvl-best').value), '');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('cruise: End run in the pause menu shows the score and saves the best', async () => {
  const game = await openGame(browser, { query: FAST, storage: { 'mr.level': 'cruise' } });
  try {
    assert.equal(await game.eval(() => window.__mr.ui('btn-start').value), 'Cruise');
    await startFromMenu(game);
    await waitRacing(game);
    await game.eval(() => window.__mr.stage({ cmd: 'cruise', score: 12345.4, nearMisses: 3 }));
    await game.frames(3);
    await game.key('Escape');
    await expectScreen(game, 'pause');
    assert.equal(await isShown(game, '#btn-end'), true, 'End run is offered in a cruise');
    await game.click('#btn-end');
    await expectScreen(game, 'results');
    const res = await game.eval(() => {
      const ui = window.__mr.ui;
      return {
        title: ui('res-title').value,
        rows: [0, 1, 2, 3, 4].map((i) => ui('res-row-' + i).value.split('|').slice(1)),
        best: ui('res-best').value,
      };
    });
    assert.equal(res.title, 'New best!');
    assert.deepEqual(res.rows.map((r) => r[0]), ['Score', 'Distance', 'Top speed', 'Near misses', 'Time']);
    assert.deepEqual(res.rows[0], ['Score', '12,345']);
    assert.deepEqual(res.rows[3], ['Near misses', '3']);
    assert.equal(res.best, 'Best score: 12,345');
    assert.equal(await stored(game, 'bestScore.cruise'), 12345);
    await game.waitFor(() => window.__mr.audio?.context === 'running', { timeout: 5000, what: 'the sound to unpause on the results' });

    await game.click('#btn-menu');
    await expectScreen(game, 'menu');
    assert.equal(await game.eval(() => window.__mr.ui('lvl-best').value), 'Best score 12,345');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});
