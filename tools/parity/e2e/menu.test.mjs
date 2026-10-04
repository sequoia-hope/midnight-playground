// test/e2e/menu.test.js against the Rust build (roadmap WP 6.2): level tabs
// and car picks, the level card, options, what shows on touch screens
// versus desktops, settings that persist across a reload, and every menu
// control reachable and tappable on phones. The DOM reads become
// `__mr.ui(id)`; `__world.level.id` is `__mr.level`.
//
//   cargo xtask web && node --test tools/parity/e2e/menu.test.mjs

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame, stored, expectScreen, isShown, LEVEL_IDS, CAR_IDS } from './harness.mjs';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

const LEVELS = [
  { id: 'sierra', num: 'LEVEL 1', title: 'Sierra to the City' },
  { id: 'coast', num: 'LEVEL 2', title: 'Coast Highway' },
  { id: 'streets', num: 'LEVEL 3', title: 'Downtown Streets' },
  { id: 'desert', num: 'LEVEL 4', title: 'Desert Run' },
  { id: 'seaside', num: 'LEVEL 5', title: 'Seaside Raceway' },
  { id: 'cruise', num: 'ENDLESS', title: 'Night City Cruise', cruise: true },
];
const LABELS = ['Vento GT', 'Brawler 69', 'Stiletto R', 'Kestrel RS', 'Ion Arc'];
const TRACKS = ['midnight-run', 'seabright', 'neon-rush', 'mirage', 'interstate', 'chrome-heart', 'afterburner'];

async function pickLevel(game, id, { touch = false } = {}) {
  const i = LEVEL_IDS.indexOf(id);
  const sel = `#level-pick .lvl-tab:nth-child(${i + 1})`;
  await (touch ? game.tap(sel) : game.click(sel));
  await game.waitFor(`window.__mr.level === ${JSON.stringify(id)} && window.__mr.mode === 'menu' && window.__mr.ready`, { timeout: 120000, what: `level ${id} to build` });
  await expectScreen(game, 'menu');
}

