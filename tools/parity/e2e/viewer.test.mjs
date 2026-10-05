// The level viewer, "god mode" (SPEC 8.6, roadmap WP 6.9): every level
// opens in it, each camera answers `__mr.viewer.set` and real input (keys,
// mouse, wheel, a fake pad as gamepad.test.mjs installs it, touches in
// phone emulation, held sideways and upright), the panel's toggles work, a
// link reopens the same picture, and the menu's "Level viewer" button
// opens it.
//
//   cargo xtask web --release && node --test tools/parity/e2e/viewer.test.mjs
//
// MR_VIEWER_LEVELS=coast,seaside limits the per-level test.

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { launch, openGame, sleep, LEVEL_IDS } from './harness.mjs';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

const LEVELS = process.env.MR_VIEWER_LEVELS ? process.env.MR_VIEWER_LEVELS.split(',') : LEVEL_IDS;
const errorsOf = (game) => game.errors.filter((e) => !/Ignored attempt to cancel a touchstart event/.test(e));
const vw = (game) => game.eval(() => window.__mr.viewer);

async function openViewer(query, o = {}) {
  const game = await openGame(browser, { query, ...o });
  await game.waitFor(() => window.__mr.viewer?.ready === true, { timeout: 60000, what: 'the viewer' });
  await game.frames(4);
  return game;
}
const settled = (game) => game.waitFor(() => !window.__mr.viewer.flying, { timeout: 10000, what: 'the flight to land' });
const set = async (game, o) => { await game.eval((x) => window.__mr.viewer.set(x), o); await game.frames(3); };
const dist = (a, b) => Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z);

for (const level of LEVELS) {
  test(`${level}: opens in the viewer; every camera, the overview whole`, async (t) => {
    const game = await openViewer(`view=god&level=${level}`);
    try {
      let v = await vw(game);
      assert.equal(v.level, level);
      assert.equal(v.mode, 'free');
      assert.equal(await game.eval('window.__mr.screen'), 'viewer');
      assert.ok(v.zone, 'a zone name');
      assert.ok(v.groups.length >= 3, `scene groups: ${v.groups}`);
      assert.ok(v.fog && !v.far && v.anim && v.follow);
      // The overview: fog off, far plane out, high above the whole route.
      await set(game, { mode: 'over' });
      await settled(game);
      v = await vw(game);
      assert.equal(v.mode, 'over');
      assert.ok(!v.fog && v.far, 'the overview turns fog off and the far plane out');
      assert.ok(Math.abs(v.pose.pitch + Math.PI / 2) < 1e-3, 'straight down');
      assert.ok(v.pose.y > 300, `high above (${v.pose.y})`);
      assert.ok(await game.eval(() => window.__mr.ui('vw-zone-0')?.visible), 'the first zone is named on the map');
      // Orbit about a point of the route, then the ride along it.
      await set(game, { mode: 'orbit', cam: [v.orbit.x + 80, v.orbit.y + 60, v.orbit.z, Math.PI / 2, -0.6], orbit: [v.orbit.x, v.orbit.y, v.orbit.z] });
      v = await vw(game);
      assert.equal(v.mode, 'orbit');
      assert.ok(v.fog && !v.far, 'leaving the overview gives fog and the far plane back');
      await set(game, { mode: 'ride' });
      await settled(game);
      v = await vw(game);
      assert.equal(v.mode, 'ride');
      const s0 = v.ride.s;
      await set(game, { ride: { v: 40 } });
      await sleep(1000);
      assert.ok((await vw(game)).ride.s > s0 + 10, 'the ride moves along the route');
      await set(game, { mode: 'free' });
      v = await vw(game);
      t.diagnostic(`${level}: ${v.groups.length} groups (${v.groups.join(', ')}), ${v.draws} draws, ${(v.tris / 1e6).toFixed(2)} M tris`);
      assert.deepEqual(errorsOf(game), []);
    } finally { await game.close(); }
  });
}

