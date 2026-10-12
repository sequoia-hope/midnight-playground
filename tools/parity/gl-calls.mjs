// WebGL2 calls per frame of a race: the Rust client's WebGL2 build or the
// JS game, in headless Chrome. The count of calls, by name, is what the
// browser's GL decoding and the driver pay for on a weak machine, where
// the Rust client (wgpu-hal's gles backend) and three.js differ most.
//
//   node tools/parity/gl-calls.mjs [--game rust|js] [--level sierra]
//       [--hq 1|0] [--secs 5] [--size WxH] [--dist next] [--query "cars=0"]
//       [--dump-shaders <dir>]   every shader source the page compiles
//       [--by-program --passes 30]   the GPU split also by shader program
//       [--angle gl|vulkan]   ANGLE's backend (default vulkan)
//       [--headed]   a window on $DISPLAY, as rust-perf.mjs's --headed
//       [--sub 'regex=>replacement' ...]   edit every shader source as it is
//                 compiled (an experiment: what a shader costs without X);
//                 --sub-vert and --sub-frag for one stage
//
// Every WebGL2RenderingContext method is wrapped before the page's scripts
// run; the race is the sports car on the autopilot (seed 1, no pursuit),
// counted for `--secs` from race time 4 s, through the registered dev
// server (`lib/server.mjs`).
//
// It also times the GPU: with `EXT_disjoint_timer_query_webgl2` (headless
// Chrome on this machine has it) a TIME_ELAPSED query spans the GL work
// issued between one requestAnimationFrame start and the next, so the
// report's "gpu" line is the GPU's own time per frame, independent of the
// CPU and of vsync, the one number that says whether a frame is heavier
// on the GPU than another build's.

import puppeteer from 'puppeteer-core';
import { baseUrl, chromeFlags } from './lib/server.mjs';

const args = process.argv.slice(2);
const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
const game = opt('--game', 'rust');
const level = opt('--level', 'sierra');
const hq = opt('--hq', '1');
const secs = Number(opt('--secs', 5));
const dist = opt('--dist', 'next');
const [w, h] = opt('--size', '1920x1080').split('x').map(Number);
const base = baseUrl(opt);

