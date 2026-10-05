//! `HUD.js`'s `HUD` class without the DOM: what each element shows, kept
//! as the class keeps it (`set` writes a text only when it changed, the
//! toast and radio timers, the zone card on a zone change, the Hot Pursuit
//! furniture on `st.pursuit`). The Bevy side ([`super`]) puts the view on
//! its nodes; the tests here are `test/unit/hud.test.js`'s.
//!
//! The DOM's class toggles are booleans here, its `style.width`s numbers
//! in percent (`toFixed(1)` where the JS writes one, [`pct`]).

use crate::ui::widgets::{locale_int, to_fixed};
use mr_track::track::Track;

/// `fmtTime`: m:ss.hh (the race clock, lap and results times).
pub use super::super::flow::fmt_time;

/// `ORD`: the ordinal suffix.
pub fn ord(n: usize) -> &'static str {
    if n % 10 == 1 && n % 100 != 11 {
        "st"
    } else if n % 10 == 2 && n % 100 != 12 {
        "nd"
    } else if n % 10 == 3 && n % 100 != 13 {
        "rd"
    } else {
        "th"
    }
}

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    mr_math::clamp(v, lo, hi)
}

/// A width as the JS writes it: `(v * 100).toFixed(1) + '%'`, as the
/// number the string reads.
pub fn pct(v: f64) -> f64 {
    to_fixed(v * 100.0, 1).parse().unwrap_or(0.0)
}

/// `st.laps`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LapsIn {
    pub lap: i32,
    pub of: u32,
    /// This lap's time; `None` once finished.
    pub time: Option<f64>,
    pub best: Option<f64>,
}

/// `st.cruise`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CruiseIn {
    pub score: f64,
    pub mult: f64,
    pub mult_timer: f64,
    pub dist: f64,
}

/// A car on the minimap: world x, z.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dot {
    pub x: f64,
    pub z: f64,
}

/// A rival on the minimap (`st.racersFull`, players skipped), in standings
/// order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RivalDot {
    pub x: f64,
    pub z: f64,
    pub color: u32,
}

/// A roadblock or spike strip (`pu.roadblocks`, `pu.spikes`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bar {
    pub x: f64,
    pub z: f64,
    pub yaw: f64,
    pub width: f64,
}

/// A police unit (`pu.units`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Unit {
    pub x: f64,
    pub z: f64,
    pub disabled: bool,
}

/// `st.pursuit`: `Pursuit.hud(damage)` with `PursuitView.hudState`'s
/// `penalties` (`play::hud::pursuit_in`).
#[derive(Clone, Debug, PartialEq)]
pub struct PursuitIn {
    pub heat: f64,
    pub heat_meter: f64,
    /// `patrol`, `pursuit` or `cooldown`.
    pub state: &'static str,
    pub bust: f64,
    pub evade: f64,
    pub damage: f64,
    pub hold: f64,
    /// `busted` or `wrecked`.
    pub hold_reason: &'static str,
    pub hold_total: f64,
    pub penalties: f64,
    pub units: Vec<Unit>,
    pub roadblocks: Vec<Bar>,
    pub spikes: Vec<Bar>,
    /// `pu.flash !== false`.
    pub flash: bool,
}

impl Default for PursuitIn {
    /// The test file's `pursuit()`.
    fn default() -> PursuitIn {
        PursuitIn {
            heat: 1.0,
            heat_meter: 0.0,
            state: "patrol",
            bust: 0.0,
            evade: 0.0,
            damage: 0.0,
            hold: 0.0,
            hold_reason: "busted",
            hold_total: 6.0,
            penalties: 0.0,
            units: Vec::new(),
            roadblocks: Vec::new(),
            spikes: Vec::new(),
            flash: true,
        }
    }
}

