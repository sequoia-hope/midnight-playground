// Hot Pursuit in the real game: the menu's mode toggle, police on the road
// and the HUD, getting busted (a penalty hold, then a reset onto the road
// ahead of the police), getting wrecked, R being locked while you're held,
// and the results screen's pursuit stats. With the mode off nothing of it
// shows.

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame } from './harness.js';
import { isShown, stored, startFromMenu, waitRacing, expectScreen, sleep } from './flow-helpers.js';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

const FAST = 'timescale=2';

test('menu: the Race / Hot Pursuit toggle is offered on sprint levels, remembered per level, and starts a pursuit', async () => {
  const game = await openGame(browser, { query: FAST });
  try {
    assert.equal(await game.eval(isShown, '#mode-pick'), true, 'shown on Level 1');
    assert.equal(await game.eval(() => document.getElementById('btn-start').textContent), 'Race');
    await game.click('#mode-pick [data-mode="pursuit"]');
    assert.equal(await stored(game, 'mode.sierra'), 'pursuit');
    assert.equal(await game.eval(() => document.getElementById('btn-start').textContent), 'Hot Pursuit');
    assert.equal(await game.eval(() => document.querySelector('#mode-pick .sel').dataset.mode), 'pursuit');
    // The cruise loop has no police.
    const menuBack = () => game.waitFor(() => window.__game.mode === 'menu' && !document.getElementById('menu').classList.contains('hidden'), { timeout: 60000, what: 'the level to load' });
    await game.click('.lvl-tab:last-child');
    await menuBack();
    assert.equal(await game.eval(isShown, '#mode-pick'), false, 'hidden on the cruise');
    await game.click('.lvl-tab:first-child');
    await menuBack();
    assert.equal(await game.eval(isShown, '#mode-pick'), true);
    assert.equal(await game.eval(() => document.getElementById('btn-start').textContent), 'Hot Pursuit', 'remembered for Level 1');
    await startFromMenu(game);
    assert.equal(await game.eval(() => !!window.__pursuit && window.__race.pursuitOn), true, 'the race has police');
    assert.equal(await game.eval(isShown, '#hud-pz'), true, 'heat stars on the HUD');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('race mode: no police, no pursuit HUD', async () => {
  const game = await openGame(browser, { query: FAST + '&autostart=sports' });
  try {
    await waitRacing(game, 30000);
    assert.equal(await game.eval(() => !!window.__pursuit || !!window.__race.pv), false);
    assert.equal(await game.eval(isShown, '#hud-pz'), false);
    assert.equal(await game.eval(isShown, '#hud-dmg'), false);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('the police chase you: units join, the heat stars and the siren run, all without errors', async () => {
  const game = await openGame(browser, { query: 'level=desert&autostart=sports&autodrive=1&pursuit=1&heat=2&timescale=2' });
  try {
    await waitRacing(game, 30000);
    await game.waitFor(() => window.__pursuit.state === 'pursuit', { timeout: 60000, what: 'a pursuit to start' });
    const s = await game.eval(() => ({
      units: __pursuit.units.filter((u) => u.active && u.mode === 'chase').length,
      stars: document.querySelectorAll('#pz-stars .on, #pz-stars .full, #pz-stars [data-on="1"]').length,
      siren: __pursuit.units.some((u) => u.active && u.siren === 'flash'),
      visible: __pursuit.units.filter((u) => u.active).every((u) => u.v.model.root.visible),
    }));
    assert.ok(s.units >= 1, 'a unit on the chase');
    assert.ok(s.siren, 'lights on');
    assert.ok(s.visible, 'active units are drawn');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('dispatch speaks in the recorded voice: the race preloads its lines, and one without a recording gets the burble', async () => {
  const game = await openGame(browser, { query: 'level=streets&pursuit=1&cops=2&' + FAST });
  try {
    await startFromMenu(game); // the Race click starts the audio
    await waitRacing(game, 30000);
    // This level's lines are fetched ahead of need.
    await game.waitFor(() => performance.getEntriesByType('resource').filter((e) => e.name.includes('/audio/radio/') && e.name.endsWith('.mp3')).length >= 40,
      { timeout: 20000, what: 'the radio clips to preload' });
    const talking = () => {
      const cur = window.__audio._radioCur;
      return cur && {
        clips: cur.srcs.filter((s) => s.buffer && !s.loop).map((s) => +s.buffer.duration.toFixed(2)),
        burble: cur.srcs.some((s) => s instanceof OscillatorNode),
        text: document.getElementById('hud-radio-text').textContent,
      };
    };
    const unit = await game.eval(async () => {
      const { RADIO } = await import('/src/game/audio/radioLines.js');
      const u = window.__pursuit.units[0].callsign;
      window.__race.pv.say(RADIO.intercept(u, 'Nob Hill'), true);
      return u;
    });
    await game.waitFor(() => window.__audio._radioCur?.srcs.some((s) => s.buffer && !s.loop), { timeout: 5000, what: 'the line to play' });
    const said = await game.eval(talking);
    assert.equal(said.text, `Unit ${unit}, speeder on Nob Hill, moving to intercept.`);
    assert.equal(said.clips.length, 2, 'the callsign, then the message');
    assert.ok(said.clips[0] > 0.4 && said.clips[0] < 2.5 && said.clips[1] > 1.5 && said.clips[1] < 6, `clip lengths ${said.clips}`);
    assert.equal(said.burble, false);

    await game.eval(() => window.__race.pv.say({ text: 'Nobody recorded this.', parts: ['Nobody recorded this.'] }, true));
    await game.waitFor(() => window.__audio._radioCur?.srcs.some((s) => s instanceof OscillatorNode), { timeout: 5000, what: 'the burble' });
    assert.deepEqual((await game.eval(talking)).clips, []);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

// Stop the car with two units beside it until the bust meter fills.
async function getBusted(game) {
  await game.eval(() => {
    const r = __race, p = __pursuit, v = r.player;
    p.state = 'pursuit';
    const [a, b] = p.units.filter((u) => !u.active);
    p.activate(a, v.s - 7, v.lat, 0, 'chase'); a.target = r.playerBody;
    p.activate(b, v.s + 1, v.lat + (v.lat > 0 ? -3.2 : 3.2), 0, 'chase'); b.target = r.playerBody;
    window.__hold = setInterval(() => { v.vx = 0; v.vz = 0; a.speed = 0; b.speed = 0; }, 16);
  });
}

test('busted: the bust meter fills, you are held for a penalty, R does nothing, then you are put back ahead of the police', async () => {
  const game = await openGame(browser, { query: 'level=sierra&autostart=sports&pursuit=1&heat=2&cops=2' });
  try {
    await waitRacing(game, 30000);
    await sleep(1500);
    await getBusted(game);
    await game.waitFor(() => __pursuit.bust > 0.2, { timeout: 5000, what: 'the bust meter to fill' });
    assert.equal(await game.eval(isShown, '#pz-bar'), true, 'the BUST bar shows');
    await game.waitFor(() => __pursuit.player.hold > 0, { timeout: 8000, what: 'the bust' });
    await game.eval(() => clearInterval(window.__hold));
    const held = await game.eval(() => ({ reason: __pursuit.player.holdReason, total: __pursuit.player.holdTotal, s: __race.player.s, busts: __pursuit.busts, units: __pursuit.units.filter((u) => u.active).map((u) => ({ s: u.s, mode: u.mode })) }));
    assert.equal(held.reason, 'busted');
    assert.equal(held.busts, 1);
    assert.ok(held.total >= 5, `a ${held.total} s penalty`);
    assert.equal(await game.eval(isShown, '#hud-hold'), true, 'the BUSTED card');
    // R is locked while held.
    await game.key('KeyR');
    await sleep(200);
    assert.ok(Math.abs(await game.eval('__race.player.s') - held.s) < 3, 'R did not move the car');
    // The clock keeps running through the hold, and the throttle does nothing.
    const t0 = await game.eval('__race.time');
    await game.key('KeyW', 800);
    assert.ok(await game.eval(() => Math.hypot(__race.player.vx, __race.player.vz)) < 0.5, 'held still');
    assert.ok(await game.eval('__race.time') > t0 + 0.5, 'the race clock runs');
    // Released: back on the road, ahead of the units that made the arrest.
    await game.waitFor(() => __pursuit.player.hold <= 0, { timeout: 20000, what: 'the release' });
    const after = await game.eval(() => ({ s: __race.player.s, locked: __race.phys.locked, pen: __race.pv.penalty, hud: document.getElementById('hud-pen').textContent, grace: __pursuit.player.grace }));
    assert.equal(after.locked, false);
    assert.ok(after.s > Math.max(...held.units.filter((u) => u.mode === 'hold').map((u) => u.s)), 'reset ahead of the police');
    assert.ok(Math.abs(after.pen - held.total) < 0.01, 'the penalty is counted');
    assert.match(after.hud, /\+\d/, 'the penalty shows under the clock');
    assert.ok(after.grace > 0);
    assert.equal(await game.eval(isShown, '#hud-hold'), false);
    await game.key('KeyW', 1000);
    assert.ok(await game.eval(() => Math.hypot(__race.player.vx, __race.player.vz)) > 3, 'drives again');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('wrecked: enough damage wrecks the car, holds it, and it comes back repaired', async () => {
  const game = await openGame(browser, { query: 'level=coast&autostart=sports&pursuit=1&cops=0&timescale=2' });
  try {
    await waitRacing(game, 30000);
    await game.key('KeyW', 1500);
    await game.eval(() => __race.pv.hurt(0.6));
    assert.equal(await game.eval(isShown, '#hud-dmg'), true, 'the damage bar');
    assert.ok(await game.eval('__race.phys.damage') > 0.5, 'the car knows it is damaged');
    await game.eval(() => __race.pv.hurt(0.5));
    const h = await game.eval(() => ({ hold: __pursuit.player.hold, reason: __pursuit.player.holdReason, wrecks: __race.pv.wrecks }));
    assert.ok(h.hold > 0);
    assert.equal(h.reason, 'wrecked');
    assert.equal(h.wrecks, 1);
    await game.waitFor(() => __pursuit.player.hold <= 0, { timeout: 20000, what: 'the release' });
    assert.equal(await game.eval('__race.pv.damage'), 0, 'repaired');
    assert.equal(await game.eval('__race.phys.damage'), 0);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('results: the pursuit stats show and the best time is kept apart from normal races', async () => {
  const game = await openGame(browser, { query: 'level=streets&autostart=sports&pursuit=1&cops=0&timescale=2' });
  try {
    await waitRacing(game, 30000);
    await game.eval(() => { __race.pv.penalty = 7.5; });
    await game.eval(() => {
      const r = __race, t = r.track;
      r.phys.reset(t.finishS - 30, 0);
      const f = t.frame(t.finishS - 30);
      r.player.vx = f.fx * 40; r.player.vz = f.fz * 40;
      for (const [i, a] of r.ais.entries()) { a.s = t.finishS - 900 - i * 20; a.writePos(); }
    });
    await expectScreen(game, 'results', 20000);
    const text = await game.eval(() => document.getElementById('res-extra').textContent);
    assert.match(text, /Busted/);
    assert.match(text, /\+7\.5 s\s*Penalty/);
    assert.ok(await stored(game, 'best.streets.pursuit') > 0, 'best time under the pursuit key');
    assert.equal(await stored(game, 'best.streets'), null, 'not the normal race key');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});
