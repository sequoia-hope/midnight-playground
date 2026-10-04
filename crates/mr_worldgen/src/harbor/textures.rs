//! Port of `src/world/harbor/textures.js` (roadmap WP 7.1): canvas textures
//! for the port (corrugated steel for sheds, the yard paving) and the
//! container atlas, unit box and material.
//!
//! The JS module caches each picture in a module variable; here they are
//! entries of the world's [`TextureCache`] under `harbor:` keys, as City's
//! are under `city:` (DECISIONS D352). `containerTexture` and
//! `rollerDoorTexture` have no caller and are not ported.

use std::f64::consts::PI;
use std::sync::Arc;

use mr_canvas::Canvas;
use mr_math::{Mulberry32, kernel};
use mr_scene::MaterialKind;
use serde_json::Value;

use crate::material::Material;
use crate::object::{Layer, SceneGraph};
use crate::textures::{Cached, Texture, TextureCache};
use crate::three_geom::{BufferGeometry, box_geometry};

/// `tex(c, repeat = true)`: sRGB, anisotropy 8.
fn tex(c: &Canvas, repeat: bool) -> Texture {
    Texture::from_canvas(c, repeat, true, 8.0)
}

/// Vertical ribs, light-grey so vertex/instance colours tint it.
pub fn corrugated_texture(cache: &mut TextureCache) -> Arc<Cached> {
    cache.cached_with("harbor:corrugated", || {
        const W: f64 = 128.0;
        const H: f64 = 128.0;
        let mut g = Canvas::new(W as u32, H as u32);
        for x in 0..W as u32 {
            let xf = f64::from(x);
            let ph = (xf % 16.0) / 16.0;
            let v = 200.0 + kernel::sin(ph * PI * 2.0) * 32.0 + if ph > 0.9 { -40.0 } else { 0.0 };
            g.set_fill_style(format!("rgb({v},{v},{v})"));
            g.fill_rect(xf, 0.0, 1.0, H);
        }
        let mut rng = Mulberry32::new(12);
        // Streaks of grime.
        for _ in 0..40 {
            let a = 0.03 + rng.next_f64() * 0.06;
            g.set_fill_style(format!("rgba(60,50,40,{a})"));
            let x = rng.next_f64() * W;
            let y = rng.next_f64() * H * 0.4 + H * 0.6;
            let w = 1.0 + rng.next_f64() * 3.0;
            let h = rng.next_f64() * H * 0.5;
            g.fill_rect(x, y, w, h);
        }
        Cached::One(tex(&g, true))
    })
}

/// 16 m of concrete slabs per tile: 4 m panels with sawn joints, per-panel
/// tone, patched panels, oil drips and tyre scuffs. Mapped with uS = vS = 16.
pub fn paving_texture(cache: &mut TextureCache) -> Arc<Cached> {
    cache.cached_with("harbor:paving", || {
        const S: f64 = 512.0;
        const P: f64 = S / 4.0;
        let mut g = Canvas::new(S as u32, S as u32);
        let mut rng = Mulberry32::new(404);
        for i in 0..4 {
            for j in 0..4 {
                let v = 168.0 + rng.next_f64() * 26.0;
                // An occasional patched (darker) panel.
                let w = if rng.next_f64() < 0.12 { -22.0 } else { 0.0 };
                g.set_fill_style(format!("rgb({},{},{})", v + w, v + w - 3.0, v + w - 8.0));
                g.fill_rect(f64::from(i) * P, f64::from(j) * P, P, P);
            }
        }
        // Aggregate speckle.
        for _ in 0..9000 {
            let v = if rng.next_f64() < 0.5 { 255 } else { 0 };
            let a = 0.03 + rng.next_f64() * 0.05;
            g.set_fill_style(format!("rgba({v},{v},{v},{a})"));
            let x = rng.next_f64() * S;
            let y = rng.next_f64() * S;
            let w = 1.0 + rng.next_f64() * 2.0;
            let h = 1.0 + rng.next_f64() * 2.0;
            g.fill_rect(x, y, w, h);
        }
        // Oil drips and scuffs, heavier along the lanes (vertical = along
        // the road).
        for _ in 0..70 {
            let a = 0.05 + rng.next_f64() * 0.12;
            g.set_fill_style(format!("rgba(40,34,30,{a})"));
            g.begin_path();
            let x = rng.next_f64() * S;
            let y = rng.next_f64() * S;
            let rx = 3.0 + rng.next_f64() * 14.0;
            let ry = 2.0 + rng.next_f64() * 9.0;
            let rot = rng.next_f64() * 3.0;
            g.ellipse(x, y, rx, ry, rot, 0.0, 7.0, false);
            g.fill();
        }
        for _ in 0..26 {
            let x = rng.next_f64() * S;
            let a = 0.04 + rng.next_f64() * 0.06;
            g.set_fill_style(format!("rgba(30,28,26,{a})"));
            let w = 4.0 + rng.next_f64() * 5.0;
            g.fill_rect(x, 0.0, w, S);
        }
        // Hairline cracks.
        g.set_stroke_style("rgba(50,45,40,0.35)");
        g.set_line_width(1.0);
        for _ in 0..14 {
            let mut x = rng.next_f64() * S;
            let mut y = rng.next_f64() * S;
            g.begin_path();
            g.move_to(x, y);
            for _ in 0..6 {
                x += (rng.next_f64() - 0.5) * 30.0;
                y += (rng.next_f64() - 0.5) * 30.0;
                g.line_to(x, y);
            }
            g.stroke();
        }
        // Sawn joints between panels.
        g.set_fill_style("rgba(45,40,36,0.75)");
        for k in 0..=4 {
            let k = f64::from(k);
            g.fill_rect(k * P - 1.0, 0.0, 2.0, S);
            g.fill_rect(0.0, k * P - 1.0, S, 2.0);
        }
        Cached::One(tex(&g, true))
    })
}

