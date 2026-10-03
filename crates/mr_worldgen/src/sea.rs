//! Port of `src/world/Sea.js` (roadmap WP 3.5).
//!
//! Open water for levels with a sea. One large plane at sea level, tinted per
//! vertex by depth (turquoise shallows and white-ish surf where the ground
//! meets the water, deep blue offshore) and alpha-faded in the shallows so
//! sand shows through. Reflections come from the sky environment map; a
//! scrolling normal map gives it ripples.
//!
//! The water shader (`patchMaterial`: two crossing scales of ripples, surf
//! lines, fresnel opacity, sun glitter) is the renderer's: the scene
//! carries the material kind `Sea` and its uniforms (SPEC 6.2).

// Index loops stay index loops (DECISIONS D52).
#![allow(clippy::needless_range_loop)]
// The JS writes 6.28 and 6.2832, not 2π; the constants stay as written.
#![allow(clippy::approx_constant)]

use std::sync::Arc;

use mr_canvas::Canvas;
use mr_math::{Mulberry32, clamp, js, kernel, smoothstep};
use mr_scene::{BufferData, MaterialKind, TextureSource};
use serde_json::{Value, json};

use crate::color::Color;
use crate::material::{Material, num, texture_value};
use crate::object::{GeoId, Image, Layer, MaterialId, NodeId, SceneGraph, TextureId};
use crate::terrain::Terrain;
use crate::textures::{Texture, TextureCache};
use crate::three_geom::{BufferAttribute, BufferGeometry};
use crate::world::{Change, Edit, Handle, UpdateCtx};

/// `waveNormals()`: a tileable ripple normal map, 256², from 40 waves.
pub fn wave_normals() -> Texture {
    const S: usize = 256;
    let mut c = Canvas::new(S as u32, S as u32);
    let mut rng = Mulberry32::new(51);
    struct Wave {
        fx: f64,
        fy: f64,
        ph: f64,
        a: f64,
    }
    let mut waves = Vec::new();
    // Integer frequencies keep it tileable; most energy runs along a wind
    // direction, with shorter choppy waves in a wider spread.
    for k in 0..40 {
        let kf = f64::from(k);
        let f = 2.0 + kf * 0.35 + rng.next_f64() * 2.0;
        let ang = 0.5 + (rng.next_f64() - 0.5) * (if k < 12 { 1.0 } else { 2.6 });
        let fx = js::round(kernel::cos(ang) * f);
        let fy = js::or(js::round(kernel::sin(ang) * f), 1.0);
        let ph = rng.next_f64() * 6.28;
        waves.push(Wave {
            fx,
            fy,
            ph,
            a: 1.4 / kernel::pow(kernel::hypot(fx, fy), 1.1),
        });
    }
    let s = S as f64;
    let hgt = |x: f64, y: f64| {
        let mut h = 0.0;
        for w in &waves {
            h += w.a * kernel::sin(((w.fx * x + w.fy * y) / s) * 6.2832 + w.ph);
        }
        h
    };
    let mut img = c.create_image_data(S as u32, S as u32);
    for y in 0..S {
        for x in 0..S {
            let (xf, yf) = (x as f64, y as f64);
            let dx = hgt(xf + 1.0, yf) - hgt(xf - 1.0, yf);
            let dy = hgt(xf, yf + 1.0) - hgt(xf, yf - 1.0);
            let i = (y * S + x) * 4;
            img.set(i, 128.0 + dx * 22.0);
            img.set(i + 1, 128.0 + dy * 22.0);
            img.set(i + 2, 255.0);
            img.set(i + 3, 255.0);
        }
    }
    c.put_image_data(&img, 0, 0);
    // CanvasTexture, RepeatWrapping, NoColorSpace (anisotropy three's 1).
    Texture {
        width: S as u32,
        height: S as u32,
        rgba: c.to_rgba(),
        source: TextureSource::Canvas,
        repeat: true,
        srgb: false,
        anisotropy: 1.0,
    }
}