/// `hud.update`'s `st` (Race.js `updateHud`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HudIn {
    pub position: usize,
    pub time: Option<f64>,
    /// m/s, `Math.hypot(vx, vz)`.
    pub speed: f64,
    pub gear: i32,
    pub rpm: f64,
    pub nitro: f64,
    pub nitro_active: bool,
    pub electric: bool,
    /// kW (`phys.powerOut`).
    pub power: f64,
    /// The player's s (`lapS`).
    pub s: f64,
    pub started: bool,
    /// Each racer's s (`lapS`), the player first.
    pub racers: Vec<f64>,
    pub laps: Option<LapsIn>,
    /// The player: x, z, yaw.
    pub player: (f64, f64, f64),
    pub traffic: Vec<Dot>,
    pub rivals: Vec<RivalDot>,
    pub cruise: Option<CruiseIn>,
    pub pursuit: Option<PursuitIn>,
}

/// The texts `set()` keeps (`this.last`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Texts {
    pub pos: String,
    pub suf: String,
    pub of: String,
    pub time: String,
    pub zone: String,
    pub speed: String,
    pub unit: String,
    pub gear: String,
    pub lap_n: String,
    pub lap_time: String,
    pub lap_best: String,
    pub score: String,
    pub mult: String,
    pub dist: String,
    pub best: String,
    pub pz_label: String,
    pub pen: String,
    pub hold_title: String,
    pub hold_sub: String,
}

/// The dial under the speed: the rev counter or, electric, the power
/// meter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dial {
    Tach { rpm: f64 },
    Power { kw: f64 },
}

/// The pursuit furniture's state (`updatePursuit`'s writes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PursuitView {
    /// `.hud-pz.flash`.
    pub flash: bool,
    /// `.hud-pz.patrol`.
    pub patrol: bool,
    /// Each star's fill, percent.
    pub stars: [f64; 5],
    /// `.pz-stars.max`.
    pub max: bool,
    /// `#pz-bar` hidden.
    pub bar_hidden: bool,
    pub bust: bool,
    pub evade: bool,
    /// `#pz-fill`'s width, percent.
    pub bar_fill: f64,
    /// `#hud-dmg-fill`: width (percent) and hue.
    pub dmg_fill: f64,
    pub dmg_hue: f64,
    pub dmg_crit: bool,
    pub dmg_flash: bool,
    pub pen_hidden: bool,
    pub hold_hidden: bool,
    pub wrecked: bool,
    /// `#hold-fill`'s width, percent.
    pub hold_fill: f64,
}

/// `#hud-center`'s classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CenterCls {
    /// `pop`.
    Pop,
    /// `pop go`: green.
    Go,
    /// `warn pop`: red and smaller.
    Warn,
}

/// The class each `hud.center` call of the JS gives its text (Race.js and
/// PursuitView.js; one class per text).
pub fn center_class(text: &str) -> CenterCls {
    match text {
        "GO!" | "WINNER!" | "ESCAPED" | "TAKEDOWN" => CenterCls::Go,
        "WRONG WAY" | "PURSUIT" | "SPIKED!" | "BUSTED" | "WRECKED" => CenterCls::Warn,
        t if t.ends_with(" PLACE") => CenterCls::Go,
        _ => CenterCls::Pop,
    }
}

/// `HUD`.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct Hud {
    pub cruise: bool,
    pub mph: bool,
    pub last_zone: Option<usize>,
    pub toast_timer: f64,
    pub toast_text: String,
    /// `#hud-toast.show`.
    pub toast_show: bool,
    pub center_timer: f64,
    pub center_text: String,
    pub center_cls: CenterCls,
    /// Counts `center()` calls: each restarts the pop.
    pub center_seq: u32,
    /// The zone card's texts, and a count of its restarts.
    pub zone_card: (String, String),
    pub zone_card_seq: u32,
    pub best_score: f64,
    pub last: Texts,
    /// `#hud-nitro`'s width, percent, and `.nitro.active`.
    pub nitro: f64,
    pub nitro_active: bool,
    /// `#speedlines`' opacity.
    pub speedlines: f64,
    /// Each route dot's `left`, percent (`toFixed(2)`).
    pub dots: Vec<f64>,
    /// `#hud-mult-fill`'s width, percent.
    pub mult_fill: f64,
    pub dial: Dial,
    pub clock: f64,
    pub radio_timer: f64,
    pub radio_text: String,
    pub radio_show: bool,
    /// Counts `radio()` calls: each restarts the slide-in.
    pub radio_seq: u32,
    pub pursuit_on: Option<bool>,
    /// `#hud-pz`, `#hud-dmg` hidden; `.hud-br.pz-on`.
    pub pz_hidden: bool,
    pub dmg_hidden: bool,
    pub pz_on: bool,
    pub pz: PursuitView,
    /// The number of racers (`setRacers`).
    pub racers: usize,
}