/// The container brands, one per atlas row (`CONTAINER_BRANDS`).
pub const CONTAINER_BRANDS: [&str; 4] = ["MERIDIAN", "OCEANIX", "NORDSTAR", ""];

/// A mask atlas, not a colour map: R = corrugation shading, G = white paint
/// (logos, codes, stripes), B = rust and grime. The container material
/// tints R by the instance colour so one texture serves every box. Four
/// variant rows (aVar = 0..3), each laid out as side | door end | roof
/// across u.
pub fn container_atlas(cache: &mut TextureCache) -> Arc<Cached> {
    cache.cached_with("harbor:containerAtlas", || {
        const W: f64 = 1024.0;
        const RH: f64 = 128.0;
        const H: f64 = RH * 4.0;
        const SW: f64 = 768.0; // side 0..768, end 768..960, roof 960..1024
        const EW: f64 = 192.0;
        let mut g = Canvas::new(W as u32, H as u32);
        let mut rng = Mulberry32::new(4040);
        const LETTERS: &[u8] = b"ABCDEFGHKLMNPRSTU";
        for row in 0..4usize {
            let y0 = row as f64 * RH;
            // Side: trapezoidal corrugation, bright crests and dark valleys.
            for x in 0..SW as u32 {
                let xf = f64::from(x);
                let ph = (xf % 10.0) / 10.0;
                let v = if ph < 0.35 {
                    200
                } else if ph < 0.5 {
                    150
                } else if ph < 0.85 {
                    175
                } else {
                    215
                };
                g.set_fill_style(format!("rgb({v},0,0)"));
                g.fill_rect(xf, y0, 1.0, RH);
            }
            // Door end: vertical ribs, two door leaves, locking bars.
            for x in SW as u32..(SW + EW) as u32 {
                let xf = f64::from(x);
                let ph = ((xf - SW) % 12.0) / 12.0;
                let v = if ph < 0.5 { 196 } else { 170 };
                g.set_fill_style(format!("rgb({v},0,0)"));
                g.fill_rect(xf, y0, 1.0, RH);
            }
            // Roof: plain, slightly dished.
            g.set_fill_style("rgb(185,0,0)");
            g.fill_rect(SW + EW, y0, W - SW - EW, RH);
            // Frame rails and corner posts (dark shading).
            g.set_fill_style("rgb(95,0,0)");
            g.fill_rect(0.0, y0, SW, 7.0);
            g.fill_rect(0.0, y0 + RH - 9.0, SW, 9.0);
            g.fill_rect(0.0, y0, 8.0, RH);
            g.fill_rect(SW - 8.0, y0, 8.0, RH);
            g.fill_rect(SW, y0, EW, 8.0);
            g.fill_rect(SW, y0 + RH - 9.0, EW, 9.0);
            g.fill_rect(SW, y0, 9.0, RH);
            g.fill_rect(SW + EW - 9.0, y0, 9.0, RH);
            g.fill_rect(SW + EW / 2.0 - 1.0, y0 + 8.0, 2.0, RH - 17.0);
            // Locking bars and handles on the doors.
            g.set_fill_style("rgb(80,0,0)");
            for f in [0.18, 0.36, 0.64, 0.82] {
                g.fill_rect(SW + EW * f - 2.0, y0 + 10.0, 4.0, RH - 20.0);
            }
            g.set_fill_style("rgb(120,0,0)");
            for f in [0.18, 0.36, 0.64, 0.82] {
                g.fill_rect(SW + EW * f - 5.0, y0 + RH * 0.55, 10.0, 4.0);
            }
            // White paint: brand on the side, codes on side and doors.
            g.set_fill_style("rgb(0,255,0)");
            g.set_global_composite_operation("lighter");
            let brand = CONTAINER_BRANDS[row];
            if !brand.is_empty() {
                g.set_font(&format!(
                    "bold {}px Arial, sans-serif",
                    if row == 1 { 52 } else { 58 }
                ));
                g.set_text_align("center");
                g.set_text_baseline("middle");
                g.fill_text(brand, SW * 0.47, y0 + RH * 0.52);
                if row == 0 {
                    g.fill_rect(SW * 0.12, y0 + RH * 0.78, SW * 0.7, 4.0);
                }
                if row == 2 {
                    g.begin_path();
                    g.arc(SW * 0.13, y0 + RH * 0.52, 18.0, 0.0, 7.0, false);
                    g.fill();
                }
                g.set_font("bold 30px Arial, sans-serif");
                g.fill_text(brand, SW + EW / 2.0, y0 + RH * 0.22);
            }
            g.set_text_align("left");
            g.set_text_baseline("alphabetic");
            g.set_font("bold 13px Arial, sans-serif");
            let letter = |rng: &mut Mulberry32| {
                LETTERS[(rng.next_f64() * LETTERS.len() as f64).floor() as usize] as char
            };
            let l1 = letter(&mut rng);
            let l2 = letter(&mut rng);
            let l3 = letter(&mut rng);
            let num = 100000.0 + (rng.next_f64() * 899999.0).floor();
            let check = (rng.next_f64() * 10.0).floor();
            let code = format!("{l1}{l2}{l3}U {num} {check}");
            g.fill_text(&code, SW - 150.0, y0 + 26.0);
            g.fill_text(&code, SW + 16.0, y0 + 30.0);
            g.fill_text("45G1", SW + 16.0, y0 + 46.0);
            g.set_font("bold 9px Arial, sans-serif");
            g.fill_text("MAX GROSS 32500 KG", SW + 16.0, y0 + RH - 22.0);
            g.set_global_composite_operation("source-over");
            // Rust and grime: drips below the top rail, scrapes, dirty
            // bottom rail.
            g.set_global_composite_operation("lighter");
            for _ in 0..90 {
                let x = rng.next_f64() * W;
                let y = y0 + 6.0 + rng.next_f64() * 10.0;
                let a = 0.15 + rng.next_f64() * 0.35;
                g.set_fill_style(format!("rgba(0,0,255,{a})"));
                let w = 1.0 + rng.next_f64() * 2.0;
                let h = 4.0 + rng.next_f64() * 40.0;
                g.fill_rect(x, y, w, h);
            }
            for _ in 0..50 {
                let a = 0.1 + rng.next_f64() * 0.3;
                g.set_fill_style(format!("rgba(0,0,255,{a})"));
                let x = rng.next_f64() * W;
                let y = y0 + rng.next_f64() * RH;
                let w = 2.0 + rng.next_f64() * 14.0;
                let h = 1.0 + rng.next_f64() * 3.0;
                g.fill_rect(x, y, w, h);
            }
            let mut grd = g.create_linear_gradient(0.0, y0 + RH - 30.0, 0.0, y0 + RH);
            grd.add_color_stop(0.0, "rgba(0,0,255,0)");
            grd.add_color_stop(1.0, "rgba(0,0,255,0.45)");
            g.set_fill_style(&grd);
            g.fill_rect(0.0, y0 + RH - 30.0, W, 30.0);
            g.set_global_composite_operation("source-over");
        }
        // `tex(c, false)`, then `colorSpace = NoColorSpace`.
        Cached::One(Texture::from_canvas(&g, false, false, 8.0))
    })
}

