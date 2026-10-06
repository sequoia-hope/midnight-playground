//! Port of `src/world/streets/textures.js`'s pictures (roadmap WP 7.2):
//! canvas textures for Downtown Streets: a street-level façade atlas (shop
//! fronts, rowhouses, lobbies), a neon sign atlas, pavement, the red/white
//! barrier stripe, the puddle blob and the elevated train. All names on
//! signs are invented.
//!
//! The JS module keeps each picture in a module variable, made once per
//! page; here they live in the world build's [`TextureCache`] under
//! `streets:` keys (DECISIONS D352). A JS pair (`{ map, emissive }`,
//! `{ h, v }`, `{ map, e }`) is cached as a façade pair: `Layer::Main` is
//! the first, `Layer::Emissive` the second. The material patch
//! (`patchStreetAtlas`) is the scenery's.

// Index loops stay index loops (DECISIONS D52).
#![allow(clippy::needless_range_loop)]
// The JS writes a random angle as `rng() * 6.28`, not 2π.
#![allow(clippy::approx_constant)]

use std::f64::consts::PI;
use std::sync::Arc;

use mp_canvas::Canvas;
use mp_math::{Mulberry32, kernel};

use crate::textures::{Cached, Texture, TextureCache};

// ── Street-level façade atlas ────────────────────────────────────
// A 4×3 grid of 384 px cells, with the metres one repeat covers [w, h].
// `cell` in the geometry is the cell index + 16 × a per-building seed, so the
// shader can vary the lights building by building (see patchStreetAtlas).
pub const S_CELLS: usize = 12;
pub const SHOP_A: usize = 0;
pub const SHOP_B: usize = 1;
pub const ROWHOUSE: usize = 2;
pub const STUCCO: usize = 3;
pub const SHUTTER: usize = 4;
pub const LOBBY: usize = 5;
pub const KONBINI: usize = 6;
pub const ROW_GROUND: usize = 7;
pub const AWNING_C: usize = 8;
pub const VENDING: usize = 9;
pub const STALL: usize = 10;
pub const POSTER: usize = 11;
pub const S_TILE: [[f64; 2]; 12] = [
    [10.0, 4.6], // shop fronts: display windows and a door under a sign band
    [9.0, 4.6],  // bar / restaurant: small warm windows, a lit doorway
    [5.4, 3.4],  // rowhouse floor: two tall sash windows with trim (tinted)
    [6.0, 3.4],  // plain stucco wall with small windows (tinted)
    [8.0, 4.6],  // closed shop: roller shutter, one lit sign box
    [12.0, 6.0], // office lobby: double-height glass, reception, ceiling lights
    [10.0, 4.6], // convenience store: bright shelves behind full glass
    [5.4, 3.5],  // rowhouse street storey: garage door and raised entry (tinted)
    [1.2, 1.0],  // awning fabric stripes (tinted), lit through from below
    [1.0, 1.9],  // drinks vending machine front
    [3.0, 2.4],  // street food stall front: noren curtain, counter, menu
    [1.4, 2.0],  // lit poster panel (bus shelters, kiosks)
];
/// Window grid and lighting kind per cell (see facades.js windowLight): 0 =
/// as drawn with a per-building brightness, 1 = flats.
pub const S_GRID: [[f64; 3]; 12] = [
    [1.0, 1.0, 0.0],
    [1.0, 1.0, 0.0],
    [2.0, 1.0, 1.0],
    [2.0, 1.0, 1.0],
    [1.0, 1.0, 0.0],
    [1.0, 1.0, 0.0],
    [1.0, 1.0, 0.0],
    [1.0, 1.0, 0.0],
    [1.0, 1.0, 0.0],
    [1.0, 1.0, 0.0],
    [1.0, 1.0, 0.0],
    [1.0, 1.0, 0.0],
];
const SC: f64 = 384.0;

/// A JS number in a template string (`${v}`).
fn js_num(v: f64) -> String {
    format!("{v}")
}

/// `tex(c, { srgb = true, repeat = true })`: anisotropy 8.
fn tex(c: &Canvas, srgb: bool, repeat: bool) -> Texture {
    Texture::from_canvas(c, repeat, srgb, 8.0)
}

fn pair(map: Texture, emissive: Texture) -> Cached {
    Cached::Facade {
        map,
        emissive,
        cols: 0,
        rows: 0,
    }
}

/// `cell(k, fn)`: both canvases clipped to cell k and moved to its corner.
fn cell(g: &mut Canvas, ge: &mut Canvas, k: usize, draw: impl FnOnce(&mut Canvas, &mut Canvas)) {
    let x0 = (k % 4) as f64 * SC;
    let y0 = (k / 4) as f64 * SC;
    g.save();
    ge.save();
    g.begin_path();
    g.rect(x0, y0, SC, SC);
    g.clip();
    ge.begin_path();
    ge.rect(x0, y0, SC, SC);
    ge.clip();
    g.translate(x0, y0);
    ge.translate(x0, y0);
    draw(g, ge);
    g.restore();
    ge.restore();
}

fn speck(g: &mut Canvas, rng: &mut Mulberry32, n: u32, a: f64) {
    for _ in 0..n {
        let c = if rng.next_f64() < 0.5 {
            "0,0,0"
        } else {
            "255,255,255"
        };
        g.set_fill_style(format!("rgba({c},{})", js_num(a * rng.next_f64())));
        let x = rng.next_f64() * SC;
        let y = rng.next_f64() * SC;
        g.fill_rect(x, y, 2.0, 2.0);
    }
}

