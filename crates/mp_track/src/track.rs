//! Port of `src/track/Track.js`.
//!
//! The track is sampled every metre. Everything else — road mesh, terrain,
//! scenery placement, physics, AI — reads these arrays, so a level's route is
//! the single source of truth for its shape.
//!
//! Two kinds of route:
//!   point-to-point: a list of turtle segments (length, turn, climb)
//!   loop:           a closed polyline, resampled to 1 m; s wraps around
//!
//! Every per-sample array is a `Float32Array` (or `Uint8Array`) in the JS, so
//! it is `Vec<f32>` (`Vec<u8>`) here, and every store rounds to `f32` at the
//! same point (SPEC 4.2). Reads widen to `f64`, as JS reads do.

use std::collections::BTreeMap;

use mp_math::{DEG, clamp, js, kernel, smoothstep};

use crate::level::{GroundFn, Level, Route, Segment, Zone};
use crate::road_types::{ROAD_TYPES, road_index};

fn gaussian_smooth(src: &[f32], sigma: f64, wrap: bool) -> Vec<f32> {
    let r = (sigma * 3.0).ceil() as i64;
    let mut w = vec![0f32; (r * 2 + 1) as usize];
    let mut sum = 0.0;
    for k in -r..=r {
        let kk = (k * k) as f64;
        w[(k + r) as usize] = kernel::exp(-kk / (2.0 * sigma * sigma)) as f32;
        sum += w[(k + r) as usize] as f64;
    }
    for wk in w.iter_mut() {
        *wk = (*wk as f64 / sum) as f32;
    }
    let n = src.len() as i64;
    let mut out = vec![0f32; src.len()];
    for i in 0..n {
        let mut acc = 0.0;
        for k in -r..=r {
            let j = if wrap {
                ((i + k) % n + n) % n
            } else {
                (i + k).clamp(0, n - 1)
            };
            acc += src[j as usize] as f64 * w[(k + r) as usize] as f64;
        }
        out[i as usize] = acc as f32;
    }
    out
}

/// Circuits with surveyed run-off: metres past the tarmac edge before the
/// ground leaves the road's plane (see surfaceY).
pub const RUNOFF_FLAT: f64 = 1.5;

/// Smoothed trapezoid in [0,1]: ramps over `r` at each end. Integral = 1 - r.
pub fn trapezoid(t: f64, r: f64) -> f64 {
    if t < r {
        return smoothstep(0.0, r, t);
    }
    if t > 1.0 - r {
        return smoothstep(0.0, r, 1.0 - t);
    }
    1.0
}

/// A named feature along the route. `turn` is missing on a loop's ramp tags.
#[derive(Clone, Debug, PartialEq)]
pub struct Tag {
    pub tag: String,
    pub s0: f64,
    pub s1: f64,
    pub turn: Option<f64>,
}

/// A level zone with where it lies on this track.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackZone {
    pub zone: Zone,
    pub id: usize,
    pub s0: f64,
    pub s1: f64,
}

/// An opening in a roadside fence (driveways, side roads).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FenceGap {
    pub s0: f64,
    pub s1: f64,
    pub side: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bounds {
    pub min_x: f64,
    pub max_x: f64,
    pub min_z: f64,
    pub max_z: f64,
}

/// Interpolated centreline frame (`frame()`'s `out`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub fx: f64,
    pub fz: f64,
    pub rx: f64,
    pub rz: f64,
    pub hw: f64,
    pub bank: f64,
    pub grade: f64,
    pub kappa: f64,
    pub wall_l: f64,
    pub wall_r: f64,
    pub zone: u8,
    pub s: f64,
}

/// `project()`'s `{s, lat, i}`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Projection {
    pub s: f64,
    pub lat: f64,
    pub i: usize,
}

/// `distanceToRoad()`: `i` is -1 and `s` missing when no road is in range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoadDistance {
    pub d: f64,
    pub i: i64,
    pub lat: f64,
    pub s: Option<f64>,
}

