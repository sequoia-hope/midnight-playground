// Module traces from the Node oracle (roadmap WP 0.4, SPEC 4.6): staged
// scenarios of the simulation's modules, stepped as Race would step them,
// with the parity kernel, seeded streams, quantised input and dt = 1/120.
// Each scenario is described in parity/scenarios.md; this file is its
// executable definition.
//
//   NODE_OPTIONS=--import=./tools/parity/kernel/register.mjs \
//     node tools/parity/sim-module.mjs [--only id,...] [--check] [--list]
//
// Writes parity/golden/sim/module/<id>.trace.gz (committed). --check
// regenerates each in memory and fails unless the trace is byte for byte the
// committed one (compared after gunzip: zlib builds compress differently).

import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import { ROOT } from './lib/jstree.mjs';
import { traceRecord, TraceWriter, readTrace } from './lib/trace.mjs';
import { Sim, level, player, rivals, grid, traffic, pursuit, simStreams, autopilot, quantiseInput } from './lib/node-sim.mjs';
import { mulberry32 } from '../../src/util/math.js';

const SEED = 1;
const sec = (s) => Math.round(s * 120);

// Input helpers. Every scenario's input is a pure function of the tick and
// the state, quantised before use.
const idle = () => ({ steer: 0, throttle: 0, brake: 0, handbrake: false, nitro: false, analog: false });
const auto = (sim) => autopilot(idle(), sim.P.v, sim.t);

// Common staging: the level, the player (and optionally the rival field) on
// Race's grid, the streams.
function stage({ lvl, car, withRivals = false, trafficCount = 0, police = null }) {
  const { L, t, world } = level(lvl);
  const streams = simStreams(SEED);
  const P = player(t, car);
  const ais = withRivals ? rivals(L, t, streams.ai) : [];
  grid(t, P, ais);
  const traf = trafficCount ? traffic(L, t, world, trafficCount, streams.traffic) : null;
  let pu = null;
  if (police) {
    pu = pursuit(L, t, P.spec, { heat: police.heat, rng: streams.pursuit, policeRng: streams.police });
    pu.setRacers([{ body: P.body, player: true, name: 'You' }, ...ais.map((a) => ({ body: a, player: false, name: a.name, ai: a }))]);
  }
  return new Sim({ t, P, ais, traf, pu, streams });
}

// The catalogue. what: one line for scenarios.md.
const LEVEL_CARS = [['sierra', 'sports'], ['coast', 'muscle'], ['streets', 'super'], ['desert', 'rally'], ['seaside', 'electric'], ['cruise', 'sports']];