/// Goods on shelves: rows of small coloured boxes, drawn into both maps.
#[allow(clippy::too_many_arguments)]
fn goods(ge: &mut Canvas, rng: &mut Mulberry32, x: f64, y: f64, w: f64, h: f64, bright: bool) {
    let mut sy = y + 18.0;
    while sy < y + h - 8.0 {
        ge.set_fill_style("rgba(0,0,0,0.5)");
        ge.fill_rect(x, sy, w, 4.0);
        let mut gx = x + 2.0;
        while gx < x + w - 6.0 {
            let hue = (rng.next_f64() * 360.0).floor();
            let gh = 8.0 + rng.next_f64() * 14.0;
            let s = 50.0 + rng.next_f64() * 40.0;
            let l = if bright {
                55.0 + rng.next_f64() * 25.0
            } else {
                25.0 + rng.next_f64() * 25.0
            };
            ge.set_fill_style(format!(
                "hsl({},{}%,{}%)",
                js_num(hue),
                js_num(s),
                js_num(l)
            ));
            let gw = 3.0 + rng.next_f64() * 5.0;
            ge.fill_rect(gx, sy - gh, gw, gh);
            gx += 5.0 + rng.next_f64() * 7.0;
        }
        sy += 30.0;
    }
}

/// A little person silhouette inside a lit window.
fn person(ge: &mut Canvas, x: f64, y: f64, s: f64) {
    ge.set_fill_style("rgba(10,8,8,0.8)");
    ge.begin_path();
    ge.arc(x, y, 5.0 * s, 0.0, 7.0, false);
    ge.fill();
    ge.fill_rect(x - 7.0 * s, y + 6.0 * s, 14.0 * s, 30.0 * s);
}

/// `streetAtlas()`: `{ map, emissive }`, cached as a façade pair.
pub fn street_atlas(cache: &mut TextureCache) -> Arc<Cached> {
    cache.cached_with("streets:streetAtlas", || {
        let (map, emissive) = draw_street_atlas();
        pair(map, emissive)
    })
}

