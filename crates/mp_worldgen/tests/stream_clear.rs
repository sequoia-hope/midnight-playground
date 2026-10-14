//! Nothing stands on the open world's roads (D1218): in the streamed
//! sections round the start and wherever a branch meets the loop, no
//! scenery (a tree, a bush, a post, a rail) has its foot or its body on
//! any road's tarmac. The road-end barricades stand there on purpose, and
//! paint and light pools lie flat on it.

mod common;

use std::sync::Arc;

use mp_worldgen::object::NodeId;
use mp_worldgen::section::{self, Section};
use mp_worldgen::stages::LevelSetup;
use mp_worldgen::stream::Planned;
use mp_worldgen::terrain_mesh::TerrainSetup;
use mp_worldgen::three_geom::math::{Matrix4, Vector3};
use mp_worldgen::world::World;

/// The loop planned as the client plans it: the open world's scenery and
/// the living world.
fn planned() -> Planned {
    let setup = LevelSetup {
        terrain: TerrainSetup {
            plan: None,
            ..TerrainSetup::default()
        },
        road: None,
    };
    Planned::new(
        common::level("peninsula"),
        None,
        setup,
        Arc::new(mp_worldgen::scenery::open_world),
        Arc::new(mp_worldgen::life::modules),
    )
    .expect("the loop plans")
}

fn built(p: &Planned, sec: Section) -> World {
    let mut b = p.build(sec);
    while !b.is_done() {
        b.step().expect("a section's job");
    }
    let mut w = b.world;
    section::cut(&mut w);
    // What `World::finish` does before the scene is assembled.
    section::clear_branch_tarmac(&mut w);
    w
}

/// Where scenery stands on a road: (group, point, road, s, lat).
fn on_the_roads(p: &Planned, w: &World) -> Vec<(String, [f64; 3], usize, f64, f64)> {
    let roads: Vec<_> = p.track.roads().collect();
    let g = &w.graph;
    let mut out = Vec::new();
    let mut stack: Vec<(NodeId, Matrix4, String)> = g
        .roots
        .iter()
        .map(|&n| (n, Matrix4::IDENTITY, String::new()))
        .collect();
    while let Some((id, parent, group)) = stack.pop() {
        let o = g.get(id);
        let local = if o.matrix_auto_update {
            Matrix4::compose(o.position, o.quaternion, o.scale)
        } else {
            o.matrix
        };
        let world = Matrix4::multiply_matrices(&parent, &local);
        let group = if o.name.is_empty() {
            group
        } else {
            o.name.clone()
        };
        let skip = matches!(group.as_str(), "terrain" | "closed")
            || group.contains("paint")
            || group.contains("lightPools");
        if let Some(gi) = o.geometry
            && !skip
        {
            // An instance's foot (with a metre's give below the surface:
            // trees are sunk into the ground), or a mesh's every vertex
            // above the surface.
            let mut pts: Vec<(Vector3, f64)> = Vec::new();
            if let Some(inst) = &o.instances {
                for i in 0..inst.count as usize {
                    let m = inst.get_matrix_at(i);
                    if mp_worldgen::object::max_scale_on_axis(&m) > 0.0 {
                        pts.push((
                            Vector3::new(0.0, 0.0, 0.0)
                                .apply_matrix4(&m)
                                .apply_matrix4(&world),
                            -1.0,
                        ));
                    }
                }
            } else if group != "road"
                && let Some(pos) = g.geometry(gi).get_attribute("position")
            {
                for i in 0..pos.count() {
                    pts.push((pos.get_vector3(i).apply_matrix4(&world), 0.15));
                }
            }
            for (v, below) in pts {
                for (r, t) in roads.iter().enumerate() {
                    let d = t.distance_to_road(v.x, v.z, 40.0);
                    let Some(s) = d.s else { continue };
                    if !t.is_loop && !(0.0..=t.length).contains(&s) {
                        continue;
                    }
                    // A branch's first metres lie under its parent.
                    if r > 0 && s < p.track.branches[r - 1].under {
                        continue;
                    }
                    let f = t.frame(if t.is_loop { t.wrap(s) } else { s });
                    let y = t.surface_y(s, d.lat);
                    if d.lat.abs() < f.hw - 0.5 && v.y > y + below && v.y < y + 6.0 {
                        out.push((group.clone(), [v.x, v.y, v.z], r, s, d.lat));
                    }
                }
            }
        }
        for &c in &o.children {
            stack.push((c, world, group.clone()));
        }
    }
    out
}

#[test]
fn no_scenery_stands_on_the_roads() {
    let p = planned();
    let t = &p.track;
    // The ring at the start, and every junction: each branch's mouth and
    // far end on the loop, and the branch's own first and last sections.
    let k0 = p.section_at(t.start_s);
    let mut ks: Vec<i64> = vec![k0 - 1, k0, k0 + 1, k0 + 2];
    let rs = p.road_sections();
    for (bi, b) in t.branches.iter().enumerate() {
        let (first, n) = rs[bi + 1];
        ks.extend([p.section_of(0, b.s), first, first + n - 1]);
        if let Some(e) = b.end {
            ks.push(p.section_of(0, e));
        }
    }
    let mut seen = Vec::new();
    let mut bad = Vec::new();
    for k in ks {
        let k = k.rem_euclid(p.sections());
        if seen.contains(&k) {
            continue;
        }
        seen.push(k);
        let w = built(&p, p.section(k));
        for (group, v, r, s, lat) in on_the_roads(&p, &w) {
            if bad.len() < 30 {
                eprintln!(
                    "section {k}: {group} at ({:.1}, {:.1}, {:.1}) on road {r} s {s:.1} lat {lat:.2}",
                    v[0], v[1], v[2]
                );
            }
            bad.push(group);
        }
    }
    assert!(
        bad.is_empty(),
        "{} pieces of scenery on the roads in sections {seen:?}",
        bad.len()
    );
}