/// Unit container (1 × 1 × 1, bottom at y = 0, length along local X) whose
/// faces map onto the atlas regions above.
pub fn container_geometry() -> BufferGeometry {
    let mut g = box_geometry(1.0, 1.0, 1.0, 1.0, 1.0, 1.0);
    g.translate(0.0, 0.5, 0.0);
    let uv = g.get_attribute_mut("uv").expect("uv");
    // BoxGeometry face order: +x, -x, +y, -y, +z, -z (4 vertices each).
    const REGION: [(f64, f64); 6] = [
        (0.75, 0.9375),
        (0.75, 0.9375),
        (0.9375, 1.0),
        (0.9375, 1.0),
        (0.0, 0.75),
        (0.0, 0.75),
    ];
    for (f, &(u0, u1)) in REGION.iter().enumerate() {
        for k in 0..4 {
            let i = f * 4 + k;
            let x = uv.get_x(i);
            uv.set_x(i, u0 + x * (u1 - u0));
            let y = uv.get_y(i);
            uv.set_y(i, 0.02 + y * 0.96);
        }
    }
    g
}

/// `containerMaterial()`: the atlas tinted per instance (kind
/// `ContainerAtlas`): the `aVar` instance attribute picks the atlas row,
/// R² × 1.9 shades the instance colour, G paints white, B browns with rust.
pub fn container_material(graph: &mut SceneGraph, cache: &mut TextureCache) -> Material {
    let map = graph.cached_texture(&container_atlas(cache), Layer::Main, "");
    Material::standard()
        .set("map", map)
        .set("roughness", 0.7)
        .set("metalness", 0.25)
        .kind(MaterialKind::ContainerAtlas, None)
        .program_key("harbor-container")
        .uniform("clippingPlanes", Value::Null)
}
