//! The trace record (SPEC 4.6; `parity/trace-format.md` is the definition,
//! `tools/parity/lib/trace.mjs` the JS writer). Both sides write the same
//! bytes for the same state, so a tick's record, or its FNV-1a 64 hash, says
//! whether the two agree. The order of fields below IS the format.
//!
//! The record is built from views of the state ([`View`]), so it can follow
//! the JS layout whatever shape the Rust state takes.

use crate::input::Input;
use crate::physics::CarPhysics;
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
    w.i32(mr_math::js::round(c(inp.steer, -1.0, 1.0) * 32767.0) as i32);
    w.i32(mr_math::js::round(c(inp.throttle, 0.0, 1.0) * 255.0) as i32);
    w.i32(mr_math::js::round(c(inp.brake, 0.0, 1.0) * 255.0) as i32);
    w.i32(i32::from(inp.handbrake) | (i32::from(inp.nitro) << 1) | (i32::from(inp.analog) << 2));
}

/// A player's vehicle and physics, and (when there is a Race) its rule
/// state: what `writePlayer` reads.
pub struct PlayerView<'a> {
    pub v: &'a Vehicle,
    pub phys: &'a CarPhysics,
    pub input: Option<&'a Input>,
}

/// Everything a record is written from. Pieces that later work packages
/// add (race, rivals, traffic, pursuit) are optional.
pub struct View<'a> {
    pub tick: i32,
    pub players: Vec<PlayerView<'a>>,
    pub n_rivals: i32,
    pub n_traffic: i32,
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
    // Race rule state and PursuitView: not yet (WP 1.5, 1.6).
    w.bool(false);
    w.bool(false);
}

/// The record of one tick.
pub fn trace_record(view: &View) -> Vec<u8> {
    let mut w = Writer::default();
    w.i32(view.tick);
    w.bool(false); // hasRace
    w.bool(false); // hasPursuit
    w.i32(view.players.len() as i32);
    w.i32(view.n_rivals);
    w.i32(view.n_traffic);
    w.i32(0);
    w.i32(0);
    for p in &view.players {
        write_input(&mut w, p.input);
    }
    for p in &view.players {
        write_player(&mut w, p);
    }
    w.bool(false); // hasTraffic
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
