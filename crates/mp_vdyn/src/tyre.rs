//! The tyre interface (SPEC 4.3) and the brush tyre (SPEC 4.4).
//!
//! A hub tells its tyre where it is and how it moves ([`HubState`]); the
//! tyre gives back the force and moment on the hub, the torque the road
//! returns about the spin axis, and readouts ([`TyreOutput`]). [`Tyre`] is an
//! enum, not a trait object, so a vehicle's state stays `Clone + PartialEq`
//! and hashable for rollback.
//!
//! Hub frame (this crate's convention, the body frame's): local `x` is the
//! rolling direction, `y` is up along the suspension axis, `z` is the axle,
//! pointing to the vehicle's right. A wheel rolling forward spins about −z;
//! `omega` is positive when it rolls forward.
//!
//! The brush model (Pacejka, *Tyre and Vehicle Dynamics*, ch. 3): bristles
//! over a contact patch of half-length `a` with a parabolic pressure
//! distribution. Slip is never found by dividing by the wheel's speed: a
//! carcass deflection per direction relaxes over a relaxation length, and
//! the slip is read from it, so the model is a spring at a standstill and
//! the classic slip model at speed.

use mp_math::smoothstep;

use crate::ground::Ground;
use crate::math::{Iso3, Vec3};

/// The carcass deflection is held to this multiple of the slip at which
/// the patch starts to slide entirely (`θs = 1`).
const SLIP_CAP: f64 = 2.0;

/// What the hub tells the tyre at the start of a substep.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HubState {
    /// Hub centre and orientation (rolling direction along local x, the
    /// axle along local z).
    pub pose: Iso3,
    /// Hub centre's linear velocity, world frame.
    pub vel: Vec3,
    /// Hub angular velocity excluding spin, world frame.
    pub ang_vel: Vec3,
    /// Spin rate about the axle (rad/s), positive rolling forward.
    pub omega: f64,
}

/// Readouts for audio, effects, telemetry and force feedback.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ContactInfo {
    pub in_contact: bool,
    /// Normal load (N).
    pub load: f64,
    /// Transient slip read from the carcass deflection: longitudinal slip
    /// ratio κ (positive driving, −1 for a locked wheel at speed) and the
    /// tangent of the slip angle (positive when the patch slides to the
    /// wheel's right).
    pub slip_ratio: f64,
    pub slip_tan: f64,
    /// Share of the patch that slides, 0..1 (1 is a full slide).
    pub sliding: f64,
    /// Speed of the patch over the ground (m/s): skids and smoke.
    pub slide_speed: f64,
    /// Speed of the wheel centre over the ground in its rolling direction,
    /// and the tread's rolling speed `ω·r_e` (m/s): the true slip ratio
    /// that ABS and traction control read is their difference over the
    /// ground speed.
    pub ground_speed: f64,
    pub roll_speed: f64,
    /// The aligning moment about the contact normal (N·m), positive about
    /// world +y for a level road.
    pub aligning: f64,
    /// Longitudinal and lateral force in the contact frame (N).
    pub fx: f64,
    pub fy: f64,
    pub point: Vec3,
    pub surface: u16,
    /// Penetration of the undeformed tyre into the ground (m).
    pub deflection: f64,
}

/// What the tyre gives back: the force and moment it applies to the hub
/// (world frame, about the hub centre), the torque about the spin axis
/// that the road returns, the rolling resistance as a friction torque
/// (magnitude, never negative: the wheel's brake logic applies it, so it
/// can stop a wheel but never spin it backwards), and readouts.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TyreOutput {
    pub force: Vec3,
    pub moment: Vec3,
    pub spin_torque: f64,
    pub rolling_torque: f64,
    pub info: ContactInfo,
}

/// The brush tyre's parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushParams {
    /// Unloaded radius (m).
    pub radius: f64,
    /// Vertical stiffness (N/m) and damping (N·s/m) of the carcass.
    pub kz: f64,
    pub cz: f64,
    /// Bristle stiffness per unit length (N/m²) and contact patch
    /// half-length (m): the slip stiffness is `C = 2·c_p·a²` (N per unit
    /// slip), the same longitudinally and laterally.
    pub cp: f64,
    pub a: f64,
    /// Peak friction at the nominal load `fz0`, and its load sensitivity:
    /// `μ = μ0·(1 − k_μ·(Fz/Fz0 − 1))`, kept above a fifth of `μ0`.
    pub mu0: f64,
    pub fz0: f64,
    pub k_mu: f64,
    /// Sliding friction as a fraction of the peak, reached as the slide
    /// speed past the peak grows well beyond `v_slide` (m/s).
    pub slide_ratio: f64,
    pub v_slide: f64,
    /// Relaxation lengths (m), lateral and longitudinal.
    pub relax_lat: f64,
    pub relax_long: f64,
    /// Damping of the bristles at a standstill (N·s/m), faded out by
    /// `v_low` (m/s): it stops a parked car ringing on its tyres.
    pub damp_lat: f64,
    pub damp_long: f64,
    pub v_low: f64,
    /// Rolling resistance coefficient: `C_rr·Fz` at the contact.
    pub c_rr: f64,
}

