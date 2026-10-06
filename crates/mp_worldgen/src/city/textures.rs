//! Port of `src/world/city/cityTextures.js` (roadmap WP 3.8): city-only
//! canvas textures: the building atlas, billboard ads, tunnel tiles and
//! sound-wall panels, the night-city façade atlas and its material patch.
//!
//! The JS module keeps its pictures in module variables (`atlas`,
//! `cityAtlas`, `adCache`, ...), made once per page; here they live in the
//! world build's [`TextureCache`] under keys of their own (`city:...`), so
//! every call hands out the same picture as the JS does. `bannerTexture` is
//! not cached in the JS, and is not here.

// Index loops stay index loops (DECISIONS D52).
#![allow(clippy::needless_range_loop)]

use std::sync::Arc;

use mp_canvas::{Canvas, ImageData};
use mp_math::{Mulberry32, js};
use mp_scene::MaterialKind;
use serde_json::Value;

use crate::material::{Material, num, texture_value};
use crate::object::TextureId;
use crate::textures::{Cached, Texture, TextureCache};

pub const CELLS: usize = 10;
/// Metres covered by one repeat of each atlas cell [width, height]. Chosen so
/// windows come out ~3 m wide and ~3.6 m per floor on every variant.
pub const CELL_TILE: [[f64; 2]; CELLS] = [
    [24.0, 72.0],
    [21.0, 60.8],
    [28.0, 93.6],
    [18.0, 50.4],
    [31.2, 105.0],
    [16.0, 38.0],
    [16.0, 16.0], // 6: roof gravel
    [24.0, 48.0], // 7: dark curtain-wall glass (crowns, podium glazing)
    [24.0, 48.0], // 8: corrugated warehouse siding with roll-up doors (bottom-aligned)
    [18.0, 36.0], // 9: brick apartments
];
pub const CELL_COLS: [f64; CELLS] = [8.0, 6.0, 10.0, 5.0, 12.0, 4.0, 1.0, 8.0, 4.0, 6.0];
pub const CELL_ROWS: [f64; CELLS] = [20.0, 16.0, 26.0, 14.0, 30.0, 10.0, 1.0, 12.0, 1.0, 12.0];
pub const ROOF_CELL: usize = 6;
pub const GLASS_CELL: usize = 7;
pub const WAREHOUSE_CELL: usize = 8;
pub const BRICK_CELL: usize = 9;

fn canvas(w: u32, h: u32) -> Canvas {
    Canvas::new(w, h)
}

/// `tex(c, { srgb = true, repeat = false })`: a CanvasTexture, anisotropy 8.
fn tex(c: &Canvas, srgb: bool, repeat: bool) -> Texture {
    Texture::from_canvas(c, repeat, srgb, 8.0)
}

/// A texture's picture as a canvas, to `drawImage` from (the pictures drawn
/// here are opaque, so the round trip through premultiplied pixels is exact).
pub fn canvas_of(t: &Texture) -> Canvas {
    let mut c = canvas(t.width, t.height);
    let img = ImageData {
        width: t.width,
        height: t.height,
        data: t.rgba.clone(),
    };
    c.put_image_data(&img, 0, 0);
    c
}

/// A JS number in a template string (`${v}`): the shortest round trip, as
/// Rust's `Display` writes the magnitudes in use here.
fn js_num(v: f64) -> String {
    format!("{v}")
}

