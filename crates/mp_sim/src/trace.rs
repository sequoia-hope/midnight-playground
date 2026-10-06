//! The trace record (SPEC 4.6; `parity/trace-format.md` is the definition,
//! `tools/parity/lib/trace.mjs` the JS writer). Both sides write the same
//! bytes for the same state, so a tick's record, or its FNV-1a 64 hash, says
//! whether the two agree. The order of fields below IS the format.
//!
//! The record is built from views of the state ([`View`]), so it can follow
//! the JS layout whatever shape the Rust state takes.

use crate::ai::AiDriver;
use crate::body::BodyId;
use crate::input::Input;
use crate::kinematic::Kinematic;
use crate::park::Park;
use crate::physics::CarPhysics;
use crate::police::Mode;
use crate::pursuit::Pursuit;
use crate::race::{PlayerRules, RaceStateKind, SimState};
use crate::traffic::{Traffic, TrafficCar};
use crate::vehicle::Vehicle;

pub const TRACE_VERSION: u32 = 1;
/// "Missing": a NaN with a payload no arithmetic produces.
pub const MISSING: u64 = 0x7FF8_0000_0000_0D1E;
pub const I32_NONE: i32 = i32::MIN;

/// A record under construction.
#[derive(Default)]
pub struct Writer {
    pub buf: Vec<u8>,
}

impl Writer {
    pub fn f64(&mut self, x: f64) {
        self.buf.extend_from_slice(&x.to_le_bytes());
    }
    pub fn opt(&mut self, x: Option<f64>) {
        match x {
            Some(v) => self.f64(v),
            None => self.buf.extend_from_slice(&MISSING.to_le_bytes()),
        }
    }
    pub fn i32(&mut self, x: i32) {
        self.buf.extend_from_slice(&x.to_le_bytes());
    }
    pub fn bool(&mut self, x: bool) {
        self.i32(i32::from(x));
    }
}

/// FNV-1a 64 over the record's bytes.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// The input that drove a tick, quantised as `InputFrame` is (`writeInput`).
pub fn write_input(w: &mut Writer, inp: Option<&Input>) {
    let Some(inp) = inp else {
        w.i32(I32_NONE);
        w.i32(0);
        w.i32(0);
        w.i32(0);
        return;
    };
    let c = |x: f64, lo: f64, hi: f64| {
        if x < lo {
            lo
        } else if x > hi {
            hi
        } else {
            x
        }
    };
    w.i32(mp_math::js::round(c(inp.steer, -1.0, 1.0) * 32767.0) as i32);
    w.i32(mp_math::js::round(c(inp.throttle, 0.0, 1.0) * 255.0) as i32);
    w.i32(mp_math::js::round(c(inp.brake, 0.0, 1.0) * 255.0) as i32);
    w.i32(i32::from(inp.handbrake) | (i32::from(inp.nitro) << 1) | (i32::from(inp.analog) << 2));
}

/// A player's vehicle and physics, and (when there is a Race) its rule
/// state: what `writePlayer` reads.
pub struct PlayerView<'a> {
    pub v: &'a Vehicle,
    pub phys: &'a CarPhysics,
    pub input: Option<&'a Input>,
    /// Race's rule state for this player, and the race's parking rows.
    pub rules: Option<(&'a PlayerRules, Option<&'a [i32]>)>,
    /// PursuitView's damage, wrecks, penalty and clock.
    pub pursuit_view: Option<(f64, i32, f64, f64)>,
}

/// The race header (`race.state`, `time`, `countdown`, `throttleAt`).
pub struct RaceHeader {
    pub state: i32,
    pub time: f64,
    pub countdown: f64,
    pub throttle_at: Option<f64>,
}

/// Everything a record is written from. Pieces that later work packages
/// add (race, pursuit) are optional.
pub struct View<'a> {
    pub tick: i32,
    pub race: Option<RaceHeader>,
    pub players: Vec<PlayerView<'a>>,
    pub rivals: &'a [AiDriver],
    pub traffic: Option<&'a Traffic>,
    pub pursuit: Option<&'a Pursuit>,
    /// PursuitView's last damaging hit by each body (its clock), when there
    /// is a PursuitView.
    pub last_hit: Option<&'a LastHit>,
    /// Draws so far from ai, police, pursuit, traffic; -1 for a stream that
    /// is not counted.
    pub streams: [i32; 4],
}

