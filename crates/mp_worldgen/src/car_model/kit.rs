//! The geometry kit at the top of `CarModel.js`: the profile helpers, the
//! loft and its surface sampler, the triangle-soup builder, decals and
//! ribbons, sweeps, lathes, airfoils, extrusions and the per-material
//! `Parts` buckets (`:138-752`).
//!
//! Every helper keeps the JS arithmetic in its order: points and normals
//! are `f64` while the JS holds them in plain arrays, and are rounded to
//! `f32` where the JS stores them into a `Float32Array` (a
//! `Float32BufferAttribute`, or the copies `loft` makes per bucket).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use mp_math::{js, kernel};
use mp_scene::BufferData;

use crate::three_geom::{
    BufferAttribute, BufferGeometry, Euler, EulerOrder, ExtrudeOptions, Matrix4, Shape, Vector2,
    extrude_geometry, lathe_geometry, merge_geometries, triangulate_shape,
};

pub(crate) const PI: f64 = core::f64::consts::PI;

/// A point or a normal.
pub type P3 = [f64; 3];
/// A point in a 2D view (`[u, v]`).
pub type P2 = [f64; 2];
/// A vertex colour: the JS passes a grey level or an `[r, g, b]` array,
/// and `colArr` makes both an array.
pub type Col = [f64; 3];

/// `colArr(v)` of a grey level.
pub const fn grey(v: f64) -> Col {
    [v, v, v]
}

/// White, the JS default colour `1`.
pub const WHITE: Col = grey(1.0);

/// `clamp01(v)`.
pub fn clamp01(v: f64) -> f64 {
    js::min(1.0, js::max(0.0, v))
}

/// `smooth(a, b, v)`: smoothstep from a to b.
pub fn smooth(a: f64, b: f64, v: f64) -> f64 {
    let t = clamp01((v - a) / (b - a));
    t * t * (3.0 - 2.0 * t)
}

/// `bump(v, c, w)`: a Gaussian bump.
pub fn bump(v: f64, c: f64, w: f64) -> f64 {
    kernel::exp(-kernel::pow((v - c) / w, 2.0))
}

/// `inRange(v, a, b)`.
pub fn in_range(v: f64, a: f64, b: f64) -> bool {
    v >= a && v <= b
}

/// `AMBER`.
pub const AMBER: Col = [1.0, 0.42, 0.06];

/// `AMBER.map((c) => c * k)`.
pub fn amber(k: f64) -> Col {
    [AMBER[0] * k, AMBER[1] * k, AMBER[2] * k]
}

/// `[...a].sort((x, y) => x - y)`: stable, ascending.
pub(crate) fn sort_asc(v: &mut [f64]) {
    v.sort_by(|a, b| {
        (a - b)
            .partial_cmp(&0.0)
            .unwrap_or(core::cmp::Ordering::Equal)
    });
}

/// `[...a].sort((x, y) => y - x)`: stable, descending.
pub(crate) fn sort_desc(v: &mut [f64]) {
    v.sort_by(|a, b| {
        (b - a)
            .partial_cmp(&0.0)
            .unwrap_or(core::cmp::Ordering::Equal)
    });
}

// ── profile helpers ─────────────────────────────────────────────────────

/// `interp(pts, z)`: piecewise-linear y(z) through points sorted by z.
pub fn interp(pts: &[P2], z: f64) -> f64 {
    if z <= pts[0][0] {
        return pts[0][1];
    }
    for i in 1..pts.len() {
        if z <= pts[i][0] {
            let (a, b) = (pts[i - 1], pts[i]);
            let t = (z - a[0]) / js::or(b[0] - a[0], 1e-9);
            return a[1] + (b[1] - a[1]) * t;
        }
    }
    pts[pts.len() - 1][1]
}

/// A function of z: a profile line (`spline`, `interp` or a constant).
pub type Line = Rc<dyn Fn(f64) -> f64>;

/// `(z) => interp(pts, z)`.
pub fn linear(pts: &[P2]) -> Line {
    let pts = pts.to_vec();
    Rc::new(move |z| interp(&pts, z))
}

/// `spline(pts)`: a monotone cubic through the profile points, a smooth
/// roof/bonnet line with no overshoot, so few points still give a flowing
/// silhouette.
pub fn spline(pts: &[P2]) -> Line {
    let pts = pts.to_vec();
    let n = pts.len();
    if n < 3 {
        return Rc::new(move |z| interp(&pts, z));
    }
    let mut d = Vec::with_capacity(n - 1);
    let mut m = vec![0.0; n];
    for i in 0..n - 1 {
        d.push((pts[i + 1][1] - pts[i][1]) / js::or(pts[i + 1][0] - pts[i][0], 1e-9));
    }
    m[0] = d[0];
    m[n - 1] = d[n - 2];
    for i in 1..n - 1 {
        m[i] = if d[i - 1] * d[i] <= 0.0 {
            0.0
        } else {
            (d[i - 1] + d[i]) / 2.0
        };
    }
    for i in 0..n - 1 {
        if d[i] == 0.0 {
            m[i] = 0.0;
            m[i + 1] = 0.0;
            continue;
        }
        let a = m[i] / d[i];
        let b = m[i + 1] / d[i];
        let s = a * a + b * b;
        if s > 9.0 {
            let t = 3.0 / f64::sqrt(s);
            m[i] = t * a * d[i];
            m[i + 1] = t * b * d[i];
        }
    }
    Rc::new(move |z| {
        if z <= pts[0][0] {
            return pts[0][1];
        }
        if z >= pts[n - 1][0] {
            return pts[n - 1][1];
        }
        let mut i = 0;
        while z > pts[i + 1][0] {
            i += 1;
        }
        let h = pts[i + 1][0] - pts[i][0];
        let t = (z - pts[i][0]) / h;
        let t2 = t * t;
        let t3 = t2 * t;
        (2.0 * t3 - 3.0 * t2 + 1.0) * pts[i][1]
            + (t3 - 2.0 * t2 + t) * h * m[i]
            + (-2.0 * t3 + 3.0 * t2) * pts[i + 1][1]
            + (t3 - t2) * h * m[i + 1]
    })
}

/// A wheel arch: `[zc, yc, R]`.
pub type Arch = [f64; 3];

/// `sill(pts, arches)`: the sill line with semicircular wheel arches cut
/// into it.
pub fn sill(pts: &[P2], arches: &[Arch]) -> Line {
    let pts = pts.to_vec();
    let arches = arches.to_vec();
    Rc::new(move |z| {
        let mut y = interp(&pts, z);
        for &[zc, yc, r] in &arches {
            let d = z - zc;
            if d.abs() <= r {
                y = js::max(y, yc + f64::sqrt(r * r - d * d));
            }
        }
        y
    })
}