/// What a route walk hands `build()`.
struct Raw {
    n: usize,
    px: Vec<f32>,
    pz: Vec<f32>,
    y_raw: Vec<f32>,
    kappa: Vec<f32>,
    zone: Vec<u8>,
    road_type: Vec<u8>,
    elev_raw: Vec<u8>,
    bank: Option<Vec<f32>>,
    hw: Option<Vec<f32>>,
    wall_l: Option<Vec<f32>>,
    wall_r: Option<Vec<f32>>,
    run_l: Option<Vec<f32>>,
    run_r: Option<Vec<f32>>,
}

#[derive(Clone)]
pub struct Track {
    pub level: Level,
    pub is_loop: bool,
    pub n: usize,
    pub length: f64,
    /// Laps of a circuit (a loop raced to a finish); 0 for everything else.
    pub laps: u32,
    pub px: Vec<f32>,
    pub py: Vec<f32>,
    pub pz: Vec<f32>,
    pub kappa: Vec<f32>,
    pub k_smooth: Vec<f32>,
    pub grade: Vec<f32>,
    pub zone: Vec<u8>,
    pub road_type: Vec<u8>,
    pub fx: Vec<f32>,
    pub fz: Vec<f32>,
    pub rx: Vec<f32>,
    pub rz: Vec<f32>,
    pub hw: Vec<f32>,
    pub margin: Vec<f32>,
    pub bank: Vec<f32>,
    /// Surveyed run-off (circuits): past the tarmac edge the ground leaves
    /// the road's plane at its own grade, rise per metre outward (see
    /// surfaceY). None elsewhere: the road's plane carries on.
    pub run_l: Option<Vec<f32>>,
    pub run_r: Option<Vec<f32>>,
    /// How loose the ground is at (x, z) off the tarmac: 0 paved (asphalt
    /// run-off, which drives like the road), 1 dirt or grass (slows the car).
    /// None: all of it is loose.
    pub loose_at: Option<GroundFn>,
    pub elevated: Vec<u8>,
    /// Collision limits either side of the centreline (positive distances).
    pub wall_l: Vec<f32>,
    pub wall_r: Vec<f32>,
    pub zone_start: Vec<usize>,
    pub zones: Vec<TrackZone>,
    pub finish_s: f64,
    pub start_s: f64,
    /// Metres of straight road past the last sample. Scenery that draws the
    /// road carrying on beyond the end sets this in plan(); cars can drive it.
    pub runout: f64,
    /// Openings in roadside fences (driveways, side roads). Scenery registers
    /// these in plan(), before the road is built.
    pub fence_gaps: Vec<FenceGap>,
    pub tags: Vec<Tag>,
    pub cell: f64,
    /// The spatial hash: cell → samples, in the JS's insertion order within
    /// a cell. Only looked up, never iterated.
    pub hash: BTreeMap<(i64, i64), Vec<usize>>,
    pub bounds: Bounds,
    pub racing_line: Vec<f32>,
    pub speed_profile: Vec<f32>,
}

impl Track {
    pub fn new(level: &Level) -> Result<Track, String> {
        let is_loop = level.is_loop();
        let mut tags = Vec::new();
        let raw = match &level.route {
            Route::Loop(spec) => build_loop(level, spec, &mut tags)?,
            Route::Segments(segs) => walk_segments(level, segs, &mut tags),
        };
        Ok(build(level, is_loop, raw, tags))
    }

    pub fn tag(&self, name: &str) -> Vec<&Tag> {
        self.tags.iter().filter(|t| t.tag == name).collect()
    }

    /// How far along a point-to-point road a car can go, runout included.
    pub fn road_end(&self) -> f64 {
        if self.is_loop {
            f64::INFINITY
        } else {
            self.length + self.runout
        }
    }

    // ── Queries ─────────────────────────────────────────────────────
    pub fn wrap(&self, s: f64) -> f64 {
        if !self.is_loop {
            return s;
        }
        let n = self.n as f64;
        ((s % n) + n) % n
    }

    /// Signed shortest distance from a to b along the road.
    pub fn ds(&self, a: f64, b: f64) -> f64 {
        let mut d = b - a;
        if self.is_loop {
            let n = self.n as f64;
            d = ((d % n) + n) % n;
            if d > n / 2.0 {
                d -= n;
            }
        }
        d
    }

    pub fn idx(&self, s: f64) -> usize {
        let n = self.n as f64;
        if self.is_loop {
            return (((js::round(s) % n) + n) % n) as usize;
        }
        clamp(js::round(s), 0.0, n - 1.0) as usize
    }

