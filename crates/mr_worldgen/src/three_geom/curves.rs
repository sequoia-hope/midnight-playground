//! Curves (three.js r180 `src/extras/core/Curve.js`, `CurvePath.js`,
//! `Path.js`, `Shape.js`, `Interpolations.js`, `src/extras/curves/*`).
//!
//! three's `Curve` serves 2D and 3D alike; here the 2D curves that paths
//! and shapes are built from are the enum [`Curve2`], and the 3D curves
//! implement the trait [`Curve3`], whose provided methods are `Curve`'s
//! (arc lengths, `getPointAt`, tangents, Frenet frames).

use core::f64::consts::PI;
use std::sync::OnceLock;

use mr_math::kernel;

use super::math::{Matrix4, Vector2, Vector3, clamp};

/// `curve.arcLengthDivisions`'s default.
pub const ARC_LENGTH_DIVISIONS: usize = 200;

// ── Interpolations ──────────────────────────────────────────────────────

/// Centripetal CatmullRom interpolation (`SplineCurve`).
pub fn catmull_rom(t: f64, p0: f64, p1: f64, p2: f64, p3: f64) -> f64 {
    let v0 = (p2 - p0) * 0.5;
    let v1 = (p3 - p1) * 0.5;
    let t2 = t * t;
    let t3 = t * t2;
    (2.0 * p1 - 2.0 * p2 + v0 + v1) * t3 + (-3.0 * p1 + 3.0 * p2 - 2.0 * v0 - v1) * t2 + v0 * t + p1
}

fn quadratic_bezier_p0(t: f64, p: f64) -> f64 {
    let k = 1.0 - t;
    k * k * p
}

fn quadratic_bezier_p1(t: f64, p: f64) -> f64 {
    2.0 * (1.0 - t) * t * p
}

fn quadratic_bezier_p2(t: f64, p: f64) -> f64 {
    t * t * p
}

pub fn quadratic_bezier(t: f64, p0: f64, p1: f64, p2: f64) -> f64 {
    quadratic_bezier_p0(t, p0) + quadratic_bezier_p1(t, p1) + quadratic_bezier_p2(t, p2)
}

fn cubic_bezier_p0(t: f64, p: f64) -> f64 {
    let k = 1.0 - t;
    k * k * k * p
}

fn cubic_bezier_p1(t: f64, p: f64) -> f64 {
    let k = 1.0 - t;
    3.0 * k * k * t * p
}

fn cubic_bezier_p2(t: f64, p: f64) -> f64 {
    3.0 * (1.0 - t) * t * t * p
}

fn cubic_bezier_p3(t: f64, p: f64) -> f64 {
    t * t * t * p
}

pub fn cubic_bezier(t: f64, p0: f64, p1: f64, p2: f64, p3: f64) -> f64 {
    cubic_bezier_p0(t, p0)
        + cubic_bezier_p1(t, p1)
        + cubic_bezier_p2(t, p2)
        + cubic_bezier_p3(t, p3)
}

// ── Arc lengths (Curve.getLengths / getUtoTmapping) ─────────────────────

/// `getLengths(divisions)` for any `getPoint`.
fn lengths_of<P: Copy>(
    divisions: usize,
    get_point: impl Fn(f64) -> P,
    dist: impl Fn(P, P) -> f64,
) -> Vec<f64> {
    let mut cache = Vec::with_capacity(divisions + 1);
    let mut last = get_point(0.0);
    let mut sum = 0.0;
    cache.push(0.0);
    for p in 1..=divisions {
        let current = get_point(p as f64 / divisions as f64);
        sum += dist(current, last);
        cache.push(sum);
        last = current;
    }
    cache
}

