//! The station player: what the radio node runs on the audio thread, the
//! same code in the browser's AudioWorklet (`wasm`) and in `mp_audio`'s
//! native worklet processor (radio.md 7, DECISIONS D1151).
//!
//! The node's params come once per block: `station` (an index in
//! [`STATIONS`], below zero off), the wall time at the moment of tuning as
//! two numbers (days since the Unix epoch and seconds into the day), a
//! `tune` serial, and `energy`. A change of station or serial re-tunes:
//! the cue for that wall time is computed ([`cue`]), the engine starts on
//! the song's next 16th with the bar's state set, and the tuner fades the
//! station playing into static and the static into the new one. Two
//! engines take turns: the one leaving keeps playing through its fade.
//! From then on the player keeps the schedule itself: when
//! a song ends it plays the next slot of the block, or the first of the
//! next block, with no gap; the main thread posts nothing per song.
//!
//! The engine's step clock rounds each 16th to whole samples, so a song
//! runs a few milliseconds off its scheduled length; over a block that is
//! under a tenth of a second against the wall clock, and any re-tune
//! re-syncs.

use crate::engine::{Engine, EngineEvent};
use crate::radio::{STATIONS, Slot, Tuner, block, block_at, cue_in, next_slot, wall_from};

pub struct Player {
    /// The two engines: `engines[cur]` the station tuned, the other the
    /// one fading out while the tuner says it is still heard.
    engines: [Engine; 2],
    cur: usize,
    /// The leaving engine's block, rendered beside the tuned one's.
    spare: (Vec<f32>, Vec<f32>),
    tuner: Tuner,
    /// The station tuned, if any.
    station: Option<usize>,
    /// The `tune` param's last value (a change re-syncs).
    serial: Option<f32>,
    energy: f32,
    block: u64,
    slot: usize,
    slots: Vec<Slot>,
}

impl Player {
    /// A player at `sample_rate`, off (silent) until tuned.
    pub fn new(seed: u32, sample_rate: f64) -> Player {
        Player {
            engines: [
                Engine::new(seed, sample_rate),
                Engine::new(seed.wrapping_add(1), sample_rate),
            ],
            cur: 0,
            spare: (Vec::new(), Vec::new()),
            tuner: Tuner::new(seed.wrapping_add(7), sample_rate),
            station: None,
            serial: None,
            energy: 1.0,
            block: 0,
            slot: 0,
            slots: Vec::new(),
        }
    }

    /// The node's params for this block.
    pub fn set_params(
        &mut self,
        station: f32,
        wall_day: f32,
        wall_sec: f32,
        tune: f32,
        energy: f32,
    ) {
        let st = if station < 0.0 {
            None
        } else {
            let i = (station + 0.5).floor() as usize;
            if i < STATIONS.len() { Some(i) } else { None }
        };
        if st != self.station || self.serial != Some(tune) {
            self.serial = Some(tune);
            self.retune(st, wall_from(wall_day as f64, wall_sec as f64));
        }
        if energy != self.energy {
            self.energy = energy;
            for e in self.engines.iter_mut() {
                e.set_energy(energy as f64);
            }
        }
    }

    /// The engine playing the station tuned.
    fn engine(&mut self) -> &mut Engine {
        &mut self.engines[self.cur]
    }

    /// The station playing becomes the one leaving: the engines swap, and
    /// the tuned one is free for the new station.
    fn hand_over(&mut self) {
        self.cur ^= 1;
        self.engines[self.cur].stop();
    }

    /// Tunes to `st` (or off) at wall time `wall`.
    fn retune(&mut self, st: Option<usize>, wall: f64) {
        // What is heard now leaves through the static: the station playing,
        // or one that was only just arriving (the tuner turns back from the
        // same point). Mid-way, while the old one still fades or only static
        // is heard, the arrival is simply replaced.
        let heard = self.station.is_some() && self.tuner.station_audible();
        if heard {
            self.hand_over();
        }
        let Some(i) = st else {
            self.engine().stop();
            self.tuner.off();
            self.station = None;
            return;
        };
        let (b, into) = block_at(wall);
        self.slots = block(&STATIONS[i], b);
        let c = cue_in(b, &self.slots, into);
        self.block = c.block;
        self.slot = c.slot;
        let step_dur = 60.0 / c.track.bpm / 4.0;
        let energy = self.energy as f64;
        let e = self.engine();
        e.set_track(&c.track);
        e.set_energy(energy);
        // On the next 16th, where the station's clock says it falls.
        e.cue(c.step, (1.0 - c.frac) * step_dur);
        self.tuner.tune();
        self.station = Some(i);
    }

