//! The canvas's current path, kept in device space: each point goes
//! through the transform in force when it is added, as the canvas spec
//! says (Blink keeps it in user space and re-maps it when the transform
//! changes, which comes to the same). Arcs and ellipses follow Blink's
//! angle rules (`CanonicalizeAngle`, `AdjustEndAngle`) and become cubic
//! Béziers of at most a quarter turn each.

use mp_math::kernel;
use tiny_skia::{Path, PathBuilder};

use crate::Matrix;

#[derive(Clone, Default)]
pub struct CanvasPath {
    pb: PathBuilder,
    /// Current point and the current subpath's start, device space.
    cur: Option<(f64, f64)>,
    start: (f64, f64),
}

impl CanvasPath {
    pub fn new() -> CanvasPath {
        CanvasPath::default()
    }

    pub fn finish(&self) -> Option<Path> {
        self.pb.clone().finish()
    }

    pub fn has_current_point(&self) -> bool {
        self.cur.is_some()
    }

    fn dev_move(&mut self, p: (f64, f64)) {
        self.pb.move_to(p.0 as f32, p.1 as f32);
        self.cur = Some(p);
        self.start = p;
    }

    fn dev_line(&mut self, p: (f64, f64)) {
        if self.cur.is_none() {
            self.dev_move(p);
            return;
        }
        self.pb.line_to(p.0 as f32, p.1 as f32);
        self.cur = Some(p);
    }

    fn dev_cubic(&mut self, c1: (f64, f64), c2: (f64, f64), p: (f64, f64)) {
        self.pb.cubic_to(
            c1.0 as f32,
            c1.1 as f32,
            c2.0 as f32,
            c2.1 as f32,
            p.0 as f32,
            p.1 as f32,
        );
        self.cur = Some(p);
    }

    pub fn move_to(&mut self, m: &Matrix, x: f64, y: f64) {
        if !(x.is_finite() && y.is_finite()) {
            return;
        }
        self.dev_move(m.apply(x, y));
    }

    pub fn line_to(&mut self, m: &Matrix, x: f64, y: f64) {
        if !(x.is_finite() && y.is_finite()) {
            return;
        }
        self.dev_line(m.apply(x, y));
    }