test('keyboard and mouse drive every camera', async () => {
  const game = await openViewer('view=god&level=seaside');
  try {
    const { page } = game;
    const vh = await game.eval('innerHeight'), vwid = await game.eval('innerWidth');
    // Free: W flies where it looks, E rises, a drag looks, the wheel sets the speed.
    let a = await vw(game);
    await game.key('KeyW', 600);
    let b = await vw(game);
    const f = { x: -Math.sin(a.pose.yaw) * Math.cos(a.pose.pitch), y: Math.sin(a.pose.pitch), z: -Math.cos(a.pose.yaw) * Math.cos(a.pose.pitch) };
    const moved = { x: b.pose.x - a.pose.x, y: b.pose.y - a.pose.y, z: b.pose.z - a.pose.z };
    assert.ok(moved.x * f.x + moved.y * f.y + moved.z * f.z > 5, `W moves ahead (${JSON.stringify(moved)})`);
    await game.key('KeyE', 400);
    assert.ok((await vw(game)).pose.y > b.pose.y + 2, 'E rises');
    a = await vw(game);
    await page.mouse.move(vwid * 0.6, vh * 0.5);
    await page.mouse.down();
    await page.mouse.move(vwid * 0.6 + 120, vh * 0.5 + 40, { steps: 6 });
    await page.mouse.up();
    await game.frames(3);
    b = await vw(game);
    assert.ok(b.pose.yaw < a.pose.yaw - 0.05, `dragging right turns right (${a.pose.yaw} → ${b.pose.yaw})`);
    assert.ok(b.pose.pitch < a.pose.pitch - 0.02, 'dragging down looks down');
    await page.mouse.wheel({ deltaY: -300 });
    await game.frames(3);
    assert.notEqual((await vw(game)).speed, b.speed, 'the wheel changes the speed');
    // Orbit (2): a drag circles at the same distance, the wheel zooms.
    await game.key('Digit2');
    a = await vw(game);
    assert.equal(a.mode, 'orbit');
    await page.mouse.move(vwid * 0.6, vh * 0.5);
    await page.mouse.down();
    await page.mouse.move(vwid * 0.6 - 150, vh * 0.5, { steps: 6 });
    await page.mouse.up();
    await game.frames(3);
    b = await vw(game);
    assert.ok(Math.abs(b.pose.yaw - a.pose.yaw) > 0.1, 'a drag circles');
    assert.ok(Math.abs(b.orbit.dist - a.orbit.dist) < 1e-6, 'at the same distance');
    await page.mouse.wheel({ deltaY: -500 });
    await game.frames(3);
    assert.ok((await vw(game)).orbit.dist < b.orbit.dist, 'the wheel zooms in');
    // The overview (3): a drag moves the map, a click flies down to orbit there.
    await game.key('Digit3');
    await settled(game);
    a = await vw(game);
    assert.equal(a.mode, 'over');
    await page.mouse.move(vwid * 0.6, vh * 0.5);
    await page.mouse.down();
    await page.mouse.move(vwid * 0.6 + 100, vh * 0.5 + 60, { steps: 5 });
    await page.mouse.up();
    await game.frames(3);
    b = await vw(game);
    assert.ok(Math.hypot(b.overview.x - a.overview.x, b.overview.z - a.overview.z) > 1, 'the drag pans the map');
    await page.mouse.click(vwid * 0.7, vh * 0.4);
    await game.frames(3);
    await settled(game);
    const o = await vw(game);
    assert.equal(o.mode, 'orbit', 'a click on the overview flies down to orbit there');
    assert.ok(o.fog && !o.far);
    // Ride (4): W speeds up, Q lowers.
    await game.key('Digit4');
    await settled(game);
    a = await vw(game);
    await game.key('KeyW', 500);
    await game.key('KeyQ', 500);
    b = await vw(game);
    assert.equal(b.mode, 'ride');
    assert.ok(b.ride.v > a.ride.v + 5, 'W speeds the ride up');
    assert.ok(b.ride.h < a.ride.h, 'Q lowers it');
    // M cycles back to free.
    await game.key('KeyM');
    assert.equal((await vw(game)).mode, 'free');
    assert.deepEqual(errorsOf(game), []);
  } finally { await game.close(); }
});

