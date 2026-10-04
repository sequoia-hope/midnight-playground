//! Strokes as Chrome's GPU canvas tessellates them (DECISIONS D652).
//!
//! Skia's GPU stroker (`StrokeTessellator` and `GrStrokeTessellationShader`)
//! turns every segment of a path into a strip of quads between "edges":
//! lines across the stroke, at the curve's point and normal for a set of
//! parameters chosen so that both the curve (Wang's formula, a quarter
//! pixel) and the turning of its normal (radial segments, a quarter pixel
//! at the stroke's radius) are followed closely. Joins are fans of the same
//! edges around the junction, on the outer side; round caps are circles of
//! such edges. The triangles are rasterised multisampled, so a pixel's
//! coverage is the samples inside any of them.
//!
//! This ports the shader's arithmetic in single precision, with the
//! kernel's transcendental functions, and returns device-space triangles
//! snapped to the GPU's 1/256 px grid.

use mr_math::kernel;
use tiny_skia::{LineCap, LineJoin, Path, PathSegment};

use crate::Matrix;

/// Skia's `kPrecision`: curves are followed within 1/4 px.
const PRECISION: f32 = 4.0;
/// Skia's `kMaxParametricSegments`: a longer curve is chopped first.
const MAX_PARAMETRIC_SEGMENTS: f32 = 32.0;
/// The binary search's depth (`kMaxResolveLevel`).
const MAX_RESOLVE_LEVEL: i32 = 5;
const PI: f32 = std::f32::consts::PI;

type P = (f32, f32);

/// A device-space triangle.
pub type Triangle = [P; 3];

fn sub(a: P, b: P) -> P {
    (a.0 - b.0, a.1 - b.1)
}

fn dot(a: P, b: P) -> f32 {
    a.0 * b.0 + a.1 * b.1
}

fn cross(a: P, b: P) -> f32 {
    a.0 * b.1 - a.1 * b.0
}

fn scale(a: P, k: f32) -> P {
    (a.0 * k, a.1 * k)
}

fn add(a: P, b: P) -> P {
    (a.0 + b.0, a.1 + b.1)
}

fn mix(a: P, b: P, t: f32) -> P {
    ((b.0 - a.0) * t + a.0, (b.1 - a.1) * t + a.1)
}

fn acos(x: f32) -> f32 {
    kernel::acos(x as f64) as f32
}

fn normalize(a: P) -> P {
    let l = (a.0 * a.0 + a.1 * a.1).sqrt();
    (a.0 / l, a.1 / l)
}

/// `robust_normalize_diff(a, b)`.
fn robust_normalize_diff(a: P, b: P) -> P {
    let d = sub(a, b);
    if d == (0.0, 0.0) {
        return (0.0, 0.0);
    }
    let inv = 1.0 / d.0.abs().max(d.1.abs());
    normalize(scale(d, inv))
}

/// The stroke's settings and transform.
pub struct StrokeStyle {
    /// Half the line width, in user space.
    pub radius: f32,
    pub join: LineJoin,
    pub miter_limit: f32,
    pub cap: LineCap,
}

/// One segment of a contour as a cubic patch (Skia writes lines as
/// `p0, p0, p1, p1` and quadratics as their cubic).
#[derive(Clone, Copy)]
struct Patch {
    p: [P; 4],
    line: bool,
}

impl Patch {
    fn first(&self) -> P {
        self.p[0]
    }

    fn last(&self) -> P {
        self.p[3]
    }

    /// The control point the next patch's join tangent comes from
    /// (`TangentPoint(last, p2, p1, p0)`).
    fn join_control(&self) -> P {
        let [p0, p1, p2, p3] = self.p;
        if p3 != p2 {
            p2
        } else if p2 != p1 {
            p1
        } else {
            p0
        }
    }

    fn tan0(&self) -> P {
        let [p0, p1, p2, p3] = self.p;
        let to = if p0 == p1 {
            if p1 == p2 { p3 } else { p2 }
        } else {
            p1
        };
        robust_normalize_diff(to, p0)
    }
}

struct Tessellator<'a> {
    m: &'a Matrix,
    radius: f32,
    /// `NUM_RADIAL_SEGMENTS_PER_RADIAN`.
    radial_per_rad: f32,
    join: LineJoin,
    miter_limit: f32,
    out: Vec<Triangle>,
}

