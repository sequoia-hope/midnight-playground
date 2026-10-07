//! The player's engine on the physical exhaust model (`mp_exhaust`, the
//! engine lab's model; docs/vision/sound.md 2.1). A deliberate change from
//! the JS game, made at the owner's request before cutover (DEVIATIONS
//! D1110): with it on, the player's combustion engine, its gear whine, its
//! turbo and its overrun pops come from the model, and the wavetable engine
//! stays silent. Rivals keep their wavetable voices (sound.md 2.3: the full
//! model costs too much for every car on a phone). Off (the default of
//! [`GameAudio::new`], so every parity run and call-log test is untouched),
//! nothing here creates a node.
//!
//! One exhaust node lives as long as the graph: a car change only moves its
//! `preset` param, so no worklet is ever left processing unheard. It plays
//! through its own shift and misfire dips (the wavetable engine's
//! `shiftG` and `misG`, mirrored) and a level into the SFX bus, behind a
//! gate like every continuous voice.

use super::{GATE_TAIL, GameAudio, GateId, clamp};
use crate::wa::{ExhaustNode, GainNode, Node, Pending};

/// The model's preset for each car (the lab's `gameCar`, one each).
pub fn preset_for(car: &str) -> Option<&'static str> {
    Some(match car {
        "sports" => "flatV8",
        "muscle" => "crossV8",
        "super" => "v10",
        "rally" => "i4turbo",
        _ => return None,
    })
}

/// The model's output level into the SFX bus, per preset, so that each car
/// sits where its wavetable engine did (the lab mixes for itself, through
/// its own compressor and makeup gain). Measured: see the `exhaust` tests.
fn trim(preset: &str) -> f64 {
    match preset {
        "flatV8" => 1.26,
        "crossV8" => 1.02,
        "v10" => 1.30,
        "i4turbo" => 0.88,
        _ => 0.85,
    }
}

pub(crate) struct ExVoice {
    node: ExhaustNode,
    pub shift: GainNode,
    pub mis: GainNode,
    out: GainNode,
    gate: GateId,
    preset: &'static str,
}

/// Where the model stands.
#[derive(Default)]
pub(crate) enum ExState {
    /// Not asked for yet.
    #[default]
    Idle,
    /// Loading (the web's worklet module and wasm).
    Preparing(Pending<bool>),
    /// Ready, with the voice built on first use.
    Ready(Option<ExVoice>),
    /// This platform cannot run it: the wavetable engine plays.
    Unavailable,
}

impl GameAudio {
    /// The owner's choice (the menu's "Classic engine sound", unticked):
    /// the player's engine on the physical model where the platform can run
    /// it. Takes effect at the next `update`.
    pub fn set_engine_model(&mut self, on: bool) {
        self.ex_want = on;
    }

    /// Whether the model voices the player's engine now.
    pub fn engine_model_active(&self) -> bool {
        self.ex_live
    }

    /// How many exhaust nodes this GameAudio has made (tests: one at most).
    pub fn exhaust_nodes(&self) -> u32 {
        self.ex_made
    }

    /// The exhaust voice for this frame, if the model is to play: starts the
    /// loading the first time, builds the voice once loaded, follows the
    /// car. `None`: the wavetable engine plays (and the model, if built, is
    /// faded out).
    fn ex_voice(&mut self) -> Option<&ExVoice> {
        let preset = if self.ex_want && !self.electric {
            preset_for(self.car)
        } else {
            None
        };
        if preset.is_some() && matches!(self.ex, ExState::Idle) {
            let ctx = self.ctx.clone()?;
            self.ex = ExState::Preparing(ctx.prepare_exhaust());
        }
        if let ExState::Preparing(p) = &self.ex {
            match p.result() {
                None => return None,
                Some(Ok(true)) => self.ex = ExState::Ready(None),
                Some(_) => self.ex = ExState::Unavailable,
            }
        }
        let preset = preset?;
        if let ExState::Ready(None) = self.ex {
            let v = self.build_exhaust(preset);
            self.ex = ExState::Ready(Some(v));
        }
        let ExState::Ready(Some(v)) = &mut self.ex else {
            return None;
        };
        if v.preset != preset {
            let idx = mp_exhaust::ORDER
                .iter()
                .position(|k| *k == preset)
                .unwrap_or(0);
            let t = self.ctx.as_ref().map_or(0.0, |c| c.current_time());
            v.node.preset.set_value_at_time(idx as f64, t);
            v.preset = preset;
        }
        let ExState::Ready(Some(v)) = &self.ex else {
            unreachable!()
        };
        Some(v)
    }

