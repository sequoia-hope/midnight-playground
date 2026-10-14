// The open world's section builder in a worker (docs/vision/peninsula.md
// stage 4c, D1215): mp_builder as its own small wasm (mp_builder.wasm, no
// wasm-bindgen), planning the level once and building its sections as the
// client asks, each handed back packed (mp_worldgen::section::pack) as a
// transferred ArrayBuffer. The client (crates/mp_game/src/worker.rs) sends
//   { type: 'plan', generation, level }       → { type: 'planned', generation } or { type: 'failed', generation, error }
//   { type: 'build', generation, k }          → { type: 'built', generation, k, bytes } or { type: 'failed', generation, k, error }
// and `generation` stamps a level's requests, so a stale result is told apart.
// A trap (a panic in the builder) ends the worker's use: every later
// request fails, and the client builds on its own thread.

let x = null;
let dead = null;
const utf8 = new TextEncoder();
const utf8d = new TextDecoder();

const text = () => utf8d.decode(new Uint8Array(x.memory.buffer, x.mpb_out_ptr(), x.mpb_out_len()));

async function load() {
  if (x) return;
  const r = await fetch('mp_builder.wasm');
  if (!r.ok) throw new Error(`mp_builder.wasm: HTTP ${r.status}`);
  const bytes = await r.arrayBuffer();
  const imports = { env: { mpb_log: (p, n) => console.warn(utf8d.decode(new Uint8Array(x.memory.buffer, p, n))) } };
  x = (await WebAssembly.instantiate(bytes, imports)).instance.exports;
}

onmessage = async (e) => {
  const m = e.data;
  if (dead) { postMessage({ type: 'failed', generation: m.generation, k: m.k, error: dead }); return; }
  try {
    await load();
    if (m.type === 'plan') {
      const id = utf8.encode(m.level);
      const p = x.mpb_alloc(id.length);
      new Uint8Array(x.memory.buffer, p, id.length).set(id);
      const ok = x.mpb_plan(p, id.length);
      x.mpb_free(p, id.length);
      if (ok) postMessage({ type: 'planned', generation: m.generation });
      else postMessage({ type: 'failed', generation: m.generation, error: text() });
      x.mpb_out_clear();
    } else if (m.type === 'build') {
      const t0 = performance.now();
      const ok = x.mpb_build(m.k);
      if (ok) {
        // A copy out of the wasm's memory, transferred to the page.
        const out = new Uint8Array(x.memory.buffer, x.mpb_out_ptr(), x.mpb_out_len()).slice();
        x.mpb_out_clear();
        postMessage({ type: 'built', generation: m.generation, k: m.k, ms: performance.now() - t0, bytes: out.buffer }, [out.buffer]);
      } else {
        postMessage({ type: 'failed', generation: m.generation, k: m.k, error: text() });
        x.mpb_out_clear();
      }
    }
  } catch (err) {
    // A trap leaves the builder's state unknown: no more from it.
    dead = `builder: ${err && err.message ? err.message : err}`;
    postMessage({ type: 'failed', generation: m.generation, k: m.k, error: dead });
  }
};