/// `buildingAtlas()`: the classic atlas, `{ map, emissive }` (cached as a
/// façade pair: `Layer::Main` is the map, `Layer::Emissive` the emissive).
pub fn building_atlas(cache: &mut TextureCache) -> Arc<Cached> {
    if let Some(a) = cache.lookup("city:buildingAtlas") {
        return a;
    }
    const W: f64 = 256.0;
    const H: f64 = 512.0;
    let mut g = canvas((W as usize * CELLS) as u32, H as u32);
    let mut ge = canvas((W as usize * CELLS) as u32, H as u32);
    // A faint glow in the wall itself so buildings read as shapes at night,
    // not just floating windows.
    ge.set_fill_style("#0d0e12");
    ge.fill_rect(0.0, 0.0, W * CELLS as f64, H);
    let mut rng = Mulberry32::new(77);
    for v in 0..6usize {
        let f = cache.facade_textures(v as u32);
        let Cached::Facade { map, emissive, .. } = &*f else {
            unreachable!("facadeTextures returns a façade")
        };
        g.draw_image(&canvas_of(map), v as f64 * W, 0.0, W, H);
        ge.set_global_composite_operation("lighten");
        ge.draw_image(&canvas_of(emissive), v as f64 * W, 0.0, W, H);
        ge.set_global_composite_operation("source-over");
        // Switch off ~40% of the lit windows: late at night most offices are dark.
        let cw = W / CELL_COLS[v];
        let rh = H / CELL_ROWS[v];
        for r in 0..CELL_ROWS[v] as usize {
            let floor_off = rng.next_f64() < 0.25;
            for q in 0..CELL_COLS[v] as usize {
                if floor_off || rng.next_f64() < 0.3 {
                    ge.set_fill_style("#0d0e12");
                    ge.fill_rect(v as f64 * W + q as f64 * cw, r as f64 * rh, cw, rh);
                }
            }
        }
    }
    // Roof: tar and gravel with patches.
    g.set_fill_style("#4a4a4c");
    g.fill_rect(6.0 * W, 0.0, W, H);
    for _ in 0..3000 {
        let v = 50.0 + rng.next_f64() * 60.0;
        g.set_fill_style(format!(
            "rgb({},{},{})",
            js_num(v),
            js_num(v),
            js_num(v + 3.0)
        ));
        let x = 6.0 * W + rng.next_f64() * W;
        let y = rng.next_f64() * H;
        g.fill_rect(x, y, 2.0, 2.0);
    }
    for _ in 0..14 {
        let c = if rng.next_f64() < 0.5 {
            "30,30,32"
        } else {
            "110,110,108"
        };
        g.set_fill_style(format!("rgba({c},0.25)"));
        let x = 6.0 * W + rng.next_f64() * W;
        let y = rng.next_f64() * H;
        let w = 20.0 + rng.next_f64() * 80.0;
        let h = 20.0 + rng.next_f64() * 80.0;
        g.fill_rect(x, y, w, h);
    }
    // Glass curtain wall: blue-black with mullions and a sky gradient.
    let mut grd = g.create_linear_gradient(0.0, 0.0, 0.0, H);
    grd.add_color_stop(0.0, "#2a3a52");
    grd.add_color_stop(1.0, "#10161f");
    g.set_fill_style(&grd);
    g.fill_rect(7.0 * W, 0.0, W, H);
    g.set_fill_style("rgba(160,190,220,0.18)");
    for x in 0..8 {
        g.fill_rect(7.0 * W + x as f64 * (W / 8.0), 0.0, 2.0, H);
    }
    for y in 0..12 {
        g.fill_rect(7.0 * W, y as f64 * (H / 12.0), W, 2.0);
    }
    // A few lit panes in the glass so crowns aren't dead at night.
    for _ in 0..18 {
        let x = (rng.next_f64() * 8.0).floor();
        let y = (rng.next_f64() * 12.0).floor();
        ge.set_fill_style(if rng.next_f64() < 0.5 {
            "rgba(170,210,255,0.8)"
        } else {
            "rgba(255,220,160,0.7)"
        });
        ge.fill_rect(
            7.0 * W + x * (W / 8.0) + 3.0,
            y * (H / 12.0) + 3.0,
            W / 8.0 - 6.0,
            H / 12.0 - 6.0,
        );
    }
    // Warehouse: corrugated metal, roll-up doors along the bottom, a band of
    // high windows. 10.7 px per metre; the bottom of the canvas is ground level.
    {
        let x0 = 8.0 * W;
        let pxm = W / 24.0;
        g.set_fill_style("#4a5058");
        g.fill_rect(x0, 0.0, W, H);
        let mut x = 0.0;
        while x < W {
            g.set_fill_style(if x % 8.0 != 0.0 {
                "rgba(255,255,255,0.07)"
            } else {
                "rgba(0,0,0,0.18)"
            });
            g.fill_rect(x0 + x, 0.0, 2.0, H);
            x += 4.0;
        }
        for _ in 0..10 {
            let c = if rng.next_f64() < 0.5 {
                "120,70,40"
            } else {
                "30,30,34"
            };
            g.set_fill_style(format!("rgba({c},0.18)"));
            let x = x0 + rng.next_f64() * W;
            let y = rng.next_f64() * H;
            let w = 10.0 + rng.next_f64() * 40.0;
            let h = 30.0 + rng.next_f64() * 120.0;
            g.fill_rect(x, y, w, h);
        }
        for d in 0..4 {
            let dx = x0 + d as f64 * 6.0 * pxm + 1.2 * pxm;
            let dw = 3.6 * pxm;
            let dh = 4.2 * pxm;
            g.set_fill_style("#2c3036");
            g.fill_rect(dx, H - dh, dw, dh);
            let mut y = H - dh;
            while y < H {
                g.set_fill_style("rgba(255,255,255,0.08)");
                g.fill_rect(dx, y, dw, 1.0);
                y += 5.0;
            }
            // Light spilling under half-open doors, and a lamp above each.
            if rng.next_f64() < 0.6 {
                ge.set_fill_style("#ffcf80");
                ge.fill_rect(dx, H - 0.45 * pxm, dw, 0.45 * pxm);
            }
            ge.set_fill_style("#fff0c8");
            ge.fill_rect(dx + dw / 2.0 - 5.0, H - dh - 0.9 * pxm, 10.0, 6.0);
            g.set_fill_style("#d8d0b8");
            g.fill_rect(dx + dw / 2.0 - 5.0, H - dh - 0.9 * pxm, 10.0, 6.0);
        }
        let wy = H - 7.2 * pxm;
        for q in 0..8 {
            let wx = x0 + q as f64 * 3.0 * pxm + 0.4 * pxm;
            g.set_fill_style("#1a2230");
            g.fill_rect(wx, wy, 2.2 * pxm, 0.9 * pxm);
            if rng.next_f64() < 0.35 {
                ge.set_fill_style(if rng.next_f64() < 0.5 {
                    "#bfe0ff"
                } else {
                    "#ffe2a8"
                });
                ge.fill_rect(wx, wy, 2.2 * pxm, 0.9 * pxm);
            }
        }
    }
    // Brick apartments: 6 × 12 windows of 3 m pitch.
    {
        let x0 = 9.0 * W;
        let cw = W / 6.0;
        let rh = H / 12.0;
        g.set_fill_style("#6b3b2e");
        g.fill_rect(x0, 0.0, W, H);
        for _ in 0..2600 {
            g.set_fill_style(if rng.next_f64() < 0.5 {
                "rgba(40,20,14,0.35)"
            } else {
                "rgba(160,110,90,0.25)"
            });
            let x = x0 + rng.next_f64() * W;
            let y = rng.next_f64() * H;
            g.fill_rect(x, y, 3.0, 1.5);
        }
        let mut y = 0.0;
        while y < H {
            g.set_fill_style("rgba(200,180,160,0.08)");
            g.fill_rect(x0, y, W, 1.0);
            y += 5.0;
        }
        for r in 0..12 {
            for q in 0..6 {
                let wx = x0 + q as f64 * cw + cw * 0.27;
                let wy = r as f64 * rh + rh * 0.22;
                let ww = cw * 0.46;
                let wh = rh * 0.56;
                g.set_fill_style("#1b1d22");
                g.fill_rect(wx, wy, ww, wh);
                g.set_fill_style("rgba(230,220,200,0.5)");
                g.fill_rect(wx - 2.0, wy + wh, ww + 4.0, 3.0);
                if rng.next_f64() < 0.42 {
                    let col = if rng.next_f64() < 0.8 {
                        ["#ffd28a", "#ffc070", "#ffe6b0"][(rng.next_f64() * 3.0).floor() as usize]
                    } else {
                        "#a8c8ff"
                    };
                    ge.set_fill_style(col);
                    ge.set_global_alpha(0.6 + rng.next_f64() * 0.4);
                    ge.fill_rect(wx, wy, ww, wh);
                    ge.set_global_alpha(1.0);
                    g.set_fill_style(col);
                    g.set_global_alpha(0.25);
                    g.fill_rect(wx, wy, ww, wh);
                    g.set_global_alpha(1.0);
                }
            }
        }
    }
    let map = tex(&g, true, false);
    let emissive = tex(&ge, true, false);
    cache.cached_with("city:buildingAtlas", || Cached::Facade {
        map,
        emissive,
        cols: 0,
        rows: 0,
    })
}