test('the panel: toggles, groups, sliders, fold', async () => {
  const game = await openViewer('view=god&level=desert');
  try {
    let v = await vw(game);
    for (const [id, key] of [['vw-fog', 'fog'], ['vw-far', 'far'], ['vw-anim', 'anim']]) {
      const before = v[key];
      await game.click('#' + id);
      v = await vw(game);
      assert.equal(v[key], !before, `${id} toggles ${key}`);
      assert.equal((await game.ui('#' + id)).value, !before, `${id} shows it`);
    }
    // Animators frozen: the sea and the sky hold still (nothing to see
    // here but the flag); time pinned by the slider.
    const tod = await game.ui('#vw-tod');
    await game.page.mouse.click(tod.x + tod.w * 0.8, tod.y + tod.h / 2);
    await game.frames(3);
    v = await vw(game);
    assert.ok(!v.follow && Math.abs(v.t - 0.8) < 0.03, `the time of day pinned at 80 % (${v.t})`);
    await game.click('#vw-follow');
    assert.ok((await vw(game)).follow, 'Follow lets it follow the camera again');
    // The route slider jumps the camera.
    const route = await game.ui('#vw-route');
    await game.page.mouse.click(route.x + route.w * 0.5, route.y + route.h / 2);
    await game.frames(3);
    v = await vw(game);
    assert.ok(Math.abs(v.s - v.routeLength * 0.5) < v.routeLength * 0.03, `half way along the route (${v.s} of ${v.routeLength})`);
    // A scene group hidden takes its draws away.
    await set(game, { cam: [v.pose.x, v.pose.y + 200, v.pose.z, v.pose.yaw, -0.5] });
    await game.frames(5);
    const d0 = (await vw(game)).draws;
    const terrain = v.groups.indexOf('terrain');
    assert.ok(terrain >= 0, `a terrain group (${v.groups})`);
    await game.click(`#vw-group-${terrain}`);
    await game.frames(5);
    v = await vw(game);
    assert.deepEqual(v.hidden, ['terrain']);
    assert.ok(v.draws < d0, `fewer draws with the terrain hidden (${d0} → ${v.draws})`);
    assert.ok(v.link.includes('hide=terrain'), 'the link carries it');
    await game.click(`#vw-group-${terrain}`);
    assert.deepEqual((await vw(game)).hidden, []);
    // Folding.
    await game.click('#vw-fold');
    assert.equal((await vw(game)).folded, true);
    assert.equal(await game.ui('#vw-fog'), null, 'folded, the body is gone');
    await game.click('#vw-fold');
    assert.equal((await vw(game)).folded, false);
    assert.deepEqual(errorsOf(game), []);
  } finally { await game.close(); }
});

// The two pictures compared in the page: the share of pixels whose
// channels differ by more than 24 (of 255).
async function differ(game, a, b) {
  const [da, db] = await Promise.all([a, b].map((f) => fs.readFile(f).then((x) => 'data:image/png;base64,' + x.toString('base64'))));
  return game.eval(async (ua, ub) => {
    const load = async (u) => { const i = new Image(); i.src = u; await i.decode(); const c = document.createElement('canvas'); c.width = i.width; c.height = i.height; const g = c.getContext('2d'); g.drawImage(i, 0, 0); return g.getImageData(0, 0, c.width, c.height).data; };
    const [pa, pb] = await Promise.all([load(ua), load(ub)]);
    if (pa.length !== pb.length) return 1;
    let n = 0;
    for (let i = 0; i < pa.length; i += 4) {
      if (Math.abs(pa[i] - pb[i]) > 24 || Math.abs(pa[i + 1] - pb[i + 1]) > 24 || Math.abs(pa[i + 2] - pb[i + 2]) > 24) n++;
    }
    return n / (pa.length / 4);
  }, da, db);
}