    /// Integer sample i and fraction t for arc length s: `[i, t, j, s]`.
    pub fn locate(&self, s: f64) -> (usize, f64, usize, f64) {
        let n = self.n as f64;
        if self.is_loop {
            let s = ((s % n) + n) % n;
            let i = s.floor();
            return (i as usize, s - i, (i as usize + 1) % self.n, s);
        }
        let s = clamp(s, 0.0, n - 1.001);
        let i = s.floor();
        (i as usize, s - i, i as usize + 1, s)
    }

    /// Interpolated centreline frame at arc length s.
    pub fn frame(&self, s: f64) -> Frame {
        let (i, t, j, sw) = self.locate(s);
        let l = |a: &[f32]| a[i] as f64 + (a[j] as f64 - a[i] as f64) * t;
        let mut out = Frame {
            x: l(&self.px),
            y: l(&self.py),
            z: l(&self.pz),
            ..Frame::default()
        };
        let fx = l(&self.fx);
        let fz = l(&self.fz);
        let len = js::or(kernel::hypot(fx, fz), 1.0);
        out.fx = fx / len;
        out.fz = fz / len;
        out.rx = -out.fz;
        out.rz = out.fx;
        out.hw = l(&self.hw);
        out.bank = l(&self.bank);
        out.grade = l(&self.grade);
        out.kappa = l(&self.k_smooth);
        out.wall_l = l(&self.wall_l);
        out.wall_r = l(&self.wall_r);
        out.zone = self.zone[i];
        out.s = sw;
        // Past the last sample the road carries straight on (the runout).
        if !self.is_loop && s > sw + 0.01 {
            let d = s - sw;
            out.x += out.fx * d;
            out.z += out.fz * d;
            out.s = s;
        }
        out
    }

    /// World position of a point at (s, lateral offset) on the road surface.
    /// The rest of the frame comes along, as `out` does in the JS.
    pub fn point_at(&self, s: f64, lat: f64) -> Frame {
        let mut f = self.frame(s);
        let x = f.x + f.rx * lat;
        let z = f.z + f.rz * lat;
        f.x = x;
        f.z = z;
        f.y = if self.run_l.is_some() && lat.abs() > f.hw + RUNOFF_FLAT {
            self.surface_y(s, lat)
        } else {
            f.y - lat * f.bank
        };
        f
    }

    pub fn surface_y(&self, s: f64, lat: f64) -> f64 {
        let (i, t, j, _) = self.locate(s);
        let y = self.py[i] as f64 + (self.py[j] as f64 - self.py[i] as f64) * t;
        let b = self.bank[i] as f64 + (self.bank[j] as f64 - self.bank[i] as f64) * t;
        if let (Some(run_l), Some(run_r)) = (&self.run_l, &self.run_r) {
            // The road's plane carries on RUNOFF_FLAT past the edge (the verge),
            // then the ground takes its own grade.
            let hw = self.hw[i] as f64 + (self.hw[j] as f64 - self.hw[i] as f64) * t + RUNOFF_FLAT;
            let a = lat.abs();
            if a > hw {
                let r = if lat > 0.0 { run_r } else { run_l };
                let e = if lat > 0.0 { hw } else { -hw };
                return y - e * b + (a - hw) * (r[i] as f64 + (r[j] as f64 - r[i] as f64) * t);
            }
        }
        y - lat * b
    }

    /// Local projection of (x,z) onto the centreline, searching near `hint`.
    pub fn project(&self, x: f64, z: f64, hint: f64) -> Projection {
        self.project_window(x, z, hint, 12)
    }

