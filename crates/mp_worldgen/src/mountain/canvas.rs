//! `Mountain.js`'s canvas helpers: `makeCanvas`, `canvasTex`, the sign
//! atlas, `roundRect`, `diamond`, `panel`, and the pictures the scenery
//! draws for itself (the snow pole stripes, the start banner, the diner's
//! neon and pole sign, the waterfall's streaks and the plunge pool's foam).

use std::sync::Arc;

use mp_canvas::Canvas;
use mp_math::{Mulberry32, kernel};
use mp_scene::{TextureSource, three};

use crate::object::{Image, SceneGraph, TextureId};
use crate::textures::Texture;

const PI: f64 = core::f64::consts::PI;

pub const FONT: &str = "\"Arial Narrow\", \"Helvetica Neue\", Arial, sans-serif";

/// A canvas as a texture: what `new THREE.CanvasTexture(c)` uploads, with
/// three's defaults (clamped, no colour space, anisotropy 1) unless set.
pub fn canvas_texture(c: &Canvas, srgb: bool, anisotropy: f64) -> Texture {
    Texture {
        width: c.width,
        height: c.height,
        rgba: c.to_rgba(),
        source: TextureSource::Canvas,
        repeat: false,
        srgb,
        anisotropy,
    }
}

/// `canvasTex(c)`: sRGB, anisotropy 8, clamped.
pub fn canvas_tex(graph: &mut SceneGraph, c: &Canvas) -> TextureId {
    let t = Arc::new(canvas_texture(c, true, 8.0));
    let desc = t.desc("", 0);
    graph.add_texture(Image::Own(t), desc)
}

/// `new THREE.CanvasTexture(c)` with three's defaults.
pub fn plain_canvas_tex(graph: &mut SceneGraph, c: &Canvas) -> TextureId {
    let t = Arc::new(canvas_texture(c, false, 1.0));
    let desc = t.desc("", 0);
    graph.add_texture(Image::Own(t), desc)
}

/// Where a face sits in the atlas (`{ u0, u1, v0, v1 }`), and whether it
/// is a diamond (`r.diamond = true`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub u0: f64,
    pub u1: f64,
    pub v0: f64,
    pub v1: f64,
    pub diamond: bool,
}

/// `SignAtlas`: sign faces drawn into one atlas so every sign is a single
/// draw call.
pub struct SignAtlas {
    pub w: f64,
    pub h: f64,
    pub canvas: Canvas,
    x: f64,
    y: f64,
    row_h: f64,
}

impl SignAtlas {
    pub fn new() -> SignAtlas {
        SignAtlas {
            w: 2048.0,
            h: 1024.0,
            canvas: Canvas::new(2048, 1024),
            x: 0.0,
            y: 0.0,
            row_h: 0.0,
        }
    }

    /// `add(w, h, draw)`: w, h in pixels; `draw(g, w, h)` paints in local
    /// coordinates.
    pub fn add(&mut self, w: f64, h: f64, draw: impl FnOnce(&mut Canvas, f64, f64)) -> Rect {
        if self.x + w > self.w {
            self.x = 0.0;
            self.y += self.row_h + 4.0;
            self.row_h = 0.0;
        }
        let (x, y) = (self.x, self.y);
        let g = &mut self.canvas;
        g.save();
        g.translate(x, y);
        g.begin_path();
        g.rect(0.0, 0.0, w, h);
        g.clip();
        draw(g, w, h);
        g.restore();
        self.x += w + 4.0;
        self.row_h = mp_math::js::max(self.row_h, h);
        Rect {
            u0: x / self.w,
            u1: (x + w) / self.w,
            v0: 1.0 - (y + h) / self.h,
            v1: 1.0 - y / self.h,
            diamond: false,
        }
    }
}

