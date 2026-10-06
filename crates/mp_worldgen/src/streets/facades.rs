//! Port of `src/world/streets/facades.js`'s atlas (roadmap WP 7.2): the
//! upper-floor façades of Downtown Streets.
//!
//! A 4×4 atlas of 256 px cells, each one a few bays by a few floors drawn
//! at close-up detail (sills, mullions, spandrels, AC units, curtains). The
//! emissive map has EVERY window lit; the shader then decides per window,
//! from its bay/floor index across the whole building and a per-building
//! seed, whether it is on and what colour it is. So a tile can repeat up a
//! 200 m tower without the pattern of lights repeating, offices light whole
//! floors while flats light odd windows, and street light bounces warm up
//! the lowest floors so no wall is a dead black slab.
//!
//! Geometry feeds three things per vertex:
//!   uv     in tile units (unbounded, like the city atlas)
//!   cell   atlas cell + 16 × building seed
//!   fdata  (street level y, bounce strength, bounce hue; 0 = sodium warm)
//!
//! The JS module keeps the atlas in a module variable, made once per page;
//! here it lives in the world build's [`TextureCache`] under
//! `streets:facadeAtlas`, as a façade pair (`Layer::Main` the map,
//! `Layer::Emissive` the emissive map; DECISIONS D352). The material and its
//! shader patch (`facadeMaterial`) are the scenery's.

// Index loops stay index loops (DECISIONS D52).
#![allow(clippy::needless_range_loop)]

use std::sync::Arc;

use mp_canvas::Canvas;
use mp_math::Mulberry32;

use crate::textures::{Cached, Texture, TextureCache};

/// The atlas cells (`F`).
pub mod f {
    pub const NAPT: usize = 0;
    pub const NTILE: usize = 1;
    pub const BRICK: usize = 2;
    pub const RIBBON: usize = 3;
    pub const CURTAIN: usize = 4;
    pub const STONE: usize = 5;
    pub const BANDS: usize = 6;
    pub const FINS: usize = 7;
    pub const DARK: usize = 8;
    pub const ROOF: usize = 9;
    pub const CROWN: usize = 10;
    pub const PANEL: usize = 11;
    pub const FAR_RES: usize = 12;
    pub const FAR_OFF: usize = 13;
    pub const HOTEL: usize = 14;
    pub const MECH: usize = 15;
}

/// Per cell: metres one repeat covers [w, h].
pub const F_TILE: [[f64; 2]; 16] = [
    [12.0, 12.0],
    [12.0, 12.0],
    [12.0, 12.8],
    [12.0, 12.0],
    [12.0, 16.0],
    [12.0, 16.0],
    [12.0, 16.0],
    [12.0, 16.0],
    [12.0, 16.0],
    [16.0, 16.0],
    [12.0, 6.0],
    [4.0, 4.0],
    [24.0, 24.0],
    [24.0, 32.0],
    [12.0, 12.0],
    [8.0, 4.0],
];

/// Per cell: the window grid [cols, rows] and the lighting kind: 0 as
/// drawn, 1 flats, 2 offices, 3 mostly dark offices.
pub const GRID: [[f64; 3]; 16] = [
    [4.0, 4.0, 1.0],
    [4.0, 4.0, 1.0],
    [4.0, 4.0, 1.0],
    [4.0, 4.0, 2.0],
    [8.0, 4.0, 2.0],
    [4.0, 4.0, 2.0],
    [6.0, 4.0, 2.0],
    [6.0, 4.0, 2.0],
    [8.0, 4.0, 3.0],
    [1.0, 1.0, 0.0],
    [1.0, 1.0, 0.0],
    [1.0, 1.0, 0.0],
    [8.0, 8.0, 1.0],
    [8.0, 8.0, 2.0],
    [4.0, 4.0, 1.0],
    [1.0, 1.0, 0.0],
];

/// Floor height of each façade cell, for stacking floors exactly.
pub fn floor_h(cell: usize) -> f64 {
    F_TILE[cell][1] / GRID[cell][1]
}

const S: f64 = 256.0;

/// A JS number in a template string (`${v}`).
fn js_num(v: f64) -> String {
    format!("{v}")
}