/// `getUtoTmapping(u, distance)` over the arc lengths: the `t` at which
/// the curve has run `u` of its length (or `distance`, when given and not
/// 0).
pub fn u_to_t_mapping(arc_lengths: &[f64], u: f64, distance: Option<f64>) -> f64 {
    let il = arc_lengths.len();
    // The targeted u distance value to get
    let target_arc_length = match distance {
        Some(d) if d != 0.0 && !d.is_nan() => d,
        _ => u * arc_lengths[il - 1],
    };

    // binary search for the index with largest value smaller than target u distance
    let mut low: i64 = 0;
    let mut high: i64 = il as i64 - 1;
    while low <= high {
        // less likely to overflow, though probably not issue here, JS doesn't really have integers, all numbers are floats
        let i = low + (high - low) / 2;
        let comparison = arc_lengths[i as usize] - target_arc_length;
        if comparison < 0.0 {
            low = i + 1;
        } else if comparison > 0.0 {
            high = i - 1;
        } else {
            high = i;
            break;
        }
    }
    let i = high;
    if arc_lengths[i as usize] == target_arc_length {
        return i as f64 / (il - 1) as f64;
    }

    // we could get finer grain at lengths, or use simple interpolation between two points
    let length_before = arc_lengths[i as usize];
    let length_after = arc_lengths[i as usize + 1];
    let segment_length = length_after - length_before;
    // determine where we are between the 'before' and 'after' points
    let segment_fraction = (target_arc_length - length_before) / segment_length;
    // add that fractional amount to t
    (i as f64 + segment_fraction) / (il - 1) as f64
}

// ── 2D curves ───────────────────────────────────────────────────────────

/// `EllipseCurve` (and `ArcCurve`, which is one with equal radii).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EllipseCurve {
    pub a_x: f64,
    pub a_y: f64,
    pub x_radius: f64,
    pub y_radius: f64,
    pub a_start_angle: f64,
    pub a_end_angle: f64,
    pub a_clockwise: bool,
    pub a_rotation: f64,
}

impl EllipseCurve {
    pub fn get_point(&self, t: f64) -> Vector2 {
        let two_pi = PI * 2.0;
        let mut delta_angle = self.a_end_angle - self.a_start_angle;
        let same_points = delta_angle.abs() < f64::EPSILON;

        // ensures that deltaAngle is 0 .. 2 PI
        while delta_angle < 0.0 {
            delta_angle += two_pi;
        }
        while delta_angle > two_pi {
            delta_angle -= two_pi;
        }
        if delta_angle < f64::EPSILON {
            if same_points {
                delta_angle = 0.0;
            } else {
                delta_angle = two_pi;
            }
        }
        if self.a_clockwise && !same_points {
            if delta_angle == two_pi {
                delta_angle = -two_pi;
            } else {
                delta_angle -= two_pi;
            }
        }

        let angle = self.a_start_angle + t * delta_angle;
        let mut x = self.a_x + self.x_radius * kernel::cos(angle);
        let mut y = self.a_y + self.y_radius * kernel::sin(angle);
        if self.a_rotation != 0.0 {
            let cos = kernel::cos(self.a_rotation);
            let sin = kernel::sin(self.a_rotation);
            let tx = x - self.a_x;
            let ty = y - self.a_y;
            // Rotate the point about the center of the ellipse.
            x = tx * cos - ty * sin + self.a_x;
            y = tx * sin + ty * cos + self.a_y;
        }
        Vector2::new(x, y)
    }
}

/// One segment of a 2D path.
#[derive(Clone, Debug, PartialEq)]
pub enum Curve2 {
    /// `LineCurve(v1, v2)`.
    Line(Vector2, Vector2),
    /// `QuadraticBezierCurve(v0, v1, v2)`.
    QuadraticBezier(Vector2, Vector2, Vector2),
    /// `CubicBezierCurve(v0, v1, v2, v3)`.
    CubicBezier(Vector2, Vector2, Vector2, Vector2),
    Ellipse(EllipseCurve),
    /// `SplineCurve(points)`.
    Spline(Vec<Vector2>),
}

