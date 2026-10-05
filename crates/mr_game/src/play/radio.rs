//! Police radio chatter in Hot Pursuit (roadmap WP 8.4, DECISIONS D980):
//! the radio half of `PursuitView.js`. `say` keeps dispatch from talking
//! over itself (a line every [`SAY_GAP`] seconds, unless it must be heard
//! now), and [`Radio::events`] is `PursuitView.events`' table: what each
//! pursuit event says, and the stingers it plays, in the JS's order.
//!
//! A line said goes two ways: its text onto the HUD (`race.radio.said`,
//! which the HUD reads every frame: `hud.radio(text, max(3, len / 14))`),
//! and its words over the radio (`audio.radioLine(parts)`, which
//! [`super::audio`] plays: the lines of the frame's ticks in their tick, in
//! order with the tick's other calls, and the lines said between frames by
//! the test bridge, `race.pv.say(line, now)`, before the ticks).
//!
//! `PursuitView.events` runs once per `Race.update`, which the parity
//! drives make once per 1/120 s tick: the countdown of the gap and the
//! events are per tick here too (each tick's events ride on its
//! [`TickAudio`]), so the lines and their place among the audio calls are
//! the JS drive's exactly. [`Radio::frame`] only clears the frame's `said`.

use super::audio::{PursuitTick, TickAudio};
use mr_audio::radio::lines::{self as radio_lines, Line};
use mr_sim::pursuit::PursuitEvent;
use mr_sim::race::DT;

/// Seconds between chatter lines (`PursuitView.js`'s `RADIO_GAP`; renamed:
/// `mr_audio`'s `RADIO_GAP` is the pause between a line's clips).
pub const SAY_GAP: f64 = 4.0;

/// One of `PursuitView.events`' calls on the audio.
#[derive(Clone, Debug, PartialEq)]
pub enum PvCall {
    /// `audio.sirenHorn()`: the pursuit is on.
    SirenHorn,
    /// `audio.escaped()`.
    Escaped,
    /// `audio.takedown(1)`: the player took a unit out.
    Takedown,
    /// `audio.spikePop()`: the player hit the spikes.
    SpikePop,
    /// `audio.impact(0.35, 0)`: the player knocked a barrier over.
    Barrier,
    /// `audio.busted()`.
    Busted,
    /// `audio.wrecked()`.
    Wrecked,
    /// `audio.radioLine(parts)`: a line that got through.
    RadioLine(Vec<String>),
}

/// `PursuitView`'s radio: the gap, and the lines said this frame.
#[derive(Clone, Debug, Default)]
pub struct Radio {
    /// The lines said during the last frame, in order (the HUD shows each).
    pub said: Vec<Line>,
    /// `radioT`: seconds until a line that need not be heard now may come.
    t: f64,
    /// The lines said outside the ticks (the test bridge's `say`), for the
    /// voice.
    direct: Vec<Line>,
}

impl Radio {
    /// A new frame: last frame's lines are gone. (The gap counts down per
    /// tick, in [`Radio::events`].)
    pub fn frame(&mut self, _dt: f64) {
        self.said.clear();
        self.direct.clear();
    }

    /// `PursuitView.say(line, force)` from outside the ticks (the test
    /// bridge): the text, and the voice before the frame's ticks.
    pub fn say(&mut self, line: Line, force: bool) -> bool {
        if !self.pass(force) {
            return false;
        }
        self.direct.push(line.clone());
        self.said.push(line);
        true
    }

    /// The lines [`Radio::say`] let through this frame, for the voice.
    pub fn direct(&self) -> &[Line] {
        &self.direct
    }

    /// `if (this.radioT > 0 && !force) return; this.radioT = RADIO_GAP;`
    fn pass(&mut self, force: bool) -> bool {
        if self.t > 0.0 && !force {
            return false;
        }
        self.t = SAY_GAP;
        true
    }

    /// A line said in a tick: the text, and the voice in its place.
    fn say_in(&mut self, calls: &mut Vec<PvCall>, line: Line, force: bool) {
        if self.pass(force) {
            calls.push(PvCall::RadioLine(line.parts.clone()));
            self.said.push(line);
        }
    }

    /// `PursuitView.events(dt)` for each tick of the frame: `radioT -= dt`,
    /// then each event's radio line and stinger, into the tick's `pv`.
    pub fn events(&mut self, ticks: &mut [TickAudio]) {
        for ta in ticks {
            let Some(pt) = &ta.pursuit else { continue };
            self.t -= DT;
            let mut calls = Vec::new();
            for e in &pt.events {
                self.event(pt, e, &mut calls);
            }
            ta.pv = calls;
        }
    }