/// `patchAtlasMaterial(material)`: `uv` in cell-tile units (unbounded), the
/// `cell` attribute picking the atlas column; `textureGrad` keeps mip
/// selection continuous across the `fract()` wrap. (No caller in the game.)
pub fn patch_atlas_material(m: Material) -> Material {
    m.kind(MaterialKind::CityAtlas, None)
        .program_key("city-atlas")
        .uniform("clippingPlanes", Value::Null)
}

// Billboard / neon advertisement. All brands are invented.
struct Ad {
    t: &'static str,
    s: &'static str,
    bg: [&'static str; 2],
    fg: &'static str,
    fg2: &'static str,
}

const ADS: [Ad; 6] = [
    Ad {
        t: "NIGHTSHIFT",
        s: "ENERGY DRINK",
        bg: ["#12002a", "#3a0060"],
        fg: "#ff3cf0",
        fg2: "#7cf6ff",
    },
    Ad {
        t: "MERIDIAN MOTORS",
        s: "SINCE 1962 · EXIT 43",
        bg: ["#1a0a00", "#442000"],
        fg: "#ffb13c",
        fg2: "#fff2c0",
    },
    Ad {
        t: "NEON NOODLE",
        s: "OPEN ALL NIGHT",
        bg: ["#001a18", "#004a40"],
        fg: "#35ffc9",
        fg2: "#ff5a7a",
    },
    Ad {
        t: "CRUISE FM 101.9",
        s: "THE SOUND OF THE CITY",
        bg: ["#07072a", "#1a1a66"],
        fg: "#8fb0ff",
        fg2: "#ffd84a",
    },
    Ad {
        t: "VELOCITY TYRES",
        s: "GRIP THE NIGHT",
        bg: ["#1a0000", "#520010"],
        fg: "#ff4050",
        fg2: "#ffffff",
    },
    Ad {
        t: "SKYLINE HOTEL",
        s: "ROOFTOP BAR · 48TH FLOOR",
        bg: ["#02101c", "#0a3050"],
        fg: "#62d6ff",
        fg2: "#ffe0a0",
    },
];
pub const AD_COUNT: usize = ADS.len();

/// `adTexture(i)` (cached per ad).
pub fn ad_texture(cache: &mut TextureCache, i: i64) -> Arc<Cached> {
    let n = ADS.len() as i64;
    let i = ((i % n) + n) % n;
    cache.cached_with(&format!("city:ad{i}"), || {
        let a = &ADS[i as usize];
        const W: f64 = 1024.0;
        const H: f64 = 384.0;
        let mut g = canvas(W as u32, H as u32);
        let mut grd = g.create_linear_gradient(0.0, 0.0, W, H);
        grd.add_color_stop(0.0, a.bg[0]);
        grd.add_color_stop(1.0, a.bg[1]);
        g.set_fill_style(&grd);
        g.fill_rect(0.0, 0.0, W, H);
        // Diagonal stripes for some visual energy.
        g.set_global_alpha(0.08);
        g.set_fill_style(a.fg);
        let mut x = -H;
        while x < W {
            g.begin_path();
            g.move_to(x, H);
            g.line_to(x + H, 0.0);
            g.line_to(x + H + 24.0, 0.0);
            g.line_to(x + 24.0, H);
            g.fill();
            x += 60.0;
        }
        g.set_global_alpha(1.0);
        g.set_text_align("center");
        g.set_text_baseline("middle");
        g.set_font("italic 900 118px \"Arial Narrow\", Arial, sans-serif");
        g.set_shadow_color(a.fg);
        g.set_shadow_blur(28.0);
        g.set_fill_style(a.fg);
        g.fill_text_max(a.t, W / 2.0, H * 0.42, W * 0.92);
        g.set_shadow_blur(0.0);
        g.set_fill_style("#ffffff");
        g.set_global_alpha(0.9);
        g.fill_text_max(a.t, W / 2.0, H * 0.42, W * 0.92);
        g.set_global_alpha(1.0);
        g.set_font("bold 44px \"Arial Narrow\", Arial, sans-serif");
        g.set_fill_style(a.fg2);
        g.fill_text_max(a.s, W / 2.0, H * 0.78, W * 0.9);
        g.set_stroke_style(a.fg);
        g.set_line_width(10.0);
        g.stroke_rect(8.0, 8.0, W - 16.0, H - 16.0);
        Cached::One(tex(&g, true, false))
    })
}

/// `tunnelTileTexture()`: cream ceramic tiles with thin grout, as in most
/// city road tunnels.
pub fn tunnel_tile_texture(cache: &mut TextureCache) -> Arc<Cached> {
    cache.cached_with("city:tunnelTiles", || {
        const S: f64 = 256.0;
        let mut g = canvas(S as u32, S as u32);
        g.set_fill_style("#8f8a80");
        g.fill_rect(0.0, 0.0, S, S);
        let mut rng = Mulberry32::new(5);
        for y in 0..8 {
            for x in 0..8 {
                let v = 226.0 + rng.next_f64() * 14.0;
                g.set_fill_style(format!(
                    "rgb({},{},{})",
                    js_num(v),
                    js_num(v - 3.0),
                    js_num(v - 10.0)
                ));
                g.fill_rect(x as f64 * 32.0 + 1.5, y as f64 * 32.0 + 1.5, 29.0, 29.0);
                g.set_fill_style("rgba(255,255,255,0.10)");
                g.fill_rect(x as f64 * 32.0 + 2.0, y as f64 * 32.0 + 2.0, 27.0, 6.0);
            }
        }
        Cached::One(tex(&g, true, true))
    })
}

/// `soundWallTexture()`.
pub fn sound_wall_texture(cache: &mut TextureCache) -> Arc<Cached> {
    cache.cached_with("city:soundWall", || {
        const W: usize = 256;
        const H: usize = 256;
        let (w, h) = (W as f64, H as f64);
        let mut g = canvas(W as u32, H as u32);
        g.set_fill_style("#9d988c");
        g.fill_rect(0.0, 0.0, w, h);
        let mut rng = Mulberry32::new(9);
        let mut img = g.get_image_data(0, 0, W as u32, H as u32);
        for i in 0..W * H {
            let n = (rng.next_f64() - 0.5) * 18.0;
            for k in 0..3 {
                let v = img.data[i * 4 + k] as f64 + n;
                img.set(i * 4 + k, v);
            }
        }
        g.put_image_data(&img, 0, 0);
        // Panel joints and vertical ribs.
        g.set_fill_style("rgba(40,38,34,0.55)");
        g.fill_rect(0.0, 0.0, 4.0, h);
        let mut x = 16.0;
        while x < w {
            g.set_fill_style("rgba(255,255,255,0.10)");
            g.fill_rect(x, 0.0, 3.0, h);
            g.set_fill_style("rgba(0,0,0,0.14)");
            g.fill_rect(x + 3.0, 0.0, 3.0, h);
            x += 20.0;
        }
        g.set_fill_style("rgba(30,28,24,0.4)");
        g.fill_rect(0.0, h - 22.0, w, 22.0);
        Cached::One(tex(&g, true, true))
    })
}

/// `parkTexture()`: grass for the tunnel lid.
pub fn park_texture(cache: &mut TextureCache) -> Arc<Cached> {
    cache.cached_with("city:park", || {
        const S: f64 = 256.0;
        let mut g = canvas(S as u32, S as u32);
        g.set_fill_style("#3f6a2e");
        g.fill_rect(0.0, 0.0, S, S);
        let mut rng = Mulberry32::new(12);
        for _ in 0..4000 {
            let v = rng.next_f64();
            g.set_fill_style(if v < 0.5 {
                "rgba(80,120,50,0.5)"
            } else {
                "rgba(40,70,28,0.5)"
            });
            let x = rng.next_f64() * S;
            let y = rng.next_f64() * S;
            g.fill_rect(x, y, 2.0, 3.0);
        }
        Cached::One(tex(&g, true, true))
    })
}

/// `bannerTexture`'s options, with the JS defaults.
#[derive(Clone, Debug)]
pub struct BannerOpts<'a> {
    pub w: u32,
    pub h: u32,
    pub bg: &'a str,
    pub fg: &'a str,
    pub checker: bool,
    pub font: &'a str,
}

