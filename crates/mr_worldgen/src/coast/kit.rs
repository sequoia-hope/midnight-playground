//! Port of `src/world/coast/kit.js` (roadmap WP 7.1): the scenery building
//! kit for the coast, which the JS copied from `Mountain.js`, whose helpers
//! were module-private.
//!
//! Where the copy is the same code, the port uses Mountain's port of it
//! (re-exported here): the rendered-surface sampler, `instanced`,
//! `placed`, `bakeStatic`, `mergedMesh`, the sign atlas, `roundRect`,
//! `diamond`, `panel`, `makeCanvas` and `canvasTex`. What differs is here:
//! `rockGeometry` (a simplex-noise lump, not flora's), `colorize`, `prep`,
//! and `rockMaterial` (no vertex colours, program key `coast-rock`).

use mr_math::{Noise2D, js};
use mr_scene::MaterialKind;
use serde_json::Value;

use crate::color::Color;
use crate::material::{Material, texture_value};
use crate::object::{Layer, SceneGraph};
use crate::textures::TextureCache;
use crate::three_geom::{BufferAttribute, BufferGeometry, icosahedron_geometry};

pub use crate::mountain::canvas::{FONT, Rect, SignAtlas, canvas_tex, diamond, panel, round_rect};
pub use crate::mountain::kit::{
    Item, Surface, SurfaceSampler, bake_static, instanced, merged_mesh, placed, placed_scaled,
};

/// `rockGeometry(seed, detail, squash = 0.72, sharp = 0.42)`: an
/// icosahedron pushed in and out by simplex noise, its underside flattened
/// so rocks sit rather than balance; faceted.
pub fn rock_geometry(seed: u32, detail: f64, squash: f64, sharp: f64) -> BufferGeometry {
    let mut g = icosahedron_geometry(1.0, detail);
    let n = Noise2D::new(seed);
    let pos = g.get_attribute_mut("position").expect("position");
    for i in 0..pos.count() {
        let (x, y, z) = (pos.get_x(i), pos.get_y(i), pos.get_z(i));
        let d = n.noise(x * 1.1 + z * 2.3 + 5.0, y * 1.3 - z * 0.7) * 0.6
            + n.noise(x * 2.9 - z * 1.7, y * 2.6 + x) * 0.3;
        let r = 1.0 + d * sharp;
        // Flatten the underside so rocks sit rather than balance.
        pos.set_xyz(i, x * r, js::max(y * r * squash, -0.3), z * r);
    }
    g.delete_attribute("uv");
    g.compute_vertex_normals(); // non-indexed → faceted
    g
}

/// `colorize(geo, hex)`: a flat vertex colour.
pub fn colorize(mut geo: BufferGeometry, hex: u32) -> BufferGeometry {
    let c = Color::hex(hex);
    let n = geo.position().count();
    let mut a = vec![0.0f32; n * 3];
    for i in 0..n {
        a[i * 3] = c.r as f32;
        a[i * 3 + 1] = c.g as f32;
        a[i * 3 + 2] = c.b as f32;
    }
    geo.set_attribute("color", BufferAttribute::from_f32(a, 3));
    geo
}

/// `prep(geo)`: non-indexed, no uv, normals recomputed (faceted).
pub fn prep(geo: BufferGeometry) -> BufferGeometry {
    let mut g = if geo.index.is_some() {
        geo.to_non_indexed()
    } else {
        geo
    };
    if g.has_attribute("uv") {
        g.delete_attribute("uv");
    }
    g.compute_vertex_normals();
    g
}

/// `rockMaterial()`: triplanar strata for rocks, instancing-aware
/// (TerrainMesh's patch assumes no instance matrix). The patch (kind
/// `TriplanarRock`, the same GLSL as Mountain's) samples `tRock` at the
/// world position projected on the three axes, weighted by the normal
/// cubed.
pub fn rock_material(graph: &mut SceneGraph, textures: &mut TextureCache) -> Material {
    let tex = graph.cached_texture(&textures.rock_texture(), Layer::Main, "");
    Material::standard()
        .set("color", 0xffffff)
        .set("roughness", 0.92)
        .set("metalness", 0.0)
        .kind(MaterialKind::TriplanarRock, None)
        .program_key("coast-rock")
        .uniform("tRock", texture_value(tex))
        .uniform("clippingPlanes", Value::Null)
}
