//! Per-frame steering (`Audio.js` `update` and the `_steer*`/`_update*`
//! methods, the decel pops).

use super::{CarState, GameAudio, RUMBLE, clamp, or0};
use crate::engine::{self, CarProfile};
use mr_math::js;
use mr_math::kernel::{pow, sin};

impl GameAudio {
    // ── Per-frame update ─────────────────────────────────────────────
    pub fn update(&mut self, dt: f64, s: &CarState) {
        if self.paused || !self.steer() {
            return;
        }
        let t = self.now();
        let prof = self.prof.unwrap_or(&engine::SPORTS);
        let rpm_max = or0(s.rpm_max, 7800.0);
        let raw_rpm = match s.rpm {
            Some(r) if r.is_finite() => r,
            _ => 800.0,
        };
        let engine_off = raw_rpm < 100.0;
        let rpm = clamp(raw_rpm, 500.0, rpm_max * 1.05);
        let rn = clamp((rpm - 800.0) / (rpm_max - 800.0), 0.0, 1.0);
        let thr = clamp(or0(s.throttle, 0.0), 0.0, 1.0);
        let speed = or0(s.speed, 0.0).abs();
        let on_ground = s.on_ground != Some(false);
        let gear = s.gear.unwrap_or(1.0);
        let g = self.graph();
        let vg = &g.vg;
        // The menu and results screens pass a stopped engine (and no motor state):
        // nothing from a pursuit may carry on there.
        if engine_off && s.motor.is_none() {
            self.pursuit_reset();
        }
        let radio_on = self.radio_cur.as_ref().is_some_and(|c| t < c.end);
        self.gate_set(vg.radio, radio_on, t);
        self.gate_set(vg.tunnel, self.env == "tunnel", t);
        self.update_turbo(dt, s, thr, prof);
        if self.electric {
            self.update_damage(dt, 5.0 + speed * 0.6, thr, s.motor.is_some());
            self.update_electric(dt, s);
            if self.gate_set(vg.eng, false, t) {
                g.eng.out.gain.set_target_at_time(0.0, t, 0.05);
            }
            if self.gate_set(vg.whine, false, t) {
                g.eng.whine_g.gain.set_target_at_time(0.0, t, 0.05);
            }
            self.prev_throttle = thr;
            self.update_environment(s, speed, on_ground);
            return;
        }
        if self.gate_set(vg.ev, false, t) {
            g.ev.out.gain.set_target_at_time(0.0, t, 0.05);
        }
        if self.gate_set(vg.eng, !engine_off, t) {
            self.steer_engine(t, prof, rpm, rn, thr, engine_off, on_ground, rpm_max);
        }

        // Transmission whine with road speed (and a louder reverse whine).
        let rev = gear == -1.0;
        let whine = if engine_off {
            0.0
        } else if rev {
            0.05 * clamp(speed / 6.0, 0.0, 1.0)
        } else {
            prof.whine * 0.016 * clamp(speed / 35.0, 0.0, 1.0) * (0.35 + 0.65 * thr)
        };
        if self.gate_set(vg.whine, whine > 1e-5, t) {
            let e = &g.eng;
            e.whine.frequency.set_target_at_time(
                if rev {
                    250.0 + speed * 90.0
                } else {
                    90.0 + speed * 26.0
                },
                t,
                0.05,
            );
            e.whine2.frequency.set_target_at_time(
                if rev {
                    375.0 + speed * 135.0
                } else {
                    140.0 + speed * 41.0
                },
                t,
                0.05,
            );
            e.whine_g.gain.set_target_at_time(whine, t, 0.08);
        }

        // Decel pops/crackle on lift-off at high revs.
        self.pop_cooldown -= dt;
        if !engine_off {
            if self.prev_throttle > 0.5 && thr < 0.15 && rn > 0.5 {
                let n = js::round((3.0 + self.random() * 4.0) * prof.pops);
                self.pop_burst(n, rn);
            } else if thr < 0.1
                && rn > 0.42
                && self.pop_cooldown <= 0.0
                && self.random() < dt * 1.6 * prof.pops
            {
                let at = t + self.random() * 0.05;
                self.pop(at, rn * 0.7);
                self.pop_cooldown = 0.12;
            }
        }
        self.prev_throttle = thr;
        self.update_damage(dt, rpm / 60.0, thr, !engine_off);
        self.update_environment(s, speed, on_ground);
    }