    pub fn project_window(&self, x: f64, z: f64, hint: f64, window: i64) -> Projection {
        let n = self.n as i64;
        let nf = self.n as f64;
        let mut best: i64 = -1;
        let mut bd = f64::INFINITY;
        let mut edge = false;
        let h = if self.is_loop {
            hint.floor()
        } else {
            clamp(hint.floor(), 0.0, nf - 1.0)
        };
        // h is integral; as an i64 the JS's double arithmetic on it is exact.
        let h = h as i64;
        if self.is_loop {
            for o in -window..=window {
                let k = (((h + o) % n) + n) % n;
                let dx = x - self.px[k as usize] as f64;
                let dz = z - self.pz[k as usize] as f64;
                let d = dx * dx + dz * dz;
                if d < bd {
                    bd = d;
                    best = k;
                    edge = o.abs() == window;
                }
            }
        } else {
            let lo = (h - window).max(0);
            let hi = (h + window).min(n - 1);
            for k in lo..=hi {
                let dx = x - self.px[k as usize] as f64;
                let dz = z - self.pz[k as usize] as f64;
                let d = dx * dx + dz * dz;
                if d < bd {
                    bd = d;
                    best = k;
                }
            }
            edge = (best == lo && lo > 0) || (best == hi && hi < n - 1);
        }
        // If the best is at the window's edge we may be far off; widen.
        if edge && window < 200 {
            return self.project_window(x, z, best as f64, window * 4);
        }
        let b = best as usize;
        let dx = x - self.px[b] as f64;
        let dz = z - self.pz[b] as f64;
        let along = dx * self.fx[b] as f64 + dz * self.fz[b] as f64;
        let bf = best as f64;
        Projection {
            s: if self.is_loop {
                self.wrap(bf + along)
            } else {
                clamp(bf + along, 0.0, nf - 1.0 + self.runout)
            },
            lat: dx * self.rx[b] as f64 + dz * self.rz[b] as f64,
            i: b,
        }
    }

    fn hash_key(&self, x: f64, z: f64) -> (i64, i64) {
        (
            (x / self.cell).floor() as i64,
            (z / self.cell).floor() as i64,
        )
    }

    /// Nearest centreline sample anywhere (radius-limited, default 96).
    /// Returns index or -1.
    pub fn nearest(&self, x: f64, z: f64, radius: f64) -> i64 {
        let c = self.cell;
        let rc = (radius / c).ceil() as i64;
        let (cx, cz) = self.hash_key(x, z);
        let mut best = -1i64;
        let mut bd = radius * radius;
        for a in -rc..=rc {
            for b in -rc..=rc {
                let Some(arr) = self.hash.get(&(cx + a, cz + b)) else {
                    continue;
                };
                for &k in arr {
                    let dx = x - self.px[k] as f64;
                    let dz = z - self.pz[k] as f64;
                    let d = dx * dx + dz * dz;
                    if d < bd {
                        bd = d;
                        best = k as i64;
                    }
                }
            }
        }
        best
    }

    /// Distance from (x,z) to the nearest bit of road, and which sample
    /// (radius default 96).
    pub fn distance_to_road(&self, x: f64, z: f64, radius: f64) -> RoadDistance {
        let k = self.nearest(x, z, radius);
        if k < 0 {
            return RoadDistance {
                d: f64::INFINITY,
                i: -1,
                lat: 0.0,
                s: None,
            };
        }
        let p = self.project_window(x, z, k as f64, 4);
        RoadDistance {
            d: p.lat.abs(),
            i: p.i as i64,
            lat: p.lat,
            s: Some(p.s),
        }
    }

    /// Blend weights across zones at s (one per zone, summing to 1; width
    /// default 250).
    pub fn zone_blend(&self, s: f64, width: f64) -> Vec<f64> {
        let zn = self.zone_start.len();
        let mut w = vec![0.0; zn];
        if zn == 1 {
            w[0] = 1.0;
            return w;
        }
        let mut prev = 1.0;
        for z in 0..zn {
            let t = if z + 1 < zn {
                let s1 = self.zone_start[z + 1] as f64;
                smoothstep(s1 - width, s1 + width, s)
            } else {
                0.0
            };
            w[z] = prev - t;
            prev = t;
        }
        w
    }
}

