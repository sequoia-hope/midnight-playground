//! Port of `src/world/raceway/textures.js` (roadmap WP 7.4): canvas
//! textures for Seaside Raceway: kerbs, tyre walls, catch fencing, the
//! crowd in the grandstands, and the banners and boards round the lap.
//!
//! The JS module caches each picture in a `Map` of its own; here they are
//! entries of the world's [`TextureCache`] under `raceway:` keys, as City's
//! are under `city:` (DECISIONS D352). `bannerAtlas().rect(name)` depends
//! only on the panel's place in [`BANNERS`], so [`banner_rect`] computes it
//! without the picture.

use std::f64::consts::PI;
use std::sync::Arc;

use mr_canvas::Canvas;
use mr_math::Mulberry32;

use crate::textures::{Cached, Texture, TextureCache};

/// `toTexture(c, { repeat = true, aniso = 8 })`: sRGB.
fn to_texture(c: &Canvas, repeat: bool) -> Texture {
    Texture::from_canvas(c, repeat, true, 8.0)
}

/// Kerb: one red and one white block along v, with a painted edge and a
/// little wear. u runs across the kerb.
pub fn kerb_texture(cache: &mut TextureCache) -> Arc<Cached> {
    cache.cached_with("raceway:kerb", || {
        let mut g = Canvas::new(64, 128);
        g.set_fill_style("#c8281e");
        g.fill_rect(0.0, 0.0, 64.0, 64.0);
        g.set_fill_style("#f1efe8");
        g.fill_rect(0.0, 64.0, 64.0, 64.0);
        let mut rng = Mulberry32::new(5);
        for _ in 0..260 {
            let a = 0.05 + rng.next_f64() * 0.12;
            g.set_fill_style(format!("rgba(40,36,32,{a})"));
            let x = rng.next_f64() * 64.0;
            let y = rng.next_f64() * 128.0;
            let w = 1.0 + rng.next_f64() * 3.0;
            let h = 1.0 + rng.next_f64() * 2.0;
            g.fill_rect(x, y, w, h);
        }
        // Rubber on the inner edge.
        let mut grd = g.create_linear_gradient(0.0, 0.0, 20.0, 0.0);
        grd.add_color_stop(0.0, "rgba(20,20,20,0.45)");
        grd.add_color_stop(1.0, "rgba(20,20,20,0)");
        g.set_fill_style(&grd);
        g.fill_rect(0.0, 0.0, 20.0, 128.0);
        Cached::One(to_texture(&g, true))
    })
}

/// Tyre wall: stacked tyres behind a belt of conveyor rubber, the belt in
/// bands of colour. u across the face (height), v along the wall.
pub fn tyre_texture(cache: &mut TextureCache) -> Arc<Cached> {
    cache.cached_with("raceway:tyre", || {
        let mut g = Canvas::new(128, 256);
        g.set_fill_style("#18191b");
        g.fill_rect(0.0, 0.0, 128.0, 256.0);
        let bands = ["#d8d6d0", "#c62a22", "#d8d6d0", "#1f4fa0"];
        for (k, b) in bands.iter().enumerate() {
            g.set_fill_style(*b);
            g.fill_rect(0.0, k as f64 * 64.0 + 6.0, 128.0, 52.0);
        }
        // Bolt lines and grime.
        let mut rng = Mulberry32::new(9);
        g.set_fill_style("rgba(0,0,0,0.35)");
        let mut y = 0.0;
        while y < 256.0 {
            g.fill_rect(0.0, y, 128.0, 2.0);
            y += 16.0;
        }
        for _ in 0..400 {
            let a = 0.05 + rng.next_f64() * 0.15;
            g.set_fill_style(format!("rgba(30,26,22,{a})"));
            let x = rng.next_f64() * 128.0;
            let y = rng.next_f64() * 256.0;
            let w = 2.0 + rng.next_f64() * 6.0;
            let h = 1.0 + rng.next_f64() * 3.0;
            g.fill_rect(x, y, w, h);
        }
        let mut grd = g.create_linear_gradient(0.0, 0.0, 128.0, 0.0);
        grd.add_color_stop(0.0, "rgba(60,45,30,0.5)");
        grd.add_color_stop(0.3, "rgba(60,45,30,0)");
        g.set_fill_style(&grd);
        g.fill_rect(0.0, 0.0, 128.0, 256.0);
        Cached::One(to_texture(&g, true))
    })
}

