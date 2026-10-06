//! Desert's canvas code: `makeSigns` (the two painted atlases and the neon
//! one), the start gantry's banners, the railway's ties and the lake bed's
//! cracked mud, drawn on mp_canvas in the JS order.

use std::f64::consts::PI;
use std::sync::Arc;

use mp_canvas::Canvas;
use mp_math::{Mulberry32, js};

use crate::beach::atlas::{
    NeonOpts, PaintedOpts, Rect, SignAtlas, neon_sign, painted_sign, round_rect,
};
use crate::object::{Image, SceneGraph, TextureId};
use crate::textures::Texture;

/// A texture of its own (`new THREE.CanvasTexture(cv)`) in the graph.
pub(crate) fn own_texture(graph: &mut SceneGraph, t: Texture) -> TextureId {
    let t = Arc::new(t);
    let desc = t.desc("", 0);
    graph.add_texture(Image::Own(t), desc)
}

/// `this.sg`: the painted atlases' cells.
pub struct Sg {
    pub dip: Rect,
    pub rocks: Rect,
    pub flood: Rect,
    pub byway: Rect,
    pub route: Rect,
    pub bb_oasis: Rect,
    pub bb_gas: Rect,
    pub bb_trials: Rect,
    pub bb_jerky: Rect,
    pub gas: Rect,
    pub price: Rect,
    pub mile: [Rect; 3],
    pub closed: Rect,
    pub end: Rect,
    pub lake: Rect,
    pub timing: Rect,
    // The second atlas.
    pub bb_snakes: Rect,
    pub bb_pie: Rect,
    pub bb_motor: Rect,
    pub bb_dino: Rect,
    pub bb_faded: Rect,
    pub mm: [Rect; 5],
    pub tower: Rect,
    pub soda: Rect,
    pub stage: Rect,
}

/// `this.nn`: the neon atlas's cells.
pub struct Nn {
    pub oasis: Rect,
    pub vacancy: Rect,
    pub diner: Rect,
    pub eat: Rect,
    pub motel_arrow: Rect,
}

/// What `makeSigns` makes: the cells and the three atlas textures (`at`,
/// `nt`, `at2`), each a new `CanvasTexture`.
pub struct Signs {
    pub sg: Sg,
    pub nn: Nn,
    pub at: TextureId,
    pub nt: TextureId,
    pub at2: TextureId,
}

/// `diamond(text, size)`: a yellow warning diamond.
fn diamond(a: &mut SignAtlas, text: &str, size: f64) -> Rect {
    a.add(192.0, 192.0, |g, w, h| {
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
        g.set_text_align("center");
        g.set_text_baseline("middle");
        g.set_font(&format!(
            "bold {size}px \"Arial Narrow\", Arial, sans-serif"
        ));
        let lines: Vec<&str> = text.split('\n').collect();
        let n = lines.len() as f64;
        for (i, l) in lines.iter().enumerate() {
            g.fill_text(
                l,
                w / 2.0,
                h / 2.0 + (i as f64 - (n - 1.0) / 2.0) * size * 0.9,
            );
        }
    })
}

/// `weathered(draw)`: sun-faded, peeling: pale blotches and a few dark
/// streaks over the sign.
fn weathered<'a>(
    draw: impl FnOnce(&mut Canvas, f64, f64) + 'a,
) -> impl FnOnce(&mut Canvas, f64, f64) + 'a {
    move |g, w, h| {
        draw(g, w, h);
        let mut r = Mulberry32::new(js::to_uint32(w * 7.0 + h));
        for _ in 0..40 {
            let a = 0.05 + r.next_f64() * 0.12;
            g.set_fill_style(format!("rgba(255,248,230,{a})"));
            g.begin_path();
            let (x, y, rad) = (
                r.next_f64() * w,
                r.next_f64() * h,
                6.0 + r.next_f64() * 40.0,
            );
            g.arc(x, y, rad, 0.0, 7.0, false);
            g.fill();
        }
        for _ in 0..12 {
            let a = 0.08 + r.next_f64() * 0.1;
            g.set_fill_style(format!("rgba(60,40,20,{a})"));
            let x = r.next_f64() * w;
            let y = r.next_f64() * h * 0.5;
            let ww = 2.0 + r.next_f64() * 4.0;
            let hh = h * (0.2 + r.next_f64() * 0.5);
            g.fill_rect(x, y, ww, hh);
        }
    }
}

