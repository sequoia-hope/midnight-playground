//! The pursuit's props from `src/game/PursuitView.js` (roadmap WP 8.2):
//! the sawhorse barrier of a heavy roadblock (`sawhorseModel`, with its
//! striped 2D-canvas board) and the spike strip (`spikeStrip`), built into a
//! [`SceneGraph`] as the JS builds them with three's classes. The JS module
//! keeps the sawhorse's geometry, materials and texture, and the spike
//! strip's material, in module variables, so every prop of a page shares
//! them; [`PropKit`] holds them for one graph.

use std::sync::Arc;

use mp_canvas::Canvas;
use mp_math::{js, kernel};
use mp_track::Track;

use crate::material::Material;
use crate::object::{GeoId, Image, MaterialId, NodeId, SceneGraph};
use crate::textures::Texture;
use crate::three_geom::{BufferAttribute, BufferGeometry, Euler, box_geometry, cone_geometry};

/// What PursuitView.js keeps in `sawhorseParts` and `spikeMat`, per graph.
#[derive(Clone, Copy, Debug, Default)]
pub struct PropKit {
    sawhorse: Option<SawhorseParts>,
    spike_mat: Option<MaterialId>,
}

#[derive(Clone, Copy, Debug)]
struct SawhorseParts {
    board: GeoId,
    leg: GeoId,
    board_mat: MaterialId,
    leg_mat: MaterialId,
}

/// A sawhorse's handle: `{ root, body, wheels: [], steerPivots: [],
/// exhausts: [], dims }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SawhorseModel {
    pub root: NodeId,
    pub body: NodeId,
}

/// `dims: { length: 2.4, width: 0.5, height: 1.0, wheelRadius: 0.3,
/// wheelBase: 1 }`.
pub const SAWHORSE_DIMS: [f64; 5] = [2.4, 0.5, 1.0, 0.3, 1.0];

/// The board's stripes: a 128 × 16 canvas of red and white diagonal bands.
pub fn sawhorse_canvas() -> Canvas {
    let mut g = Canvas::new(128, 16);
    for i in 0..8 {
        let x = f64::from(i) * 16.0;
        g.set_fill_style(if i % 2 == 1 { "#f4f4f0" } else { "#e2261c" });
        g.begin_path();
        g.move_to(x, 16.0);
        g.line_to(x + 16.0, 0.0);
        g.line_to(x + 32.0, 0.0);
        g.line_to(x + 16.0, 16.0);
        g.fill();
    }
    g.set_fill_style("#e2261c");
    g.begin_path();
    g.move_to(-16.0, 16.0);
    g.line_to(0.0, 0.0);
    g.line_to(16.0, 0.0);
    g.line_to(0.0, 16.0);
    g.fill();
    g
}

fn sawhorse_parts(graph: &mut SceneGraph) -> SawhorseParts {
    // `new THREE.CanvasTexture(c)`, `colorSpace = SRGBColorSpace`: clamped,
    // anisotropy 1.
    let tex = Arc::new(Texture::from_canvas(&sawhorse_canvas(), false, true, 1.0));
    let desc = tex.desc("", 0);
    let tex = graph.add_texture(Image::Own(tex), desc);
    let board = graph.add_geometry(box_geometry(0.06, 0.28, 2.4, 1.0, 1.0, 1.0));
    let leg = graph.add_geometry(box_geometry(0.05, 1.0, 0.06, 1.0, 1.0, 1.0));
    let board_mat = graph.add_material(
        Material::standard()
            .set("map", tex)
            .set("roughness", 0.5)
            .set("emissive", 0xffffff)
            .set("emissiveMap", tex)
            .set("emissiveIntensity", 0.18),
    );
    let leg_mat = graph.add_material(
        Material::standard()
            .set("color", 0xd8d8d0)
            .set("roughness", 0.7),
    );
    SawhorseParts {
        board,
        leg,
        board_mat,
        leg_mat,
    }
}

