//! What a fill or stroke paints with: a colour, a gradient, or another
//! canvas (drawImage); and how it lands on the pixels (composite
//! operations, Gaussian blur for shadows and `filter: blur()`).

use mp_math::kernel;

use crate::Matrix;
use crate::color::{self, Rgba};

/// A `CanvasGradient`.
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub kind: GradientKind,
    /// Offsets and colours in the order the canvas keeps them (sorted by
    /// offset; equal offsets in the order added).
    pub stops: Vec<(f64, Rgba)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GradientKind {
    Linear {
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    },
    Radial {
        x0: f64,
        y0: f64,
        r0: f64,
        x1: f64,
        y1: f64,
        r1: f64,
    },
}

impl Gradient {
    /// `addColorStop(offset, color)`. The browser throws on an offset
    /// outside 0..1 or a colour it cannot parse; a port that does either has
    /// a bug, so this panics.
    pub fn add_color_stop(&mut self, offset: f64, color: &str) {
        assert!(
            (0.0..=1.0).contains(&offset),
            "addColorStop: offset {offset} outside 0..1"
        );
        let c = color::parse(color).unwrap_or_else(|| panic!("addColorStop: bad colour {color:?}"));
        let at = self
            .stops
            .iter()
            .position(|(o, _)| *o > offset)
            .unwrap_or(self.stops.len());
        self.stops.insert(at, (offset, c));
    }

    /// The gradient parameter at a user-space point, or `None` where a
    /// radial gradient is not drawn.
    fn t(&self, x: f64, y: f64) -> Option<f64> {
        match self.kind {
            GradientKind::Linear { x0, y0, x1, y1 } => {
                let (dx, dy) = (x1 - x0, y1 - y0);
                let len2 = dx * dx + dy * dy;
                if len2 == 0.0 {
                    return None;
                }
                Some(((x - x0) * dx + (y - y0) * dy) / len2)
            }
            GradientKind::Radial {
                x0,
                y0,
                r0,
                x1,
                y1,
                r1,
            } => {
                // The largest ω with |p - c(ω)| = r(ω) and r(ω) ≥ 0, where
                // c and r interpolate the two circles (HTML spec).
                let (cdx, cdy, dr) = (x1 - x0, y1 - y0, r1 - r0);
                let (px, py) = (x - x0, y - y0);
                let a = cdx * cdx + cdy * cdy - dr * dr;
                let b = px * cdx + py * cdy + r0 * dr;
                let c = px * px + py * py - r0 * r0;
                if a.abs() < 1e-12 {
                    if b == 0.0 {
                        return None;
                    }
                    let w = c / (2.0 * b);
                    return (r0 + w * dr >= 0.0).then_some(w);
                }
                let disc = b * b - a * c;
                if disc < 0.0 {
                    return None;
                }
                let sq = disc.sqrt();
                let (w1, w2) = ((b + sq) / a, (b - sq) / a);
                let (hi, lo) = if w1 > w2 { (w1, w2) } else { (w2, w1) };
                if r0 + hi * dr >= 0.0 {
                    Some(hi)
                } else if r0 + lo * dr >= 0.0 {
                    Some(lo)
                } else {
                    None
                }
            }
        }
    }

    /// Premultiplied colour at parameter `t`, padded at both ends; the
    /// stops interpolate unpremultiplied, as Skia's gradients do by default.
    fn color_at(&self, t: f64) -> [f32; 4] {
        let s = &self.stops;
        if s.is_empty() {
            return [0.0; 4];
        }
        let t = t.clamp(0.0, 1.0);
        if t <= s[0].0 {
            return s[0].1.premul();
        }
        for w in s.windows(2) {
            let ((o0, c0), (o1, c1)) = (w[0], w[1]);
            if t <= o1 {
                if o1 <= o0 {
                    return c1.premul();
                }
                let f = ((t - o0) / (o1 - o0)) as f32;
                let mix = |a: f32, b: f32| a + (b - a) * f;
                return Rgba {
                    r: mix(c0.r, c1.r),
                    g: mix(c0.g, c1.g),
                    b: mix(c0.b, c1.b),
                    a: mix(c0.a, c1.a),
                }
                .premul();
            }
        }
        s[s.len() - 1].1.premul()
    }
}

/// A fill or stroke style.
#[derive(Clone, Debug, PartialEq)]
pub enum Style {
    Color(Rgba),
    Gradient(Gradient),
}

/// What a style setter accepts: a CSS colour string or a gradient.
pub trait IntoStyle {
    /// `None` for a colour string the browser would ignore.
    fn into_style(self) -> Option<Style>;
}

impl IntoStyle for &str {
    fn into_style(self) -> Option<Style> {
        color::parse(self).map(Style::Color)
    }
}

