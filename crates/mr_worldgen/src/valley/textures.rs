//! The canvas textures of `src/world/Valley.js` (roadmap WP 3.7): the wood
//! signs, the creek's ripple normals, the neon "OPEN" and the corn strip.
//! Each draws on an [`mr_canvas::Canvas`] as the JS draws on its 2D
//! context, from the same streams in the same order.

use std::f64::consts::PI;

use mr_canvas::Canvas;
use mr_math::{Mulberry32, js, kernel};
use mr_scene::TextureSource;

use crate::textures::Texture;

/// `canvasTexture(w, h, draw)`: a `CanvasTexture` in sRGB, anisotropy 8,
/// clamped (the callers change the wrap and colour space after).
fn canvas_texture(w: u32, h: u32, draw: impl FnOnce(&mut Canvas, f64, f64)) -> Texture {
    let mut c = Canvas::new(w, h);
    draw(&mut c, f64::from(w), f64::from(h));
    Texture {
        width: w,
        height: h,
        rgba: c.to_rgba(),
        source: TextureSource::Canvas,
        repeat: false,
        srgb: true,
        anisotropy: 8.0,
    }
}

/// `woodSign(lines, opts)`'s options, with the JS defaults in
/// [`WoodSign::DEFAULT`].
#[derive(Clone, Copy, Debug)]
pub struct WoodSign<'a> {
    pub w: u32,
    pub h: u32,
    pub bg: &'a str,
    pub fg: &'a str,
    pub font: &'a str,
    pub sub: &'a str,
}

impl WoodSign<'static> {
    pub const DEFAULT: WoodSign<'static> = WoodSign {
        w: 512,
        h: 192,
        bg: "#5a3f28",
        fg: "#f1e2c0",
        font: "bold 64px Georgia, serif",
        sub: "italic 30px Georgia, serif",
    };
}

/// `woodSign(lines, opts)`. The plank noise draws from the page's
/// `Math.random` (`random`; DECISIONS D20, D332).
pub fn wood_sign(lines: &[&str], o: WoodSign, random: &mut Mulberry32) -> Texture {
    canvas_texture(o.w, o.h, |g, w, h| {
        g.set_fill_style(o.bg);
        g.fill_rect(0.0, 0.0, w, h);
        // Planks
        let mut y = 0.0;
        while y < h {
            g.set_fill_style("rgba(0,0,0,0.18)");
            g.fill_rect(0.0, y, w, 3.0);
            y += h / 4.0;
        }
        for _ in 0..60 {
            let light = random.next_f64() < 0.5;
            g.set_fill_style(if light {
                "rgba(255,230,200,0.05)"
            } else {
                "rgba(0,0,0,0.05)"
            });
            let x = random.next_f64() * w;
            let yy = random.next_f64() * h;
            let ww = 40.0 + random.next_f64() * 120.0;
            g.fill_rect(x, yy, ww, 2.0);
        }
        g.set_stroke_style(o.fg);
        g.set_line_width(5.0);
        g.stroke_rect(12.0, 12.0, w - 24.0, h - 24.0);
        g.set_fill_style(o.fg);
        g.set_text_align("center");
        g.set_text_baseline("middle");
        g.set_font(o.font);
        let two = lines.len() > 1 && !lines[1].is_empty();
        g.fill_text(lines[0], w / 2.0, if two { h * 0.4 } else { h / 2.0 });
        if two {
            g.set_font(o.sub);
            g.fill_text(lines[1], w / 2.0, h * 0.72);
        }
    })
}