/// Catch fence: diamond wire mesh with a top and bottom cable (alpha).
/// (`t.generateMipmaps = true` is three's default.)
pub fn fence_texture(cache: &mut TextureCache) -> Arc<Cached> {
    cache.cached_with("raceway:fence", || {
        let mut g = Canvas::new(128, 128);
        g.clear_rect(0.0, 0.0, 128.0, 128.0);
        g.set_stroke_style("rgba(190,196,200,0.95)");
        g.set_line_width(1.6);
        let mut k = -128.0;
        while k < 256.0 {
            g.begin_path();
            g.move_to(k, 0.0);
            g.line_to(k + 128.0, 128.0);
            g.stroke();
            g.begin_path();
            g.move_to(k + 128.0, 0.0);
            g.line_to(k, 128.0);
            g.stroke();
            k += 16.0;
        }
        g.set_fill_style("rgba(150,155,160,1)");
        g.fill_rect(0.0, 0.0, 128.0, 3.0);
        g.fill_rect(0.0, 125.0, 128.0, 3.0);
        Cached::One(to_texture(&g, true))
    })
}

/// A crowd seen from the track: rows of heads and shirts in every colour,
/// with gaps. u along the row, v up the tiers (one tier per 32 px).
pub fn crowd_texture(cache: &mut TextureCache) -> Arc<Cached> {
    cache.cached_with("raceway:crowd", || {
        const W: f64 = 512.0;
        const H: f64 = 256.0;
        let mut g = Canvas::new(W as u32, H as u32);
        g.set_fill_style("#6c6f73");
        g.fill_rect(0.0, 0.0, W, H);
        let mut rng = Mulberry32::new(31);
        let shirts = [
            "#d8423a", "#f2f0ea", "#2f5fb8", "#f4c542", "#2c2c30", "#3fa35a", "#e8742a", "#8a4fc2",
            "#b8d4ea", "#c8b08a",
        ];
        let skin = ["#f0c8a0", "#d9a57a", "#a86e48", "#6e4630", "#f4d6b8"];
        let pick = |rng: &mut Mulberry32, arr: &[&'static str]| -> &'static str {
            arr[(rng.next_f64() * arr.len() as f64).floor() as usize]
        };
        for row in 0..(H / 32.0) as u32 {
            let row = f64::from(row);
            // Seat back.
            g.set_fill_style("#4f6fa0");
            g.fill_rect(0.0, row * 32.0 + 26.0, W, 4.0);
            let mut x = 2.0;
            while x < W {
                // `continue` still runs the loop's update, which draws.
                'seat: {
                    if rng.next_f64() < 0.18 {
                        break 'seat; // empty seat
                    }
                    let y = row * 32.0 + 8.0 + rng.next_f64() * 3.0;
                    g.set_fill_style(pick(&mut rng, &shirts));
                    g.fill_rect(x, y + 7.0, 7.0, 12.0);
                    let head = if rng.next_f64() < 0.2 {
                        "#222"
                    } else {
                        pick(&mut rng, &skin)
                    };
                    g.set_fill_style(head);
                    g.begin_path();
                    g.arc(x + 3.5, y + 4.0, 3.4, 0.0, PI * 2.0, false);
                    g.fill();
                    if rng.next_f64() < 0.15 {
                        // cap
                        g.set_fill_style(pick(&mut rng, &shirts));
                        g.fill_rect(x - 1.0, y - 1.0, 9.0, 3.0);
                    }
                }
                x += 9.0 + rng.next_f64() * 3.0;
            }
        }
        Cached::One(to_texture(&g, true))
    })
}

/// One panel of the banner atlas (`BANNERS`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Banner {
    /// `{ name, bg, fg, text, sub?, checker? }`.
    Sign {
        name: &'static str,
        bg: &'static str,
        fg: &'static str,
        text: &'static str,
        sub: Option<&'static str>,
        checker: bool,
    },
    /// `{ name, board }`: a braking board.
    Board {
        name: &'static str,
        board: &'static str,
    },
}

impl Banner {
    pub fn name(&self) -> &'static str {
        match self {
            Banner::Sign { name, .. } | Banner::Board { name, .. } => name,
        }
    }
}

const fn sign(
    name: &'static str,
    bg: &'static str,
    fg: &'static str,
    text: &'static str,
) -> Banner {
    Banner::Sign {
        name,
        bg,
        fg,
        text,
        sub: None,
        checker: false,
    }
}