impl Default for BannerOpts<'_> {
    fn default() -> Self {
        BannerOpts {
            w: 1024,
            h: 192,
            bg: "#101014",
            fg: "#f4f4f4",
            checker: false,
            font: "italic 900 120px \"Arial Narrow\", Arial, sans-serif",
        }
    }
}

/// `bannerTexture(text, opts)`: the tunnel name / portal sign and the
/// finish banner (a new texture every call).
pub fn banner_texture(text: &str, o: &BannerOpts) -> Texture {
    let (w, h) = (o.w as f64, o.h as f64);
    let mut g = canvas(o.w, o.h);
    g.set_fill_style(o.bg);
    g.fill_rect(0.0, 0.0, w, h);
    if o.checker {
        let sq = h / 4.0;
        for y in 0..4 {
            let mut x = 0.0;
            while x < w / sq {
                if y != 1 && y != 2 {
                    g.set_fill_style(if (x as i64 + y) % 2 != 0 {
                        "#111"
                    } else {
                        "#f2f2f2"
                    });
                    g.fill_rect(x * sq, y as f64 * sq, sq, sq);
                }
                x += 1.0;
            }
        }
        g.set_fill_style("#111");
        g.fill_rect(0.0, sq, w, sq * 2.0);
    }
    g.set_fill_style(o.fg);
    g.set_text_align("center");
    g.set_text_baseline("middle");
    g.set_font(o.font);
    g.fill_text_max(text, w / 2.0, h / 2.0 + 4.0, w * 0.9);
    tex(&g, true, false)
}

