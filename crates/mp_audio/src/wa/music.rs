//! The radio node's processor on the native backend: [`mp_music`]'s
//! station player in an AudioWorklet processor, as `web/music-worklet.js`
//! runs it in a browser (the same calls in the same order, so both render
//! the same samples; DECISIONS D1151).
//!
//! The params are read once per 128-frame block and handed to the player
//! whole: `station` (an index in `mp_music::radio::STATIONS`, -1 off),
//! `wallDay` and `wallSec` (the wall time at tuning), `tune` (a serial; a
//! change re-syncs) and `energy`. The player is built on the first block,
//! where the sample rate is known.

use mp_music::player::Player;
use web_audio_api::worklet::{AudioParamValues, AudioWorkletGlobalScope, AudioWorkletProcessor};
use web_audio_api::{AudioParamDescriptor, AutomationRate};

/// The player's seed (the web worklet's too, `RADIO_SEED` in `web.rs`).
pub const SEED: u32 = 2026;

pub struct RadioProcessor {
    seed: u32,
    player: Option<Player>,
}

impl AudioWorkletProcessor for RadioProcessor {
    /// The player's seed.
    type ProcessorOptions = u32;

    fn constructor(seed: u32) -> Self {
        RadioProcessor { seed, player: None }
    }

    fn parameter_descriptors() -> Vec<AudioParamDescriptor> {
        // Only each block's first value is read, as a browser's k-rate param
        // gives it. They are a-rate here for the reason the exhaust's are
        // (`super::exhaust`): web-audio-api 1.7.0 gives a k-rate param its
        // value from before the block's events.
        let k =
            |name: &str, default_value: f32, min_value: f32, max_value: f32| AudioParamDescriptor {
                name: name.into(),
                automation_rate: AutomationRate::A,
                default_value,
                min_value,
                max_value,
            };
        vec![
            k("station", -1.0, -1.0, 63.0),
            k("wallDay", 0.0, 0.0, 1e6),
            k("wallSec", 0.0, 0.0, 86400.0),
            k("tune", 0.0, 0.0, 1e9),
            k("energy", 1.0, 0.0, 1.0),
        ]
    }

    fn process<'a, 'b>(
        &mut self,
        _inputs: &'b [&'a [&'a [f32]]],
        outputs: &'b mut [&'a mut [&'a mut [f32]]],
        params: AudioParamValues<'b>,
        scope: &'b AudioWorkletGlobalScope,
    ) -> bool {
        let first = |name: &str| params.get(name)[0];
        let (station, wall_day, wall_sec, tune, energy) = (
            first("station"),
            first("wallDay"),
            first("wallSec"),
            first("tune"),
            first("energy"),
        );
        let sr = scope.sample_rate as f64;
        let p = self
            .player
            .get_or_insert_with(|| Player::new(self.seed, sr));
        p.set_params(station, wall_day, wall_sec, tune, energy);
        let [out, ..] = outputs else {
            return true;
        };
        match &mut out[..] {
            [l, r, ..] => p.process(l, r),
            [m] => {
                let mut r = vec![0.0; m.len()];
                p.process(m, &mut r);
            }
            [] => {}
        }
        // A source that keeps playing (silence while off) until it is let
        // go of.
        true
    }
}
