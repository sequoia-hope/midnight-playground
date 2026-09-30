// Seaside Raceway, the circuit: its menu card, the start lights, the lap
// counter, a lap ticking over at the line, the finish after three laps,
// and the lap times on the results screen. The laps are driven by putting
// the car just short of the line (the race counts progress, so it's moved
// on too), as the finish-line tests do on the sprints.

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame } from './harness.js';
import { isShown, startFromMenu, waitRacing, expectScreen, sleep } from './flow-helpers.js';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

// Put the player `before` metres short of the line with `laps` laps done,
// doing 40 m/s, with the rivals well behind.
const nearLine = (laps, before = 40) => `(() => {
  const r = __race, t = r.track, s = t.wrap(t.startS - ${before});
  r.phys.reset(s, 0);
  const f = t.frame(s);
  r.player.vx = f.fx * 40; r.player.vz = f.fz * 40;
  r.player.prog = t.startS + ${laps} * t.n - ${before};
  r.progS.set(r.player, r.player.s);
  for (const a of r.ais) { a.s = t.wrap(s - 400); a.prog = r.player.prog - 400; r.progS.set(a, a.s); a.writePos(); }
})()`;

test('menu: Seaside Raceway has its card, with laps and the lap length', async () => {
  const game = await openGame(browser, { query: 'level=seaside' });
  try {
    await game.waitFor(`window.__world?.level.id === 'seaside' && __game.mode === 'menu'`, { timeout: 30000, what: 'the circuit to build' });
    assert.equal(await game.eval(`document.getElementById('lvl-name').textContent`), 'Seaside Raceway');
    assert.match(await game.eval(`document.getElementById('lvl-len').textContent`), /^3 laps of 3\.6 km/);
    assert.equal(await game.eval(isShown, '#mode-pick'), false, 'no Hot Pursuit on a closed circuit');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('the start lights come on through the countdown and go out at GO', async () => {
  const game = await openGame(browser, { query: 'level=seaside' });
  try {
    await startFromMenu(game);
    const lit = () => game.eval(`__world.scenery[0].lampMats.filter((m) => m.emissiveIntensity > 0).length`);
    await game.waitFor(`__race.state === 'countdown' && __race.countdown < 1.2`, { timeout: 15000, what: 'the end of the countdown' });
    assert.ok(await lit() >= 4, `${await lit()} lights on with a second to go`);
    await waitRacing(game);
    await sleep(100);
    assert.equal(await lit(), 0, 'all out at GO');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('three laps: the lap ticks over at the line, the race ends on lap 3, results list the laps', async () => {
  const game = await openGame(browser, { query: 'level=seaside&timescale=2' });
  try {
    await startFromMenu(game);
    // Lap 1 of 3 up on the HUD from the grid.
    assert.equal(await game.eval(isShown, '#hud-lap'), true);
    assert.equal(await game.eval(`document.getElementById('hud-lap-n').textContent`), 'LAP 1/3');
    await waitRacing(game);
    await sleep(500);
    // Across the line at the end of lap 1.
    await game.eval(nearLine(1));
    await game.waitFor(`__race.lap === 2`, { timeout: 10000, what: 'lap 2' });
    const l2 = await game.eval(`({ laps: __race.lapTimes.length, hud: document.getElementById('hud-lap-n').textContent, best: document.getElementById('hud-lap-best').textContent, state: __race.state })`);
    assert.equal(l2.laps, 1, 'one lap time');
    assert.equal(l2.hud, 'LAP 2/3');
    assert.match(l2.best, /^BEST LAP \d:\d\d\.\d\d$/);
    assert.equal(l2.state, 'racing', 'still racing');
    // Lap 3 is the last.
    await game.eval(nearLine(2));
    await game.waitFor(`__race.lap === 3`, { timeout: 10000, what: 'lap 3' });
    assert.equal(await game.eval(`__race.state`), 'racing');
    // Over the line a third time: the finish, and the results.
    await game.eval(nearLine(3));
    await game.waitFor(`__race.playerFinished === true`, { timeout: 10000, what: 'the finish' });
    assert.equal(await game.eval(`__race.lapTimes.length`), 3, 'three lap times');
    await expectScreen(game, 'results', 15000);
    const res = await game.eval(`({ title: document.getElementById('res-title').textContent, stats: [...document.querySelectorAll('#res-extra .res-stat small')].map((e) => e.textContent) })`);
    assert.equal(res.title, 'You win!');
    assert.deepEqual(res.stats.slice(0, 3).map((s) => s.replace(' ★', '')), ['Lap 1', 'Lap 2', 'Lap 3']);
    assert.match(res.stats[3], /lap record/i);
    // The lap record is kept for the menu card.
    assert.ok(await game.eval(`JSON.parse(localStorage.getItem('mr.bestLap.seaside')) > 0`));
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('the other levels show no lap counter', async () => {
  const game = await openGame(browser, { query: 'level=sierra' });
  try {
    await startFromMenu(game);
    assert.equal(await game.eval(isShown, '#hud-lap'), false);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});