fn walk_segments(level: &Level, segs: &[Segment], tags: &mut Vec<Tag>) -> Raw {
    let total_len = js::round(segs.iter().fold(0.0, |a, s| a + s.len));
    let n = total_len as usize + 1;
    let mut px = vec![0f32; n];
    let mut pz = vec![0f32; n];
    let mut kappa = vec![0f32; n];
    let mut grade_raw = vec![0f32; n];
    let mut zone = vec![0u8; n];
    let mut road_type = vec![0u8; n];
    let mut elev_raw = vec![0u8; n];
    let mut x = level.start_x.unwrap_or(0.0);
    let mut z = level.start_z.unwrap_or(0.0);
    let mut h = js::or_opt(level.start_heading, 0.0) * DEG;
    let mut cur_zone = 0u8;
    let mut cur_road = segs[0].extra.road.unwrap_or("mountain");
    let mut i = 0usize;
    px[0] = x as f32;
    pz[0] = z as f32;
    for seg in segs {
        let (len, turn_deg, rise, extra) = (seg.len, seg.turn, seg.rise, &seg.extra);
        if let Some(zn) = extra.zone {
            cur_zone = zn;
        }
        if let Some(r) = extra.road {
            cur_road = r;
        }
        let s0 = i as f64;
        if let Some(tag) = extra.tag {
            tags.push(Tag {
                tag: tag.to_string(),
                s0,
                s1: s0 + len,
                turn: Some(turn_deg),
            });
        }
        let r = if turn_deg.abs() > 120.0 { 0.22 } else { 0.32 };
        let k_peak = (turn_deg * DEG) / (len * (1.0 - r));
        let road = road_index(cur_road) as u8;
        let mut k = 0.0;
        while k < len {
            let t = (k + 0.5) / len;
            let kap = k_peak * trapezoid(t, r);
            kappa[i] = kap as f32;
            grade_raw[i] = (rise / len) as f32;
            zone[i] = cur_zone;
            road_type[i] = road;
            elev_raw[i] = u8::from(extra.elevated);
            h += kap;
            x += kernel::cos(h - kap * 0.5);
            z += kernel::sin(h - kap * 0.5);
            i += 1;
            px[i] = x as f32;
            pz[i] = z as f32;
            k += 1.0;
        }
    }
    kappa[n - 1] = 0.0;
    grade_raw[n - 1] = grade_raw[n - 2];
    zone[n - 1] = zone[n - 2];
    road_type[n - 1] = road_type[n - 2];
    elev_raw[n - 1] = elev_raw[n - 2];
    let mut y_raw = vec![0f32; n];
    if let Some(elevation) = &level.elevation {
        // The level owns the ground (Downtown Streets): the road sits on it.
        for k in 0..n {
            y_raw[k] = elevation(px[k] as f64, pz[k] as f64) as f32;
        }
    } else {
        y_raw[0] = level.start_height.unwrap_or(100.0) as f32;
        for k in 1..n {
            y_raw[k] = (y_raw[k - 1] as f64 + grade_raw[k - 1] as f64) as f32;
        }
    }
    Raw {
        n,
        px,
        pz,
        y_raw,
        kappa,
        zone,
        road_type,
        elev_raw,
        bank: None,
        hw: None,
        wall_l: None,
        wall_r: None,
        run_l: None,
        run_r: None,
    }
}