impl BrushParams {
    /// Slip stiffness `C = 2·c_p·a²` (N per unit slip).
    pub fn stiffness(&self) -> f64 {
        2.0 * self.cp * self.a * self.a
    }

    /// Peak friction coefficient at load `fz`, before surface and grip
    /// scaling (load sensitivity, SPEC 4.4).
    pub fn mu_at(&self, fz: f64) -> f64 {
        let k = 1.0 - self.k_mu * (fz / self.fz0 - 1.0);
        self.mu0 * if k > 0.2 { k } else { 0.2 }
    }
}

/// The steady-state brush force magnitude for the theoretical slip
/// magnitude `s` (SPEC 4.4): `μFz·(3θs − 3(θs)² + (θs)³)` while `θs < 1`,
/// then `μFz`, with `θ = C/(3μFz)`. Also returns `λ = θs`.
pub fn brush_force(c: f64, mu: f64, fz: f64, s: f64) -> (f64, f64) {
    if fz <= 0.0 || mu <= 0.0 {
        return (0.0, 0.0);
    }
    let theta = c / (3.0 * mu * fz);
    let l = theta * s;
    if l < 1.0 {
        (mu * fz * (3.0 * l - 3.0 * l * l + l * l * l), l)
    } else {
        (mu * fz, l)
    }
}

/// The brush model's pneumatic trail at `λ = θs`: `a/3` at zero slip,
/// falling to zero as the patch starts to slide entirely.
pub fn pneumatic_trail(a: f64, l: f64) -> f64 {
    if l >= 1.0 {
        return 0.0;
    }
    let m = 1.0 - l;
    a / 3.0 * (m * m * m) / (1.0 - l + l * l / 3.0)
}

/// Sliding friction: the peak `mu` while the patch still grips, falling
/// toward `mu·ratio` as the speed of the slide beyond the peak grows.
pub fn sliding_mu(mu: f64, ratio: f64, v_slide: f64, excess: f64) -> f64 {
    let e = excess / v_slide;
    mu * (ratio + (1.0 - ratio) / (1.0 + e * e))
}

/// A rigid brush tyre and its transient state.
#[derive(Clone, Debug, PartialEq)]
pub struct BrushTyre {
    pub p: BrushParams,
    /// Carcass deflections (m): longitudinal and lateral, in the direction
    /// the patch slides (the force opposes them).
    pub qx: f64,
    pub qy: f64,
    /// Scales the friction (a spiked tyre, a test): 1 normally.
    pub grip: f64,
}

impl BrushTyre {
    pub fn new(p: BrushParams) -> BrushTyre {
        BrushTyre {
            p,
            qx: 0.0,
            qy: 0.0,
            grip: 1.0,
        }
    }