/// Banners and boards, all in one atlas.
pub const BANNERS: [Banner; 13] = [
    Banner::Sign {
        name: "seaside",
        bg: "#10325c",
        fg: "#f4f1e8",
        text: "SEASIDE RACEWAY",
        sub: Some("MONTEREY COUNTY"),
        checker: false,
    },
    sign("midnight", "#15121c", "#ff5a8a", "MIDNIGHT RACER"),
    sign("vento", "#d81e36", "#ffffff", "VENTO GT"),
    sign("kestrel", "#ee5a12", "#1a1a1a", "KESTREL RS"),
    sign("ion", "#dfe7ee", "#0e6f86", "ION ARC"),
    sign("stiletto", "#f2b705", "#141414", "STILETTO"),
    sign("brawler", "#1f4fd8", "#ffffff", "BRAWLER 69"),
    sign("tyres", "#1b1b1d", "#f2c230", "MERIDIAN TYRES"),
    sign("oil", "#0d6b3a", "#f4f1e8", "SEABRIGHT OIL"),
    Banner::Sign {
        name: "startfinish",
        bg: "#f4f1e8",
        fg: "#111111",
        text: "START · FINISH",
        sub: None,
        checker: true,
    },
    Banner::Board {
        name: "b150",
        board: "150",
    },
    Banner::Board {
        name: "b100",
        board: "100",
    },
    Banner::Board {
        name: "b50",
        board: "50",
    },
];

const PW: f64 = 512.0;
const PH: f64 = 96.0;
const COLS: usize = 2;

/// The atlas's size in pixels.
fn atlas_size() -> (f64, f64) {
    let rows = BANNERS.len().div_ceil(COLS);
    (PW * COLS as f64, PH * rows as f64)
}

/// `bannerAtlas().rect(name)`: `[u0, v0, u1, v1]` of a panel.
pub fn banner_rect(name: &str) -> [f64; 4] {
    let i = BANNERS
        .iter()
        .position(|b| b.name() == name)
        .unwrap_or_else(|| panic!("no banner {name}"));
    let (cw, ch) = atlas_size();
    let x = (i % COLS) as f64 * PW;
    let y = (i / COLS) as f64 * PH;
    [x / cw, 1.0 - (y + PH) / ch, (x + PW) / cw, 1.0 - y / ch]
}

/// `bannerAtlas().texture`: the picture (not repeated).
pub fn banner_atlas(cache: &mut TextureCache) -> Arc<Cached> {
    cache.cached_with("raceway:banners", || {
        let (cw, ch) = atlas_size();
        let mut g = Canvas::new(cw as u32, ch as u32);
        for (i, b) in BANNERS.iter().enumerate() {
            let x = (i % COLS) as f64 * PW;
            let y = (i / COLS) as f64 * PH;
            g.save();
            g.begin_path();
            g.rect(x, y, PW, PH);
            g.clip();
            match *b {
                Banner::Board { board, .. } => {
                    // Braking board: black numerals on white with a red frame.
                    g.set_fill_style("#f4f2ec");
                    g.fill_rect(x, y, PW, PH);
                    g.set_fill_style("#c8281e");
                    g.fill_rect(x, y, PW, 10.0);
                    g.fill_rect(x, y + PH - 10.0, PW, 10.0);
                    g.set_fill_style("#111");
                    g.set_font("bold 76px \"Arial Black\", Arial, sans-serif");
                    g.set_text_align("center");
                    g.set_text_baseline("middle");
                    g.fill_text(board, x + PW / 2.0, y + PH / 2.0 + 3.0);
                }
                Banner::Sign {
                    bg,
                    fg,
                    text,
                    sub,
                    checker,
                    ..
                } => {
                    g.set_fill_style(bg);
                    g.fill_rect(x, y, PW, PH);
                    if checker {
                        for k in 0..8 {
                            for j in 0..3 {
                                let (kf, jf) = (f64::from(k), f64::from(j));
                                g.set_fill_style(if (k + j) % 2 == 1 { "#111" } else { "#f4f1e8" });
                                g.fill_rect(x + kf * 12.0, y + 12.0 + jf * 24.0, 12.0, 24.0);
                                g.fill_rect(
                                    x + PW - 96.0 + kf * 12.0,
                                    y + 12.0 + jf * 24.0,
                                    12.0,
                                    24.0,
                                );
                            }
                        }
                    }
                    g.set_fill_style(fg);
                    g.set_text_align("center");
                    g.set_text_baseline("middle");
                    let px = if sub.is_some() { 50 } else { 62 };
                    g.set_font(&format!("bold {px}px \"Arial Black\", Arial, sans-serif"));
                    let ty = y + if sub.is_some() {
                        PH * 0.4
                    } else {
                        PH / 2.0 + 3.0
                    };
                    g.fill_text_max(
                        text,
                        x + PW / 2.0,
                        ty,
                        PW - if checker { 210.0 } else { 30.0 },
                    );
                    if let Some(sub) = sub {
                        g.set_font("bold 22px Arial, sans-serif");
                        g.fill_text(sub, x + PW / 2.0, y + PH * 0.8);
                    }
                }
            }
            g.restore();
        }
        Cached::One(to_texture(&g, false))
    })
}
