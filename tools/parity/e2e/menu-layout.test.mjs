// No part of the menu covers another (D1204), at desktop and phone sizes,
// held either way, scrolled to the top, the middle and the foot: the
// controls the bridge reports (`__mp.uiNodes`, each with the part of it its
// screen shows and the control it sits inside) never overlap one another
// unless one is part of the other, none shows under the page's build line
// (`#build`), and none runs off the side of the window. The Settings
// screen is held to the same.
//
//   cargo xtask web --release && node --test tools/parity/e2e/menu-layout.test.mjs

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame, sleep } from './harness.mjs';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

// CSS px. The desktop ones include a short window (the compact layout with
// a mouse); the phones are the common ones of both platforms, both ways up.
const DESKTOP = [[1280, 800], [1920, 1080], [1024, 640], [800, 600], [1366, 520], [1000, 480]];
const PHONE = [
  [844, 390], [667, 375], [932, 430], [915, 412], [740, 360],
  [390, 844], [375, 667], [412, 915], [360, 740],
];

// Everything wrong with the screen as it is laid out now.
const problems = () => {
  const nodes = Object.entries(window.__mp.uiNodes || {})
    .filter(([k, o]) => !k.startsWith('touch-') && o.visible && o.shown && o.shown.w > 0.5 && o.shown.h > 0.5);
  const byId = Object.fromEntries(nodes);
  const all = window.__mp.uiNodes || {};
  const within = (a, b) => { // a is part of b
    for (let p = all[a]?.parent; p; p = all[p]?.parent) if (p === b) return true;
    return false;
  };
  const cut = (a, b) => {
    const w = Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x);
    const h = Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y);
    return w > 1 && h > 1 ? [Math.round(w), Math.round(h)] : null;
  };
  const out = [];
  const ids = Object.keys(byId);
  for (let i = 0; i < ids.length; i++) {
    for (let j = i + 1; j < ids.length; j++) {
      const [a, b] = [ids[i], ids[j]];
      if (within(a, b) || within(b, a)) continue;
      const c = cut(byId[a].shown, byId[b].shown);
      if (c) out.push(`${a} and ${b} overlap by ${c[0]}×${c[1]}`);
    }
  }
  const el = document.getElementById('build');
  if (window.__mp.screen === 'menu' && !el.hidden) {
    const r = el.getBoundingClientRect();
    const line = { x: r.left, y: r.top, w: r.width, h: r.height };
    for (const id of ids) {
      const c = cut(byId[id].shown, line);
      if (c) out.push(`${id} shows under the build line by ${c[0]}×${c[1]}`);
    }
  }
  for (const id of ids) {
    const o = byId[id];
    if (o.x < -0.5 || o.x + o.w > window.innerWidth + 0.5) out.push(`${id} runs off the side (${Math.round(o.x)} to ${Math.round(o.x + o.w)} of ${window.innerWidth})`);
  }
  return out;
};

// The checks at each scroll position: the top, a control in the middle,
// the lowest control (`reveal` scrolls it to the middle, as far as it goes).
async function check(game, where) {
  const found = [];
  const ids = await game.eval(() => {
    const n = Object.entries(window.__mp.uiNodes || {}).filter(([k, o]) => !k.startsWith('touch-') && o.visible && o.h > 0);
    n.sort((a, b) => (a[1].y + a[1].h) - (b[1].y + b[1].h));
    return n.map(([k]) => k);
  });
  for (const id of [ids[0], ids[Math.floor(ids.length / 2)], ids[ids.length - 1]]) {
    await game.eval((i) => window.__mp.reveal(i), id);
    await game.frames(4);
    for (const p of await game.eval(problems)) found.push(`${where}, scrolled to ${id}: ${p}`);
  }
  return found;
}

// Resize, and wait for the page to measure its build line and the menu to
// lay out again around it. The client must still be drawing: a resize on a
// phone's screen once quit it (D1204), which no console error showed.
async function resize(game, w, h, touch) {
  const before = await game.eval(() => window.__mp.frames);
  await game.page.setViewport({ width: w, height: h, deviceScaleFactor: touch ? 3 : 1, isMobile: touch, hasTouch: touch, isLandscape: w > h });
  await sleep(500);
  await game.frames(8);
  assert.ok(await game.eval(() => window.__mp.frames) > before, `the client still draws at ${w}×${h}`);
}

// Press a control through the bridge and wait for the screen it opens; a
// press that lands while the screen is being rebuilt (the build line's
// height arriving after a resize) finds no control, so it is sent again.
async function go(game, id, screen) {
  for (let i = 0; i < 20 && (await game.screen()) !== screen; i++) {
    await game.eval((v) => window.__mp.stage({ cmd: 'act', id: v }), id);
    await game.frames(4);
  }
  assert.equal(await game.screen(), screen, `${id} opens ${screen}`);
}

async function sweep(device, sizes, touch) {
  const game = await openGame(browser, { device });
  const found = [];
  try {
    for (const [w, h] of sizes) {
      await resize(game, w, h, touch);
      assert.equal(await game.screen(), 'menu');
      assert.equal(await game.eval(() => document.getElementById('build').hidden), false, 'the build line shows on the menu');
      found.push(...await check(game, `${device} ${w}×${h} menu`));
      await go(game, 'btn-settings', 'settings');
      found.push(...await check(game, `${device} ${w}×${h} settings`));
      await go(game, 'btn-settings-done', 'menu');
    }
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
  assert.deepEqual(found, []);
}

test('desktop: no part of the menu covers another', () => sweep('desktop', DESKTOP, false));

test('phone: no part of the menu covers another, either way up', () => sweep('iphone', PHONE, true));
