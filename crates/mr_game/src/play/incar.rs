//! The player's car out of its own bumper view (the owner's report of
//! 2026-10-05, DECISIONS D1040).
//!
//! `CameraRig`'s bumper mode puts the eye 1.2 m ahead of the car's origin
//! and 0.72 m up (and, looking back, 2.3 m behind and 1.02 m up): inside
//! the body, under the bonnet. The JS draws what that eye sees of its own
//! car: on the GT the eye sits inside the front wheel-arch liner, whose
//! inward faces fill the lower half of the view, and other kinds show
//! other insides. While the drawn eye is inside the player's car (its box
//! grown by the camera's near distance, inside which three clips the
//! model open anyway), the car's meshes are not drawn. Its root, the
//! headlight's anchor and what the effects hang on the body stay, so the
//! spot, the pools and the flames are as before.
//!
//! [`triangles`] is also what the tests ray-cast: the model as drawn, in
//! its root's frame.

use bevy::math::{DMat4, DQuat, DVec3};
use mr_scene::{NodeType, Scene};

/// The camera's near distance (`new PerspectiveCamera(62, …, 0.3, 9000)`).
pub const NEAR: f64 = 0.3;

/// One drawn triangle of a model, in its root's frame; `double` when its
/// material draws both sides (three's `side`: 0 front, 1 back, 2 double),
/// `back` when only its back faces draw.
#[derive(Clone, Copy, Debug)]
pub struct Tri {
    pub p: [DVec3; 3],
    #[cfg_attr(not(test), allow(dead_code))]
    pub double: bool,
    #[cfg_attr(not(test), allow(dead_code))]
    pub back: bool,
}

/// Every triangle the model drawn from scene node `root` shows at rest
/// (hidden nodes and their subtrees left out), in the root's frame.
pub fn triangles(scene: &Scene, root: usize) -> Vec<Tri> {
    let mut out = Vec::new();
    walk(scene, root, DMat4::IDENTITY, &mut out);
    out
}

/// Node `i`, whose matrix in the root's frame is `m` (the root's own
/// placement is the pose's, so the root is at the identity).
fn walk(scene: &Scene, i: usize, m: DMat4, out: &mut Vec<Tri>) {
    let n = &scene.nodes[i];
    if !n.visible {
        return;
    }
    if let (Some(mesh), NodeType::Mesh) = (n.mesh, n.ty) {
        let desc = &scene.meshes[mesh as usize];
        let parts: Vec<(u32, (u32, u32))> = if n.multi_material == Some(true) {
            desc.groups
                .iter()
                .enumerate()
                .filter_map(|(g, grp)| {
                    let mi = *n.materials.get(grp.material_index as usize)?;
                    Some((mi, crate::convert::draw_span(scene, desc, Some(g))))
                })
                .collect()
        } else {
            n.materials
                .first()
                .map(|&mi| vec![(mi, crate::convert::draw_span(scene, desc, None))])
                .unwrap_or_default()
        };
        if let Some(pos) = desc.attribute("position") {
            let pos = &scene.buffers[pos.accessor as usize];
            let idx = desc.index.map(|b| &scene.buffers[b as usize]);
            let vertex = |k: usize| -> DVec3 {
                let v = idx.map_or(k, |b| b.data.get(k) as usize);
                let s = pos.item_size as usize;
                m.transform_point3(DVec3::new(
                    pos.data.get(v * s),
                    pos.data.get(v * s + 1),
                    pos.data.get(v * s + 2),
                ))
            };
            // A mirroring matrix turns the winding over (three flips its
            // front face for a negative determinant).
            let mirrored = m.determinant() < 0.0;
            for (mi, (start, count)) in parts {
                let side = scene.materials[mi as usize].number("side").unwrap_or(0.0);
                let (start, count) = (start as usize, count as usize);
                for t in (start..start + count - count % 3).step_by(3) {
                    out.push(Tri {
                        p: [vertex(t), vertex(t + 1), vertex(t + 2)],
                        double: side == 2.0,
                        back: (side == 1.0) != mirrored,
                    });
                }
            }
        }
    }
    for &c in &n.children {
        let local = DMat4::from_cols_array(&scene.nodes[c as usize].matrix);
        walk(scene, c as usize, m * local, out);
    }
}