    fn event(&mut self, pt: &PursuitTick, e: &PursuitEvent, calls: &mut Vec<PvCall>) {
        let callsign = |i: usize| pt.callsigns.get(i).copied().unwrap_or(0);
        match e {
            PursuitEvent::Pursuit { .. } => {
                calls.push(PvCall::SirenHorn);
                self.say_in(calls, radio_lines::pursuit(pt.heading, &pt.zone), true);
            }
            PursuitEvent::Reacquired { .. } => self.say_in(calls, radio_lines::spotted(), false),
            PursuitEvent::Cooldown => self.say_in(calls, radio_lines::lost(), false),
            PursuitEvent::Escaped { .. } => {
                calls.push(PvCall::Escaped);
                self.say_in(calls, radio_lines::escaped(), true);
            }
            PursuitEvent::Heat { heat } => self.say_in(calls, radio_lines::heat(*heat), true),
            PursuitEvent::Spotted {
                unit, player: true, ..
            } => self.say_in(
                calls,
                radio_lines::intercept(callsign(*unit), &pt.zone),
                false,
            ),
            PursuitEvent::Join { unit } => {
                if pt.chasing {
                    self.say_in(calls, radio_lines::joining(callsign(*unit)), false);
                }
            }
            PursuitEvent::Takedown {
                unit,
                by_player: true,
                ..
            } => {
                calls.push(PvCall::Takedown);
                self.say_in(calls, radio_lines::unit_down(callsign(*unit)), true);
            }
            PursuitEvent::Roadblock { heavy, .. } => {
                self.say_in(calls, radio_lines::roadblock(*heavy), true)
            }
            PursuitEvent::Spikes { .. } => self.say_in(calls, radio_lines::spikes(), true),
            PursuitEvent::Spiked { player: true, .. } => {
                calls.push(PvCall::SpikePop);
                self.say_in(calls, radio_lines::spiked(), true);
            }
            PursuitEvent::Barrier { player: true, .. } => calls.push(PvCall::Barrier),
            PursuitEvent::Busted { player, name, .. } => {
                if *player {
                    calls.push(PvCall::Busted);
                    self.say_in(calls, radio_lines::busted(), true);
                } else {
                    self.say_in(calls, radio_lines::rival_busted(name), false);
                }
            }
            PursuitEvent::Wrecked { .. } => {
                calls.push(PvCall::Wrecked);
                self.say_in(calls, radio_lines::wrecked(), true);
            }
            _ => {}
        }
    }
}

/// `PursuitView.zoneName()`: the zone the player is in, as dispatch says
/// it.
pub fn zone_name(t: &mr_track::Track, s: f64) -> String {
    let z = usize::from(t.zone[t.idx(s)]);
    t.zones
        .get(z)
        .map_or_else(String::new, |z| radio_lines::place_name(z.zone.name))
}

/// `PursuitView.heading()`: the compass heading from the road direction
/// (+x east, +z south).
pub fn heading(t: &mr_track::Track, s: f64) -> &'static str {
    use mr_math::{js, kernel};
    let f = t.frame(s);
    let a = kernel::atan2(f.fz, f.fx);
    let k = ((js::round(a / (std::f64::consts::PI / 4.0)) % 8.0) + 8.0) % 8.0;
    radio_lines::DIRS[k as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tick(events: Vec<PursuitEvent>, chasing: bool) -> TickAudio {
        TickAudio {
            pursuit: Some(PursuitTick {
                events,
                zone: "Sierra Pass".into(),
                heading: "east",
                callsigns: vec![12, 17],
                chasing,
                ..PursuitTick::default()
            }),
            ..TickAudio::default()
        }
    }

    #[test]
    fn a_line_that_need_not_be_heard_waits_for_the_gap() {
        let mut r = Radio::default();
        let mut ticks = vec![tick(
            vec![
                PursuitEvent::Spotted {
                    unit: 0,
                    racer: 0,
                    player: true,
                },
                PursuitEvent::Pursuit { heat: 1 },
            ],
            true,
        )];
        r.frame(DT);
        r.events(&mut ticks);
        // The intercept, then the siren and the forced pursuit call.
        assert_eq!(
            ticks[0].pv,
            vec![
                PvCall::RadioLine(vec![
                    "Unit 12.".into(),
                    "Speeder on Sierra Pass, moving to intercept.".into()
                ]),
                PvCall::SirenHorn,
                PvCall::RadioLine(vec![
                    "All units, suspect heading east on Sierra Pass. Pursuit is on.".into()
                ]),
            ]
        );
        assert_eq!(r.said.len(), 2);
        // Within the gap a lost visual is not said; four seconds on it is.
        let mut ticks: Vec<TickAudio> = (0..480)
            .map(|i| {
                tick(
                    if i == 0 {
                        vec![PursuitEvent::Cooldown]
                    } else {
                        vec![]
                    },
                    true,
                )
            })
            .collect();
        r.frame(4.0);
        r.events(&mut ticks);
        assert!(r.said.is_empty());
        let mut ticks = vec![tick(vec![PursuitEvent::Cooldown], true)];
        r.frame(DT);
        r.events(&mut ticks);
        assert_eq!(r.said, vec![radio_lines::lost()]);
        // A unit joining is only called out during a pursuit.
        let mut r = Radio::default();
        let mut ticks = vec![tick(vec![PursuitEvent::Join { unit: 1 }], false)];
        r.events(&mut ticks);
        assert!(ticks[0].pv.is_empty());
        let mut ticks = vec![tick(vec![PursuitEvent::Join { unit: 1 }], true)];
        r.events(&mut ticks);
        assert_eq!(r.said, vec![radio_lines::joining(17)]);
        // The bridge's say: forced through the gap, voiced before the ticks.
        assert!(!r.say(radio_lines::spikes(), false));
        assert!(r.say(radio_lines::spikes(), true));
        assert_eq!(r.direct(), &[radio_lines::spikes()]);
    }
}