test('a link reopens the same view', async (t) => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'mr-viewer-'));
  // Frozen animators and a pinned time of day, so the two pictures can
  // match: the link carries both.
  const a = await openViewer('view=god&level=coast&anim=0&t=0.3&panel=0', { downloads: dir });
  let link;
  try {
    const v = await vw(a);
    await set(a, { mode: 'orbit', cam: [v.pose.x + 37.123, v.pose.y + 52.5, v.pose.z - 11.77, 0.8123, -0.4567], orbit: [v.pose.x, v.pose.y - 10, v.pose.z] });
    await a.frames(20);
    await a.shot('link-a.png', dir);
    // The address follows the view once it settles.
    await a.waitFor(() => location.search.includes('cam='), { what: 'the address to carry the pose' });
    link = await a.eval(() => window.__mr.viewer.link);
    assert.ok(/mode=orbit/.test(link) && /cam=/.test(link) && /orbit=/.test(link) && /anim=0/.test(link) && /t=0.3/.test(link), link);
    t.diagnostic(link);
  } finally { await a.close(); }
  const b = await openViewer(link, { downloads: dir });
  try {
    await b.frames(20);
    await b.shot('link-b.png', dir);
    const pa = (await vw(b)).pose;
    assert.equal((await vw(b)).mode, 'orbit');
    const frac = await differ(b, path.join(dir, 'link-a.png'), path.join(dir, 'link-b.png'));
    t.diagnostic(`pixels that differ: ${(frac * 100).toFixed(2)} % (pose ${JSON.stringify(pa)})`);
    assert.ok(frac < 0.01, `the same picture (${(frac * 100).toFixed(2)} % differ)`);
  } finally { await b.close(); }
});

test('the menu\'s Level viewer button opens the viewer on the chosen level', async () => {
  const game = await openGame(browser, { storage: { 'mr.level': 'seaside' } });
  try {
    assert.equal(await game.screen(), 'menu');
    // A click that navigates: no frames waited after it (the page goes).
    const navClick = async (id) => {
      const { x, y } = await game.center(id);
      await Promise.all([game.page.waitForNavigation({ timeout: 30000 }), game.page.mouse.click(x, y)]);
    };
    await navClick('#btn-viewer');
    await game.waitFor(() => window.__mr?.viewer?.ready === true, { timeout: 120000, interval: 250, what: 'the viewer after the button' });
    const v = await vw(game);
    assert.equal(v.level, 'seaside');
    assert.ok(await game.eval(() => new URLSearchParams(location.search).get('view') === 'god'));
    // And back.
    await navClick('#vw-menu');
    await game.waitFor(() => window.__mr?.screen === 'menu' && window.__mr.ready, { timeout: 120000, interval: 250, what: 'the menu again' });
    assert.equal(await game.eval('window.__mr.level'), 'seaside');
    assert.deepEqual(errorsOf(game), []);
  } finally { await game.close(); }
});

test('gamepad: sticks, triggers, bumpers, Y and A', async () => {
  const game = await openViewer('view=god&level=seaside');
  try {
    await game.eval(() => {
      const pad = { id: 'Test Pad (STANDARD GAMEPAD Vendor: 045e Product: 028e)', index: 0, connected: true, mapping: 'standard', timestamp: 0,
        axes: [0, 0, 0, 0], buttons: Array.from({ length: 17 }, () => ({ pressed: false, touched: false, value: 0 })) };
      window.__pad = pad;
      Object.defineProperty(navigator, 'getGamepads', { configurable: true, value: () => [pad, null, null, null] });
    });
    await game.waitFor(() => window.__mr.pads?.connected === true, { what: 'the pad' });
    const axis = (i, x) => game.eval((j, v) => { window.__pad.axes[j] = v; }, i, x);
    const btn = (i, on) => game.eval((j, v) => { const b = window.__pad.buttons[j]; b.pressed = v; b.value = v ? 1 : 0; }, i, on);
    const press = async (i) => { await btn(i, true); await sleep(120); await btn(i, false); await sleep(120); };
    let a = await vw(game);
    await axis(1, -1); await sleep(600); await axis(1, 0);
    let b = await vw(game);
    assert.ok(dist(a.pose, b.pose) > 5, 'the left stick flies');
    a = b;
    await axis(2, 1); await sleep(400); await axis(2, 0);
    b = await vw(game);
    assert.ok(b.pose.yaw < a.pose.yaw - 0.2, 'the right stick looks');
    await btn(7, true); await sleep(500); await btn(7, false);
    assert.ok((await vw(game)).pose.y > b.pose.y + 3, 'RT rises');
    const sp = (await vw(game)).speed;
    await press(5);
    assert.ok((await vw(game)).speed > sp, 'RB speeds up');
    await press(3);
    assert.equal((await vw(game)).mode, 'orbit', 'Y: the next camera');
    await press(3);
    await settled(game);
    assert.equal((await vw(game)).mode, 'over');
    await press(0);
    await sleep(200);
    await settled(game);
    assert.equal((await vw(game)).mode, 'orbit', 'A in the overview dives to the middle');
    assert.deepEqual(errorsOf(game), []);
  } finally { await game.close(); }
});