fn build_loop(
    _level: &Level,
    spec: &crate::level::LoopSpec,
    tags: &mut Vec<Tag>,
) -> Result<Raw, String> {
    // path() gives the closed centreline, and optionally surveyed heights,
    // camber, half-widths and barrier distances at the same points.
    let p = (spec.path)()?;
    let (xs, zs) = (&p.x, &p.z);
    let m = xs.len();
    // Cumulative arc length around the closed polyline.
    let mut cum = vec![0f64; m + 1];
    for k in 0..m {
        let j = (k + 1) % m;
        cum[k + 1] = cum[k] + kernel::hypot(xs[j] - xs[k], zs[j] - zs[k]);
    }
    let total = cum[m];
    let n = js::round(total) as usize;
    let step = total / n as f64;
    let mut px = vec![0f32; n];
    let mut pz = vec![0f32; n];
    // The extras that the path carries, in the JS's order: y, bank, hw,
    // wallL, wallR, runL, runR.
    let src: [Option<&Vec<f64>>; 7] = [
        p.y.as_ref(),
        p.bank.as_ref(),
        p.hw.as_ref(),
        p.wall_l.as_ref(),
        p.wall_r.as_ref(),
        p.run_l.as_ref(),
        p.run_r.as_ref(),
    ];
    let mut ex: [Option<Vec<f32>>; 7] = Default::default();
    for (e, s) in ex.iter_mut().zip(src.iter()) {
        if s.is_some() {
            *e = Some(vec![0f32; n]);
        }
    }
    let mut seg = 0usize;
    for i in 0..n {
        let d = i as f64 * step;
        while cum[seg + 1] < d {
            seg += 1;
        }
        let t = (d - cum[seg]) / (cum[seg + 1] - cum[seg]);
        let j = (seg + 1) % m;
        px[i] = (xs[seg] + (xs[j] - xs[seg]) * t) as f32;
        pz[i] = (zs[seg] + (zs[j] - zs[seg]) * t) as f32;
        for (e, s) in ex.iter_mut().zip(src.iter()) {
            if let (Some(e), Some(s)) = (e, s) {
                e[i] = (s[seg] + (s[j] - s[seg]) * t) as f32;
            }
        }
    }
    let [ey, ebank, ehw, ewall_l, ewall_r, erun_l, erun_r] = ex;
    // Tags by fraction (f0/f1) or metres (s0/s1); elevated ones lift the
    // road with long ramps.
    let base_y = spec.base_y.unwrap_or(0.0);
    let mut y_raw = ey.unwrap_or_else(|| vec![base_y as f32; n]);
    let mut elev_raw = vec![0u8; n];
    let nf = n as f64;
    for tg in &spec.tags {
        let s0 = js::round(tg.s0.unwrap_or_else(|| tg.f0.unwrap() * nf));
        let s1 = js::round(tg.s1.unwrap_or_else(|| tg.f1.unwrap() * nf));
        tags.push(Tag {
            tag: tg.tag.to_string(),
            s0,
            s1,
            turn: Some(0.0),
        });
        if let Some(elevated) = tg.elevated.filter(|&e| e != 0.0) {
            let ramp = 260.0;
            tags.push(Tag {
                tag: format!("{}-up", tg.tag),
                s0: s0 - ramp,
                s1: s0,
                turn: None,
            });
            tags.push(Tag {
                tag: format!("{}-down", tg.tag),
                s0: s1,
                s1: s1 + ramp,
                turn: None,
            });
            let mut s = s0 - ramp;
            while s <= s1 + ramp {
                let k = (((s % nf) + nf) % nf) as usize;
                let up = smoothstep(s0 - ramp, s0, s);
                let down = 1.0 - smoothstep(s1, s1 + ramp, s);
                y_raw[k] = (y_raw[k] as f64 + elevated * js::min(up, down)) as f32;
                if y_raw[k] as f64 > base_y + 1.4 {
                    elev_raw[k] = 1;
                }
                s += 1.0;
            }
        }
    }
    let kappa = vec![0f32; n];
    // Zones start at the given metres into the lap (zone 0 from s = 0).
    let mut zone = vec![0u8; n];
    let starts = spec.zones.clone().unwrap_or_else(|| vec![0.0]);
    for (i, zi) in zone.iter_mut().enumerate() {
        let mut z = 0;
        while z + 1 < starts.len() && i as f64 >= starts[z + 1] {
            z += 1;
        }
        *zi = z as u8;
    }
    let mut road_type = vec![road_index(spec.road.unwrap_or("freeway")) as u8; n];
    for r in &spec.roads {
        let ni = n as i64;
        for s in r.s0..r.s1 {
            road_type[(((s % ni) + ni) % ni) as usize] = road_index(r.road) as u8;
        }
    }
    Ok(Raw {
        n,
        px,
        pz,
        y_raw,
        kappa,
        zone,
        road_type,
        elev_raw,
        bank: ebank,
        hw: ehw,
        wall_l: ewall_l,
        wall_r: ewall_r,
        run_l: erun_l,
        run_r: erun_r,
    })
}