    /// The combustion engine's pitch, load crossfade, level and limiter.
    #[allow(clippy::too_many_arguments)]
    fn steer_engine(
        &mut self,
        t: f64,
        prof: &CarProfile,
        rpm: f64,
        rn: f64,
        thr: f64,
        engine_off: bool,
        on_ground: bool,
        rpm_max: f64,
    ) {
        let g = self.graph();
        let e = &g.eng;
        // Pitch: the wave holds one 720° cycle, so it plays at rpm / 120.
        let fc = rpm / 120.0;
        let k = 0.022;
        for c in [&e.l, &e.r] {
            c.on.frequency.set_target_at_time(fc, t, k);
            c.off.frequency.set_target_at_time(fc, t, k);
        }
        e.rum.frequency.set_target_at_time(fc, t, k);
        // Load crossfade: sharp on-throttle pulses vs soft overrun burble.
        let load = pow(thr, 0.7);
        let g_on = 0.12 + 0.88 * load;
        let g_off = 0.75 * (1.0 - load) + 0.08;
        let drive = prof.drive * (0.65 + 0.7 * load + 0.35 * rn);
        let lp = prof.lp_base + prof.lp_range * (0.25 * rn + 0.45 * load * (0.4 + 0.6 * rn));
        for (c, right) in [(&e.l, false), (&e.r, true)] {
            c.g_on.gain.set_target_at_time(g_on, t, 0.04);
            c.g_off.gain.set_target_at_time(g_off, t, 0.06);
            c.drive.gain.set_target_at_time(drive, t, 0.05);
            c.lp.frequency
                .set_target_at_time(if right { lp * 1.06 } else { lp }, t, 0.04);
        }
        let ff = fc * prof.cyl as f64; // firing frequency
        e.in1
            .frequency
            .set_target_at_time(clamp(ff * 1.1, 60.0, 8000.0), t, 0.03);
        e.in2
            .frequency
            .set_target_at_time(clamp(ff * 2.3, 120.0, 10000.0), t, 0.03);
        e.in_g.gain.set_target_at_time(
            prof.intake * (0.04 + 0.5 * load * (0.3 + 0.7 * rn)),
            t,
            0.05,
        );
        e.rasp_bp
            .frequency
            .set_target_at_time(1800.0 + rn * 2600.0, t, 0.05);
        e.rasp_depth
            .gain
            .set_target_at_time(prof.rasp * 0.1 * load * (0.3 + 0.7 * rn), t, 0.05);
        // Near idle almost the whole rumble wave gets through the low-pass, so ease it back there.
        e.rum_g.gain.set_target_at_time(
            prof.rumble * RUMBLE * (0.55 + 0.45 * load) * (0.65 + 0.35 * js::min(1.0, rn * 4.0)),
            t,
            0.05,
        );

        // Level: idle burble is modest, full-load high rpm is loud.
        let vol = if engine_off {
            0.0
        } else {
            prof.trim * (0.45 + 0.27 * load + 0.2 * rn)
        };
        e.out
            .gain
            .set_target_at_time(vol, t, if engine_off { 0.15 } else { 0.05 });

        // Rev limiter bounce.
        let limiting = !engine_off && rpm >= rpm_max * 0.975 && thr > 0.6 && on_ground;
        if limiting != self.limiting {
            self.limiting = limiting;
            e.lim_g
                .gain
                .set_target_at_time(if limiting { 0.7 } else { 1.0 }, t, 0.01);
            e.lim_depth
                .gain
                .set_target_at_time(if limiting { 0.3 } else { 0.0 }, t, 0.01);
        }
    }