/// `new Sea(world, seaY)`: the mesh (added to the world's root by the
/// caller, as `world.scene.add`) and the handles its updater animates.
pub struct Sea {
    pub y: f64,
    pub mesh: NodeId,
    pub geometry: GeoId,
    pub material: MaterialId,
    /// `this.normalMap`.
    pub normal_map: TextureId,
}

/// The sea's mesh geometry over the terrain's bounds (`step` 20 m).
pub fn sea_geometry(t: &Terrain, sea_y: f64) -> BufferGeometry {
    let step = 20.0;
    let (x0, z0) = (t.min_x, t.min_z);
    let nx = ((t.max_x - t.min_x) / step).ceil() as usize + 1;
    let nz = ((t.max_z - t.min_z) / step).ceil() as usize + 1;
    // Float32Array.
    let mut h = vec![0f32; nx * nz];
    for j in 0..nz {
        for i in 0..nx {
            let x = x0 + i as f64 * step;
            let z = z0 + j as f64 * step;
            let f = t.far(x, z);
            // Exact heights near the road; far away, only which side matters.
            h[j * nx + i] = (if f.d < 700.0 {
                t.height_at(x, z)
            } else if t.sea_side(&f) > 0.5 {
                sea_y - 30.0
            } else {
                sea_y + 50.0
            }) as f32;
        }
    }
    let hk = |k: usize| f64::from(h[k]);
    let mut pos = vec![0f32; nx * nz * 3];
    let mut col = vec![0f32; nx * nz * 4];
    let deep = Color::hex(0x0d3350);
    let mid = Color::hex(0x14607a);
    let shallow = Color::hex(0x3fa7a4);
    let surf = Color::hex(0xcfe7e6);
    for j in 0..nz {
        for i in 0..nx {
            let k = j * nx + i;
            pos[k * 3] = (x0 + i as f64 * step) as f32;
            pos[k * 3 + 1] = sea_y as f32;
            pos[k * 3 + 2] = (z0 + j as f64 * step) as f32;
            let depth = sea_y - hk(k);
            let mut c = deep;
            c.lerp(mid, 1.0 - smoothstep(6.0, 25.0, depth));
            c.lerp(shallow, 1.0 - smoothstep(0.8, 6.0, depth));
            c.lerp(surf, (1.0 - smoothstep(-0.4, 0.9, depth)) * 0.45);
            col[k * 4] = c.r as f32;
            col[k * 4 + 1] = c.g as f32;
            col[k * 4 + 2] = c.b as f32;
            col[k * 4 + 3] = clamp(0.55 + smoothstep(0.2, 5.0, depth) * 0.42, 0.0, 0.97) as f32;
        }
    }
    let mut idx: Vec<u32> = Vec::new();
    for j in 0..nz - 1 {
        for i in 0..nx - 1 {
            let a = j * nx + i;
            let b = a + 1;
            let d = a + nx;
            let e = d + 1;
            // Skip quads that are entirely dry land.
            if js::min_n(&[hk(a), hk(b), hk(d), hk(e)]) > sea_y + 0.5 {
                continue;
            }
            idx.extend_from_slice(&[a as u32, d as u32, b as u32, b as u32, d as u32, e as u32]);
        }
    }
    let mut g = BufferGeometry::new();
    let n = nx * nz;
    g.set_attribute("position", BufferAttribute::from_f32(pos.clone(), 3));
    g.set_attribute("color", BufferAttribute::from_f32(col, 4));
    let normals: Vec<f32> = (0..n * 3)
        .map(|q| if q % 3 == 1 { 1.0 } else { 0.0 })
        .collect();
    g.set_attribute("normal", BufferAttribute::from_f32(normals, 3));
    let mut uv = vec![0f32; n * 2];
    for k in 0..n {
        uv[k * 2] = (f64::from(pos[k * 3]) / 60.0) as f32;
        uv[k * 2 + 1] = (f64::from(pos[k * 3 + 2]) / 60.0) as f32;
    }
    g.set_attribute("uv", BufferAttribute::from_f32(uv, 2));
    // Water depth per vertex drives the surf band in the shader.
    let depth: Vec<f32> = (0..n).map(|k| (sea_y - hk(k)) as f32).collect();
    g.set_attribute("aDepth", BufferAttribute::from_f32(depth, 1));
    // The JS picks the index type by the list's length, not its values.
    let index = if idx.len() > 65535 {
        BufferAttribute::new(BufferData::U32(idx), 1, false)
    } else {
        BufferAttribute::new(
            BufferData::U16(idx.iter().map(|&v| v as u16).collect()),
            1,
            false,
        )
    };
    g.set_index_attribute(Some(index));
    g.compute_bounding_sphere();
    g
}