impl IntoStyle for &String {
    fn into_style(self) -> Option<Style> {
        self.as_str().into_style()
    }
}

impl IntoStyle for String {
    fn into_style(self) -> Option<Style> {
        self.as_str().into_style()
    }
}

impl IntoStyle for &Gradient {
    fn into_style(self) -> Option<Style> {
        Some(Style::Gradient(self.clone()))
    }
}

impl IntoStyle for Gradient {
    fn into_style(self) -> Option<Style> {
        Some(Style::Gradient(self))
    }
}

/// A style resolved for one draw: device pixel centre to premultiplied
/// colour, with `globalAlpha` applied.
pub enum Source<'a> {
    Solid([f32; 4]),
    Gradient {
        g: &'a Gradient,
        inv: Matrix,
        alpha: f32,
    },
    /// Another canvas's premultiplied pixels, sampled bilinearly in the
    /// source rectangle `[sx0, sx1) × [sy0, sy1)`; `inv` maps device to
    /// source pixel coordinates.
    Image {
        data: &'a [u8],
        width: usize,
        inv: Matrix,
        rect: (f64, f64, f64, f64),
        alpha: f32,
    },
}

impl Source<'_> {
    pub fn from_style<'s>(style: &'s Style, m: &Matrix, alpha: f64) -> Option<Source<'s>> {
        match style {
            Style::Color(c) => {
                let p = c.premul();
                let a = alpha as f32;
                Some(Source::Solid([p[0] * a, p[1] * a, p[2] * a, p[3] * a]))
            }
            Style::Gradient(g) => Some(Source::Gradient {
                g,
                inv: m.invert()?,
                alpha: alpha as f32,
            }),
        }
    }

    /// Whether every pixel would be transparent (nothing to draw).
    pub fn is_clear(&self) -> bool {
        match self {
            Source::Solid(c) => c[3] == 0.0,
            Source::Gradient { alpha, .. } | Source::Image { alpha, .. } => *alpha == 0.0,
        }
    }

    #[inline]
    pub fn sample(&self, px: f64, py: f64) -> [f32; 4] {
        match self {
            Source::Solid(c) => *c,
            Source::Gradient { g, inv, alpha } => {
                let (ux, uy) = inv.apply(px, py);
                match g.t(ux, uy) {
                    Some(t) => {
                        let c = g.color_at(t);
                        let a = c[3] * alpha;
                        let d = dither(px, py);
                        [
                            (c[0] * alpha + d).clamp(0.0, a),
                            (c[1] * alpha + d).clamp(0.0, a),
                            (c[2] * alpha + d).clamp(0.0, a),
                            a,
                        ]
                    }
                    None => [0.0; 4],
                }
            }
            Source::Image {
                data,
                width,
                inv,
                rect,
                alpha,
            } => {
                let (sx, sy) = inv.apply(px, py);
                let (x0, y0, x1, y1) = *rect;
                // Bilinear between texel centres, clamped to the source
                // rectangle's edge texels.
                let fx = (sx - 0.5).clamp(x0, x1 - 1.0);
                let fy = (sy - 0.5).clamp(y0, y1 - 1.0);
                let (ix, iy) = (fx.floor(), fy.floor());
                let (tx, ty) = ((fx - ix) as f32, (fy - iy) as f32);
                let ix1 = (ix + 1.0).min(x1 - 1.0);
                let iy1 = (iy + 1.0).min(y1 - 1.0);
                let px = |x: f64, y: f64| {
                    let i = (y as usize * *width + x as usize) * 4;
                    [
                        data[i] as f32 / 255.0,
                        data[i + 1] as f32 / 255.0,
                        data[i + 2] as f32 / 255.0,
                        data[i + 3] as f32 / 255.0,
                    ]
                };
                let (a, b, c, d) = (px(ix, iy), px(ix1, iy), px(ix, iy1), px(ix1, iy1));
                let mut o = [0.0f32; 4];
                for k in 0..4 {
                    let top = a[k] + (b[k] - a[k]) * tx;
                    let bot = c[k] + (d[k] - c[k]) * tx;
                    o[k] = (top + (bot - top) * ty) * alpha;
                }
                o
            }
        }
    }
}

/// Skia's gradient dither on the GPU (`make_dither_lut` and the dither
/// effect, for 8-bit targets): an 8×8 ordered pattern of ±½ level added to
/// the premultiplied colour channels (not alpha), clamped to alpha. Chrome's
/// canvas gradients are drawn with it (measured: the RGB of a translucent
/// white gradient dips below 255 in this pattern).
fn dither(px: f64, py: f64) -> f32 {
    let (x, y) = (px.floor() as i64 as u32 & 7, py.floor() as i64 as u32 & 7);
    let m = (y & 1) << 5 | (x & 1) << 4 | (y & 2) << 2 | (x & 2) << 1 | (y & 4) >> 1 | (x & 4) >> 2;
    let value = m as f32 / 64.0 - 63.0 / 128.0;
    let byte = ((value + 0.5) * 255.0 + 0.5) as u8;
    (byte as f32 / 255.0 - 0.5) / 255.0
}