impl Hud {
    /// `new HUD(track, level)`; the racers are `setRacers`'.
    pub fn new(cruise: bool, racers: usize) -> Hud {
        let mut h = Hud {
            cruise,
            mph: true,
            last_zone: None,
            toast_timer: 0.0,
            toast_text: String::new(),
            toast_show: false,
            center_timer: 0.0,
            center_text: String::new(),
            center_cls: CenterCls::Pop,
            center_seq: 0,
            zone_card: (String::new(), String::new()),
            zone_card_seq: 0,
            best_score: 0.0,
            last: Texts::default(),
            nitro: 50.0,
            nitro_active: false,
            speedlines: 0.0,
            dots: vec![0.0; racers],
            mult_fill: 0.0,
            dial: Dial::Tach { rpm: 0.0 },
            clock: 0.0,
            radio_timer: 0.0,
            radio_text: String::new(),
            radio_show: false,
            radio_seq: 0,
            pursuit_on: None,
            pz_hidden: true,
            dmg_hidden: true,
            pz_on: false,
            pz: PursuitView {
                pen_hidden: true,
                hold_hidden: true,
                bar_hidden: true,
                ..PursuitView::default()
            },
            racers,
        };
        h.last.of = racers.to_string();
        h.set_pursuit(false);
        h
    }

    /// Shows or hides all the Hot Pursuit furniture. `update()` also calls
    /// it from `st.pursuit`, so a race without the mode never shows any of
    /// it.
    pub fn set_pursuit(&mut self, on: bool) {
        if Some(on) == self.pursuit_on {
            return;
        }
        self.pursuit_on = Some(on);
        self.pz_hidden = !on;
        self.dmg_hidden = !on;
        self.pz_on = on;
        if !on {
            self.pz.pen_hidden = true;
            self.pz.hold_hidden = true;
            self.radio_show = false;
            self.radio_timer = 0.0;
        }
    }

    /// A line of police radio chatter, bottom centre. Rate limiting is the
    /// caller's job; a new line replaces the current one.
    pub fn radio(&mut self, text: &str, dur: f64) {
        self.radio_text = text.to_string();
        self.radio_show = true;
        self.radio_seq += 1;
        self.radio_timer = dur;
    }

    pub fn center(&mut self, text: &str, cls: CenterCls, dur: f64) {
        self.center_text = text.to_string();
        self.center_cls = cls;
        self.center_seq += 1;
        self.center_timer = dur;
    }

    pub fn toast(&mut self, text: &str, dur: f64) {
        self.toast_text = text.to_string();
        self.toast_show = true;
        self.toast_timer = dur;
    }

    fn zone_card_show(&mut self, name: &str, sub: &str) {
        self.zone_card = (name.to_string(), sub.to_string());
        self.zone_card_seq += 1;
    }