fn draw_street_atlas() -> (Texture, Texture) {
    const S: f64 = SC;
    let (w, h) = ((S * 4.0) as u32, (S * 3.0) as u32);
    let mut c = Canvas::new(w, h);
    let mut ce = Canvas::new(w, h);
    ce.set_fill_style("#000");
    ce.fill_rect(0.0, 0.0, S * 4.0, S * 3.0);
    let mut rng = Mulberry32::new(311);
    let rng = &mut rng;

    // Shop fronts: dark frame, big display windows lit warm or cool.
    for (k, warm) in [(SHOP_A, false), (SHOP_B, true)] {
        cell(&mut c, &mut ce, k, |g, ge| {
            g.set_fill_style(if warm { "#2e2420" } else { "#26282c" });
            g.fill_rect(0.0, 0.0, S, S);
            speck(g, rng, 600, 0.08);
            // Sign band (the neon signs sit over it) with a lit box sign.
            g.set_fill_style("#131216");
            g.fill_rect(0.0, 0.0, S, 64.0);
            g.set_fill_style("#3a3a40");
            g.fill_rect(0.0, 62.0, S, 6.0);
            ge.set_fill_style(if warm { "#4a2008" } else { "#0a2440" });
            ge.fill_rect(14.0, 12.0, S - 28.0, 38.0);
            ge.set_fill_style(if warm { "#ffb060" } else { "#9fe0ff" });
            ge.set_font("700 26px \"Arial Narrow\", Arial, sans-serif");
            ge.set_text_align("center");
            ge.set_text_baseline("middle");
            ge.fill_text(
                if warm {
                    "BAR · GRILL · LATE"
                } else {
                    "FASHION · GIFTS"
                },
                S / 2.0,
                32.0,
            );
            let panes: [[f64; 4]; 2] = if k == SHOP_A {
                [[14.0, 88.0, 150.0, 250.0], [220.0, 88.0, 150.0, 250.0]]
            } else {
                [[18.0, 110.0, 96.0, 150.0], [270.0, 110.0, 96.0, 150.0]]
            };
            let door: [f64; 4] = if k == SHOP_A {
                [168.0, 96.0, 48.0, 288.0]
            } else {
                [140.0, 92.0, 104.0, 292.0]
            };
            for [px, py, pw, ph] in panes {
                g.set_fill_style("#111317");
                g.fill_rect(px, py, pw, ph);
                let mut grd = ge.create_linear_gradient(0.0, py, 0.0, py + ph);
                const COOL: [[&str; 2]; 3] = [
                    ["#d0f0ff", "#3a6cb8"],
                    ["#ffe0f4", "#a03c84"],
                    ["#f0ffe0", "#4a8a3c"],
                ];
                let cols = if warm {
                    ["#ffd49a", "#b85a20"]
                } else {
                    COOL[(rng.next_f64() * 3.0).floor() as usize]
                };
                grd.add_color_stop(0.0, cols[0]);
                grd.add_color_stop(1.0, cols[1]);
                ge.set_fill_style(&grd);
                ge.fill_rect(px, py, pw, ph);
                // Spotlights along the top, goods or diners inside.
                let mut q = px + 12.0;
                while q < px + pw - 6.0 {
                    ge.set_fill_style("#ffffff");
                    ge.begin_path();
                    ge.arc(q, py + 6.0, 3.0, 0.0, 7.0, false);
                    ge.fill();
                    q += 26.0;
                }
                if warm {
                    ge.set_fill_style("rgba(60,20,4,0.55)");
                    ge.fill_rect(px, py + ph * 0.62, pw, ph * 0.38); // tables
                    for _ in 0..2 {
                        let x = px + 20.0 + rng.next_f64() * (pw - 40.0);
                        person(ge, x, py + ph * 0.38, 1.1);
                    }
                } else {
                    goods(ge, rng, px + 4.0, py + 30.0, pw - 8.0, ph - 40.0, true);
                    if rng.next_f64() < 0.7 {
                        // mannequin
                        ge.set_fill_style("rgba(20,16,20,0.65)");
                        let mx = px + pw * (0.3 + rng.next_f64() * 0.4);
                        ge.begin_path();
                        ge.arc(mx, py + 60.0, 8.0, 0.0, 7.0, false);
                        ge.fill();
                        ge.fill_rect(mx - 12.0, py + 70.0, 24.0, 70.0);
                    }
                }
                // Window lettering and a reflection sheen.
                g.set_stroke_style("#6a6a70");
                g.set_line_width(4.0);
                g.stroke_rect(px, py, pw, ph);
                ge.set_fill_style("rgba(255,255,255,0.12)");
                ge.begin_path();
                ge.move_to(px + pw * 0.1, py + ph);
                ge.line_to(px + pw * 0.45, py);
                ge.line_to(px + pw * 0.6, py);
                ge.line_to(px + pw * 0.25, py + ph);
                ge.fill();
            }
            g.set_fill_style("#0c0c0e");
            g.fill_rect(door[0], door[1], door[2], door[3]);
            let mut dg = ge.create_linear_gradient(0.0, door[1], 0.0, door[1] + door[3]);
            dg.add_color_stop(0.0, if warm { "#c07030" } else { "#4a78a0" });
            dg.add_color_stop(1.0, if warm { "#502008" } else { "#182838" });
            ge.set_fill_style(&dg);
            ge.fill_rect(door[0] + 6.0, door[1] + 6.0, door[2] - 12.0, door[3] - 6.0);
            if warm {
                person(ge, door[0] + door[2] / 2.0, door[1] + 90.0, 1.6);
            }
            ge.set_fill_style("#000");
            ge.fill_rect(door[0] + door[2] / 2.0 - 2.0, door[1], 4.0, door[3]);
            g.set_fill_style("#c8c0a8");
            g.fill_rect(door[0] + door[2] / 2.0 + 6.0, door[1] + 150.0, 4.0, 30.0); // handle
            // Stall risers.
            g.set_fill_style("#3a3634");
            g.fill_rect(0.0, S - 30.0, S, 30.0);
            g.set_fill_style("rgba(255,255,255,0.08)");
            g.fill_rect(0.0, S - 30.0, S, 2.0);
        });
    }

    // Rowhouse floor: light base so vertex colours tint it; two tall sash
    // windows (at a quarter and three quarters across) with white casings.
    cell(&mut c, &mut ce, ROWHOUSE, |g, ge| {
        g.set_fill_style("#e8e4dc");
        g.fill_rect(0.0, 0.0, S, S);
        g.set_fill_style("#cfc9bf");
        let mut y = 0.0;
        while y < S {
            g.fill_rect(0.0, y, S, 3.0); // shiplap siding
            y += 16.0;
        }
        g.set_fill_style("rgba(0,0,0,0.05)");
        let mut y = 3.0;
        while y < S {
            g.fill_rect(0.0, y, S, 2.0);
            y += 16.0;
        }
        g.set_fill_style("#fbfaf6");
        g.fill_rect(0.0, 0.0, S, 20.0); // floor band
        for cxw in [S * 0.25, S * 0.75] {
            let (ww, wy, wh) = (96.0, 80.0, 236.0);
            let wx = cxw - ww / 2.0;
            g.set_fill_style("#fbfaf6");
            g.fill_rect(wx - 14.0, wy - 12.0, ww + 28.0, wh + 24.0); // casing
            g.set_fill_style("#fbfaf6");
            g.fill_rect(wx - 24.0, wy - 34.0, ww + 48.0, 20.0); // hood
            g.set_fill_style("#e0dcd2");
            g.begin_path();
            g.move_to(wx - 24.0, wy - 34.0);
            g.line_to(cxw, wy - 64.0);
            g.line_to(wx + ww + 24.0, wy - 34.0);
            g.fill(); // pediment
            g.set_fill_style("#fbfaf6");
            g.fill_rect(wx - 20.0, wy + wh + 10.0, ww + 40.0, 12.0); // sill
            g.set_fill_style("#1b2230");
            g.fill_rect(wx, wy, ww, wh);
            let mut grd = ge.create_linear_gradient(0.0, wy, 0.0, wy + wh);
            grd.add_color_stop(0.0, "#ffe0a8");
            grd.add_color_stop(1.0, "#c07838");
            ge.set_fill_style(&grd);
            ge.fill_rect(wx, wy, ww, wh);
            // Lace curtains and a lamp.
            ge.set_fill_style("rgba(255,250,240,0.35)");
            ge.fill_rect(wx, wy, ww * 0.3, wh);
            ge.fill_rect(wx + ww * 0.7, wy, ww * 0.3, wh);
            ge.set_fill_style("rgba(0,0,0,0.5)");
            let bh = 30.0 + rng.next_f64() * 50.0;
            ge.fill_rect(wx, wy, ww, bh); // blind
            g.set_fill_style("#fbfaf6");
            ge.set_fill_style("#000");
            for [rx, ry, rw, rh] in [
                [wx, wy + wh / 2.0 - 4.0, ww, 8.0],
                [wx + ww / 2.0 - 3.0, wy, 6.0, wh],
            ] {
                g.fill_rect(rx, ry, rw, rh);
                ge.fill_rect(rx, ry, rw, rh);
            }
        }
    });

    // Stucco wall with small windows.
    cell(&mut c, &mut ce, STUCCO, |g, ge| {
        g.set_fill_style("#dcd6cc");
        g.fill_rect(0.0, 0.0, S, S);
        speck(g, rng, 900, 0.06);
        for cxw in [S * 0.25, S * 0.75] {
            let (wx, wy, ww, wh) = (cxw - 32.0, 120.0, 64.0, 140.0);
            g.set_fill_style("#20242c");
            g.fill_rect(wx, wy, ww, wh);
            g.set_fill_style("#f4f2ec");
            g.fill_rect(wx - 6.0, wy + wh, ww + 12.0, 10.0);
            ge.set_fill_style("#ffb860");
            ge.fill_rect(wx, wy, ww, wh);
            ge.set_fill_style("rgba(0,0,0,0.5)");
            let bh = 20.0 + rng.next_f64() * 60.0;
            ge.fill_rect(wx, wy, ww, bh);
        }
    });

    // Closed shop: roller shutter with graffiti and a lit sign box.
    cell(&mut c, &mut ce, SHUTTER, |g, ge| {
        g.set_fill_style("#26262a");
        g.fill_rect(0.0, 0.0, S, S);
        g.set_fill_style("#17161a");
        g.fill_rect(0.0, 0.0, S, 64.0);
        g.set_fill_style("#7d8088");
        g.fill_rect(20.0, 80.0, S - 40.0, 290.0);
        let mut y = 84.0;
        while y < 370.0 {
            g.set_fill_style("#5c5f66");
            g.fill_rect(20.0, y, S - 40.0, 4.0);
            g.set_fill_style("rgba(255,255,255,0.12)");
            g.fill_rect(20.0, y + 5.0, S - 40.0, 1.0);
            y += 12.0;
        }
        const TAG: [&str; 4] = ["#ff3c8a", "#37d0ff", "#ffd23c", "#8aff4a"];
        g.set_line_width(9.0);
        g.set_line_cap("round");
        for _ in 0..4 {
            g.set_stroke_style(TAG[(rng.next_f64() * TAG.len() as f64).floor() as usize]);
            g.begin_path();
            let mut x = 60.0 + rng.next_f64() * 220.0;
            let mut y = 180.0 + rng.next_f64() * 140.0;
            g.move_to(x, y);
            for _ in 0..5 {
                x += (rng.next_f64() - 0.3) * 50.0;
                y += (rng.next_f64() - 0.5) * 50.0;
                g.line_to(x, y);
            }
            g.stroke();
        }
        ge.set_fill_style("#301008");
        ge.fill_rect(12.0, 14.0, S - 24.0, 36.0);
        ge.set_fill_style("#ff7040");
        ge.set_font("700 24px \"Arial Narrow\", Arial, sans-serif");
        ge.set_text_align("center");
        ge.set_text_baseline("middle");
        ge.fill_text("PAWN · GOLD · CASH", S / 2.0, 32.0);
    });

    // Lobby: double-height glass with mullions; inside, a downlit ceiling, a
    // warm stone back wall with the lifts, a reception desk and a polished
    // floor that reflects the lights.
    cell(&mut c, &mut ce, LOBBY, |g, ge| {
        g.set_fill_style("#15181d");
        g.fill_rect(0.0, 0.0, S, S);
        g.set_fill_style("#1c2530");
        g.fill_rect(8.0, 30.0, S - 16.0, S - 38.0);
        // Ceiling.
        ge.set_fill_style("#1a140e");
        ge.fill_rect(8.0, 30.0, S - 16.0, 50.0);
        // Back wall: stone panels.
        let mut wg = ge.create_linear_gradient(0.0, 80.0, 0.0, 260.0);
        wg.add_color_stop(0.0, "#6a5234");
        wg.add_color_stop(1.0, "#8a6c46");
        ge.set_fill_style(&wg);
        ge.fill_rect(8.0, 80.0, S - 16.0, 180.0);
        ge.set_fill_style("rgba(0,0,0,0.3)");
        let mut x = 8.0;
        while x < S {
            ge.fill_rect(x, 80.0, 2.0, 180.0);
            x += 46.0;
        }
        ge.fill_rect(8.0, 170.0, S - 16.0, 2.0);
        // Lifts: a lit recess with steel doors.
        ge.set_fill_style("#e8d4ae");
        ge.fill_rect(210.0, 150.0, 130.0, 110.0);
        ge.set_fill_style("#3a342c");
        for x in [222.0, 262.0, 302.0] {
            ge.fill_rect(x, 162.0, 30.0, 98.0);
        }
        // Floor: polished, dark with streaks of reflected light.
        let mut fg = ge.create_linear_gradient(0.0, 260.0, 0.0, S);
        fg.add_color_stop(0.0, "#3a2e22");
        fg.add_color_stop(1.0, "#6a5640");
        ge.set_fill_style(&fg);
        ge.fill_rect(8.0, 260.0, S - 16.0, S - 268.0);
        let mut x = 40.0;
        while x < S {
            ge.set_fill_style("#fff6e0");
            ge.begin_path();
            ge.ellipse(x, 52.0, 14.0, 4.0, 0.0, 0.0, 7.0, false);
            ge.fill();
            let mut rg = ge.create_linear_gradient(0.0, 262.0, 0.0, S);
            rg.add_color_stop(0.0, "rgba(255,230,190,0.35)");
            rg.add_color_stop(1.0, "rgba(255,230,190,0)");
            ge.set_fill_style(&rg);
            ge.fill_rect(x - 6.0, 262.0, 12.0, S - 262.0);
            x += 76.0;
        }
        // Reception desk with a lit front edge, a guard behind it.
        person(ge, 110.0, 250.0, 1.2);
        ge.set_fill_style("#1e1810");
        ge.fill_rect(50.0, 280.0, 150.0, 44.0);
        ge.set_fill_style("#ffd890");
        ge.fill_rect(50.0, 280.0, 150.0, 3.0);
        ge.fill_rect(50.0, 321.0, 150.0, 3.0);
        // A lit logo on the back wall.
        ge.set_fill_style("#ffe8c0");
        ge.fill_rect(60.0, 110.0, 110.0, 6.0);
        ge.fill_rect(60.0, 124.0, 70.0, 4.0);
        // Mullions and a transom.
        ge.set_fill_style("#000");
        g.set_fill_style("#343c48");
        let mut x = 8.0;
        while x < S {
            ge.fill_rect(x, 30.0, 7.0, S - 30.0);
            g.fill_rect(x, 30.0, 7.0, S - 30.0);
            x += 75.0;
        }
        ge.fill_rect(0.0, 176.0, S, 7.0);
        g.fill_rect(0.0, 176.0, S, 7.0);
        g.set_fill_style("#2a2e36");
        g.fill_rect(0.0, 0.0, S, 30.0);
    });

    // Convenience store: blue-white fluorescent, full glass, colourful shelves.
    cell(&mut c, &mut ce, KONBINI, |g, ge| {
        g.set_fill_style("#e8eaec");
        g.fill_rect(0.0, 0.0, S, S);
        g.set_fill_style("#1a8a4a");
        g.fill_rect(0.0, 0.0, S, 22.0);
        g.set_fill_style("#ff7a1a");
        g.fill_rect(0.0, 22.0, S, 14.0);
        g.set_fill_style("#d02a2a");
        g.fill_rect(0.0, 36.0, S, 10.0);
        ge.set_fill_style("#304030");
        ge.fill_rect(0.0, 0.0, S, 46.0);
        let mut grd = ge.create_linear_gradient(0.0, 60.0, 0.0, S);
        grd.add_color_stop(0.0, "#c8d4e0");
        grd.add_color_stop(1.0, "#6a8098");
        ge.set_fill_style(&grd);
        ge.fill_rect(6.0, 60.0, S - 12.0, S - 80.0);
        goods(ge, rng, 10.0, 110.0, S - 20.0, 220.0, true);
        let mut x = 20.0;
        while x < S {
            ge.set_fill_style("#ffffff");
            ge.fill_rect(x, 64.0, 40.0, 4.0);
            x += 60.0;
        }
        person(ge, 90.0, 190.0, 1.3);
        person(ge, 290.0, 200.0, 1.2);
        ge.set_fill_style("#000");
        g.set_fill_style("#9aa0a8");
        for x in [6.0, 128.0, 256.0, S - 10.0] {
            ge.fill_rect(x, 60.0, 5.0, S - 80.0);
            g.fill_rect(x, 60.0, 5.0, S - 80.0);
        }
        g.set_fill_style("#6a6e74");
        g.fill_rect(0.0, S - 20.0, S, 20.0);
    });

    // Rowhouse street storey: a garage door on one side, the recessed entry
    // up a few steps on the other. Tinted by vertex colour like the floors.
    cell(&mut c, &mut ce, ROW_GROUND, |g, ge| {
        g.set_fill_style("#e4e0d8");
        g.fill_rect(0.0, 0.0, S, S);
        g.set_fill_style("#d0cbc2");
        let mut y = 0.0;
        while y < S {
            g.fill_rect(0.0, y, S, 3.0);
            y += 16.0;
        }
        g.set_fill_style("#fbfaf6");
        g.fill_rect(0.0, 0.0, S, 18.0);
        // Garage door (panelled, white-ish).
        let (gx, gw, gy) = (20.0, 200.0, 150.0);
        g.set_fill_style("#fbfaf6");
        g.fill_rect(gx - 10.0, gy - 14.0, gw + 20.0, S - gy + 14.0);
        g.set_fill_style("#d8d4cc");
        g.fill_rect(gx, gy, gw, S - gy);
        g.set_fill_style("rgba(0,0,0,0.12)");
        let mut y = gy + 10.0;
        while y < S {
            let mut x = gx + 8.0;
            while x < gx + gw - 8.0 {
                g.fill_rect(x, y, 40.0, 28.0);
                x += 48.0;
            }
            y += 38.0;
        }
        ge.set_fill_style("#fff0c8");
        ge.fill_rect(gx + gw / 2.0 - 8.0, gy - 30.0, 16.0, 10.0); // lamp over garage
        // Entry: panelled door with a lit fan transom, raised.
        let (dx, dw, dy, dh) = (262.0, 76.0, 70.0, 230.0);
        g.set_fill_style("#fbfaf6");
        g.fill_rect(dx - 16.0, dy - 40.0, dw + 32.0, dh + 40.0);
        g.set_fill_style("#3a2418");
        g.fill_rect(dx, dy, dw, dh);
        g.set_fill_style("rgba(0,0,0,0.25)");
        g.fill_rect(dx + 10.0, dy + 20.0, dw - 20.0, 80.0);
        g.fill_rect(dx + 10.0, dy + 120.0, dw - 20.0, 90.0);
        ge.set_fill_style("#ffc070");
        ge.begin_path();
        ge.ellipse(dx + dw / 2.0, dy - 4.0, dw / 2.0, 28.0, 0.0, PI, 0.0, false);
        ge.fill();
        ge.set_fill_style("#ffd890");
        ge.fill_rect(dx + 14.0, dy + 24.0, dw - 28.0, 70.0);
        ge.set_fill_style("rgba(0,0,0,0.5)");
        ge.fill_rect(dx + dw / 2.0 - 2.0, dy + 24.0, 4.0, 70.0);
    });

    // Awning fabric: two-tone stripes (tinted); lit faintly through.
    cell(&mut c, &mut ce, AWNING_C, |g, ge| {
        let mut x = 0.0;
        while x < S {
            g.set_fill_style("#f4f0e8");
            g.fill_rect(x, 0.0, 32.0, S);
            g.set_fill_style("#b8b0a8");
            g.fill_rect(x + 32.0, 0.0, 32.0, S);
            x += 64.0;
        }
        g.set_fill_style("rgba(0,0,0,0.25)");
        g.fill_rect(0.0, S - 30.0, S, 30.0);
        let mut x = 0.0;
        while x < S {
            g.set_fill_style("rgba(0,0,0,0.3)");
            g.begin_path();
            g.arc(x + 16.0, S - 30.0, 16.0, 0.0, PI, false);
            g.fill();
            x += 32.0;
        }
        ge.set_fill_style("#2a1a10");
        ge.fill_rect(0.0, 0.0, S, S);
    });

    // Vending machine: lit display of cans, buttons, pickup slot.
    cell(&mut c, &mut ce, VENDING, |g, ge| {
        g.set_fill_style("#d8dce0");
        g.fill_rect(0.0, 0.0, S, S);
        const COLS: [&str; 3] = ["#d8202a", "#2050c0", "#f0f0f0"];
        let col = COLS[(rng.next_f64() * 3.0).floor() as usize];
        g.set_fill_style(col);
        g.fill_rect(0.0, 0.0, S, 50.0);
        g.fill_rect(0.0, S - 40.0, S, 40.0);
        ge.set_fill_style("#e8f4ff");
        ge.fill_rect(20.0, 60.0, S - 40.0, 200.0);
        for r in 0..4 {
            for q in 0..8 {
                let cx = 36.0 + f64::from(q) * 40.0;
                let cy = 80.0 + f64::from(r) * 48.0;
                let hue = (rng.next_f64() * 360.0).floor();
                let l = 45.0 + rng.next_f64() * 20.0;
                ge.set_fill_style(format!("hsl({},70%,{}%)", js_num(hue), js_num(l)));
                ge.fill_rect(cx - 9.0, cy, 18.0, 30.0);
                ge.set_fill_style("rgba(0,0,0,0.3)");
                ge.fill_rect(cx - 9.0, cy + 26.0, 18.0, 4.0);
                ge.set_fill_style("#ff4040");
                ge.fill_rect(cx - 5.0, cy + 34.0, 10.0, 4.0);
            }
        }
        g.set_fill_style("#1a1c20");
        g.fill_rect(60.0, 290.0, S - 120.0, 36.0);
        g.set_fill_style("#404448");
        g.fill_rect(S - 70.0, 270.0, 40.0, 14.0);
        ge.set_fill_style("#80ff80");
        ge.fill_rect(S - 66.0, 272.0, 16.0, 6.0);
    });

    // Food stall: noren curtain, steaming counter, lit menu boards.
    cell(&mut c, &mut ce, STALL, |g, ge| {
        g.set_fill_style("#3a2418");
        g.fill_rect(0.0, 0.0, S, S);
        let mut grd = ge.create_linear_gradient(0.0, 0.0, 0.0, S);
        grd.add_color_stop(0.0, "#ffc880");
        grd.add_color_stop(1.0, "#a04a10");
        ge.set_fill_style(&grd);
        ge.fill_rect(0.0, 60.0, S, 190.0);
        // Noren: split cloth panels hanging from the top.
        let mut x = 0.0;
        while x < S {
            g.set_fill_style(if rng.next_f64() < 0.5 {
                "#1c2a50"
            } else {
                "#6a1414"
            });
            g.fill_rect(x + 2.0, 0.0, 44.0, 120.0);
            ge.set_fill_style("#000");
            ge.fill_rect(x + 2.0, 0.0, 44.0, 120.0);
            g.set_fill_style("#f0e8d8");
            g.set_font("700 30px sans-serif");
            g.set_text_align("center");
            g.fill_text("●", x + 24.0, 70.0);
            x += 48.0;
        }
        person(ge, 140.0, 150.0, 1.6);
        person(ge, 270.0, 160.0, 1.5);
        // Counter with pots.
        g.set_fill_style("#6a4a30");
        g.fill_rect(0.0, 250.0, S, S - 250.0);
        g.set_fill_style("rgba(0,0,0,0.3)");
        let mut x = 0.0;
        while x < S {
            g.fill_rect(x, 250.0, 2.0, S - 250.0);
            x += 24.0;
        }
        ge.set_fill_style("rgba(0,0,0,0.85)");
        ge.fill_rect(0.0, 250.0, S, S - 250.0);
        ge.set_fill_style("#ffe0b0");
        ge.fill_rect(0.0, 250.0, S, 6.0);
        for x in [70.0, 190.0, 310.0] {
            ge.set_fill_style("#c8c8c8");
            ge.fill_rect(x - 30.0, 214.0, 60.0, 36.0);
        }
    });

    // Poster panel: backlit ad (invented brands).
    cell(&mut c, &mut ce, POSTER, |g, ge| {
        g.set_fill_style("#202224");
        g.fill_rect(0.0, 0.0, S, S);
        let mut grd = ge.create_linear_gradient(0.0, 0.0, 0.0, S);
        grd.add_color_stop(0.0, "#2a0a50");
        grd.add_color_stop(1.0, "#ff3ca8");
        ge.set_fill_style(&grd);
        ge.fill_rect(16.0, 16.0, S - 32.0, S - 32.0);
        ge.set_fill_style("#ffffff");
        ge.set_font("900 italic 64px \"Arial Narrow\", Arial, sans-serif");
        ge.set_text_align("center");
        ge.fill_text("NIGHT", S / 2.0, 130.0);
        ge.fill_text("SHIFT", S / 2.0, 196.0);
        ge.set_font("700 26px Arial");
        ge.set_fill_style("#7cf6ff");
        ge.fill_text("ENERGY · ALL NIGHT", S / 2.0, 300.0);
        ge.set_fill_style("rgba(255,255,255,0.9)");
        ge.begin_path();
        ge.arc(S / 2.0, 240.0, 22.0, 0.0, 7.0, false);
        ge.fill();
        g.set_fill_style("#505458");
        g.fill_rect(0.0, 0.0, S, 16.0);
        g.fill_rect(0.0, S - 16.0, S, 16.0);
        g.fill_rect(0.0, 0.0, 16.0, S);
        g.fill_rect(S - 16.0, 0.0, 16.0, S);
    });

    (tex(&c, true, false), tex(&ce, true, false))
}

