//! The subset of the Canvas 2D API that the texture generators use, on
//! tiny-skia, with the same method names in snake case so texture code
//! ports line for line (SPEC 5.3, roadmap WP 3.2).
//!
//! ```
//! use mr_canvas::Canvas;
//! let mut g = Canvas::new(128, 128);
//! g.set_fill_style("#f2c230");
//! g.fill_rect(0.0, 0.0, 128.0, 128.0);
//! let rgba = g.to_rgba(); // what a CanvasTexture uploads
//! assert_eq!(&rgba[..4], &[242, 194, 48, 255]);
//! ```
//!
//! # What the game uses
//!
//! Every 2D-context call in `src/world/**` and `src/vehicles/CarModel.js`
//! (the texture generators), counted on 2026-10-03 (receivers `g`, `ge`,
//! `gm`, `gv`, `g2`, `fg`, `this.g`; property counts are assignments):
//!
//! | API | Uses | Notes |
//! |---|---|---|
//! | `fillStyle =` | 497 | `#rgb`, `#rrggbb`, `rgb()`, `rgba()`, `hsl()`, `white`, `black`, gradients |
//! | `fillRect` | 472 | |
//! | `addColorStop` | 88 | |
//! | `fillText` | 74 | 4 with `maxWidth` |
//! | `beginPath` | 67 | |
//! | `font =` | 66 | `[italic] [bold\|700\|900] Npx family, ...` |
//! | `lineTo` | 63 | |
//! | `fill` | 51 | nonzero only |
//! | `textAlign =` | 44 | `center` 37, `left` 4, `right` 3 |
//! | `textBaseline =` | 37 | `middle` 36, `alphabetic` 1 |
//! | `lineWidth =` | 35 | |
//! | `strokeStyle =` | 35 | |
//! | `createLinearGradient` | 33 | |
//! | `moveTo` | 30 | |
//! | `shadowBlur =` | 22 | offsets never set |
//! | `stroke` | 21 | |
//! | `quadraticCurveTo` | 20 | |
//! | `save`, `restore` | 17 each | |
//! | `arc` | 16 | one anticlockwise |
//! | `globalAlpha =` | 14 | |
//! | `closePath` | 12 | |
//! | `putImageData` | 12 | |
//! | `translate` | 12 | |
//! | `shadowColor =` | 12 | |
//! | `strokeRect`, `rect`, `ellipse` | 11 each | `ellipse` with rotation, and end < start |
//! | `clip` | 10 | rectangles |
//! | `clearRect` | 8 | |
//! | `globalCompositeOperation =` | 7 | `source-over` 3, `lighter` 2, `destination-out` 1, `lighten` 1 |
//! | `createImageData` | 7 | |
//! | `strokeText` | 5 | |
//! | `getImageData` | 5 | whole canvas |
//! | `rotate` | 5 | |
//! | `drawImage` | 5 | canvas sources, 5- and 9-argument forms |
//! | `lineCap =` | 4 | `round` 2, `butt` 2 |
//! | `lineJoin =` | 2 | `round` |
//! | `filter =` | 2 | `blur(6px)`, `none` |
//! | `scale` | 2 | |
//! | `createRadialGradient` | 2 | inner radius 0, same centres |
//! | `roundRect` | 1 | one radius |
//! | `measureText` | 1 | `.width` |
//!
//! Not used, so not here: `bezierCurveTo` (present anyway, it is the
//! primitive arcs become), `arcTo`, `setTransform`, `transform`,
//! `setLineDash`, `createPattern`, `createConicGradient`, shadow offsets,
//! `miterLimit` (settable, never set), the even-odd fill rule, `isPointIn*`,
//! `direction`, `letterSpacing`, image smoothing settings. Composite
//! operations and filters outside the table panic rather than draw wrongly.
//! Synthetic bold is not done: the bundled faces cover every weight the
//! game asks for (DECISIONS D150).
//!
//! # Semantics kept from Chrome
//!
//! - Pixels are premultiplied RGBA8 in sRGB, blended without linearising.
//!   `get_image_data` unpremultiplies (`round(c·255/a)`), `put_image_data`
//!   premultiplies and ignores transform, clip, alpha and compositing.
//!   [`ImageData::set`] stores as a `Uint8ClampedArray` does (round half to
//!   even).
//! - Anti-aliasing is exact area coverage (the closest single rule to
//!   Chrome's accelerated canvas, which is exact for rectangles; DECISIONS
//!   D151), the nonzero rule.
//! - Path points are transformed when added; strokes are made in the user
//!   space of the transform at stroke time.
//! - `arc`/`ellipse` angles follow Blink's canonicalisation (a sweep of 2π
//!   or more is one turn; `end < start` clockwise wraps).
//! - Gradients interpolate unpremultiplied and pad, with Skia's 8×8 ordered
//!   dither on the colour channels (D153); a radial gradient is the
//!   two-circle cone of the HTML spec.
//! - Shadows: the shape's alpha blurred with σ = `shadowBlur`/2, in
//!   `shadowColor`, composited before the shape. `filter: blur(r)` blurs the
//!   shape's layer with σ = r.
//! - Text (D152): per-character fallback through the family list, shaping
//!   word by word with harfrust (HarfBuzz), baselines from Blink's
//!   normalised typo metrics, glyph origins on Skia's grid (quarter pixel
//!   along the baseline, whole pixel across), outlines from skrifa hinted by
//!   the light automatic hinter, and Skia's A8 mask gamma for the fill
//!   colour.
//!
//! The parity of all this is checked against Chrome by
//! `tests/probes.rs` (small scenes for every feature above) and by
//! `mr_worldgen`'s texture gate, both from `tools/parity/textures.mjs`;
//! [`compare`] has the metric and the side-by-side sheet.