    pub fn update(&mut self, dt: f64, st: &HudIn, track: &Track) {
        let set = |slot: &mut String, v: String| {
            if *slot != v {
                *slot = v;
            }
        };
        set(&mut self.last.pos, st.position.to_string());
        set(&mut self.last.suf, ord(st.position).to_string());
        set(&mut self.last.time, fmt_time(st.time));
        let spd = st.speed * if self.mph { 2.23694 } else { 3.6 };
        set(
            &mut self.last.speed,
            mr_math::js::round(spd.abs()).to_string(),
        );
        set(
            &mut self.last.unit,
            if self.mph { "MPH" } else { "KM/H" }.to_string(),
        );
        set(
            &mut self.last.gear,
            match st.gear {
                -1 => "R".to_string(),
                0 => "N".to_string(),
                _ if st.electric => "D".to_string(),
                g => g.to_string(),
            },
        );
        self.nitro = pct(st.nitro);
        self.nitro_active = st.nitro_active;
        self.speedlines =
            clamp((st.speed - 45.0) / 35.0, 0.0, 1.0) * if st.nitro_active { 1.0 } else { 0.6 };

        let z = track.zone[track.idx(st.s)] as usize;
        if Some(z) != self.last_zone {
            self.last_zone = Some(z);
            let zone = &track.zones[z].zone;
            set(&mut self.last.zone, zone.name.to_string());
            // (On a circuit, only the first time round.)
            if st.started && !st.laps.is_some_and(|l| l.lap > 1 || l.time.is_none()) {
                self.zone_card_show(zone.name, zone.sub);
            }
        }

        if let Some(l) = st.laps {
            set(&mut self.last.lap_n, format!("LAP {}/{}", l.lap, l.of));
            set(
                &mut self.last.lap_time,
                l.time.map_or(String::new(), |t| fmt_time(Some(t))),
            );
            set(
                &mut self.last.lap_best,
                l.best
                    .map_or(String::new(), |b| format!("BEST LAP {}", fmt_time(Some(b)))),
            );
        }

        // Route dots: along the route, or round the lap on a circuit.
        let len = if track.laps > 0 {
            track.n as f64
        } else {
            track.finish_s
        };
        for (i, s) in st.racers.iter().enumerate() {
            if let Some(d) = self.dots.get_mut(i) {
                *d = to_fixed(clamp(s / len, 0.0, 1.0) * 100.0, 2)
                    .parse()
                    .unwrap_or(0.0);
            }
        }

        if let Some(cr) = st.cruise {
            set(&mut self.last.score, locale_int(cr.score.floor()));
            set(
                &mut self.last.mult,
                format!("×{}", crate::ui::store::js_number(cr.mult)),
            );
            self.mult_fill = pct(if cr.mult > 1.0 {
                clamp(cr.mult_timer / 6.0, 0.0, 1.0)
            } else {
                0.0
            });
            let km = cr.dist / 1000.0;
            set(
                &mut self.last.dist,
                if self.mph {
                    format!("{} mi", to_fixed(km / 1.60934, 1))
                } else {
                    format!("{} km", to_fixed(km, 1))
                },
            );
            set(
                &mut self.last.best,
                locale_int(mr_math::js::max(self.best_score, cr.score).floor()),
            );
        }
        if self.toast_timer > 0.0 {
            self.toast_timer -= dt;
            if self.toast_timer <= 0.0 {
                self.toast_show = false;
            }
        }
        if self.radio_timer > 0.0 {
            self.radio_timer -= dt;
            if self.radio_timer <= 0.0 {
                self.radio_show = false;
            }
        }
        self.clock += dt;
        self.set_pursuit(st.pursuit.is_some());
        if let Some(pu) = &st.pursuit {
            self.update_pursuit(pu);
        }
        self.dial = if st.electric {
            Dial::Power { kw: st.power }
        } else {
            Dial::Tach { rpm: st.rpm }
        };
    }