/// `stations(z0, z1, n, extra)`: uniform spacing plus every key z (profile
/// vertices, arch samples, window edges) so creases land exactly on a
/// station.
pub fn stations(z0: f64, z1: f64, n: usize, extra: &[f64]) -> Vec<f64> {
    let mut zs = Vec::new();
    for i in 0..=n {
        zs.push(z0 + ((z1 - z0) * i as f64) / n as f64);
    }
    for &z in extra {
        if z > z0 && z < z1 {
            zs.push(z);
        }
    }
    sort_asc(&mut zs);
    let mut out: Vec<f64> = Vec::new();
    for z in zs {
        if out.is_empty() || z - out[out.len() - 1] > 0.004 {
            out.push(z);
        }
    }
    let last = out.len() - 1;
    if z1 - out[last] < 0.004 {
        out[last] = z1;
    }
    out
}

/// `archZs(arches, n)`.
pub fn arch_zs(arches: &[Arch], n: usize) -> Vec<f64> {
    let mut out = Vec::new();
    for &[zc, _, r] in arches {
        out.extend([
            zc - r - 0.006,
            zc + r + 0.006,
            zc - r - 0.05,
            zc + r + 0.05,
            zc - r - 0.12,
            zc + r + 0.12,
        ]);
        for k in 0..=n {
            out.push(zc + r * kernel::cos((PI * k as f64) / n as f64));
        }
    }
    out
}

/// `endZs(z0, z1, r, n)`: extra stations packed towards both ends, where
/// plan-view rounding bends fast.
pub fn end_zs(z0: f64, z1: f64, r: f64, n: usize) -> Vec<f64> {
    let mut out = Vec::new();
    for k in 1..=n {
        let d = r * (1.0 - kernel::cos((k as f64 / (n + 1) as f64) * PI / 2.0));
        out.push(z0 + d);
        out.push(z1 - d);
    }
    out
}

// ── loft ────────────────────────────────────────────────────────────────

/// A point of a half ring: `{ x, y, t }`. Tags: 0 underside, 1 flank, 2
/// shoulder, 3 top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hp {
    pub x: f64,
    pub y: f64,
    pub t: u8,
}

/// `col(x, y, z) -> vertex colour`.
pub type ColFn = Box<dyn Fn(f64, f64, f64) -> Col>;

/// `mat(tag, y, z, ax) -> bucket`.
pub type MatFn = Box<dyn Fn(u8, f64, f64, f64) -> &'static str>;

/// The loft's options object `o`: `{ zs, top(z), bot(z), halfW(y, z),
/// rTop, rBot, crown, nb, nt, side, sideN, crownX, nCrown, mat, capMat,
/// col }` (`caps` is never false in the game, so the caps are always
/// made).
pub struct LoftOpts {
    pub zs: Vec<f64>,
    pub top: Line,
    pub bot: Line,
    pub half_w: Rc<dyn Fn(f64, f64) -> f64>,
    pub r_top: f64,
    pub r_bot: f64,
    pub crown: f64,
    pub nb: usize,
    pub nt: usize,
    /// `side`: absolute y levels.
    pub side: Vec<f64>,
    pub side_n: usize,
    /// `crownX`: absolute x.
    pub crown_x: Vec<f64>,
    pub n_crown: usize,
    pub mat: MatFn,
    pub cap_mat: Option<Box<dyn Fn(bool) -> &'static str>>,
    pub col: Option<ColFn>,
}

/// `halfRing(o, z)`: the half ring from the bottom centre up to the top
/// centre.
pub fn half_ring(o: &LoftOpts, z: f64) -> Vec<Hp> {
    let yt = (o.top)(z);
    let yb = js::min((o.bot)(z), yt - 0.004);
    let h = yt - yb;
    let rb = js::min(o.r_bot, h * 0.3);
    let crown = js::min(o.crown, h * 0.12);
    let rt = js::min(o.r_top, js::max(0.0, h - rb - crown) * 0.6);
    let mut hr = vec![Hp {
        x: 0.0,
        y: yb,
        t: 0,
    }];
    let hwb = js::max(0.004, (o.half_w)(yb + rb, z));
    hr.push(Hp {
        x: js::max(0.002, hwb - rb),
        y: yb,
        t: 0,
    });
    for k in 1..=o.nb {
        let a = -PI / 2.0 + (PI / 2.0) * (k as f64 / o.nb as f64);
        hr.push(Hp {
            x: js::max(0.003, hwb - rb + rb * kernel::cos(a)),
            y: yb + rb + rb * kernel::sin(a),
            t: 1,
        });
    }
    let y_lo = yb + rb;
    let y_hi = yt - crown - rt;
    let mut lv = Vec::new();
    for k in 1..=o.side_n {
        lv.push(y_lo + ((y_hi - y_lo) * k as f64) / (o.side_n + 1) as f64);
    }
    lv.extend_from_slice(&o.side);
    sort_asc(&mut lv);
    // Clamp into the flank and keep strictly ascending: coincident ring
    // points give zero-area faces, and a vertex with only those gets a NaN
    // normal (which bloom smears into a black blotch).
    let n = lv.len();
    let eps = js::max(1e-4, js::min(0.002, (y_hi - y_lo) / (n + 2) as f64));
    for k in 0..n {
        let mut y = js::min(
            y_hi - eps * (n - k) as f64,
            js::max(y_lo + eps * (k + 1) as f64, lv[k]),
        );
        if k > 0 && y <= lv[k - 1] + eps * 0.5 {
            y = lv[k - 1] + eps * 0.5;
        }
        lv[k] = y;
    }
    for &y in &lv {
        hr.push(Hp {
            x: js::max(0.004, (o.half_w)(y, z)),
            y,
            t: 1,
        });
    }
    let hwt = js::max(0.006, (o.half_w)(y_hi, z));
    let cx = js::max(0.004, hwt - rt);
    let cy = y_hi;
    for k in 0..=o.nt {
        let a = (PI / 2.0) * (k as f64 / o.nt as f64);
        hr.push(Hp {
            x: cx + rt * kernel::cos(a),
            y: cy + rt * kernel::sin(a) + if k == 0 { 0.0 } else { 1e-5 * k as f64 },
            t: 2,
        });
    }
    let mut xs = Vec::new();
    for k in (1..=o.n_crown).rev() {
        xs.push((cx * k as f64) / (o.n_crown + 1) as f64);
    }
    for &x in &o.crown_x {
        xs.push(js::min(cx * 0.995, x));
    }
    sort_desc(&mut xs);
    let len = xs.len();
    for k in 0..len {
        let lim = cx * (1.0 - (0.004 * (k + 1) as f64));
        let mut x = js::min(xs[k], lim);
        if k > 0 && x >= xs[k - 1] - 1e-4 {
            x = xs[k - 1] - 1e-4;
        }
        xs[k] = js::max(1e-4 * (len - k) as f64, x);
    }
    for &x in &xs {
        let f = if cx > 0.0 { x / cx } else { 0.0 };
        hr.push(Hp {
            x,
            y: yt - crown * f * f,
            t: 3,
        });
    }
    hr.push(Hp {
        x: 0.0,
        y: yt,
        t: 3,
    });
    hr
}