/// `tex(c, srgb = true)`: a CanvasTexture, anisotropy 8, no wrapping set.
fn tex(c: &Canvas, srgb: bool) -> Texture {
    Texture::from_canvas(c, false, srgb, 8.0)
}

/// Speckle + streaks so plain wall reads as a material, not a fill.
#[allow(clippy::too_many_arguments)]
fn grime(g: &mut Canvas, x: f64, y: f64, w: f64, h: f64, rng: &mut Mulberry32, n: u32, dark: f64) {
    for _ in 0..n {
        let style = if rng.next_f64() < 0.5 {
            format!("rgba(0,0,0,{})", js_num(dark * rng.next_f64()))
        } else {
            format!("rgba(255,255,255,{})", js_num(dark * 0.6 * rng.next_f64()))
        };
        g.set_fill_style(style);
        let rx = x + rng.next_f64() * w;
        let ry = y + rng.next_f64() * h;
        let rw = 1.0 + rng.next_f64() * 3.0;
        let rh = 1.0 + rng.next_f64() * 3.0;
        g.fill_rect(rx, ry, rw, rh);
    }
    for _ in 0..6 {
        let gx = x + rng.next_f64() * w;
        let gy = y + rng.next_f64() * h * 0.6;
        let mut grd = g.create_linear_gradient(0.0, gy, 0.0, gy + h * 0.4);
        grd.add_color_stop(
            0.0,
            &format!("rgba(20,18,16,{})", js_num(0.12 * rng.next_f64())),
        );
        grd.add_color_stop(1.0, "rgba(20,18,16,0)");
        g.set_fill_style(&grd);
        let gw = 2.0 + rng.next_f64() * 5.0;
        g.fill_rect(gx, gy, gw, h * 0.4);
    }
}

/// A lit room seen through a window: warm or cool base, a ceiling glow, a
/// curtain or blind, and furniture silhouettes. The shader tints and dims it.
#[allow(clippy::too_many_arguments)]
fn room(ge: &mut Canvas, x: f64, y: f64, w: f64, h: f64, rng: &mut Mulberry32, office: bool) {
    let mut grd = ge.create_linear_gradient(0.0, y, 0.0, y + h);
    if office {
        grd.add_color_stop(0.0, "#f4f8ff");
        grd.add_color_stop(0.25, "#b8c4d0");
        grd.add_color_stop(1.0, "#58606a");
    } else {
        grd.add_color_stop(0.0, "#ffe2b0");
        grd.add_color_stop(0.6, "#d89a58");
        grd.add_color_stop(1.0, "#8a5a30");
    }
    ge.set_fill_style(&grd);
    ge.fill_rect(x, y, w, h);
    if office {
        // Ceiling light strips and desk partitions.
        ge.set_fill_style("#ffffff");
        let mut q = x + 3.0;
        while q < x + w - 4.0 {
            ge.fill_rect(q, y + 2.0, 7.0, 2.0);
            q += 11.0;
        }
        ge.set_fill_style("rgba(20,24,30,0.55)");
        ge.fill_rect(x, y + h * 0.62, w, h * 0.38);
        for _ in 0..3 {
            let rx = x + rng.next_f64() * w;
            let rw = 3.0 + rng.next_f64() * 5.0;
            ge.fill_rect(rx, y + h * 0.45, rw, h * 0.2);
        }
    } else {
        // Curtains drawn part way from either side, or a blind from the top.
        let c = if rng.next_f64() < 0.5 {
            "120,40,30"
        } else {
            "40,50,70"
        };
        ge.set_fill_style(format!("rgba({c},0.55)"));
        if rng.next_f64() < 0.5 {
            let a = w * (0.15 + rng.next_f64() * 0.25);
            ge.fill_rect(x, y, a, h);
            let b = x + w * (0.7 + rng.next_f64() * 0.2);
            ge.fill_rect(b, y, w, h);
        } else {
            let a = h * (0.2 + rng.next_f64() * 0.5);
            ge.fill_rect(x, y, w, a);
        }
        ge.set_fill_style("rgba(30,18,10,0.6)");
        if rng.next_f64() < 0.6 {
            // sofa, shelf
            let rx = x + w * rng.next_f64() * 0.7;
            ge.fill_rect(rx, y + h * 0.65, w * 0.3, h * 0.35);
        }
        if rng.next_f64() < 0.4 {
            ge.begin_path();
            let cx = x + w * (0.2 + rng.next_f64() * 0.6);
            ge.arc(cx, y + h * 0.3, 3.0, 0.0, 7.0, false);
            ge.set_fill_style("#fff4d8");
            ge.fill();
        }
    }
}

