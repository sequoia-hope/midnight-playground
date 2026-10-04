// test/e2e/race-button.test.js against the Rust build (roadmap WP 6.2): the
// Race button starts a race on a phone (a tap), on a desktop (a click) and
// from the keyboard (Tab to it, Enter); on a phone it goes fullscreen
// inside the tap unless the setting is off; a second tap while the race is
// starting starts one race. The JS's audio-unlock checks wait for the
// client's sound (M5).
//
//   cargo xtask web && node --test tools/parity/e2e/race-button.test.mjs

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

test('phone (landscape): tapping Race first thing starts the race', async () => {
  const game = await openGame(browser, { device: 'phone' });
  try {
    assert.equal(await game.screen(), 'menu');
    assert.ok((await game.snapshot()).touchUI, 'phone gets the touch UI');
    await game.tap('#btn-start');
    const s = await expectRaceStarts(game);
    assert.ok(s.fullscreen, 'Race goes fullscreen on a phone');
    await game.frames(3);
    assert.equal((await game.ui('#touch [data-tap="pause"]'))?.visible, true, 'touch controls show');
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
    await game.tap('#car-pick .pick:nth-child(2)');
    assert.equal((await game.ui('#car-pick .pick:nth-child(2)')).sel, true);
    await game.tap('#btn-start');
    await expectRaceStarts(game);
    assert.equal(await game.eval('__mr.race && __mr.race.state !== undefined'), true);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('phone: Race with fullscreen turned off stays windowed', async () => {
  const game = await openGame(browser, { device: 'phone', storage: { 'mr.fullscreen': false } });
  try {
    assert.equal((await game.ui('#opt-fullscreen')).value, false);
    await game.tap('#btn-start');
    const s = await expectRaceStarts(game);
    assert.equal(s.fullscreen, false);
  } finally { await game.close(); }
});

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

test('desktop: clicking Race starts the race, windowed', async () => {
  const game = await openGame(browser, { device: 'desktop' });
  try {
    assert.equal((await game.snapshot()).touchUI, false);
    await game.click('#btn-start');
    const s = await expectRaceStarts(game);
    assert.equal(s.fullscreen, false, 'desktop never forces fullscreen');
    assert.equal(await game.ui('#touch [data-tap="pause"]'), null, 'no touch controls on desktop');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('desktop: Race from the keyboard (Tab to it, Enter)', async () => {
  const game = await openGame(browser, { device: 'desktop' });
  try {
    for (let i = 0; i < 40 && (await game.eval('window.__mr.focus')) !== 'btn-start'; i++) await game.key('Tab');
    assert.equal(await game.eval('window.__mr.focus'), 'btn-start');
    await game.key('Enter');
    await expectRaceStarts(game);
  } finally { await game.close(); }
});