pub fn write_player(w: &mut Writer, p: &PlayerView) {
    let (v, ph) = (p.v, p.phys);
    for x in [
        v.x,
        v.y,
        v.z,
        v.yaw,
        v.vx,
        v.vy,
        v.vz,
        v.yaw_rate,
        v.s,
        v.lat,
        v.speed,
        v.steer_angle,
    ] {
        w.f64(x);
    }
    w.bool(v.on_ground);
    w.f64(v.visual_yaw);
    w.i32(ph.gear);
    w.f64(ph.rpm);
    w.f64(ph.shift_timer);
    w.f64(ph.nitro);
    w.bool(ph.nitro_active);
    w.bool(ph.drifting);
    w.f64(ph.drift_time);
    w.f64(ph.slip);
    w.f64(ph.skid);
    w.f64(ph.scrape);
    w.i32(ph.scrape_side.unwrap_or(0));
    w.f64(ph.air_time);
    w.bool(ph.locked);
    w.f64(ph.boost);
    w.f64(ph.power_out);
    w.f64(ph.regen);
    w.f64(ph.damage);
    w.f64(ph.spiked);
    w.f64(ph.off_track.unwrap_or(0.0));
    match p.rules {
        Some((r, rows)) => {
            w.bool(true);
            w.opt(v.prog);
            w.opt(r.last_s);
            w.opt(r.odo);
            w.f64(r.dist);
            w.i32(r.lap);
            w.f64(r.lap_start);
            w.i32(r.lap_times.len() as i32);
            w.opt(r.lap_times.last().copied());
            for x in [r.score, r.mult, r.mult_timer, r.top_speed] {
                w.f64(x);
            }
            w.i32(r.near_misses);
            for x in [r.bonus_cooldown, r.reset_cooldown, r.wrong_way] {
                w.f64(x);
            }
            w.opt(r.stuck);
            w.f64(r.last_drift);
            w.f64(r.finish_delay);
            w.bool(r.finished);
            w.opt(r.finish_time);
            w.bool(r.reported);
            w.opt(r.pass_timer);
            w.opt(r.pass_lat);
            write_park(w, r.park.as_ref());
            let rows = rows.unwrap_or(&[]);
            w.i32(rows.len() as i32);
            for n in rows {
                w.i32(*n);
            }
        }
        None => w.bool(false),
    }
    match p.pursuit_view {
        Some((damage, wrecks, penalty, t)) => {
            w.bool(true);
            w.f64(damage);
            w.i32(wrecks);
            w.f64(penalty);
            w.f64(t);
        }
        None => w.bool(false),
    }
}

fn mode_code(m: Mode) -> i32 {
    m as i32
}