/// The box round the triangles, `(min, max)`.
pub fn bounds(tris: &[Tri]) -> (DVec3, DVec3) {
    tris.iter().flat_map(|t| t.p).fold(
        (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
        |(lo, hi), p| (lo.min(p), hi.max(p)),
    )
}

/// Whether the eye is inside the car placed at `(pos, q)` whose model's
/// box is `bounds`, grown by the near distance.
pub fn eye_inside(bounds: (DVec3, DVec3), pos: DVec3, q: DQuat, eye: DVec3) -> bool {
    let l = q.inverse() * (eye - pos);
    let (lo, hi) = (bounds.0 - DVec3::splat(NEAR), bounds.1 + DVec3::splat(NEAR));
    l.cmpge(lo).all() && l.cmple(hi).all()
}

/// The distance along the ray `o + t·d` (d of unit length) to the nearest
/// triangle it hits and that faces it as drawn (Möller–Trumbore, culled
/// as three culls), past `t_min`.
#[cfg(test)]
pub fn first_hit(tris: &[Tri], o: DVec3, d: DVec3, t_min: f64) -> Option<f64> {
    let mut best: Option<f64> = None;
    for tri in tris {
        let [a, b, c] = tri.p;
        let (e1, e2) = (b - a, c - a);
        let n = e1.cross(e2);
        let facing = d.dot(n);
        if !tri.double && ((facing < 0.0) == tri.back) {
            continue;
        }
        let pv = d.cross(e2);
        let det = e1.dot(pv);
        if det.abs() < 1e-12 {
            continue;
        }
        let inv = 1.0 / det;
        let tv = o - a;
        let u = tv.dot(pv) * inv;
        if !(0.0..=1.0).contains(&u) {
            continue;
        }
        let qv = tv.cross(e1);
        let v = d.dot(qv) * inv;
        if v < 0.0 || u + v > 1.0 {
            continue;
        }
        let t = e2.dot(qv) * inv;
        if t > t_min && best.is_none_or(|b| t < b) {
            best = Some(t);
        }
    }
    best
}

/// The triangles of node `i`'s subtree in the frame of the root above it.
#[cfg(test)]
pub fn triangles_under(scene: &Scene, i: usize) -> Vec<Tri> {
    let mut m = DMat4::IDENTITY;
    let mut k = i;
    while let Some(p) = scene.nodes[k].parent {
        m = DMat4::from_cols_array(&scene.nodes[k].matrix) * m;
        k = p as usize;
    }
    let mut out = Vec::new();
    walk(scene, i, m, &mut out);
    out
}

#[cfg(test)]
mod tests {
    //! The owner's two reports as tests: the bumper view sees the road, not
    //! the inside of the car; the tyres sit on the road at the start, not in
    //! it. Geometric: the models as built for the race, the camera as the
    //! rig places it, the pose as `Vehicle.sync` places it; nothing drawn.
    use super::*;
    use crate::play::camera::{self, CameraRig, MODES};
    use crate::play::pose::{self, Pose};
    use mr_math::kernel;
    use mr_sim::autopilot::autopilot;
    use mr_sim::input::{Input, InputFrame};
    use mr_sim::physics::CAR_SPECS;
    use mr_sim::race::{LevelRuntime, RaceOpts, SimState, step};
    use mr_worldgen::car_model::{BuildOpts, Lod, build_vehicle};
    use mr_worldgen::object::SceneGraph;
    use mr_worldgen::textures::TextureCache;

    /// The player's model as `play::spawn` asks for it (`buildVehicle(kind,
    /// { color, lod: 'high', seed: 1 })`) as a scene: the root's node and
    /// the wheels' (`[fl, fr, rl, rr]`).
    fn player_model(kind: &str) -> (Scene, usize, Vec<usize>) {
        let mut graph = SceneGraph::new();
        let mut textures = TextureCache::new();
        let opts = BuildOpts {
            lod: Some(Lod::High),
            far: false,
            color: Some(0xd8232a),
            seed: 1,
            ..BuildOpts::default()
        };
        let m = build_vehicle(&mut graph, &mut textures, kind, &opts).expect("the car builds");
        graph.roots.push(m.root);
        let (scene, handles) = graph.finish();
        let root = handles.node(m.root).unwrap() as usize;
        let wheels = m
            .wheels
            .iter()
            .map(|&w| handles.node(w).unwrap() as usize)
            .collect();
        (scene, root, wheels)
    }

    fn race(id: &str, car: &'static str) -> (LevelRuntime, SimState) {
        let mut level = mr_levels::level_by_id(id);
        if id == "seaside" {
            let p = crate::native::repo_root().join("assets/seaside/survey.bin");
            let d = mr_levels::SeasideData::parse(&std::fs::read(p).unwrap()).unwrap();
            mr_levels::seaside::prepare(&mut level, std::sync::Arc::new(d));
        }
        let lr = LevelRuntime::new(level).unwrap();
        let st = SimState::new(
            &lr,
            RaceOpts {
                car,
                seed: 1,
                pursuit: false,
                heat: 1.0,
            },
        );
        (lr, st)
    }

    /// `ticks` more of the race, the autopilot driving the player.
    fn drive(lr: &LevelRuntime, st: &mut SimState, ticks: usize) {
        let mut ev = Vec::new();
        for _ in 0..ticks {
            let mut inp = Input::default();
            autopilot(&mut inp, &st.players[0].v, &lr.track);
            step(lr, st, &[InputFrame::quantise(&inp)], &mut ev);
        }
    }

    const LEVELS: [&str; 3] = ["sierra", "coast", "streets"];
    const ASPECT: f64 = 1.6;

    /// The bumper view, ahead and looking back: rays through the lower
    /// middle of the picture (below the horizon, the middle half of its
    /// width) come down on the road without meeting the car as it is drawn.
    /// Every car kind, three levels, three moments of a race. Also checks
    /// the test's teeth: drawn whole, the GT's own wheel-arch liner is in
    /// the way (the bug).
    #[test]
    fn bumper_view_sees_the_road() {
        let bumper = MODES.iter().position(|m| m.name == "bumper").unwrap();
        let mut blocked_when_drawn = 0;
        for (kind, _) in CAR_SPECS {
            let (scene, root, _) = player_model(kind);
            let tris = triangles(&scene, root);
            let bounds = bounds(&tris);
            for level in LEVELS {
                let (lr, mut st) = race(level, kind);
                let t = &*lr.track;
                // The grid as the countdown ends, then on the move.
                let mut done = 0;
                for ticks in [480, 720, 1200] {
                    drive(&lr, &mut st, ticks - done);
                    done = ticks;
                    let v = &st.players[0].v;
                    let (pos, q) = pose::root(t, &Pose::of(v));
                    for look_back in [false, true] {
                        let mut rig = CameraRig::default();
                        while rig.mode != bumper {
                            rig.cycle();
                        }
                        let car = camera::Car::of(v);
                        let view = rig.update(1.0 / 60.0, t, &car, look_back, false, ASPECT);
                        let hidden = eye_inside(bounds, pos, q, view.eye);
                        let at = format!(
                            "{kind} on {level} at tick {ticks}{}",
                            if look_back { ", looking back" } else { "" }
                        );
                        assert!(hidden, "{at}: the car is drawn round the eye");
                        let f = (view.target - view.eye).normalize();
                        let r = f.cross(DVec3::Y).normalize();
                        let u = r.cross(f);
                        let tv = kernel::tan(view.fov.to_radians() / 2.0);
                        let th = tv * ASPECT;
                        let up = q * DVec3::Y;
                        for ny in [-0.15, -0.3, -0.5, -0.7, -0.9] {
                            for nx in [-0.5, -0.25, 0.0, 0.25, 0.5] {
                                let d = (f + r * (nx * th) + u * (ny * tv)).normalize();
                                // In the car's frame, past the near plane.
                                let (o_l, d_l) = (q.inverse() * (view.eye - pos), q.inverse() * d);
                                let hit = first_hit(&tris, o_l, d_l, NEAR / d.dot(f));
                                assert!(
                                    hidden || hit.is_none(),
                                    "{at}: the car blocks the view at ({nx}, {ny})"
                                );
                                if hit.is_some() && kind == "sports" {
                                    blocked_when_drawn += 1;
                                }
                                // The ray comes down on the road, through
                                // the lower middle of the picture.
                                if nx.abs() > 0.25 || ny > -0.3 {
                                    continue;
                                }
                                let dn = d.dot(up);
                                assert!(dn < 0.0, "{at}: ({nx}, {ny}) looks up");
                                let k = (pos - view.eye).dot(up) / dn;
                                let p = view.eye + d * k;
                                let pr = t.project(p.x, p.z, v.s);
                                let hw = t.frame(pr.s).hw;
                                assert!(
                                    k < 60.0 && pr.lat.abs() < hw + 1.0,
                                    "{at}: ({nx}, {ny}) meets the ground {k:.1} m off, \
                                     {:.1} m across a road {hw:.1} m wide",
                                    pr.lat
                                );
                            }
                        }
                    }
                }
            }
        }
        // Of 3 levels × 3 moments × 2 directions × 25 rays, the GT drawn
        // whole stops most of the forward ones.
        assert!(
            blocked_when_drawn > 100,
            "drawn whole, the GT blocks only {blocked_when_drawn} rays"
        );
    }

    /// The chase, far and intro views never hide the car.
    #[test]
    fn chase_views_keep_the_car() {
        for (kind, _) in CAR_SPECS {
            let (scene, root, _) = player_model(kind);
            let bounds = bounds(&triangles(&scene, root));
            let (lr, st) = race("coast", kind);
            let t = &*lr.track;
            let v = &st.players[0].v;
            let (pos, q) = pose::root(t, &Pose::of(v));
            let car = camera::Car::of(v);
            let mut rig = CameraRig::default();
            for k in 0..240 {
                let view = rig.update(1.0 / 60.0, t, &car, false, false, ASPECT);
                let view = rig.intro(1.0 / 60.0, &car, view);
                assert!(
                    !eye_inside(bounds, pos, q, view.eye),
                    "{kind}: intro frame {k}"
                );
            }
            for _ in 0..2 {
                for look_back in [false, true] {
                    let view = rig.update(1.0 / 60.0, t, &car, look_back, false, ASPECT);
                    assert!(
                        !eye_inside(bounds, pos, q, view.eye),
                        "{kind}: mode {}",
                        rig.mode
                    );
                }
                rig.cycle();
            }
        }
    }

    /// At the start, while the countdown's camera swings round the car,
    /// every tyre's lowest point sits on the road under it (not in it, and
    /// not floating), and no headlight pool lies over more than its lift of
    /// a tyre: the grid's pools (the row behind's lie under the row ahead)
    /// were drawn over the tyres' bottoms (D1042). Every car kind on every
    /// level, at night.
    #[test]
    fn tyres_sit_on_the_road_at_the_start() {
        use crate::play::effects::{CarIn, Effects, POOL_LIFT};
        let (mut covered, mut sunk) = (0, 0);
        for level in mr_levels::levels() {
            for (kind, _) in CAR_SPECS {
                let (scene, _, wheels) = player_model(kind);
                let (lr, st) = race(level.id, kind);
                let t = &*lr.track;
                let v = &st.players[0].v;
                let (pos, q) = pose::root(t, &Pose::of(v));
                // The racers' pools after a frame, as the JS places them and
                // as laid on the road.
                let racers: Vec<_> = st
                    .players
                    .iter()
                    .map(|p| &p.v)
                    .chain(st.rivals.iter().map(|r| &r.k.v))
                    .collect();
                let ins: Vec<CarIn> = racers
                    .iter()
                    .map(|v| CarIn {
                        x: v.x,
                        y: v.y,
                        z: v.z,
                        yaw: v.yaw,
                        visual_yaw: v.visual_yaw,
                        on_ground: true,
                        visible: true,
                        ..CarIn::default()
                    })
                    .collect();
                let mut fx = Effects::new(1);
                for k in 0..racers.len() {
                    fx.add_car(k == 0, 0);
                }
                fx.update(1.0 / 60.0, 1.0, &ins, &[]);
                let level_pools: Vec<_> = fx.cars.iter().map(|c| c.pool).collect();
                let hints: Vec<f64> = racers.iter().map(|v| v.s).collect();
                fx.lay_pools(t, &hints);
                for (k, &w) in wheels.iter().enumerate() {
                    let low = triangles_under(&scene, w)
                        .iter()
                        .flat_map(|tr| tr.p)
                        .map(|p| pos + q * p)
                        .min_by(|a, b| a.y.total_cmp(&b.y))
                        .unwrap();
                    let pr = t.project(low.x, low.z, v.s);
                    let gap = low.y - t.surface_y(pr.s, pr.lat);
                    let at = format!("{kind} on {}: wheel {k}", level.id);
                    assert!(
                        (-0.005..0.03).contains(&gap),
                        "{at}'s lowest point is {gap:+.4} m off the road"
                    );
                    // How far a pool's plane is over the tyre's lowest
                    // point, where the pool reaches it.
                    let over = |p: &crate::play::effects::Pool| {
                        let [x, y, z] = p.axes.unwrap_or([
                            [kernel::cos(p.rot_y), 0.0, -kernel::sin(p.rot_y)],
                            [0.0, 1.0, 0.0],
                            [kernel::sin(p.rot_y), 0.0, kernel::cos(p.rot_y)],
                        ]);
                        let d = low - DVec3::new(p.x, p.y, p.z);
                        let (lx, ly, lz) = (
                            d.dot(DVec3::from(x)),
                            d.dot(DVec3::from(y)),
                            d.dot(DVec3::from(z)),
                        );
                        (lx.abs() <= p.sx / 2.0 && lz.abs() <= p.sz / 2.0).then_some(-ly)
                    };
                    for (c, before) in fx.cars.iter().zip(&level_pools) {
                        if let Some(h) = over(&c.pool) {
                            covered += 1;
                            assert!(
                                h <= POOL_LIFT + 0.002,
                                "{at}: a pool lies {h:.3} m up its tyre"
                            );
                        }
                        // The teeth: as the JS places them, pools cover the
                        // bottom few centimetres of tyres.
                        if over(before).is_some_and(|h| h > 0.03) {
                            sunk += 1;
                        }
                    }
                }
            }
        }
        assert!(
            covered > 50 && sunk > 50,
            "pools under the player's tyres: {covered}, sunk as the JS lays them: {sunk}"
        );
    }
}