    pub fn quadratic_curve_to(&mut self, m: &Matrix, cx: f64, cy: f64, x: f64, y: f64) {
        if ![cx, cy, x, y].iter().all(|v| v.is_finite()) {
            return;
        }
        if self.cur.is_none() {
            self.dev_move(m.apply(cx, cy));
        }
        let c = m.apply(cx, cy);
        let p = m.apply(x, y);
        self.pb
            .quad_to(c.0 as f32, c.1 as f32, p.0 as f32, p.1 as f32);
        self.cur = Some(p);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn bezier_curve_to(
        &mut self,
        m: &Matrix,
        c1x: f64,
        c1y: f64,
        c2x: f64,
        c2y: f64,
        x: f64,
        y: f64,
    ) {
        if ![c1x, c1y, c2x, c2y, x, y].iter().all(|v| v.is_finite()) {
            return;
        }
        if self.cur.is_none() {
            self.dev_move(m.apply(c1x, c1y));
        }
        self.dev_cubic(m.apply(c1x, c1y), m.apply(c2x, c2y), m.apply(x, y));
    }

    pub fn close_path(&mut self) {
        if self.cur.is_some() {
            self.pb.close();
            self.cur = Some(self.start);
        }
    }

    pub fn rect(&mut self, m: &Matrix, x: f64, y: f64, w: f64, h: f64) {
        if ![x, y, w, h].iter().all(|v| v.is_finite()) {
            return;
        }
        self.move_to(m, x, y);
        self.line_to(m, x + w, y);
        self.line_to(m, x + w, y + h);
        self.line_to(m, x, y + h);
        self.close_path();
    }

    /// `roundRect(x, y, w, h, r)` with one radius for every corner.
    pub fn round_rect(&mut self, m: &Matrix, x: f64, y: f64, w: f64, h: f64, r: f64) {
        if ![x, y, w, h, r].iter().all(|v| v.is_finite()) {
            return;
        }
        assert!(
            r >= 0.0,
            "roundRect: negative radius (a RangeError in the browser)"
        );
        // Normalise a negative width or height (the corners swap), then
        // scale the radii down if two would overlap along a side.
        let (x, w) = if w < 0.0 { (x + w, -w) } else { (x, w) };
        let (y, h) = if h < 0.0 { (y + h, -h) } else { (y, h) };
        let mut r = r;
        let scale = (w / (2.0 * r)).min(h / (2.0 * r));
        if scale < 1.0 {
            r *= scale;
        }
        self.move_to(m, x + r, y);
        self.line_to(m, x + w - r, y);
        self.corner(m, x + w - r, y + r, r, -0.5);
        self.line_to(m, x + w, y + h - r);
        self.corner(m, x + w - r, y + h - r, r, 0.0);
        self.line_to(m, x + r, y + h);
        self.corner(m, x + r, y + h - r, r, 0.5);
        self.line_to(m, x, y + r);
        self.corner(m, x + r, y + r, r, 1.0);
        self.close_path();
        self.move_to(m, x, y);
    }

    /// A quarter circle clockwise from angle `turn·π`.
    fn corner(&mut self, m: &Matrix, cx: f64, cy: f64, r: f64, turn: f64) {
        if r > 0.0 {
            let a0 = turn * std::f64::consts::PI;
            self.arc_segments(m, cx, cy, r, r, 0.0, a0, a0 + std::f64::consts::FRAC_PI_2);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn arc(
        &mut self,
        m: &Matrix,
        x: f64,
        y: f64,
        r: f64,
        a0: f64,
        a1: f64,
        anticlockwise: bool,
    ) {
        self.ellipse(m, x, y, r, r, 0.0, a0, a1, anticlockwise);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn ellipse(
        &mut self,
        m: &Matrix,
        x: f64,
        y: f64,
        rx: f64,
        ry: f64,
        rotation: f64,
        a0: f64,
        a1: f64,
        anticlockwise: bool,
    ) {
        if ![x, y, rx, ry, rotation, a0, a1]
            .iter()
            .all(|v| v.is_finite())
        {
            return;
        }
        assert!(
            rx >= 0.0 && ry >= 0.0,
            "arc/ellipse: negative radius (an IndexSizeError in the browser)"
        );
        let (s, e) = angles(a0 as f32, a1 as f32, anticlockwise);
        self.arc_segments(m, x, y, rx, ry, rotation, s as f64, e as f64);
    }

    /// Lines to the arc's start (or moves there), then the arc from `s` to
    /// `e` radians in pieces of at most a quarter turn.
    #[allow(clippy::too_many_arguments)]
    fn arc_segments(
        &mut self,
        m: &Matrix,
        x: f64,
        y: f64,
        rx: f64,
        ry: f64,
        rot: f64,
        s: f64,
        e: f64,
    ) {
        let (sr, cr) = (kernel::sin(rot), kernel::cos(rot));
        let at = |t: f64| {
            let (px, py) = (rx * kernel::cos(t), ry * kernel::sin(t));
            m.apply(x + px * cr - py * sr, y + px * sr + py * cr)
        };
        let tangent = |t: f64| {
            let (px, py) = (-rx * kernel::sin(t), ry * kernel::cos(t));
            m.apply_vec(px * cr - py * sr, px * sr + py * cr)
        };
        self.dev_line(at(s));
        let sweep = e - s;
        if sweep == 0.0 {
            return;
        }
        let n = (sweep.abs() / std::f64::consts::FRAC_PI_2 - 1e-9)
            .ceil()
            .max(1.0) as usize;
        let step = sweep / n as f64;
        let k = 4.0 / 3.0 * kernel::tan(step / 4.0);
        for i in 0..n {
            let t0 = s + step * i as f64;
            let t1 = if i + 1 == n { e } else { t0 + step };
            let (p0, p1) = (at(t0), at(t1));
            let (d0, d1) = (tangent(t0), tangent(t1));
            self.dev_cubic(
                (p0.0 + k * d0.0, p0.1 + k * d0.1),
                (p1.0 - k * d1.0, p1.1 - k * d1.1),
                p1,
            );
        }
    }
}

const TWO_PI: f32 = std::f32::consts::PI * 2.0;

/// Blink's start and end angles for `arc`/`ellipse` (float, as Blink
/// computes them): the start moved into [0, 2π), the end following, then
/// the sweep limited to one turn in the drawing direction.
fn angles(start: f32, end: f32, anticlockwise: bool) -> (f32, f32) {
    // CanonicalizeAngle.
    let mut s = start % TWO_PI;
    if s < 0.0 {
        s += TWO_PI;
        if s >= TWO_PI {
            s -= TWO_PI;
        }
    }
    let delta = s - start;
    let e = end + delta;
    // AdjustEndAngle.
    let e = if !anticlockwise && e - s >= TWO_PI {
        s + TWO_PI
    } else if anticlockwise && s - e >= TWO_PI {
        s - TWO_PI
    } else if !anticlockwise && s > e {
        s + (TWO_PI - (s - e) % TWO_PI)
    } else if anticlockwise && s < e {
        s - (TWO_PI - (e - s) % TWO_PI)
    } else {
        e
    };
    (s, e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blink_angles() {
        // ellipse(..., 0, 7): a full turn.
        assert_eq!(angles(0.0, 7.0, false), (0.0, TWO_PI));
        // arc(..., 0, PI, true): anticlockwise the long way round.
        let (s, e) = angles(0.0, std::f32::consts::PI, true);
        assert_eq!(s, 0.0);
        assert!((e + std::f32::consts::PI).abs() < 1e-6);
        // ellipse(..., PI, 0): clockwise half turn.
        let (s, e) = angles(std::f32::consts::PI, 0.0, false);
        assert!((e - s - std::f32::consts::PI).abs() < 1e-6);
    }
}
