//! Exact-area coverage, the anti-aliasing Chrome's accelerated canvas and
//! FreeType's glyph renderer both approximate closely (DECISIONS D151).
//!
//! A path is flattened to lines; each line adds its signed area to the
//! cells it crosses (the accumulation scheme of font-rs and FreeType's
//! "smooth" renderer), and a running sum along each row gives the winding
//! integrated over the pixel. Coverage is `min(|w|, 1)`: the nonzero rule.
//! Work is limited to a box (the path's bounds within the canvas).

use tiny_skia::{Path, PathSegment, Point};

/// Coverage over a box of device pixels: `data[y * w + x]` for the pixel at
/// `(x0 + x, y0 + y)`, in 0..=1.
#[derive(Clone, Debug)]
pub struct Coverage {
    pub x0: i32,
    pub y0: i32,
    pub w: usize,
    pub h: usize,
    pub data: Vec<f32>,
}

impl Coverage {
    pub fn empty() -> Coverage {
        Coverage {
            x0: 0,
            y0: 0,
            w: 0,
            h: 0,
            data: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }

    /// Coverage at a device pixel (0 outside the box).
    pub fn at(&self, x: i32, y: i32) -> f32 {
        let (lx, ly) = (x - self.x0, y - self.y0);
        if lx < 0 || ly < 0 || lx as usize >= self.w || ly as usize >= self.h {
            0.0
        } else {
            self.data[ly as usize * self.w + lx as usize]
        }
    }

    /// The union of two coverages, as overlapping glyph masks composite:
    /// `1 - (1 - a)(1 - b)`, over the box of `self` grown to hold `other`.
    pub fn union(self, other: &Coverage) -> Coverage {
        if other.is_empty() {
            return self;
        }
        if self.is_empty() {
            return other.clone();
        }
        let x0 = self.x0.min(other.x0);
        let y0 = self.y0.min(other.y0);
        let x1 = (self.x0 + self.w as i32).max(other.x0 + other.w as i32);
        let y1 = (self.y0 + self.h as i32).max(other.y0 + other.h as i32);
        let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
        let mut out = Coverage {
            x0,
            y0,
            w,
            h,
            data: vec![0.0; w * h],
        };
        for src in [&self, other] {
            for y in 0..src.h {
                let oy = (src.y0 - y0) as usize + y;
                for x in 0..src.w {
                    let ox = (src.x0 - x0) as usize + x;
                    let a = &mut out.data[oy * w + ox];
                    let b = src.data[y * src.w + x];
                    *a = 1.0 - (1.0 - *a) * (1.0 - b);
                }
            }
        }
        out
    }
}

/// Integer pixel box `[x0, x1) × [y0, y1)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IBox {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl IBox {
    pub fn intersect(self, o: IBox) -> Option<IBox> {
        let b = IBox {
            x0: self.x0.max(o.x0),
            y0: self.y0.max(o.y0),
            x1: self.x1.min(o.x1),
            y1: self.y1.min(o.y1),
        };
        (b.x0 < b.x1 && b.y0 < b.y1).then_some(b)
    }

    /// The pixels touched by a float rectangle.
    pub fn around(l: f32, t: f32, r: f32, b: f32) -> IBox {
        let c = |v: f32| v.clamp(-1.0e7, 1.0e7);
        IBox {
            x0: c(l).floor() as i32,
            y0: c(t).floor() as i32,
            x1: c(r).ceil() as i32,
            y1: c(b).ceil() as i32,
        }
    }
}

/// Flattening tolerance in device pixels.
const TOL: f32 = 0.02;

/// Accumulates lines into a box and resolves them to coverage.
pub struct Rasterizer {
    bx: IBox,
    w: usize,
    h: usize,
    /// `h` rows of `w + 2` cells.
    acc: Vec<f32>,
}

impl Rasterizer {
    pub fn new(bx: IBox) -> Rasterizer {
        let w = (bx.x1 - bx.x0) as usize;
        let h = (bx.y1 - bx.y0) as usize;
        Rasterizer {
            bx,
            w,
            h,
            acc: vec![0.0; (w + 2) * h],
        }
    }

    /// Adds every contour of a device-space path, each closed (a fill).
    pub fn add_path(&mut self, path: &Path) {
        let ox = self.bx.x0 as f32;
        let oy = self.bx.y0 as f32;
        let loc = |p: Point| (p.x - ox, p.y - oy);
        let mut start = (0.0, 0.0);
        let mut cur = (0.0, 0.0);
        let mut open = false;
        for seg in path.segments() {
            match seg {
                PathSegment::MoveTo(p) => {
                    if open {
                        self.line(cur, start);
                    }
                    start = loc(p);
                    cur = start;
                    open = true;
                }
                PathSegment::LineTo(p) => {
                    let p = loc(p);
                    self.line(cur, p);
                    cur = p;
                }
                PathSegment::QuadTo(c, p) => {
                    let (c, p) = (loc(c), loc(p));
                    self.quad(cur, c, p);
                    cur = p;
                }
                PathSegment::CubicTo(c1, c2, p) => {
                    let (c1, c2, p) = (loc(c1), loc(c2), loc(p));
                    self.cubic(cur, c1, c2, p);
                    cur = p;
                }
                PathSegment::Close => {
                    self.line(cur, start);
                    cur = start;
                    open = false;
                }
            }
        }
        if open {
            self.line(cur, start);
        }
    }