/// `paintedSign(text, o)` with the options the calls below fill in.
fn ps<'a>(text: &'a str, o: PaintedOpts<'a>) -> impl FnOnce(&mut Canvas, f64, f64) + 'a {
    painted_sign(text, o)
}

/// `makeSigns()`: the atlases, drawn in the JS order.
pub fn make_signs(graph: &mut SceneGraph) -> Signs {
    let mut a = SignAtlas::new(2048, Some("#ffffff"));
    let mut n = SignAtlas::new(1024, Some("#0b0a10"));
    let d = PaintedOpts::default();
    let dip = diamond(&mut a, "DIP", 64.0);
    let rocks = diamond(&mut a, "FALLING\nROCKS", 36.0);
    let flood = diamond(&mut a, "FLASH\nFLOOD", 38.0);
    let byway = a.add(
        768.0,
        256.0,
        ps(
            "RED ROCK CANYON",
            PaintedOpts {
                bg: "#6b3a24",
                fg: "#f6e4c4",
                font: "bold 92px Georgia, serif",
                sub: Some("SCENIC BYWAY"),
                sub_font: "bold 46px Georgia, serif",
                border: Some("#f6e4c4"),
                ..d
            },
        ),
    );
    let route = a.add(256.0, 256.0, |g, w, h| {
        g.set_fill_style("#fff");
        g.fill_rect(0.0, 0.0, w, h);
        g.set_fill_style("#111");
        g.begin_path();
        g.move_to(20.0, 30.0);
        g.line_to(w - 20.0, 30.0);
        g.line_to(w - 26.0, 120.0);
        g.quadratic_curve_to(w - 40.0, 200.0, w / 2.0, h - 16.0);
        g.quadratic_curve_to(40.0, 200.0, 26.0, 120.0);
        g.close_path();
        g.fill();
        g.set_fill_style("#fff");
        g.begin_path();
        g.move_to(34.0, 44.0);
        g.line_to(w - 34.0, 44.0);
        g.line_to(w - 38.0, 122.0);
        g.quadratic_curve_to(w - 52.0, 188.0, w / 2.0, h - 32.0);
        g.quadratic_curve_to(52.0, 188.0, 38.0, 122.0);
        g.close_path();
        g.fill();
        g.set_fill_style("#111");
        g.set_text_align("center");
        g.set_text_baseline("middle");
        g.set_font("bold 34px Arial");
        g.fill_text("U S", w / 2.0, 72.0);
        g.set_font("bold 30px Arial");
        g.fill_text("ROUTE", w / 2.0, 104.0);
        g.set_font("bold 92px \"Arial Narrow\", Arial");
        g.fill_text("66", w / 2.0, 170.0);
    });
    let bb_oasis = a.add(
        1016.0,
        380.0,
        ps(
            "OASIS MOTEL",
            PaintedOpts {
                bg: "#1f6f7a",
                fg: "#fff6e0",
                font: "bold 150px \"Arial Black\", Arial",
                sub: Some("POOL · COLOR TV · DINER — 2 MILES"),
                sub_font: "bold 52px Arial",
                stripe: Some("#f2b233"),
                ..d
            },
        ),
    );
    let bb_gas = a.add(
        1016.0,
        380.0,
        ps(
            "LAST GAS",
            PaintedOpts {
                bg: "#f2e8d0",
                fg: "#b8322a",
                font: "bold 170px \"Arial Black\", Arial",
                sub: Some("NEXT SERVICES 98 MILES"),
                sub_font: "bold 58px Arial",
                stripe: Some("#b8322a"),
                ..d
            },
        ),
    );
    let bb_trials = a.add(
        1016.0,
        380.0,
        ps(
            "SILVER LAKE",
            PaintedOpts {
                bg: "#111418",
                fg: "#ffd84d",
                font: "bold 150px \"Arial Black\", Arial",
                sub: Some("SPEED TRIALS TONIGHT →"),
                sub_font: "bold 62px Arial",
                stripe: Some("#e84a2a"),
                ..d
            },
        ),
    );
    let bb_jerky = a.add(
        1016.0,
        380.0,
        ps(
            "JERKY · FIREWORKS",
            PaintedOpts {
                bg: "#e8c86a",
                fg: "#4a2a18",
                font: "bold 110px \"Arial Black\", Arial",
                sub: Some("TRADING POST — NEXT EXIT"),
                sub_font: "bold 58px Arial",
                stripe: Some("#4a2a18"),
                ..d
            },
        ),
    );
    let gas = a.add(
        512.0,
        128.0,
        ps(
            "LAST CHANCE GAS",
            PaintedOpts {
                bg: "#c23a2e",
                fg: "#fff",
                ..d
            },
        ),
    );
    let price = a.add(256.0, 256.0, |g, w, _h| {
        g.set_fill_style("#fff");
        g.fill_rect(0.0, 0.0, w, _h);
        g.set_fill_style("#c23a2e");
        g.fill_rect(0.0, 0.0, w, 64.0);
        g.set_fill_style("#fff");
        g.set_font("bold 44px Arial");
        g.set_text_align("center");
        g.set_text_baseline("middle");
        g.fill_text("GAS", w / 2.0, 32.0);
        g.set_fill_style("#111");
        g.set_font("bold 40px \"Courier New\", monospace");
        for (i, l) in ["REG 5.89", "PLS 6.19", "DSL 6.49"].iter().enumerate() {
            g.fill_text(l, w / 2.0, 100.0 + i as f64 * 56.0);
        }
    });
    let mile = ["1 MILE", "1/2 MILE", "1/4 MILE"].map(|m| {
        a.add(
            384.0,
            192.0,
            ps(
                m,
                PaintedOpts {
                    bg: "#111",
                    fg: "#fff",
                    font: "bold 96px \"Arial Narrow\", Arial",
                    border: Some("#ffd84d"),
                    ..d
                },
            ),
        )
    });
    let closed = a.add(
        512.0,
        192.0,
        ps(
            "ROAD CLOSED",
            PaintedOpts {
                bg: "#f2f2f2",
                fg: "#111",
                font: "bold 84px \"Arial Narrow\", Arial",
                border: Some("#111"),
                ..d
            },
        ),
    );
    let end = a.add(
        768.0,
        256.0,
        ps(
            "END OF COURSE",
            PaintedOpts {
                bg: "#c23a2e",
                fg: "#fff",
                font: "bold 110px \"Arial Narrow\", Arial",
                border: Some("#fff"),
                ..d
            },
        ),
    );
    let lake = a.add(
        768.0,
        256.0,
        ps(
            "SILVER LAKE",
            PaintedOpts {
                bg: "#1c2a3a",
                fg: "#e8eef4",
                font: "bold 110px Georgia, serif",
                sub: Some("DRY LAKE · SPEED TRIALS"),
                sub_font: "bold 44px Georgia, serif",
                border: Some("#e8eef4"),
                ..d
            },
        ),
    );
    let timing = a.add(
        512.0,
        128.0,
        ps(
            "TIMING",
            PaintedOpts {
                bg: "#f2f2f2",
                fg: "#111",
                font: "bold 90px \"Arial Black\", Arial",
                ..d
            },
        ),
    );
    let nd = NeonOpts::default();
    let oasis = n.add(
        1024.0,
        300.0,
        neon_sign(
            "Oasis",
            NeonOpts {
                color: "#5ff2ff",
                font: "bold 200px \"Brush Script MT\", \"Segoe Script\", cursive",
                sub: Some("MOTEL · CAFE · GAS"),
                sub_color: "#ff5fd2",
                sub_font: "bold 58px Arial, sans-serif",
                ..nd
            },
        ),
    );
    let vacancy = n.add(
        512.0,
        160.0,
        neon_sign(
            "Vacancy",
            NeonOpts {
                color: "#ff3b6b",
                sub: Some("COLOR TV · POOL"),
                sub_color: "#5ff2ff",
                ..nd
            },
        ),
    );
    let diner = n.add(
        512.0,
        170.0,
        neon_sign(
            "Sidewinder Cafe",
            NeonOpts {
                color: "#ffb24d",
                sub: Some("OPEN ALL NIGHT"),
                sub_color: "#62f0ff",
                ..nd
            },
        ),
    );
    let eat = n.add(
        256.0,
        128.0,
        neon_sign(
            "EAT",
            NeonOpts {
                color: "#ff4040",
                font: "bold 96px \"Arial Black\", Arial",
                frame: false,
                ..nd
            },
        ),
    );
    // Second atlas: the extra billboards, mile markers and roadside
    // lettering added with the Route 66 dressing.
    let mut a2 = SignAtlas::new(2048, Some("#ffffff"));
    let wp = |a2: &mut SignAtlas, text: &'static str, o: PaintedOpts<'static>| {
        a2.add(1016.0, 380.0, weathered(painted_sign(text, o)))
    };
    let bb_snakes = wp(
        &mut a2,
        "SEE LIVE RATTLERS!",
        PaintedOpts {
            bg: "#f2d64a",
            fg: "#1a1a1a",
            font: "bold 104px \"Arial Black\", Arial",
            sub: Some("GILA MONSTERS · TARANTULAS — 5 MI"),
            sub_font: "bold 50px Arial",
            stripe: Some("#b8322a"),
            ..PaintedOpts::default()
        },
    );
    let bb_pie = wp(
        &mut a2,
        "HOME MADE PIE",
        PaintedOpts {
            bg: "#f4ece0",
            fg: "#2e5a8a",
            font: "italic bold 128px Georgia, serif",
            sub: Some("SIDEWINDER CAFE · EXIT NOW"),
            sub_font: "bold 56px Arial",
            stripe: Some("#2e5a8a"),
            ..PaintedOpts::default()
        },
    );
    let bb_motor = wp(
        &mut a2,
        "ROUTE 66 MOTOR CO.",
        PaintedOpts {
            bg: "#1d3f75",
            fg: "#ffd84d",
            font: "bold 96px \"Arial Black\", Arial",
            sub: Some("TIRES · TOWING · COLD BEER"),
            sub_font: "bold 58px Arial",
            stripe: Some("#ffd84d"),
            ..PaintedOpts::default()
        },
    );
    let bb_dino = wp(
        &mut a2,
        "DINOSAUR PARK",
        PaintedOpts {
            bg: "#3a7a3a",
            fg: "#fff6d8",
            font: "bold 130px \"Arial Black\", Arial",
            sub: Some("LIFE SIZE! KIDS FREE — NEXT RIGHT"),
            sub_font: "bold 50px Arial",
            stripe: Some("#e8a030"),
            ..PaintedOpts::default()
        },
    );
    let bb_faded = wp(
        &mut a2,
        "ICE COLD WATER",
        PaintedOpts {
            bg: "#d8c8a8",
            fg: "#8a4a3a",
            font: "bold 120px \"Arial Black\", Arial",
            sub: Some("FREE · 20 MI · FREE"),
            sub_font: "bold 62px Arial",
            stripe: Some("#8a4a3a"),
            ..PaintedOpts::default()
        },
    );
    let mm = [0, 1, 2, 3, 4].map(|k| {
        a2.add(96.0, 256.0, |g, w, h| {
            g.set_fill_style("#0b6b3a");
            g.fill_rect(0.0, 0.0, w, h);
            g.set_stroke_style("#fff");
            g.set_line_width(5.0);
            g.stroke_rect(5.0, 5.0, w - 10.0, h - 10.0);
            g.set_fill_style("#fff");
            g.set_text_align("center");
            g.set_text_baseline("middle");
            g.set_font("bold 26px Arial");
            g.fill_text("MILE", w / 2.0, 40.0);
            g.set_font("bold 62px \"Arial Narrow\", Arial");
            let digits = (141 + k).to_string();
            for (i, d) in digits.chars().enumerate() {
                g.fill_text(&d.to_string(), w / 2.0, 96.0 + i as f64 * 56.0);
            }
        })
    });
    let tower = a2.add(512.0, 160.0, |g, w, h| {
        g.clear_rect(0.0, 0.0, w, h);
        g.set_fill_style("#d8d4cc");
        g.fill_rect(0.0, 0.0, w, h);
        g.set_fill_style("#b8322a");
        g.set_text_align("center");
        g.set_text_baseline("middle");
        g.set_font("italic bold 110px Georgia, serif");
        g.fill_text("Oasis", w / 2.0, h / 2.0 + 6.0);
    });
    let soda = a2.add(128.0, 256.0, |g, w, h| {
        g.set_fill_style("#c8202a");
        g.fill_rect(0.0, 0.0, w, h);
        g.set_fill_style("#fff");
        g.set_font("italic bold 30px Georgia, serif");
        g.set_text_align("center");
        g.save();
        g.translate(w / 2.0, h * 0.35);
        g.rotate(-PI / 2.0);
        g.fill_text("Ice Cold", 0.0, 10.0);
        g.restore();
        g.set_fill_style("#222");
        g.fill_rect(16.0, h * 0.62, w - 32.0, h * 0.3);
        g.set_fill_style("#e8e8e8");
        for i in 0..4 {
            g.fill_rect(24.0 + i as f64 * 22.0, h * 0.66, 14.0, 22.0);
        }
    });
    let stage = a2.add(
        768.0,
        192.0,
        painted_sign(
            "SILVER LAKE SPEED TRIALS",
            PaintedOpts {
                bg: "#111418",
                fg: "#ffd84d",
                font: "bold 64px \"Arial Black\", Arial",
                stripe: Some("#e84a2a"),
                ..PaintedOpts::default()
            },
        ),
    );
    let at2 = a2.texture(graph);
    let motel_arrow = n.add(512.0, 256.0, |g, w, h| {
        g.set_fill_style("#0b0a10");
        g.fill_rect(0.0, 0.0, w, h);
        g.set_line_join("round");
        g.set_line_cap("round");
        let arrow = |g: &mut Canvas| {
            g.begin_path();
            g.move_to(24.0, 70.0);
            g.line_to(380.0, 70.0);
            g.line_to(380.0, 30.0);
            g.line_to(490.0, 128.0);
            g.line_to(380.0, 226.0);
            g.line_to(380.0, 186.0);
            g.line_to(24.0, 186.0);
            g.close_path();
        };
        g.set_shadow_color("#ff3b6b");
        g.set_shadow_blur(24.0);
        g.set_stroke_style("#ff3b6b");
        g.set_line_width(10.0);
        arrow(g);
        g.stroke();
        g.set_shadow_blur(0.0);
        g.set_stroke_style("#ffd2e0");
        g.set_line_width(3.0);
        arrow(g);
        g.stroke();
        g.set_font("bold 90px \"Arial Black\", Arial");
        g.set_text_align("center");
        g.set_text_baseline("middle");
        g.set_shadow_color("#5ff2ff");
        g.set_shadow_blur(22.0);
        g.set_fill_style("#5ff2ff");
        g.fill_text("MOTEL", 210.0, 132.0);
        g.set_shadow_blur(0.0);
        g.set_fill_style("#e8ffff");
        g.set_font("bold 86px \"Arial Black\", Arial");
        g.fill_text("MOTEL", 210.0, 132.0);
    });
    let at = a.texture(graph);
    let nt = n.texture(graph);
    Signs {
        sg: Sg {
            dip,
            rocks,
            flood,
            byway,
            route,
            bb_oasis,
            bb_gas,
            bb_trials,
            bb_jerky,
            gas,
            price,
            mile,
            closed,
            end,
            lake,
            timing,
            bb_snakes,
            bb_pie,
            bb_motor,
            bb_dino,
            bb_faded,
            mm,
            tower,
            soda,
            stage,
        },
        nn: Nn {
            oasis,
            vacancy,
            diner,
            eat,
            motel_arrow,
        },
        at,
        nt,
        at2,
    }
}