// ── Neon signs ───────────────────────────────────────────────────
// A 4×4 atlas of horizontal signs (2:1) and a strip of 8 vertical blade
// signs (1:4). Drawn as glowing tubes on black for additive blending.
const WORDS: [&str; 16] = [
    "RAMEN",
    "KARAOKE",
    "HOTEL",
    "LIVE JAZZ",
    "NOODLES",
    "ARCADE",
    "OPEN 24H",
    "SUSHI",
    "DINER",
    "CLUB 88",
    "TATTOO",
    "DUMPLINGS",
    "LIQUOR",
    "CAFE",
    "PAWN",
    "BILLIARDS",
];
const BLADES: [&str; 8] = [
    "HOTEL", "BAR", "RAMEN", "CLUB", "EAT", "DANCE", "SAKE", "JAZZ",
];
pub const NEON_COLORS: [&str; 8] = [
    "#ff3ca8", "#3ce8ff", "#ff5a3c", "#ffd23c", "#7cff5a", "#b45aff", "#ff8a2a", "#5a8cff",
];
pub const H_SIGNS: usize = WORDS.len();
pub const V_SIGNS: usize = BLADES.len();

#[allow(clippy::too_many_arguments)]
fn neon_text(g: &mut Canvas, text: &str, x: f64, y: f64, size: f64, col: &str, max_w: f64) {
    g.set_font(&format!(
        "700 {}px \"Arial Narrow\", Arial, sans-serif",
        js_num(size)
    ));
    g.set_text_align("center");
    g.set_text_baseline("middle");
    g.set_line_join("round");
    for (blur, lw, c) in [
        (size * 0.5, size * 0.16, col),
        (size * 0.2, size * 0.1, col),
        (0.0, size * 0.045, "#ffffff"),
    ] {
        g.set_shadow_color(col);
        g.set_shadow_blur(blur);
        g.set_stroke_style(c);
        g.set_line_width(lw);
        g.stroke_text_max(text, x, y, max_w);
    }
    g.set_shadow_blur(0.0);
}

