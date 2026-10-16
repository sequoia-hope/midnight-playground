//! The road as `mp_vdyn`'s [`Ground`] (docs/vehicle-dynamics/SPEC.md 6): a
//! sim car's tyres and chassis query the track through this adapter.
//!
//! It reads only what the arcade physics reads (`project`, `surface_y`,
//! `frame`'s walls, `run_l` and `loose_at`; SPEC 9 breadcrumb 1), so the
//! track stays a ribbon behind a small interface.

use mp_track::Track;
use mp_vdyn::{Aabb, Collider, Ground, GroundSample, SdfSample, Surface, Vec3};
use pk_math::clamp;

/// Surface ids a sim car's tyres report.
pub const SURFACE_ROAD: u16 = 0;
pub const SURFACE_RUNOFF: u16 = 1;

/// Steps of the finite differences that give the surface's normal (m):
/// across the road, and along it (the track is sampled every metre).
const D_LAT: f64 = 0.05;
const D_S: f64 = 0.5;

/// The track as ground, near arc length `hint` on road `road` of the
/// level's network (where the car is: every projection searches from
/// there, as the arcade physics does).
pub struct TrackGround<'a> {
    pub track: &'a Track,
    pub road: usize,
    pub hint: f64,
}

impl<'a> TrackGround<'a> {
    pub fn new(track: &'a Track, road: usize, hint: f64) -> TrackGround<'a> {
        TrackGround { track, road, hint }
    }

    /// The road under (x, z) and the projection onto it.
    fn on(&self, x: f64, z: f64) -> (&'a Track, mp_track::Projection) {
        let (k, p) = self.track.project_road(x, z, self.road, self.hint);
        (self.track.road(k), p)
    }

    /// How far into loose run-off (x, z) lies at lateral offset `lat` from
    /// the centreline at `hw` half-width: 0 on the road, up to 1 (the
    /// arcade physics' `off`).
    fn off(&self, t: &Track, x: f64, z: f64, p: &mp_track::Projection, hw: f64) -> f64 {
        let lat = p.lat;
        let mut off = if t.run_l.is_some() {
            clamp((lat.abs() - hw - 0.4) / 1.5, 0.0, 1.0)
        } else if t.open_ground.is_some() {
            // An open level: past the shoulder the car is on the land.
            clamp((lat.abs() - hw - t.margin[p.i] as f64) / 1.5, 0.0, 1.0)
        } else {
            0.0
        };
        if off > 0.0
            && let Some(loose_at) = &t.loose_at
        {
            off *= loose_at(x, z);
        }
        off
    }
}

impl Ground for TrackGround<'_> {
    fn height(&self, x: f64, z: f64) -> GroundSample {
        let (t, p) = self.on(x, z);
        let y = t.ground_y(x, z, p.s, p.lat);
        let f = t.frame(p.s);
        // Slopes across and along the road, from the surface itself, so
        // bank, grade and run-off all come out as the arcade sees them.
        let g_lat =
            (t.surface_y(p.s, p.lat + D_LAT) - t.surface_y(p.s, p.lat - D_LAT)) / (2.0 * D_LAT);
        let g_s = (t.surface_y(p.s + D_S, p.lat) - t.surface_y(p.s - D_S, p.lat)) / (2.0 * D_S);
        // n = t_lat × t_s, with t_s = (fx, g_s, fz), t_lat = (rx, g_lat, rz).
        let normal = Vec3::new(
            g_lat * f.fz - f.fx * g_s,
            f.fx * f.fx + f.fz * f.fz,
            -f.fz * g_s - g_lat * f.fx,
        )
        .normalize();
        let off = self.off(t, x, z, &p, f.hw);
        GroundSample {
            y,
            normal,
            surface: Surface {
                // The arcade's run-off: a third less grip (an open level's
                // land a tenth, D1217), and the tyres plough (its `off`
                // drag, as rolling resistance).
                mu: if t.open_ground.is_some() && t.run_l.is_none() {
                    1.0 - (1.0 - crate::physics::OPEN_LAND_GRIP) * off
                } else {
                    1.0 - 0.3 * off
                },
                rolling: 1.0 + 9.0 * off,
                loose: off,
                id: if off > 0.0 {
                    SURFACE_RUNOFF
                } else {
                    SURFACE_ROAD
                },
            },
        }
    }

    fn sdf(&self, p: Vec3) -> SdfSample {
        // Within the corridor the ribbon's signed distance is its height
        // difference along the normal (SPEC 6).
        let g = self.height(p.x, p.z);
        SdfSample {
            dist: (p.y - g.y) * g.normal.y,
            normal: g.normal,
            surface: g.surface,
        }
    }

    fn colliders(&self, aabb: Aabb, out: &mut Vec<Collider>) {
        let cx = (aabb.min.x + aabb.max.x) / 2.0;
        let cz = (aabb.min.z + aabb.max.z) / 2.0;
        let (t, p) = self.on(cx, cz);
        let f = t.frame(p.s);
        let y = t.surface_y(p.s, p.lat);
        // The corridor's walls as planes facing the road, from the frame
        // nearest the box's centre. An open level has none.
        if t.open_ground.is_some() {
            return;
        }
        out.push(Collider::Plane {
            point: Vec3::new(f.x + f.rx * f.wall_r, y, f.z + f.rz * f.wall_r),
            normal: Vec3::new(-f.rx, 0.0, -f.rz),
        });
        out.push(Collider::Plane {
            point: Vec3::new(f.x - f.rx * f.wall_l, y, f.z - f.rz * f.wall_l),
            normal: Vec3::new(f.rx, 0.0, f.rz),
        });
        // The ends of a point-to-point road, where the arcade stops cars
        // (one metre in from the start, three short of the runout's end).
        if !t.is_loop {
            let reach = (aabb.max.x - aabb.min.x).max(aabb.max.z - aabb.min.z);
            if p.s < 1.0 + reach {
                let e = t.frame(1.0);
                out.push(Collider::Plane {
                    point: Vec3::new(e.x, e.y, e.z),
                    normal: Vec3::new(e.fx, 0.0, e.fz),
                });
            }
            let end = t.road_end() - 3.0;
            if p.s > end - reach {
                let e = t.frame(end);
                out.push(Collider::Plane {
                    point: Vec3::new(e.x, e.y, e.z),
                    normal: Vec3::new(-e.fx, 0.0, -e.fz),
                });
            }
        }
    }
}
