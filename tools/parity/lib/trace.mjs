// The trace record (SPEC 4.6; parity/trace-format.md is the definition,
// this is its JS writer). Both the JS reference runs and, later, the Rust
// port (mr_sim::trace_record) write the same bytes for the same state, so a
// tick's record, or its FNV-1a 64 hash, says whether the two agree.
//
// Runs in Node (the module oracle) and in the page (the game oracle; the
// recorder imports it from /tools/parity/lib/trace.mjs). No dependencies.
//
// The order of fields below IS the format. Change it only together with
// trace-format.md and TRACE_VERSION.

export const TRACE_VERSION = 1;

// ── Encodings ────────────────────────────────────────────────────────────
// "Missing" (null or undefined where the state allows it): a NaN with a
// payload no arithmetic produces, so it never collides with a real NaN.
const MISSING_HI = 0x7ff80000, MISSING_LO = 0x00000d1e;
export const I32_NONE = -2147483648;

export const ENUMS = {
  raceState: ['countdown', 'racing', 'finished'],
  parkKind: ['circuit', 'lane'],
  pursuitState: ['patrol', 'pursuit', 'cooldown'],
  mode: ['parked', 'chase', 'oncoming', 'search', 'standdown', 'hold', 'disabled', 'block'],
  behaviour: ['chase', 'bump', 'pit', 'roll', 'box'],
  slot: ['ahead', 'left', 'right', 'behind'],
  siren: ['off', 'flash', 'disabled'],
  holdReason: ['busted', 'wrecked'],
};
// null / undefined → -1; an unknown name is an error, not a silent code.
function code(table, v) {
  if (v === null || v === undefined) return -1;
  const i = ENUMS[table].indexOf(v);
  if (i < 0) throw new Error(`trace: ${table} has no code for ${JSON.stringify(v)}`);
  return i;
}

class Writer {
  constructor(size = 1 << 16) { this.buf = new ArrayBuffer(size); this.dv = new DataView(this.buf); this.n = 0; }
  room(k) {
    if (this.n + k <= this.buf.byteLength) return;
    const next = new ArrayBuffer(Math.max(this.buf.byteLength * 2, this.n + k));
    new Uint8Array(next).set(new Uint8Array(this.buf, 0, this.n));
    this.buf = next; this.dv = new DataView(next);
  }
  // A number the state always has. A non-number here is a bug in the
  // recorder or the game, so it fails loudly.
  f64(x) {
    if (typeof x !== 'number') throw new Error(`trace: expected a number, got ${x}`);
    this.room(8); this.dv.setFloat64(this.n, x, true); this.n += 8;
  }
  // A number that may be missing (null or undefined).
  opt(x) {
    if (x === null || x === undefined) { this.room(8); this.dv.setUint32(this.n, MISSING_LO, true); this.dv.setUint32(this.n + 4, MISSING_HI, true); this.n += 8; return; }
    this.f64(x);
  }
  i32(x) {
    if (!Number.isInteger(x) || x < -2147483648 || x > 2147483647) throw new Error(`trace: expected an i32, got ${x}`);
    this.room(4); this.dv.setInt32(this.n, x, true); this.n += 4;
  }
  bool(x) { this.i32(x ? 1 : 0); }
  bytes() { return new Uint8Array(this.buf, 0, this.n).slice(); }
}

// FNV-1a 64 over bytes, in two 32-bit halves (exact in doubles: the low
// half times 0x1b3 is under 2^41). Returns [lo, hi] as unsigned ints.
export function fnv1a64(bytes) {
  let lo = 0x84222325, hi = 0xcbf29ce4; // offset basis 0xcbf29ce484222325
  for (let i = 0; i < bytes.length; i++) {
    lo ^= bytes[i];
    // h * 0x100000001b3 = h * 0x1b3 + (h << 40)
    const t = (lo >>> 0) * 0x1b3;
    const carry = Math.floor(t / 4294967296);
    hi = ((hi >>> 0) * 0x1b3 + carry + ((lo << 8) >>> 0)) >>> 0;
    lo = t >>> 0;
  }
  return [lo >>> 0, hi >>> 0];
}
export const hashHex = ([lo, hi]) => hi.toString(16).padStart(8, '0') + lo.toString(16).padStart(8, '0');