    // Engine distress from setDamage(): nothing below 0.5, then knock and
    // rattle at crank rate, random misfires, and steam above 0.8.
    fn update_damage(&mut self, dt: f64, rate: f64, load: f64, running: bool) {
        let t = self.now();
        let g = self.graph();
        let dm = &g.dmg;
        let d = if running { self.damage } else { 0.0 };
        let k = clamp((d - 0.5) / 0.5, 0.0, 1.0);
        if !self.gate_set(g.vg.dmg, k > 0.0, t) {
            return;
        }
        dm.out
            .gain
            .set_target_at_time(if k > 0.0 { 1.0 } else { 0.0 }, t, 0.1);
        dm.pulse
            .frequency
            .set_target_at_time(clamp(rate, 4.0, 140.0), t, 0.03);
        dm.knock
            .gain
            .set_target_at_time(0.5 * k * (0.5 + 0.5 * load), t, 0.08);
        dm.rattle.gain.set_target_at_time(0.1 * k * k, t, 0.08);
        let steam = clamp((d - 0.8) / 0.2, 0.0, 1.0);
        dm.steam
            .gain
            .set_target_at_time(0.045 * steam * (0.8 + 0.2 * sin(t * 2.3)), t, 0.2);
        self.misfire_t -= dt;
        if k > 0.0 && !self.electric && self.misfire_t <= 0.0 && self.random() < dt * 3.0 * k {
            // The engine stumbles for a few cycles; sometimes it spits a bang.
            let mg = &g.eng.mis_g.gain;
            let len = 0.04 + self.random() * 0.08;
            mg.cancel_scheduled_values(t);
            mg.set_target_at_time(0.2, t, 0.008);
            mg.set_target_at_time(1.0, t + len, 0.03);
            if self.random() < 0.4 {
                self.pop(t + len * 0.5, 0.3 + 0.4 * k);
            }
            self.misfire_t = len + 0.15;
        }
    }

    fn update_turbo(&mut self, _dt: f64, s: &CarState, thr: f64, prof: &CarProfile) {
        let t = self.now();
        let g = self.graph();
        let tb = &g.turbo;
        let on = prof.turbo != 0.0 && !self.electric;
        let boost = if on {
            clamp(or0(s.boost, 0.0), 0.0, 1.0)
        } else {
            0.0
        };
        let rn = clamp((or0(s.rpm, 800.0) - 800.0) / 7000.0, 0.0, 1.0);
        if self.gate_set(g.vg.turbo, boost > 0.001, t) {
            tb.out
                .gain
                .set_target_at_time(if on { 1.0 } else { 0.0 }, t, 0.1);
            let wf = 1900.0 + boost * 3600.0 + rn * 900.0;
            tb.w1.frequency.set_target_at_time(wf, t, 0.08);
            tb.w2.frequency.set_target_at_time(wf * 1.505, t, 0.08);
            tb.wg
                .gain
                .set_target_at_time(0.045 * boost * boost * (0.4 + 0.6 * thr), t, 0.06);
            tb.hiss_bp
                .frequency
                .set_target_at_time(1400.0 + boost * 2600.0, t, 0.06);
            tb.hg.gain.set_target_at_time(0.1 * boost * thr, t, 0.06);
        }
        // Blow-off: lifting with boost up vents it.
        if on && self.prev_throttle > 0.5 && thr < 0.2 && self.boost > 0.35 {
            self.blow_off(self.boost);
        }
        self.boost = boost;
    }

