// Every level loads, starts and runs: with the autopilot at double speed the
// player makes progress along the route, everyone stays on sane positions,
// the renderer keeps producing frames, and nothing throws.

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame } from './harness.js';
import { LEVELS } from '../../src/levels/index.js';
import { waitRacing } from './flow-helpers.js';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

// Positions and progress of every car in the race, read in the page.
const sample = () => {
  const r = window.__race, t = r.track;
  const cars = [r.player, ...r.ais.map((a) => a.v)];
  const standings = r.standings();
  return {
    state: r.state,
    loop: !!t.loop,
    length: t.length,
    finishS: t.finishS,
    s: r.player.s,
    dist: r.dist,
    time: r.time,
    lat: r.player.lat,
    hw: t.frame(r.player.s).hw,
    place: standings.findIndex((x) => x.player) + 1,
    racers: standings.length,
    rivals: r.ais.length,
    finite: cars.every((v) => [v.x, v.y, v.z, v.s, v.vx, v.vz, v.yaw].every(Number.isFinite)),
    aiS: r.ais.map((a) => a.s),
    fps: window.__stats.fps ?? 0,
  };
};

for (const level of LEVELS) {
  test(`${level.id}: loads, races and makes progress on the autopilot`, async () => {
    const game = await openGame(browser, { query: `level=${level.id}&autostart=sports&autodrive=1&timescale=2` });
    try {
      assert.equal(await game.eval('window.__world.level.id'), level.id);
      await waitRacing(game, 25000);
      const a = await game.eval(sample);
      assert.equal(a.rivals, level.rivals?.length ?? 0, 'the level\'s rivals are on the grid');
      assert.equal(a.racers, a.rivals + 1);
      await game.waitFor(`window.__race.dist > ${a.dist + 60}`, { timeout: 20000, what: `the player to cover 60 m on ${level.id}` });
      const b = await game.eval(sample);

      assert.ok(b.finite, 'no NaN or Infinity in any car\'s position or velocity');
      assert.ok(b.time > a.time, 'the race clock runs');
      assert.ok(b.dist > a.dist + 60, `distance covered (${a.dist.toFixed(1)} → ${b.dist.toFixed(1)} m)`);
      if (!b.loop) {
        assert.ok(b.s > a.s + 50, `moving forward along the route (s ${a.s.toFixed(1)} → ${b.s.toFixed(1)})`);
        assert.ok(b.s < b.finishS, 'not past the finish yet');
        assert.ok(b.aiS.every((s) => s > 0 && s < b.finishS + 1000), 'rivals are on the route');
      } else {
        assert.ok(b.s >= 0 && b.s < b.length, 's stays wrapped on a loop');
      }
      assert.ok(Math.abs(b.lat) < b.hw + 3, `on the road (lat ${b.lat.toFixed(2)}, half width ${b.hw.toFixed(2)})`);
      assert.ok(b.place >= 1 && b.place <= b.racers, `a sane position (${b.place} of ${b.racers})`);
      assert.equal(b.state, 'racing');
      await game.waitFor('(window.__stats.fps ?? 0) > 0', { timeout: 5000, what: 'frame stats' });
      assert.deepEqual(game.errors, []);
    } finally { await game.close(); }
  });
}