/// The pursuit section (trace-format.md, "if hasPursuit").
pub fn write_pursuit(
    w: &mut Writer,
    pu: &Pursuit,
    last_hit_police: impl Fn(usize) -> Option<f64>,
    last_hit_saw: impl Fn(usize) -> Option<f64>,
) {
    w.i32(pu.state as i32);
    w.i32(pu.heat);
    w.i32(pu.max_heat);
    for x in [
        pu.heat_meter,
        pu.bust,
        pu.evade,
        pu.time,
        pu.spawn_t,
        pu.prop_t,
    ] {
        w.f64(x);
    }
    w.opt(pu.patrol_t);
    w.i32(pu.takedowns);
    w.i32(pu.busts);
    let n = pu.units.len();
    let racer_ref = |ri: usize| pu.racers[ri].id.trace_ref();
    match &pu.roadblock {
        Some(rb) => {
            w.bool(true);
            w.f64(rb.s);
            w.f64(rb.gap_lat);
            w.bool(rb.heavy);
            w.bool(rb.passed);
            w.bool(rb.touched);
            w.i32(rb.cars.len() as i32);
            for &c in &rb.cars {
                w.i32(BodyId::Police(n + c).trace_ref());
            }
        }
        None => w.bool(false),
    }
    match &pu.spikes {
        Some(sp) => {
            w.bool(true);
            w.f64(sp.s);
            w.f64(sp.lat0);
            w.f64(sp.lat1);
            w.i32(BodyId::Police(n + sp.car).trace_ref());
            w.bool(sp.passed);
            let mut mask = 0i32;
            for (i, &h) in sp.hit.iter().enumerate() {
                if h {
                    mask |= 1 << i;
                }
            }
            w.i32(mask);
        }
        None => w.bool(false),
    }
    w.i32(pu.spots.len() as i32);
    for s in &pu.spots {
        w.bool(s.used);
    }
    w.i32(pu.racers.len() as i32);
    for r in &pu.racers {
        w.f64(r.bust);
        w.f64(r.hold);
        w.f64(r.hold_total);
        w.i32(r.hold_reason.map_or(-1, |h| h as i32));
        w.f64(r.grace);
        w.bool(r.finished);
        w.opt(r.prev_s);
    }
    for i in 0..n + pu.block_cars.len() {
        let u = pu.police(i);
        w.bool(u.active);
        write_kinematic(w, &u.k);
        w.i32(mode_code(u.mode));
        w.i32(u.behaviour as i32);
        w.f64(u.beh_t);
        w.f64(u.mode_t);
        w.i32(u.slot.map_or(-1, |s| s as i32));
        w.i32(u.pit_side);
        w.f64(u.pit_cooldown);
        w.f64(u.pit_push.unwrap_or(0.0));
        w.i32(u.target.map_or(-1, racer_ref));
        for x in [
            u.last_seen_s,
            u.health,
            u.disabled_t,
            u.park_lat,
            u.block_yaw,
        ] {
            w.f64(x);
        }
        w.opt(u.gap_lat);
        w.f64(u.lane_lat);
        w.f64(u.cap);
        w.f64(u.weave);
        w.opt(u.retarget);
        w.bool(u.uturned);
        w.f64(u.avoid.unwrap_or(0.0));
        w.f64(u.avoid_timer.unwrap_or(0.0));
        w.i32(u.siren as i32);
        w.opt(last_hit_police(i));
    }
    for (i, b) in pu.sawhorses.iter().enumerate() {
        w.bool(b.active);
        w.bool(b.broken);
        write_kinematic(w, &b.k);
        w.f64(b.h);
        w.f64(b.vy);
        w.f64(b.age);
        w.opt(b.gap_lat);
        w.opt(last_hit_saw(i));
    }
}

/// PursuitView's last damaging hit per body, by pool index (`lastHit`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LastHit {
    pub rivals: Vec<Option<f64>>,
    pub traffic: Vec<Option<f64>>,
    pub police: Vec<Option<f64>>,
    pub sawhorses: Vec<Option<f64>>,
}

/// Track coordinates and the world pose a kinematic car writes.
pub fn write_kinematic(w: &mut Writer, k: &Kinematic) {
    w.f64(k.s);
    w.f64(k.lat);
    w.f64(k.speed);
    w.f64(k.lat_vel);
    w.i32(k.dir);
    w.f64(k.spin);
    w.f64(k.spin_rate);
    w.f64(k.stunned);
    let v = &k.v;
    for x in [v.x, v.y, v.z, v.yaw, v.visual_yaw, v.vx, v.vz] {
        w.f64(x);
    }
}

pub fn write_park(w: &mut Writer, park: Option<&Park>) {
    match park {
        None => w.i32(-1),
        Some(Park::Circuit) => w.i32(0),
        Some(Park::Lane {
            stop_at,
            lane_lat,
            s0,
            lat0,
        }) => {
            w.i32(1);
            for x in [*stop_at, *lane_lat, *s0, *lat0] {
                w.f64(x);
            }
        }
    }
}

