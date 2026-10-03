//! From `src/world/Road.js`: the generic `extrude` sweep and the run
//! helpers (`runs`, `chunks`, `groupRuns`) the road and the scenery use
//! (roadmap WP 3.3). The `Road` class itself joins this module in WP 3.5.
//!
//! Road surface, shoulders, markings and barriers, all extruded along the
//! track centreline from cross-section profiles.

// Index loops stay index loops (DECISIONS D52).
#![allow(clippy::needless_range_loop)]

use mr_math::js;
use mr_track::{Frame, Track};

use crate::three_geom::{BufferAttribute, BufferGeometry};

pub const ROAD_STEP: f64 = 2.0;

/// `(f, s) => number`.
pub type LatFn = Box<dyn Fn(&Frame, f64) -> f64 + Send + Sync>;

/// `(f, s, p) => [r, g, b]`.
pub type ColorFn = Box<dyn Fn(&Frame, f64, usize) -> [f64; 3] + Send + Sync>;

/// A profile value: a number, or a function of the frame and s (`(f, s)`).
pub enum Lat {
    Num(f64),
    Fn(LatFn),
}

impl Lat {
    /// `(f, s) => ...`.
    pub fn f(f: impl Fn(&Frame, f64) -> f64 + Send + Sync + 'static) -> Lat {
        Lat::Fn(Box::new(f))
    }
}

impl From<f64> for Lat {
    fn from(v: f64) -> Lat {
        Lat::Num(v)
    }
}

/// One point of a cross-section: `{lat, dy, u, abs, gapAfter}`.
pub struct ProfilePoint {
    pub lat: Lat,
    /// `None` is the JS's missing `dy` (0).
    pub dy: Option<Lat>,
    /// `None`: u is `lat / 4`.
    pub u: Option<f64>,
    /// y is `dy` alone, not relative to the road.
    pub abs: bool,
    /// No faces between this point and the next.
    pub gap_after: bool,
}

impl ProfilePoint {
    /// `{ lat }`.
    pub fn new(lat: impl Into<Lat>) -> ProfilePoint {
        ProfilePoint {
            lat: lat.into(),
            dy: None,
            u: None,
            abs: false,
            gap_after: false,
        }
    }

    /// `{ ..., dy }`.
    pub fn dy(mut self, dy: impl Into<Lat>) -> ProfilePoint {
        self.dy = Some(dy.into());
        self
    }

    /// `{ ..., u }`.
    pub fn u(mut self, u: f64) -> ProfilePoint {
        self.u = Some(u);
        self
    }

    pub fn abs(mut self) -> ProfilePoint {
        self.abs = true;
        self
    }

    pub fn gap_after(mut self) -> ProfilePoint {
        self.gap_after = true;
        self
    }
}

/// `color`: one colour for every vertex, or `(f, s, p) => [r, g, b]`.
pub enum ExtrudeColor {
    Rgb([f64; 3]),
    Fn(ColorFn),
}

/// `extrude`'s options: `{ step = ROAD_STEP, vScale = 8, flat = false,
/// color = null }` (`flat` is unused by the JS body and not ported).
pub struct ExtrudeOpts {
    pub step: f64,
    pub v_scale: f64,
    pub color: Option<ExtrudeColor>,
}

impl Default for ExtrudeOpts {
    fn default() -> Self {
        ExtrudeOpts {
            step: ROAD_STEP,
            v_scale: 8.0,
            color: None,
        }
    }
}

