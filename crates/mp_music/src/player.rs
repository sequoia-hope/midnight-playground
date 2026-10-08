//! The station player: what the radio node runs on the audio thread, the
//! same code in the browser's AudioWorklet (`wasm`) and in `mp_audio`'s
//! native worklet processor (radio.md 7, DECISIONS D1151).
//!
//! The node's params come once per block: `station` (an index in
//! [`STATIONS`], below zero off), the wall time at the moment of tuning as
//! two numbers (days since the Unix epoch and seconds into the day), a
//! `tune` serial, and `energy`. A change of station or serial re-tunes:
//! the cue for that wall time is computed ([`cue`]), the engine starts on
//! the song's next 16th with the bar's state set, and the tuner plays its
//! sweep and lock. From then on the player keeps the schedule itself: when
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
    engine: Engine,
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
            engine: Engine::new(seed, sample_rate),
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
            self.engine.set_energy(energy as f64);
        }
    }

    /// Tunes to `st` (or off) at wall time `wall`.
    fn retune(&mut self, st: Option<usize>, wall: f64) {
        let Some(i) = st else {
            self.engine.stop();
            if self.station.is_some() {
                self.tuner.off();
            }
            self.station = None;
            return;
        };
        let (b, into) = block_at(wall);
        self.slots = block(&STATIONS[i], b);
        let c = cue_in(b, &self.slots, into);
        self.block = c.block;
        self.slot = c.slot;
        let step_dur = 60.0 / c.track.bpm / 4.0;
        self.engine.set_track(&c.track);
        self.engine.set_energy(self.energy as f64);
        // On the next 16th, where the station's clock says it falls.
        self.engine.cue(c.step, (1.0 - c.frac) * step_dur);
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
        let t = &self.slots[self.slot].track;
        self.engine.set_track(t);
        self.engine.set_energy(self.energy as f64);
        self.engine.play(0, 0.0);
    }

    /// Renders one block (`l.len()` frames) of the station through the
    /// tuner; silence when off.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        if self.station.is_some() {
            self.engine.process(l, r);
            let ended = self
                .engine
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
        self.tuner.process(l, r);
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
        // The sweep, then the lock, then the station: a few seconds in all.
        let (rms, peak) = render(&mut p, (3.0 * sr / 128.0) as usize);
        assert!(rms > 0.005, "the station is heard: rms {rms}");
        assert!(peak <= 1.0, "peak {peak}");
        assert_eq!(p.position().map(|x| x.0), Some(2));
        // Off again: the click and the hiss, then silence.
        p.set_params(-1.0, day as f32, sec as f32, 1.0, 1.0);
        render(&mut p, 200);
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