impl Tessellator<'_> {
    fn device(&self, p: P) -> P {
        let (x, y) = self.m.apply(p.0 as f64, p.1 as f64);
        let snap = |v: f64| ((v as f32) * 256.0).round() / 256.0;
        (snap(x), snap(y))
    }

    fn tri(&mut self, a: P, b: P, c: P) {
        let t = [self.device(a), self.device(b), self.device(c)];
        self.out.push(t);
    }

    /// The strip between consecutive edges (each a pair of points, the
    /// +outset side first).
    fn strip(&mut self, edges: &[(P, P)]) {
        for w in edges.windows(2) {
            let ((a0, a1), (b0, b1)) = (w[0], w[1]);
            self.tri(a0, a1, b0);
            self.tri(a1, b0, b1);
        }
    }

    fn vector_scale(&self, v: P) -> P {
        let (x, y) = self.m.apply_vec(v.0 as f64, v.1 as f64);
        (x as f32, y as f32)
    }

    /// `wangs_formula_cubic(PRECISION, p0..p3, AFFINE_MATRIX)`.
    fn wangs_cubic(&self, p: &[P; 4]) -> f32 {
        let d0 = self.vector_scale(add(add(scale(p[1], -2.0), p[2]), p[0]));
        let d1 = self.vector_scale(add(add(scale(p[2], -2.0), p[3]), p[1]));
        let m = dot(d0, d0).max(dot(d1, d1));
        (0.75 * PRECISION * m.sqrt()).sqrt().ceil().max(1.0)
    }

    /// The body of one patch: its edges from parameter 0 to 1.
    fn body(&mut self, patch: &Patch) {
        let [p0, p1, p2, p3] = patch.p;
        let num_parametric = if patch.line || (p0 == p1 && p2 == p3) {
            1.0
        } else {
            self.wangs_cubic(&patch.p)
        };
        let mut tan0 = patch.tan0();
        let mut tan1 = robust_normalize_diff(
            p3,
            if p3 == p2 {
                if p2 == p1 { p0 } else { p1 }
            } else {
                p2
            },
        );
        if tan0 == (0.0, 0.0) {
            // A point: a stroke-width circle.
            tan0 = (1.0, 0.0);
            tan1 = (-1.0, 0.0);
        }
        let turn = cross(sub(p2, p0), sub(p3, p1));
        let cos_theta = dot(tan0, tan1).clamp(-1.0, 1.0);
        let mut rotation = acos(cos_theta);
        if turn < 0.0 {
            rotation = -rotation;
        }
        let num_radial = (rotation.abs() * self.radial_per_rad).ceil().max(1.0);
        let rads_per_segment = rotation / num_radial;
        let num_combined = num_parametric + num_radial - 1.0;
        let r = self.radius;
        let mut edges = Vec::with_capacity(num_combined as usize + 1);
        for id in 0..=num_combined as i32 {
            let id = id as f32;
            let (tangent, coord) = if id != 0.0 && id < num_combined {
                edge_point(&patch.p, id, num_parametric, rads_per_segment, tan0)
            } else if id == 0.0 {
                (tan0, p0)
            } else {
                (tan1, p3)
            };
            let ortho = (tangent.1, -tangent.0);
            edges.push((add(coord, scale(ortho, r)), sub(coord, scale(ortho, r))));
        }
        self.strip(&edges);
    }

    /// The join at `p0` from the incoming tangent `prev` to `tan0`.
    fn join(&mut self, p0: P, prev: P, tan0: P) {
        let turn = cross(prev, tan0);
        let cos_theta = dot(prev, tan0).clamp(-1.0, 1.0);
        let mut rotation = acos(cos_theta);
        if turn < 0.0 {
            rotation = -rotation;
        }
        let num_radial = match self.join {
            LineJoin::Round => (rotation.abs() * self.radial_per_rad).ceil().max(1.0),
            LineJoin::Bevel => 1.0,
            _ => 2.0,
        };
        let rads_per_segment = rotation / num_radial;
        let nearly_parallel = turn.abs() < 1e-2;
        let sides: &[f32] = if !nearly_parallel || cos_theta < 0.0 {
            if turn < 0.0 { &[-1.0] } else { &[1.0] }
        } else {
            &[1.0, -1.0]
        };
        let angle0 = {
            let a = acos(prev.0.clamp(-1.0, 1.0));
            if prev.1 >= 0.0 { a } else { -a }
        };
        let r = self.radius;
        for &side in sides {
            let mut pts = Vec::with_capacity(num_radial as usize + 1);
            for k in 0..=num_radial as i32 {
                let k = k as f32;
                let mut outset = side;
                let tangent = if k == 0.0 {
                    prev
                } else if k >= num_radial {
                    tan0
                } else {
                    let a = (k * rads_per_segment + angle0) as f64;
                    (kernel::cos(a) as f32, kernel::sin(a) as f32)
                };
                if matches!(self.join, LineJoin::Miter | LineJoin::MiterClip) && k == 1.0 {
                    outset *= miter_extent(cos_theta, self.miter_limit);
                }
                let ortho = (tangent.1, -tangent.0);
                pts.push(add(p0, scale(ortho, r * outset)));
            }
            for w in pts.windows(2) {
                self.tri(p0, w[0], w[1]);
            }
        }
    }

    /// A round cap: a stroke-width circle (a half-turn point stroke).
    fn circle(&mut self, p: P) {
        let patch = Patch {
            p: [p; 4],
            line: true,
        };
        self.body(&patch);
    }
}