#![forbid(unsafe_code)]

pub mod canvas;
pub mod color;
pub mod compare;
pub mod fonts;
pub mod paint;
pub mod path;
pub mod raster;
pub mod text;

pub use canvas::{Canvas, ImageData, TextMetrics, to_uint8_clamp};
pub use color::Rgba;
pub use fonts::{FontBook, FontSpec};
pub use paint::{Gradient, Style};

use mr_math::kernel;

/// A 2D affine transform in the canvas's convention:
/// `x' = a·x + c·y + e`, `y' = b·x + d·y + f`. Doubles, as Blink keeps it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Matrix {
    pub const IDENTITY: Matrix = Matrix {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    pub fn translate(x: f64, y: f64) -> Matrix {
        Matrix {
            e: x,
            f: y,
            ..Matrix::IDENTITY
        }
    }

    pub fn scale(x: f64, y: f64) -> Matrix {
        Matrix {
            a: x,
            d: y,
            ..Matrix::IDENTITY
        }
    }

    pub fn rotate(angle: f64) -> Matrix {
        let (s, c) = (kernel::sin(angle), kernel::cos(angle));
        Matrix {
            a: c,
            b: s,
            c: -s,
            d: c,
            e: 0.0,
            f: 0.0,
        }
    }

    /// `self · o`: `o` applies first (the canvas's `transform` order).
    pub fn mul(&self, o: &Matrix) -> Matrix {
        Matrix {
            a: self.a * o.a + self.c * o.b,
            b: self.b * o.a + self.d * o.b,
            c: self.a * o.c + self.c * o.d,
            d: self.b * o.c + self.d * o.d,
            e: self.a * o.e + self.c * o.f + self.e,
            f: self.b * o.e + self.d * o.f + self.f,
        }
    }

    #[inline]
    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// The linear part only (a direction).
    #[inline]
    pub fn apply_vec(&self, x: f64, y: f64) -> (f64, f64) {
        (self.a * x + self.c * y, self.b * x + self.d * y)
    }

    pub fn invert(&self) -> Option<Matrix> {
        let det = self.a * self.d - self.b * self.c;
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        let (a, b, c, d) = (self.d / det, -self.b / det, -self.c / det, self.a / det);
        Some(Matrix {
            a,
            b,
            c,
            d,
            e: -(a * self.e + c * self.f),
            f: -(b * self.e + d * self.f),
        })
    }

    pub fn to_skia(&self) -> tiny_skia::Transform {
        tiny_skia::Transform::from_row(
            self.a as f32,
            self.b as f32,
            self.c as f32,
            self.d as f32,
            self.e as f32,
            self.f as f32,
        )
    }
}
