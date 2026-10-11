// A CPU profile of the Rust client racing in headless Chrome, by function
// and by Bevy system (DECISIONS D1180's method, formalised).
//
//   cargo xtask web --release && cargo xtask prof
//   node tools/parity/rust-prof.mjs --level sierra [--backend webgpu|webgl2]
//       [--phone | --size WxH] [--throttle N] [--hq 1|0] [--secs N]
//       [--dist prof] [--query "cars=0"] [--tag name]
//   node tools/parity/rust-prof.mjs --from parity/report/perf/prof-sierra-webgl2.cpuprofile
//       [--callers <regex>] [--depth N] [--top N]
//
// The build must keep its function names (`cargo xtask prof` writes
// dist/prof/; dist/next/ is stripped and shows `wasm-function[1234]`).
// Chrome's sampling profiler (CDP `Profiler`, 250 µs) runs for `--secs`
// (default 10) from race time 4 s of the sports car on the autopilot (seed
// 1, no pursuit), through the registered dev server (`lib/server.mjs`).
//
// Three reports from the samples:
//   self      the functions with the most samples of their own
//   inclusive the functions with the most samples under them, without the
//             schedule's and the glue's frames every sample passes through
//   systems   inclusive time by the nearest enclosing Bevy system: a sample
//             is attributed to the innermost `FunctionSystem<fn(...), (),
//             (), path::to::system>` frame above it (the system's name is
//             that last generic argument); samples under no system are
//             "(outside systems)": the browser, GC, wasm-bindgen glue
//   callers   with `--callers <regex>`: the paths (callers' chains, `--depth`
//             frames) that reach the matching functions, by inclusive time
//
// `--phone` is a phone in landscape with the CPU throttled 4x and hq off,
// as rust-perf.mjs emulates one; `--throttle` alone throttles the desktop
// viewport (a 2017 laptop is about a 4x-throttled core here). The raw
// profile goes to parity/report/perf/prof-<level>-<backend>[-tag].cpuprofile
// (`--from` reports an existing one again).

import puppeteer from 'puppeteer-core';
import fs from 'node:fs';
import path from 'node:path';
import { ROOT } from './lib/jstree.mjs';
import { baseUrl, chromeFlags } from './lib/server.mjs';

const args = process.argv.slice(2);
const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
const level = opt('--level', 'sierra');
const backend = opt('--backend', 'webgpu');
const phone = args.includes('--phone');
const throttle = Number(opt('--throttle', phone ? 4 : 0));
const hq = opt('--hq', phone ? '0' : '1');
const secs = Number(opt('--secs', 10));
const dist = opt('--dist', 'prof');
const tag = opt('--tag', null);
const from = opt('--from', null);
const callersRe = opt('--callers', null);
const depth = Number(opt('--depth', 6));
const top = Number(opt('--top', 40));

const clean = (s) => s.replace(/::h[0-9a-f]{16}$/, '').replace(/\[[0-9a-f]{16}\]/g, '');
const name = (n) => clean(n.callFrame.functionName || `(${n.callFrame.url.split('/').pop()}:${n.callFrame.lineNumber})`);

/// The Bevy system a frame belongs to, from its function name, or null.
function systemOf(fn) {
  if (!fn.includes('FunctionSystem<') && !fn.includes('SystemState<')) return null;
  const m = fn.match(/, \(\), \(\), ([^<>]+?)> as /) || fn.match(/, ([a-z_][\w:]*?)> as /);
  return m ? m[1] : null;
}