    /// The song ended: the next slot, straight away.
    fn next_song(&mut self) {
        let Some(i) = self.station else { return };
        let (b, s, slots) = next_slot(&STATIONS[i], self.block, self.slot, &self.slots);
        self.block = b;
        self.slot = s;
        self.slots = slots;
        let e = &mut self.engines[self.cur];
        e.set_track(&self.slots[self.slot].track);
        e.set_energy(self.energy as f64);
        e.play(0, 0.0);
    }

    /// Renders one block (`l.len()` frames) of the station through the
    /// tuner (with the station leaving, while it fades); silence when off.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        if self.station.is_some() {
            let e = &mut self.engines[self.cur];
            e.process(l, r);
            let ended = e
                .take_events()
                .iter()
                .any(|e| matches!(e, EngineEvent::End));
            if ended {
                self.next_song();
            }
        } else {
            l.fill(0.0);
            r.fill(0.0);
        }
        let other = &mut self.engines[self.cur ^ 1];
        if self.tuner.leaving() {
            let n = l.len();
            self.spare.0.resize(n, 0.0);
            self.spare.1.resize(n, 0.0);
            other.process(&mut self.spare.0[..n], &mut self.spare.1[..n]);
            // Its song may end mid-fade: it just stops.
            other.take_events();
            self.tuner
                .process(l, r, Some((&self.spare.0[..n], &self.spare.1[..n])));
        } else {
            if other.playing() {
                other.stop();
            }
            self.tuner.process(l, r, None);
        }
    }

    /// Where the player is: the station, block and slot, for tests.
    pub fn position(&self) -> Option<(usize, u64, usize)> {
        self.station.map(|s| (s, self.block, self.slot))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::radio::wall_parts;

    fn render(p: &mut Player, blocks: usize) -> (f64, f64) {
        let mut l = vec![0.0f32; 128];
        let mut r = vec![0.0f32; 128];
        let (mut sum, mut peak) = (0.0, 0.0f64);
        for _ in 0..blocks {
            p.process(&mut l, &mut r);
            for x in l.iter().chain(r.iter()) {
                let v = *x as f64;
                assert!(v.is_finite());
                sum += v * v;
                peak = peak.max(v.abs());
            }
        }
        ((sum / (blocks as f64 * 256.0)).sqrt(), peak)
    }

    #[test]
    fn off_is_silent_and_a_station_plays() {
        let sr = 48000.0;
        let mut p = Player::new(2026, sr);
        p.set_params(-1.0, 0.0, 0.0, 0.0, 1.0);
        let (rms, _) = render(&mut p, 20);
        assert_eq!(rms, 0.0);
        let wall = crate::radio::EPOCH + 5.0 * 86400.0 + 600.0;
        let (day, sec) = wall_parts(wall);
        p.set_params(2.0, day as f32, sec as f32, 1.0, 1.0);
        // Static, then the station: a second or so.
        let (rms, peak) = render(&mut p, (3.0 * sr / 128.0) as usize);
        assert!(rms > 0.005, "the station is heard: rms {rms}");
        assert!(peak <= 1.0, "peak {peak}");
        assert_eq!(p.position().map(|x| x.0), Some(2));
        // Off again: the station into static, the static away, silence.
        p.set_params(-1.0, day as f32, sec as f32, 1.0, 1.0);
        render(&mut p, (1.5 * sr / 128.0) as usize);
        let (rms, _) = render(&mut p, 20);
        assert_eq!(rms, 0.0);
    }

    #[test]
    fn a_song_ending_hands_over_to_the_next_slot() {
        let sr = 48000.0;
        let mut p = Player::new(2026, sr);
        // Tune in a few seconds before the end of the block's first song.
        let st = &STATIONS[0];
        let slots = block(st, 3);
        let wall = crate::radio::EPOCH + 3.0 * crate::radio::BLOCK_SECS + slots[0].secs - 2.0;
        let (day, sec) = wall_parts(wall);
        p.set_params(0.0, day as f32, sec as f32, 1.0, 1.0);
        assert_eq!(p.position(), Some((0, 3, 0)));
        render(&mut p, (4.0 * sr / 128.0) as usize);
        assert_eq!(p.position(), Some((0, 3, 1)), "the next song plays");
    }
}