impl Default for SignAtlas {
    fn default() -> Self {
        SignAtlas::new()
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

/// `diamond(g, w, h, inner)`: a yellow warning diamond, then `inner(g, cx,
/// cy)` in black.
pub fn diamond(g: &mut Canvas, w: f64, h: f64, inner: impl FnOnce(&mut Canvas, f64, f64)) {
    g.clear_rect(0.0, 0.0, w, h);
    g.save();
    g.translate(w / 2.0, h / 2.0);
    g.rotate(PI / 4.0);
    let s = w * 0.69;
    round_rect(g, -s / 2.0, -s / 2.0, s, s, 12.0);
    g.set_fill_style("#111");
    g.fill();
    round_rect(g, -s / 2.0 + 7.0, -s / 2.0 + 7.0, s - 14.0, s - 14.0, 9.0);
    g.set_fill_style("#f5c518");
    g.fill();
    g.restore();
    g.set_fill_style("#111");
    g.set_stroke_style("#111");
    inner(g, w / 2.0, h / 2.0);
}

/// `panel(g, w, h, bg, fg, border, lines, sizes)`: a bordered board with
/// centred lines of bold text.
#[allow(clippy::too_many_arguments)]
pub fn panel(
    g: &mut Canvas,
    w: f64,
    h: f64,
    bg: &str,
    fg: &str,
    border: &str,
    lines: &[&str],
    sizes: &[f64],
) {
    round_rect(g, 2.0, 2.0, w - 4.0, h - 4.0, 16.0);
    g.set_fill_style(border);
    g.fill();
    round_rect(g, 9.0, 9.0, w - 18.0, h - 18.0, 11.0);
    g.set_fill_style(bg);
    g.fill();
    g.set_fill_style(fg);
    g.set_text_align("center");
    g.set_text_baseline("middle");
    let total = sizes.iter().fold(0.0, |a, b| a + b * 1.12);
    let mut y = h / 2.0 - total / 2.0;
    for (i, l) in lines.iter().enumerate() {
        g.set_font(&format!("bold {}px {FONT}", sizes[i]));
        y += sizes[i] * 0.56;
        g.fill_text(l, w / 2.0, y + 2.0);
        y += sizes[i] * 0.56;
    }
}

/// The snow poles' orange and black bands (8 × 64).
pub fn snow_pole_canvas() -> Canvas {
    let mut g = Canvas::new(8, 64);
    for k in 0..8 {
        g.set_fill_style(if k % 2 == 1 { "#111" } else { "#ff6a13" });
        g.fill_rect(0.0, k as f64 * 8.0, 8.0, 8.0);
    }
    g
}

/// `bannerTex(front)`'s canvas: front says SIERRA PASS, back (seen when
/// looking back) START.
pub fn banner_canvas(front: bool) -> Canvas {
    let mut g = Canvas::new(1024, 160);
    g.set_fill_style("#0d0f14");
    g.fill_rect(0.0, 0.0, 1024.0, 160.0);
    let sq = 20.0;
    let mut y = 0.0;
    while y < 160.0 {
        let mut x = 0.0;
        while x < 120.0 {
            g.set_fill_style(if ((x + y) / sq) % 2.0 != 0.0 {
                "#f2f2f2"
            } else {
                "#111"
            });
            g.fill_rect(x, y, sq, sq);
            g.fill_rect(1024.0 - 120.0 + x, y, sq, sq);
            x += sq;
        }
        y += sq;
    }
    g.set_fill_style("#ff3860");
    g.fill_rect(120.0, 0.0, 784.0, 8.0);
    g.fill_rect(120.0, 152.0, 784.0, 8.0);
    g.set_fill_style("#fff");
    g.set_text_align("center");
    g.set_text_baseline("middle");
    g.set_font(&format!("italic 900 84px {FONT}"));
    g.fill_text(if front { "SIERRA PASS" } else { "START" }, 512.0, 70.0);
    g.set_font(&format!("bold 26px {FONT}"));
    g.set_fill_style("#ffb347");
    g.fill_text(
        if front {
            "STAGE 1 · MIDNIGHT RACER"
        } else {
            "SIERRA PASS"
        },
        512.0,
        132.0,
    );
    g
}

/// The diner's neon roof sign.
pub fn neon_canvas() -> Canvas {
    let mut g = Canvas::new(512, 128);
    g.set_fill_style("#1a0f10");
    round_rect(&mut g, 4.0, 4.0, 504.0, 120.0, 18.0);
    g.fill();
    g.set_stroke_style("#ff4a6a");
    g.set_line_width(6.0);
    round_rect(&mut g, 12.0, 12.0, 488.0, 104.0, 14.0);
    g.stroke();
    g.set_font(&format!("italic 900 78px {FONT}"));
    g.set_text_align("center");
    g.set_text_baseline("middle");
    g.set_shadow_color("#ff3860");
    g.set_shadow_blur(18.0);
    g.set_fill_style("#ffd1dc");
    g.fill_text("DINER", 256.0, 68.0);
    g
}

/// The tall pole sign by the road.
pub fn pole_sign_canvas() -> Canvas {
    let mut g = Canvas::new(256, 320);
    g.set_fill_style("#2b1d14");
    round_rect(&mut g, 4.0, 4.0, 248.0, 312.0, 20.0);
    g.fill();
    g.set_fill_style("#f1e4c8");
    round_rect(&mut g, 14.0, 14.0, 228.0, 292.0, 14.0);
    g.fill();
    g.set_fill_style("#7a2a1c");
    g.set_text_align("center");
    g.set_text_baseline("middle");
    g.set_font(&format!("900 46px {FONT}"));
    g.fill_text("PINE", 128.0, 62.0);
    g.fill_text("RIDGE", 128.0, 110.0);
    g.set_fill_style("#2b1d14");
    g.fill_rect(34.0, 142.0, 188.0, 4.0);
    g.set_font(&format!("bold 40px {FONT}"));
    g.fill_text("GAS · EATS", 128.0, 186.0);
    g.set_font(&format!("bold 30px {FONT}"));
    g.set_fill_style("#a3272c");
    g.fill_text("OPEN 24 HRS", 128.0, 240.0);
    g.set_font(&format!("bold 24px {FONT}"));
    g.set_fill_style("#2b1d14");
    g.fill_text("LAST STOP BEFORE PASS", 128.0, 280.0);
    g
}

/// The waterfall's streaks (128 × 256), seeded 3.
pub fn waterfall_canvas() -> Canvas {
    let mut g = Canvas::new(128, 256);
    let mut rng = Mulberry32::new(3);
    g.set_fill_style("rgba(255,255,255,0.4)");
    g.fill_rect(0.0, 0.0, 128.0, 256.0);
    for _ in 0..320 {
        let x = rng.next_f64() * 128.0;
        let w = 1.0 + rng.next_f64() * 4.0;
        let y = rng.next_f64() * 256.0;
        let h = 60.0 + rng.next_f64() * 190.0;
        let mut grd = g.create_linear_gradient(0.0, y, 0.0, y + h);
        let a = 0.3 + rng.next_f64() * 0.65;
        grd.add_color_stop(0.0, "rgba(255,255,255,0)");
        grd.add_color_stop(0.3, &format!("rgba(255,255,255,{a})"));
        grd.add_color_stop(1.0, "rgba(255,255,255,0)");
        g.set_fill_style(&grd);
        g.fill_rect(x, y, w, h);
        g.fill_rect(x, y - 256.0, w, h);
    }
    let mut edge = g.create_linear_gradient(0.0, 0.0, 128.0, 0.0);
    edge.add_color_stop(0.0, "rgba(0,0,0,1)");
    edge.add_color_stop(0.18, "rgba(0,0,0,0.1)");
    edge.add_color_stop(0.82, "rgba(0,0,0,0.1)");
    edge.add_color_stop(1.0, "rgba(0,0,0,1)");
    g.set_global_composite_operation("destination-out");
    g.set_fill_style(&edge);
    g.fill_rect(0.0, 0.0, 128.0, 256.0);
    g
}

/// The plunge pool's churning foam (128 × 128), seeded 8.
pub fn foam_canvas() -> Canvas {
    let mut g = Canvas::new(128, 128);
    let mut fr = Mulberry32::new(8);
    for _ in 0..160 {
        let a = fr.next_f64() * PI * 2.0;
        let r = 20.0 + fr.next_f64() * 42.0;
        g.set_fill_style(format!("rgba(255,255,255,{})", 0.15 + fr.next_f64() * 0.5));
        g.begin_path();
        let cx = 64.0 + kernel::cos(a) * r * 0.9;
        let cy = 64.0 + kernel::sin(a) * r * 0.9;
        g.arc(cx, cy, 2.0 + fr.next_f64() * 7.0, 0.0, PI * 2.0, false);
        g.fill();
    }
    g
}

/// A clamped texture's wrap modes changed (`tex.wrapS`, `tex.wrapT`).
pub fn set_wrap(graph: &mut SceneGraph, t: TextureId, wrap_s: u32, wrap_t: u32) {
    let d = &mut graph.texture_mut(t).desc;
    d.wrap_s = wrap_s;
    d.wrap_t = wrap_t;
}

/// `texture.repeat.set(u, v)`.
pub fn set_repeat(graph: &mut SceneGraph, t: TextureId, u: f64, v: f64) {
    graph.texture_mut(t).desc.repeat = [u, v];
}

/// three's `RepeatWrapping`, for the waterfall's `wrapT`.
pub const REPEAT: u32 = three::REPEAT_WRAPPING;
/// three's `ClampToEdgeWrapping`.
pub const CLAMP: u32 = three::CLAMP_TO_EDGE_WRAPPING;