/// `fixNormals(g, fb)`: replace NaN / zero normals (degenerate faces) with
/// a sane fallback.
pub fn fix_normals(g: &mut BufferGeometry, fb: P3) {
    let Some(n) = g.get_attribute_mut("normal") else {
        return;
    };
    let BufferData::F32(a) = &mut n.array else {
        panic!("normals are a Float32Array");
    };
    let mut i = 0;
    while i < a.len() {
        let l = kernel::hypot3(f64::from(a[i]), f64::from(a[i + 1]), f64::from(a[i + 2]));
        if !(l > 1e-8) {
            a[i] = fb[0] as f32;
            a[i + 1] = fb[1] as f32;
            a[i + 2] = fb[2] as f32;
        }
        i += 3;
    }
}

/// The buckets a part fills: `{ [bucket]: geometry }` in insertion order.
pub type Buckets = Vec<(&'static str, BufferGeometry)>;

fn bucket_slot<T>(
    list: &mut Vec<(&'static str, T)>,
    b: &'static str,
    make: impl FnOnce() -> T,
) -> usize {
    match list.iter().position(|(k, _)| *k == b) {
        Some(i) => i,
        None => {
            list.push((b, make()));
            list.len() - 1
        }
    }
}

/// What `loft(o)` returns: `{ geo, S }`.
pub struct Loft {
    pub geo: Buckets,
    pub s: Surf,
}

/// `loft(o)`: rounded cross-sections at the stations, faces assigned to
/// material buckets, normals smoothed over the whole skin, flat end caps.
pub fn loft(o: LoftOpts) -> Loft {
    let halves: Vec<Vec<Hp>> = o.zs.iter().map(|&z| half_ring(&o, z)).collect();
    let rings: Vec<Vec<Hp>> = halves
        .iter()
        .map(|half| {
            let mut ring = half.clone();
            for k in (1..=half.len() - 2).rev() {
                ring.push(Hp {
                    x: -half[k].x,
                    y: half[k].y,
                    t: half[k].t,
                });
            }
            ring
        })
        .collect();
    let m = rings[0].len();
    let mut pos = Vec::new();
    let mut cols = Vec::new();
    for (i, ring) in rings.iter().enumerate() {
        for p in ring {
            pos.extend([p.x, p.y, o.zs[i]]);
            let c = match &o.col {
                Some(f) => f(p.x, p.y, o.zs[i]),
                None => WHITE,
            };
            cols.extend(c);
        }
    }
    let mut buckets: Vec<(&'static str, Vec<u32>)> = Vec::new();
    let mut all = Vec::new();
    for i in 0..rings.len() - 1 {
        for j in 0..m {
            let j2 = (j + 1) % m;
            let a = (i * m + j) as u32;
            let b = (i * m + j2) as u32;
            let c = ((i + 1) * m + j2) as u32;
            let d = ((i + 1) * m + j) as u32;
            let (pa, pb, pc, pd) = (rings[i][j], rings[i][j2], rings[i + 1][j2], rings[i + 1][j]);
            let tag = pa.t.min(pb.t);
            let y = (pa.y + pb.y + pc.y + pd.y) / 4.0;
            let ax = (pa.x.abs() + pb.x.abs() + pc.x.abs() + pd.x.abs()) / 4.0;
            let z = (o.zs[i] + o.zs[i + 1]) / 2.0;
            let bk = (o.mat)(tag, y, z, ax);
            let s = bucket_slot(&mut buckets, bk, Vec::new);
            buckets[s].1.extend([a, b, c, a, c, d]);
            all.extend([a, b, c, a, c, d]);
        }
    }
    // Smooth normals over the whole skin, then split per material.
    let mut skin = BufferGeometry::new();
    skin.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
    skin.set_index(&all);
    skin.compute_vertex_normals();
    fix_normals(&mut skin, [0.0, 1.0, 0.0]);
    let pp = skin.position().as_f32().expect("f32").to_vec();
    let nn = skin
        .get_attribute("normal")
        .and_then(BufferAttribute::as_f32)
        .expect("normals")
        .to_vec();
    let mut out: Buckets = Vec::new();
    for (b, idx) in &buckets {
        let mut p = vec![0f32; idx.len() * 3];
        let mut n = vec![0f32; idx.len() * 3];
        let mut c = vec![0f32; idx.len() * 3];
        for (k, &v) in idx.iter().enumerate() {
            let v = v as usize;
            for e in 0..3 {
                p[k * 3 + e] = pp[v * 3 + e];
                n[k * 3 + e] = nn[v * 3 + e];
                c[k * 3 + e] = cols[v * 3 + e] as f32;
            }
        }
        let mut g = BufferGeometry::new();
        g.set_attribute("position", BufferAttribute::from_f32(p, 3));
        g.set_attribute("normal", BufferAttribute::from_f32(n, 3));
        g.set_attribute("color", BufferAttribute::from_f32(c, 3));
        out.push((b, g));
    }
    // Flat end caps (fan from the ring centroid).
    for (ri, dir) in [(0usize, -1.0f64), (rings.len() - 1, 1.0)] {
        let ring = &rings[ri];
        let z = o.zs[ri];
        let ys: Vec<f64> = ring.iter().map(|p| p.y).collect();
        if js::max_n(&ys) - js::min_n(&ys) < 0.01 {
            continue;
        }
        let mut cy = 0.0;
        for &y in &ys {
            cy += y;
        }
        let cy = cy / ys.len() as f64;
        let mut t = Tris::default();
        let nn = [0.0, 0.0, dir];
        let cc = match &o.col {
            Some(f) => f(0.0, cy, z),
            None => WHITE,
        };
        for j in 0..m {
            let (a, b) = (ring[j], ring[(j + 1) % m]);
            t.tri(
                [0.0, cy, z],
                [a.x, a.y, z],
                [b.x, b.y, z],
                nn,
                nn,
                nn,
                cc,
                cc,
                cc,
            );
        }
        let g = t.geo();
        let b = match &o.cap_mat {
            Some(f) => f(dir > 0.0),
            None => (o.mat)(1, cy, z, 0.0),
        };
        match out.iter().position(|(k, _)| *k == b) {
            Some(i) => {
                let merged = merge_geometries(&[&out[i].1, &g], false).expect("the cap merges");
                out[i].1 = merged;
            }
            None => out.push((b, g)),
        }
    }
    let o = Rc::new(o);
    Loft {
        geo: out,
        s: Surf::new(o),
    }
}

// ── triangle soup builder ───────────────────────────────────────────────

/// `Tris`: a triangle soup with per-vertex normals and colours, held as the
/// JS holds them (plain numbers) until `geo()`.
#[derive(Clone, Debug, Default)]
pub struct Tris {
    p: Vec<f64>,
    n: Vec<f64>,
    c: Vec<f64>,
}

impl Tris {
    /// `tri(a, b, c, na, nb, nc, ca, cb, cc)`: a triangle with per-vertex
    /// normals, wound so its face agrees with them.
    #[allow(clippy::too_many_arguments)]
    pub fn tri(
        &mut self,
        a: P3,
        mut b: P3,
        mut c: P3,
        na: P3,
        mut nb: P3,
        mut nc: P3,
        ca: Col,
        mut cb: Col,
        mut cc: Col,
    ) {
        let (ux, uy, uz) = (b[0] - a[0], b[1] - a[1], b[2] - a[2]);
        let (vx, vy, vz) = (c[0] - a[0], c[1] - a[1], c[2] - a[2]);
        let fx = uy * vz - uz * vy;
        let fy = uz * vx - ux * vz;
        let fz = ux * vy - uy * vx;
        if fx * (na[0] + nb[0] + nc[0])
            + fy * (na[1] + nb[1] + nc[1])
            + fz * (na[2] + nb[2] + nc[2])
            < 0.0
        {
            core::mem::swap(&mut b, &mut c);
            core::mem::swap(&mut nb, &mut nc);
            core::mem::swap(&mut cb, &mut cc);
        }
        self.p
            .extend([a[0], a[1], a[2], b[0], b[1], b[2], c[0], c[1], c[2]]);
        self.n.extend([
            na[0], na[1], na[2], nb[0], nb[1], nb[2], nc[0], nc[1], nc[2],
        ]);
        self.c.extend([
            ca[0], ca[1], ca[2], cb[0], cb[1], cb[2], cc[0], cc[1], cc[2],
        ]);
    }

    /// `quad(a, b, c, d, na, nb, nc, nd, ca, cb, cc, cd)`.
    #[allow(clippy::too_many_arguments)]
    pub fn quad(
        &mut self,
        a: P3,
        b: P3,
        c: P3,
        d: P3,
        na: P3,
        nb: P3,
        nc: P3,
        nd: P3,
        ca: Col,
        cb: Col,
        cc: Col,
        cd: Col,
    ) {
        self.tri(a, b, c, na, nb, nc, ca, cb, cc);
        self.tri(a, c, d, na, nc, nd, ca, cc, cd);
    }

    /// `flat(a, b, c, hint, col)`: a flat-shaded triangle facing roughly
    /// along `hint`.
    pub fn flat(&mut self, a: P3, b: P3, c: P3, hint: P3, col: Col) {
        let (ux, uy, uz) = (b[0] - a[0], b[1] - a[1], b[2] - a[2]);
        let (vx, vy, vz) = (c[0] - a[0], c[1] - a[1], c[2] - a[2]);
        let mut fx = uy * vz - uz * vy;
        let mut fy = uz * vx - ux * vz;
        let mut fz = ux * vy - uy * vx;
        let l = kernel::hypot3(fx, fy, fz);
        if !(l > 1e-12) {
            return;
        }
        fx /= l;
        fy /= l;
        fz /= l;
        if fx * hint[0] + fy * hint[1] + fz * hint[2] < 0.0 {
            fx = -fx;
            fy = -fy;
            fz = -fz;
        }
        let f = [fx, fy, fz];
        self.tri(a, b, c, f, f, f, col, col, col);
    }

    /// `flatQuad(a, b, c, d, hint, col)`.
    pub fn flat_quad(&mut self, a: P3, b: P3, c: P3, d: P3, hint: P3, col: Col) {
        self.flat(a, b, c, hint, col);
        self.flat(a, c, d, hint, col);
    }

    /// `mirrorX()`: append a copy mirrored across X (winding is fixed up by
    /// `tri()`).
    pub fn mirror_x(&mut self) {
        let (p, n, c) = (self.p.clone(), self.n.clone(), self.c.clone());
        let mut i = 0;
        while i < p.len() {
            let v = |k: usize| [-p[i + k * 3], p[i + k * 3 + 1], p[i + k * 3 + 2]];
            let w = |k: usize| [-n[i + k * 3], n[i + k * 3 + 1], n[i + k * 3 + 2]];
            let q = |k: usize| [c[i + k * 3], c[i + k * 3 + 1], c[i + k * 3 + 2]];
            self.tri(v(0), v(1), v(2), w(0), w(1), w(2), q(0), q(1), q(2));
            i += 9;
        }
    }

    /// `geo()`: `position`, `normal` and `color` as `Float32BufferAttribute`s,
    /// normals fixed.
    pub fn geo(&self) -> BufferGeometry {
        let mut g = BufferGeometry::new();
        g.set_attribute("position", BufferAttribute::from_f64(&self.p, 3));
        g.set_attribute("normal", BufferAttribute::from_f64(&self.n, 3));
        g.set_attribute("color", BufferAttribute::from_f64(&self.c, 3));
        fix_normals(&mut g, [0.0, 1.0, 0.0]);
        g
    }
}

// ── surface sampling + decals ───────────────────────────────────────────

/// A 2D view onto the loft:
///   top:   u = x, v = z  (dropped onto the upper surface)
///   side:  u = z, v = y  (pushed onto the +X flank)
///   front: u = x, v = y  (pushed back onto the nose)
///   rear:  u = x, v = y  (pushed forward onto the tail)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Top,
    Side,
    Front,
    Rear,
}

impl View {
    /// `VIEW_DIR[view]`.
    fn dir(self) -> P3 {
        match self {
            View::Top => [0.0, 1.0, 0.0],
            View::Side => [1.0, 0.0, 0.0],
            View::Front => [0.0, 0.0, 1.0],
            View::Rear => [0.0, 0.0, -1.0],
        }
    }
}

/// A surface point and its normal (facing the viewer).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct At {
    pub p: P3,
    pub n: P3,
}

/// `Surf`: maps a view's 2D (u, v) onto the loft. Its ring cache is keyed
/// by `Math.round(z * 2e4)` and holds the ring of the first z asked for in
/// that key, as the JS `Map` does.
pub struct Surf {
    o: Rc<LoftOpts>,
    pub z0: f64,
    pub z1: f64,
    cache: RefCell<BTreeMap<i64, Rc<Vec<Hp>>>>,
}

impl Surf {
    pub fn new(o: Rc<LoftOpts>) -> Surf {
        let z0 = o.zs[0];
        let z1 = o.zs[o.zs.len() - 1];
        Surf {
            o,
            z0,
            z1,
            cache: RefCell::new(BTreeMap::new()),
        }
    }

    /// `ring(z)`.
    pub fn ring(&self, z: f64) -> Rc<Vec<Hp>> {
        let k = js::round(z * 2e4) as i64;
        if let Some(r) = self.cache.borrow().get(&k) {
            return r.clone();
        }
        let r = Rc::new(half_ring(&self.o, z));
        let mut cache = self.cache.borrow_mut();
        if cache.len() < 4000 {
            cache.insert(k, r.clone());
        }
        r
    }

    /// `topY(z, x)`.
    pub fn top_y(&self, z: f64, x: f64) -> f64 {
        let h = self.ring(z);
        let x = x.abs();
        for i in (1..h.len()).rev() {
            let (a, b) = (h[i], h[i - 1]);
            if (x >= a.x && x <= b.x) || (x <= a.x && x >= b.x) {
                return a.y + (b.y - a.y) * ((x - a.x) / js::or(b.x - a.x, 1e-9));
            }
        }
        let mut m = h[0];
        for &p in h.iter() {
            if p.x > m.x {
                m = p;
            }
        }
        m.y
    }

    /// `sideX(z, y)`.
    pub fn side_x(&self, z: f64, y: f64) -> f64 {
        let h = self.ring(z);
        let mut best = -1.0;
        for i in 1..h.len() {
            let (a, b) = (h[i - 1], h[i]);
            if (y >= a.y && y <= b.y) || (y <= a.y && y >= b.y) {
                let x = a.x + (b.x - a.x) * ((y - a.y) / js::or(b.y - a.y, 1e-9));
                if x > best {
                    best = x;
                }
            }
        }
        if best < 0.0 {
            best = if y < h[0].y { h[1].x } else { h[h.len() - 2].x };
        }
        best
    }

    /// `inside(z, x, y)` (on a fresh ring, not the cache).
    pub fn inside(&self, z: f64, x: f64, y: f64) -> bool {
        let h = half_ring(&self.o, z);
        let ax = x.abs();
        let mut c = false;
        let mut j = h.len() - 1;
        for i in 0..h.len() {
            let (a, b) = (h[i], h[j]);
            if (a.y > y) != (b.y > y) && ax < ((b.x - a.x) * (y - a.y)) / (b.y - a.y) + a.x {
                c = !c;
            }
            j = i;
        }
        c
    }

    /// `endZ(x, y, front)`: NaN outside the silhouette (no surface to land
    /// on: the decal drops it, rather than smearing it along the flank).
    pub fn end_z(&self, x: f64, y: f64, front: bool) -> f64 {
        let span = js::min(1.4, (self.z1 - self.z0) * 0.45);
        let mut zo = if front { self.z1 } else { self.z0 };
        let mut zi = if front {
            self.z1 - span
        } else {
            self.z0 + span
        };
        if self.inside(zo, x, y) {
            return zo;
        }
        if !self.inside(zi, x, y) {
            return f64::NAN;
        }
        for _ in 0..18 {
            let m = (zo + zi) / 2.0;
            if self.inside(m, x, y) {
                zi = m;
            } else {
                zo = m;
            }
        }
        (zo + zi) / 2.0
    }

    /// `map(view, u, v)`.
    pub fn map(&self, view: View, u: f64, v: f64) -> P3 {
        match view {
            View::Top => [u, self.top_y(v, u), v],
            View::Side => [self.side_x(u, v), v, u],
            View::Front => [u, v, self.end_z(u, v, true)],
            View::Rear => [u, v, self.end_z(u, v, false)],
        }
    }

    /// `at(view, u, v, off)`: the surface point and normal (facing the
    /// viewer), lifted `off` along it; `None` where the view ray misses the
    /// body.
    pub fn at(&self, view: View, u: f64, v: f64, off: f64) -> Option<At> {
        let h = 0.005;
        let p = self.map(view, u, v);
        if p[2].is_nan() {
            return None;
        }
        let mut pu = self.map(view, u + h, v);
        let mut pv = self.map(view, u, v + h);
        if pu[2].is_nan() {
            let c = self.map(view, u - h, v);
            pu = [2.0 * p[0] - c[0], 2.0 * p[1] - c[1], 2.0 * p[2] - c[2]];
        }
        if pv[2].is_nan() {
            let c = self.map(view, u, v - h);
            pv = [2.0 * p[0] - c[0], 2.0 * p[1] - c[1], 2.0 * p[2] - c[2]];
        }
        let (ax, ay, az) = (pu[0] - p[0], pu[1] - p[1], pu[2] - p[2]);
        let (bx, by, bz) = (pv[0] - p[0], pv[1] - p[1], pv[2] - p[2]);
        let mut n = [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx];
        let d = view.dir();
        let mut l = kernel::hypot3(n[0], n[1], n[2]);
        if !(l > 1e-10) {
            n = d;
            l = 1.0;
        }
        n = [n[0] / l, n[1] / l, n[2] / l];
        if n[0] * d[0] + n[1] * d[1] + n[2] * d[2] < 0.0 {
            n = [-n[0], -n[1], -n[2]];
        }
        Some(At {
            p: [p[0] + n[0] * off, p[1] + n[1] * off, p[2] + n[2] * off],
            n,
        })
    }
}

/// A decal's colour: a constant, or `col(u, w)` over the decal's grid.
#[derive(Clone, Copy)]
pub enum DCol {
    C(Col),
    F(fn(f64, f64) -> f64),
}

/// `decal`'s options: `{ off = 0.004, nu = 6, nv = 3, col = 1, mirror =
/// false }`.
#[derive(Clone, Copy)]
pub struct DecalOpts {
    pub off: f64,
    pub nu: usize,
    pub nv: usize,
    pub col: DCol,
    pub mirror: bool,
}

impl Default for DecalOpts {
    fn default() -> Self {
        DecalOpts {
            off: 0.004,
            nu: 6,
            nv: 3,
            col: DCol::C(WHITE),
            mirror: false,
        }
    }
}

/// `decal(S, view, poly, opts)`: a filled polygon `[[u, v], ...]` in a
/// view, meshed in rows along v (each row spans the polygon's u-extent, so
/// shapes should be u-convex per row).
pub fn decal(s: &Surf, view: View, poly: &[P2], o: DecalOpts) -> BufferGeometry {
    let (nu, nv) = (o.nu, o.nv);
    let mut vmin = f64::INFINITY;
    let mut vmax = f64::NEG_INFINITY;
    for &[_, v] in poly {
        vmin = js::min(vmin, v);
        vmax = js::max(vmax, v);
    }
    let mut vs = Vec::new();
    for i in 0..=nv {
        vs.push(vmin + ((vmax - vmin) * i as f64) / nv as f64);
    }
    for &[_, v] in poly {
        vs.push(v);
    }
    sort_asc(&mut vs);
    let mut rows: Vec<(f64, Vec<Option<At>>)> = Vec::new();
    for v in vs {
        if let Some(last) = rows.last()
            && v - last.0 < 1e-4
        {
            continue;
        }
        let mut us = Vec::new();
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            if (a[1] - b[1]).abs() < 1e-9 {
                if (a[1] - v).abs() < 1e-6 {
                    us.push(a[0]);
                    us.push(b[0]);
                }
                continue;
            }
            if v >= js::min(a[1], b[1]) - 1e-7 && v <= js::max(a[1], b[1]) + 1e-7 {
                us.push(a[0] + ((b[0] - a[0]) * (v - a[1])) / (b[1] - a[1]));
            }
        }
        if us.is_empty() {
            continue;
        }
        let u0 = js::min_n(&us);
        let u1 = js::max_n(&us);
        let mut pts = Vec::with_capacity(nu + 1);
        for j in 0..=nu {
            pts.push(s.at(view, u0 + ((u1 - u0) * j as f64) / nu as f64, v, o.off));
        }
        rows.push((v, pts));
    }
    let mut t = Tris::default();
    let cf = |u: f64, w: f64| -> Col {
        match o.col {
            DCol::C(c) => c,
            DCol::F(f) => grey(f(u, w)),
        }
    };
    let last = rows.len() as f64 - 1.0;
    for i in 0..rows.len().saturating_sub(1) {
        let (a, b) = (&rows[i].1, &rows[i + 1].1);
        for j in 0..nu {
            let (Some(a0), Some(a1), Some(b0), Some(b1)) = (a[j], a[j + 1], b[j], b[j + 1]) else {
                continue;
            };
            let (jf, nuf, i0, i1) = (j as f64, nu as f64, i as f64, (i + 1) as f64);
            t.quad(
                a0.p,
                a1.p,
                b1.p,
                b0.p,
                a0.n,
                a1.n,
                b1.n,
                b0.n,
                cf(jf / nuf, i0 / last),
                cf((jf + 1.0) / nuf, i0 / last),
                cf((jf + 1.0) / nuf, i1 / last),
                cf(jf / nuf, i1 / last),
            );
        }
    }
    if o.mirror {
        t.mirror_x();
    }
    t.geo()
}