function report(profile, meta) {
  const byId = new Map(profile.nodes.map((n) => [n.id, n]));
  const parent = new Map();
  for (const n of profile.nodes) for (const c of n.children || []) parent.set(c, n.id);
  for (const n of profile.nodes) n.hits = 0;
  for (const s of profile.samples) byId.get(s).hits++;
  const total = profile.samples.length;
  const pct = (x) => ((100 * x) / total).toFixed(1).padStart(5);
  const sorted = (m) => [...m].sort((a, b) => b[1] - a[1]);

  if (callersRe) {
    // Inclusive time of each subtree, then the chains above the outermost
    // matching frames.
    const tot = new Map();
    const sub = (n) => {
      if (tot.has(n.id)) return tot.get(n.id);
      let t = n.hits;
      for (const c of n.children || []) t += sub(byId.get(c));
      tot.set(n.id, t);
      return t;
    };
    const re = new RegExp(callersRe);
    const paths = new Map();
    for (const n of profile.nodes) {
      if (!re.test(name(n))) continue;
      let a = parent.get(n.id), inner = false;
      while (a !== undefined) {
        if (re.test(name(byId.get(a)))) { inner = true; break; }
        a = parent.get(a);
      }
      if (inner) continue;
      const chain = [];
      let id = parent.get(n.id);
      while (id !== undefined && chain.length < depth) {
        chain.push(name(byId.get(id)).slice(0, 300));
        id = parent.get(id);
      }
      const k = chain.join('\n      <- ');
      paths.set(k, (paths.get(k) || 0) + sub(n));
    }
    console.log(`--- callers of /${callersRe}/ (inclusive, % of ${total} samples) ---`);
    for (const [k, v] of sorted(paths).slice(0, top)) console.log(`${pct(v)}\n      <- ${k}\n`);
    return;
  }

  const self = new Map();
  const systems = new Map();
  const incl = new Map();
  for (const n of profile.nodes) {
    if (!n.hits) continue;
    self.set(name(n), (self.get(name(n)) || 0) + n.hits);
    const seen = new Set();
    for (let id = n.id; id !== undefined; id = parent.get(id)) {
      const k = name(byId.get(id));
      if (!seen.has(k)) { seen.add(k); incl.set(k, (incl.get(k) || 0) + n.hits); }
    }
    let id = n.id, sys = null;
    while (id !== undefined) {
      sys = systemOf(byId.get(id).callFrame.functionName);
      if (sys) break;
      id = parent.get(id);
    }
    const k = sys ? clean(sys) : '(outside systems)';
    systems.set(k, (systems.get(k) || 0) + n.hits);
  }
  const idle = (self.get('(idle)') || 0) + (self.get('(program)') || 0);
  console.log(`${meta}\nsamples ${total}, idle+program ${pct(idle).trim()}%, gc ${pct(self.get('(garbage collector)') || 0).trim()}%`);
  console.log('--- self ---');
  for (const [k, v] of sorted(self).slice(0, top)) console.log(pct(v), k.slice(0, 160));
  console.log('--- by system (inclusive, nearest enclosing) ---');
  for (const [k, v] of sorted(systems).slice(0, top)) console.log(pct(v), k);
  // Inclusive time by function, without the schedule's and the glue's
  // frames that every sample passes through.
  const skip = /^\(|run_unsafe|apply_deferred|executor|Schedule|FnOnce|call_once|closure|World::run|App::update|__wbg|wasm-function|wasm-to-js|js-to-wasm|run_schedule|SystemState|FunctionSystem|RenderGraphRunner|run_graph/;
  console.log('--- inclusive, by function ---');
  for (const [k, v] of sorted(incl).filter(([k]) => !skip.test(k)).slice(0, top)) console.log(pct(v), k.slice(0, 160));
}

if (from) {
  const profile = JSON.parse(fs.readFileSync(from, 'utf8'));
  report(profile, `${from}`);
  process.exit(0);
}

const base = baseUrl(opt);
const browser = await puppeteer.launch({
  executablePath: process.env.CHROME_PATH || '/usr/bin/google-chrome',
  headless: 'new',
  args: chromeFlags({ backend, race: true }),
  protocolTimeout: 600000,
});
try {
  const page = await browser.newPage();
  if (phone) {
    await page.setViewport({ width: 844, height: 390, deviceScaleFactor: 3, isMobile: true, hasTouch: true, isLandscape: true });
  } else {
    const [w, h] = opt('--size', '1280x800').split('x').map(Number);
    await page.setViewport({ width: w, height: h, deviceScaleFactor: 1 });
  }
  const cdp = await page.createCDPSession();
  if (throttle > 1) await cdp.send('Emulation.setCPUThrottlingRate', { rate: throttle });
  const q = new URLSearchParams({ level, autostart: 'sports', autodrive: '1', seed: '1', pursuit: '0', hq });
  for (const [k, v] of new URLSearchParams(opt('--query', ''))) q.set(k, v);
  const url = `${base}dist/${dist}/index.html?${q}`;
  console.error(`${url}  (${backend}${phone ? ', phone' : ''}${throttle > 1 ? `, throttle ${throttle}` : ''})`);
  await page.goto(url);
  await page.waitForFunction('(window.__mp?.race?.time ?? -1) >= 4', { timeout: 600000, polling: 200 });
  const got = await page.evaluate(() => ({ backend: window.__mp?.backend, named: !!window.__mp }));
  await cdp.send('Profiler.enable');
  await cdp.send('Profiler.setSamplingInterval', { interval: 250 });
  await cdp.send('Profiler.start');
  const s0 = await page.evaluate(() => window.__mp.race.s);
  await new Promise((r) => setTimeout(r, secs * 1000));
  const { profile } = await cdp.send('Profiler.stop');
  const s1 = await page.evaluate(() => window.__mp.race.s);
  const dir = path.join(ROOT, 'parity', 'report', 'perf');
  fs.mkdirSync(dir, { recursive: true });
  const file = path.join(dir, `prof-${level}-${backend}${tag ? '-' + tag : ''}.cpuprofile`);
  fs.writeFileSync(file, JSON.stringify(profile));
  if (!profile.nodes.some((n) => /FunctionSystem</.test(n.callFrame.functionName))) {
    console.error(`warning: no Bevy system names in the profile; is dist/${dist}/ a names-kept build (cargo xtask prof)?`);
  }
  report(profile, `${level} ${got.backend || backend}, hq ${hq}, s ${s0.toFixed(0)} to ${s1.toFixed(0)} over ${secs} s; written ${path.relative(ROOT, file)}`);
} finally {
  await browser.close();
}
