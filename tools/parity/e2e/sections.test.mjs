// The menu's flyover sections (DECISIONS D676, D740 on): a menu-first run
// downloads no level; every level's section is built in the client; a tab
// shows its section within a frame or two; Race frees them and loads the
// level whole; Main menu keeps the raced level for its tab and builds the
// sections again. Prints the times and the wasm memory it sees.
//
//   cargo xtask web --release && node --test tools/parity/e2e/sections.test.mjs
//   (MP_SHOTS=<dir> saves a picture of each tab's section;
//    MP_BACKEND=webgl2 runs the WebGL2 build)

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import os from 'node:os';
import { launch, openGame, startFromMenu, waitRacing, LEVEL_IDS, expectScreen } from './harness.mjs';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

const MB = (b) => +(b / 1048576).toFixed(0);
const shots = process.env.MP_SHOTS || null;
const backend = process.env.MP_BACKEND ? `backend=${process.env.MP_BACKEND}&` : '';

test('sections: instant tabs, Race loads the level, Main menu builds them again', async () => {
  const load = os.loadavg()[0].toFixed(1);
  const game = await openGame(browser, { query: backend + 'timescale=2', storage: { 'mr.level': 'sierra' }, downloads: shots });
  try {
    const boot = await game.eval(() => ({
      ready: window.__mp.readyMs, first: window.__mp.firstFrameMs, sceneUrl: window.__mp.sceneUrl ?? null,
      mem: window.__mp.wasmMemoryBytes(), sec: window.__mp.sections, backend: window.__mp.backend,
    }));
    // Frame times from here: the other sections are built behind the menu.
    await game.eval(() => {
      window.__ft = []; let last = performance.now();
      const f = (now) => { window.__ft.push(now - last); last = now; requestAnimationFrame(f); };
      requestAnimationFrame(f);
    });
    const frameStats = () => game.eval(() => {
      const a = window.__ft.splice(0).sort((x, y) => x - y);
      const q = (p) => a.length ? +a[Math.min(a.length - 1, Math.floor(p * a.length))].toFixed(1) : null;
      return { n: a.length, p50: q(0.5), p95: q(0.95), max: a.length ? +a[a.length - 1].toFixed(1) : null, over50: a.filter((x) => x > 50).length };
    });
    assert.equal(boot.sceneUrl, null, 'no level downloaded at boot');
    assert.equal(boot.sec.active, true);
    assert.equal(boot.sec.shown, 'sierra');
    await game.waitFor(() => window.__mp.sections?.allMs != null, { timeout: 180000, interval: 250, what: 'every section' });
    await game.frames(10);
    const all = await game.eval(() => ({ sec: window.__mp.sections, mem: window.__mp.wasmMemoryBytes(), t: performance.now() }));
    console.log(`# ${boot.backend}, load average ${load}: menu ready ${(boot.ready / 1000).toFixed(2)} s (first frame ${(boot.first / 1000).toFixed(2)} s, `
      + `first section shown ${(boot.sec.firstMs / 1000).toFixed(2)} s after its build began), `
      + `wasm ${MB(boot.mem)} MB; all sections ${(all.sec.allMs / 1000).toFixed(2)} s after the first was asked for, wasm ${MB(all.mem)} MB`);
    for (const s of all.sec.list) console.log(`#   ${s.id}: ${s.state}, build ${s.buildMs} ms, spawn ${s.spawnMs} ms, ${s.nodes} nodes, ${s.entities} entities${s.generated ? '' : ' (terrain and road only)'}`);
    for (const s of all.sec.list) assert.equal(s.state, 'up', s.id);
    console.log(`#   frames while the others were built behind the menu: ${JSON.stringify(await frameStats())}`);
    // Each tab: shown within a few frames, no loading screen, no download.
    for (const id of [...LEVEL_IDS.filter((l) => l !== 'sierra'), 'sierra']) {
      await frameStats();
      const t0 = await game.eval(() => performance.now());
      await game.click(`#lvl-tab-${id}`);
      const r = await game.waitFor(`(() => { const m = window.__mp; return m.sections.shown === ${JSON.stringify(id)} && m.level === ${JSON.stringify(id)} && { t: performance.now(), screen: m.screen }; })()`,
        { timeout: 5000, interval: 16, what: id + ' shown' });
      assert.equal(r.screen, 'menu');
      // The click takes two frames in the harness (`click` waits them).
      console.log(`#   tab ${id}: shown ${(r.t - t0).toFixed(0)} ms after the click (harness's two frames included)`);
      await game.frames(60);
      console.log(`#     frames after the switch: ${JSON.stringify(await frameStats())}`);
      if (shots) await game.shot(`section-${process.env.MP_BACKEND || 'webgpu'}-${id}.png`, shots);
    }
    const mem1 = await game.eval(() => window.__mp.wasmMemoryBytes());
    console.log(`#   wasm after the tabs: ${MB(mem1)} MB`);
    assert.equal(await game.eval(() => window.__mp.sceneUrl ?? null), null, 'still nothing downloaded');
    // Race on Coast: the sections go, the level is loaded whole.
    await game.click('#lvl-tab-coast');
    await game.waitFor(() => window.__mp.sections.shown === 'coast');
    const t1 = await game.eval(() => performance.now());
    await startFromMenu(game, { timeout: 240000 });
    const t2 = await game.eval(() => performance.now());
    await waitRacing(game, 60000);
    const race = await game.eval(() => ({ level: window.__mp.level, sec: window.__mp.sections, mem: window.__mp.wasmMemoryBytes(), mb: window.__mp.sceneMB ?? null }));
    console.log(`#   Race on coast: racing (countdown) ${((t2 - t1) / 1000).toFixed(2)} s after the click, `
      + `${race.mb == null ? 'built in the client' : 'a ' + race.mb + ' MB export downloaded'}, wasm ${MB(race.mem)} MB`);
    assert.equal(race.level, 'coast');
    assert.equal(race.sec.active, false);
    assert.equal(race.sec.full, 'coast');
    for (const s of race.sec.list) assert.equal(s.state, 'queued', 'freed: ' + s.id);
    // Main menu: Coast stays whole for its tab; the sections come back.
    await game.key('Escape');
    await expectScreen(game, 'pause');
    await game.click('#btn-quit');
    await expectScreen(game, 'menu');
    await game.waitFor(() => window.__mp.sections?.active && window.__mp.sections.allMs != null, { timeout: 180000, interval: 250, what: 'the sections again' });
    let back = await game.eval(() => window.__mp.sections);
    assert.equal(back.full, 'coast');
    assert.equal(back.shown, null);
    await game.click('#lvl-tab-desert');
    await game.waitFor(() => window.__mp.sections.shown === 'desert' && window.__mp.sections.full === null, { timeout: 5000, what: 'desert shown, coast freed' });
    await game.click('#lvl-tab-coast');
    await game.waitFor(() => window.__mp.sections.shown === 'coast', { timeout: 5000, what: "coast's section" });
    const mem2 = await game.eval(() => window.__mp.wasmMemoryBytes());
    console.log(`#   back on the menu, Coast freed for Desert's section: wasm ${MB(mem2)} MB`);
    // And a race from there: Seaside, downloaded.
    await game.click('#lvl-tab-seaside');
    await game.waitFor(() => window.__mp.sections.shown === 'seaside');
    await startFromMenu(game, { timeout: 240000 });
    await waitRacing(game, 60000);
    assert.equal(await game.eval(() => window.__mp.level), 'seaside');
    console.log(`#   Race on seaside after that: wasm ${MB(await game.eval(() => window.__mp.wasmMemoryBytes()))} MB`);
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});