    fn update_electric(&mut self, _dt: f64, s: &CarState) {
        let t = self.now();
        let g = self.graph();
        let ev = &g.ev;
        // Menu / results pass no motor state: switched off.
        if !self.gate_set(g.vg.ev, s.motor.is_some(), t) {
            return;
        }
        let motor = clamp(s.motor.unwrap_or(0.0), 0.0, 1.05);
        let speed = or0(s.speed, 0.0).abs();
        let load = clamp(or0(s.power, 0.0) / 600.0, 0.0, 1.0);
        let regen = clamp(or0(s.regen, 0.0), 0.0, 1.0);
        let boost = if s.nitro == Some(true) { 1.0 } else { 0.0 };
        let moving = clamp(motor * 25.0, 0.0, 1.0);
        // The owner's request (DEVIATIONS, D1061): the motor's whine is
        // hushed, a faint one pulling away and almost none at speed, where
        // the wind and the road carry it (`EV_WIND`).
        let hush = ev_hush(motor);
        let f1 = 45.0 + motor * 1450.0;
        let k = 0.03;
        ev.f1.o.frequency.set_target_at_time(f1, t, k);
        ev.f2.o.frequency.set_target_at_time(f1 * 2.003, t, k);
        ev.f3.o.frequency.set_target_at_time(f1 * 3.02, t, k);
        ev.mesh.o.frequency.set_target_at_time(f1 * 4.37, t, k);
        ev.regen.o.frequency.set_target_at_time(f1 * 1.5, t, k);
        ev.saw.o.frequency.set_target_at_time(f1 * 0.5, t, k);
        ev.saw_lp.frequency.set_target_at_time(
            js::min(9000.0, 120.0 + f1 * (2.2 + 1.4 * load)),
            t,
            k,
        );
        ev.growl.o.frequency.set_target_at_time(f1 * 0.25, t, k);
        ev.growl_bp
            .frequency
            .set_target_at_time(200.0 + f1 * 0.9, t, k);
        ev.air_bp
            .frequency
            .set_target_at_time(500.0 + f1 * 1.7, t, 0.05);
        let tc = 0.05;
        ev.f1.g.gain.set_target_at_time(
            0.06 * moving * (0.3 + 0.7 * load) * (1.0 - 0.4 * regen) * hush,
            t,
            tc,
        );
        ev.f2
            .g
            .gain
            .set_target_at_time(0.028 * moving * (0.2 + 0.8 * load) * hush, t, tc);
        ev.f3
            .g
            .gain
            .set_target_at_time(0.012 * moving * load * hush, t, tc);
        ev.mesh
            .g
            .gain
            .set_target_at_time(0.006 * moving * (0.3 + load + regen) * hush, t, tc);
        ev.regen
            .g
            .gain
            .set_target_at_time(0.03 * regen * moving * hush, t, tc);
        ev.saw
            .g
            .gain
            .set_target_at_time(0.03 * moving * (0.25 + 0.75 * load) * hush, t, tc);
        ev.growl.g.gain.set_target_at_time(
            0.06 * boost * moving,
            t,
            if boost != 0.0 { 0.04 } else { 0.15 },
        );
        ev.air_g.gain.set_target_at_time(
            0.05 * clamp(speed / 50.0, 0.0, 1.0) * (0.4 + 0.6 * load),
            t,
            0.08,
        );
        let idle = 1.0 - clamp(motor * 40.0, 0.0, 1.0);
        ev.hum.g.gain.set_target_at_time(0.01 * idle, t, 0.2);
        ev.hum2.g.gain.set_target_at_time(0.004 * idle, t, 0.2);
        ev.out
            .gain
            .set_target_at_time(if s.motor.is_none() { 0.0 } else { 3.4 }, t, 0.1);
    }