fn at(k: usize) -> (f64, f64) {
    ((k % 4) as f64 * S, (k / 4) as f64 * S)
}

/// The window grid of cell k: (x, y, w, h, col, row) for each bay; row 0
/// is the lowest floor (canvas bottom), matching the shader's floor index.
fn bays(k: usize) -> Vec<(f64, f64, f64, f64, usize, usize)> {
    let (x0, y0) = at(k);
    let cols = GRID[k][0] as usize;
    let rows = GRID[k][1] as usize;
    let cw = S / cols as f64;
    let rh = S / rows as f64;
    let mut out = Vec::new();
    for r in 0..rows {
        for q in 0..cols {
            out.push((
                x0 + q as f64 * cw,
                y0 + S - (r + 1) as f64 * rh,
                cw,
                rh,
                q,
                r,
            ));
        }
    }
    out
}

/// `clip(k, fn)`: both canvases clipped to cell k while `fn` draws.
fn clip(g: &mut Canvas, ge: &mut Canvas, k: usize, draw: impl FnOnce(&mut Canvas, &mut Canvas)) {
    let (x0, y0) = at(k);
    g.save();
    ge.save();
    g.begin_path();
    g.rect(x0, y0, S, S);
    g.clip();
    ge.begin_path();
    ge.rect(x0, y0, S, S);
    ge.clip();
    draw(g, ge);
    g.restore();
    ge.restore();
}

/// `facadeAtlas()`: `{ map, emissive }`, cached as a façade pair.
pub fn facade_atlas(cache: &mut TextureCache) -> Arc<Cached> {
    cache.cached_with("streets:facadeAtlas", || {
        let (map, emissive) = draw_atlas();
        Cached::Facade {
            map,
            emissive,
            cols: 0,
            rows: 0,
        }
    })
}