/// The start gantry's banner (`bannerTex(front)`).
pub fn start_banner(front: bool) -> Texture {
    let mut g = Canvas::new(1024, 160);
    g.set_fill_style("#1a0e0a");
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
    let mut grd = g.create_linear_gradient(120.0, 0.0, 904.0, 0.0);
    grd.add_color_stop(0.0, "#ff9a3c");
    grd.add_color_stop(1.0, "#c8431e");
    g.set_fill_style(grd);
    g.fill_rect(120.0, 0.0, 784.0, 8.0);
    g.fill_rect(120.0, 152.0, 784.0, 8.0);
    g.set_fill_style("#fff");
    g.set_text_align("center");
    g.set_text_baseline("middle");
    g.set_font("italic 900 84px \"Arial Narrow\", Arial, sans-serif");
    g.fill_text(if front { "RED ROCK CANYON" } else { "START" }, 512.0, 70.0);
    g.set_font("bold 26px \"Arial Narrow\", Arial, sans-serif");
    g.set_fill_style("#ffc89a");
    g.fill_text(
        if front {
            "STAGE 4 · DESERT RUN"
        } else {
            "RED ROCK CANYON"
        },
        512.0,
        132.0,
    );
    Texture::from_canvas(&g, false, true, 8.0)
}

/// The railway bed: dark sleepers across pale ballast.
pub fn ties_texture() -> Texture {
    let mut g = Canvas::new(64, 256);
    g.set_fill_style("#8c7c68");
    g.fill_rect(0.0, 0.0, 64.0, 256.0);
    let mut rng = Mulberry32::new(5);
    for _ in 0..900 {
        let v = 90.0 + rng.next_f64() * 70.0;
        g.set_fill_style(format!("rgb({},{},{})", v, v * 0.9, v * 0.78));
        let x = rng.next_f64() * 64.0;
        let y = rng.next_f64() * 256.0;
        g.fill_rect(x, y, 2.0, 2.0);
    }
    let mut y = 0.0;
    while y < 256.0 {
        g.set_fill_style("#3a2e26");
        g.fill_rect(6.0, y + 2.0, 52.0, 9.0);
        g.set_fill_style("rgba(0,0,0,0.25)");
        g.fill_rect(6.0, y + 10.0, 52.0, 2.0);
        y += 16.0;
    }
    Texture::from_canvas(&g, true, true, 8.0)
}