impl Sea {
    /// `new Sea(world, seaY)`: the mesh is made but not added anywhere.
    pub fn new(
        graph: &mut SceneGraph,
        textures: &mut TextureCache,
        terrain: &Terrain,
        sea_y: f64,
    ) -> Sea {
        let g = sea_geometry(terrain, sea_y);
        let normal_tex = Arc::new(wave_normals());
        let desc = normal_tex.desc("", 0);
        let normal_map = graph.add_texture(Image::Own(normal_tex), desc);
        let foam = graph.cached_texture(&textures.terrain_detail_texture(), Layer::Main, "");
        // patchMaterial: two crossing scales of ripples (the second
        // counter-scrolling, so the pattern never visibly slides as one
        // sheet), ripples calmed with distance so the far sea doesn't sparkle
        // into noise; surf lines rolling up the beach where the water is
        // shallow; fresnel opacity (see-through looking down, mirror-like
        // toward the horizon); and a sharp sun/moon glitter on the ripple
        // facets.
        let m = Material::standard()
            .set("vertexColors", true)
            .set("transparent", true)
            .set("roughness", 0.12)
            .set("metalness", 0.0)
            .set("normalMap", normal_map)
            .set("normalScale", crate::material::Param::Vec2(0.5, 0.5))
            .set("envMapIntensity", 1.3)
            .kind(MaterialKind::Sea, None)
            .uniform("uTime", num(0.0))
            .uniform("uOff2", json!({ "vec": [num(0.0), num(0.0)] }))
            .uniform("tFoam", texture_value(foam))
            .uniform("clippingPlanes", Value::Null);
        let material = graph.add_material(m);
        let geometry = graph.add_geometry(g);
        let mesh = graph.mesh(geometry, material);
        let o = graph.get_mut(mesh);
        o.receive_shadow = true;
        o.render_order = 1.0;
        Sea {
            y: sea_y,
            mesh,
            geometry,
            material,
            normal_map,
        }
    }

    /// The updater the sea registers: the normal map scrolls, `uTime`
    /// runs, the second ripple scale counter-scrolls (`uOff2`).
    pub fn animator(&self) -> impl FnMut(&UpdateCtx, &mut Vec<Edit>) + Send + Sync + 'static {
        let (tex, mat) = (self.normal_map, self.material);
        let mut offset = [0.0f64; 2];
        let mut time = 0.0f64;
        let mut off2 = [0.0f64; 2];
        move |u: &UpdateCtx, out: &mut Vec<Edit>| {
            let dt = u.dt;
            offset[0] += dt * 0.012;
            offset[1] += dt * 0.007;
            time += dt;
            off2[0] -= dt * 0.021;
            off2[1] += dt * 0.016;
            out.push(Edit {
                target: Handle::Texture(tex),
                change: Change::TextureOffset(offset),
            });
            out.push(Edit {
                target: Handle::Material(mat),
                change: Change::Number {
                    prop: "uTime",
                    value: time,
                },
            });
            out.push(Edit {
                target: Handle::Material(mat),
                change: Change::Vector {
                    prop: "uOff2",
                    value: off2.to_vec(),
                },
            });
        }
    }
}