fn draw_atlas() -> (Texture, Texture) {
    use f::*;
    let n = (S * 4.0) as u32;
    let mut c = Canvas::new(n, n);
    let mut ce = Canvas::new(n, n);
    ce.set_fill_style("#000");
    ce.fill_rect(0.0, 0.0, S * 4.0, S * 4.0);
    let mut rng = Mulberry32::new(4242);
    let rng = &mut rng;

    // 0 · Neon District flats: stained concrete, small framed windows, AC
    // units and a drainpipe.
    clip(&mut c, &mut ce, NAPT, |g, ge| {
        let (x0, y0) = at(NAPT);
        g.set_fill_style("#6f6a64");
        g.fill_rect(x0, y0, S, S);
        grime(g, x0, y0, S, S, rng, 900, 0.1);
        for (x, y, w, h, _, _) in bays(NAPT) {
            g.set_fill_style("rgba(0,0,0,0.18)");
            g.fill_rect(x, y + h - 5.0, w, 3.0); // floor slab line
            let (wx, wy, ww, wh) = (x + w * 0.2, y + h * 0.2, w * 0.6, h * 0.52);
            g.set_fill_style("#8a8680");
            g.fill_rect(wx - 3.0, wy - 3.0, ww + 6.0, wh + 8.0);
            g.set_fill_style("#1a1f26");
            g.fill_rect(wx, wy, ww, wh);
            g.set_fill_style("#9a968e");
            g.fill_rect(wx + ww / 2.0 - 1.0, wy, 2.0, wh);
            room(ge, wx, wy, ww, wh, rng, false);
            ge.set_fill_style("#000");
            ge.fill_rect(wx + ww / 2.0 - 1.0, wy, 2.0, wh);
            if rng.next_f64() < 0.55 {
                // AC unit under the window.
                g.set_fill_style("#c8c6c0");
                g.fill_rect(wx + ww * 0.55, wy + wh + 6.0, ww * 0.42, h * 0.14);
                g.set_fill_style("#5a5a58");
                for q in 0..5 {
                    g.fill_rect(
                        wx + ww * 0.58 + f64::from(q) * 4.0,
                        wy + wh + 9.0,
                        2.0,
                        h * 0.1,
                    );
                }
            }
        }
        g.set_fill_style("#4c4a46");
        g.fill_rect(x0 + S - 8.0, y0, 4.0, S);
    });

    // 1 · Tiled flats with balconies (railings, laundry, plants).
    clip(&mut c, &mut ce, NTILE, |g, ge| {
        let (x0, y0) = at(NTILE);
        g.set_fill_style("#b8b2a4");
        g.fill_rect(x0, y0, S, S);
        g.set_fill_style("rgba(0,0,0,0.08)");
        let mut y = 0.0;
        while y < S {
            g.fill_rect(x0, y0 + y, S, 1.0);
            y += 6.0;
        }
        let mut x = 0.0;
        while x < S {
            g.fill_rect(x0 + x, y0, 1.0, S);
            x += 12.0;
        }
        grime(g, x0, y0, S, S, rng, 500, 0.08);
        for (x, y, w, h, _, _) in bays(NTILE) {
            let (wx, wy, ww, wh) = (x + 6.0, y + 8.0, w - 12.0, h * 0.72);
            g.set_fill_style("#222830");
            g.fill_rect(wx, wy, ww, wh);
            room(ge, wx, wy, ww, wh, rng, false);
            // Sliding door frames.
            g.set_fill_style("#707478");
            g.fill_rect(wx + ww * 0.5 - 1.0, wy, 3.0, wh);
            g.fill_rect(wx, wy, ww, 2.0);
            ge.set_fill_style("#000");
            ge.fill_rect(wx + ww * 0.5 - 1.0, wy, 3.0, wh);
            // Balcony slab and railing in front.
            let by = y + h * 0.62;
            g.set_fill_style("#d6d0c4");
            g.fill_rect(x + 2.0, y + h - 7.0, w - 4.0, 7.0);
            g.set_fill_style("#3a3c40");
            g.fill_rect(x + 2.0, by, w - 4.0, 2.0);
            let mut q = x + 4.0;
            while q < x + w - 3.0 {
                g.fill_rect(q, by, 1.5, y + h - 7.0 - by);
                q += 4.0;
            }
            ge.set_fill_style("rgba(0,0,0,0.6)");
            ge.fill_rect(x + 2.0, by, w - 4.0, 2.0);
            let mut q = x + 4.0;
            while q < x + w - 3.0 {
                ge.fill_rect(q, by, 1.5, y + h - 7.0 - by);
                q += 4.0;
            }
            if rng.next_f64() < 0.35 {
                // laundry
                const CS: [&str; 4] = ["#d84a4a", "#f0f0e8", "#4a78c8", "#e8c040"];
                for q in 0..4 {
                    let qf = f64::from(q);
                    g.set_fill_style(CS[(rng.next_f64() * 4.0).floor() as usize]);
                    g.fill_rect(x + 8.0 + qf * 12.0, by - 14.0, 9.0, 12.0);
                    ge.set_fill_style("#000");
                    ge.fill_rect(x + 8.0 + qf * 12.0, by - 14.0, 9.0, 12.0);
                }
            }
            if rng.next_f64() < 0.4 {
                g.set_fill_style("#2e5a2a");
                g.begin_path();
                g.arc(x + w - 12.0, by - 4.0, 7.0, 0.0, 7.0, false);
                g.fill();
            }
        }
    });

    // 2 · Brick walk-ups: arched lintels, stone sills, a belt course.
    clip(&mut c, &mut ce, BRICK, |g, ge| {
        let (x0, y0) = at(BRICK);
        g.set_fill_style("#6e3a2c");
        g.fill_rect(x0, y0, S, S);
        let mut y = 0.0;
        while y < S {
            let mut x = if (y / 4.0) % 2.0 != 0.0 { 0.0 } else { 5.0 };
            while x < S {
                let v = rng.next_f64();
                g.set_fill_style(if v < 0.3 {
                    "rgba(40,16,10,0.35)"
                } else if v < 0.6 {
                    "rgba(170,100,76,0.25)"
                } else {
                    "rgba(120,60,40,0.15)"
                });
                g.fill_rect(x0 + x, y0 + y, 9.0, 3.0);
                x += 10.0;
            }
            y += 4.0;
        }
        for (x, y, w, h, _, r) in bays(BRICK) {
            if r == 0 {
                g.set_fill_style("#b8ab98");
                g.fill_rect(x, y + h - 6.0, w, 6.0);
            }
            let (wx, wy, ww, wh) = (x + w * 0.24, y + h * 0.22, w * 0.52, h * 0.56);
            g.set_fill_style("#5a2a1e");
            g.begin_path();
            g.ellipse(
                wx + ww / 2.0,
                wy,
                ww / 2.0 + 4.0,
                8.0,
                0.0,
                std::f64::consts::PI,
                0.0,
                false,
            );
            g.fill();
            g.set_fill_style("#1b1d22");
            g.fill_rect(wx, wy, ww, wh);
            g.set_fill_style("#d8d0c0");
            g.fill_rect(wx - 3.0, wy + wh, ww + 6.0, 4.0);
            g.set_fill_style("#e8e0d0");
            g.fill_rect(wx, wy + wh * 0.5 - 1.0, ww, 2.0);
            g.fill_rect(wx + ww / 2.0 - 1.0, wy, 2.0, wh);
            room(ge, wx, wy, ww, wh, rng, false);
            ge.set_fill_style("#000");
            ge.fill_rect(wx, wy + wh * 0.5 - 1.0, ww, 2.0);
            ge.fill_rect(wx + ww / 2.0 - 1.0, wy, 2.0, wh);
        }
    });

    // 3 · Mid-century offices: concrete frame with strip windows.
    clip(&mut c, &mut ce, RIBBON, |g, ge| {
        let (x0, y0) = at(RIBBON);
        g.set_fill_style("#8c8a84");
        g.fill_rect(x0, y0, S, S);
        grime(g, x0, y0, S, S, rng, 600, 0.09);
        for (x, y, w, h, _, _) in bays(RIBBON) {
            let (wy, wh) = (y + h * 0.3, h * 0.5);
            g.set_fill_style("#1c232c");
            g.fill_rect(x + 3.0, wy, w - 6.0, wh);
            g.set_fill_style("#a8a69e");
            g.fill_rect(x + w / 3.0, wy, 2.0, wh);
            g.fill_rect(x + (2.0 * w) / 3.0, wy, 2.0, wh);
            g.set_fill_style("#6c6a64");
            g.fill_rect(x, y + h - 4.0, w, 4.0);
            room(ge, x + 3.0, wy, w - 6.0, wh, rng, true);
            ge.set_fill_style("#000");
            ge.fill_rect(x + w / 3.0, wy, 2.0, wh);
            ge.fill_rect(x + (2.0 * w) / 3.0, wy, 2.0, wh);
        }
        g.set_fill_style("rgba(0,0,0,0.25)");
        for q in 0..4 {
            g.fill_rect(x0 + f64::from(q) * 64.0, y0, 4.0, S);
        }
    });

    // 4 · Curtain wall: blue-grey vision glass, mullions every 1.5 m, dark
    // spandrel panels at each slab.
    clip(&mut c, &mut ce, CURTAIN, |g, ge| {
        let (x0, y0) = at(CURTAIN);
        let mut grd = g.create_linear_gradient(0.0, y0, 0.0, y0 + S);
        grd.add_color_stop(0.0, "#3a4a60");
        grd.add_color_stop(1.0, "#1a2230");
        g.set_fill_style(&grd);
        g.fill_rect(x0, y0, S, S);
        for (x, y, w, h, _, _) in bays(CURTAIN) {
            let sp = h * 0.3; // spandrel at the bottom of each floor
            g.set_fill_style("#141a22");
            g.fill_rect(x, y + h - sp, w, sp);
            g.set_fill_style("rgba(140,170,210,0.12)");
            g.fill_rect(x, y + h - sp, w, 2.0);
            g.set_fill_style(format!(
                "rgba(150,180,220,{})",
                js_num(0.05 + rng.next_f64() * 0.1)
            ));
            g.fill_rect(x + 1.0, y, w - 2.0, h - sp);
            room(ge, x + 1.0, y, w - 2.0, h - sp, rng, true);
            g.set_fill_style("#8a96a4");
            g.fill_rect(x, y, 2.0, h);
            ge.set_fill_style("#000");
            ge.fill_rect(x, y, 2.0, h);
        }
    });

    // 5 · Limestone grid with deep punched windows.
    clip(&mut c, &mut ce, STONE, |g, ge| {
        let (x0, y0) = at(STONE);
        g.set_fill_style("#c4b8a2");
        g.fill_rect(x0, y0, S, S);
        grime(g, x0, y0, S, S, rng, 800, 0.08);
        g.set_fill_style("rgba(80,70,56,0.25)");
        let mut y = 0.0;
        while y < S {
            g.fill_rect(x0, y0 + y, S, 1.0);
            y += 16.0;
        }
        for (x, y, w, h, _, _) in bays(STONE) {
            let (wx, wy, ww, wh) = (x + w * 0.16, y + h * 0.14, w * 0.68, h * 0.64);
            g.set_fill_style("#8a7e6a");
            g.fill_rect(wx - 4.0, wy - 4.0, ww + 8.0, wh + 8.0); // reveal shadow
            g.set_fill_style("#20262e");
            g.fill_rect(wx, wy, ww, wh);
            g.set_fill_style("#5a5e62");
            g.fill_rect(wx + ww / 2.0 - 1.0, wy, 3.0, wh);
            room(ge, wx, wy, ww, wh, rng, true);
            ge.set_fill_style("#000");
            ge.fill_rect(wx + ww / 2.0 - 1.0, wy, 3.0, wh);
            g.set_fill_style("#d8ccb6");
            g.fill_rect(wx - 5.0, wy + wh + 4.0, ww + 10.0, 3.0);
        }
    });

    // 6 · Ribbon glass between dark aluminium bands.
    clip(&mut c, &mut ce, BANDS, |g, ge| {
        let (x0, y0) = at(BANDS);
        g.set_fill_style("#26292e");
        g.fill_rect(x0, y0, S, S);
        for (x, y, w, h, _, _) in bays(BANDS) {
            let (wy, wh) = (y + h * 0.08, h * 0.6);
            g.set_fill_style("#2c3a4a");
            g.fill_rect(x, wy, w, wh);
            g.set_fill_style("rgba(170,200,230,0.15)");
            g.fill_rect(x, wy, w, 3.0);
            room(ge, x, wy, w, wh, rng, true);
            g.set_fill_style("#50565e");
            g.fill_rect(x, wy, 1.5, wh);
            ge.set_fill_style("#000");
            ge.fill_rect(x, wy, 1.5, wh);
            g.set_fill_style("rgba(255,255,255,0.06)");
            g.fill_rect(x, y + h * 0.72, w, 2.0);
        }
    });

    // 7 · Bronze fins: deep vertical fins between tall glass.
    clip(&mut c, &mut ce, FINS, |g, ge| {
        let (x0, y0) = at(FINS);
        g.set_fill_style("#1e2630");
        g.fill_rect(x0, y0, S, S);
        for (x, y, w, h, _, _) in bays(FINS) {
            g.set_fill_style("#18202a");
            g.fill_rect(x, y + h - 10.0, w, 10.0);
            room(ge, x + 7.0, y + 2.0, w - 14.0, h - 14.0, rng, true);
        }
        for q in 0..=6 {
            let fx = x0 + (f64::from(q) * S) / 6.0 - 5.0;
            let mut grd = g.create_linear_gradient(fx, 0.0, fx + 10.0, 0.0);
            grd.add_color_stop(0.0, "#5a4028");
            grd.add_color_stop(0.5, "#a8804e");
            grd.add_color_stop(1.0, "#4a3420");
            g.set_fill_style(&grd);
            g.fill_rect(fx, y0, 10.0, S);
            ge.set_fill_style("#000");
            ge.fill_rect(fx, y0, 10.0, S);
        }
    });

    // 8 · Dark reflective glass for crowns and sleek shafts.
    clip(&mut c, &mut ce, DARK, |g, ge| {
        let (x0, y0) = at(DARK);
        let mut grd = g.create_linear_gradient(x0, y0, x0 + S, y0 + S);
        grd.add_color_stop(0.0, "#2a3446");
        grd.add_color_stop(0.5, "#141a24");
        grd.add_color_stop(1.0, "#222a38");
        g.set_fill_style(&grd);
        g.fill_rect(x0, y0, S, S);
        for (x, y, w, h, _, _) in bays(DARK) {
            g.set_fill_style("rgba(160,190,230,0.2)");
            g.fill_rect(x, y, 1.0, h);
            g.fill_rect(x, y + h - 1.0, w, 1.0);
            room(ge, x + 1.0, y + 1.0, w - 2.0, h - 8.0, rng, true);
        }
    });

    // 9 · Roof: tar and gravel with patches and walkway pads.
    clip(&mut c, &mut ce, ROOF, |g, _ge| {
        let (x0, y0) = at(ROOF);
        g.set_fill_style("#48484a");
        g.fill_rect(x0, y0, S, S);
        for _ in 0..2400 {
            let v = 50.0 + rng.next_f64() * 60.0;
            g.set_fill_style(format!(
                "rgb({},{},{})",
                js_num(v),
                js_num(v),
                js_num(v + 3.0)
            ));
            let rx = x0 + rng.next_f64() * S;
            let ry = y0 + rng.next_f64() * S;
            g.fill_rect(rx, ry, 2.0, 2.0);
        }
        for _ in 0..10 {
            let c = if rng.next_f64() < 0.5 {
                "30,30,32"
            } else {
                "110,110,108"
            };
            g.set_fill_style(format!("rgba({c},0.25)"));
            let rx = x0 + rng.next_f64() * S;
            let ry = y0 + rng.next_f64() * S;
            let rw = 20.0 + rng.next_f64() * 60.0;
            let rh = 20.0 + rng.next_f64() * 60.0;
            g.fill_rect(rx, ry, rw, rh);
        }
        g.set_fill_style("rgba(160,160,150,0.35)");
        for q in 0..8 {
            g.fill_rect(x0 + 20.0 + f64::from(q) * 28.0, y0 + 120.0, 24.0, 16.0);
        }
    });

    // 10 · Crown: lit louvres and a glowing top band (always on).
    clip(&mut c, &mut ce, CROWN, |g, ge| {
        let (x0, y0) = at(CROWN);
        g.set_fill_style("#1a1c20");
        g.fill_rect(x0, y0, S, S);
        let mut grd = ge.create_linear_gradient(0.0, y0, 0.0, y0 + S);
        grd.add_color_stop(0.0, "#fff4e0");
        grd.add_color_stop(0.35, "#e0b878");
        grd.add_color_stop(1.0, "#402a10");
        ge.set_fill_style(&grd);
        ge.fill_rect(x0, y0 + 20.0, S, S - 40.0);
        ge.set_fill_style("rgba(0,0,0,0.55)");
        let mut y = 24.0;
        while y < S - 20.0 {
            ge.fill_rect(x0, y0 + y, S, 4.0);
            y += 10.0;
        }
        ge.set_fill_style("#000");
        let mut x = 0.0;
        while x < S {
            ge.fill_rect(x0 + x, y0, 4.0, S);
            x += 21.0;
        }
        g.set_fill_style("#3a3a3e");
        let mut x = 0.0;
        while x < S {
            g.fill_rect(x0 + x, y0, 4.0, S);
            x += 21.0;
        }
        g.fill_rect(x0, y0, S, 20.0);
        g.fill_rect(x0, y0 + S - 20.0, S, 20.0);
    });

    // 11 · Plain panel: metal/stone cladding for piers, penthouses, parapets.
    clip(&mut c, &mut ce, PANEL, |g, _ge| {
        let (x0, y0) = at(PANEL);
        g.set_fill_style("#9a968e");
        g.fill_rect(x0, y0, S, S);
        grime(g, x0, y0, S, S, rng, 700, 0.1);
        g.set_fill_style("rgba(0,0,0,0.2)");
        let mut q = 0.0;
        while q < S {
            g.fill_rect(x0 + q, y0, 2.0, S);
            g.fill_rect(x0, y0 + q, S, 2.0);
            q += 64.0;
        }
    });

    // 12 · Far flats: 8 × 8 small windows (seen from a distance).
    clip(&mut c, &mut ce, FAR_RES, |g, ge| {
        let (x0, y0) = at(FAR_RES);
        g.set_fill_style("#5a5450");
        g.fill_rect(x0, y0, S, S);
        grime(g, x0, y0, S, S, rng, 400, 0.12);
        for (x, y, w, h, _, _) in bays(FAR_RES) {
            g.set_fill_style("#16191e");
            g.fill_rect(x + w * 0.22, y + h * 0.2, w * 0.56, h * 0.55);
            ge.set_fill_style(if rng.next_f64() < 0.7 {
                "#ffc880"
            } else {
                "#fff0d0"
            });
            ge.fill_rect(x + w * 0.22, y + h * 0.2, w * 0.56, h * 0.55);
            ge.set_fill_style("rgba(0,0,0,0.5)");
            let a = h * 0.55 * rng.next_f64();
            ge.fill_rect(x + w * 0.22, y + h * 0.2, w * 0.56, a);
        }
    });

    // 13 · Far offices: 8 × 8 bays of glass.
    clip(&mut c, &mut ce, FAR_OFF, |g, ge| {
        let (x0, y0) = at(FAR_OFF);
        g.set_fill_style("#1e242c");
        g.fill_rect(x0, y0, S, S);
        for (x, y, w, h, _, _) in bays(FAR_OFF) {
            g.set_fill_style("#2a3442");
            g.fill_rect(x + 2.0, y + 3.0, w - 4.0, h - 10.0);
            ge.set_fill_style("#dfe8ff");
            ge.fill_rect(x + 2.0, y + 3.0, w - 4.0, h - 10.0);
            ge.set_fill_style("rgba(0,0,0,0.4)");
            ge.fill_rect(x + 2.0, y + h * 0.5, w - 4.0, h * 0.3);
        }
    });

    // 14 · Painted hotel: pastel stucco, shuttered windows, pilasters.
    clip(&mut c, &mut ce, HOTEL, |g, ge| {
        let (x0, y0) = at(HOTEL);
        g.set_fill_style("#d8d0c4");
        g.fill_rect(x0, y0, S, S);
        grime(g, x0, y0, S, S, rng, 600, 0.08);
        for (x, y, w, h, _, _) in bays(HOTEL) {
            g.set_fill_style("rgba(255,255,255,0.35)");
            g.fill_rect(x, y + h - 6.0, w, 4.0);
            let (wx, wy, ww, wh) = (x + w * 0.3, y + h * 0.2, w * 0.4, h * 0.58);
            g.set_fill_style("#1c2026");
            g.fill_rect(wx, wy, ww, wh);
            g.set_fill_style("#3a6a5a");
            g.fill_rect(wx - ww * 0.42, wy, ww * 0.38, wh);
            g.fill_rect(wx + ww * 1.04, wy, ww * 0.38, wh);
            g.set_fill_style("rgba(0,0,0,0.3)");
            let mut q = wy;
            while q < wy + wh {
                g.fill_rect(wx - ww * 0.42, q, ww * 0.38, 1.5);
                g.fill_rect(wx + ww * 1.04, q, ww * 0.38, 1.5);
                q += 5.0;
            }
            room(ge, wx, wy, ww, wh, rng, false);
        }
        g.set_fill_style("rgba(0,0,0,0.12)");
        for q in 0..4 {
            g.fill_rect(x0 + f64::from(q) * 64.0, y0, 5.0, S);
        }
    });

    // 15 · Mechanical penthouse: louvred screen.
    clip(&mut c, &mut ce, MECH, |g, _ge| {
        let (x0, y0) = at(MECH);
        g.set_fill_style("#4a4c50");
        g.fill_rect(x0, y0, S, S);
        g.set_fill_style("#2a2c30");
        let mut y = 8.0;
        while y < S {
            g.fill_rect(x0, y0 + y, S, 4.0);
            y += 9.0;
        }
        g.set_fill_style("#6a6c70");
        let mut x = 0.0;
        while x < S {
            g.fill_rect(x0 + x, y0, 4.0, S);
            x += 64.0;
        }
    });

    (tex(&c, true), tex(&ce, true))
}
