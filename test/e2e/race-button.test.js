// The Race button starts a race: on a phone (a tap), on a desktop (a click)
// and from the keyboard.
//
// Regression: on phones the very first touch is a pointerdown, which is not
// a user gesture, and the game created its AudioContext there. That context
// stays suspended and its resume() promise never settles; startRace awaited
// it, so tapping Race did nothing at all. Desktop was fine because a mouse
// pointerdown *is* a gesture.

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame } from './harness.js';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

const raceStarted = () => window.__race && ['countdown', 'racing'].includes(window.__race.state)
  && document.getElementById('menu').classList.contains('hidden');

async function expectRaceStarts(game) {
  await game.waitFor(raceStarted, { timeout: 15000, what: 'the race to start' });
  const s = await game.snapshot();
  assert.equal(s.screen, 'none', 'no menu or other screen over the race');
  assert.equal(s.mode, 'race');
  return s;
}

test('phone (landscape): tapping Race first thing starts the race', async () => {
  const game = await openGame(browser, { device: 'phone' });
  try {
    assert.equal(await game.screen(), 'menu');
    assert.ok((await game.snapshot()).touchUI, 'phone gets the touch UI');
    await game.tap('#btn-start');
    const s = await expectRaceStarts(game);
    assert.equal(s.audio, 'running', 'the tap unlocks audio');
    assert.ok(s.fullscreen, 'Race goes fullscreen on a phone');
    assert.equal(await game.eval(() => !document.getElementById('touch').classList.contains('hidden')), true, 'touch controls show');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('phone (portrait): scroll to Race and tap it', async () => {
  const game = await openGame(browser, { device: 'phonePortrait' });
  try {
    await game.tap('#btn-start');
    await expectRaceStarts(game);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('phone: Race still starts after touching other menu controls first', async () => {
  const game = await openGame(browser, { device: 'phone' });
  try {
    // The first touch lands on a car, not on Race.
    await game.tap('#car-pick .pick:nth-child(2)');
    assert.equal(await game.eval(() => document.querySelector('#car-pick .pick:nth-child(2)').classList.contains('sel')), true);
    await game.tap('#btn-start');
    const s = await expectRaceStarts(game);
    assert.equal(s.audio, 'running');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('phone: Race with fullscreen turned off stays windowed', async () => {
  const game = await openGame(browser, { device: 'phone', storage: { 'mr.fullscreen': false } });
  try {
    assert.equal(await game.eval(() => document.getElementById('opt-fullscreen').checked), false);
    await game.tap('#btn-start');
    const s = await expectRaceStarts(game);
    assert.equal(s.fullscreen, false);
  } finally { await game.close(); }
});

test('phone: a second tap while the race is starting starts exactly one race', async () => {
  const game = await openGame(browser, { device: 'phone' });
  try {
    await game.eval(() => {
      window.__races = 0;
      let r;
      Object.defineProperty(window, '__race', { configurable: true, get: () => r, set: (v) => { r = v; window.__races++; } });
      // Hold the start for a second (as a slow phone compiling shaders
      // would) so the second tap lands while the first is still starting.
      const rd = window.__world.renderer, compile = rd.compileAsync.bind(rd);
      rd.compileAsync = (...a) => new Promise((res) => setTimeout(() => res(compile(...a)), 1000));
    });
    const { x, y } = await game.center('#btn-start');
    await game.page.touchscreen.tap(x, y);
    await new Promise((r) => setTimeout(r, 200));
    assert.equal(await game.screen(), 'menu', 'still starting when the second tap lands');
    await game.page.touchscreen.tap(x, y);
    await expectRaceStarts(game);
    await new Promise((r) => setTimeout(r, 1500));
    assert.equal(await game.eval('window.__races'), 1);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('desktop: clicking Race starts the race, windowed', async () => {
  const game = await openGame(browser, { device: 'desktop' });
  try {
    assert.equal((await game.snapshot()).touchUI, false);
    await game.click('#btn-start');
    const s = await expectRaceStarts(game);
    assert.equal(s.audio, 'running');
    assert.equal(s.fullscreen, false, 'desktop never forces fullscreen');
    assert.equal(await game.eval(() => document.getElementById('touch').classList.contains('hidden')), true, 'no touch controls on desktop');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('desktop: Race from the keyboard (Tab to it, Enter)', async () => {
  const game = await openGame(browser, { device: 'desktop' });
  try {
    await game.eval(() => document.getElementById('btn-start').focus());
    await game.key('Enter');
    const s = await expectRaceStarts(game);
    assert.equal(s.audio, 'running');
  } finally { await game.close(); }
});