impl Curve2 {
    pub fn get_point(&self, t: f64) -> Vector2 {
        match self {
            Curve2::Line(v1, v2) => {
                if t == 1.0 {
                    *v2
                } else {
                    (*v2 - *v1).multiply_scalar(t) + *v1
                }
            }
            Curve2::QuadraticBezier(v0, v1, v2) => Vector2::new(
                quadratic_bezier(t, v0.x, v1.x, v2.x),
                quadratic_bezier(t, v0.y, v1.y, v2.y),
            ),
            Curve2::CubicBezier(v0, v1, v2, v3) => Vector2::new(
                cubic_bezier(t, v0.x, v1.x, v2.x, v3.x),
                cubic_bezier(t, v0.y, v1.y, v2.y, v3.y),
            ),
            Curve2::Ellipse(e) => e.get_point(t),
            Curve2::Spline(points) => {
                let n = points.len() as i64;
                let p = (n - 1) as f64 * t;
                let int_point = p.floor();
                let weight = p - int_point;
                let ip = int_point as i64;
                let p0 = points[(if ip == 0 { ip } else { ip - 1 }) as usize];
                let p1 = points[ip as usize];
                let p2 = points[(if ip > n - 2 { n - 1 } else { ip + 1 }) as usize];
                let p3 = points[(if ip > n - 3 { n - 1 } else { ip + 2 }) as usize];
                Vector2::new(
                    catmull_rom(weight, p0.x, p1.x, p2.x, p3.x),
                    catmull_rom(weight, p0.y, p1.y, p2.y, p3.y),
                )
            }
        }
    }

    /// `getPoints(divisions)`.
    pub fn get_points(&self, divisions: usize) -> Vec<Vector2> {
        (0..=divisions)
            .map(|d| self.get_point(d as f64 / divisions as f64))
            .collect()
    }

    /// `getLengths(divisions)`.
    pub fn get_lengths(&self, divisions: usize) -> Vec<f64> {
        lengths_of(divisions, |t| self.get_point(t), Vector2::distance_to)
    }

    /// `getLength()`.
    pub fn get_length(&self) -> f64 {
        *self.get_lengths(ARC_LENGTH_DIVISIONS).last().unwrap()
    }

    /// `getPointAt(u)`: a line maps `u` to `t` directly.
    pub fn get_point_at(&self, u: f64) -> Vector2 {
        match self {
            Curve2::Line(..) => self.get_point(u),
            _ => {
                let t = u_to_t_mapping(&self.get_lengths(ARC_LENGTH_DIVISIONS), u, None);
                self.get_point(t)
            }
        }
    }
}

// ── CurvePath, Path, Shape ──────────────────────────────────────────────

/// `Path` (a `CurvePath` of 2D curves with a pen position).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path {
    pub curves: Vec<Curve2>,
    pub auto_close: bool,
    pub current_point: Vector2,
}

impl Path {
    pub fn new() -> Self {
        Path::default()
    }

    /// `new Path(points)`: `moveTo` the first, `lineTo` the rest.
    pub fn from_points(points: &[Vector2]) -> Self {
        let mut p = Path::new();
        p.set_from_points(points);
        p
    }

    pub fn set_from_points(&mut self, points: &[Vector2]) -> &mut Self {
        self.move_to(points[0].x, points[0].y);
        for p in &points[1..] {
            self.line_to(p.x, p.y);
        }
        self
    }

    pub fn add(&mut self, curve: Curve2) {
        self.curves.push(curve);
    }

    /// `closePath()`: a line back to the start, if the path does not end
    /// there.
    pub fn close_path(&mut self) -> &mut Self {
        let start_point = self.curves[0].get_point(0.0);
        let end_point = self.curves[self.curves.len() - 1].get_point(1.0);
        if !start_point.equals(end_point) {
            self.curves.push(Curve2::Line(end_point, start_point));
        }
        self
    }

    pub fn move_to(&mut self, x: f64, y: f64) -> &mut Self {
        self.current_point = Vector2::new(x, y);
        self
    }

    pub fn line_to(&mut self, x: f64, y: f64) -> &mut Self {
        self.curves
            .push(Curve2::Line(self.current_point, Vector2::new(x, y)));
        self.current_point = Vector2::new(x, y);
        self
    }

    pub fn quadratic_curve_to(&mut self, a_cpx: f64, a_cpy: f64, a_x: f64, a_y: f64) -> &mut Self {
        self.curves.push(Curve2::QuadraticBezier(
            self.current_point,
            Vector2::new(a_cpx, a_cpy),
            Vector2::new(a_x, a_y),
        ));
        self.current_point = Vector2::new(a_x, a_y);
        self
    }

    pub fn bezier_curve_to(
        &mut self,
        a_cp1x: f64,
        a_cp1y: f64,
        a_cp2x: f64,
        a_cp2y: f64,
        a_x: f64,
        a_y: f64,
    ) -> &mut Self {
        self.curves.push(Curve2::CubicBezier(
            self.current_point,
            Vector2::new(a_cp1x, a_cp1y),
            Vector2::new(a_cp2x, a_cp2y),
            Vector2::new(a_x, a_y),
        ));
        self.current_point = Vector2::new(a_x, a_y);
        self
    }

