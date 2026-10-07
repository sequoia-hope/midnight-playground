//! The exhaust node's processor on the native backend: [`mp_exhaust`]'s
//! engine in an AudioWorklet processor, as `web/exhaust-worklet.js` runs it
//! in a browser (the same calls in the same order, so both render the same
//! samples).
//!
//! The params are read once per 128-frame block: `rpm`, `throttle`,
//! `boost` and `speed` become the engine's targets (it glides to them
//! itself), `running` fades it in or out, and `preset` rebuilds the engine
//! for another car when its rounded value changes. The engine is built on
//! the first block, where the sample rate is known.

use mp_exhaust::{Engine, ORDER, State, preset};
use web_audio_api::worklet::{AudioParamValues, AudioWorkletGlobalScope, AudioWorkletProcessor};
use web_audio_api::{AudioParamDescriptor, AutomationRate};

/// The engines' noise seed (the web worklet's too).
pub const SEED: u32 = 12345;

/// An engine for preset `index` (out of range: the first) at `state`.
pub fn engine(index: usize, state: State, sample_rate: f64) -> Engine {
    let key = ORDER.get(index).copied().unwrap_or(ORDER[0]);
    let p = preset(key).expect("ORDER names presets");
    Engine::new(p, state, SEED, sample_rate)
}

/// An engine for preset `index` as a new node starts it: idling (the
/// wasm's `mpx_new`).
pub fn idle_engine(index: usize, sample_rate: f64) -> Engine {
    let key = ORDER.get(index).copied().unwrap_or(ORDER[0]);
    let idle = preset(key).expect("ORDER names presets").idle;
    engine(
        index,
        State {
            rpm: idle,
            ..State::default()
        },
        sample_rate,
    )
}

/// The preset a `preset` param value names.
fn preset_index(v: f32) -> usize {
    let i = (v as f64 + 0.5).floor();
    if i >= 0.0 && i < ORDER.len() as f64 {
        i as usize
    } else {
        0
    }
}

pub struct ExhaustProcessor {
    preset: usize,
    engine: Option<Engine>,
}

impl AudioWorkletProcessor for ExhaustProcessor {
    /// The preset's index in [`ORDER`].
    type ProcessorOptions = usize;

    fn constructor(preset: usize) -> Self {
        ExhaustProcessor {
            preset,
            engine: None,
        }
    }

    fn parameter_descriptors() -> Vec<AudioParamDescriptor> {
        // Only each block's first value is read, as a browser's k-rate param
        // gives it. They are a-rate here because web-audio-api 1.7.0 gives
        // a k-rate param its value from before the block's events (an event
        // at a block's start shows a block late; Chrome applies it in that
        // block); an a-rate param's first value is the block-start value.
        let k =
            |name: &str, default_value: f32, min_value: f32, max_value: f32| AudioParamDescriptor {
                name: name.into(),
                automation_rate: AutomationRate::A,
                default_value,
                min_value,
                max_value,
            };
        vec![
            k("rpm", 800.0, 0.0, 30000.0),
            k("throttle", 0.0, 0.0, 1.0),
            k("boost", 0.0, 0.0, 1.0),
            k("speed", 0.0, f32::MIN, f32::MAX),
            k("running", 1.0, 0.0, 1.0),
            k("preset", 0.0, 0.0, (ORDER.len() - 1) as f32),
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
        let state = State {
            rpm: first("rpm") as f64,
            throttle: first("throttle") as f64,
            boost: first("boost") as f64,
            speed: first("speed") as f64,
        };
        let running = first("running") >= 0.5;
        let preset = preset_index(first("preset"));
        let sr = scope.sample_rate as f64;
        if self.engine.is_none() {
            self.engine = Some(idle_engine(self.preset, sr));
        }
        if preset != self.preset {
            // Another car: a new engine, already at the current state.
            self.preset = preset;
            self.engine = Some(engine(preset, state, sr));
        }
        let e = self.engine.as_mut().expect("built above");
        e.set_state(state, false);
        e.set_running(running);
        let [out, ..] = outputs else {
            return true;
        };
        match &mut out[..] {
            [l, r, ..] => e.process(l, r),
            [m] => {
                let mut r = vec![0.0; m.len()];
                e.process(m, &mut r);
            }
            [] => {}
        }
        // A source that keeps playing (silence once faded out) until it is
        // let go of.
        true
    }
}