/// `waterNormalTexture()`: tileable ripple normals, a sum of
/// integer-frequency waves (which wrap cleanly), 128². Repeat-wrapped, no
/// colour space.
pub fn water_normal_texture() -> Texture {
    const S: usize = 128;
    let s = S as f64;
    let mut rng = Mulberry32::new(77);
    struct Wave {
        fx: f64,
        fy: f64,
        ph: f64,
        a: f64,
    }
    let mut waves = Vec::new();
    for k in 0..14 {
        let fx = (rng.next_f64() * 7.0).floor() - 3.0;
        let fy = js::or((rng.next_f64() * 7.0).floor() - 3.0, 1.0);
        let ph = rng.next_f64() * PI * 2.0;
        waves.push(Wave {
            fx,
            fy,
            ph,
            a: 0.6 / (1.0 + f64::from(k) * 0.25),
        });
    }
    let hgt = |x: f64, y: f64| {
        let mut h = 0.0;
        for w in &waves {
            h += w.a * kernel::sin(((w.fx * x + w.fy * y) / s) * PI * 2.0 + w.ph);
        }
        h
    };
    let mut t = canvas_texture(S as u32, S as u32, |g, _, _| {
        let mut img = g.create_image_data(S as u32, S as u32);
        for y in 0..S {
            for x in 0..S {
                let (xf, yf) = (x as f64, y as f64);
                let dx = hgt(xf + 1.0, yf) - hgt(xf - 1.0, yf);
                let dy = hgt(xf, yf + 1.0) - hgt(xf, yf - 1.0);
                let i = (y * S + x) * 4;
                img.set(i, 128.0 + dx * 60.0);
                img.set(i + 1, 128.0 + dy * 60.0);
                img.set(i + 2, 255.0);
                img.set(i + 3, 255.0);
            }
        }
        g.put_image_data(&img, 0, 0);
    });
    t.repeat = true;
    t.srgb = false;
    t
}

/// The neon "OPEN" sign: an emissive map that is also its alpha map.
pub fn neon_texture() -> Texture {
    canvas_texture(256, 96, |g, w, h| {
        g.set_fill_style("#000");
        g.fill_rect(0.0, 0.0, w, h);
        g.set_font("bold 70px \"Arial Black\", Arial, sans-serif");
        g.set_text_align("center");
        g.set_text_baseline("middle");
        g.set_shadow_color("#ff3040");
        g.set_shadow_blur(18.0);
        g.set_stroke_style("#ff5060");
        g.set_line_width(6.0);
        g.stroke_text("OPEN", w / 2.0, h / 2.0);
        g.set_shadow_blur(0.0);
        g.set_line_width(2.0);
        g.set_stroke_style("#ffd0d0");
        g.stroke_text("OPEN", w / 2.0, h / 2.0);
    })
}

/// The corn rows' strip: stalks, leaves and tassels on a transparent
/// canvas, 256×128 (the caller repeats it along U).
pub fn corn_texture() -> Texture {
    canvas_texture(256, 128, |g, w, h| {
        let mut rng = Mulberry32::new(12);
        g.clear_rect(0.0, 0.0, w, h);
        for k in 0..9 {
            let x = (f64::from(k) + 0.5) * (w / 9.0) + (rng.next_f64() - 0.5) * 8.0;
            let top = 6.0 + rng.next_f64() * 16.0;
            let lean = (rng.next_f64() - 0.5) * 8.0;
            let green = ["#6f8e30", "#7d9a36", "#8aa23c", "#9aa448"]
                [(rng.next_f64() * 4.0).floor() as usize];
            g.set_stroke_style(green);
            g.set_line_width(3.0);
            g.begin_path();
            g.move_to(x, h);
            g.quadratic_curve_to(x, h * 0.5, x + lean, top);
            g.stroke();
            // Arching leaves in pairs up the stalk.
            let mut y = h - 14.0;
            while y > top + 16.0 {
                for sd in [-1.0, 1.0] {
                    let len = 14.0 + rng.next_f64() * 16.0;
                    g.set_stroke_style(if rng.next_f64() < 0.2 {
                        "#b0a654"
                    } else {
                        green
                    });
                    g.set_line_width(3.5);
                    g.begin_path();
                    g.move_to(x, y);
                    let ey = y + 4.0 + rng.next_f64() * 6.0;
                    g.quadratic_curve_to(x + sd * len * 0.7, y - 12.0, x + sd * len, ey);
                    g.stroke();
                }
                y -= 13.0 + rng.next_f64() * 6.0;
            }
            // Tassel.
            g.set_stroke_style("#c8b46a");
            g.set_line_width(2.0);
            for j in -1..=1 {
                g.begin_path();
                g.move_to(x + lean, top + 4.0);
                g.line_to(x + lean + f64::from(j) * 5.0, top - 5.0);
                g.stroke();
            }
        }
    })
}