// ── The record ───────────────────────────────────────────────────────────
// A view of the state, built by fromRace() for the game or by a scenario for
// the module oracle:
//   { tick, race: { state, time, countdown, throttleAt } | null,
//     players: [{ v, phys, rule (Race | null), pv (PursuitView | null) }],
//     rivals: [AIDriver], traffic: Traffic | { cars: [...] } | null,
//     pursuit: Pursuit | null, pv: PursuitView | null,
//     passed, nearMiss: Map/WeakMap by traffic car (Race), streams: {name: rng with .draws} }

export function traceRecord(view) {
  const w = new Writer();
  const refs = bodyRefs(view);
  const lastHit = view.pv?.lastHit ?? null;
  const pu = view.pursuit ?? null;
  const tcars = view.traffic?.cars ?? [];

  // Header.
  w.i32(view.tick);
  const R = view.race;
  w.bool(!!R);
  w.bool(!!pu);
  if (R) {
    w.i32(code('raceState', R.state));
    w.f64(R.time);
    w.f64(R.countdown);
    w.opt(R.throttleAt);
  }
  w.i32(view.players.length);
  w.i32(view.rivals.length);
  w.i32(tcars.length);
  w.i32(pu ? pu.units.length + pu.blockCars.length : 0);
  w.i32(pu ? pu.sawhorses.length : 0);
  // The input that drove this tick, quantised as the Rust InputFrame is.
  for (const P of view.players) writeInput(w, P.input);

  for (const P of view.players) writePlayer(w, P, view);
  for (const a of view.rivals) {
    writeKinematic(w, a);
    w.f64(a.avoid); w.f64(a.avoidTimer); w.f64(a.nitro); w.f64(a.nitroTimer);
    w.bool(a.finished); w.opt(a.finishTime); w.f64(a.throttle); w.bool(a.nitroActive);
    w.f64(a.hold); w.opt(a.holdLat); w.f64(a.spiked); w.opt(a.prog);
    writePark(w, a.park);
    if (pu) w.opt(lastHit?.get(a));
  }

  if (view.traffic) {
    w.bool(true);
    w.f64(view.traffic.nextSpawnS ?? 0); w.f64(view.traffic.nextOppS ?? 0); w.i32(view.traffic.maxActive ?? 0);
  } else w.bool(false);
  for (const c of tcars) {
    w.bool(c.active);
    writeKinematic(w, c);
    w.f64(c.crashed); w.f64(c.cruise); w.f64(c.laneLat); w.i32(c.lane ?? -1);
    w.opt(view.passed?.get(c));
    w.bool(view.nearMiss?.get(c) === 'hit');
    if (pu) w.opt(lastHit?.get(c));
  }

  if (pu) {
    w.i32(code('pursuitState', pu.state)); w.i32(pu.heat); w.i32(pu.maxHeat);
    w.f64(pu.heatMeter); w.f64(pu.bust); w.f64(pu.evade); w.f64(pu.time);
    w.f64(pu.spawnT); w.f64(pu.propT); w.opt(pu.patrolT);
    w.i32(pu.takedowns); w.i32(pu.busts);
    const rb = pu.roadblock;
    w.bool(!!rb);
    if (rb) {
      w.f64(rb.s); w.f64(rb.gapLat); w.bool(rb.heavy); w.bool(rb.passed); w.bool(rb.touched);
      w.i32(rb.cars.length); for (const c of rb.cars) w.i32(refs.get(c));
    }
    const sp = pu.spikes;
    w.bool(!!sp);
    if (sp) {
      w.f64(sp.s); w.f64(sp.lat0); w.f64(sp.lat1); w.i32(refs.get(sp.car)); w.bool(sp.passed);
      let mask = 0; pu.racers.forEach((r, i) => { if (sp.hit.has(r)) mask |= 1 << i; }); w.i32(mask);
    }
    w.i32(pu.spots.length); for (const s of pu.spots) w.bool(s.used);
    w.i32(pu.racers.length);
    for (const r of pu.racers) {
      w.f64(r.bust); w.f64(r.hold); w.f64(r.holdTotal); w.i32(code('holdReason', r.holdReason));
      w.f64(r.grace); w.bool(r.finished); w.opt(r.prevS);
    }
    for (const u of [...pu.units, ...pu.blockCars]) {
      w.bool(u.active);
      writeKinematic(w, u);
      w.i32(code('mode', u.mode)); w.i32(code('behaviour', u.behaviour)); w.f64(u.behT); w.f64(u.modeT);
      w.i32(code('slot', u.slot)); w.i32(u.pitSide); w.f64(u.pitCooldown); w.f64(u.pitPush ?? 0);
      w.i32(u.target ? refs.get(u.target) ?? fail('unknown target') : -1);
      w.f64(u.lastSeenS); w.f64(u.health); w.f64(u.disabledT); w.f64(u.parkLat); w.f64(u.blockYaw);
      w.opt(u.gapLat); w.f64(u.laneLat); w.f64(u.cap); w.f64(u.weave); w.opt(u.retarget);
      w.bool(u.uturned); w.f64(u.avoid ?? 0); w.f64(u.avoidTimer ?? 0); w.i32(code('siren', u.siren));
      w.opt(lastHit?.get(u));
    }
    for (const b of pu.sawhorses) {
      w.bool(b.active); w.bool(b.broken);
      writeKinematic(w, b);
      w.f64(b.h); w.f64(b.vy); w.f64(b.age); w.opt(b.gapLat);
      w.opt(lastHit?.get(b));
    }
  }

  // Where each named random stream is (draws so far), or -1.
  for (const name of ['ai', 'police', 'pursuit', 'traffic']) {
    const s = view.streams?.[name];
    w.i32(s && typeof s.draws === 'number' ? s.draws : -1);
  }
  return w.bytes();
}

