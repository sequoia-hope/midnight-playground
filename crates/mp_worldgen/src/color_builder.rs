//! Port of `src/world/beach/ColorBuilder.js` (roadmap WP 3.3).
//!
//! A Builder that paints plain-coloured pieces with vertex colours and merges
//! them into one "solid" bucket per channel, instead of one mesh per paint
//! colour. Keys found in `palette` go to solid:<channel>; anything else
//! (glass, emissive, textured, transparent) keeps its own bucket as usual.
//! Set `channel` before drawing ('near' casts shadows, 'far' doesn't).
//!
//! It is the [`Builder`] in [`Paint::Palette`] mode: the shape methods are
//! `Builder`'s, and `add` and `build` come here.

use crate::builder::{BuildOpts, Builder, Paint, lookup, normalise, paint};
use crate::color::Color;
use crate::material::Material;
use crate::object::{MaterialId, NodeId, SceneGraph};
use crate::three_geom::{BufferGeometry, Matrix4, merge_geometries};

impl Builder {
    /// `new ColorBuilder(palette)`: `palette` is `[(key, hex)]` in the JS
    /// object's order; the channel starts as `near`.
    pub fn new_color(palette: &[(&str, u32)]) -> Builder {
        let mut b = Builder::new();
        b.paint = Paint::Palette {
            palette: palette
                .iter()
                .map(|(k, hex)| (k.to_string(), Color::hex(*hex)))
                .collect(),
            channel: "near".to_string(),
        };
        b
    }

    /// `B.channel = c` on a ColorBuilder.
    pub fn set_channel(&mut self, c: &str) {
        if let Paint::Palette { channel, .. } = &mut self.paint {
            *channel = c.to_string();
        }
    }

    /// ColorBuilder's `add`, with `m` = `frame × local`.
    pub(crate) fn color_add(&mut self, key: &str, geo: &BufferGeometry, m: &Matrix4) {
        let Paint::Palette { palette, channel } = &self.paint else {
            unreachable!("color_add on a ColorBuilder")
        };
        let col = palette.iter().find(|(k, _)| k == key).map(|&(_, c)| c);
        let mut g = normalise(geo);
        g.apply_matrix4(m);
        let mut bucket = key.to_string();
        if let Some(col) = col {
            paint(&mut g, col);
            bucket = format!("solid:{channel}");
        }
        self.bucket(&bucket).push(g);
    }

    /// ColorBuilder's `build`: meshes named `beach:<key>`; the solid
    /// buckets use `materials.solid`, or else one new
    /// `MeshStandardMaterial({ vertexColors: true, roughness: 0.85 })`
    /// made for this call, and `solid:near` alone casts shadows.
    pub(crate) fn color_build(
        &mut self,
        graph: &mut SceneGraph,
        materials: &[(&str, MaterialId)],
        opts: &BuildOpts,
    ) -> Vec<NodeId> {
        let mut meshes = Vec::new();
        // The JS makes the fallback material up front; one nothing draws
        // never reaches the scene, so making it on first use is the same.
        let mut solid = lookup(materials, "solid");
        for (key, geos) in std::mem::take(&mut self.buckets) {
            let is_solid = key.starts_with("solid:");
            let mat = if is_solid {
                Some(*solid.get_or_insert_with(|| {
                    graph.add_material(
                        Material::standard()
                            .set("vertexColors", true)
                            .set("roughness", 0.85),
                    )
                }))
            } else {
                lookup(materials, &key)
            };
            let Some(mat) = mat else {
                graph.log.push(format!("Beach: no material for {key}"));
                continue;
            };
            let refs: Vec<&BufferGeometry> = geos.iter().collect();
            let Some(mut merged) = merge_geometries(&refs, false) else {
                continue;
            };
            merged.compute_bounding_sphere();
            let geo = graph.add_geometry(merged);
            let mesh = graph.mesh(geo, mat);
            let o = graph.get_mut(mesh);
            o.name = format!("beach:{key}");
            o.cast_shadow = if is_solid {
                key == "solid:near"
            } else {
                opts.cast_shadow.includes(&key)
            };
            o.receive_shadow = opts.receive_shadow;
            o.matrix_auto_update = false;
            o.update_matrix();
            meshes.push(mesh);
        }
        meshes
    }
}