/// `neonAtlas()`: `{ h, v }`, cached as a pair (`Layer::Main` is `h`,
/// `Layer::Emissive` is `v`).
pub fn neon_atlas(cache: &mut TextureCache) -> Arc<Cached> {
    cache.cached_with("streets:neon", || {
        let (h, v) = draw_neon_atlas();
        pair(h, v)
    })
}

fn draw_neon_atlas() -> (Texture, Texture) {
    // Horizontal: 4×4 cells of 256×128.
    let mut g = Canvas::new(1024, 512);
    g.set_fill_style("#000");
    g.fill_rect(0.0, 0.0, 1024.0, 512.0);
    for (i, w) in WORDS.iter().enumerate() {
        let cx = (i % 4) as f64 * 256.0 + 128.0;
        let cy = (i / 4) as f64 * 128.0 + 64.0;
        let col = NEON_COLORS[i % NEON_COLORS.len()];
        g.save();
        g.begin_path();
        g.rect(cx - 124.0, cy - 60.0, 248.0, 120.0);
        g.clip();
        // A tube border on some.
        if i % 3 == 0 {
            g.set_shadow_color(col);
            g.set_shadow_blur(14.0);
            g.set_stroke_style(col);
            g.set_line_width(5.0);
            g.stroke_rect(cx - 112.0, cy - 48.0, 224.0, 96.0);
            g.set_shadow_blur(0.0);
            g.set_stroke_style("#fff");
            g.set_line_width(1.5);
            g.stroke_rect(cx - 112.0, cy - 48.0, 224.0, 96.0);
        }
        let size = if w.chars().count() > 7 { 46.0 } else { 62.0 };
        neon_text(&mut g, w, cx, cy + 2.0, size, col, 200.0);
        g.restore();
    }
    // Vertical: 8 cells of 128×512, letters stacked.
    let mut gv = Canvas::new(1024, 512);
    gv.set_fill_style("#000");
    gv.fill_rect(0.0, 0.0, 1024.0, 512.0);
    for (i, w) in BLADES.iter().enumerate() {
        let cx = i as f64 * 128.0 + 64.0;
        let col = NEON_COLORS[(i * 3 + 1) % NEON_COLORS.len()];
        gv.save();
        gv.begin_path();
        gv.rect(cx - 62.0, 2.0, 124.0, 508.0);
        gv.clip();
        gv.set_shadow_color(col);
        gv.set_shadow_blur(12.0);
        gv.set_stroke_style(col);
        gv.set_line_width(5.0);
        gv.stroke_rect(cx - 50.0, 14.0, 100.0, 484.0);
        gv.set_shadow_blur(0.0);
        let letters: Vec<char> = w.chars().collect();
        let n = letters.len() as f64;
        let step = mp_math::js::min(96.0, 460.0 / n);
        for (k, ch) in letters.iter().enumerate() {
            let y = 256.0 - ((n - 1.0) / 2.0 - k as f64) * step;
            let size = mp_math::js::min(84.0, step * 0.95);
            neon_text(&mut gv, &ch.to_string(), cx, y, size, col, 90.0);
        }
        gv.restore();
    }
    (tex(&g, true, false), tex(&gv, true, false))
}