    // s.offroad (0..1, how far onto the verge), s.slip (slip angle, rad) and
    // s.scrapeSide (-1 left / 1 right) are optional; older callers still work.
    fn update_environment(&mut self, s: &CarState, speed: f64, on_ground: bool) {
        let t = self.now();
        let g = self.graph();
        let vg = &g.vg;
        let off = if on_ground {
            clamp(or0(s.offroad, 0.0), 0.0, 1.0)
        } else {
            0.0
        };
        if self.gate_set(vg.road, speed > 0.05, t) {
            // Wind: builds with the square of speed; louder in the air.
            let sp = clamp(speed / 80.0, 0.0, 1.3);
            let air = if on_ground { 1.0 } else { 1.35 };
            // The electric car's near-silent motor leaves the wind and the
            // road to carry its speed: both louder, more so the faster
            // (D1061); the road rumble by half as many dB. 1 for every other
            // car, so their sound is the JS's.
            let ev = if self.electric { ev_wind(speed) } else { 1.0 };
            g.wind
                .g
                .gain
                .set_target_at_time(0.2 * sp * sp * air * ev, t, 0.1);
            g.wind
                .f
                .frequency
                .set_target_at_time(350.0 + speed * 22.0, t, 0.1);
            for w in &g.wind_hi {
                w.bed.g.gain.set_target_at_time(
                    0.06 * pow(clamp((speed - 12.0) / 70.0, 0.0, 1.3), 2.2) * air * ev,
                    t,
                    0.12,
                );
                w.bed
                    .f
                    .frequency
                    .set_target_at_time(900.0 + speed * 28.0, t, 0.15);
            }
            g.rumble.g.gain.set_target_at_time(
                if on_ground {
                    (0.13 + 0.22 * off) * clamp(speed / 50.0, 0.0, 1.0) * ev.sqrt()
                } else {
                    0.0
                },
                t,
                0.05,
            );
            g.rumble
                .f
                .frequency
                .set_target_at_time(80.0 + speed * 1.5 + off * 60.0, t, 0.1);
        }

        // Squeal follows the skid amount; the slip angle and speed raise its pitch.
        // Off the tarmac tyres don't squeal, they plough.
        let skid = if on_ground {
            clamp(or0(s.skid, 0.0), 0.0, 1.0) * (1.0 - off)
        } else {
            0.0
        };
        let sq_lvl = 0.17 * pow(skid, 1.3) * clamp(speed / 6.0, 0.0, 1.0);
        if self.gate_set(vg.squeal, sq_lvl > 0.0, t) {
            let slip = clamp(or0(s.slip, 0.0).abs(), 0.0, 0.9);
            let sq = &g.squeal;
            let f = 780.0 + slip * 520.0 + clamp(speed, 0.0, 70.0) * 3.5 + skid * 120.0;
            sq.out
                .gain
                .set_target_at_time(sq_lvl, t, if skid > 0.05 { 0.035 } else { 0.07 });
            sq.o1.frequency.set_target_at_time(f, t, 0.08);
            sq.o2.frequency.set_target_at_time(f * 2.03, t, 0.08);
            sq.tbp.frequency.set_target_at_time(f * 1.15, t, 0.08);
            sq.nbp.frequency.set_target_at_time(f * 1.05, t, 0.08);
            sq.nbp2.frequency.set_target_at_time(f * 2.2, t, 0.08);
            // A light, cornering squeal is mostly tone; a big slide is mostly scrub noise.
            sq.tone.gain.set_target_at_time(0.75 - 0.35 * skid, t, 0.1);
        }

        // Gravel.
        let gv = off * clamp(speed / 14.0, 0.0, 1.0);
        if self.gate_set(vg.gravel, gv > 0.0, t) {
            g.gravel.g.gain.set_target_at_time(0.3 * gv, t, 0.06);
            g.gravel.src.playback_rate.set_target_at_time(
                clamp(0.55 + speed / 45.0, 0.5, 1.9),
                t,
                0.1,
            );
        }

        // Wall scrape, from the side that's touching.
        let scrape = clamp(or0(s.scrape, 0.0), 0.0, 1.0);
        if self.gate_set(vg.scrape, scrape > 0.0, t) {
            let sc = &g.scrape;
            sc.g.gain.set_target_at_time(0.32 * scrape, t, 0.03);
            sc.src
                .playback_rate
                .set_target_at_time(clamp(0.7 + speed / 60.0, 0.6, 1.8), t, 0.05);
            sc.grit.g.gain.set_target_at_time(0.1 * scrape, t, 0.03);
            sc.grit.f.frequency.set_target_at_time(
                1800.0 + clamp(speed, 0.0, 60.0) * 25.0,
                t,
                0.05,
            );
            if let Some(side) = s.scrape_side
                && side != 0.0
                && !side.is_nan()
            {
                sc.pan
                    .pan
                    .set_target_at_time(0.55 * js::sign(side), t, 0.05);
            }
        }

        // Nitro hiss, rumble and flame (the electric car's boost has its own growl).
        let nitro = if s.nitro == Some(true) { 1.0 } else { 0.0 };
        if self.gate_set(vg.nitro, nitro > 0.0, t) {
            let comb = if self.electric { 0.0 } else { nitro };
            let on = nitro != 0.0;
            g.nitro_hiss.g.gain.set_target_at_time(
                (if self.electric { 0.035 } else { 0.07 }) * nitro,
                t,
                if on { 0.04 } else { 0.15 },
            );
            g.nitro_rumble
                .g
                .gain
                .set_target_at_time(0.24 * comb, t, if on { 0.05 } else { 0.2 });
            g.nitro_flame
                .g
                .gain
                .set_target_at_time(0.12 * comb, t, if on { 0.05 } else { 0.2 });
            g.nitro_flutter
                .gain
                .set_target_at_time(0.06 * comb, t, 0.05);
            g.nitro_flame
                .f
                .frequency
                .set_target_at_time(300.0 + speed * 5.0, t, 0.1);
        }
    }