test('desktop: one tab per level and one pick per car, defaults selected', async () => {
  const game = await openGame(browser);
  try {
    const menu = await game.eval(() => {
      const ui = window.__mr.ui;
      return {
        tabs: ['sierra', 'coast', 'streets', 'desert', 'seaside', 'cruise'].map((l) => ui('lvl-tab-' + l)),
        picks: ['sports', 'muscle', 'super', 'rally', 'electric'].map((k) => ui('pick-' + k)),
        world: window.__mr.level,
        start: ui('btn-start').value,
        name: ui('lvl-name').value, num: ui('lvl-num').value, len: ui('lvl-len').value, best: ui('lvl-best').value,
      };
    });
    assert.equal(menu.tabs.filter(Boolean).length, LEVELS.length);
    LEVELS.forEach((l, i) => assert.ok(menu.tabs[i].value.includes(l.title), `tab ${i} is ${l.title}`));
    assert.deepEqual(menu.tabs.map((t) => t.sel), LEVELS.map((l) => l.id === 'sierra'), 'Level 1 selected by default');
    assert.equal(menu.picks.filter(Boolean).length, CAR_IDS.length);
    CAR_IDS.forEach((k, i) => assert.ok(menu.picks[i].value.includes(LABELS[i]), `pick ${i} is ${LABELS[i]}`));
    assert.deepEqual(menu.picks.map((p) => p.sel), CAR_IDS.map((k) => k === 'sports'), 'Vento GT selected by default');
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
    const sel = () => ['sports', 'muscle', 'super', 'rally', 'electric'].map((k) => window.__mr.ui('pick-' + k).sel);
    await game.frames(3);
    assert.deepEqual(await game.eval(sel), CAR_IDS.map((_, i) => i === 2));
    assert.equal(await stored(game, 'car'), CAR_IDS[2]);
    await game.reload();
    assert.deepEqual(await game.eval(sel), CAR_IDS.map((_, i) => i === 2), 'still selected after reload');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('desktop: picking a level builds it, updates the card and the button, and persists', async () => {
  const game = await openGame(browser);
  try {
    const card = () => {
      const ui = window.__mr.ui;
      return {
        num: ui('lvl-num').value, name: ui('lvl-name').value, desc: ui('lvl-desc').value, start: ui('btn-start').value,
        sel: ['sierra', 'coast', 'streets', 'desert', 'seaside', 'cruise'].findIndex((l) => ui('lvl-tab-' + l).sel),
      };
    };
    for (const id of ['streets', 'cruise']) {
      const l = LEVELS.find((x) => x.id === id);
      await pickLevel(game, id);
      const c = await game.eval(card);
      assert.equal(c.name, l.title);
      assert.equal(c.num, l.num);
      assert.ok(c.desc.length > 20, 'a description');
      assert.equal(c.start, l.cruise ? 'Cruise' : 'Race');
      assert.equal(c.sel, LEVELS.indexOf(l));
      assert.equal(await stored(game, 'level'), id);
    }
    assert.match(await game.eval(() => window.__mr.ui('lvl-len').value), /Endless loop/);
    await game.reload();
    assert.equal(await game.eval('window.__mr.level'), 'cruise', 'the saved level loads on start-up');
    assert.equal((await game.eval(card)).start, 'Cruise');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('desktop: MPH and High quality checkboxes persist; the track picker is populated and persists', async () => {
  const game = await openGame(browser);
  try {
    const opts = () => ({
      mph: window.__mr.ui('opt-mph').value, hq: window.__mr.ui('opt-hq').value, track: window.__mr.ui('opt-track').value,
    });
    const o = await game.eval(opts);
    assert.equal(o.mph, true, 'MPH on by default');
    assert.equal(o.hq, true, 'High quality on by default on desktop');
    assert.equal(o.track, 'auto');

    await game.click('#opt-mph');
    await game.click('#opt-hq');
    // The picker: open it, every song is offered, pick the third.
    await game.click('#opt-track');
    const offered = await game.eval(() => Object.entries(window.__mr.uiNodes).filter(([k]) => k.startsWith('option-'))
      .sort((a, b) => a[1].y - b[1].y).map(([k]) => k.slice(7)));
    assert.deepEqual(offered, ['auto', ...TRACKS], "the level's own track, then every song");
    await game.click(`#option-${TRACKS[2]}`);
    assert.equal(await stored(game, 'mph'), false);
    assert.equal(await stored(game, 'hq'), false);
    assert.equal(await stored(game, 'track'), TRACKS[2]);

    await game.reload();
    const a = await game.eval(opts);
    assert.equal(a.mph, false);
    assert.equal(a.hq, false);
    assert.equal(a.track, TRACKS[2]);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('desktop: keyboard help shows; touch-only options and the rotate hint do not', async () => {
  const game = await openGame(browser);
  try {
    assert.equal(await isShown(game, '#menu-controls'), true, 'keyboard help');
    assert.equal(await isShown(game, '#touch-help'), false);
    assert.equal(await game.ui('#opt-autogas'), null, 'no Auto gas');
    assert.equal(await game.ui('#opt-fullscreen'), null, 'no Fullscreen');
    assert.equal(await isShown(game, '#rotate-hint'), false);
  } finally { await game.close(); }
});

test('phone: touch-only options and touch help show, keyboard help does not; High quality off by default', async () => {
  const game = await openGame(browser, { device: 'phone' });
  try {
    assert.equal(await isShown(game, '#menu-controls'), false, 'no keyboard help');
    assert.equal(await isShown(game, '#touch-help'), true);
    assert.ok(await isShown(game, '#opt-autogas'), 'Auto gas');
    assert.ok(await isShown(game, '#opt-fullscreen'), 'Fullscreen');
    assert.equal((await game.ui('#opt-hq')).value, false);
    assert.equal((await game.ui('#opt-fullscreen')).value, true);
    assert.equal((await game.ui('#opt-autogas')).value, false);

    await game.tap('#opt-autogas');
    await game.tap('#opt-fullscreen');
    assert.equal(await stored(game, 'autogas'), true);
    assert.equal(await stored(game, 'fullscreen'), false);
    await game.reload();
    assert.equal((await game.ui('#opt-autogas')).value, true);
    assert.equal((await game.ui('#opt-fullscreen')).value, false);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('rotate hint: on a portrait phone menu only, gone once the race starts', async () => {
  const land = await openGame(browser, { device: 'phone' });
  try {
    assert.equal(await isShown(land, '#rotate-hint'), false, 'not in landscape');
  } finally { await land.close(); }
  const game = await openGame(browser, { device: 'phonePortrait' });
  try {
    assert.equal(await isShown(game, '#rotate-hint'), true, 'portrait menu shows it');
    await game.tap('#btn-start');
    await game.waitFor(() => window.__mr.mode === 'race', { timeout: 30000, what: 'the race' });
    assert.equal(await isShown(game, '#rotate-hint'), false, 'hidden while racing');
  } finally { await game.close(); }
});

const MENU_CONTROLS = [
  ...LEVELS.map((_, i) => `#level-pick .lvl-tab:nth-child(${i + 1})`),
  ...CAR_IDS.map((_, i) => `#car-pick .pick:nth-child(${i + 1})`),
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

      await game.tap(`#car-pick .pick:nth-child(${CAR_IDS.length})`);
      assert.equal(await stored(game, 'car'), CAR_IDS[CAR_IDS.length - 1]);
      const i = LEVEL_IDS.indexOf('desert');
      await game.tap(`#level-pick .lvl-tab:nth-child(${i + 1})`);
      await game.waitFor(() => window.__mr.level === 'desert' && window.__mr.mode === 'menu' && window.__mr.ready, { timeout: 120000, what: 'desert to build' });
      assert.equal(await game.screen(), 'menu');
      assert.deepEqual(game.errors, []);
    } finally { await game.close(); }
  });
}