function fail(msg) { throw new Error('trace: ' + msg); }

// InputFrame: steer as i16 (±32767), throttle and brake as u8, flags
// handbrake 1, nitro 2, analog 4. A tick with no input (the player is not
// driven) writes I32_NONE for the steer and zeros.
export function writeInput(w, inp) {
  if (!inp) { w.i32(I32_NONE); w.i32(0); w.i32(0); w.i32(0); return; }
  const c = (x, lo, hi) => (x < lo ? lo : x > hi ? hi : x);
  w.i32(Math.round(c(inp.steer, -1, 1) * 32767));
  w.i32(Math.round(c(inp.throttle, 0, 1) * 255));
  w.i32(Math.round(c(inp.brake, 0, 1) * 255));
  w.i32((inp.handbrake ? 1 : 0) | (inp.nitro ? 2 : 0) | (inp.analog ? 4 : 0));
}

// Track coordinates and the world pose a KinematicCar writes.
function writeKinematic(w, k) {
  w.f64(k.s); w.f64(k.lat); w.f64(k.speed); w.f64(k.latVel); w.i32(k.dir);
  w.f64(k.spin); w.f64(k.spinRate); w.f64(k.stunned);
  const v = k.v;
  w.f64(v.x); w.f64(v.y); w.f64(v.z); w.f64(v.yaw); w.f64(v.visualYaw); w.f64(v.vx); w.f64(v.vz);
}

function writePark(w, park) {
  if (!park) { w.i32(-1); return; }
  w.i32(code('parkKind', park.kind));
  if (park.kind === 'lane') { w.f64(park.stopAt); w.f64(park.laneLat); w.f64(park.s0); w.f64(park.lat0); }
}

function writePlayer(w, P, view) {
  const v = P.v, ph = P.phys;
  w.f64(v.x); w.f64(v.y); w.f64(v.z); w.f64(v.yaw);
  w.f64(v.vx); w.f64(v.vy); w.f64(v.vz); w.f64(v.yawRate);
  w.f64(v.s); w.f64(v.lat); w.f64(v.speed); w.f64(v.steerAngle);
  w.bool(v.onGround); w.f64(v.visualYaw);
  w.i32(ph.gear); w.f64(ph.rpm); w.f64(ph.shiftTimer); w.f64(ph.nitro); w.bool(ph.nitroActive);
  w.bool(ph.drifting); w.f64(ph.driftTime); w.f64(ph.slip); w.f64(ph.skid); w.f64(ph.scrape);
  w.i32(ph.scrapeSide ?? 0); w.f64(ph.airTime); w.bool(ph.locked); w.f64(ph.boost);
  w.f64(ph.powerOut); w.f64(ph.regen); w.f64(ph.damage); w.f64(ph.spiked); w.f64(ph.offTrack ?? 0);
  const r = P.rule;
  w.bool(!!r);
  if (r) {
    w.opt(v.prog); w.opt(r.lastS); w.opt(r.odo); w.f64(r.dist);
    w.i32(r.lap); w.f64(r.lapStart); w.i32(r.lapTimes.length); w.opt(r.lapTimes.at(-1));
    w.f64(r.score); w.f64(r.mult); w.f64(r.multTimer); w.f64(r.topSpeed); w.i32(r.nearMisses);
    w.f64(r.bonusCooldown); w.f64(r.resetCooldown); w.f64(r.wrongWay); w.opt(r.stuck);
    w.f64(r.lastDrift); w.f64(r.finishDelay); w.bool(r.playerFinished); w.opt(r.playerTime);
    w.bool(r.reported); w.opt(r.passTimer); w.opt(r.passLat);
    writePark(w, r.park);
    const rows = r.parkRows ?? [];
    w.i32(rows.length); for (const n of rows) w.i32(n);
  }
  const pv = P.pv;
  w.bool(!!pv);
  if (pv) { w.f64(pv.damage); w.i32(pv.wrecks); w.f64(pv.penalty); w.f64(pv.t); }
}

