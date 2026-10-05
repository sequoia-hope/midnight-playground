//! The HUD's one UI material (`hud.wgsl`): the dial, the minimap and the
//! speed lines, told apart by `head.x`. One type, so Bevy builds one
//! pipeline and the wasm carries one copy of the material plugin.

use super::dials::{DialDraw, Fill};
use super::minimap::{MAX_ROAD, MAX_SHAPES, Scene, Shape};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::ui_render::prelude::UiMaterial;

pub const DIAL: f32 = 1.0;
pub const MINIMAP: f32 = 2.0;
pub const SPEEDLINES: f32 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq, ShaderType)]
pub struct HudParams {
    pub head: Vec4,
    pub v: [Vec4; 8],
    pub road: [Vec4; MAX_ROAD / 2],
    pub shapes: [Vec4; MAX_SHAPES * 4],
}

impl HudParams {
    pub fn new(kind: f32) -> HudParams {
        HudParams {
            head: Vec4::new(kind, 0.0, 0.0, 0.0),
            v: [Vec4::ZERO; 8],
            road: [Vec4::ZERO; MAX_ROAD / 2],
            shapes: [Vec4::ZERO; MAX_SHAPES * 4],
        }
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct HudMaterial {
    #[uniform(0)]
    pub p: HudParams,
}

impl UiMaterial for HudMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://mr_game/play/hud/hud.wgsl".into()
    }
}

pub fn plugin(app: &mut App) {
    bevy::asset::embedded_asset!(app, "hud.wgsl");
    app.add_plugins(bevy::ui_render::UiMaterialPlugin::<HudMaterial>::default());
}

fn srgb(c: u32) -> Vec3 {
    Vec3::new(
        ((c >> 16) & 255) as f32 / 255.0,
        ((c >> 8) & 255) as f32 / 255.0,
        (c & 255) as f32 / 255.0,
    )
}

/// The dial's uniforms.
pub fn dial(d: &DialDraw) -> HudParams {
    let mut p = HudParams::new(DIAL);
    let (solid, mid, stops) = match d.paint {
        Fill::Grad(g) => (0.0, g.mid as f32, g.c),
        Fill::Solid(c) => (1.0, 0.5, [c, c, c]),
    };
    p.v[0] = Vec4::new(d.fill.0 as f32, d.fill.1 as f32, solid, mid);
    p.v[1] = Vec4::new(d.zone.0 as f32, d.zone.1 as f32, 0.0, d.zone.3 as f32);
    p.v[2] = srgb(d.zone.2).extend(1.0);
    for (i, c) in stops.iter().enumerate() {
        p.v[3 + i] = srgb(*c).extend(1.0);
    }
    p.v[6] = Vec4::new(d.ticks.0 as f32, d.ticks.1 as f32, d.ticks.2 as f32, 0.0);
    p
}

/// The minimap's uniforms.
pub fn minimap(sc: &Scene) -> HudParams {
    let mut p = HudParams::new(MINIMAP);
    let n = sc.road.len().min(MAX_ROAD);
    for (i, &(x, y)) in sc.road.iter().take(n).enumerate() {
        let v = &mut p.road[i / 2];
        if i % 2 == 0 {
            v.x = x as f32;
            v.y = y as f32;
        } else {
            v.z = x as f32;
            v.w = y as f32;
        }
    }
    let m = sc.shapes.len().min(MAX_SHAPES);
    for (i, s) in sc.shapes.iter().take(m).enumerate() {
        let o = i * 4;
        match *s {
            Shape::Dot {
                x,
                y,
                r,
                fill,
                stroke,
            } => {
                p.shapes[o] = Vec4::new(x as f32, y as f32, r as f32, 0.0);
                let (sw, sc) = stroke.unwrap_or((0.0, [0.0; 4]));
                p.shapes[o + 1] = Vec4::new(0.0, sw as f32, 0.0, 0.0);
                p.shapes[o + 2] = Vec4::from_array(fill);
                p.shapes[o + 3] = Vec4::from_array(sc);
            }
            Shape::Bar { a, b, w, color } => {
                p.shapes[o] = Vec4::new(a.0 as f32, a.1 as f32, b.0 as f32, b.1 as f32);
                p.shapes[o + 1] = Vec4::new(1.0, w as f32, 0.0, 0.0);
                p.shapes[o + 2] = Vec4::from_array(color);
            }
        }
    }
    p.head = Vec4::new(MINIMAP, n as f32, m as f32, 0.0);
    p
}

/// The speed lines' uniforms.
pub fn speedlines(opacity: f64) -> HudParams {
    let mut p = HudParams::new(SPEEDLINES);
    p.head.y = opacity as f32;
    p
}
