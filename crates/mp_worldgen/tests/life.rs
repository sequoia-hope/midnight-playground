//! The living world (vision M14, `mp_worldgen::life`): the Coast Highway
//! and Seaside Raceway built through `level_jobs_with` with the life
//! module, then its animator run frame by frame.
//!
//! - Birds fly by day near the camera and roost at night; the coast has
//!   its pelicans over the sea.
//! - People stand where cars can't go: never on the road.
//! - Fans cheer as the player's car comes by and settle once it has gone;
//!   Seabright's walkers move.
//! - Levels built without the module are untouched (the parity tests build
//!   them through `level_jobs`).

mod common;

use std::sync::{Mutex, OnceLock};

use mp_worldgen::life;
use mp_worldgen::object::NodeId;
use mp_worldgen::scenery::scenery_factory;
use mp_worldgen::stages::{LevelSetup, level_stages};
use mp_worldgen::world::{
    Build, CameraView, Change, Edit, Handle, UpdateCtx, World, level_jobs_with,
};

fn build(id: &str) -> World {
    let setup = LevelSetup {
        terrain: common::terrain_setup(id),
        road: None,
    };
    let mut b = Build::new(
        common::world(id),
        level_jobs_with(level_stages(setup), scenery_factory(None), life::modules),
    );
    while !b.is_done() {
        b.step().expect("the level builds");
    }
    assert!(
        b.world.graph.log.is_empty(),
        "{id} logged {:?}",
        b.world.graph.log
    );
    b.world
}

fn coast() -> &'static Mutex<World> {
    static W: OnceLock<Mutex<World>> = OnceLock::new();
    W.get_or_init(|| Mutex::new(build("coast")))
}

fn seaside() -> &'static Mutex<World> {
    static W: OnceLock<Mutex<World>> = OnceLock::new();
    W.get_or_init(|| Mutex::new(build("seaside")))
}

/// A named group's instanced meshes.
fn group(w: &World, name: &str) -> Vec<NodeId> {
    let g = w
        .graph
        .objects
        .iter()
        .position(|o| o.name == name)
        .unwrap_or_else(|| panic!("no {name}"));
    w.graph.objects[g].children.clone()
}

fn ctx(w: &World, s: f64, dt: f64, night: f64) -> UpdateCtx {
    let t = w.track.as_ref().expect("a track");
    let f = t.frame(t.wrap(s));
    UpdateCtx {
        dt,
        night,
        camera: Some(CameraView {
            position: [f.x, f.y + 3.0, f.z],
            fov: 60.0,
            viewport_height: 1080.0,
        }),
        s,
    }
}

/// Runs every animator for one frame.
fn frame(w: &mut World, u: &UpdateCtx) -> Vec<Edit> {
    let mut out = Vec::new();
    for a in &mut w.animators {
        a.update(u, &mut out);
    }
    out
}

fn on(e: &Edit, nodes: &[NodeId]) -> bool {
    matches!(e.target, Handle::Node(n) if nodes.contains(&n))
}

fn count(edits: &[Edit], node: NodeId) -> Option<u32> {
    edits
        .iter()
        .rev()
        .find_map(|e| match (e.target, &e.change) {
            (Handle::Node(n), Change::InstanceCount(c)) if n == node => Some(*c),
            _ => None,
        })
}

/// Where each person stands at build time: the torso's translation.
fn people(w: &World) -> Vec<[f64; 3]> {
    let nodes = group(w, "life:people");
    let inst = w.graph.objects[nodes[2].0 as usize]
        .instances
        .as_ref()
        .expect("instanced");
    (0..inst.count as usize)
        .map(|i| {
            let m = inst.get_matrix_at(i).elements;
            [m[12], m[13], m[14]]
        })
        .collect()
}

#[test]
fn birds_fly_by_day_and_roost_at_night() {
    let mut w = coast().lock().unwrap();
    let birds = group(&w, "life:birds");
    assert_eq!(birds.len(), 3, "a body and two wings");
    let mut seen = 0;
    for k in 0..40 {
        let u = ctx(&w, 500.0 + f64::from(k) * 120.0, 1.0 / 60.0, 0.1);
        let e = frame(&mut w, &u);
        seen = seen.max(count(&e, birds[0]).unwrap_or(0));
        assert!(e.iter().filter(|e| on(e, &birds)).all(|e| match &e.change {
            Change::InstanceMatrix { matrix, .. } => matrix.iter().all(|v| v.is_finite()),
            _ => true,
        }));
    }
    assert!(
        seen >= 5,
        "birds near the camera somewhere along the coast ({seen})"
    );
    let u = ctx(&w, 3000.0, 1.0 / 60.0, 0.95);
    let e = frame(&mut w, &u);
    assert_eq!(count(&e, birds[0]), Some(0), "roosting at night");
}