    pub fn update_pursuit(&mut self, pu: &PursuitIn) {
        let p = &mut self.pz;
        let flash = pu.flash;
        p.flash = flash;
        p.patrol = pu.state == "patrol";
        // Stars: whole ones up to the heat, the next one filling with the
        // meter.
        let heat = clamp(mr_math::js::or(pu.heat, 1.0).floor(), 1.0, 5.0) as usize;
        for (i, f) in p.stars.iter_mut().enumerate() {
            let v = if i < heat {
                1.0
            } else if i == heat {
                clamp(pu.heat_meter, 0.0, 1.0)
            } else {
                0.0
            };
            *f = pct(v);
        }
        p.max = heat == 5;
        // Bar: BUST while it's filling, EVADE in cooldown, nothing in patrol.
        let bust = pu.bust > 0.0;
        let evade = !bust && pu.state == "cooldown";
        p.bar_hidden = !bust && !evade;
        p.bust = bust;
        p.evade = evade;
        if bust || evade {
            let label = if bust { "BUST" } else { "EVADE" };
            if self.last.pz_label != label {
                self.last.pz_label = label.to_string();
            }
            p.bar_fill = pct(clamp(if bust { pu.bust } else { pu.evade }, 0.0, 1.0));
        }
        // Damage: green → amber → red, pulsing past 75 %.
        let d = clamp(pu.damage, 0.0, 1.0);
        p.dmg_fill = pct(d);
        p.dmg_hue = mr_math::js::round(125.0 * (1.0 - d));
        p.dmg_crit = d > 0.75;
        p.dmg_flash = flash;
        // Penalty served, under the race clock.
        let pen = pu.penalties;
        p.pen_hidden = pen <= 0.0;
        if pen > 0.0 {
            self.last.pen = format!("+{} s", to_fixed(pen, 1));
        }
        // The hold card while the car is held for a penalty.
        let held = pu.hold > 0.0;
        p.hold_hidden = !held;
        if held {
            let wrecked = pu.hold_reason == "wrecked";
            p.wrecked = wrecked;
            self.last.hold_title = if wrecked { "WRECKED" } else { "BUSTED" }.to_string();
            self.last.hold_sub = format!("+{} s PENALTY", to_fixed(pu.hold, 1));
            let total = if pu.hold_total != 0.0 {
                pu.hold_total
            } else {
                pu.hold
            };
            p.hold_fill = pct(clamp(pu.hold / total, 0.0, 1.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_time_shows_m_ss_hh() {
        for (t, s) in [
            (0.0, "0:00.00"),
            (5.2, "0:05.20"),
            (59.5, "0:59.50"),
            (60.0, "1:00.00"),
            (65.432, "1:05.43"),
            (185.123, "3:05.12"),
            (3600.0, "60:00.00"),
        ] {
            assert_eq!(fmt_time(Some(t)), s, "{t}");
        }
    }

    #[test]
    fn fmt_time_shows_dashes_for_no_time() {
        for t in [
            None,
            Some(f64::NAN),
            Some(f64::INFINITY),
            Some(f64::NEG_INFINITY),
        ] {
            assert_eq!(fmt_time(t), "--:--.--", "{t:?}");
        }
    }

    // The last few milliseconds of a minute round up to the next minute,
    // not to ":60.00".
    #[test]
    fn fmt_time_rounds_across_the_minute() {
        assert_eq!(fmt_time(Some(59.996)), "1:00.00");
        assert_eq!(fmt_time(Some(119.999)), "2:00.00");
        assert_eq!(fmt_time(Some(0.004)), "0:00.00");
        assert_eq!(fmt_time(Some(9.996)), "0:10.00");
    }

    #[test]
    fn ordinals() {
        let got: Vec<String> = [1, 2, 3, 4, 11, 12, 13, 21, 22, 23, 101, 111]
            .iter()
            .map(|&n| format!("{n}{}", ord(n)))
            .collect();
        assert_eq!(
            got,
            [
                "1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd", "23rd",
                "101st", "111th"
            ]
        );
    }

    /// The test file's `makeTrack()`: 1000 samples, one zone, finish at
    /// 900.
    pub(crate) fn make_track() -> Track {
        let mut t = super::super::minimap::tests::straight_track(1000);
        t.finish_s = 900.0;
        t
    }

    /// The test file's `state(pu)`.
    fn state(pu: Option<PursuitIn>) -> HudIn {
        HudIn {
            position: 1,
            time: Some(12.3),
            speed: 30.0,
            gear: 3,
            nitro: 0.5,
            nitro_active: false,
            rpm: 4000.0,
            s: 10.0,
            started: true,
            racers: vec![10.0],
            player: (0.0, 0.0, 0.0),
            pursuit: pu,
            ..HudIn::default()
        }
    }

    fn pursuit(f: impl FnOnce(&mut PursuitIn)) -> Option<PursuitIn> {
        let mut p = PursuitIn::default();
        f(&mut p);
        Some(p)
    }

    fn hud() -> (Hud, Track) {
        (Hud::new(false, 1), make_track())
    }

    const DT: f64 = 1.0 / 60.0;

    #[test]
    fn no_pursuit_furniture_without_st_pursuit() {
        let (mut h, t) = hud();
        h.update(DT, &state(None), &t);
        assert!(h.pz_hidden && h.dmg_hidden && h.pz.pen_hidden && h.pz.hold_hidden);
        assert!(!h.radio_show);
        assert!(!h.pz_on);
        assert_eq!(h.last.time, "0:12.30");
    }

    #[test]
    fn st_pursuit_shows_the_stars_and_damage_and_dropping_it_hides_them_again() {
        let (mut h, t) = hud();
        h.update(DT, &state(pursuit(|_| {})), &t);
        assert!(!h.pz_hidden);
        assert!(!h.dmg_hidden);
        assert!(h.pz_on);
        h.update(DT, &state(None), &t);
        assert!(h.pz_hidden);
        assert!(h.dmg_hidden);
    }

    #[test]
    fn set_pursuit_shows_and_hides_the_furniture() {
        let (mut h, _) = hud();
        h.set_pursuit(true);
        assert!(!h.pz_hidden);
        h.set_pursuit(false);
        assert!(h.pz_hidden);
    }

    #[test]
    fn five_stars_full_up_to_the_heat_the_next_one_filling_with_the_meter() {
        let (mut h, t) = hud();
        h.update(
            DT,
            &state(pursuit(|p| {
                p.heat = 2.0;
                p.heat_meter = 0.4;
            })),
            &t,
        );
        assert_eq!(h.pz.stars, [100.0, 100.0, 40.0, 0.0, 0.0]);
        assert!(!h.pz.max);
        h.update(
            DT,
            &state(pursuit(|p| {
                p.heat = 5.0;
                p.heat_meter = 0.7;
            })),
            &t,
        );
        assert_eq!(h.pz.stars, [100.0; 5]);
        assert!(h.pz.max);
    }

    #[test]
    fn the_bar_hidden_in_patrol_bust_while_the_bust_meter_fills_evade_in_cooldown() {
        let (mut h, t) = hud();
        h.update(DT, &state(pursuit(|p| p.state = "patrol")), &t);
        assert!(h.pz.bar_hidden);
        assert!(h.pz.patrol);

        h.update(
            DT,
            &state(pursuit(|p| {
                p.state = "pursuit";
                p.bust = 0.6;
            })),
            &t,
        );
        assert!(!h.pz.bar_hidden);
        assert!(h.pz.bust);
        assert_eq!(h.last.pz_label, "BUST");
        assert_eq!(to_fixed(h.pz.bar_fill, 1), "60.0");

        // In pursuit with no bust there is nothing to show.
        h.update(
            DT,
            &state(pursuit(|p| {
                p.state = "pursuit";
                p.evade = 0.3;
            })),
            &t,
        );
        assert!(h.pz.bar_hidden);

        h.update(
            DT,
            &state(pursuit(|p| {
                p.state = "cooldown";
                p.evade = 0.25;
            })),
            &t,
        );
        assert!(h.pz.evade);
        assert!(!h.pz.bust);
        assert_eq!(h.last.pz_label, "EVADE");
        assert_eq!(to_fixed(h.pz.bar_fill, 1), "25.0");

        // Bust wins over evade, even in cooldown.
        h.update(
            DT,
            &state(pursuit(|p| {
                p.state = "cooldown";
                p.evade = 0.5;
                p.bust = 0.1;
            })),
            &t,
        );
        assert_eq!(h.last.pz_label, "BUST");
    }

    #[test]
    fn damage_bar_width_critical_above_75_percent() {
        let (mut h, t) = hud();
        h.update(DT, &state(pursuit(|p| p.damage = 0.5)), &t);
        assert_eq!(to_fixed(h.pz.dmg_fill, 1), "50.0");
        assert!(!h.pz.dmg_crit);
        h.update(DT, &state(pursuit(|p| p.damage = 0.9)), &t);
        assert!(h.pz.dmg_crit);
    }

    #[test]
    fn the_hold_card_counts_down_the_penalty() {
        let (mut h, t) = hud();
        h.update(DT, &state(pursuit(|_| {})), &t);
        assert!(h.pz.hold_hidden);
        h.update(
            DT,
            &state(pursuit(|p| {
                p.hold = 4.26;
                p.hold_total = 6.0;
                p.hold_reason = "busted";
            })),
            &t,
        );
        assert!(!h.pz.hold_hidden);
        assert_eq!(h.last.hold_title, "BUSTED");
        assert_eq!(h.last.hold_sub, "+4.3 s PENALTY");
        assert_eq!(to_fixed(h.pz.hold_fill, 1), "71.0");
        assert!(!h.pz.wrecked);
        h.update(
            DT,
            &state(pursuit(|p| {
                p.hold = 2.0;
                p.hold_total = 5.0;
                p.hold_reason = "wrecked";
            })),
            &t,
        );
        assert_eq!(h.last.hold_title, "WRECKED");
        assert!(h.pz.wrecked);
        h.update(DT, &state(pursuit(|p| p.hold = 0.0)), &t);
        assert!(h.pz.hold_hidden);
    }

    #[test]
    fn penalty_served_shows_under_the_race_time_once_there_is_some() {
        let (mut h, t) = hud();
        h.update(DT, &state(pursuit(|p| p.penalties = 0.0)), &t);
        assert!(h.pz.pen_hidden);
        h.update(DT, &state(pursuit(|p| p.penalties = 12.5)), &t);
        assert!(!h.pz.pen_hidden);
        assert_eq!(h.last.pen, "+12.5 s");
    }

    #[test]
    fn flash_off_turns_the_blinking_off() {
        let (mut h, t) = hud();
        h.update(DT, &state(pursuit(|p| p.flash = true)), &t);
        assert!(h.pz.flash);
        h.update(
            DT,
            &state(pursuit(|p| {
                p.flash = false;
                p.damage = 0.9;
            })),
            &t,
        );
        assert!(!h.pz.flash);
        assert!(!h.pz.dmg_flash);
    }

    #[test]
    fn radio_chatter_shows_for_its_duration() {
        let (mut h, t) = hud();
        h.update(DT, &state(pursuit(|_| {})), &t);
        h.radio("Unit 12 in pursuit", 2.0);
        assert_eq!(h.radio_text, "Unit 12 in pursuit");
        assert!(h.radio_show);
        h.update(1.5, &state(pursuit(|_| {})), &t);
        assert!(h.radio_show);
        h.update(0.6, &state(pursuit(|_| {})), &t);
        assert!(!h.radio_show);
    }

    // The toast's timing, as `toast(text, dur)` and `update(dt)` keep it:
    // on for `dur` seconds of updates, then off (the CSS fades it).
    #[test]
    fn a_toast_shows_for_its_duration() {
        let (mut h, t) = hud();
        h.toast("PERFECT START", 1.6);
        assert!(h.toast_show);
        assert_eq!(h.toast_text, "PERFECT START");
        for _ in 0..95 {
            h.update(DT, &state(None), &t);
        }
        assert!(h.toast_show, "1.58 s in");
        h.update(DT, &state(None), &t);
        h.update(DT, &state(None), &t);
        assert!(!h.toast_show, "1.61 s in");
        // The text stays for the fade.
        assert_eq!(h.toast_text, "PERFECT START");
        // A new toast replaces the text and starts the timer again.
        h.toast("NEAR MISS  +N₂O", 1.6);
        h.update(1.0, &state(None), &t);
        h.toast("DRIFT 2.1s", 1.6);
        h.update(1.0, &state(None), &t);
        assert!(h.toast_show);
        assert_eq!(h.toast_text, "DRIFT 2.1s");
    }

    #[test]
    fn the_readouts() {
        let (mut h, t) = hud();
        let mut st = state(None);
        st.position = 2;
        st.speed = 31.0;
        h.update(DT, &st, &t);
        assert_eq!((&*h.last.pos, &*h.last.suf, &*h.last.of), ("2", "nd", "1"));
        assert_eq!(h.last.speed, "69"); // 31 m/s × 2.23694 = 69.3
        assert_eq!(h.last.unit, "MPH");
        assert_eq!(h.last.gear, "3");
        assert_eq!(h.nitro, 50.0);
        h.mph = false;
        st.gear = -1;
        st.speed = -10.0;
        h.update(DT, &st, &t);
        assert_eq!((&*h.last.speed, &*h.last.unit), ("36", "KM/H"));
        assert_eq!(h.last.gear, "R");
        st.gear = 0;
        h.update(DT, &st, &t);
        assert_eq!(h.last.gear, "N");
        st.gear = 2;
        st.electric = true;
        st.power = 300.0;
        h.update(DT, &st, &t);
        assert_eq!(h.last.gear, "D");
        assert_eq!(h.dial, Dial::Power { kw: 300.0 });
    }

    #[test]
    fn speed_lines_from_45_m_s_stronger_on_nitro() {
        let (mut h, t) = hud();
        let mut st = state(None);
        st.speed = 45.0;
        h.update(DT, &st, &t);
        assert_eq!(h.speedlines, 0.0);
        st.speed = 62.5;
        h.update(DT, &st, &t);
        assert!((h.speedlines - 0.3).abs() < 1e-12);
        st.nitro_active = true;
        st.speed = 100.0;
        h.update(DT, &st, &t);
        assert_eq!(h.speedlines, 1.0);
        assert!(h.nitro_active);
    }

    #[test]
    fn the_zone_card_on_a_zone_change_once_started_and_on_a_circuit_only_on_lap_one() {
        let (mut h, t) = hud();
        let mut st = state(None);
        st.started = false;
        h.update(DT, &st, &t);
        assert_eq!(h.last.zone, "SIERRA PASS");
        assert_eq!(h.zone_card_seq, 0, "not before the start");
        let (mut h, _) = hud();
        st.started = true;
        h.update(DT, &st, &t);
        assert_eq!(h.zone_card_seq, 1);
        assert_eq!(h.zone_card.0, "SIERRA PASS");
        h.update(DT, &st, &t);
        assert_eq!(h.zone_card_seq, 1, "only on a change");
        // A circuit's second lap, or after the finish: no card.
        for laps in [
            LapsIn {
                lap: 2,
                of: 3,
                time: Some(1.0),
                best: None,
            },
            LapsIn {
                lap: 3,
                of: 3,
                time: None,
                best: None,
            },
        ] {
            let (mut h, _) = hud();
            st.laps = Some(laps);
            h.update(DT, &st, &t);
            assert_eq!(h.zone_card_seq, 0);
        }
    }

    #[test]
    fn laps_route_dots_and_the_cruise_panel() {
        let (mut h, t) = hud();
        let mut st = state(None);
        st.laps = Some(LapsIn {
            lap: 2,
            of: 3,
            time: Some(61.234),
            best: Some(59.996),
        });
        st.racers = vec![450.0];
        h.update(DT, &st, &t);
        assert_eq!(h.last.lap_n, "LAP 2/3");
        assert_eq!(h.last.lap_time, "1:01.23");
        assert_eq!(h.last.lap_best, "BEST LAP 1:00.00");
        // Along the route: s / finishS.
        assert_eq!(h.dots, [50.0]);
        st.laps = Some(LapsIn {
            lap: 3,
            of: 3,
            time: None,
            best: None,
        });
        h.update(DT, &st, &t);
        assert_eq!((&*h.last.lap_time, &*h.last.lap_best), ("", ""));

        let (mut h, _) = (Hud::new(true, 1), ());
        h.best_score = 12000.0;
        st.cruise = Some(CruiseIn {
            score: 15234.7,
            mult: 3.0,
            mult_timer: 1.5,
            dist: 4023.35,
        });
        h.update(DT, &st, &t);
        assert_eq!(h.last.score, "15,234");
        assert_eq!(h.last.mult, "×3");
        assert_eq!(h.mult_fill, 25.0);
        assert_eq!(h.last.dist, "2.5 mi");
        assert_eq!(h.last.best, "15,234");
        st.cruise = Some(CruiseIn {
            score: 100.0,
            mult: 1.0,
            mult_timer: 5.0,
            dist: 4023.35,
        });
        h.mph = false;
        h.update(DT, &st, &t);
        assert_eq!(h.mult_fill, 0.0);
        assert_eq!(h.last.dist, "4.0 km");
        assert_eq!(h.last.best, "12,000");
    }

    #[test]
    fn centre_classes() {
        assert_eq!(center_class("3"), CenterCls::Pop);
        assert_eq!(center_class("GO!"), CenterCls::Go);
        assert_eq!(center_class("2nd PLACE"), CenterCls::Go);
        assert_eq!(center_class("WRONG WAY"), CenterCls::Warn);
        assert_eq!(center_class("FINAL LAP"), CenterCls::Pop);
        assert_eq!(center_class("LAP 2/3"), CenterCls::Pop);
    }
}
