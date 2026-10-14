//! The far ends of roads closed (docs/vision/peninsula.md, stage 4d). An
//! open level's roads end where the atlas's corridors end (Highway 1 at
//! Half Moon Bay on the south coast level, Highway 84 at La Honda, Stage
//! Road at Pescadero): the arcade stops a car three metres short of the
//! end (`Track::road_end`), and here the end is seen before it is felt:
//! three striped sawhorses across the lanes (the pursuit's roadblock
//! prop) and a ROAD CLOSED board on the shoulder. A loop has no end, and
//! a branch that meets another road of the network (`Track::open_end`)
//! none either.
//!
//! Built with the road, under a group `closed` of the world's root; a
//! streamed section keeps it when its road's last stretch is its own
//! (the cut goes by the drawables' centres).

use mp_canvas::Canvas;
use mp_math::kernel;
use mp_track::Track;

use crate::material::Material;
use crate::mountain::canvas::{FONT, canvas_tex};
use crate::object::{NodeId, SceneGraph};
use crate::pursuit_props::{PropKit, sawhorse_model};
use crate::section::Section;
use crate::terrain::Terrain;
use crate::three_geom::{Euler, box_geometry, plane_geometry};

/// Metres short of a road's end the sawhorses stand (the car's nose
/// reaches a metre short of the end).
const BACK: f64 = 1.2;
/// Sawhorses this far apart across the road.
const PITCH: f64 = 2.7;

/// The board: a work-zone orange panel reading ROAD CLOSED.
pub fn board_canvas() -> Canvas {
    let mut g = Canvas::new(256, 160);
    g.set_fill_style("#e8761a");
    g.fill_rect(0.0, 0.0, 256.0, 160.0);
    g.set_fill_style("#101010");
    g.fill_rect(10.0, 10.0, 236.0, 140.0);
    g.set_fill_style("#e8761a");
    g.fill_rect(16.0, 16.0, 224.0, 128.0);
    g.set_fill_style("#101010");
    g.set_text_align("center");
    g.set_text_baseline("middle");
    g.set_font(&format!("bold 56px {FONT}"));
    g.fill_text("ROAD", 128.0, 54.0);
    g.fill_text("CLOSED", 128.0, 110.0);
    g
}

/// Closes every road of the network that ends, on an open level (a
/// point-to-point level's road ends at its finish, as it always has).
pub fn build(
    graph: &mut SceneGraph,
    root: NodeId,
    track: &Track,
    terrain: &Terrain,
    section: Option<&Section>,
) {
    if !track.level.open {
        return;
    }
    let ends: Vec<(usize, &Track)> = track
        .roads()
        .enumerate()
        .filter(|(_, t)| !t.is_loop && !t.open_end && t.length > 2.0 * BACK)
        .collect();
    if ends.is_empty() {
        return;
    }
    let mut kit = PropKit::default();
    let mut board = None;
    let group = graph.group("closed");
    let mut any = false;
    for (r, t) in ends {
        // A streamed section builds only its own road's end.
        if let Some(sec) = section
            && sec.partition
            && (sec.road != r || !sec.holds_on(t, t.length - BACK))
        {
            continue;
        }
        let s = t.length - BACK;
        let f = t.frame(s);
        let yaw = kernel::atan2(f.rx, f.rz);
        let mut lat = -f.hw + 1.4;
        while lat <= f.hw - 1.4 {
            let m = sawhorse_model(graph, &mut kit);
            let o = graph.get_mut(m.root);
            o.position.x = f.x + f.rx * lat;
            o.position.y = t.surface_y(s, lat);
            o.position.z = f.z + f.rz * lat;
            o.set_rotation(&Euler::new(0.0, yaw, 0.0));
            graph.add(group, m.root);
            lat += PITCH;
        }
        // The board on the right shoulder, facing the car.
        let (bg, pg, bm, pm) = *board.get_or_insert_with(|| {
            let tex = canvas_tex(graph, &board_canvas());
            let bm = graph.add_material(
                Material::standard()
                    .set("map", tex)
                    .set("roughness", 0.6)
                    .set("emissive", 0xffffff)
                    .set("emissiveMap", tex)
                    .set("emissiveIntensity", 0.25),
            );
            let pm = graph.add_material(
                Material::standard()
                    .set("color", 0x8a8a84)
                    .set("roughness", 0.8),
            );
            (
                graph.add_geometry(plane_geometry(1.6, 1.0, 1.0, 1.0)),
                graph.add_geometry(box_geometry(0.08, 1.6, 0.08, 1.0, 1.0, 1.0)),
                bm,
                pm,
            )
        });
        let lat = f.hw + f.margin.max(0.6) + 0.9;
        let base = terrain.height_at(f.x + f.rx * lat, f.z + f.rz * lat);
        let post = graph.mesh(pg, pm);
        {
            let o = graph.get_mut(post);
            o.position.x = f.x + f.rx * lat;
            o.position.y = base + 0.8;
            o.position.z = f.z + f.rz * lat;
        }
        graph.add(group, post);
        let b = graph.mesh(bg, bm);
        {
            let o = graph.get_mut(b);
            o.position.x = f.x + f.rx * lat;
            o.position.y = base + 1.7;
            o.position.z = f.z + f.rz * lat;
            // The plane faces +Z; turned to face back along the road.
            o.set_rotation(&Euler::new(0.0, kernel::atan2(-f.fx, -f.fz), 0.0));
        }
        graph.add(group, b);
        any = true;
    }
    if any {
        graph.add(root, group);
        graph.log.push("Closed: the road ends barred".into());
    }
}