    pub fn spline_thru(&mut self, pts: &[Vector2]) -> &mut Self {
        let mut npts = vec![self.current_point];
        npts.extend_from_slice(pts);
        self.curves.push(Curve2::Spline(npts));
        self.current_point = pts[pts.len() - 1];
        self
    }

    /// `arc(aX, aY, aRadius, aStartAngle, aEndAngle, aClockwise)`, centred
    /// relative to the pen.
    pub fn arc(
        &mut self,
        a_x: f64,
        a_y: f64,
        a_radius: f64,
        a_start_angle: f64,
        a_end_angle: f64,
        a_clockwise: bool,
    ) -> &mut Self {
        let x0 = self.current_point.x;
        let y0 = self.current_point.y;
        self.absarc(
            a_x + x0,
            a_y + y0,
            a_radius,
            a_start_angle,
            a_end_angle,
            a_clockwise,
        )
    }

    pub fn absarc(
        &mut self,
        a_x: f64,
        a_y: f64,
        a_radius: f64,
        a_start_angle: f64,
        a_end_angle: f64,
        a_clockwise: bool,
    ) -> &mut Self {
        self.absellipse(
            a_x,
            a_y,
            a_radius,
            a_radius,
            a_start_angle,
            a_end_angle,
            a_clockwise,
            0.0,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn ellipse(
        &mut self,
        a_x: f64,
        a_y: f64,
        x_radius: f64,
        y_radius: f64,
        a_start_angle: f64,
        a_end_angle: f64,
        a_clockwise: bool,
        a_rotation: f64,
    ) -> &mut Self {
        let x0 = self.current_point.x;
        let y0 = self.current_point.y;
        self.absellipse(
            a_x + x0,
            a_y + y0,
            x_radius,
            y_radius,
            a_start_angle,
            a_end_angle,
            a_clockwise,
            a_rotation,
        )
    }

    /// `absellipse(...)`: joins the pen to the ellipse's start with a line
    /// if the path has curves and they differ.
    #[allow(clippy::too_many_arguments)]
    pub fn absellipse(
        &mut self,
        a_x: f64,
        a_y: f64,
        x_radius: f64,
        y_radius: f64,
        a_start_angle: f64,
        a_end_angle: f64,
        a_clockwise: bool,
        a_rotation: f64,
    ) -> &mut Self {
        let curve = EllipseCurve {
            a_x,
            a_y,
            x_radius,
            y_radius,
            a_start_angle,
            a_end_angle,
            a_clockwise,
            a_rotation,
        };
        if !self.curves.is_empty() {
            // if a previous curve is present, attempt to join
            let first_point = curve.get_point(0.0);
            if !first_point.equals(self.current_point) {
                self.line_to(first_point.x, first_point.y);
            }
        }
        self.curves.push(Curve2::Ellipse(curve));
        let last_point = curve.get_point(1.0);
        self.current_point = last_point;
        self
    }

    /// `getCurveLengths()`.
    pub fn get_curve_lengths(&self) -> Vec<f64> {
        let mut lengths = Vec::with_capacity(self.curves.len());
        let mut sums = 0.0;
        for c in &self.curves {
            sums += c.get_length();
            lengths.push(sums);
        }
        lengths
    }

    /// `getLength()`.
    pub fn get_length(&self) -> f64 {
        *self.get_curve_lengths().last().unwrap()
    }

    /// `getPoint(t)`: the point at `t` of the whole length; `None` past the
    /// end (three returns `null`).
    pub fn get_point(&self, t: f64) -> Option<Vector2> {
        let d = t * self.get_length();
        let curve_lengths = self.get_curve_lengths();
        // To think about boundaries points.
        for (i, &len) in curve_lengths.iter().enumerate() {
            if len >= d {
                let diff = len - d;
                let curve = &self.curves[i];
                let segment_length = curve.get_length();
                let u = if segment_length == 0.0 {
                    0.0
                } else {
                    1.0 - diff / segment_length
                };
                return Some(curve.get_point_at(u));
            }
        }
        None
    }

    /// `getSpacedPoints(divisions)` (three's default 40).
    pub fn get_spaced_points(&self, divisions: usize) -> Vec<Option<Vector2>> {
        let mut points: Vec<Option<Vector2>> = (0..=divisions)
            .map(|i| self.get_point(i as f64 / divisions as f64))
            .collect();
        if self.auto_close {
            points.push(points[0]);
        }
        points
    }

    /// `getPoints(divisions)` (three's default 12): each curve sampled at a
    /// resolution for its kind, consecutive duplicates dropped.
    pub fn get_points(&self, divisions: usize) -> Vec<Vector2> {
        let mut points: Vec<Vector2> = Vec::new();
        let mut last: Option<Vector2> = None;
        for curve in &self.curves {
            let resolution = match curve {
                Curve2::Ellipse(_) => divisions * 2,
                Curve2::Line(..) => 1,
                Curve2::Spline(pts) => divisions * pts.len(),
                _ => divisions,
            };
            let pts = curve.get_points(resolution);
            for point in pts {
                if let Some(l) = last
                    && l.equals(point)
                {
                    continue; // ensures no consecutive points are duplicates
                }
                points.push(point);
                last = Some(point);
            }
        }
        if self.auto_close && points.len() > 1 && !points[points.len() - 1].equals(points[0]) {
            points.push(points[0]);
        }
        points
    }
}

/// `Shape`: a path with holes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shape {
    pub path: Path,
    pub holes: Vec<Path>,
}

impl core::ops::Deref for Shape {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.path
    }
}