/// `ribbon`'s options: `{ off = 0.004, col = 1, closed = false, mirror =
/// false, step = 0.04 }`.
#[derive(Clone, Copy)]
pub struct RibbonOpts {
    pub off: f64,
    pub col: Col,
    pub closed: bool,
    pub mirror: bool,
    pub step: f64,
}

impl Default for RibbonOpts {
    fn default() -> Self {
        RibbonOpts {
            off: 0.004,
            col: WHITE,
            closed: false,
            mirror: false,
            step: 0.04,
        }
    }
}

/// `ribbon(S, view, pts, w, opts)`: a ribbon decal of width w along a
/// polyline in a view. (Every caller passes a number for `w`.)
pub fn ribbon(s: &Surf, view: View, pts: &[P2], w: f64, o: RibbonOpts) -> BufferGeometry {
    let mut q: Vec<P2> = Vec::new();
    let segs = if o.closed { pts.len() } else { pts.len() - 1 };
    for i in 0..segs {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        let n = js::max(
            1.0,
            (kernel::hypot(b[0] - a[0], b[1] - a[1]) / o.step).ceil(),
        );
        let mut k = 0.0;
        while k < n {
            q.push([
                a[0] + ((b[0] - a[0]) * k) / n,
                a[1] + ((b[1] - a[1]) * k) / n,
            ]);
            k += 1.0;
        }
    }
    if !o.closed {
        q.push(pts[pts.len() - 1]);
    }
    let len = q.len();
    let mut ls = Vec::with_capacity(len);
    let mut rs = Vec::with_capacity(len);
    for i in 0..len {
        let p = q[i];
        let pa = q[if i > 0 {
            i - 1
        } else if o.closed {
            len - 1
        } else {
            0
        }];
        let pb = q[if i < len - 1 {
            i + 1
        } else if o.closed {
            0
        } else {
            len - 1
        }];
        let mut tu = pb[0] - pa[0];
        let mut tv = pb[1] - pa[1];
        let l = js::or(kernel::hypot(tu, tv), 1.0);
        tu /= l;
        tv /= l;
        let hw = w / 2.0;
        ls.push(s.at(view, p[0] - tv * hw, p[1] + tu * hw, o.off));
        rs.push(s.at(view, p[0] + tv * hw, p[1] - tu * hw, o.off));
    }
    let mut t = Tris::default();
    let end = if o.closed { len } else { len - 1 };
    for i in 0..end {
        let j = (i + 1) % len;
        let (Some(li), Some(ri), Some(lj), Some(rj)) = (ls[i], rs[i], ls[j], rs[j]) else {
            continue;
        };
        t.quad(
            li.p, ri.p, rj.p, lj.p, li.n, ri.n, rj.n, lj.n, o.col, o.col, o.col, o.col,
        );
    }
    if o.mirror {
        t.mirror_x();
    }
    t.geo()
}