/// `pavementTexture()`: concrete slabs with joints (world-mapped, 1 repeat
/// = 3 m).
pub fn pavement_texture(cache: &mut TextureCache) -> Arc<Cached> {
    cache.cached_with("streets:pavement", || {
        const S: f64 = 256.0;
        let mut g = Canvas::new(S as u32, S as u32);
        g.set_fill_style("#9d9891");
        g.fill_rect(0.0, 0.0, S, S);
        let mut rng = Mulberry32::new(5150);
        let mut img = g.get_image_data(0, 0, S as u32, S as u32);
        for i in 0..(S * S) as usize {
            let n = (rng.next_f64() - 0.5) * 18.0;
            for k in 0..3 {
                let v = f64::from(img.data[i * 4 + k]) + n;
                img.set(i * 4 + k, v);
            }
        }
        g.put_image_data(&img, 0, 0);
        for _ in 0..14 {
            g.set_fill_style(format!(
                "rgba(40,36,30,{})",
                js_num(0.05 + rng.next_f64() * 0.08)
            ));
            g.begin_path();
            let x = rng.next_f64() * S;
            let y = rng.next_f64() * S;
            let rx = 6.0 + rng.next_f64() * 26.0;
            let ry = 4.0 + rng.next_f64() * 14.0;
            let rot = rng.next_f64() * 3.0;
            g.ellipse(x, y, rx, ry, rot, 0.0, 7.0, false);
            g.fill();
        }
        g.set_fill_style("rgba(30,28,26,0.55)");
        g.fill_rect(0.0, 0.0, S, 3.0);
        g.fill_rect(0.0, 0.0, 3.0, S);
        g.fill_rect(0.0, S / 2.0, S, 2.0);
        g.fill_rect(S / 2.0, 0.0, 2.0, S);
        Cached::One(tex(&g, true, true))
    })
}