/// Cracked mud for the lake bed: a canvas of polygon cracks.
pub fn lakebed_texture() -> Texture {
    const S: usize = 512;
    let sf = S as f64;
    let mut g = Canvas::new(S as u32, S as u32);
    g.set_fill_style("#d6cfc0");
    g.fill_rect(0.0, 0.0, sf, sf);
    let mut rng = Mulberry32::new(81);
    for _ in 0..6000 {
        let v = 190.0 + rng.next_f64() * 40.0;
        g.set_fill_style(format!("rgba({},{},{},0.35)", v, v * 0.96, v * 0.9));
        let x = rng.next_f64() * sf;
        let y = rng.next_f64() * sf;
        g.fill_rect(x, y, 2.0, 2.0);
    }
    // Voronoi-ish cracks from jittered cells (tileable by wrapping).
    let mut cells: Vec<[f64; 2]> = Vec::new();
    for j in 0..12 {
        for i in 0..12 {
            let cx = (i as f64 + 0.1 + rng.next_f64() * 0.8) * sf / 12.0;
            let cy = (j as f64 + 0.1 + rng.next_f64() * 0.8) * sf / 12.0;
            cells.push([cx, cy]);
        }
    }
    let mut img = g.get_image_data(0, 0, S as u32, S as u32);
    for y in 0..S {
        for x in 0..S {
            let (xf, yf) = (x as f64, y as f64);
            let mut d1 = 1e9;
            let mut d2 = 1e9;
            for &[cx, cy] in &cells {
                let mut dx = (xf - cx).abs();
                let mut dy = (yf - cy).abs();
                dx = js::min(dx, sf - dx);
                dy = js::min(dy, sf - dy);
                let d = dx * dx + dy * dy;
                if d < d1 {
                    d2 = d1;
                    d1 = d;
                } else if d < d2 {
                    d2 = d;
                }
            }
            let e = d2.sqrt() - d1.sqrt();
            if e < 1.8 {
                let k = (y * S + x) * 4;
                let a = 1.0 - e / 1.8;
                let (r, gg, b) = (
                    f64::from(img.data[k]),
                    f64::from(img.data[k + 1]),
                    f64::from(img.data[k + 2]),
                );
                img.set(k, r - 48.0 * a);
                img.set(k + 1, gg - 50.0 * a);
                img.set(k + 2, b - 52.0 * a);
            }
        }
    }
    g.put_image_data(&img, 0, 0);
    Texture::from_canvas(&g, true, true, 8.0)
}