/// `expandPoly(poly, d)`: offset a polygon outward by d (miter, clamped at
/// sharp corners).
pub fn expand_poly(poly: &[P2], d: f64) -> Vec<P2> {
    let n = poly.len();
    let mut area = 0.0;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        area += a[0] * b[1] - b[0] * a[1];
    }
    let s = if area > 0.0 { 1.0 } else { -1.0 };
    poly.iter()
        .enumerate()
        .map(|(i, &p)| {
            let (a, b) = (poly[(i + n - 1) % n], poly[(i + 1) % n]);
            let mut e1 = [p[0] - a[0], p[1] - a[1]];
            let mut e2 = [b[0] - p[0], b[1] - p[1]];
            let l1 = js::or(kernel::hypot(e1[0], e1[1]), 1.0);
            let l2 = js::or(kernel::hypot(e2[0], e2[1]), 1.0);
            e1 = [e1[0] / l1, e1[1] / l1];
            e2 = [e2[0] / l2, e2[1] / l2];
            let n1 = [e1[1], -e1[0]];
            let n2 = [e2[1], -e2[0]];
            let mut nx = n1[0] + n2[0];
            let mut ny = n1[1] + n2[1];
            let l = js::or(kernel::hypot(nx, ny), 1.0);
            nx /= l;
            ny /= l;
            let k = d / js::max(0.4, nx * n1[0] + ny * n1[1]);
            [p[0] + s * nx * k, p[1] + s * ny * k]
        })
        .collect()
}