impl core::ops::DerefMut for Shape {
    fn deref_mut(&mut self) -> &mut Path {
        &mut self.path
    }
}

impl Shape {
    pub fn new() -> Self {
        Shape::default()
    }

    /// `new Shape(points)`.
    pub fn from_points(points: &[Vector2]) -> Self {
        Shape {
            path: Path::from_points(points),
            holes: Vec::new(),
        }
    }

    pub fn get_points_holes(&self, divisions: usize) -> Vec<Vec<Vector2>> {
        self.holes.iter().map(|h| h.get_points(divisions)).collect()
    }

    /// `extractPoints(divisions)`: the outline's points and each hole's.
    pub fn extract_points(&self, divisions: usize) -> (Vec<Vector2>, Vec<Vec<Vector2>>) {
        (self.get_points(divisions), self.get_points_holes(divisions))
    }
}

// ── 3D curves ───────────────────────────────────────────────────────────

/// The Frenet frames of a curve (`computeFrenetFrames`).
#[derive(Clone, Debug, PartialEq)]
pub struct FrenetFrames {
    pub tangents: Vec<Vector3>,
    pub normals: Vec<Vector3>,
    pub binormals: Vec<Vector3>,
}

/// A 3D `Curve`: implement `get_point`; the rest are three's `Curve`
/// methods.
pub trait Curve3 {
    /// `getPoint(t)`, t in [0, 1].
    fn get_point(&self, t: f64) -> Vector3;

    /// Where `getLengths()` (at the default 200 divisions) is cached, if
    /// the curve keeps one. The values are the same either way.
    fn arc_length_cache(&self) -> Option<&OnceLock<Vec<f64>>> {
        None
    }

    /// `getPoints(divisions)`.
    fn get_points(&self, divisions: usize) -> Vec<Vector3> {
        (0..=divisions)
            .map(|d| self.get_point(d as f64 / divisions as f64))
            .collect()
    }

    /// `getSpacedPoints(divisions)`.
    fn get_spaced_points(&self, divisions: usize) -> Vec<Vector3> {
        (0..=divisions)
            .map(|d| self.get_point_at(d as f64 / divisions as f64))
            .collect()
    }

    /// `getLength()`.
    fn get_length(&self) -> f64 {
        *self.get_lengths().last().unwrap()
    }

    /// `getLengths()`: cumulative chord lengths at 200 divisions.
    fn get_lengths(&self) -> Vec<f64> {
        match self.arc_length_cache() {
            Some(cache) => cache
                .get_or_init(|| self.get_lengths_with(ARC_LENGTH_DIVISIONS))
                .clone(),
            None => self.get_lengths_with(ARC_LENGTH_DIVISIONS),
        }
    }

