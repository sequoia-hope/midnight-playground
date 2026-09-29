// The main menu: level tabs and car picks, the level card, options, what
// shows on touch screens versus desktops, and settings that persist across a
// reload. On phones every control must be reachable and tappable.

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame } from './harness.js';
import { LEVELS } from '../../src/levels/index.js';
import { CAR_SPECS } from '../../src/vehicles/CarPhysics.js';
import { TRACKS } from '../../src/game/audio/tracks.js';
import { isShown, stored, expectScreen } from './flow-helpers.js';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

const CARS = Object.keys(CAR_SPECS);

// Pick a level from its tab and wait for its world to finish building.
async function pickLevel(game, id, { touch = false } = {}) {
  const i = LEVELS.findIndex((l) => l.id === id);
  const sel = `#level-pick .lvl-tab:nth-child(${i + 1})`;
  await (touch ? game.tap(sel) : game.click(sel));
  await game.waitFor(`window.__world?.level.id === ${JSON.stringify(id)} && window.__game.mode === 'menu'`, { timeout: 30000, what: `level ${id} to build` });
  await expectScreen(game, 'menu');
}

test('desktop: one tab per level and one pick per car, defaults selected', async () => {
  const game = await openGame(browser);
  try {
    const menu = await game.eval(() => ({
      tabs: [...document.querySelectorAll('#level-pick .lvl-tab')].map((b) => ({ text: b.textContent, sel: b.classList.contains('sel') })),
      picks: [...document.querySelectorAll('#car-pick .pick')].map((b) => ({ text: b.textContent, sel: b.classList.contains('sel') })),
      world: window.__world.level.id,
      start: document.getElementById('btn-start').textContent,
      name: document.getElementById('lvl-name').textContent,
      num: document.getElementById('lvl-num').textContent,
      len: document.getElementById('lvl-len').textContent,
      best: document.getElementById('lvl-best').textContent,
    }));
    assert.equal(menu.tabs.length, LEVELS.length);
    LEVELS.forEach((l, i) => assert.ok(menu.tabs[i].text.includes(l.title), `tab ${i} is ${l.title}`));
    assert.deepEqual(menu.tabs.map((t) => t.sel), LEVELS.map((l) => l.id === 'sierra'), 'Level 1 selected by default');
    assert.equal(menu.picks.length, CARS.length);
    CARS.forEach((k, i) => assert.ok(menu.picks[i].text.includes(CAR_SPECS[k].label), `pick ${i} is ${CAR_SPECS[k].label}`));
    assert.deepEqual(menu.picks.map((p) => p.sel), CARS.map((k) => k === 'sports'), 'Vento GT selected by default');
    assert.equal(menu.world, 'sierra');
    assert.equal(menu.start, 'Race');
    assert.equal(menu.name, LEVELS[0].title);
    assert.equal(menu.num, LEVELS[0].num);
    assert.match(menu.len, /\d+\.\d km · \d+\.\d mi · 5 rivals · traffic/);
    assert.equal(menu.best, '', 'no best time yet');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('desktop: picking a car selects it and persists across a reload', async () => {
  const game = await openGame(browser);
  try {
    await game.click('#car-pick .pick:nth-child(3)');
    const sel = () => [...document.querySelectorAll('#car-pick .pick')].map((b) => b.classList.contains('sel'));
    assert.deepEqual(await game.eval(sel), CARS.map((_, i) => i === 2));
    assert.equal(await stored(game, 'car'), CARS[2]);
    await game.reload();
    assert.deepEqual(await game.eval(sel), CARS.map((_, i) => i === 2), 'still selected after reload');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('desktop: picking a level builds it, updates the card and the button, and persists', async () => {
  const game = await openGame(browser);
  try {
    const card = () => ({
      num: document.getElementById('lvl-num').textContent,
      name: document.getElementById('lvl-name').textContent,
      desc: document.getElementById('lvl-desc').textContent,
      start: document.getElementById('btn-start').textContent,
      sel: [...document.querySelectorAll('#level-pick .lvl-tab')].findIndex((b) => b.classList.contains('sel')),
    });
    for (const id of ['streets', 'cruise']) {
      const l = LEVELS.find((x) => x.id === id);
      await pickLevel(game, id);
      const c = await game.eval(card);
      assert.equal(c.name, l.title);
      assert.equal(c.num, l.num);
      assert.equal(c.desc, l.desc);
      assert.equal(c.start, l.mode === 'cruise' ? 'Cruise' : 'Race');
      assert.equal(c.sel, LEVELS.indexOf(l));
      assert.equal(await stored(game, 'level'), id);
    }
    assert.match(await game.eval(() => document.getElementById('lvl-len').textContent), /Endless loop/);
    await game.reload();
    assert.equal(await game.eval('window.__world.level.id'), 'cruise', 'the saved level loads on start-up');
    assert.equal((await game.eval(card)).start, 'Cruise');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('desktop: MPH and High quality checkboxes persist; the track picker is populated and persists', async () => {
  const game = await openGame(browser);
  try {
    const opts = () => ({
      mph: document.getElementById('opt-mph').checked,
      hq: document.getElementById('opt-hq').checked,
      track: document.getElementById('opt-track').value,
      tracks: [...document.getElementById('opt-track').options].map((o) => o.value),
    });
    const o = await game.eval(opts);
    assert.equal(o.mph, true, 'MPH on by default');
    assert.equal(o.hq, true, 'High quality on by default on desktop');
    assert.equal(o.track, 'auto');
    assert.deepEqual(o.tracks, ['auto', ...TRACKS.map((t) => t.id)], "the level's own track, then every song");

    await game.click('#opt-mph');
    await game.click('#opt-hq');
    const pick = TRACKS[2].id;
    await game.eval((id) => {
      const s = document.getElementById('opt-track');
      s.value = id;
      s.dispatchEvent(new Event('change', { bubbles: true }));
    }, pick);
    assert.equal(await stored(game, 'mph'), false);
    assert.equal(await stored(game, 'hq'), false);
    assert.equal(await stored(game, 'track'), pick);

    await game.reload();
    const after = await game.eval(opts);
    assert.equal(after.mph, false);
    assert.equal(after.hq, false);
    assert.equal(after.track, pick);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('desktop: keyboard help shows; touch-only options and the rotate hint do not', async () => {
  const game = await openGame(browser);
  try {
    assert.equal(await game.eval(isShown, '#menu .controls'), true, 'keyboard help');
    assert.equal(await game.eval(isShown, '#menu .touch-help'), false);
    assert.equal(await game.eval(() => document.getElementById('opt-autogas').closest('label').getClientRects().length), 0, 'no Auto gas');
    assert.equal(await game.eval(() => document.getElementById('opt-fullscreen').closest('label').getClientRects().length), 0, 'no Fullscreen');
    assert.equal(await game.eval(isShown, '#rotate-hint'), false);
  } finally { await game.close(); }
});

test('phone: touch-only options and touch help show, keyboard help does not; High quality off by default', async () => {
  const game = await openGame(browser, { device: 'phone' });
  try {
    assert.equal(await game.eval(isShown, '#menu .controls'), false, 'no keyboard help');
    assert.equal(await game.eval(isShown, '#menu .touch-help'), true);
    assert.ok(await game.eval(() => document.getElementById('opt-autogas').closest('label').getClientRects().length > 0), 'Auto gas');
    assert.ok(await game.eval(() => document.getElementById('opt-fullscreen').closest('label').getClientRects().length > 0), 'Fullscreen');
    assert.equal(await game.eval(() => document.getElementById('opt-hq').checked), false);
    assert.equal(await game.eval(() => document.getElementById('opt-fullscreen').checked), true);
    assert.equal(await game.eval(() => document.getElementById('opt-autogas').checked), false);

    // Tapping the checkboxes works and persists.
    await game.tap('#opt-autogas');
    await game.tap('#opt-fullscreen');
    assert.equal(await stored(game, 'autogas'), true);
    assert.equal(await stored(game, 'fullscreen'), false);
    await game.reload();
    assert.equal(await game.eval(() => document.getElementById('opt-autogas').checked), true);
    assert.equal(await game.eval(() => document.getElementById('opt-fullscreen').checked), false);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('rotate hint: on a portrait phone menu only, gone once the race starts', async () => {
  const land = await openGame(browser, { device: 'phone' });
  try {
    assert.equal(await land.eval(isShown, '#rotate-hint'), false, 'not in landscape');
  } finally { await land.close(); }
  const game = await openGame(browser, { device: 'phonePortrait' });
  try {
    assert.equal(await game.eval(isShown, '#rotate-hint'), true, 'portrait menu shows it');
    await game.tap('#btn-start');
    await game.waitFor(() => window.__game.mode === 'race', { timeout: 20000, what: 'the race' });
    assert.equal(await game.eval(isShown, '#rotate-hint'), false, 'hidden while racing');
  } finally { await game.close(); }
});

// Every control on the menu, in the order a player meets them.
const MENU_CONTROLS = [
  ...LEVELS.map((_, i) => `#level-pick .lvl-tab:nth-child(${i + 1})`),
  ...CARS.map((_, i) => `#car-pick .pick:nth-child(${i + 1})`),
  '#btn-start',
  '#menu .vol-music', '#menu .vol-sfx', '#opt-track',
  '#opt-mph', '#opt-hq', '#opt-autogas', '#opt-fullscreen',
];

for (const device of ['phone', 'phonePortrait']) {
  test(`${device}: every menu control can be scrolled to and is not covered`, async () => {
    const game = await openGame(browser, { device });
    try {
      const problems = [];
      for (const sel of MENU_CONTROLS) {
        try {
          const { x, y } = await game.center(sel);
          const vp = await game.eval(() => ({ w: innerWidth, h: innerHeight }));
          if (x < 0 || y < 0 || x > vp.w || y > vp.h) problems.push(`${sel} centre (${x}, ${y}) is off screen`);
        } catch (e) { problems.push(e.message); }
      }
      assert.deepEqual(problems, []);

      // And a tap on one really lands: pick the last car.
      await game.tap(`#car-pick .pick:nth-child(${CARS.length})`);
      assert.equal(await stored(game, 'car'), CARS[CARS.length - 1]);
      // A level tab switches the level by touch too.
      const i = LEVELS.findIndex((l) => l.id === 'desert');
      await game.tap(`#level-pick .lvl-tab:nth-child(${i + 1})`);
      await game.waitFor(() => window.__world?.level.id === 'desert' && window.__game.mode === 'menu', { timeout: 30000, what: 'desert to build' });
      assert.equal(await game.screen(), 'menu');
      assert.deepEqual(game.errors, []);
    } finally { await game.close(); }
  });
}