/// `sawhorseModel()`: a striped board on two A-frame legs, built as a
/// minimal vehicle model (+Z along the board) so it moves like one.
pub fn sawhorse_model(graph: &mut SceneGraph, kit: &mut PropKit) -> SawhorseModel {
    let p = *kit.sawhorse.get_or_insert_with(|| sawhorse_parts(graph));
    let root = graph.group("");
    let body = graph.group("");
    graph.add(root, body);
    let board = graph.mesh(p.board, p.board_mat);
    graph.get_mut(board).position.y = 0.9;
    graph.add(body, board);
    for z in [-0.95, 0.95] {
        for x in [-1.0, 1.0] {
            let leg = graph.mesh(p.leg, p.leg_mat);
            let o = graph.get_mut(leg);
            o.position.x = x * 0.2;
            o.position.y = 0.48;
            o.position.z = z;
            o.set_rotation(&Euler::new(0.0, 0.0, x * 0.38));
            graph.add(body, leg);
        }
    }
    SawhorseModel { root, body }
}

/// `mergeAll(geos)`: a small non-indexed merge of positions and normals
/// (the strips are rebuilt per placement).
fn merge_all(geos: &[BufferGeometry]) -> BufferGeometry {
    let mut pos: Vec<f32> = Vec::new();
    let mut nor: Vec<f32> = Vec::new();
    for g in geos {
        let x = if g.index.is_some() {
            g.to_non_indexed()
        } else {
            g.clone()
        };
        let arr = |name: &str| -> Vec<f32> {
            let a = x.get_attribute(name).expect("position and normal");
            (0..a.count() * a.item_size)
                .map(|k| a.raw(k) as f32)
                .collect()
        };
        pos.extend(arr("position"));
        nor.extend(arr("normal"));
    }
    let mut out = BufferGeometry::new();
    out.set_attribute("position", BufferAttribute::from_f32(pos, 3));
    out.set_attribute("normal", BufferAttribute::from_f32(nor, 3));
    out
}

/// The strip's geometry across `w` metres: a dark base with two rows of
/// small steel pyramids.
pub fn spike_geometry(w: f64) -> BufferGeometry {
    let mut parts = Vec::new();
    let mut base = box_geometry(w, 0.03, 0.34, 1.0, 1.0, 1.0);
    base.translate(0.0, 0.02, 0.0);
    parts.push(base);
    let n = js::round(w / 0.16);
    let mut i = 0.0;
    while i < n {
        for z in [-0.09, 0.09] {
            let mut sp =
                cone_geometry(0.03, 0.09, 4.0, 1.0, false, 0.0, std::f64::consts::PI * 2.0);
            sp.translate(
                -w / 2.0 + (i + 0.5) * (w / n) + if z > 0.0 { 0.04 } else { 0.0 },
                0.08,
                z,
            );
            parts.push(sp);
        }
        i += 1.0;
    }
    merge_all(&parts)
}

/// `spikeMat`: dark steel, made once per graph.
pub fn spike_material(graph: &mut SceneGraph, kit: &mut PropKit) -> MaterialId {
    *kit.spike_mat.get_or_insert_with(|| {
        graph.add_material(
            Material::standard()
                .set("color", 0x3a3d42)
                .set("metalness", 0.7)
                .set("roughness", 0.35),
        )
    })
}

/// `spikeStrip(track, s, lat0, lat1)`: a spike strip across the road from
/// `lat0` to `lat1` at `s`, as a mesh not yet added to any parent.
pub fn spike_strip(
    graph: &mut SceneGraph,
    kit: &mut PropKit,
    track: &Track,
    s: f64,
    lat0: f64,
    lat1: f64,
) -> NodeId {
    let mat = spike_material(graph, kit);
    let f = track.frame(s);
    let geo = graph.add_geometry(spike_geometry(lat1 - lat0));
    let mesh = graph.mesh(geo, mat);
    let c = (lat0 + lat1) / 2.0;
    let p = track.point_at(s, c);
    let o = graph.get_mut(mesh);
    o.position.x = p.x;
    o.position.y = p.y + 0.02;
    o.position.z = p.z;
    // Local X across the road (the frame's right vector), Z along it.
    o.set_rotation(&Euler::new(0.0, -kernel::atan2(f.rz, f.rx), 0.0));
    o.receive_shadow = true;
    mesh
}
