//! Port of `src/world/beach/atlas.js` (roadmap WP 7.1; Desert uses it too).
//!
//! Signs for the whole town are drawn into two canvas atlases (painted signs
//! and neon), so every storefront, pole sign and billboard in Seabright
//! shares two materials instead of one each.

use std::sync::Arc;

use mp_canvas::Canvas;
use mp_scene::TextureSource;

use crate::object::{Image, SceneGraph, TextureId};
use crate::textures::Texture;
use crate::three_geom::{BufferGeometry, plane_geometry};

/// An atlas cell: `{ u0, u1, v0, v1, aspect }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub u0: f64,
    pub u1: f64,
    pub v0: f64,
    pub v1: f64,
    pub aspect: f64,
}

/// `SignAtlas(size = 2048, bg = null)`.
pub struct SignAtlas {
    pub s: f64,
    pub canvas: Canvas,
    x: f64,
    y: f64,
    row_h: f64,
    pad: f64,
}

impl SignAtlas {
    pub fn new(size: u32, bg: Option<&str>) -> SignAtlas {
        let mut canvas = Canvas::new(size, size);
        let s = f64::from(size);
        if let Some(bg) = bg {
            canvas.set_fill_style(bg);
            canvas.fill_rect(0.0, 0.0, s, s);
        }
        SignAtlas {
            s,
            canvas,
            x: 0.0,
            y: 0.0,
            row_h: 0.0,
            pad: 4.0,
        }
    }

    /// Reserve a w×h pixel cell, draw into it, return its UV rectangle.
    /// Panics where the JS throws `'sign atlas full'`.
    pub fn add(&mut self, w: f64, h: f64, draw: impl FnOnce(&mut Canvas, f64, f64)) -> Rect {
        let s = self.s;
        let p = self.pad;
        if self.x + w + p > s {
            self.x = 0.0;
            self.y += self.row_h + p;
            self.row_h = 0.0;
        }
        assert!(self.y + h <= s, "sign atlas full");
        let (x, y) = (self.x, self.y);
        self.x += w + p;
        self.row_h = mp_math::js::max(self.row_h, h);
        let g = &mut self.canvas;
        g.save();
        g.translate(x, y);
        g.begin_path();
        g.rect(0.0, 0.0, w, h);
        g.clip();
        draw(g, w, h);
        g.restore();
        // Inset half a texel so mipmaps don't bleed neighbours in.
        let e = 1.5;
        Rect {
            u0: (x + e) / s,
            u1: (x + w - e) / s,
            v0: 1.0 - (y + h - e) / s,
            v1: 1.0 - (y + e) / s,
            aspect: w / h,
        }
    }

    /// The canvas as uploaded: sRGB, anisotropy 8, clamped.
    pub fn picture(&self) -> Texture {
        Texture {
            width: self.canvas.width,
            height: self.canvas.height,
            rgba: self.canvas.to_rgba(),
            source: TextureSource::Canvas,
            repeat: false,
            srgb: true,
            anisotropy: 8.0,
        }
    }

    /// `texture()`: a new `CanvasTexture` of the atlas.
    pub fn texture(&self, graph: &mut SceneGraph) -> TextureId {
        let t = Arc::new(self.picture());
        let desc = t.desc("", 0);
        graph.add_texture(Image::Own(t), desc)
    }
}

/// A plane w×h (metres) textured with an atlas cell; front faces +Z.
pub fn sign_geometry(rect: &Rect, w: f64, h: f64) -> BufferGeometry {
    let mut g = plane_geometry(w, h, 1.0, 1.0);
    let uv = g.get_attribute_mut("uv").expect("uv");
    for i in 0..uv.count() {
        let (x, y) = (uv.get_x(i), uv.get_y(i));
        uv.set_xy(
            i,
            rect.u0 + x * (rect.u1 - rect.u0),
            rect.v0 + y * (rect.v1 - rect.v0),
        );
    }
    g
}

// ── Drawing helpers ─────────────────────────────────────────────────────

/// `paintedSign(text, opts)`'s options; `Default` is the JS defaults.
#[derive(Clone, Copy, Debug)]
pub struct PaintedOpts<'a> {
    pub bg: &'a str,
    pub fg: &'a str,
    pub border: Option<&'a str>,
    pub font: &'a str,
    pub sub: Option<&'a str>,
    pub sub_font: &'a str,
    pub stripe: Option<&'a str>,
}

impl Default for PaintedOpts<'_> {
    fn default() -> Self {
        PaintedOpts {
            bg: "#f4efe2",
            fg: "#1f5f8b",
            border: None,
            font: "bold 80px \"Arial Black\", Arial, sans-serif",
            sub: None,
            sub_font: "bold 34px Arial, sans-serif",
            stripe: None,
        }
    }
}

/// `paintedSign(text, opts)`: the drawing function for an atlas cell.
pub fn painted_sign<'a>(
    text: &'a str,
    o: PaintedOpts<'a>,
) -> impl FnOnce(&mut Canvas, f64, f64) + 'a {
    move |g, w, h| {
        g.set_fill_style(o.bg);
        g.fill_rect(0.0, 0.0, w, h);
        if let Some(stripe) = o.stripe {
            g.set_fill_style(stripe);
            g.fill_rect(0.0, h - h * 0.14, w, h * 0.14);
            g.fill_rect(0.0, 0.0, w, h * 0.08);
        }
        if let Some(border) = o.border {
            g.set_stroke_style(border);
            let lw = mp_math::js::max(6.0, h * 0.06);
            g.set_line_width(lw);
            g.stroke_rect(lw / 2.0, lw / 2.0, w - lw, h - lw);
        }
        g.set_fill_style(o.fg);
        g.set_text_align("center");
        g.set_text_baseline("middle");
        g.set_font(o.font);
        fit_text(g, text, w * 0.9);
        g.fill_text(
            text,
            w / 2.0,
            if o.sub.is_some() { h * 0.42 } else { h / 2.0 },
        );
        if let Some(sub) = o.sub {
            g.set_font(o.sub_font);
            fit_text(g, sub, w * 0.9);
            g.fill_text(sub, w / 2.0, h * 0.76);
        }
    }
}