pub fn write_rival(w: &mut Writer, a: &AiDriver) {
    write_kinematic(w, &a.k);
    w.f64(a.avoid);
    w.f64(a.avoid_timer);
    w.f64(a.nitro);
    w.f64(a.nitro_timer);
    w.bool(a.finished);
    w.opt(a.finish_time);
    w.f64(a.throttle);
    w.bool(a.nitro_active);
    w.f64(a.hold);
    w.opt(a.hold_lat);
    w.f64(a.spiked);
    w.opt(a.prog);
    write_park(w, a.park.as_ref());
}

/// `passed` and `near_miss_hit` are Race's, about this car (None without a
/// Race).
pub fn write_traffic_car(w: &mut Writer, c: &TrafficCar, passed: Option<f64>, near_miss_hit: bool) {
    w.bool(c.active);
    write_kinematic(w, &c.k);
    w.f64(c.crashed);
    w.f64(c.cruise);
    w.f64(c.lane_lat);
    w.i32(c.lane.unwrap_or(-1));
    w.opt(passed);
    w.bool(near_miss_hit);
}

/// The record of one tick.
pub fn trace_record(view: &View) -> Vec<u8> {
    let mut w = Writer::default();
    let tcars: &[TrafficCar] = view.traffic.map_or(&[], |t| &t.cars);
    w.i32(view.tick);
    w.bool(view.race.is_some());
    w.bool(view.pursuit.is_some());
    if let Some(r) = &view.race {
        w.i32(r.state);
        w.f64(r.time);
        w.f64(r.countdown);
        w.opt(r.throttle_at);
    }
    w.i32(view.players.len() as i32);
    w.i32(view.rivals.len() as i32);
    w.i32(tcars.len() as i32);
    w.i32(
        view.pursuit
            .map_or(0, |p| (p.units.len() + p.block_cars.len()) as i32),
    );
    w.i32(view.pursuit.map_or(0, |p| p.sawhorses.len() as i32));
    for p in &view.players {
        write_input(&mut w, p.input);
    }
    for p in &view.players {
        write_player(&mut w, p);
    }
    let has_pu = view.pursuit.is_some();
    let lh =
        |pool: fn(&LastHit) -> &Vec<Option<f64>>, i: usize| view.last_hit.and_then(|l| pool(l)[i]);
    for (i, a) in view.rivals.iter().enumerate() {
        write_rival(&mut w, a);
        if has_pu {
            w.opt(lh(|l| &l.rivals, i));
        }
    }
    match view.traffic {
        Some(t) => {
            w.bool(true);
            w.f64(t.next_spawn_s);
            w.f64(t.next_opp_s);
            w.i32(t.max_active as i32);
        }
        None => w.bool(false),
    }
    let rules = view.players.first().and_then(|p| p.rules.map(|r| r.0));
    for (i, c) in tcars.iter().enumerate() {
        let (passed, hit) = match rules {
            Some(r) => (r.passed[i], r.near_miss_hit[i]),
            None => (None, false),
        };
        write_traffic_car(&mut w, c, passed, hit);
        if has_pu {
            w.opt(lh(|l| &l.traffic, i));
        }
    }
    if let Some(pu) = view.pursuit {
        write_pursuit(
            &mut w,
            pu,
            |i| lh(|l| &l.police, i),
            |i| lh(|l| &l.sawhorses, i),
        );
    }
    for s in view.streams {
        w.i32(s);
    }
    w.buf
}

/// A trace file (`TraceWriter` in trace.mjs): the hash of every tick and
/// the full record every `interval` ticks and at the last.
pub struct TraceFile {
    pub meta: String,
    pub interval: u32,
    pub hashes: Vec<u64>,
    pub full: Vec<(u32, Vec<u8>)>,
    last: Option<(u32, Vec<u8>)>,
}

impl TraceFile {
    pub fn new(meta: String) -> TraceFile {
        TraceFile {
            meta,
            interval: 120,
            hashes: Vec::new(),
            full: Vec::new(),
            last: None,
        }
    }

    pub fn add(&mut self, tick: u32, bytes: Vec<u8>) {
        assert_eq!(
            tick as usize,
            self.hashes.len() + 1,
            "trace: tick out of order"
        );
        self.hashes.push(fnv1a64(&bytes));
        if tick.is_multiple_of(self.interval) {
            self.full.push((tick, bytes.clone()));
        }
        self.last = Some((tick, bytes));
    }