/// `miter_extent(cosTheta, miterLimit)`.
fn miter_extent(cos_theta: f32, miter_limit: f32) -> f32 {
    let x = cos_theta * 0.5 + 0.5;
    if x * miter_limit * miter_limit >= 1.0 {
        1.0 / x.sqrt()
    } else {
        x.sqrt()
    }
}

/// The point and tangent of edge `id` of a curve's strip: the shader's
/// search for the last parametric edge at or before it, then the radial
/// edge's parameter, whichever is later.
fn edge_point(p: &[P; 4], id: f32, num_parametric: f32, rads_per_segment: f32, tan0: P) -> (P, P) {
    let [p0, p1, p2, p3] = *p;
    let c = sub(p1, p0);
    let d = sub(p3, p0);
    let e = sub(p2, p1);
    let b = sub(e, c);
    let a = add(scale(e, -3.0), d);
    let b_ = scale(b, num_parametric * 2.0);
    let c_ = scale(c, num_parametric * num_parametric);
    let mut last_parametric = 0.0f32;
    let max_parametric = (num_parametric - 1.0).min(id);
    let neg_abs_rads = -rads_per_segment.abs();
    let max_rotation0 = (1.0 + id) * rads_per_segment.abs();
    for exp in (0..MAX_RESOLVE_LEVEL).rev() {
        let test = last_parametric + (1 << exp) as f32;
        if test <= max_parametric {
            let mut tt = add(scale(a, test), b_);
            tt = add(scale(tt, test), c_);
            let cos_rotation = dot(normalize(tt), tan0);
            let max_rotation = (test * neg_abs_rads + max_rotation0).min(PI);
            if cos_rotation >= kernel::cos(max_rotation as f64) as f32 {
                last_parametric = test;
            }
        }
    }
    let parametric_t = last_parametric / num_parametric;
    let last_radial = id - last_parametric;
    let angle0 = {
        let a0 = acos(tan0.0.clamp(-1.0, 1.0));
        if tan0.1 >= 0.0 { a0 } else { -a0 }
    };
    let radial_angle = (last_radial * rads_per_segment + angle0) as f64;
    let mut tangent = (
        kernel::cos(radial_angle) as f32,
        kernel::sin(radial_angle) as f32,
    );
    let norm = (-tangent.1, tangent.0);
    let (qa, b_over_2, qc) = (dot(norm, a), dot(norm, b), dot(norm, c));
    let discr_over_4 = (b_over_2 * b_over_2 - qa * qc).max(0.0);
    let mut q = discr_over_4.sqrt();
    if b_over_2 > 0.0 {
        q = -q;
    }
    q -= b_over_2;
    let _5qa = -0.5 * q * qa;
    let root = if (q * q + _5qa).abs() < (qa * qc + _5qa).abs() {
        (q, qa)
    } else {
        (qc, q)
    };
    let mut radial_t = if root.1 != 0.0 { root.0 / root.1 } else { 0.0 };
    radial_t = radial_t.clamp(0.0, 1.0);
    if last_radial == 0.0 {
        radial_t = 0.0;
    }
    let t = parametric_t.max(radial_t);
    let ab = mix(p0, p1, t);
    let bc = mix(p1, p2, t);
    let cd = mix(p2, p3, t);
    let abc = mix(ab, bc, t);
    let bcd = mix(bc, cd, t);
    let abcd = mix(abc, bcd, t);
    if t != radial_t {
        tangent = robust_normalize_diff(bcd, abc);
    }
    (tangent, abcd)
}

/// Chops a cubic patch into `n` pieces of equal parameter.
fn chop(p: &[P; 4], n: usize, out: &mut Vec<Patch>) {
    // Sub-curve [t0, t1] by blossoming the cubic.
    let blossom = |u: f32, v: f32, w: f32| {
        let ab = |t: f32, a: P, b: P| mix(a, b, t);
        let l1 = [ab(u, p[0], p[1]), ab(u, p[1], p[2]), ab(u, p[2], p[3])];
        let l2 = [ab(v, l1[0], l1[1]), ab(v, l1[1], l1[2])];
        ab(w, l2[0], l2[1])
    };
    for i in 0..n {
        let t0 = i as f32 / n as f32;
        let t1 = (i + 1) as f32 / n as f32;
        out.push(Patch {
            p: [
                if i == 0 { p[0] } else { blossom(t0, t0, t0) },
                blossom(t0, t0, t1),
                blossom(t0, t1, t1),
                if i + 1 == n {
                    p[3]
                } else {
                    blossom(t1, t1, t1)
                },
            ],
            line: false,
        });
    }
}