    fn build_exhaust(&mut self, preset: &'static str) -> ExVoice {
        let ctx = self.ctx.clone().expect("a context");
        let g = self.graph();
        let node = ctx.create_exhaust(preset);
        self.ex_made += 1;
        node.running.set_value(0.0);
        let shift = ctx.create_gain();
        let mis = ctx.create_gain();
        let out = ctx.create_gain();
        out.gain.set_value(0.0);
        let _ = node.connect(&shift);
        let _ = shift.connect(&mis);
        let _ = mis.connect(&out);
        // Built connected, as every gate's voice is.
        let _ = out.connect(&g.sfx_bus);
        let gate = self.add_gate(
            vec![(Node::clone(&out), Node::clone(&g.sfx_bus))],
            GATE_TAIL,
            vec![out.gain.clone()],
        );
        // Shut until the first update opens it.
        self.gate_set(gate, false, 0.0);
        ExVoice {
            node,
            shift,
            mis,
            out,
            gate,
            preset,
        }
    }

    /// Steers the model for this frame (`update`'s combustion branch);
    /// returns whether it plays, in which case the wavetable engine, its
    /// whine, its turbo and its pops stay quiet.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn steer_exhaust(
        &mut self,
        t: f64,
        rpm: f64,
        rpm_max: f64,
        thr: f64,
        speed: f64,
        boost: f64,
        engine_off: bool,
    ) -> bool {
        let Some(v) = self.ex_voice() else {
            self.ex_live = false;
            self.ex_quiet(t);
            return false;
        };
        let (gate, preset) = (v.gate, v.preset);
        self.ex_live = true;
        if !self.gate_set(gate, !engine_off, t) {
            return true;
        }
        let ExState::Ready(Some(v)) = &self.ex else {
            return true;
        };
        let p = mp_exhaust::preset(preset).expect("a preset");
        // The game's rpm runs from 800 to rpmMax; the model's from its idle
        // to its redline. Map one range onto the other, so the limiter, the
        // overrun and the pops come where the game's rpm puts them.
        let k = clamp((rpm - 800.0) / (rpm_max - 800.0), -0.1, 1.05);
        let mrpm = p.idle + k * (p.redline - p.idle);
        v.node.rpm.set_value_at_time(mrpm, t);
        v.node.throttle.set_value_at_time(thr, t);
        v.node
            .boost
            .set_value_at_time(if p.turbo != 0.0 { boost } else { 0.0 }, t);
        v.node.speed.set_value_at_time(speed, t);
        v.node
            .running
            .set_value_at_time(if engine_off { 0.0 } else { 1.0 }, t);
        v.out.gain.set_target_at_time(trim(preset), t, 0.05);
        true
    }

    /// The model is not playing this frame: fade it and let its gate shut.
    pub(super) fn ex_quiet(&mut self, t: f64) {
        let gate = match &self.ex {
            ExState::Ready(Some(v)) => {
                v.node.running.set_value_at_time(0.0, t);
                v.gate
            }
            _ => return,
        };
        self.gate_set(gate, false, t);
    }

    /// The exhaust voice, if built (shift and misfire dips).
    pub(super) fn ex_built(&self) -> Option<&ExVoice> {
        match &self.ex {
            ExState::Ready(Some(v)) if self.ex_live => Some(v),
            _ => None,
        }
    }
}