// Every body a target or a prop list can point at, by pool and index:
// player 0..99, rival 100+i, traffic 200+i, unit 300+i (units then block
// cars, one list), sawhorse 500+i.
function bodyRefs(view) {
  const m = new Map();
  view.players.forEach((P, i) => { m.set(P.body ?? P.v, i); });
  view.rivals.forEach((a, i) => m.set(a, 100 + i));
  (view.traffic?.cars ?? []).forEach((c, i) => m.set(c, 200 + i));
  if (view.pursuit) {
    [...view.pursuit.units, ...view.pursuit.blockCars].forEach((u, i) => m.set(u, 300 + i));
    view.pursuit.sawhorses.forEach((b, i) => m.set(b, 500 + i));
  }
  return m;
}

// The view of a running game's Race.
export function fromRace(race, tick, streams, input = null) {
  return {
    tick,
    race: { state: race.state, time: race.time, countdown: race.countdown, throttleAt: race.throttleAt },
    players: [{ v: race.player, phys: race.phys, rule: race, pv: race.pv, body: race.playerBody, input }],
    rivals: race.ais,
    traffic: race.traffic,
    pursuit: race.pv?.pursuit ?? null,
    pv: race.pv ?? null,
    passed: race.passed,
    nearMiss: race.nearMiss,
    streams,
  };
}

// ── Trace files ──────────────────────────────────────────────────────────
// Layout (little-endian): 'MRTRACE\0', u32 format version, u32 length of the
// JSON meta, the meta (UTF-8), u32 tick count N, u32 full-record interval K,
// N × u64 hashes (FNV-1a 64 of each tick's record, as lo then hi u32),
// u32 count of full records M, then M × (u32 tick, u32 byte length, bytes).
// A full record is kept for every tick that is a multiple of K, and for the
// last tick.
export class TraceWriter {
  constructor(meta, interval = 120) { this.meta = meta; this.interval = interval; this.hashes = []; this.full = []; this.last = null; }
  add(tick, bytes) {
    if (tick !== this.hashes.length + 1) throw new Error(`trace: tick ${tick} after ${this.hashes.length}`);
    this.hashes.push(fnv1a64(bytes));
    if (tick % this.interval === 0) this.full.push([tick, bytes]);
    this.last = [tick, bytes];
  }
  finish() {
    if (this.last && this.full.at(-1)?.[0] !== this.last[0]) this.full.push(this.last);
    const meta = new TextEncoder().encode(JSON.stringify(this.meta));
    let size = 8 + 4 + 4 + meta.length + 8 + this.hashes.length * 8 + 4;
    for (const [, b] of this.full) size += 8 + b.length;
    const out = new Uint8Array(size), dv = new DataView(out.buffer);
    out.set(new TextEncoder().encode('MRTRACE\0'), 0);
    let n = 8;
    dv.setUint32(n, TRACE_VERSION, true); n += 4;
    dv.setUint32(n, meta.length, true); n += 4;
    out.set(meta, n); n += meta.length;
    dv.setUint32(n, this.hashes.length, true); n += 4;
    dv.setUint32(n, this.interval, true); n += 4;
    for (const [lo, hi] of this.hashes) { dv.setUint32(n, lo, true); dv.setUint32(n + 4, hi, true); n += 8; }
    dv.setUint32(n, this.full.length, true); n += 4;
    for (const [tick, b] of this.full) { dv.setUint32(n, tick, true); dv.setUint32(n + 4, b.length, true); n += 8; out.set(b, n); n += b.length; }
    return out;
  }
}