    fn quad(&mut self, p0: (f32, f32), p1: (f32, f32), p2: (f32, f32)) {
        let dx = p0.0 - 2.0 * p1.0 + p2.0;
        let dy = p0.1 - 2.0 * p1.1 + p2.1;
        let dd = (dx * dx + dy * dy).sqrt();
        let n = ((dd / (8.0 * TOL)).sqrt().ceil() as usize).clamp(1, 512);
        let mut prev = p0;
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let mt = 1.0 - t;
            let p = (
                mt * mt * p0.0 + 2.0 * mt * t * p1.0 + t * t * p2.0,
                mt * mt * p0.1 + 2.0 * mt * t * p1.1 + t * t * p2.1,
            );
            self.line(prev, p);
            prev = p;
        }
    }

    fn cubic(&mut self, p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), p3: (f32, f32)) {
        let len = |x: f32, y: f32| (x * x + y * y).sqrt();
        let d1 = len(p0.0 - 2.0 * p1.0 + p2.0, p0.1 - 2.0 * p1.1 + p2.1);
        let d2 = len(p1.0 - 2.0 * p2.0 + p3.0, p1.1 - 2.0 * p2.1 + p3.1);
        let dd = d1.max(d2);
        let n = ((3.0 * dd / (4.0 * TOL)).sqrt().ceil() as usize).clamp(1, 1024);
        let mut prev = p0;
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let mt = 1.0 - t;
            let a = mt * mt * mt;
            let b = 3.0 * mt * mt * t;
            let c = 3.0 * mt * t * t;
            let d = t * t * t;
            let p = (
                a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0,
                a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1,
            );
            self.line(prev, p);
            prev = p;
        }
    }

    /// A line in box coordinates: clipped to the rows, clamped to the
    /// columns (a part left of the box still winds the pixels to its right).
    pub fn line(&mut self, a: (f32, f32), b: (f32, f32)) {
        if !(a.0.is_finite() && a.1.is_finite() && b.0.is_finite() && b.1.is_finite()) {
            return;
        }
        let h = self.h as f32;
        let (mut a, mut b) = (a, b);
        // Rows.
        let (lo, hi) = if a.1 < b.1 { (a.1, b.1) } else { (b.1, a.1) };
        if hi <= 0.0 || lo >= h || a.1 == b.1 {
            return;
        }
        let at_y =
            |p: (f32, f32), q: (f32, f32), y: f32| (p.0 + (q.0 - p.0) * (y - p.1) / (q.1 - p.1), y);
        if a.1 < 0.0 {
            a = at_y(a, b, 0.0);
        } else if a.1 > h {
            a = at_y(a, b, h);
        }
        if b.1 < 0.0 {
            b = at_y(a, b, 0.0);
        } else if b.1 > h {
            b = at_y(a, b, h);
        }
        // Columns: split where the line crosses 0 or w, then clamp.
        let w = self.w as f32;
        let mut pts = [a, a, a, b];
        let mut n = 1;
        let mut ts = [2.0f32, 2.0];
        let mut k = 0;
        for edge in [0.0, w] {
            if (a.0 - edge) * (b.0 - edge) < 0.0 {
                ts[k] = (edge - a.0) / (b.0 - a.0);
                k += 1;
            }
        }
        if ts[0] > ts[1] {
            ts.swap(0, 1);
        }
        for &t in &ts[..k] {
            pts[n] = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            n += 1;
        }
        pts[n] = b;
        n += 1;
        for i in 0..n - 1 {
            let p = (pts[i].0.clamp(0.0, w), pts[i].1);
            let q = (pts[i + 1].0.clamp(0.0, w), pts[i + 1].1);
            self.line_inside(p, q);
        }
    }

    /// font-rs's accumulation for a line within the box.
    fn line_inside(&mut self, p0: (f32, f32), p1: (f32, f32)) {
        if p0.1 == p1.1 {
            return;
        }
        let stride = self.w + 2;
        let (dir, p0, p1) = if p0.1 < p1.1 {
            (1.0f32, p0, p1)
        } else {
            (-1.0, p1, p0)
        };
        let dxdy = (p1.0 - p0.0) / (p1.1 - p0.1);
        let mut x = p0.0;
        let y_start = p0.1.max(0.0) as usize;
        let y_end = (p1.1.ceil() as usize).min(self.h);
        for y in y_start..y_end {
            let row = y * stride;
            let dy = ((y + 1) as f32).min(p1.1) - (y as f32).max(p0.1);
            let xnext = x + dxdy * dy;
            let d = dy * dir;
            let (x0, x1) = if x < xnext { (x, xnext) } else { (xnext, x) };
            let x0floor = x0.floor();
            let x0i = x0floor as usize;
            let x1ceil = x1.ceil();
            let x1i = x1ceil as usize;
            if x1i <= x0i + 1 {
                let xmf = 0.5 * (x + xnext) - x0floor;
                self.acc[row + x0i] += d - d * xmf;
                self.acc[row + x0i + 1] += d * xmf;
            } else {
                let s = (x1 - x0).recip();
                let x0f = x0 - x0floor;
                let a0 = 0.5 * s * (1.0 - x0f) * (1.0 - x0f);
                let x1f = x1 - x1ceil + 1.0;
                let am = 0.5 * s * x1f * x1f;
                self.acc[row + x0i] += d * a0;
                if x1i == x0i + 2 {
                    self.acc[row + x0i + 1] += d * (1.0 - a0 - am);
                } else {
                    let a1 = s * (1.5 - x0f);
                    self.acc[row + x0i + 1] += d * (a1 - a0);
                    for xi in x0i + 2..x1i - 1 {
                        self.acc[row + xi] += d * s;
                    }
                    let a2 = a1 + (x1i - x0i - 3) as f32 * s;
                    self.acc[row + x1i - 1] += d * (1.0 - a2 - am);
                }
                self.acc[row + x1i] += d * am;
            }
            x = xnext;
        }
    }

    pub fn finish(self) -> Coverage {
        let stride = self.w + 2;
        let mut data = vec![0.0f32; self.w * self.h];
        for y in 0..self.h {
            let mut acc = 0.0f32;
            for x in 0..self.w {
                acc += self.acc[y * stride + x];
                data[y * self.w + x] = acc.abs().min(1.0);
            }
        }
        Coverage {
            x0: self.bx.x0,
            y0: self.bx.y0,
            w: self.w,
            h: self.h,
            data,
        }
    }
}