/// `ellipse(cu, cv, ru, rv, n = 12, rot = 0)`.
pub fn ellipse(cu: f64, cv: f64, ru: f64, rv: f64, n: usize) -> Vec<P2> {
    let rot = 0.0;
    (0..n)
        .map(|i| {
            let a = (i as f64 / n as f64) * 2.0 * PI;
            let x = kernel::cos(a) * ru;
            let y = kernel::sin(a) * rv;
            [
                cu + x * kernel::cos(rot) - y * kernel::sin(rot),
                cv + x * kernel::sin(rot) + y * kernel::cos(rot),
            ]
        })
        .collect()
}

// ── sweeps and lathes ───────────────────────────────────────────────────

/// A sweep's axis: `'x'` (section [z, y]) or `'z'` (section [x, y]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Z,
}

/// `sweep(sec, axis, a0, a1, { n = 1, caps = true, col = 1 })`: a closed
/// 2D section swept along an axis (smooth around, flat end caps). No
/// caller passes `fn` or `caps: false`, so the section is never tapered
/// (`s` 1, `du` and `dv` 0, applied as the JS applies them) and the caps
/// are always made.
pub fn sweep(sec: &[P2], axis: Axis, a0: f64, a1: f64, n: usize) -> BufferGeometry {
    let col = WHITE;
    let mut t = Tris::default();
    let mut cu = 0.0;
    let mut cv = 0.0;
    for &[u, v] in sec {
        cu += u;
        cv += v;
    }
    cu /= sec.len() as f64;
    cv /= sec.len() as f64;
    let p3 = |a: f64, [u, v]: P2| -> P3 {
        match axis {
            Axis::X => [a, v, u],
            Axis::Z => [u, v, a],
        }
    };
    let n3 = |u: f64, v: f64| -> P3 {
        match axis {
            Axis::X => [0.0, v, u],
            Axis::Z => [u, v, 0.0],
        }
    };
    let m = sec.len();
    let ring = |_t: f64| -> Vec<P2> {
        let (s, du, dv) = (1.0, 0.0, 0.0);
        sec.iter()
            .map(|&[u, v]| [cu + (u - cu) * s + du, cv + (v - cv) * s + dv])
            .collect()
    };
    let mut rings: Vec<(f64, Vec<P2>)> = Vec::new();
    for i in 0..=n {
        rings.push((
            a0 + ((a1 - a0) * i as f64) / n as f64,
            ring(i as f64 / n as f64),
        ));
    }
    let norms: Vec<Vec<P3>> = rings
        .iter()
        .map(|(_, r)| {
            r.iter()
                .enumerate()
                .map(|(k, p)| {
                    let (pa, pb) = (r[(k + m - 1) % m], r[(k + 1) % m]);
                    let mut nu = pb[1] - pa[1];
                    let mut nv = -(pb[0] - pa[0]);
                    if nu * (p[0] - cu) + nv * (p[1] - cv) < 0.0 {
                        nu = -nu;
                        nv = -nv;
                    }
                    let l = js::or(kernel::hypot(nu, nv), 1.0);
                    n3(nu / l, nv / l)
                })
                .collect()
        })
        .collect();
    for i in 0..n {
        let (a, b) = (&rings[i], &rings[i + 1]);
        for k in 0..m {
            let k2 = (k + 1) % m;
            t.quad(
                p3(a.0, a.1[k]),
                p3(a.0, a.1[k2]),
                p3(b.0, b.1[k2]),
                p3(b.0, b.1[k]),
                norms[i][k],
                norms[i][k2],
                norms[i + 1][k2],
                norms[i + 1][k],
                col,
                col,
                col,
                col,
            );
        }
    }
    for (ri, dir) in [(0usize, -1.0f64), (n, 1.0)] {
        let r = &rings[ri];
        let mut contour: Vec<Vector2> = r.1.iter().map(|&[u, v]| Vector2::new(u, v)).collect();
        let faces = triangulate_shape(&mut contour, &mut []);
        let nn = match axis {
            Axis::X => [dir, 0.0, 0.0],
            Axis::Z => [0.0, 0.0, dir],
        };
        for [a, b, c] in faces {
            t.tri(
                p3(r.0, r.1[a]),
                p3(r.0, r.1[b]),
                p3(r.0, r.1[c]),
                nn,
                nn,
                nn,
                col,
                col,
                col,
            );
        }
    }
    t.geo()
}

