//! `TrackGround`: the road as `mp_vdyn`'s `Ground`. Its height is the
//! arcade's `surface_y`, its normal leans with the bank and the grade, the
//! signed distance has the right sign, and the walls face the road.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

use mp_sim::ground::{SURFACE_ROAD, TrackGround};
use mp_sim::race::LevelRuntime;
use mp_track::Track;
use mp_vdyn::{Aabb, Collider, Ground, Vec3};

fn track(id: &str) -> std::sync::Arc<Track> {
    LevelRuntime::new(mp_levels::level_by_id(id)).unwrap().track
}

#[test]
fn height_is_the_arcades_surface_and_the_normal_follows_bank_and_grade() {
    for id in ["sierra", "coast", "desert"] {
        let t = track(id);
        let mut checked = 0;
        let mut s = 5.0;
        while s < t.length - 5.0 {
            for lat in [-3.0, 0.0, 2.5] {
                let p = t.point_at(s, lat);
                let g = TrackGround::new(&t, s);
                let h = g.height(p.x, p.z);
                let pr = t.project(p.x, p.z, s);
                assert_eq!(h.y, t.surface_y(pr.s, pr.lat), "{id} s {s}");
                let n = h.normal;
                assert!((n.length() - 1.0).abs() < 1e-12);
                assert!(n.y > 0.9, "{id} s {s}: {n:?}");
                // Uphill along the road, the normal leans back: n·f has
                // the opposite sign of the grade.
                let f = t.frame(pr.s);
                let along = n.x * f.fx + n.z * f.fz;
                if f.grade.abs() > 0.02 {
                    assert!(along * f.grade < 0.0, "{id} s {s}: grade {}", f.grade);
                }
                // Banked down to the right (bank > 0), it leans right.
                let across = n.x * f.rx + n.z * f.rz;
                if f.bank.abs() > 0.02 && lat.abs() < f.hw {
                    assert!(across * f.bank > 0.0, "{id} s {s}: bank {}", f.bank);
                }
                assert_eq!(h.surface.id, SURFACE_ROAD);
                checked += 1;
            }
            s += 37.0;
        }
        assert!(checked > 30);
    }
}

#[test]
fn signed_distance_and_walls() {
    let t = track("sierra");
    let s = 200.0;
    let p = t.point_at(s, 1.0);
    let g = TrackGround::new(&t, s);
    let above = g.sdf(Vec3::new(p.x, p.y + 0.5, p.z));
    let below = g.sdf(Vec3::new(p.x, p.y - 0.2, p.z));
    assert!(above.dist > 0.45 && above.dist <= 0.5, "{}", above.dist);
    assert!(below.dist < -0.15 && below.dist >= -0.2, "{}", below.dist);
    let mut out = Vec::new();
    let c = Vec3::new(p.x, p.y, p.z);
    g.colliders(
        Aabb {
            min: c - Vec3::new(2.0, 1.0, 2.0),
            max: c + Vec3::new(2.0, 1.0, 2.0),
        },
        &mut out,
    );
    assert_eq!(out.len(), 2);
    for col in out {
        let Collider::Plane { point, normal } = col;
        // The car is on the open side of both walls.
        assert!((c - point).dot(normal) > 0.0);
    }
}