/// `neonSign(text, opts)`'s options; `Default` is the JS defaults.
#[derive(Clone, Copy, Debug)]
pub struct NeonOpts<'a> {
    pub color: &'a str,
    pub glow: Option<&'a str>,
    pub font: &'a str,
    pub sub: Option<&'a str>,
    pub sub_color: &'a str,
    pub sub_font: &'a str,
    pub frame: bool,
}

impl Default for NeonOpts<'_> {
    fn default() -> Self {
        NeonOpts {
            color: "#ff4fa3",
            glow: None,
            font: "bold 86px \"Brush Script MT\", \"Segoe Script\", cursive",
            sub: None,
            sub_color: "#62f0ff",
            sub_font: "bold 40px Arial, sans-serif",
            frame: true,
        }
    }
}

/// `neonSign(text, opts)`: the drawing function for an atlas cell.
pub fn neon_sign<'a>(text: &'a str, o: NeonOpts<'a>) -> impl FnOnce(&mut Canvas, f64, f64) + 'a {
    move |g, w, h| {
        g.set_fill_style("#0b0a10");
        g.fill_rect(0.0, 0.0, w, h);
        if o.frame {
            g.set_stroke_style(o.sub_color);
            g.set_line_width(5.0);
            g.set_shadow_color(o.sub_color);
            g.set_shadow_blur(12.0);
            round_rect(g, 10.0, 10.0, w - 20.0, h - 20.0, 18.0);
            g.stroke();
        }
        g.set_text_align("center");
        g.set_text_baseline("middle");
        g.set_font(o.font);
        fit_text(g, text, w * 0.86);
        let y = if o.sub.is_some() { h * 0.42 } else { h / 2.0 };
        // `glow || color`.
        g.set_shadow_color(match o.glow {
            Some(c) if !c.is_empty() => c,
            _ => o.color,
        });
        g.set_shadow_blur(22.0);
        g.set_stroke_style(o.color);
        g.set_line_width(7.0);
        g.stroke_text(text, w / 2.0, y);
        g.set_shadow_blur(0.0);
        g.set_line_width(2.5);
        g.set_stroke_style("#fff4fb");
        g.stroke_text(text, w / 2.0, y);
        if let Some(sub) = o.sub {
            g.set_font(o.sub_font);
            fit_text(g, sub, w * 0.86);
            g.set_shadow_color(o.sub_color);
            g.set_shadow_blur(14.0);
            g.set_fill_style(o.sub_color);
            g.fill_text(sub, w / 2.0, h * 0.78);
            g.set_shadow_blur(0.0);
        }
    }
}

/// Where `/(\d+(\.\d+)?)px/` first matches in `s`: the byte range of the
/// number (without `px`).
fn px_match(s: &str) -> Option<(usize, usize)> {
    let b = s.as_bytes();
    let digit = |i: usize| i < b.len() && b[i].is_ascii_digit();
    for i in 0..b.len() {
        if !digit(i) {
            continue;
        }
        let mut j = i;
        while digit(j) {
            j += 1;
        }
        // The optional fraction (greedy, and dropped on backtracking).
        let mut ends = Vec::with_capacity(2);
        if j < b.len() && b[j] == b'.' && digit(j + 1) {
            let mut k = j + 1;
            while digit(k) {
                k += 1;
            }
            ends.push(k);
        }
        ends.push(j);
        for e in ends {
            if s[e..].starts_with("px") {
                return Some((i, e));
            }
        }
    }
    None
}

/// `fitText(g, text, maxW)`: shrink the font's pixel size so the text fits.
fn fit_text(g: &mut Canvas, text: &str, max_w: f64) {
    let m = g.measure_text(text);
    if m.width > max_w {
        let font = g.font().to_string();
        let (a, e) = px_match(&font).expect("a font with a pixel size");
        let px: f64 = font[a..e].parse().expect("a number");
        let size = (px * max_w / m.width).floor();
        let mut out = String::with_capacity(font.len());
        out.push_str(&font[..a]);
        out.push_str(&format!("{size}px"));
        out.push_str(&font[e + 2..]);
        g.set_font(&out);
    }
}

/// `roundRect(g, x, y, w, h, r)`: the path, with quadratic corners.
pub fn round_rect(g: &mut Canvas, x: f64, y: f64, w: f64, h: f64, r: f64) {
    g.begin_path();
    g.move_to(x + r, y);
    g.line_to(x + w - r, y);
    g.quadratic_curve_to(x + w, y, x + w, y + r);
    g.line_to(x + w, y + h - r);
    g.quadratic_curve_to(x + w, y + h, x + w - r, y + h);
    g.line_to(x + r, y + h);
    g.quadratic_curve_to(x, y + h, x, y + h - r);
    g.line_to(x, y + r);
    g.quadratic_curve_to(x, y, x + r, y);
    g.close_path();
}

#[cfg(test)]
mod tests {
    use super::px_match;

    #[test]
    fn the_pixel_size_is_found_as_the_regex_finds_it() {
        let f = "italic 900 84px \"Arial Narrow\"";
        assert_eq!(px_match(f).map(|(a, e)| &f[a..e]), Some("84"));
        let f = "bold 80.5px Arial";
        assert_eq!(px_match(f).map(|(a, e)| &f[a..e]), Some("80.5"));
        assert_eq!(px_match("bold Arial"), None);
    }
}