/// Generic extrusion. profile: [{lat(f,i), dy(f,i), u}] across the section.
/// ranges: [[s0, s1], ...]. Produces one BufferGeometry.
pub fn extrude(
    track: &Track,
    ranges: &[[f64; 2]],
    profile: &[ProfilePoint],
    o: &ExtrudeOpts,
) -> BufferGeometry {
    let mut pos: Vec<f64> = Vec::new();
    let mut uv: Vec<f64> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let mut col: Vec<f64> = Vec::new();
    let p_len = profile.len();
    for &[s0, s1] in ranges {
        if s1 - s0 < 0.5 {
            continue;
        }
        let base = pos.len() / 3;
        let mut rows = 0;
        let mut s = s0;
        loop {
            let ss = js::min(s, s1);
            let f = track.frame(ss);
            for (p, pr) in profile.iter().enumerate() {
                let lat = match &pr.lat {
                    Lat::Fn(l) => l(&f, ss),
                    Lat::Num(v) => *v,
                };
                let dy = match &pr.dy {
                    Some(Lat::Fn(d)) => d(&f, ss),
                    // `pr.dy || 0`
                    Some(Lat::Num(v)) => js::or(*v, 0.0),
                    None => 0.0,
                };
                let x = f.x + f.rx * lat;
                let z = f.z + f.rz * lat;
                let y = (if pr.abs { 0.0 } else { f.y - lat * f.bank }) + dy;
                pos.extend_from_slice(&[x, y, z]);
                uv.extend_from_slice(&[pr.u.unwrap_or(lat / 4.0), ss / o.v_scale]);
                match &o.color {
                    Some(ExtrudeColor::Rgb(c)) => col.extend_from_slice(c),
                    Some(ExtrudeColor::Fn(cf)) => col.extend_from_slice(&cf(&f, ss, p)),
                    None => {}
                }
            }
            rows += 1;
            if ss >= s1 {
                break;
            }
            s += o.step;
        }
        for r in 0..rows.max(1) - 1 {
            for p in 0..p_len.saturating_sub(1) {
                if profile[p].gap_after {
                    continue;
                }
                let a = (base + r * p_len + p) as u32;
                let b = a + 1;
                let c = a + p_len as u32;
                let d = c + 1;
                idx.extend_from_slice(&[a, b, c, b, d, c]);
            }
        }
    }
    let mut g = BufferGeometry::new();
    g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
    g.set_attribute("uv", BufferAttribute::from_f64(&uv, 2));
    if o.color.is_some() {
        g.set_attribute("color", BufferAttribute::from_f64(&col, 3));
    }
    g.set_index(&idx);
    g.compute_vertex_normals();
    g
}

/// Split [0, len] into chunks for frustum culling (`size` 560 in the JS
/// default).
pub fn chunks(s0: f64, s1: f64, size: f64) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    let mut s = s0;
    while s < s1 {
        out.push([s, js::min(s1, s + size)]);
        s += size;
    }
    out
}

/// Split runs into chunks and bundle neighbouring pieces so each mesh covers
/// about `size` metres of road (fewer draw calls, still frustum-cullable).
pub fn group_runs(ranges: &[[f64; 2]], size: f64) -> Vec<Vec<[f64; 2]>> {
    let pieces: Vec<[f64; 2]> = ranges
        .iter()
        .flat_map(|r| chunks(r[0], r[1], size))
        .collect();
    let mut groups = Vec::new();
    let mut cur: Vec<[f64; 2]> = Vec::new();
    let mut start: Option<f64> = None;
    for p in pieces {
        if start.is_none() {
            start = Some(p[0]);
        }
        if p[1] - start.unwrap() > size && !cur.is_empty() {
            groups.push(std::mem::take(&mut cur));
            start = Some(p[0]);
        }
        cur.push(p);
    }
    if !cur.is_empty() {
        groups.push(cur);
    }
    groups
}

/// Runs where pred(s) holds, sampled every `step` metres from `s0` to `s1`
/// (the JS defaults: 2, 0 and `track.length`).
pub fn runs(pred: impl Fn(f64) -> bool, step: f64, s0: f64, s1: f64) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    let mut start: Option<f64> = None;
    let mut s = s0;
    while s <= s1 {
        let ok = pred(s);
        if ok && start.is_none() {
            start = Some(s);
        }
        if !ok && let Some(st) = start {
            out.push([st, s]);
            start = None;
        }
        s += step;
    }
    if let Some(st) = start {
        out.push([st, s1]);
    }
    out
}