    pub fn finish(mut self) -> Vec<u8> {
        if let Some((t, b)) = self.last.take()
            && self.full.last().map(|f| f.0) != Some(t)
        {
            self.full.push((t, b));
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"MRTRACE\0");
        out.extend_from_slice(&TRACE_VERSION.to_le_bytes());
        out.extend_from_slice(&(self.meta.len() as u32).to_le_bytes());
        out.extend_from_slice(self.meta.as_bytes());
        out.extend_from_slice(&(self.hashes.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.interval.to_le_bytes());
        for h in &self.hashes {
            out.extend_from_slice(&(*h as u32).to_le_bytes());
            out.extend_from_slice(&((*h >> 32) as u32).to_le_bytes());
        }
        out.extend_from_slice(&(self.full.len() as u32).to_le_bytes());
        for (tick, b) in &self.full {
            out.extend_from_slice(&tick.to_le_bytes());
            out.extend_from_slice(&(b.len() as u32).to_le_bytes());
            out.extend_from_slice(b);
        }
        out
    }
}

/// A trace file read back: its hashes and full records.
pub struct ReadTrace {
    pub version: u32,
    pub meta: String,
    pub interval: u32,
    pub hashes: Vec<u64>,
    pub full: Vec<(u32, Vec<u8>)>,
}

pub fn read_trace(b: &[u8]) -> Result<ReadTrace, String> {
    let mut p = 0usize;
    let mut take = |n: usize| -> Result<&[u8], String> {
        let s = b.get(p..p + n).ok_or("trace: file ends early")?;
        p += n;
        Ok(s)
    };
    if take(8)? != b"MRTRACE\0" {
        return Err("not a trace file".into());
    }
    let u32le = |s: &[u8]| u32::from_le_bytes([s[0], s[1], s[2], s[3]]);
    let version = u32le(take(4)?);
    let ml = u32le(take(4)?) as usize;
    let meta = String::from_utf8_lossy(take(ml)?).into_owned();
    let n = u32le(take(4)?) as usize;
    let interval = u32le(take(4)?);
    let mut hashes = Vec::with_capacity(n);
    for _ in 0..n {
        let s = take(8)?;
        hashes.push(u32le(&s[..4]) as u64 | (u32le(&s[4..]) as u64) << 32);
    }
    let m = u32le(take(4)?) as usize;
    let mut full = Vec::with_capacity(m);
    for _ in 0..m {
        let tick = u32le(take(4)?);
        let len = u32le(take(4)?) as usize;
        full.push((tick, take(len)?.to_vec()));
    }
    Ok(ReadTrace {
        version,
        meta,
        interval,
        hashes,
        full,
    })
}

/// The record of a race state after a tick (the game oracle's `fromRace`).
pub fn race_record(st: &SimState, inputs: &[Input]) -> Vec<u8> {
    let r = &st.race;
    let players = st
        .players
        .iter()
        .enumerate()
        .map(|(i, p)| PlayerView {
            v: &p.v,
            phys: &p.phys,
            input: inputs.get(i),
            rules: Some((&p.rules, r.park_rows.as_deref())),
            pursuit_view: st
                .pv
                .as_ref()
                .map(|pv| (pv.damage, pv.wrecks, pv.penalty, pv.t)),
        })
        .collect();
    trace_record(&View {
        tick: st.tick as i32,
        race: Some(RaceHeader {
            state: match r.state {
                RaceStateKind::Countdown => 0,
                RaceStateKind::Racing => 1,
                RaceStateKind::Finished => 2,
            },
            time: r.time,
            countdown: r.countdown,
            throttle_at: r.throttle_at,
        }),
        players,
        rivals: &st.rivals,
        traffic: Some(&st.traffic),
        pursuit: st.pv.as_ref().map(|pv| &pv.pursuit),
        last_hit: st.pv.as_ref().map(|pv| &pv.last_hit),
        streams: st.rng.draws(),
    })
}