/// The triangles of a user-space path's stroke, in device space.
pub fn stroke_triangles(path: &Path, style: &StrokeStyle, m: &Matrix) -> Vec<Triangle> {
    let max_scale = ((m.a * m.a + m.b * m.b).max(m.c * m.c + m.d * m.d)).sqrt() as f32;
    let cos_theta = 1.0 - (1.0 / PRECISION) / (max_scale * style.radius);
    let radial_per_rad = 0.5 / acos(cos_theta.max(-1.0));
    let mut t = Tessellator {
        m,
        radius: style.radius,
        radial_per_rad,
        join: style.join,
        miter_limit: style.miter_limit,
        out: Vec::new(),
    };
    // Contours as lists of patches.
    let mut contours: Vec<(Vec<Patch>, bool)> = Vec::new();
    let mut cur: Vec<Patch> = Vec::new();
    let mut start = (0.0, 0.0);
    let mut at = (0.0, 0.0);
    let chop_curve = |p: [P; 4], line: bool, t: &Tessellator, cur: &mut Vec<Patch>| {
        if line {
            cur.push(Patch { p, line: true });
            return;
        }
        let n = t.wangs_cubic(&p);
        if n > MAX_PARAMETRIC_SEGMENTS {
            let pieces = (n / MAX_PARAMETRIC_SEGMENTS).ceil() as usize;
            chop(&p, pieces, cur);
        } else {
            cur.push(Patch { p, line: false });
        }
    };
    for seg in path.segments() {
        match seg {
            PathSegment::MoveTo(p) => {
                if !cur.is_empty() {
                    contours.push((std::mem::take(&mut cur), false));
                }
                start = (p.x, p.y);
                at = start;
            }
            PathSegment::LineTo(p) => {
                let p = (p.x, p.y);
                if p != at {
                    chop_curve([at, at, p, p], true, &t, &mut cur);
                }
                at = p;
            }
            PathSegment::QuadTo(c, p) => {
                let (c, p) = ((c.x, c.y), (p.x, p.y));
                if !(p == c && c == at) {
                    // The quadratic as a cubic (`writeQuadPatch`).
                    let q1 = mix(at, c, 2.0 / 3.0);
                    let q2 = mix(p, c, 2.0 / 3.0);
                    chop_curve([at, q1, q2, p], false, &t, &mut cur);
                }
                at = p;
            }
            PathSegment::CubicTo(c1, c2, p) => {
                let (c1, c2, p) = ((c1.x, c1.y), (c2.x, c2.y), (p.x, p.y));
                if !(p == c2 && c2 == c1 && c1 == at) {
                    chop_curve([at, c1, c2, p], false, &t, &mut cur);
                }
                at = p;
            }
            PathSegment::Close => {
                if !cur.is_empty() {
                    if at != start {
                        cur.push(Patch {
                            p: [at, at, start, start],
                            line: true,
                        });
                    }
                    contours.push((std::mem::take(&mut cur), true));
                }
                at = start;
            }
        }
    }
    if !cur.is_empty() {
        contours.push((cur, false));
    }
    for (patches, closed) in &contours {
        let n = patches.len();
        for (i, patch) in patches.iter().enumerate() {
            let prev = if i > 0 {
                Some(&patches[i - 1])
            } else if *closed {
                Some(&patches[n - 1])
            } else {
                None
            };
            if let Some(prev) = prev {
                let p0 = patch.first();
                let jc = prev.join_control();
                if jc != p0 {
                    let prev_tan = robust_normalize_diff(p0, jc);
                    let tan0 = patch.tan0();
                    if tan0 != (0.0, 0.0) {
                        t.join(p0, prev_tan, tan0);
                    }
                }
            }
            t.body(patch);
        }
        if !*closed {
            let (first, last) = (&patches[0], &patches[n - 1]);
            match style.cap {
                LineCap::Butt => {}
                LineCap::Round => {
                    t.circle(last.last());
                    t.circle(first.first());
                }
                LineCap::Square => {
                    // A line of half the width beyond each end.
                    let end = last.last();
                    let dir = robust_normalize_diff(end, last.join_control());
                    let ext = add(end, scale(dir, style.radius));
                    t.body(&Patch {
                        p: [end, end, ext, ext],
                        line: true,
                    });
                    let begin = first.first();
                    let dir = first.tan0();
                    let ext = sub(begin, scale(dir, style.radius));
                    t.body(&Patch {
                        p: [ext, ext, begin, begin],
                        line: true,
                    });
                }
            }
        }
    }
    t.out
}
