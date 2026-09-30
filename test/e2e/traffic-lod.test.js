// Traffic's far LOD (CarModel's far model, switched by Traffic.lod): a car
// a long way from the camera draws in a handful of calls, sits exactly where
// the full model does, keeps its own lights, and swaps back up close.

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame } from './harness.js';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

test('far traffic draws in 4 calls, in the same place, with working lights', async () => {
  const game = await openGame(browser, { query: 'autostart=sports' });
  try {
    await game.waitFor(() => !!window.__race?.traffic, { what: 'the race' });
    const r = await game.eval(() => {
      const THREE = window.__THREE;
      // Draw calls a model makes: visible meshes, one per material group.
      const calls = (root) => {
        let n = 0;
        const visit = (o) => {
          if (!o.visible) return;
          if (o.isMesh) n += Array.isArray(o.material) ? o.geometry.groups.length : 1;
          o.children.forEach(visit);
        };
        visit(root);
        return n;
      };
      // Bounds of what's drawn, in the car's own frame (Box3.setFromObject
      // counts hidden meshes too, and a world box of a yawed car overshoots).
      const box = (root) => {
        const b = new THREE.Box3(), g = new THREE.Box3(), m4 = new THREE.Matrix4();
        const toCar = root.matrixWorld.clone().invert();
        const visit = (o) => {
          if (!o.visible) return;
          if (o.isMesh) { o.geometry.computeBoundingBox(); b.union(g.copy(o.geometry.boundingBox).applyMatrix4(m4.multiplyMatrices(toCar, o.matrixWorld))); }
          o.children.forEach(visit);
        };
        visit(root);
        return b;
      };
      const out = {};
      for (const kind of ['sedan', 'hatch', 'van', 'pickup', 'boxtruck']) {
        const m = __race.traffic.pool[kind][0].v.model;
        m.root.visible = true;
        // At rest: the far model bakes the wheels in, unturned, under a body
        // that isn't rolling.
        m.body.rotation.set(0, 0, 0);
        for (const w of m.wheels) w.rotation.x = 0;
        for (const p of m.steerPivots) p.rotation.y = 0;
        m.setFar(false);
        m.root.updateMatrixWorld(true);
        const near = calls(m.root);
        const nearBox = box(m.root);
        m.setFar(true);
        m.root.updateMatrixWorld(true);
        const far = calls(m.root);
        const farBox = box(m.root);
        // The tail lights (on the kinds that have them) are still this car's own.
        const tail = m.body.children.find((o) => o.name === 'tail' && o.visible);
        m.setBrake(0); const off = tail?.material.emissiveIntensity;
        m.setBrake(1); const on = tail?.material.emissiveIntensity;
        m.setBrake(0);
        m.setFar(false);
        m.root.visible = false;
        const gap = Math.max(...['min', 'max'].flatMap((k) => ['x', 'y', 'z'].map((a) => Math.abs(nearBox[k][a] - farBox[k][a]))));
        out[kind] = { near, far, gap, brakes: tail ? on > off : null };
      }
      return out;
    });
    for (const [kind, c] of Object.entries(r)) {
      assert.ok(c.far <= 4, `${kind}: far model draws in ${c.far} calls (full model ${c.near})`);
      assert.ok(c.near >= 3 * c.far, `${kind}: ${c.near} → ${c.far} calls`);
      assert.ok(c.gap < 0.02, `${kind}: far model's bounds are ${c.gap.toFixed(3)} m off the full model's`);
      if (c.brakes !== null) assert.ok(c.brakes, `${kind}: brake lights still light up far away`);
    }
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('traffic switches to the far model past ~90 m and back close up', async () => {
  const game = await openGame(browser, { query: 'autostart=sports' });
  try {
    await game.waitFor(() => !!window.__race?.traffic, { what: 'the race' });
    const r = await game.eval(() => {
      const tr = __race.traffic, c = tr.pool.sedan[0];
      tr.activate(c, __race.player.s + 50, 0, 1);
      const at = (d) => { c.v.x = d; c.v.z = 0; tr.lod(0, 0); return c.v.model.isFar; };
      const seq = [at(40), at(100), at(90), at(80), at(90), at(200), at(10)];
      tr.despawn(c);
      return seq;
    });
    // 90 m sits inside the hysteresis band: it keeps whichever it had.
    assert.deepEqual(r, [false, true, true, false, false, true, false]);
  } finally { await game.close(); }
});