export const SCENARIOS = [
  // ── Physics alone (WP 1.3) ──
  ...['sports', 'muscle', 'super', 'rally', 'electric'].map((car) => ({
    id: `phys-launch-${car}`, ticks: sec(20),
    what: `${car} alone on Sierra's grid: autopilot steering; full throttle 12 s, full brake 3 s, coast 5 s`,
    setup: () => stage({ lvl: 'sierra', car }),
    input: (sim, k) => { const a = auto(sim); const tt = k / 120; return { ...a, throttle: tt < 12 ? 1 : 0, brake: tt >= 12 && tt < 15 ? 1 : 0, nitro: false }; },
  })),
  ...LEVEL_CARS.map(([lvl, car]) => ({
    id: `phys-autopilot-${lvl}`, ticks: sec(90),
    what: `${car} alone on ${lvl} from the grid, the autopilot driving, 90 s`,
    setup: () => stage({ lvl, car }),
    input: (sim) => auto(sim),
  })),
  {
    id: 'phys-handbrake', ticks: sec(40),
    what: 'sports alone on Sierra: autopilot, plus every 4 s a 1.2 s handbrake pull with full lock toward the autopilot\'s steer',
    setup: () => stage({ lvl: 'sierra', car: 'sports' }),
    input: (sim, k) => { const a = auto(sim); if (k % 480 >= 240 && k % 480 < 384) { a.handbrake = true; a.steer = a.steer >= 0 ? 1 : -1; } return a; },
  },
  {
    id: 'phys-wall', ticks: sec(30),
    what: 'muscle alone on Streets: autopilot with steer + 0.6 for 1 s in every 3 s, into the right-hand wall',
    setup: () => stage({ lvl: 'streets', car: 'muscle' }),
    input: (sim, k) => { const a = auto(sim); if (k % 360 < 120) a.steer = Math.min(1, a.steer + 0.6); return a; },
  },
  {
    id: 'phys-analog', ticks: sec(40),
    what: 'super alone on Coast: analogue steering (inp.analog), the autopilot\'s steer times 0.8 plus 0.2 sin(3t)',
    setup: () => stage({ lvl: 'coast', car: 'super' }),
    input: (sim, k) => { const a = auto(sim); a.analog = true; a.steer = Math.max(-1, Math.min(1, a.steer * 0.8 + 0.2 * Math.sin(3 * k / 120))); return a; },
  },
  {
    id: 'phys-reverse', ticks: sec(10),
    what: 'sports alone on Sierra\'s grid: brake held 4 s from a standstill (reverses), steer 0.5; then throttle 6 s, steer 0',
    setup: () => stage({ lvl: 'sierra', car: 'sports' }),
    input: (sim, k) => (k < 480 ? { ...idle(), brake: 1, steer: 0.5 } : { ...idle(), throttle: 1 }),
  },
  {
    id: 'phys-spiked-damaged', ticks: sec(30),
    what: 'rally alone on Desert with spiked = 10 and damage = 0.8 set before the first tick; autopilot',
    setup: () => { const sim = stage({ lvl: 'desert', car: 'rally' }); sim.P.phys.spiked = 10; sim.P.phys.damage = 0.8; return sim; },
    input: (sim) => auto(sim),
  },
  {
    id: 'phys-frame-dt', ticks: 2400, frameDt: true,
    what: 'sports alone on Sierra: CarPhysics.update called with frame times drawn from mulberry32(7) among 1/60, 1/144, 1/30, 0.025 and 0.05, not 1/120 (the substep rule); autopilot input per call. Each call is one trace tick',
    setup: () => stage({ lvl: 'sierra', car: 'sports' }),
    input: (sim) => auto(sim),
  },
  // ── Rivals and collisions (WP 1.4) ──
  ...LEVEL_CARS.filter(([l]) => l !== 'cruise').map(([lvl, car]) => ({
    id: `ai-field-${lvl}`, ticks: sec(60),
    what: `${car} (autopilot) and ${lvl}'s rival field from Race's grid, collisions, no traffic, 60 s`,
    setup: () => stage({ lvl, car, withRivals: true }),
    input: (sim) => auto(sim),
  })),
  {
    id: 'collide-rear-pin', ticks: sec(15),
    what: 'Sierra s = 1500: the sports car stopped 0.5 m off the right wall; the first rival placed 40 m behind on the same line at 35 m/s, its racing line pulled to the wall (bias +4); the player holds the brake throughout (so it reverses once stopped)',
    setup: () => {
      const sim = stage({ lvl: 'sierra', car: 'sports', withRivals: true });
      const t = sim.t, f = t.frame(1500);
      sim.P.phys.reset(1500, f.wallR - sim.P.v.halfW - 0.5);
      sim.ais = sim.ais.slice(0, 1);
      const a = sim.ais[0];
      a.s = 1460; a.lat = sim.P.v.lat; a.speed = 35; a.bias = 4; a.writePos();
      return sim;
    },
    input: () => ({ ...idle(), brake: 1 }),
  },
  // ── Traffic (WP 1.4) ──
  ...LEVEL_CARS.map(([lvl, car]) => ({
    id: `traffic-${lvl}`, ticks: sec(60),
    what: `${car} (autopilot) on ${lvl} with traffic (${lvl === 'cruise' ? 30 : 22} cars, as Race), collisions and the crash flag, no rivals, 60 s`,
    setup: () => stage({ lvl, car, trafficCount: lvl === 'cruise' ? 30 : 22 }),
    input: (sim) => auto(sim),
  })),
  {
    id: 'full-field-sierra', ticks: sec(120),
    what: 'sports (autopilot), Sierra\'s rival field and 22 traffic cars from Race\'s grid, 120 s: the whole race field without Race\'s rules',
    setup: () => stage({ lvl: 'sierra', car: 'sports', withRivals: true, trafficCount: 22 }),
    input: (sim) => auto(sim),
  },
  // ── Pursuit (WP 1.6) ──
  ...[['sierra', 'sports'], ['coast', 'rally'], ['streets', 'muscle'], ['desert', 'electric']].map(([lvl, car]) => ({
    id: `pursuit-${lvl}`, ticks: sec(90),
    what: `${car} (autopilot), ${lvl}'s rivals, 18 traffic cars and the police from heat 3, 90 s; Pursuit's hit rules and the PIT yaw kick, no PursuitView damage`,
    setup: () => stage({ lvl, car, withRivals: true, trafficCount: 18, police: { heat: 3 } }),
    input: (sim) => auto(sim),
  })),
  {
    id: 'pursuit-heat5-props', ticks: sec(60),
    what: 'sports (autopilot) on Sierra with rivals and 18 traffic cars, heat 5, the pursuit forced on before the first tick (state pursuit, propT 0, spawnT 0): units, roadblocks, spikes, boxes',
    setup: () => {
      const sim = stage({ lvl: 'sierra', car: 'sports', withRivals: true, trafficCount: 18, police: { heat: 5 } });
      sim.pu.state = 'pursuit'; sim.pu.propT = 0; sim.pu.spawnT = 0;
      return sim;
    },
    input: (sim) => auto(sim),
  },
];