fn build(level: &Level, is_loop: bool, raw: Raw, tags: Vec<Tag>) -> Track {
    let Raw {
        n,
        px,
        pz,
        y_raw,
        mut kappa,
        zone,
        road_type,
        elev_raw,
        ..
    } = raw;
    let w = is_loop;
    let length = if is_loop { n as f64 } else { (n - 1) as f64 };
    let laps = if is_loop { level.laps.unwrap_or(0) } else { 0 };

    // Elevation: smooth crest/sag kinks.
    let sigma = level
        .elevation_smooth
        .or(level.loop_spec().and_then(|l| l.elevation_smooth))
        .unwrap_or(9.0);
    let py = gaussian_smooth(&y_raw, sigma, w);
    let prev = |k: usize| {
        if w {
            (k + n - 1) % n
        } else {
            k.saturating_sub(1)
        }
    };
    let next = |k: usize| if w { (k + 1) % n } else { (k + 1).min(n - 1) };
    let mut grade = vec![0f32; n];
    for k in 0..n {
        let (a, b) = (prev(k), next(k));
        let div = if w || (k > 0 && k < n - 1) { 2.0 } else { 1.0 };
        grade[k] = ((py[b] as f64 - py[a] as f64) / div) as f32;
    }

    // Frames.
    let mut fx = vec![0f32; n];
    let mut fz = vec![0f32; n];
    let mut rx = vec![0f32; n];
    let mut rz = vec![0f32; n];
    for k in 0..n {
        let (a, b) = (prev(k), next(k));
        let mut dx = px[b] as f64 - px[a] as f64;
        let mut dz = pz[b] as f64 - pz[a] as f64;
        let l = js::or(kernel::hypot(dx, dz), 1.0);
        dx /= l;
        dz /= l;
        fx[k] = dx as f32;
        fz[k] = dz as f32;
        rx[k] = -dz as f32;
        rz[k] = dx as f32;
    }
    // Loops get their curvature from the geometry.
    if is_loop {
        for k in 0..n {
            let a = (k + n - 1) % n;
            let b = (k + 1) % n;
            let h0 = kernel::atan2(fz[a] as f64, fx[a] as f64);
            let h1 = kernel::atan2(fz[b] as f64, fx[b] as f64);
            let mut d = h1 - h0;
            while d > core::f64::consts::PI {
                d -= core::f64::consts::PI * 2.0;
            }
            while d < -core::f64::consts::PI {
                d += core::f64::consts::PI * 2.0;
            }
            kappa[k] = (d / 2.0) as f32;
        }
    }

    // Width, barriers and banking.
    let mut hw_raw = vec![0f32; n];
    let mut margin_raw = vec![0f32; n];
    let mut bank_k = vec![0f32; n];
    for k in 0..n {
        let rt = &ROAD_TYPES[road_type[k] as usize];
        hw_raw[k] = rt.hw as f32;
        margin_raw[k] = rt.margin as f32;
        bank_k[k] = rt.bank.unwrap_or(1.0) as f32; // kerbed streets stay level across
    }
    // A surveyed road brings its own width.
    let hw = match &raw.hw {
        Some(h) => gaussian_smooth(h, 3.0, w),
        None => gaussian_smooth(&hw_raw, 40.0, w),
    };
    let margin = gaussian_smooth(&margin_raw, 40.0, w);
    let k_smooth = gaussian_smooth(&kappa, 10.0, w);
    let mut bank_raw = vec![0f32; n];
    for k in 0..n {
        bank_raw[k] = (clamp(k_smooth[k] as f64 * 40.0, -0.085, 0.085) * bank_k[k] as f64) as f32;
    }
    // A surveyed road brings its own camber.
    let bank = match &raw.bank {
        Some(b) => gaussian_smooth(b, 2.0, w),
        None => gaussian_smooth(&bank_raw, 12.0, w),
    };

    // Collision limits either side of the centreline (positive distances).
    let mut wall_l = vec![0f32; n];
    let mut wall_r = vec![0f32; n];
    for k in 0..n {
        // Surveyed barriers (a circuit's walls), never closer than a metre
        // and a half off the tarmac.
        let open = hw[k] as f64 + margin[k] as f64;
        wall_l[k] = match &raw.wall_l {
            Some(wl) => js::max(hw[k] as f64 + 1.5, wl[k] as f64),
            None => open,
        } as f32;
        wall_r[k] = match &raw.wall_r {
            Some(wr) => js::max(hw[k] as f64 + 1.5, wr[k] as f64),
            None => open,
        } as f32;
    }

    // Zones.
    let zn = level.zones.len();
    let mut zone_start = vec![0usize; zn];
    for k in 1..n {
        if zone[k] != zone[k - 1] {
            zone_start[zone[k] as usize] = k;
        }
    }
    let zones = level
        .zones
        .iter()
        .enumerate()
        .map(|(idx, z)| TrackZone {
            zone: z.clone(),
            id: idx,
            s0: zone_start[idx] as f64,
            s1: if idx < zn - 1 {
                zone_start[idx + 1] as f64
            } else {
                length
            },
        })
        .collect();
    let finish_s = if is_loop {
        f64::INFINITY
    } else {
        length - level.finish_runoff.unwrap_or(180.0)
    };
    let start_s = match level.loop_spec() {
        Some(l) => l.start_s.unwrap_or(120.0),
        None => 60.0,
    };

    let mut t = Track {
        level: level.clone(),
        is_loop,
        n,
        length,
        laps,
        px,
        py,
        pz,
        kappa,
        k_smooth,
        grade,
        zone,
        road_type,
        fx,
        fz,
        rx,
        rz,
        hw,
        margin,
        bank,
        run_l: raw.run_l,
        run_r: raw.run_r,
        loose_at: level.loose_ground.clone(),
        elevated: elev_raw,
        wall_l,
        wall_r,
        zone_start,
        zones,
        finish_s,
        start_s,
        runout: 0.0,
        fence_gaps: Vec::new(),
        tags,
        cell: 32.0,
        hash: BTreeMap::new(),
        bounds: Bounds::default(),
        racing_line: Vec::new(),
        speed_profile: Vec::new(),
    };
    build_spatial_hash(&mut t);
    build_racing_line(&mut t);
    t
}