/// `barrierTexture()`: water-filled barrier, white body with a red/white
/// chevron band.
pub fn barrier_texture(cache: &mut TextureCache) -> Arc<Cached> {
    cache.cached_with("streets:barrier", || {
        let mut g = Canvas::new(256, 128);
        g.set_fill_style("#e8e6e0");
        g.fill_rect(0.0, 0.0, 256.0, 128.0);
        g.set_fill_style("#d0201c");
        let mut x = -128.0;
        while x < 256.0 {
            g.begin_path();
            g.move_to(x, 110.0);
            g.line_to(x + 32.0, 30.0);
            g.line_to(x + 64.0, 30.0);
            g.line_to(x + 32.0, 110.0);
            g.close_path();
            g.fill();
            x += 64.0;
        }
        g.set_fill_style("#1a1a1a");
        g.fill_rect(0.0, 118.0, 256.0, 10.0);
        g.set_fill_style("rgba(0,0,0,0.25)");
        g.fill_rect(0.0, 0.0, 256.0, 6.0);
        Cached::One(tex(&g, true, true))
    })
}

/// `puddleTexture()`: an irregular soft blob (alpha in all channels) for
/// additive reflections of the lights above it on the wet road.
pub fn puddle_texture(cache: &mut TextureCache) -> Arc<Cached> {
    cache.cached_with("streets:puddle", || {
        const S: f64 = 128.0;
        let mut g = Canvas::new(S as u32, S as u32);
        g.set_fill_style("#000");
        g.fill_rect(0.0, 0.0, S, S);
        let mut rng = Mulberry32::new(808);
        g.set_filter("blur(6px)");
        for _ in 0..9 {
            let a = rng.next_f64() * 6.28;
            let r = 12.0 + rng.next_f64() * 22.0;
            g.set_fill_style(format!(
                "rgba(255,255,255,{})",
                js_num(0.5 + rng.next_f64() * 0.5)
            ));
            g.begin_path();
            let x = S / 2.0 + kernel::cos(a) * r * 0.8;
            let y = S / 2.0 + kernel::sin(a) * r * 0.5;
            let rx = 14.0 + rng.next_f64() * 22.0;
            let ry = 8.0 + rng.next_f64() * 14.0;
            let rot = rng.next_f64() * 3.0;
            g.ellipse(x, y, rx, ry, rot, 0.0, 7.0, false);
            g.fill();
        }
        g.set_filter("none");
        Cached::One(tex(&g, true, false))
    })
}