#[test]
fn pelicans_fly_along_the_coast_beside_the_player() {
    let mut w = coast().lock().unwrap();
    let birds = group(&w, "life:birds");
    // Drive the coast at 20 m/s: the pelicans, near the player, are always
    // among the birds drawn, and their bodies are over the sea side.
    let t = w.track.as_ref().unwrap().clone();
    let start = t.zone_start[0] as f64 + 400.0;
    let mut drawn_min = u32::MAX;
    for k in 0..300 {
        let s = start + f64::from(k) * 20.0 / 10.0;
        let u = ctx(&w, s, 0.1, 0.2);
        let e = frame(&mut w, &u);
        drawn_min = drawn_min.min(count(&e, birds[0]).unwrap_or(0));
    }
    assert!(
        drawn_min >= 7,
        "the V of seven is always there ({drawn_min})"
    );
}

#[test]
fn nobody_stands_on_the_road() {
    for (name, w, at_least) in [("coast", coast(), 100), ("seaside", seaside(), 200)] {
        let w = w.lock().unwrap();
        let t = w.track.as_ref().unwrap();
        let ps = people(&w);
        assert!(ps.len() >= at_least, "{name}: {} people", ps.len());
        for p in ps {
            let d = t.distance_to_road(p[0], p[2], 60.0);
            let i = d.i.max(0) as usize;
            let hw = f64::from(t.hw.get(i).copied().unwrap_or(5.0));
            assert!(
                d.d > hw + 1.0,
                "{name}: someone at {p:?} is {:.1} m from the centreline (half-width {hw:.1})",
                d.d
            );
            assert!(p.iter().all(|v| v.is_finite()));
        }
    }
}

#[test]
fn fans_cheer_as_the_car_comes_by_and_settle_after() {
    let mut w = seaside().lock().unwrap();
    let nodes = group(&w, "life:people");
    let arm_r = nodes[5];
    // Racing past the start line (inside the first banner zone).
    let mut moved = 0;
    for k in 0..120 {
        let s = -100.0 + f64::from(k) * 40.0 / 60.0;
        let u = ctx(&w, s, 1.0 / 60.0, 0.0);
        let e = frame(&mut w, &u);
        moved += e.iter().filter(|e| on(e, &[arm_r])).count();
    }
    assert!(moved > 200, "fans by the start line waved ({moved})");
    // Far away: the fans who waved are put back at rest, once.
    let u = ctx(&w, 1500.0, 1.0 / 60.0, 0.0);
    let back = frame(&mut w, &u).iter().filter(|e| on(e, &[arm_r])).count();
    assert!(back > 0, "the fans settle");
    let u = ctx(&w, 1500.0, 1.0 / 60.0, 0.0);
    let again = frame(&mut w, &u).iter().filter(|e| on(e, &[arm_r])).count();
    assert_eq!(again, 0, "and stay settled");
}

#[test]
fn seabright_walkers_walk() {
    let mut w = coast().lock().unwrap();
    let nodes = group(&w, "life:people");
    let t = w.track.as_ref().unwrap();
    let z = w
        .level
        .zones
        .iter()
        .position(|z| z.scenery == "Beach")
        .unwrap();
    let mid = t.zone_start[z] as f64 + 900.0;
    let u = ctx(&w, mid, 1.0 / 60.0, 0.0);
    let a = frame(&mut w, &u);
    let walked: Vec<&Edit> = a.iter().filter(|e| on(e, &[nodes[2]])).collect();
    assert!(
        walked.len() > 10,
        "walkers near the camera move ({})",
        walked.len()
    );
    let pos = |e: &[Edit], idx: u32| {
        e.iter().find_map(|x| match (&x.target, &x.change) {
            (Handle::Node(n), Change::InstanceMatrix { index, matrix })
                if *n == nodes[2] && *index == idx =>
            {
                Some([matrix[12], matrix[14]])
            }
            _ => None,
        })
    };
    let Change::InstanceMatrix { index, .. } = walked[0].change else {
        unreachable!()
    };
    let p0 = pos(&a, index).unwrap();
    for _ in 0..60 {
        frame(&mut w, &u);
    }
    let b = frame(&mut w, &u);
    let p1 = pos(&b, index).unwrap();
    let d = ((p1[0] - p0[0]).powi(2) + (p1[1] - p0[1]).powi(2)).sqrt();
    assert!(
        (0.6..2.5).contains(&d),
        "a second's walk is a metre or so ({d})"
    );
}