export function readTrace(bytes) {
  const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (new TextDecoder().decode(bytes.subarray(0, 8)) !== 'MRTRACE\0') throw new Error('not a trace file');
  let n = 8;
  const version = dv.getUint32(n, true); n += 4;
  const ml = dv.getUint32(n, true); n += 4;
  const meta = JSON.parse(new TextDecoder().decode(bytes.subarray(n, n + ml))); n += ml;
  const N = dv.getUint32(n, true); n += 4;
  const interval = dv.getUint32(n, true); n += 4;
  const hashes = [];
  for (let i = 0; i < N; i++) { hashes.push([dv.getUint32(n, true), dv.getUint32(n + 4, true)]); n += 8; }
  const M = dv.getUint32(n, true); n += 4;
  const full = [];
  for (let i = 0; i < M; i++) { const tick = dv.getUint32(n, true), len = dv.getUint32(n + 4, true); n += 8; full.push([tick, bytes.subarray(n, n + len)]); n += len; }
  return { version, meta, interval, hashes, full };
}

// ── Reading a record back into named fields ─────────────────────────────
// Mirrors traceRecord field for field (and trace-format.md), so two
// records can be diffed by name: decodeRecord(bytes) → nested object, and
// flatten() → [[path, value]] in record order. Missing values decode as
// null; NaN as NaN.

class Reader {
  constructor(bytes) { this.dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength); this.n = 0; this.len = bytes.byteLength; }
  f64() { const v = this.dv.getFloat64(this.n, true); this.n += 8; return v; }
  opt() { const lo = this.dv.getUint32(this.n, true), hi = this.dv.getUint32(this.n + 4, true); if (lo === MISSING_LO && hi === MISSING_HI) { this.n += 8; return null; } return this.f64(); }
  i32() { const v = this.dv.getInt32(this.n, true); this.n += 4; return v; }
  bool() { return this.i32() !== 0; }
  e(table) { const c = this.i32(); return c < 0 ? null : ENUMS[table][c] ?? `?${c}`; }
  fields(o, spec) { for (const [name, kind] of spec) o[name] = kind === 'f' ? this.f64() : kind === 'o' ? this.opt() : kind === 'i' ? this.i32() : kind === 'b' ? this.bool() : this.e(kind); return o; }
}
const F = (names, kind = 'f') => names.split(' ').map((n) => [n, kind]);
const KIN = [...F('s lat speed latVel'), ['dir', 'i'], ...F('spin spinRate stunned x y z yaw visualYaw vx vz')];
function readPark(r) { const k = r.i32(); if (k < 0) return null; const p = { kind: ENUMS.parkKind[k] }; if (p.kind === 'lane') r.fields(p, F('stopAt laneLat s0 lat0')); return p; }