// ── Night-city façades (City.js) ──────────────────────────────
// A second atlas with the same cell layout as buildingAtlas(), drawn with
// every window unlit, plus a glass mask. patchCityMaterial() then decides
// per window, in the shader, which rooms have their lights on, their colour
// temperature and blinds, reflects the city glow in the dark glass and turns
// the ground floor into shopfronts — so each building gets its own pattern
// instead of the atlas repeating the same lit windows every few floors.
struct Facade {
    wall: &'static str,
    glass: &'static str,
    frame: &'static str,
    /// Glass fraction of the bay's width.
    gx: f64,
    /// Top/bottom spandrel fractions.
    gt: f64,
    gb: f64,
    panes: u32,
    band: Option<&'static str>,
    sill: Option<&'static str>,
    balcony: bool,
    brick: bool,
}

const fn facade(
    wall: &'static str,
    glass: &'static str,
    frame: &'static str,
    gx: f64,
    gt: f64,
    gb: f64,
    panes: u32,
) -> Facade {
    Facade {
        wall,
        glass,
        frame,
        gx,
        gt,
        gb,
        panes,
        band: None,
        sill: None,
        balcony: false,
        brick: false,
    }
}

/// `Object.entries(FACADE)`: integer keys, so in ascending order.
const FACADE: [(usize, Facade); 7] = [
    (
        0,
        Facade {
            band: Some("rgba(20,22,26,0.35)"),
            ..facade("#595e66", "#16202b", "#2c3036", 0.84, 0.2, 0.12, 2)
        },
    ),
    (
        1,
        Facade {
            sill: Some("rgba(225,215,195,0.45)"),
            ..facade("#7d7163", "#1a1f26", "#3b352f", 0.62, 0.3, 0.16, 2)
        },
    ),
    (
        2,
        facade("#27313e", "#172434", "#3a4656", 0.94, 0.24, 0.06, 1),
    ),
    (
        3,
        Facade {
            balcony: true,
            ..facade("#877a68", "#1c2027", "#4a4238", 0.56, 0.3, 0.12, 2)
        },
    ),
    (
        4,
        facade("#2f3743", "#121a26", "#46525f", 0.96, 0.14, 0.04, 1),
    ),
    (
        5,
        Facade {
            sill: Some("rgba(235,228,210,0.4)"),
            ..facade("#8e8474", "#1d2026", "#4c463c", 0.58, 0.3, 0.2, 2)
        },
    ),
    (
        9,
        Facade {
            brick: true,
            ..facade("#6b3b2e", "#1b1d22", "#d8cfc0", 0.46, 0.22, 0.22, 2)
        },
    ),
];