/// Coverage of a device-space path (nonzero), limited to `limit`.
pub fn fill_coverage(path: &Path, limit: IBox) -> Coverage {
    let b = path.bounds();
    let Some(bx) = IBox::around(b.left(), b.top(), b.right(), b.bottom()).intersect(limit) else {
        return Coverage::empty();
    };
    let mut r = Rasterizer::new(bx);
    r.add_path(path);
    r.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::PathBuilder;

    fn rect(l: f32, t: f32, r: f32, b: f32) -> Path {
        let mut pb = PathBuilder::new();
        pb.move_to(l, t);
        pb.line_to(r, t);
        pb.line_to(r, b);
        pb.line_to(l, b);
        pb.close();
        pb.finish().unwrap()
    }

    #[test]
    fn rect_edges_are_exact_area() {
        let c = fill_coverage(
            &rect(10.3, 10.0, 30.3, 15.0),
            IBox {
                x0: 0,
                y0: 0,
                x1: 64,
                y1: 64,
            },
        );
        assert!((c.at(10, 12) - 0.7).abs() < 1e-5);
        assert!((c.at(11, 12) - 1.0).abs() < 1e-5);
        assert!((c.at(30, 12) - 0.3).abs() < 1e-5);
        assert_eq!(c.at(31, 12), 0.0);
        assert_eq!(c.at(12, 15), 0.0);
    }

    #[test]
    fn clipped_to_the_box() {
        // Mostly outside on the left and the top: what is inside is still full.
        let c = fill_coverage(
            &rect(-50.0, -50.0, 5.5, 5.0),
            IBox {
                x0: 0,
                y0: 0,
                x1: 16,
                y1: 16,
            },
        );
        assert!((c.at(0, 0) - 1.0).abs() < 1e-5);
        assert!((c.at(5, 4) - 0.5).abs() < 1e-5);
        assert_eq!(c.at(6, 4), 0.0);
    }

    #[test]
    fn nonzero_and_holes() {
        let mut pb = PathBuilder::new();
        for (l, t, r, b, cw) in [(0.0, 0.0, 10.0, 10.0, true), (2.0, 2.0, 8.0, 8.0, false)] {
            pb.move_to(l, t);
            if cw {
                pb.line_to(r, t);
                pb.line_to(r, b);
                pb.line_to(l, b);
            } else {
                pb.line_to(l, b);
                pb.line_to(r, b);
                pb.line_to(r, t);
            }
            pb.close();
        }
        let c = fill_coverage(
            &pb.finish().unwrap(),
            IBox {
                x0: 0,
                y0: 0,
                x1: 16,
                y1: 16,
            },
        );
        assert_eq!(c.at(5, 5), 0.0);
        assert!((c.at(1, 5) - 1.0).abs() < 1e-5);
    }
}