/// `trainTexture()`: one texture for the whole elevated train car. Left
/// half: the side (silver, a band of lit windows with passengers, doors, a
/// stripe); right quarter: the cab end; last quarter: the roof. `{ map, e }`
/// cached as a pair.
pub fn train_texture(cache: &mut TextureCache) -> Arc<Cached> {
    cache.cached_with("streets:train", || {
        const W: f64 = 1024.0;
        const H: f64 = 128.0;
        let mut g = Canvas::new(W as u32, H as u32);
        let mut ge = Canvas::new(W as u32, H as u32);
        let mut rng = Mulberry32::new(99);
        ge.set_fill_style("#000");
        ge.fill_rect(0.0, 0.0, W, H);
        // Side (0..640).
        let mut sg = g.create_linear_gradient(0.0, 0.0, 0.0, H);
        sg.add_color_stop(0.0, "#c8ccd0");
        sg.add_color_stop(0.5, "#9aa0a6");
        sg.add_color_stop(1.0, "#6a7076");
        g.set_fill_style(&sg);
        g.fill_rect(0.0, 0.0, 640.0, H);
        let mut x = 0.0;
        while x < 640.0 {
            g.set_fill_style("rgba(0,0,0,0.06)");
            g.fill_rect(x, 70.0, 2.0, 58.0);
            x += 6.0;
        }
        g.set_fill_style("#1a8a5a");
        g.fill_rect(0.0, 92.0, 640.0, 10.0);
        ge.set_fill_style("#0a3020");
        ge.fill_rect(0.0, 92.0, 640.0, 10.0);
        let mut x = 20.0;
        while x < 620.0 {
            let door = (x - 20.0) % 180.0 == 120.0;
            g.set_fill_style("#20262c");
            if door {
                g.fill_rect(x, 26.0, 44.0, 94.0);
                ge.set_fill_style("#e8e0c8");
                ge.fill_rect(x + 4.0, 30.0, 16.0, 46.0);
                ge.fill_rect(x + 24.0, 30.0, 16.0, 46.0);
                x += 60.0;
                continue;
            }
            g.fill_rect(x, 28.0, 48.0, 44.0);
            let mut grd = ge.create_linear_gradient(0.0, 28.0, 0.0, 72.0);
            grd.add_color_stop(0.0, "#fff6e0");
            grd.add_color_stop(1.0, "#c8b890");
            ge.set_fill_style(&grd);
            ge.fill_rect(x + 2.0, 30.0, 44.0, 40.0);
            ge.set_fill_style("rgba(10,10,14,0.85)");
            for _ in 0..2 {
                if rng.next_f64() < 0.7 {
                    let px = x + 8.0 + rng.next_f64() * 30.0;
                    ge.begin_path();
                    ge.arc(px, 46.0, 4.0, 0.0, 7.0, false);
                    ge.fill();
                    ge.fill_rect(px - 6.0, 51.0, 12.0, 20.0);
                }
            }
            x += 60.0;
        }
        // Cab end (640..896): windscreen, headlights, destination sign.
        g.set_fill_style("#b0b6bc");
        g.fill_rect(640.0, 0.0, 256.0, H);
        g.set_fill_style("#1a2028");
        g.fill_rect(680.0, 22.0, 176.0, 50.0);
        ge.set_fill_style("#403828");
        ge.fill_rect(684.0, 26.0, 168.0, 42.0);
        ge.set_fill_style("#ffb030");
        ge.set_font("700 18px Arial");
        ge.set_text_align("center");
        ge.fill_text("LOOP · DOWNTOWN", 768.0, 16.0);
        for x in [676.0, 860.0] {
            ge.set_fill_style("#ffffff");
            ge.begin_path();
            ge.arc(x, 96.0, 9.0, 0.0, 7.0, false);
            ge.fill();
            g.set_fill_style("#eee");
            g.begin_path();
            g.arc(x, 96.0, 9.0, 0.0, 7.0, false);
            g.fill();
        }
        g.set_fill_style("#1a8a5a");
        g.fill_rect(640.0, 108.0, 256.0, 8.0);
        // Roof (896..1024).
        g.set_fill_style("#7a8086");
        g.fill_rect(896.0, 0.0, 128.0, H);
        let mut y = 0.0;
        while y < H {
            g.set_fill_style("rgba(0,0,0,0.2)");
            g.fill_rect(896.0, y, 128.0, 3.0);
            y += 16.0;
        }
        pair(tex(&g, true, false), tex(&ge, true, false))
    })
}