/// Warm-light bias per cell (0 = cool offices … 1 = warm homes) for the shader.
pub const CELL_WARM: [f64; CELLS] = [0.35, 0.55, 0.2, 0.8, 0.15, 0.75, 0.5, 0.25, 0.6, 0.85];

/// `cityFacadeAtlas()`'s three pictures.
pub struct CityAtlas {
    pub map: Arc<Cached>,
    pub emissive: Arc<Cached>,
    /// Not sRGB (`NoColorSpace`).
    pub mask: Arc<Cached>,
}

/// `cityFacadeAtlas()`.
pub fn city_facade_atlas(cache: &mut TextureCache) -> CityAtlas {
    if let (Some(map), Some(emissive), Some(mask)) = (
        cache.lookup("city:facade:map"),
        cache.lookup("city:facade:emissive"),
        cache.lookup("city:facade:mask"),
    ) {
        return CityAtlas {
            map,
            emissive,
            mask,
        };
    }
    const W: f64 = 256.0;
    const H: f64 = 512.0;
    let cw_px = (W as usize * CELLS) as u32;
    let mut g = canvas(cw_px, H as u32);
    let mut ge = canvas(cw_px, H as u32);
    let mut gm = canvas(cw_px, H as u32);
    let mut rng = Mulberry32::new(3131);
    // Roof and warehouse cells come straight from the classic atlas.
    let old = building_atlas(cache);
    {
        let Cached::Facade { map, emissive, .. } = &*old else {
            unreachable!("the building atlas is a pair")
        };
        let (om, oe) = (canvas_of(map), canvas_of(emissive));
        for k in [ROOF_CELL, WAREHOUSE_CELL] {
            let x = k as f64 * W;
            g.draw_image_sub(&om, x, 0.0, W, H, x, 0.0, W, H);
            ge.draw_image_sub(&oe, x, 0.0, W, H, x, 0.0, W, H);
        }
    }
    gm.set_fill_style("#000");
    gm.fill_rect(0.0, 0.0, W * CELLS as f64, H);
    for (k, st) in &FACADE {
        let k = *k;
        let x0 = k as f64 * W;
        let cols = CELL_COLS[k];
        let rows = CELL_ROWS[k];
        let bw = W / cols;
        let fh = H / rows;
        ge.set_fill_style("#000");
        ge.fill_rect(x0, 0.0, W, H);
        g.set_fill_style(st.wall);
        g.fill_rect(x0, 0.0, W, H);
        if st.brick {
            for _ in 0..2600 {
                g.set_fill_style(if rng.next_f64() < 0.5 {
                    "rgba(40,20,14,0.35)"
                } else {
                    "rgba(160,110,90,0.25)"
                });
                let x = x0 + rng.next_f64() * W;
                let y = rng.next_f64() * H;
                g.fill_rect(x, y, 3.0, 1.5);
            }
            let mut y = 0.0;
            while y < H {
                g.set_fill_style("rgba(200,180,160,0.08)");
                g.fill_rect(x0, y, W, 1.0);
                y += 5.0;
            }
        } else {
            // Panel-to-panel tone variation and rain streaks.
            for r in 0..rows as usize {
                for q in 0..cols as usize {
                    let c = if rng.next_f64() < 0.5 {
                        "255,255,255"
                    } else {
                        "0,0,0"
                    };
                    let a = 0.02 + rng.next_f64() * 0.04;
                    g.set_fill_style(format!("rgba({c},{})", js_num(a)));
                    g.fill_rect(x0 + q as f64 * bw, r as f64 * fh, bw, fh);
                }
            }
            for _ in 0..40 {
                g.set_fill_style("rgba(0,0,0,0.06)");
                let x = x0 + rng.next_f64() * W;
                let y = rng.next_f64() * H;
                let w = 1.0 + rng.next_f64() * 2.0;
                let h = 20.0 + rng.next_f64() * 120.0;
                g.fill_rect(x, y, w, h);
            }
        }
        for r in 0..rows as usize {
            let y = r as f64 * fh;
            // The emissive map keeps a baked lit pattern: the shader fades to it
            // once windows are too small to light one by one, so distant towers
            // stay speckled (and mip-filtered) instead of turning into grey slabs.
            let fr = rng.next_f64();
            let floor_lit = if fr < 0.25 {
                0.0
            } else if fr > 0.92 {
                0.9
            } else {
                0.3
            };
            // Spandrel band / floor slab line.
            if let Some(band) = st.band {
                g.set_fill_style(band);
                g.fill_rect(x0, y + fh * (1.0 - st.gb) - 1.0, W, fh * st.gb + 1.0);
            }
            if st.balcony {
                g.set_fill_style("rgba(220,210,190,0.45)");
                g.fill_rect(x0, y + fh - 3.0, W, 3.0);
                g.set_fill_style("rgba(30,30,32,0.5)");
                g.fill_rect(x0, y + fh * 0.62, W, 1.0);
            }
            for q in 0..cols as usize {
                let wx = x0 + q as f64 * bw + bw * (1.0 - st.gx) / 2.0;
                let ww = bw * st.gx;
                let wy = y + fh * st.gt;
                let wh = fh * (1.0 - st.gt - st.gb);
                // Frame, then glass inset one pixel, with a sky sheen at the top.
                g.set_fill_style(st.frame);
                g.fill_rect(wx - 1.0, wy - 1.0, ww + 2.0, wh + 2.0);
                let mut grd = g.create_linear_gradient(0.0, wy, 0.0, wy + wh);
                grd.add_color_stop(0.0, &shade(st.glass, 1.6));
                grd.add_color_stop(0.45, st.glass);
                grd.add_color_stop(1.0, &shade(st.glass, 0.8));
                g.set_fill_style(&grd);
                g.fill_rect(wx, wy, ww, wh);
                gm.set_fill_style("#fff");
                gm.fill_rect(wx, wy, ww, wh);
                if rng.next_f64() < floor_lit {
                    let col = if rng.next_f64() < 0.6 {
                        ["#ffd28f", "#ffe6bd", "#ffc27a"][(rng.next_f64() * 3.0).floor() as usize]
                    } else {
                        ["#cfe4ff", "#e6f2ff"][(rng.next_f64() * 2.0).floor() as usize]
                    };
                    ge.set_fill_style(col);
                    ge.set_global_alpha(0.5 + rng.next_f64() * 0.4);
                    ge.fill_rect(wx, wy, ww, wh);
                    ge.set_global_alpha(1.0);
                }
                // Mullions between panes.
                g.set_fill_style(st.frame);
                gm.set_fill_style("#000");
                for p in 1..st.panes {
                    let mx = wx + (ww * p as f64) / st.panes as f64 - 1.0;
                    g.fill_rect(mx, wy, 2.0, wh);
                    gm.fill_rect(mx, wy, 2.0, wh);
                }
                if let Some(sill) = st.sill {
                    g.set_fill_style(sill);
                    g.fill_rect(wx - 2.0, wy + wh, ww + 4.0, 3.0);
                    g.fill_rect(wx - 1.0, wy - 3.0, ww + 2.0, 2.0);
                }
                if st.brick {
                    g.set_fill_style("rgba(230,220,200,0.5)");
                    g.fill_rect(wx - 2.0, wy + wh, ww + 4.0, 3.0);
                }
                if !st.brick && st.gx < 0.8 {
                    // Reveal shadow on the upper and left edges of punched windows.
                    g.set_fill_style("rgba(0,0,0,0.35)");
                    g.fill_rect(wx, wy, ww, 2.0);
                    g.fill_rect(wx, wy, 2.0, wh);
                }
            }
        }
        // Curtain walls: continuous vertical mullions over the spandrels.
        if st.gx > 0.9 {
            g.set_fill_style(shade(st.frame, 1.2));
            for q in 0..=cols as usize {
                g.fill_rect(x0 + q as f64 * bw - 1.0, 0.0, 2.0, H);
            }
        }
    }
    // Curtain-wall glass (crowns, podium glazing): blue-black with a sky
    // gradient; every pane is glass.
    {
        let x0 = GLASS_CELL as f64 * W;
        let cols = CELL_COLS[GLASS_CELL];
        let rows = CELL_ROWS[GLASS_CELL];
        let mut grd = g.create_linear_gradient(0.0, 0.0, 0.0, H);
        grd.add_color_stop(0.0, "#2a3a52");
        grd.add_color_stop(1.0, "#10161f");
        g.set_fill_style(&grd);
        g.fill_rect(x0, 0.0, W, H);
        ge.set_fill_style("#000");
        ge.fill_rect(x0, 0.0, W, H);
        for _ in 0..20 {
            ge.set_fill_style(if rng.next_f64() < 0.5 {
                "#aac8f0"
            } else {
                "#f0d8a8"
            });
            let x = x0 + (rng.next_f64() * cols).floor() * (W / cols) + 2.0;
            let y = (rng.next_f64() * rows).floor() * (H / rows) + 3.0;
            ge.fill_rect(x, y, W / cols - 4.0, H / rows - 5.0);
        }
        gm.set_fill_style("#fff");
        gm.fill_rect(x0, 0.0, W, H);
        g.set_fill_style("rgba(160,190,220,0.22)");
        gm.set_fill_style("#000");
        for x in 0..cols as usize {
            g.fill_rect(x0 + x as f64 * (W / cols), 0.0, 2.0, H);
            gm.fill_rect(x0 + x as f64 * (W / cols), 0.0, 2.0, H);
        }
        for y in 0..rows as usize {
            g.fill_rect(x0, y as f64 * (H / rows), W, 2.0);
            gm.fill_rect(x0, y as f64 * (H / rows), W, 3.0);
        }
    }
    let map = cache.cached_with("city:facade:map", || Cached::One(tex(&g, true, false)));
    let emissive = cache.cached_with("city:facade:emissive", || {
        Cached::One(tex(&ge, true, false))
    });
    let mask = cache.cached_with("city:facade:mask", || Cached::One(tex(&gm, false, false)));
    CityAtlas {
        map,
        emissive,
        mask,
    }
}