/// `globalCompositeOperation` values in use, plus the clear that
/// `clearRect` does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    SourceOver,
    DestinationOut,
    Lighter,
    Lighten,
    Clear,
}

impl Op {
    /// `None` for a name the browser would ignore. A valid name this crate
    /// does not implement panics: the texture code would be silently wrong.
    pub fn parse(s: &str) -> Option<Op> {
        Some(match s {
            "source-over" => Op::SourceOver,
            "destination-out" => Op::DestinationOut,
            "lighter" => Op::Lighter,
            "lighten" => Op::Lighten,
            "source-in" | "source-out" | "source-atop" | "destination-over" | "destination-in"
            | "destination-atop" | "copy" | "xor" | "multiply" | "screen" | "overlay"
            | "darken" | "color-dodge" | "color-burn" | "hard-light" | "soft-light"
            | "difference" | "exclusion" | "hue" | "saturation" | "color" | "luminosity" => {
                panic!("mp_canvas: globalCompositeOperation {s:?} is not implemented")
            }
            _ => return None,
        })
    }

    /// The result for a premultiplied source already scaled by coverage
    /// (Porter-Duff and the separable blend; coverage scales linearly
    /// through each).
    #[inline]
    pub fn blend(self, s: [f32; 4], d: [f32; 4]) -> [f32; 4] {
        match self {
            Op::SourceOver => {
                let k = 1.0 - s[3];
                [
                    s[0] + d[0] * k,
                    s[1] + d[1] * k,
                    s[2] + d[2] * k,
                    s[3] + d[3] * k,
                ]
            }
            Op::DestinationOut => {
                let k = 1.0 - s[3];
                [d[0] * k, d[1] * k, d[2] * k, d[3] * k]
            }
            Op::Lighter => [
                (s[0] + d[0]).min(1.0),
                (s[1] + d[1]).min(1.0),
                (s[2] + d[2]).min(1.0),
                (s[3] + d[3]).min(1.0),
            ],
            Op::Lighten => {
                let (sa, da) = (s[3], d[3]);
                let ch = |sc: f32, dc: f32| sc + dc - (sc * da).min(dc * sa);
                [
                    ch(s[0], d[0]),
                    ch(s[1], d[1]),
                    ch(s[2], d[2]),
                    sa + da - sa * da,
                ]
            }
            // `s` here is the coverage in its alpha.
            Op::Clear => {
                let k = 1.0 - s[3];
                [d[0] * k, d[1] * k, d[2] * k, d[3] * k]
            }
        }
    }
}

/// A Gaussian kernel of standard deviation `sigma`, out to 3σ, normalised.
pub fn gauss_kernel(sigma: f64) -> Vec<f32> {
    let r = (3.0 * sigma).ceil() as i32;
    let mut k: Vec<f64> = (-r..=r)
        .map(|i| kernel::exp(-((i * i) as f64) / (2.0 * sigma * sigma)))
        .collect();
    let sum: f64 = k.iter().sum();
    for v in &mut k {
        *v /= sum;
    }
    k.into_iter().map(|v| v as f32).collect()
}

/// Blurs `ch` interleaved channels of a `w × h` buffer in place (zero
/// outside), separably.
pub fn blur(buf: &mut [f32], w: usize, h: usize, ch: usize, sigma: f64) {
    if sigma <= 0.0 || w == 0 || h == 0 {
        return;
    }
    let k = gauss_kernel(sigma);
    let r = (k.len() / 2) as isize;
    let mut tmp = vec![0.0f32; buf.len()];
    for y in 0..h {
        for x in 0..w {
            for c in 0..ch {
                let mut acc = 0.0f32;
                for (j, kv) in k.iter().enumerate() {
                    let sx = x as isize + j as isize - r;
                    if sx >= 0 && (sx as usize) < w {
                        acc += buf[(y * w + sx as usize) * ch + c] * kv;
                    }
                }
                tmp[(y * w + x) * ch + c] = acc;
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            for c in 0..ch {
                let mut acc = 0.0f32;
                for (j, kv) in k.iter().enumerate() {
                    let sy = y as isize + j as isize - r;
                    if sy >= 0 && (sy as usize) < h {
                        acc += tmp[(sy as usize * w + x) * ch + c] * kv;
                    }
                }
                buf[(y * w + x) * ch + c] = acc;
            }
        }
    }
}
