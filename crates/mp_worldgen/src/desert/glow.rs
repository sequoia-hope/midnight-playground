//! Port of `src/world/desert/glow.js` (roadmap WP 7.3): night lights that
//! flicker individually (flares, camp fires, chaser bulbs) without a draw
//! call or a CPU update each: one Points cloud or one InstancedMesh per
//! kind, with a per-point phase attribute and a shared time uniform driving
//! the flicker in the vertex shader.
//!
//! The JS shares one uniform object, `glowTime`, between every material
//! made here; the port gives each material its own `uTime` uniform and the
//! Desert animator writes the same value into all of them each frame. The
//! patches (kinds `FlickerPoints` and `GroundPool`) are the renderer's.

use mp_scene::{MaterialKind, NodeType, three};
use serde_json::{Value, json};

use crate::color::Color;
use crate::material::{Material, num};
use crate::object::{Layer, MaterialId, NodeId, SceneGraph};
use crate::textures::TextureCache;
use crate::three_geom::{BufferAttribute, BufferGeometry, Matrix4, plane_geometry};

/// A glow: `{ x, y, z, ph }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glow {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// `ph` (the JS falls back to the item's index where it is missing).
    pub ph: Option<f64>,
}

/// `flickerPoints`' options: `{ size = 1, color = 0xffffff, rate = 1,
/// depth = 0.35, blink = 0 }` (every caller passes a `THREE.Color`).
#[derive(Clone, Copy, Debug)]
pub struct PointOpts {
    pub size: f64,
    pub color: Color,
    pub rate: f64,
    pub depth: f64,
    pub blink: f64,
}

impl Default for PointOpts {
    fn default() -> Self {
        PointOpts {
            size: 1.0,
            color: Color::hex(0xffffff),
            rate: 1.0,
            depth: 0.35,
            blink: 0.0,
        }
    }
}

/// Screen-facing glows. `rate` scales the flicker speed, `depth` how far it
/// dips (0 = steady), `blink` > 0 turns it into a hard on/off chase instead
/// (chaser bulbs: `ph` picks the step). Returns the Points and its
/// material; `time` is `glowTime.value` now.
pub fn flicker_points(
    graph: &mut SceneGraph,
    textures: &mut TextureCache,
    items: &[Glow],
    o: &PointOpts,
    time: f64,
) -> (NodeId, MaterialId) {
    let mut pos = vec![0f32; items.len() * 3];
    let mut ph = vec![0f32; items.len()];
    for (i, p) in items.iter().enumerate() {
        pos[i * 3] = p.x as f32;
        pos[i * 3 + 1] = p.y as f32;
        pos[i * 3 + 2] = p.z as f32;
        ph[i] = p.ph.unwrap_or(i as f64) as f32;
    }
    let mut g = BufferGeometry::new();
    g.set_attribute("position", BufferAttribute::from_f32(pos, 3));
    g.set_attribute("ph", BufferAttribute::from_f32(ph, 1));
    g.compute_bounding_sphere();
    let glow = graph.cached_texture(&textures.glow_texture(), Layer::Main, "");
    let m = graph.add_material(
        Material::points()
            .set("size", o.size)
            .set("map", glow)
            .set("color", o.color)
            .set("transparent", true)
            .set("depthWrite", false)
            .set("blending", three::ADDITIVE_BLENDING as f64)
            .set("sizeAttenuation", true)
            .kind(
                MaterialKind::FlickerPoints,
                Some(json!({ "rate": num(o.rate), "depth": num(o.depth), "blink": num(o.blink) })),
            )
            .program_key(&format!(
                "desert-flicker-{}-{}-{}",
                o.rate, o.depth, o.blink
            ))
            .uniform("uTime", num(time))
            .uniform("clippingPlanes", Value::Null),
    );
    let geo = graph.add_geometry(g);
    let pts = graph.drawable(NodeType::Points, geo, m);
    graph.get_mut(pts).matrix_auto_update = false;
    (pts, m)
}

/// A pool of light on the ground: `{ x, y, z, r, c: [r, g, b], fl, ph }`
/// (`fl` the flicker depth, 0 = steady).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pool {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub r: f64,
    pub c: [f64; 3],
    pub fl: Option<f64>,
    pub ph: Option<f64>,
}

/// Pools of light on the ground (flat additive quads). Returns the
/// InstancedMesh and its material.
pub fn flicker_pools(
    graph: &mut SceneGraph,
    textures: &mut TextureCache,
    items: &[Pool],
    time: f64,
) -> (NodeId, MaterialId) {
    let mut geo = plane_geometry(1.0, 1.0, 1.0, 1.0);
    geo.rotate_x(-std::f64::consts::PI / 2.0);
    let glow = graph.cached_texture(&textures.glow_texture(), Layer::Main, "");
    let mat = graph.add_material(
        Material::basic()
            .set("map", glow)
            .set("transparent", true)
            .set("depthWrite", false)
            .set("blending", three::ADDITIVE_BLENDING as f64)
            .set("polygonOffset", true)
            .set("polygonOffsetFactor", -4.0)
            .set("polygonOffsetUnits", -4.0)
            .kind(MaterialKind::GroundPool, None)
            .program_key("desert-pools")
            .uniform("uTime", num(time))
            .uniform("clippingPlanes", Value::Null),
    );
    let mut ph = vec![0f32; items.len()];
    let mut fl = vec![0f32; items.len()];
    let mut mats = Vec::with_capacity(items.len());
    for (i, p) in items.iter().enumerate() {
        let m = Matrix4::make_scale(p.r * 2.0, 1.0, p.r * 2.0).set_position(p.x, p.y, p.z);
        mats.push((m, Color::new(p.c[0], p.c[1], p.c[2])));
        ph[i] = p.ph.unwrap_or(0.0) as f32;
        fl[i] = p.fl.unwrap_or(0.0) as f32;
    }
    geo.set_instanced_attribute("ph", BufferAttribute::from_f32(ph, 1));
    geo.set_instanced_attribute("fl", BufferAttribute::from_f32(fl, 1));
    let g = graph.add_geometry(geo);
    let im = graph.instanced_mesh(g, mat, items.len() as u32);
    {
        let inst = graph.get_mut(im).instances.as_mut().expect("instanced");
        for (i, (m, c)) in mats.into_iter().enumerate() {
            inst.set_matrix_at(i, &m);
            inst.set_color_at(i, c);
        }
    }
    graph.get_mut(im).render_order = 2.0;
    graph.compute_instance_bounding_sphere(im);
    graph.get_mut(im).matrix_auto_update = false;
    (im, mat)
}