for (const device of ['phone', 'phonePortrait']) {
  test(`touch (${device}): look, the stick, pinch, the overview's tap`, async () => {
    const game = await openViewer('view=god&level=seaside', { device });
    try {
      const touch = (type, pts) => game.cdp.send('Input.dispatchTouchEvent', { type, touchPoints: pts });
      const w = await game.eval('innerWidth'), h = await game.eval('innerHeight');
      let v = await vw(game);
      assert.equal(v.touch, true);
      assert.equal(v.folded, true, 'the panel starts folded on a phone');
      await game.tap('#vw-fold');
      assert.equal((await vw(game)).folded, false);
      await game.tap('#vw-fold');
      // One finger looks (grabbing the view: right turns left).
      let a = await vw(game);
      const p0 = { x: w * 0.6, y: h * 0.4, id: 1 };
      await touch('touchStart', [p0]);
      for (let i = 1; i <= 5; i++) await touch('touchMove', [{ ...p0, x: p0.x + i * 20 }]);
      await touch('touchEnd', []);
      await game.frames(3);
      let b = await vw(game);
      assert.ok(b.pose.yaw > a.pose.yaw + 0.05, `a finger turns the view (${a.pose.yaw} → ${b.pose.yaw})`);
      // The stick, pushed up, flies ahead.
      const st = await game.ui('#vw-stick');
      const c = { x: st.x + st.w / 2, y: st.y + st.h / 2, id: 2 };
      a = await vw(game);
      await touch('touchStart', [c]);
      await touch('touchMove', [{ ...c, y: c.y - st.h / 2 }]);
      await sleep(700);
      await touch('touchEnd', []);
      b = await vw(game);
      assert.ok(dist(a.pose, b.pose) > 5, 'the stick flies');
      // Rise.
      const rise = await game.ui('#vw-rise');
      a = await vw(game);
      await touch('touchStart', [{ x: rise.x + rise.w / 2, y: rise.y + rise.h / 2, id: 3 }]);
      await sleep(500);
      await touch('touchEnd', []);
      assert.ok((await vw(game)).pose.y > a.pose.y + 3, 'the rise button rises');
      // Orbit, pinch to zoom in.
      await set(game, { mode: 'orbit' });
      a = await vw(game);
      const m = { x: w / 2, y: h / 2 };
      await touch('touchStart', [{ x: m.x - 30, y: m.y, id: 4 }, { x: m.x + 30, y: m.y, id: 5 }]);
      for (let i = 1; i <= 5; i++) await touch('touchMove', [{ x: m.x - 30 - i * 15, y: m.y, id: 4 }, { x: m.x + 30 + i * 15, y: m.y, id: 5 }]);
      await touch('touchEnd', []);
      await game.frames(3);
      b = await vw(game);
      assert.ok(b.orbit.dist < a.orbit.dist * 0.7, `pinching out zooms in (${a.orbit.dist} → ${b.orbit.dist})`);
      // The overview: a tap flies there.
      await set(game, { mode: 'over' });
      await settled(game);
      await game.page.touchscreen.tap(w * 0.55, h * 0.45);
      await game.frames(3);
      await settled(game);
      assert.equal((await vw(game)).mode, 'orbit', 'a tap on the overview flies down');
      assert.deepEqual(errorsOf(game), []);
    } finally { await game.close(); }
  });
}