    pub fn step(&mut self, hub: &HubState, ground: &dyn Ground, h: f64) -> TyreOutput {
        let p = self.p;
        let centre = hub.pose.pos;
        let g = ground.height(centre.x, centre.z);
        let n = g.normal;
        // Height of the hub over the ground plane, along its normal.
        let d = (centre.y - g.y) * n.y;
        let pen = p.radius - d;
        if pen <= 0.0 {
            self.qx = 0.0;
            self.qy = 0.0;
            return TyreOutput::default();
        }
        let c = centre - n * d;
        let fwd = hub.pose.rot.rotate(Vec3::X);
        let xc = fwd.reject(n).normalize();
        let yc = xc.cross(n);
        let vc = hub.vel + hub.ang_vel.cross(c - centre);
        let vx = vc.dot(xc);
        let vy = vc.dot(yc);

        // Vertical: a spring and damper from the hub to the ground.
        let pen_rate = -hub.vel.dot(n);
        let fz = p.kz * pen + p.cz * pen_rate;
        let fz = if fz > 0.0 { fz } else { 0.0 };

        // Rolling: the effective radius of a loaded tyre.
        let re = p.radius - pen / 3.0;
        let vr = hub.omega * re;
        let vsx = vx - vr;
        let vsy = vy;
        let avx = vx.abs();

        // Transient slip: the carcass deflection relaxes over its length
        // (implicit, so it is stable at any speed).
        self.qx = (self.qx + h * vsx) / (1.0 + h * avx / p.relax_long);
        self.qy = (self.qy + h * vsy) / (1.0 + h * avx / p.relax_lat);
        let sx = self.qx / p.relax_long;
        let sy = self.qy / p.relax_lat;

        let mu = p.mu_at(fz) * self.grip * g.surface.mu;
        let cs = p.stiffness();
        let theta = if fz > 0.0 && mu > 0.0 {
            cs / (3.0 * mu * fz)
        } else {
            0.0
        };
        // Keep the deflection from winding up past a full slide (a wheel
        // spinning at a standstill, say), so it lets go at once. A capped
        // patch slides, so its slip points along the slide; scaling the two
        // deflections alike would bend it, their relaxation lengths
        // differing.
        let s_mag = (sx * sx + sy * sy).sqrt();
        let (sx, sy) = if theta * s_mag > SLIP_CAP {
            let v_mag = (vsx * vsx + vsy * vsy).sqrt();
            let (ux, uy) = if v_mag > 0.0 {
                (vsx / v_mag, vsy / v_mag)
            } else {
                (sx / s_mag, sy / s_mag)
            };
            let cap = SLIP_CAP / theta;
            self.qx = ux * cap * p.relax_long;
            self.qy = uy * cap * p.relax_lat;
            (ux * cap, uy * cap)
        } else {
            (sx, sy)
        };

        // Theoretical slip σ = κ/(1+κ), tanα/(1+κ): the slide over the
        // rolling speed. The sign of travel fades in over half a metre a
        // second, where the deflection is a spring and this barely matters.
        let dir = mp_math::clamp(vx / 0.5, -1.0, 1.0);
        let den = 1.0 - sx * dir;
        let den = if den > 0.1 { den } else { 0.1 };
        let tx = sx / den;
        let ty = sy / den;
        let t_mag = (tx * tx + ty * ty).sqrt();
        let (f_mag, l) = brush_force(cs, mu, fz, t_mag);
        let slide_speed = (vsx * vsx + vsy * vsy).sqrt();
        let (f_mag, mu_eff) = if l >= 1.0 {
            let m = sliding_mu(mu, p.slide_ratio, p.v_slide, slide_speed * (1.0 - 1.0 / l));
            (m * fz, m)
        } else {
            (f_mag, mu)
        };
        let (mut fx, mut fy) = if t_mag > 0.0 {
            (-f_mag * tx / t_mag, -f_mag * ty / t_mag)
        } else {
            (0.0, 0.0)
        };
        // Pneumatic trail: the lateral force acts behind the patch centre.
        let trail = pneumatic_trail(p.a, l);
        let mz = trail * fy;

        // Low-speed damping of the bristles, then the friction limit.
        let low = 1.0 - smoothstep(0.0, p.v_low, avx);
        if low > 0.0 {
            fx -= p.damp_long * vsx * low;
            fy -= p.damp_lat * vsy * low;
            let f = (fx * fx + fy * fy).sqrt();
            let cap = mu_eff * fz;
            if f > cap {
                let k = cap / f;
                fx *= k;
                fy *= k;
            }
        }

        let force = n * fz + xc * fx + yc * fy;
        let moment = (c - centre).cross(force) + n * mz;
        TyreOutput {
            force,
            moment,
            spin_torque: -fx * re,
            rolling_torque: p.c_rr * fz * g.surface.rolling * re,
            info: ContactInfo {
                in_contact: true,
                load: fz,
                slip_ratio: -sx * dir,
                slip_tan: sy,
                sliding: if l < 1.0 { l } else { 1.0 },
                slide_speed,
                ground_speed: vx,
                roll_speed: vr,
                aligning: mz,
                fx,
                fy,
                point: c,
                surface: g.surface.id,
                deflection: pen,
            },
        }
    }

    pub fn hash_into(&self, out: &mut Vec<u8>) {
        for v in [self.qx, self.qy, self.grip] {
            out.extend_from_slice(&v.to_bits().to_le_bytes());
        }
    }
}

/// A tyre behind the one wheel interface (SPEC 4.3, VD-4). The soft tyre
/// (V6) and a Magic Formula tyre join this enum later.
#[derive(Clone, Debug, PartialEq)]
pub enum Tyre {
    Brush(BrushTyre),
}

impl Tyre {
    pub fn step(&mut self, hub: &HubState, ground: &dyn Ground, h: f64) -> TyreOutput {
        match self {
            Tyre::Brush(t) => t.step(hub, ground, h),
        }
    }

    pub fn radius(&self) -> f64 {
        match self {
            Tyre::Brush(t) => t.p.radius,
        }
    }

    /// Scales the tyre's friction (1 normally).
    pub fn set_grip(&mut self, grip: f64) {
        match self {
            Tyre::Brush(t) => t.grip = grip,
        }
    }

    pub fn hash_into(&self, out: &mut Vec<u8>) {
        match self {
            Tyre::Brush(t) => {
                out.push(0);
                t.hash_into(out);
            }
        }
    }
}