    fn pop_burst(&mut self, n: f64, strength: f64) {
        let t0 = self.now();
        let mut at = t0 + 0.03;
        let mut i = 0.0;
        while i < n {
            self.pop(at, strength * (1.0 - i / (n + 2.0)));
            at += 0.035 + self.random() * 0.11;
            i += 1.0;
        }
        self.pop_cooldown = at - t0;
    }
}

// ── The electric car's quiet motor (the owner's request, D1061) ─────────
/// The motor's whine pulling away (motor ≤ `EV_HUSH_FROM`): −7 dB.
const EV_HUSH_LOW: f64 = 0.45;
/// And from `EV_HUSH_TO` of the motor's top speed up: −18 dB.
const EV_HUSH_HIGH: f64 = 0.125;
const EV_HUSH_FROM: f64 = 0.1;
const EV_HUSH_TO: f64 = 0.6;
/// The electric car's wind: up to this many times the others' (+7 dB) at
/// `EV_WIND_AT` m/s and beyond, rising from 1 at a standstill.
const EV_WIND: f64 = 2.25;
const EV_WIND_AT: f64 = 60.0;

/// The motor voices' scale for the motor's speed (0..1.05).
fn ev_hush(motor: f64) -> f64 {
    let k = clamp(
        (motor - EV_HUSH_FROM) / (EV_HUSH_TO - EV_HUSH_FROM),
        0.0,
        1.0,
    );
    EV_HUSH_LOW + (EV_HUSH_HIGH - EV_HUSH_LOW) * k
}

/// The electric car's wind scale at `speed` m/s (1 for the others).
fn ev_wind(speed: f64) -> f64 {
    1.0 + (EV_WIND - 1.0) * clamp(speed / EV_WIND_AT, 0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_electric_motor_hushes_with_speed_and_its_wind_grows() {
        assert_eq!(ev_hush(0.0), EV_HUSH_LOW, "a faint whine pulling away");
        assert_eq!(ev_hush(0.1), EV_HUSH_LOW);
        assert!(ev_hush(0.3) < ev_hush(0.2), "quieter the faster");
        assert_eq!(ev_hush(0.6), EV_HUSH_HIGH, "−18 dB at speed");
        assert_eq!(ev_hush(1.05), EV_HUSH_HIGH);
        assert_eq!(ev_wind(0.0), 1.0, "the others' wind at a standstill");
        assert_eq!(ev_wind(30.0), 1.625);
        assert_eq!(ev_wind(90.0), EV_WIND, "+7 dB from 60 m/s");
    }
}