/// `shade(hex, k)`: a `#rrggbb` colour scaled, as an `rgb()` string.
fn shade(hex: &str, k: f64) -> String {
    let n = u32::from_str_radix(&hex[1..], 16).expect("a hex colour");
    let f = |v: u32| js::max(0.0, js::min(255.0, js::round(v as f64 * k)));
    format!(
        "rgb({},{},{})",
        js_num(f(n >> 16)),
        js_num(f((n >> 8) & 255)),
        js_num(f(n & 255))
    )
}

/// Pack a per-building random seed into the `cell` attribute: the classic
/// patch reads floor(cell + 0.5), so the fraction is free. The seed is
/// quantised to 64 levels and stored mid-bin so interpolation noise can't
/// flip it between pixels (the shader hash would turn that into sparkle).
pub fn cell_seed(cell: usize, seed: f64) -> f64 {
    cell as f64 - 0.4 + (((seed * 64.0).floor() + 0.5) / 64.0) * 0.8
}

/// `patchCityMaterial(material, { mask, ground })`: like
/// `patchAtlasMaterial`, for `cityFacadeAtlas()`: lit windows, glass
/// reflections, street-level shopfronts and light spill are computed per
/// pixel (the GLSL is the renderer's, kind `CityFacade`). `ground` is the
/// pavement height (shopfronts sit on it).
pub fn patch_city_material(m: Material, mask: TextureId, ground: f64) -> Material {
    m.kind(MaterialKind::CityFacade, None)
        .program_key("city-atlas-lit")
        .uniform("uMask", texture_value(mask))
        .uniform("uGround", num(ground))
        .uniform("clippingPlanes", Value::Null)
}