    /// `getLengths(divisions)`.
    fn get_lengths_with(&self, divisions: usize) -> Vec<f64> {
        lengths_of(divisions, |t| self.get_point(t), Vector3::distance_to)
    }

    /// `getUtoTmapping(u, distance)`.
    fn get_u_to_t_mapping(&self, u: f64, distance: Option<f64>) -> f64 {
        match self.arc_length_cache() {
            Some(cache) => {
                let lengths = cache.get_or_init(|| self.get_lengths_with(ARC_LENGTH_DIVISIONS));
                u_to_t_mapping(lengths, u, distance)
            }
            None => u_to_t_mapping(&self.get_lengths(), u, distance),
        }
    }

    /// `getPointAt(u)`: the point at fraction `u` of the arc length.
    fn get_point_at(&self, u: f64) -> Vector3 {
        let t = self.get_u_to_t_mapping(u, None);
        self.get_point(t)
    }

    /// `getTangent(t)`: by finite difference (delta 0.0001), normalised.
    fn get_tangent(&self, t: f64) -> Vector3 {
        let delta = 0.0001;
        let mut t1 = t - delta;
        let mut t2 = t + delta;
        // Capping in case of danger
        if t1 < 0.0 {
            t1 = 0.0;
        }
        if t2 > 1.0 {
            t2 = 1.0;
        }
        let pt1 = self.get_point(t1);
        let pt2 = self.get_point(t2);
        (pt2 - pt1).normalize()
    }

    /// `getTangentAt(u)`.
    fn get_tangent_at(&self, u: f64) -> Vector3 {
        let t = self.get_u_to_t_mapping(u, None);
        self.get_tangent(t)
    }

    /// `computeFrenetFrames(segments, closed)`; see
    /// http://www.cs.indiana.edu/pub/techreports/TR425.pdf
    fn compute_frenet_frames(&self, segments: usize, closed: bool) -> FrenetFrames {
        let mut normal = Vector3::default();
        let mut tangents = Vec::with_capacity(segments + 1);
        let mut normals = Vec::with_capacity(segments + 1);
        let mut binormals = Vec::with_capacity(segments + 1);
        let mut vec;

        // compute the tangent vectors for each segment on the curve
        for i in 0..=segments {
            let u = i as f64 / segments as f64;
            tangents.push(self.get_tangent_at(u));
        }

        // select an initial normal vector perpendicular to the first tangent vector,
        // and in the direction of the minimum tangent xyz component
        let mut min = f64::MAX;
        let tx = tangents[0].x.abs();
        let ty = tangents[0].y.abs();
        let tz = tangents[0].z.abs();
        if tx <= min {
            min = tx;
            normal = Vector3::new(1.0, 0.0, 0.0);
        }
        if ty <= min {
            min = ty;
            normal = Vector3::new(0.0, 1.0, 0.0);
        }
        if tz <= min {
            normal = Vector3::new(0.0, 0.0, 1.0);
        }
        vec = Vector3::cross_vectors(tangents[0], normal).normalize();
        normals.push(Vector3::cross_vectors(tangents[0], vec));
        binormals.push(Vector3::cross_vectors(tangents[0], normals[0]));

        // compute the slowly-varying normal and binormal vectors for each segment on the curve
        for i in 1..=segments {
            let mut n = normals[i - 1];
            vec = Vector3::cross_vectors(tangents[i - 1], tangents[i]);
            if vec.length() > f64::EPSILON {
                vec = vec.normalize();
                let theta = kernel::acos(clamp(tangents[i - 1].dot(tangents[i]), -1.0, 1.0)); // clamp for floating pt errors
                n = n.apply_matrix4(&Matrix4::make_rotation_axis(vec, theta));
            }
            normals.push(n);
            binormals.push(Vector3::cross_vectors(tangents[i], normals[i]));
        }

        // if the curve is closed, postprocess the vectors so the first and last normal vectors are the same
        if closed {
            let mut theta = kernel::acos(clamp(normals[0].dot(normals[segments]), -1.0, 1.0));
            theta /= segments as f64;
            vec = Vector3::cross_vectors(normals[0], normals[segments]);
            if tangents[0].dot(vec) > 0.0 {
                theta = -theta;
            }
            for i in 1..=segments {
                // twist a little...
                normals[i] = normals[i]
                    .apply_matrix4(&Matrix4::make_rotation_axis(tangents[i], theta * i as f64));
                binormals[i] = Vector3::cross_vectors(tangents[i], normals[i]);
            }
        }

        FrenetFrames {
            tangents,
            normals,
            binormals,
        }
    }
}