const browser = await puppeteer.launch({
  executablePath: process.env.CHROME_PATH || '/usr/bin/google-chrome',
  headless: args.includes('--headed') ? false : 'new', // --headed: a window on $DISPLAY (a laptop's headless Chrome may miss the GPU)
  args: chromeFlags({ backend: 'webgl2', race: true, rust: game === 'rust', angle: opt('--angle', 'vulkan') }),
  protocolTimeout: 600000,
  dumpio: !!process.env.MP_DUMPIO, // Chrome's own stderr, e.g. with INTEL_DEBUG=perf for Mesa's shader notes
});
try {
  const page = await browser.newPage();
  page.on('pageerror', (e) => console.error(`[page error] ${e}`));
  page.on('console', (m) => { if (m.type() === 'error') console.error(`[page] ${m.text()}`); });
  await page.setViewport({ width: w, height: h, deviceScaleFactor: 1 });
  const subs = [];
  for (let i = 0; i < args.length; i++) {
    const stage = { '--sub': 'both', '--sub-vert': 'vert', '--sub-frag': 'frag' }[args[i]];
    if (stage) { const [re, to] = args[i + 1].split('=>'); subs.push([re, to ?? '', stage]); }
  }
  await page.evaluateOnNewDocument((subs) => {
    const counts = {};
    let frames = 0;
    const gpu = [];
    window.__gl = { counts, frames: () => frames, gpu, timer: null, shaders: [], programs: {} };
    // Each program's sources, by its number (the same numbering as the
    // `--by-program` split), gathered through attachShader and linkProgram.
    const shaderSrc = new WeakMap();
    const attached = new WeakMap();
    window.__gl.onShader = (k, a) => {
      if (k === 'shaderSource') shaderSrc.set(a[0], a[1]);
      else if (k === 'attachShader') { if (!attached.has(a[0])) attached.set(a[0], []); attached.get(a[0]).push(a[1]); }
      else if (k === 'linkProgram') {
        if (!progIds.has(a[0])) progIds.set(a[0], nextProg++);
        window.__gl.programs[`p${progIds.get(a[0])}`] = (attached.get(a[0]) || []).map((sh) => shaderSrc.get(sh) || '');
      }
    };
    // The page's WebGL2 context, for the GPU timer.
    let ctx = null, ext = null, active = null;
    const pending = [];
    const getContext = HTMLCanvasElement.prototype.getContext;
    HTMLCanvasElement.prototype.getContext = function (type, ...rest) {
      const c = getContext.call(this, type, ...rest);
      // The newest context: the page probes WebGL2 on a throwaway canvas
      // before the game's own is made.
      if (c && type === 'webgl2' && c !== ctx) {
        if (active) { ctx.endQuery(ext.TIME_ELAPSED_EXT); active = null; }
        pending.length = 0;
        ctx = c;
        ext = c.getExtension('EXT_disjoint_timer_query_webgl2');
        window.__gl.timer = !!ext;
      }
      return c;
    };
    // The frame's GPU time, and its split by render target: the query
    // is cut at every bindFramebuffer, each piece keyed by the framebuffer
    // (numbered in order of creation; 0 is the canvas) and the viewport
    // it was last drawn with.
    const byTarget = {};
    window.__gl.byTarget = byTarget;
    // Triangles (and instances) drawn, by the same key: vertex work.
    const trisBy = {}, instBy = {}, vertsBy = {}, nonIndexedBy = {};
    window.__gl.trisBy = trisBy; window.__gl.instBy = instBy; window.__gl.vertsBy = vertsBy; window.__gl.nonIndexedBy = nonIndexedBy;
    window.__gl.onDraw = (k, a) => {
      const key = keyNow();
      drawsBy[key] = (drawsBy[key] || 0) + 1;
      let verts = 0, inst = 1;
      if (k === 'drawElements') verts = a[1];
      else if (k === 'drawElementsInstanced') { verts = a[1]; inst = a[4]; }
      else if (k === 'drawArrays') verts = a[2];
      else if (k === 'drawArraysInstanced') { verts = a[2]; inst = a[3]; }
      else return;
      const tris = a[0] === 4 ? verts / 3 : a[0] === 5 || a[0] === 6 ? Math.max(0, verts - 2) : 0;
      trisBy[key] = (trisBy[key] || 0) + tris * inst;
      instBy[key] = (instBy[key] || 0) + inst;
      // Vertex shader runs: every vertex of a non-indexed draw; an indexed
      // draw's index count is the upper bound (the cache reuses hits).
      vertsBy[key] = (vertsBy[key] || 0) + verts * inst;
      if (k.startsWith('drawArrays')) nonIndexedBy[key] = (nonIndexedBy[key] || 0) + 1;
    };
    const fbIds = new WeakMap();
    let nextFb = 1, curFb = 'fb0', curVp = '', frameGpu = 0;
    // Each framebuffer's attachments, so a framebuffer bound again is keyed
    // by what it holds (three.js attaches once, at creation).
    const fbAtts = new WeakMap();
    let curFbObj = null;
    // wgpu draws every pass through one framebuffer and swaps its
    // attachments, so the colour attachment (texture or renderbuffer,
    // numbered in order of first use) is part of the key.
    const attIds = new WeakMap();
    let nextAtt = 1, curAtt = '';
    // What each attachment is (format, size, samples), from the storage
    // call made on the texture or renderbuffer bound at the time; the
    // depth attachment in use when a colour one is attached is kept too.
    const storage = new WeakMap(), bound = {};
    const fmtNames = { 0x881A: 'RGBA16F', 0x8058: 'RGBA8', 0x8C43: 'SRGB8_ALPHA8', 0x8CAC: 'DEPTH32F', 0x88F0: 'DEPTH24_STENCIL8', 0x8CAD: 'DEPTH32F_STENCIL8', 0x81A6: 'DEPTH24', 0x81A5: 'DEPTH16', 0x1908: 'RGBA', 0x1907: 'RGB', 0x8229: 'R8', 0x822F: 'RG16F', 0x822E: 'R32F', 0x8814: 'RGBA32F', 0x822D: 'R16F', 0x822B: 'RG8', 0x8059: 'RGB10_A2', 0x8C3A: 'R11F_G11F_B10F', 0x8D62: 'RGB565', 0x8056: 'RGBA4', 0x8057: 'RGB5_A1', 0x8D46: 'STENCIL8' };
    const fmtName = (f) => fmtNames[f] || `0x${Number(f).toString(16)}`;
    window.__gl.attachments = {};
    window.__gl.depthFor = {};
    window.__gl.onStorage = (k, a) => {
      if (k === 'bindTexture' || k === 'bindRenderbuffer') { bound[a[0]] = a[1]; return; }
      const obj = bound[a[0] === 0x8515 || (a[0] >= 0x8515 && a[0] <= 0x851A) ? 0x8513 : a[0]];
      if (!obj) return;
      if (k === 'texStorage2D') storage.set(obj, `tex ${fmtName(a[2])} ${a[3]}x${a[4]}`);
      else if (k === 'texImage2D' && a[1] === 0) storage.set(obj, `tex ${fmtName(a[2])} ${a[3]}x${a[4]}`);
      else if (k === 'renderbufferStorageMultisample') storage.set(obj, `rb ${fmtName(a[2])} ${a[3]}x${a[4]} x${a[1]}`);
      else if (k === 'renderbufferStorage') storage.set(obj, `rb ${fmtName(a[1])} ${a[2]}x${a[3]}`);
    };
    let curDepth = '';
    // `--by-program`: the key also names the program in use (numbered in
    // order of creation), splitting a pass by shader.
    const byProgram = new URLSearchParams(location.search).has('__byprogram');
    const progIds = new WeakMap();
    let nextProg = 1, curProg = '';
    const keyNow = () => `${curFb}/${curAtt} ${curVp}${byProgram ? ' ' + curProg : ''}`;
    window.__gl.onUseProgram = (prog) => {
      if (!byProgram) return;
      if (prog && !progIds.has(prog)) progIds.set(prog, nextProg++);
      curProg = prog ? `p${progIds.get(prog)}` : '';
      cut(keyNow());
    };
    const drawsBy = {};
    window.__gl.drawsBy = drawsBy;
    window.__gl.onAttach = (attachment, obj) => {
      if (attachment !== 0x8CE0 /* COLOR_ATTACHMENT0 */ && attachment !== 0x8D00 /* DEPTH_ATTACHMENT */ && attachment !== 0x821A /* DEPTH_STENCIL */) return;
      const atts = curFbObj ? (fbAtts.get(curFbObj) || fbAtts.set(curFbObj, {}).get(curFbObj)) : {};
      if (!obj || typeof obj !== 'object') { if (attachment === 0x8CE0) { curAtt = ''; atts.c = ''; } else atts.d = ''; return; }
      if (!attIds.has(obj)) attIds.set(obj, nextAtt++);
      const id = (attachment === 0x8CE0 ? 'c' : 'd') + attIds.get(obj);
      window.__gl.attachments[id] = storage.get(obj) || '?';
      if (attachment !== 0x8CE0) { curDepth = id; atts.d = id; if (atts.c) window.__gl.depthFor[atts.c] = id; return; }
      atts.c = id;
      curAtt = id;
      if (atts.d) window.__gl.depthFor[id] = atts.d;
      if (active) active.key = keyNow();
      else cut(keyNow());
    };
    const cut = (key) => {
      if (!ctx || !ext) return;
      if (active) { ctx.endQuery(ext.TIME_ELAPSED_EXT); pending.push([active.q, active.key]); active = null; }
      while (pending.length && ctx.getQueryParameter(pending[0][0], ctx.QUERY_RESULT_AVAILABLE)) {
        const [q, k] = pending.shift();
        if (!ctx.getParameter(ext.GPU_DISJOINT_EXT)) {
          const ms = ctx.getQueryParameter(q, ctx.QUERY_RESULT) / 1e6;
          if (k === null) { gpu.push(frameGpu); frameGpu = 0; } else { frameGpu += ms; byTarget[k] = (byTarget[k] || 0) + ms; }
        }
        ctx.deleteQuery(q);
      }
      active = { q: ctx.createQuery(), key };
      ctx.beginQuery(ext.TIME_ELAPSED_EXT, active.q);
    };
    const frameBoundary = () => {
      if (!ctx || !ext) return;
      // A zero-length piece keyed null marks the frame's end.
      cut(null);
      cut(keyNow());
    };
    window.__gl.onBindFramebuffer = (fb) => {
      curFbObj = fb || null;
      if (!fb) { curFb = 'fb0'; curAtt = ''; }
      else { if (!fbIds.has(fb)) fbIds.set(fb, nextFb++); curFb = `fb${fbIds.get(fb)}`; curAtt = (fbAtts.get(fb) || {}).c || ''; }
      cut(keyNow());
    };
    window.__gl.onViewport = (w, h) => {
      curVp = `${w}x${h}`;
      if (active) active.key = keyNow();
    };
    const P = WebGL2RenderingContext.prototype;
    for (const k of Object.getOwnPropertyNames(P)) {
      const d = Object.getOwnPropertyDescriptor(P, k);
      if (typeof d.value !== 'function' || k === 'constructor') continue;
      const f = d.value;
      P[k] = function (...a) {
        counts[k] = (counts[k] || 0) + 1;
        if (k === 'shaderSource') {
          const isVert = /gl_Position/.test(a[1]);
          for (const [re, to, stage] of subs) if (stage === 'both' || (stage === 'vert') === isVert) a[1] = a[1].replace(new RegExp(re, 'g'), to);
        }
        const r = f.apply(this, a);
        if (k === 'shaderSource') window.__gl.shaders.push(a[1]);
        if (k === 'shaderSource' || k === 'attachShader' || k === 'linkProgram') window.__gl.onShader(k, a);
        if (k === 'useProgram' && this === ctx) window.__gl.onUseProgram(a[0]);
        if (k.startsWith('draw') && this === ctx) window.__gl.onDraw(k, a);
        else if (k === 'bindFramebuffer' && this === ctx) window.__gl.onBindFramebuffer(a[1]);
        else if (k === 'viewport' && this === ctx) window.__gl.onViewport(a[2], a[3]);
        else if ((k === 'framebufferTexture2D' || k === 'framebufferTextureLayer' || k === 'framebufferRenderbuffer') && this === ctx) window.__gl.onAttach(a[1], k === 'framebufferTextureLayer' ? a[2] : a[3]);
        else if ((k === 'bindTexture' || k === 'bindRenderbuffer' || k === 'texStorage2D' || k === 'texImage2D' || k === 'renderbufferStorage' || k === 'renderbufferStorageMultisample') && this === ctx) window.__gl.onStorage(k, a);
        return r;
      };
    }
    const raf = window.requestAnimationFrame.bind(window);
    let cur = -1;
    window.requestAnimationFrame = (cb) => raf((ts) => {
      if (ts !== cur) { cur = ts; frames++; frameBoundary(); }
      return cb(ts);
    });
  }, subs);
  const q = new URLSearchParams({ level, autostart: 'sports', autodrive: '1', seed: '1', pursuit: '0', hq });
  for (const [k, v] of new URLSearchParams(opt('--query', ''))) q.set(k, v);
  if (args.includes('--by-program')) q.set('__byprogram', '1');
  const url = game === 'rust' ? `${base}dist/${dist}/index.html?${q}` : `${base}index.html?${q}`;
  console.error(url);
  await page.goto(url);
  const rt = game === 'rust' ? '(window.__mp?.race?.time ?? -1)' : '(window.__race?.time ?? -1)';
  await page.waitForFunction(`${rt} >= 4`, { timeout: Number(opt('--timeout', 180)) * 1000, polling: 200 });
  await page.evaluate(() => { for (const k in window.__gl.counts) delete window.__gl.counts[k]; for (const o of [window.__gl.byTarget, window.__gl.trisBy, window.__gl.instBy, window.__gl.drawsBy, window.__gl.vertsBy, window.__gl.nonIndexedBy]) for (const k in o) delete o[k]; window.__gl.gpu.length = 0; });
  const f0 = await page.evaluate(() => window.__gl.frames());
  await new Promise((r) => setTimeout(r, secs * 1000));
  const { counts, f1, gpu, timer, byTarget, trisBy, instBy, drawsBy, vertsBy, nonIndexedBy, attachments, depthFor } = await page.evaluate(() => ({ counts: window.__gl.counts, f1: window.__gl.frames(), gpu: window.__gl.gpu, timer: window.__gl.timer, byTarget: window.__gl.byTarget, trisBy: window.__gl.trisBy, instBy: window.__gl.instBy, drawsBy: window.__gl.drawsBy, vertsBy: window.__gl.vertsBy, nonIndexedBy: window.__gl.nonIndexedBy, attachments: window.__gl.attachments, depthFor: window.__gl.depthFor }));
  const n = f1 - f0;
  const rows = Object.entries(counts).sort((a, b) => b[1] - a[1]);
  const total = rows.reduce((a, [, v]) => a + v, 0);
  const draws = rows.filter(([k]) => /^draw/.test(k)).reduce((a, [, v]) => a + v, 0);
  console.log(`${game} ${level} hq ${hq} ${w}x${h}: ${n} frames, ${(total / n).toFixed(0)} GL calls a frame, ${(draws / n).toFixed(0)} draws`);
  if (timer && gpu.length) {
    const s = [...gpu].sort((a, b) => a - b);
    const q = (p) => s[Math.min(s.length - 1, Math.floor(p * s.length))].toFixed(2);
    console.log(`gpu ms a frame (${s.length} timed): p50 ${q(0.5)}, p90 ${q(0.9)}, p99 ${q(0.99)}, max ${q(1)}, mean ${(s.reduce((a, b) => a + b, 0) / s.length).toFixed(2)}`);
    const tn = s.length;
    const allTris = Object.values(trisBy).reduce((a, b) => a + b, 0);
    const allVerts = Object.values(vertsBy).reduce((a, b) => a + b, 0);
    console.log(`triangles a frame: ${(allTris / n / 1e3).toFixed(0)}k, vertices submitted ${(allVerts / n / 1e3).toFixed(0)}k; by pass (gpu ms, k triangles, k vertices, instances, draws of which non-indexed):`);
    for (const [k, v] of Object.entries(byTarget).sort((a, b) => b[1] - a[1]).slice(0, Number(opt('--passes', 12)))) console.log(`  ${(v / tn).toFixed(2).padStart(6)} ms  ${((trisBy[k] || 0) / n / 1e3).toFixed(0).padStart(6)}k  ${((vertsBy[k] || 0) / n / 1e3).toFixed(0).padStart(6)}k  ${((instBy[k] || 0) / n).toFixed(0).padStart(6)}  ${((drawsBy[k] || 0) / n).toFixed(0).padStart(5)}/${((nonIndexedBy[k] || 0) / n).toFixed(0)}  ${k}`);
    // The targets behind the keys: each colour attachment's storage and
    // the depth attachment it was last paired with.
    const used = new Set(Object.keys(byTarget).map((k) => k.split(' ')[0].split('/')[1]).filter(Boolean));
    for (const id of [...used].sort()) console.log(`  ${id} = ${attachments[id] || '?'}${id.startsWith('c') && depthFor[id] ? `, depth ${depthFor[id]} = ${attachments[depthFor[id]] || '?'}` : ''}`);
  } else {
    console.log(`gpu: ${timer === false ? 'no EXT_disjoint_timer_query_webgl2' : 'no timed frames'}`);
  }
  for (const [k, v] of rows.slice(0, Number(opt('--top', 40)))) console.log((v / n).toFixed(1).padStart(8), k);
  const dump = opt('--dump-shaders', null);
  if (dump) {
    const fs = await import('node:fs');
    fs.mkdirSync(dump, { recursive: true });
    const programs = await page.evaluate(() => window.__gl.programs);
    let n = 0;
    for (const [id, srcs] of Object.entries(programs)) for (const src of srcs) { fs.writeFileSync(`${dump}/${id}-${/gl_Position/.test(src) ? 'vert' : 'frag'}.glsl`, src); n++; }
    console.log(`${n} shader sources of ${Object.keys(programs).length} programs in ${dump}`);
  }
} finally {
  await browser.close();
}