const FRAME_DTS = [1 / 60, 1 / 144, 1 / 30, 0.025, 0.05];

// The trace file's bytes, before gzip.
export function run(sc) {
  const sim = sc.setup();
  const w = new TraceWriter({ id: sc.id, seed: SEED, ticks: sc.ticks, source: 'module' });
  const dtRng = sc.frameDt ? mulberry32(7) : null;
  for (let k = 0; k < sc.ticks; k++) {
    const inp = quantiseInput(sc.input(sim, k));
    if (dtRng) {
      // Only the player's physics, at a frame time instead of the tick.
      sim.tick++;
      sim.P.phys.update(FRAME_DTS[Math.floor(dtRng() * FRAME_DTS.length)], inp);
      sim.P.phys.events.length = 0;
    } else sim.step(inp);
    w.add(sim.tick, traceRecord(sim.view(inp)));
  }
  return Buffer.from(w.finish());
}

// Compare traces, not gzip output: zlib versions compress differently.
function firstDiff(a, b) {
  const A = readTrace(new Uint8Array(a)), B = readTrace(new Uint8Array(b));
  const n = Math.min(A.hashes.length, B.hashes.length);
  for (let i = 0; i < n; i++) if (A.hashes[i][0] !== B.hashes[i][0] || A.hashes[i][1] !== B.hashes[i][1]) return `tick ${i + 1}`;
  return A.hashes.length !== B.hashes.length ? `length ${A.hashes.length} vs ${B.hashes.length}` : 'the metadata or full records';
}