/// `CatmullRomCurve3`'s `curveType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveType {
    Centripetal,
    Chordal,
    CatmullRom,
}

/// `CatmullRomCurve3(points, closed, curveType, tension)`; three's
/// defaults `[], false, 'centripetal', 0.5`.
#[derive(Clone, Debug)]
pub struct CatmullRomCurve3 {
    pub points: Vec<Vector3>,
    pub closed: bool,
    pub curve_type: CurveType,
    pub tension: f64,
    cache: OnceLock<Vec<f64>>,
}

impl CatmullRomCurve3 {
    pub fn new(points: Vec<Vector3>, closed: bool, curve_type: CurveType, tension: f64) -> Self {
        CatmullRomCurve3 {
            points,
            closed,
            curve_type,
            tension,
            cache: OnceLock::new(),
        }
    }

    /// `updateArcLengths()`: after changing `points`.
    pub fn update_arc_lengths(&mut self) {
        self.cache = OnceLock::new();
    }
}

/// `CubicPoly`: a cubic p(s) = c0 + c1 s + c2 s² + c3 s³ with p(0) = x0,
/// p(1) = x1, p'(0) = t0, p'(1) = t1.
#[derive(Clone, Copy, Default)]
struct CubicPoly {
    c0: f64,
    c1: f64,
    c2: f64,
    c3: f64,
}

impl CubicPoly {
    fn init(x0: f64, x1: f64, t0: f64, t1: f64) -> Self {
        CubicPoly {
            c0: x0,
            c1: t0,
            c2: -3.0 * x0 + 3.0 * x1 - 2.0 * t0 - t1,
            c3: 2.0 * x0 - 2.0 * x1 + t0 + t1,
        }
    }

    fn init_catmull_rom(x0: f64, x1: f64, x2: f64, x3: f64, tension: f64) -> Self {
        CubicPoly::init(x1, x2, tension * (x2 - x0), tension * (x3 - x1))
    }

    #[allow(clippy::too_many_arguments)]
    fn init_nonuniform_catmull_rom(
        x0: f64,
        x1: f64,
        x2: f64,
        x3: f64,
        dt0: f64,
        dt1: f64,
        dt2: f64,
    ) -> Self {
        // compute tangents when parameterized in [t1,t2]
        let mut t1 = (x1 - x0) / dt0 - (x2 - x0) / (dt0 + dt1) + (x2 - x1) / dt1;
        let mut t2 = (x2 - x1) / dt1 - (x3 - x1) / (dt1 + dt2) + (x3 - x2) / dt2;
        // rescale tangents for parametrization in [0,1]
        t1 *= dt1;
        t2 *= dt1;
        CubicPoly::init(x1, x2, t1, t2)
    }

    fn calc(&self, t: f64) -> f64 {
        let t2 = t * t;
        let t3 = t2 * t;
        self.c0 + self.c1 * t + self.c2 * t2 + self.c3 * t3
    }
}

impl Curve3 for CatmullRomCurve3 {
    fn arc_length_cache(&self) -> Option<&OnceLock<Vec<f64>>> {
        Some(&self.cache)
    }