/// The colours of `latheX`: one grey level, or one per profile point.
#[derive(Clone, Copy)]
pub enum Greys<'a> {
    One(f64),
    Per(&'a [f64]),
}

/// `latheX(prof, seg, cols, phi0 = 0, phiLen = 2π)`: a lathe about the X
/// axis, `prof = [[radius, x], ...]`.
pub fn lathe_x(prof: &[P2], seg: f64, cols: Greys) -> BufferGeometry {
    lathe_x_arc(prof, seg, cols, 0.0, 2.0 * PI)
}

/// `latheX` with its sweep.
pub fn lathe_x_arc(prof: &[P2], seg: f64, cols: Greys, phi0: f64, phi_len: f64) -> BufferGeometry {
    let pts: Vec<Vector2> = prof.iter().map(|&[r, x]| Vector2::new(r, x)).collect();
    let mut g = lathe_geometry(&pts, seg, phi0, phi_len);
    g.rotate_z(-PI / 2.0);
    let n = g.position().count();
    let np = prof.len();
    let mut c = vec![0f32; n * 3];
    for i in 0..n {
        let v = match cols {
            Greys::Per(a) => a[i % np],
            Greys::One(v) => v,
        } as f32;
        c[i * 3] = v;
        c[i * 3 + 1] = v;
        c[i * 3 + 2] = v;
    }
    g.set_attribute("color", BufferAttribute::from_f32(c, 3));
    g
}

/// `airfoil(chord, thick, n = 7, camber = 0.05)`: a section [z, y] (leading
/// edge forward at +chord/2), cambered for downforce.
pub fn airfoil(chord: f64, thick: f64, n: usize) -> Vec<P2> {
    let camber = 0.05;
    let mut up = Vec::new();
    let mut lo = Vec::new();
    for i in 0..=n {
        let x = (1.0 - kernel::cos((i as f64 / n as f64) * PI)) / 2.0;
        let yt = 5.0
            * thick
            * (0.2969 * f64::sqrt(x) - 0.126 * x - 0.3516 * x * x + 0.2843 * kernel::pow(x, 3.0)
                - 0.1036 * kernel::pow(x, 4.0))
            * chord;
        let yc = -camber * chord * 4.0 * x * (1.0 - x);
        up.push([(0.5 - x) * chord, yc + yt]);
        lo.push([(0.5 - x) * chord, yc - yt]);
    }
    let mut out: Vec<P2> = up[..n].to_vec();
    for i in (1..=n).rev() {
        out.push(lo[i]);
    }
    out
}

// ── parts ───────────────────────────────────────────────────────────────

/// `shapeFrom(pts)`.
fn shape_from(pts: &[P2]) -> Shape {
    let mut s = Shape::new();
    s.move_to(pts[0][0], pts[0][1]);
    for p in &pts[1..] {
        s.line_to(p[0], p[1]);
    }
    s.close_path();
    s
}

/// `extrude(pts, width, bevel = 0.01)`: a (z, y) outline extruded across
/// `width` centred on X (flat-shaded).
pub fn extrude(pts: &[P2], width: f64, bevel: f64) -> BufferGeometry {
    let depth = js::max(0.005, width - 2.0 * bevel);
    let mut g = extrude_geometry(
        &[shape_from(pts)],
        &ExtrudeOptions {
            depth,
            steps: 1.0,
            curve_segments: 1.0,
            bevel_enabled: bevel > 0.0,
            bevel_thickness: bevel,
            bevel_size: bevel,
            bevel_offset: -bevel,
            bevel_segments: 1.0,
        },
    );
    g.translate(0.0, 0.0, -depth / 2.0);
    g.rotate_y(-PI / 2.0);
    g.compute_vertex_normals();
    g
}

/// `prep`'s colour: the JS default `1` (left alone), or a grey level or
/// array the colours are multiplied by.
pub type PCol = Option<Col>;

/// A grey level as the JS passes it: `1` is the default.
pub fn pc(v: f64) -> PCol {
    if v == 1.0 { None } else { Some(grey(v)) }
}