export function decodeRecord(bytes) {
  const r = new Reader(bytes), out = {};
  out.tick = r.i32();
  const hasRace = r.bool(), hasPursuit = r.bool();
  if (hasRace) out.race = r.fields({}, [['state', 'raceState'], ...F('time countdown'), ['throttleAt', 'o']]);
  const [np, nRivals, nTraffic, nPolice, nSaw] = [r.i32(), r.i32(), r.i32(), r.i32(), r.i32()];
  out.inputs = Array.from({ length: np }, () => r.fields({}, [['steer', 'i'], ['throttle', 'i'], ['brake', 'i'], ['flags', 'i']]));
  out.players = [];
  for (let i = 0; i < np; i++) {
    const p = r.fields({}, [...F('x y z yaw vx vy vz yawRate s lat speed steerAngle'), ['onGround', 'b'], ['visualYaw', 'f'], ['gear', 'i'], ...F('rpm shiftTimer nitro'), ['nitroActive', 'b'], ['drifting', 'b'], ...F('driftTime slip skid scrape'), ['scrapeSide', 'i'], ['airTime', 'f'], ['locked', 'b'], ...F('boost powerOut regen damage spiked offTrack')]);
    if (r.bool()) {
      p.rules = r.fields({}, [...F('prog lastS odo', 'o'), ['dist', 'f'], ['lap', 'i'], ['lapStart', 'f'], ['lapCount', 'i'], ['lastLap', 'o'], ...F('score mult multTimer topSpeed'), ['nearMisses', 'i'], ...F('bonusCooldown resetCooldown wrongWay'), ['stuck', 'o'], ...F('lastDrift finishDelay'), ['finished', 'b'], ['finishTime', 'o'], ['reported', 'b'], ...F('passTimer passLat', 'o')]);
      p.rules.park = readPark(r);
      const n = r.i32(); p.rules.parkRows = Array.from({ length: n }, () => r.i32());
    }
    if (r.bool()) p.pursuitView = r.fields({}, [['damage', 'f'], ['wrecks', 'i'], ...F('penalty t')]);
    out.players.push(p);
  }
  out.rivals = [];
  for (let i = 0; i < nRivals; i++) {
    const a = r.fields({}, [...KIN, ...F('avoid avoidTimer nitro nitroTimer'), ['finished', 'b'], ['finishTime', 'o'], ['throttle', 'f'], ['nitroActive', 'b'], ['hold', 'f'], ['holdLat', 'o'], ['spiked', 'f'], ['prog', 'o']]);
    a.park = readPark(r);
    if (hasPursuit) a.lastHit = r.opt();
    out.rivals.push(a);
  }
  if (r.bool()) out.trafficState = r.fields({}, [...F('nextSpawnS nextOppS'), ['maxActive', 'i']]);
  out.traffic = [];
  for (let i = 0; i < nTraffic; i++) {
    const c = r.fields({}, [['active', 'b'], ...KIN, ...F('crashed cruise laneLat'), ['lane', 'i'], ['passed', 'o'], ['nearMissHit', 'b']]);
    if (hasPursuit) c.lastHit = r.opt();
    out.traffic.push(c);
  }
  if (hasPursuit) {
    const p = out.pursuit = r.fields({}, [['state', 'pursuitState'], ['heat', 'i'], ['maxHeat', 'i'], ...F('heatMeter bust evade time spawnT propT'), ['patrolT', 'o'], ['takedowns', 'i'], ['busts', 'i']]);
    if (r.bool()) { p.roadblock = r.fields({}, [...F('s gapLat'), ['heavy', 'b'], ['passed', 'b'], ['touched', 'b']]); const n = r.i32(); p.roadblock.cars = Array.from({ length: n }, () => r.i32()); }
    if (r.bool()) p.spikes = r.fields({}, [...F('s lat0 lat1'), ['car', 'i'], ['passed', 'b'], ['hit', 'i']]);
    const ns = r.i32(); p.spotsUsed = Array.from({ length: ns }, () => r.bool());
    const nr = r.i32(); p.racers = Array.from({ length: nr }, () => r.fields({}, [...F('bust hold holdTotal'), ['holdReason', 'holdReason'], ['grace', 'f'], ['finished', 'b'], ['prevS', 'o']]));
    p.police = Array.from({ length: nPolice }, () => r.fields({}, [['active', 'b'], ...KIN, ['mode', 'mode'], ['behaviour', 'behaviour'], ...F('behT modeT'), ['slot', 'slot'], ['pitSide', 'i'], ...F('pitCooldown pitPush'), ['target', 'i'], ...F('lastSeenS health disabledT parkLat blockYaw'), ['gapLat', 'o'], ...F('laneLat cap weave'), ['retarget', 'o'], ['uturned', 'b'], ...F('avoid avoidTimer'), ['siren', 'siren'], ['lastHit', 'o']]));
    p.sawhorses = Array.from({ length: nSaw }, () => r.fields({}, [['active', 'b'], ['broken', 'b'], ...KIN, ...F('h vy age'), ...F('gapLat lastHit', 'o')]));
  }
  out.streams = r.fields({}, F('ai police pursuit traffic', 'i'));
  if (r.n !== r.len) throw new Error(`decodeRecord: ${r.len - r.n} bytes left over`);
  return out;
}

export function flatten(o, prefix = '', out = []) {
  if (o === null || typeof o !== 'object') { out.push([prefix, o]); return out; }
  for (const [k, v] of Object.entries(o)) flatten(v, prefix ? `${prefix}.${k}` : k, out);
  return out;
}