fn build_spatial_hash(t: &mut Track) {
    t.cell = 32.0;
    let mut hash: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
    for k in (0..t.n).step_by(2) {
        let key = t.hash_key(t.px[k] as f64, t.pz[k] as f64);
        hash.entry(key).or_default().push(k);
    }
    t.hash = hash;
    let (mut min_x, mut max_x, mut min_z, mut max_z) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for k in 0..t.n {
        min_x = js::min(min_x, t.px[k] as f64);
        max_x = js::max(max_x, t.px[k] as f64);
        min_z = js::min(min_z, t.pz[k] as f64);
        max_z = js::max(max_z, t.pz[k] as f64);
    }
    t.bounds = Bounds {
        min_x,
        max_x,
        min_z,
        max_z,
    };
}

// ── AI helpers: racing line and target-speed profile ───────────
fn build_racing_line(t: &mut Track) {
    let n = t.n;
    let w = t.is_loop;
    // Aim for the inside of corners: offset proportional to smoothed
    // curvature, looked-ahead a little so the apex is early-ish.
    let k2 = gaussian_smooth(&t.kappa, 35.0, w);
    let mut line = vec![0f32; n];
    for i in 0..n {
        let j = if w { (i + 12) % n } else { (n - 1).min(i + 12) };
        let lim = t.hw[i] as f64 - 1.6;
        line[i] = clamp(k2[j] as f64 * 420.0, -lim, lim) as f32;
    }
    let mut racing_line = gaussian_smooth(&line, 18.0, w);
    // Smoothing can carry the line past a narrowing (circuit straights
    // are wider than their corners): hold it on the tarmac.
    for i in 0..n {
        let lim = t.hw[i] as f64 - 1.6;
        racing_line[i] = clamp(racing_line[i] as f64, -lim, lim) as f32;
    }
    t.racing_line = racing_line;

    // Speed a well-driven car can carry through each metre.
    let mut vmax = vec![0f32; n];
    let (a_lat, v_top) = (15.5, 69.5);
    for i in 0..n {
        let k = (t.k_smooth[i] as f64).abs() + 1e-5;
        vmax[i] = js::min(v_top, (a_lat / k).sqrt()) as f32;
    }
    // Backward pass: you have to brake before a corner (twice round a loop).
    let brake = 11.0;
    let passes = if w { 2 } else { 1 };
    for _ in 0..passes {
        let top = n as i64 - 2 + i64::from(w);
        for i in (0..=top).rev() {
            let i = i as usize;
            let j = if w { (i + 1) % n } else { i + 1 };
            let vj = vmax[j] as f64;
            vmax[i] = js::min(vmax[i] as f64, (vj * vj + 2.0 * brake).sqrt()) as f32;
        }
    }
    t.speed_profile = vmax;
}