/// `prep(geom, pos = [0, 0, 0], rot = [0, 0, 0], col = 1)`: non-indexed,
/// only `position`, `normal` and `color`, rotated (YXZ) and placed, no
/// groups.
pub fn prep(geom: BufferGeometry, pos: P3, rot: P3, col: PCol) -> BufferGeometry {
    let mut g = if geom.index.is_some() {
        geom.to_non_indexed()
    } else {
        geom
    };
    let keys: Vec<String> = g.attributes.iter().map(|(k, _)| k.clone()).collect();
    for k in keys {
        if k != "position" && k != "normal" && k != "color" {
            g.delete_attribute(&k);
        }
    }
    if !g.has_attribute("normal") {
        g.compute_vertex_normals();
    }
    if !g.has_attribute("color") || col.is_some() {
        let n = g.position().count();
        let cc = col.unwrap_or(WHITE);
        let old = g.get_attribute("color").map(|a| a.array.clone());
        let mut c = vec![0f32; n * 3];
        for i in 0..n {
            for e in 0..3 {
                let o = old.as_ref().map_or(1.0, |a| a.get(i * 3 + e));
                c[i * 3 + e] = (o * cc[e]) as f32;
            }
        }
        g.set_attribute("color", BufferAttribute::from_f32(c, 3));
    }
    if is_truthy(rot[0]) || is_truthy(rot[1]) || is_truthy(rot[2]) {
        g.apply_matrix4(&Matrix4::make_rotation_from_euler(&Euler::with_order(
            rot[0],
            rot[1],
            rot[2],
            EulerOrder::YXZ,
        )));
    }
    g.translate(pos[0], pos[1], pos[2]);
    g.groups.clear();
    g
}

/// A number's truthiness (`0`, `-0` and NaN are false).
fn is_truthy(x: f64) -> bool {
    !(x == 0.0 || x.is_nan())
}

/// `boxUV(g, tile = 0.05)`: box-projected UVs (5 cm tiles) for textured
/// buckets such as carbon.
pub fn box_uv(g: &mut BufferGeometry) {
    let tile = 0.05;
    let p = g.position().as_f32().expect("f32").to_vec();
    let n = g
        .get_attribute("normal")
        .and_then(BufferAttribute::as_f32)
        .expect("normals")
        .to_vec();
    let count = p.len() / 3;
    let mut uv = vec![0f32; count * 2];
    for i in 0..count {
        let ax = f64::from(n[i * 3]).abs();
        let ay = f64::from(n[i * 3 + 1]).abs();
        let az = f64::from(n[i * 3 + 2]).abs();
        let pf = |k: usize| f64::from(p[i * 3 + k]);
        let (u, v) = if ax >= ay && ax >= az {
            (pf(2), pf(1))
        } else if ay >= az {
            (pf(0), pf(2))
        } else {
            (pf(0), pf(1))
        };
        uv[i * 2] = (u / tile) as f32;
        uv[i * 2 + 1] = (v / tile) as f32;
    }
    g.set_attribute("uv", BufferAttribute::from_f32(uv, 2));
}

/// No rotation.
pub const R0: P3 = [0.0, 0.0, 0.0];
/// The origin.
pub const O: P3 = [0.0, 0.0, 0.0];

/// `Parts`: geometry gathered into per-material buckets, merged by
/// `build()`.
pub struct Parts {
    b: Vec<(&'static str, Vec<BufferGeometry>)>,
    pub hi: bool,
}

impl Parts {
    pub fn new(hi: bool) -> Parts {
        Parts { b: Vec::new(), hi }
    }

    /// `add(bucket, geom, pos, rot, col)`: an empty geometry is skipped.
    pub fn add(&mut self, bucket: &'static str, geom: BufferGeometry, pos: P3, rot: P3, col: PCol) {
        if geom.vertex_count() == 0 {
            return;
        }
        let g = prep(geom, pos, rot, col);
        let s = bucket_slot(&mut self.b, bucket, Vec::new);
        self.b[s].1.push(g);
    }

    /// `add(bucket, geom)`.
    pub fn add0(&mut self, bucket: &'static str, geom: BufferGeometry) {
        self.add(bucket, geom, O, R0, None);
    }

    /// `addAll(map)`.
    pub fn add_all(&mut self, geo: Buckets) {
        for (k, g) in geo {
            self.add0(k, g);
        }
    }

    /// `box(bucket, w, h, d, pos, rot, col)`.
    #[allow(clippy::too_many_arguments)]
    pub fn box_(
        &mut self,
        bucket: &'static str,
        w: f64,
        h: f64,
        d: f64,
        pos: P3,
        rot: P3,
        col: PCol,
    ) {
        self.add(
            bucket,
            crate::three_geom::box_geometry(w, h, d, 1.0, 1.0, 1.0),
            pos,
            rot,
            col,
        );
    }

    /// `pair(bucket, make, x, y, z, rot = [0, 0, 0], col)`: a mirrored pair
    /// across X.
    #[allow(clippy::too_many_arguments)]
    pub fn pair(
        &mut self,
        bucket: &'static str,
        make: impl Fn() -> BufferGeometry,
        x: f64,
        y: f64,
        z: f64,
        rot: P3,
        col: PCol,
    ) {
        self.add(bucket, make(), [x, y, z], rot, col);
        self.add(bucket, make(), [-x, y, z], [rot[0], -rot[1], -rot[2]], col);
    }

    /// `cyl(bucket, rt, rb, h, seg, pos, rot, col)`.
    #[allow(clippy::too_many_arguments)]
    pub fn cyl(
        &mut self,
        bucket: &'static str,
        rt: f64,
        rb: f64,
        h: f64,
        seg: f64,
        pos: P3,
        rot: P3,
        col: PCol,
    ) {
        self.add(bucket, cylinder(rt, rb, h, seg), pos, rot, col);
    }

    /// `build()`: each bucket merged, normals fixed, carbon given box UVs
    /// (high detail), colours dropped (low detail), bounding sphere.
    pub fn build(self) -> Buckets {
        let mut out = Vec::new();
        for (k, list) in self.b {
            let refs: Vec<&BufferGeometry> = list.iter().collect();
            let mut g = merge_geometries(&refs, false).expect("a bucket's parts merge");
            fix_normals(&mut g, [0.0, 1.0, 0.0]);
            if k == "carbon" && self.hi {
                box_uv(&mut g);
            }
            if !self.hi {
                g.delete_attribute("color");
            }
            g.compute_bounding_sphere();
            out.push((k, g));
        }
        out
    }
}

/// `new THREE.CylinderGeometry(rt, rb, h, seg)`.
pub fn cylinder(rt: f64, rb: f64, h: f64, seg: f64) -> BufferGeometry {
    crate::three_geom::cylinder_geometry(rt, rb, h, seg, 1.0, false, 0.0, 2.0 * PI)
}

/// `Box(w, h, d)`: a maker of boxes.
pub fn bx(w: f64, h: f64, d: f64) -> impl Fn() -> BufferGeometry {
    move || crate::three_geom::box_geometry(w, h, d, 1.0, 1.0, 1.0)
}

/// `Disc(r, t, seg)`: a maker of discs.
pub fn disc(r: f64, t: f64, seg: f64) -> impl Fn() -> BufferGeometry {
    move || cylinder(r, r, t, seg)
}