// parity/scenarios.md: the prose below plus the table from the catalogue.
const DOC = `# Staged simulation scenarios (module traces)

The catalogue of the module oracle's scenarios (SPEC 4.6, roadmap WP 0.4).
Generated by \`node tools/parity/sim-module.mjs --doc\` from the catalogue
in that file, which is the executable definition: read its \`setup\` and
\`input\` functions, and \`tools/parity/lib/node-sim.mjs\`, for the details.
Each scenario's golden is \`parity/golden/sim/module/<id>.trace.gz\` (a gzipped
trace file, parity/trace-format.md), committed. M1 replays every one in Rust
and requires the hash of every tick to match.

## Common ground

- **Numbers.** The parity kernel is on (every inexact \`Math\` function from
  \`mp_math\`); dt is 1/120 s; the player's input is quantised as the Rust
  \`InputFrame\` (\`quantiseInput\`, src/parity/sim.js) before each tick.
- **Streams.** \`simStreams(1)\`: rivals draw from \`ai\`, the police weave from
  \`police\`, the pursuit from \`pursuit\`, traffic from \`traffic\` (mulberry32
  seed 99, Traffic's own).
- **Levels.** \`new Track(level)\` with \`runout\` from
  parity/golden/sim/world-data.json, and the opposite carriageway (range and
  lanes from the same file, height \`oppY\` from city/freeway.js) where the
  level has one.
- **Vehicles.** Models carry only the dimensions the game's CarModel gives
  that kind (world-data.json \`kinds\`): the high-detail ones for the player
  and the rivals, the low far ones for traffic and police. Masses, rival
  options and the grid are Race's (Race.js:59-98); traffic counts are Race's
  (22, 30 on the cruise loop, 18 with police); police are built as
  PursuitView builds them.
- **Input.** \`autopilot\` (src/game/autopilot.js) unless the scenario says
  otherwise; \`k\` is the tick index from 0.
- **One tick** (\`Sim.step\`, tools/parity/lib/node-sim.mjs), in Race.update's
  order without Race's own rules: the agent list (player, rivals, active
  traffic, pursuit bodies); player physics; rivals (ctx: playerS after
  physics, started, time); traffic (with the odometer on loops); the pursuit
  (bodies that joined are appended); collisions, then \`writePos\` for rivals,
  active traffic and pursuit bodies; per hit, the pursuit's \`onHit\` (its PIT
  return kicks the player's yaw rate by 2.2 × push, as PursuitView does) and
  the crash flag (SPEC 4.3 step 12); clear the physics and pursuit events.
  Not included: Race's rules (countdown, laps, bonuses, finish, parking) and
  PursuitView's (damage, holds, release); the whole-race recordings cover
  those.
- **Clock.** Physics is never locked (no countdown); the clock starts at 0
  and adds dt at the start of each tick, before anything moves.

## Scenarios

| Id | Ticks | What |
|---|---:|---|
`;

function main() {
  const args = process.argv.slice(2);
  if (args.includes('--list')) { for (const s of SCENARIOS) console.log(s.id.padEnd(24), s.what); return; }
  if (args.includes('--doc')) {
    const rows = SCENARIOS.map((s) => `| \`${s.id}\` | ${s.ticks} | ${s.what.replace(/\|/g, '\\|')} |`).join('\n');
    fs.writeFileSync(path.join(ROOT, 'parity/scenarios.md'), DOC + rows + '\n');
    console.log('wrote parity/scenarios.md');
    return;
  }
  const check = args.includes('--check');
  const only = args.includes('--only') ? args[args.indexOf('--only') + 1].split(',') : null;
  const dir = path.join(ROOT, 'parity/golden/sim/module');
  fs.mkdirSync(dir, { recursive: true });
  let bad = 0, total = 0;
  for (const sc of SCENARIOS.filter((s) => !only || only.includes(s.id))) {
    const t0 = Date.now();
    const out = run(sc);
    const file = path.join(dir, sc.id + '.trace.gz');
    const ms = Date.now() - t0;
    if (check) {
      const old = fs.existsSync(file) ? zlib.gunzipSync(fs.readFileSync(file)) : null;
      const same = old !== null && Buffer.compare(old, out) === 0;
      if (!same) bad++;
      console.log(`${sc.id}: ${same ? 'identical' : `DIFFERS (${old ? 'first at ' + firstDiff(old, out) : 'no golden'})`} (${ms} ms)`);
    } else {
      const gz = zlib.gzipSync(out, { level: 9 });
      total += gz.length;
      fs.writeFileSync(file, gz);
      console.log(`${sc.id}: ${sc.ticks} ticks, ${(gz.length / 1024).toFixed(0)} KB, ${ms} ms`);
    }
  }
  console.log(`total ${(total / 1e6).toFixed(2)} MB`);
  if (bad) { console.error(`${bad} scenario(s) differ from the committed goldens`); process.exit(1); }
}

if (import.meta.url === `file://${process.argv[1]}`) main();