    fn get_point(&self, t: f64) -> Vector3 {
        let points = &self.points;
        let l = points.len() as i64;
        let p = (l - if self.closed { 0 } else { 1 }) as f64 * t;
        let mut int_point = p.floor() as i64;
        let mut weight = p - int_point as f64;

        if self.closed {
            int_point += if int_point > 0 {
                0
            } else {
                ((int_point.abs() as f64 / l as f64).floor() as i64 + 1) * l
            };
        } else if weight == 0.0 && int_point == l - 1 {
            int_point = l - 2;
            weight = 1.0;
        }

        // 4 points (p1 & p2 defined below). p0 and p3 may both be three's
        // shared `tmp` vector; then p0 reads what p3 last wrote into it.
        let mut p0_is_tmp = false;
        let mut p0 = if self.closed || int_point > 0 {
            points[((int_point - 1) % l) as usize]
        } else {
            // extrapolate first point
            p0_is_tmp = true;
            (points[0] - points[1]) + points[0]
        };
        let p1 = points[(int_point % l) as usize];
        let p2 = points[((int_point + 1) % l) as usize];
        let p3 = if self.closed || int_point + 2 < l {
            points[((int_point + 2) % l) as usize]
        } else {
            // extrapolate last point
            let tmp =
                (points[(l - 1) as usize] - points[(l - 2) as usize]) + points[(l - 1) as usize];
            if p0_is_tmp {
                p0 = tmp;
            }
            tmp
        };

        let (px, py, pz) = if matches!(self.curve_type, CurveType::Centripetal | CurveType::Chordal)
        {
            // init Centripetal / Chordal Catmull-Rom
            let pow = if self.curve_type == CurveType::Chordal {
                0.5
            } else {
                0.25
            };
            let mut dt0 = kernel::pow(p0.distance_to_squared(p1), pow);
            let mut dt1 = kernel::pow(p1.distance_to_squared(p2), pow);
            let mut dt2 = kernel::pow(p2.distance_to_squared(p3), pow);
            // safety check for repeated points
            if dt1 < 1e-4 {
                dt1 = 1.0;
            }
            if dt0 < 1e-4 {
                dt0 = dt1;
            }
            if dt2 < 1e-4 {
                dt2 = dt1;
            }
            (
                CubicPoly::init_nonuniform_catmull_rom(p0.x, p1.x, p2.x, p3.x, dt0, dt1, dt2),
                CubicPoly::init_nonuniform_catmull_rom(p0.y, p1.y, p2.y, p3.y, dt0, dt1, dt2),
                CubicPoly::init_nonuniform_catmull_rom(p0.z, p1.z, p2.z, p3.z, dt0, dt1, dt2),
            )
        } else {
            (
                CubicPoly::init_catmull_rom(p0.x, p1.x, p2.x, p3.x, self.tension),
                CubicPoly::init_catmull_rom(p0.y, p1.y, p2.y, p3.y, self.tension),
                CubicPoly::init_catmull_rom(p0.z, p1.z, p2.z, p3.z, self.tension),
            )
        };

        Vector3::new(px.calc(weight), py.calc(weight), pz.calc(weight))
    }
}

/// `LineCurve3(v1, v2)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineCurve3 {
    pub v1: Vector3,
    pub v2: Vector3,
}

impl Curve3 for LineCurve3 {
    fn get_point(&self, t: f64) -> Vector3 {
        if t == 1.0 {
            self.v2
        } else {
            (self.v2 - self.v1).multiply_scalar(t) + self.v1
        }
    }

    fn get_point_at(&self, u: f64) -> Vector3 {
        self.get_point(u)
    }

    fn get_tangent(&self, _t: f64) -> Vector3 {
        (self.v2 - self.v1).normalize()
    }

    fn get_tangent_at(&self, u: f64) -> Vector3 {
        self.get_tangent(u)
    }
}

/// `QuadraticBezierCurve3(v0, v1, v2)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuadraticBezierCurve3 {
    pub v0: Vector3,
    pub v1: Vector3,
    pub v2: Vector3,
}

impl Curve3 for QuadraticBezierCurve3 {
    fn get_point(&self, t: f64) -> Vector3 {
        let (v0, v1, v2) = (self.v0, self.v1, self.v2);
        Vector3::new(
            quadratic_bezier(t, v0.x, v1.x, v2.x),
            quadratic_bezier(t, v0.y, v1.y, v2.y),
            quadratic_bezier(t, v0.z, v1.z, v2.z),
        )
    }
}

/// `CubicBezierCurve3(v0, v1, v2, v3)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CubicBezierCurve3 {
    pub v0: Vector3,
    pub v1: Vector3,
    pub v2: Vector3,
    pub v3: Vector3,
}

impl Curve3 for CubicBezierCurve3 {
    fn get_point(&self, t: f64) -> Vector3 {
        let (v0, v1, v2, v3) = (self.v0, self.v1, self.v2, self.v3);
        Vector3::new(
            cubic_bezier(t, v0.x, v1.x, v2.x, v3.x),
            cubic_bezier(t, v0.y, v1.y, v2.y, v3.y),
            cubic_bezier(t, v0.z, v1.z, v2.z, v3.z),
        )
    }
}
