//! Car-to-car contacts (port of `src/vehicles/Collisions.js`). Each car is
//! two circles (front and rear axle) — cheap, and good enough for
//! door-to-door racing, rear-enders and T-bones.

use mp_math::{clamp, js, kernel, smoothstep};
use mp_track::Track;

use crate::body::{Body, BodySet};

/// A contact (`{ type: 'carhit', a, b, strength, x, y, z }`); `a` and `b`
/// are indices in the body list.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub a: usize,
    pub b: usize,
    pub strength: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// The two axle circles and their radius. The JS keeps them in a
/// `Float32Array`, so each value is rounded to f32 (SPEC 4.2).
fn circles(b: &dyn Body) -> [f64; 5] {
    let v = b.v();
    let yaw = v.yaw + js::or(v.visual_yaw, 0.0);
    let fx = kernel::cos(yaw);
    let fz = kernel::sin(yaw);
    let off = js::max(0.0, v.half_l - v.half_w * 1.05);
    let r = v.half_w * 1.08;
    [
        js::fround(v.x + fx * off),
        js::fround(v.z + fz * off),
        js::fround(v.x - fx * off),
        js::fround(v.z - fz * off),
        js::fround(r),
    ]
}

pub fn resolve_collisions<S: BodySet + ?Sized>(bodies: &mut S, t: &Track, events: &mut Vec<Hit>) {
    let n = bodies.count();
    for i in 0..n {
        for j in i + 1..n {
            let (a, b) = (bodies.body(i), bodies.body(j));
            if a.kinematic_only() && b.kinematic_only() && !a.crashy() && !b.crashy() {
                continue;
            }
            let (av, bv) = (a.v(), b.v());
            let dx0 = bv.x - av.x;
            let dz0 = bv.z - av.z;
            let reach = av.half_l + bv.half_l + 0.5;
            if dx0 * dx0 + dz0 * dz0 > reach * reach {
                continue;
            }
            if (av.y - bv.y).abs() > 2.5 {
                continue; // one flying over the other
            }
            let ca = circles(a);
            let cb = circles(b);
            let (mut best, mut nx, mut nz, mut px, mut pz) = (0.0, 0.0, 0.0, 0.0, 0.0);
            for ia in 0..2 {
                for ib in 0..2 {
                    let (ax, az, bx, bz) = (ca[ia * 2], ca[ia * 2 + 1], cb[ib * 2], cb[ib * 2 + 1]);
                    let dx = bx - ax;
                    let dz = bz - az;
                    let d = js::or(kernel::hypot(dx, dz), 0.001);
                    let pen = ca[4] + cb[4] - d;
                    if pen > best {
                        best = pen;
                        nx = dx / d;
                        nz = dz / d;
                        px = (ax + bx) / 2.0;
                        pz = (az + bz) / 2.0;
                    }
                }
            }
            if best <= 0.0 {
                continue;
            }
            let (ma, mb) = (a.mass(), b.mass());
            let ia = 1.0 / ma;
            let ib = 1.0 / mb;
            // Separate.
            let sa = best * ia / (ia + ib);
            let sb = best * ib / (ia + ib);
            bodies.body_mut(i).translate(t, -nx * sa, -nz * sa);
            bodies.body_mut(j).translate(t, nx * sb, nz * sb);
            // Impulse.
            let (avx, avz) = bodies.body(i).velocity();
            let (bvx, bvz) = bodies.body(j).velocity();
            let vr = (bvx - avx) * nx + (bvz - avz) * nz;
            if vr >= 0.0 {
                continue;
            }
            let e = 0.3;
            let jn = -(1.0 + e) * vr / (ia + ib);
            // Tangential friction so side-swipes drag a little.
            let tx = -nz;
            let tz = nx;
            let vt = (bvx - avx) * tx + (bvz - avz) * tz;
            let jt = clamp(-vt / (ia + ib), -jn * 0.3, jn * 0.3);
            bodies.body_mut(i).set_velocity(
                avx - (nx * jn + tx * jt) * ia,
                avz - (nz * jn + tz * jt) * ia,
            );
            bodies.body_mut(j).set_velocity(
                bvx + (nx * jn + tx * jt) * ib,
                bvz + (nz * jn + tz * jt) * ib,
            );
            // Spin from off-centre hits (2D cross product of lever arm × impulse).
            // Only real hits spin: two cars leaning on each other (door to door,
            // or one pinned against a wall) touch every frame at a crawl, and
            // spinning them each time would override the steering.
            let spin_k = 0.35 * smoothstep(1.0, 4.0, -vr);
            let (ax, az) = (bodies.body(i).v().x, bodies.body(i).v().z);
            let (bx, bz) = (bodies.body(j).v().x, bodies.body(j).v().z);
            let (ra_x, ra_z, rb_x, rb_z) = (px - ax, pz - az, px - bx, pz - bz);
            bodies
                .body_mut(i)
                .add_spin(-(ra_x * nz - ra_z * nx) * jn * ia * spin_k);
            bodies
                .body_mut(j)
                .add_spin((rb_x * nz - rb_z * nx) * jn * ib * spin_k);
            let y = (bodies.body(i).v().y + bodies.body(j).v().y) / 2.0 + 0.6;
            events.push(Hit {
                a: i,
                b: j,
                strength: clamp(-vr / 20.0, 0.0, 1.0),
                x: px,
                y,
                z: pz,
            });
        }
    }
}
