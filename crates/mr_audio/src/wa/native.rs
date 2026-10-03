//! The native backend (SPEC 7.1): the `web-audio-api` crate (1.7.0), a Rust
//! implementation of the same API, so the native client runs the same graph
//! as the browser. It renders offline (the L4 band comparison of SPEC 7.5)
//! and live; a live context plays through a device only with the
//! `native-device` feature (cpal), and otherwise processes the graph with
//! the crate's `"none"` sink.
//!
//! The crate panics on calls a browser would reject; the facade never
//! passes one on. Promises (`resume`, `suspend`, `close`,
//! `decodeAudioData`) settle at the next [`super::AudioContext::settle`],
//! which the client calls every frame.

use super::backend::{Attr, Backend, BufferId, NodeId, Op, WaveId};
use super::{
    AudioError, BiquadFilterType, ContextState, Decoded, Dest, ErrorName, NodeKind, OscillatorType,
    OverSampleType, ParamId, ParamName, Pending,
};
use std::collections::HashMap;
use web_audio_api::context::{
    AudioContext as WaContext, AudioContextLatencyCategory, AudioContextOptions, AudioContextState,
    BaseAudioContext, OfflineAudioContext,
};
use web_audio_api::node::{self as wn, AudioNode, AudioScheduledSourceNode};
use web_audio_api::{AudioBuffer, AudioParam, PeriodicWave, PeriodicWaveOptions};

// One per live node, in a map: the size of the largest variant does not matter.
#[allow(clippy::large_enum_variant)]
enum NativeNode {
    Destination(wn::AudioDestinationNode),
    Gain(wn::GainNode),
    Biquad(wn::BiquadFilterNode),
    Osc(wn::OscillatorNode),
    Src(wn::AudioBufferSourceNode),
    Shaper(wn::WaveShaperNode),
    Comp(wn::DynamicsCompressorNode),
    Conv(wn::ConvolverNode),
    Delay(wn::DelayNode),
    Merger(wn::ChannelMergerNode),
    Pan(wn::StereoPannerNode),
    Analyser(wn::AnalyserNode),
}

impl NativeNode {
    fn node(&self) -> &dyn AudioNode {
        match self {
            NativeNode::Destination(n) => n,
            NativeNode::Gain(n) => n,
            NativeNode::Biquad(n) => n,
            NativeNode::Osc(n) => n,
            NativeNode::Src(n) => n,
            NativeNode::Shaper(n) => n,
            NativeNode::Comp(n) => n,
            NativeNode::Conv(n) => n,
            NativeNode::Delay(n) => n,
            NativeNode::Merger(n) => n,
            NativeNode::Pan(n) => n,
            NativeNode::Analyser(n) => n,
        }
    }

    fn param(&self, name: ParamName) -> Option<&AudioParam> {
        use ParamName as P;
        Some(match (self, name) {
            (NativeNode::Gain(n), P::Gain) => n.gain(),
            (NativeNode::Biquad(n), P::Frequency) => n.frequency(),
            (NativeNode::Biquad(n), P::Detune) => n.detune(),
            (NativeNode::Biquad(n), P::Q) => n.q(),
            (NativeNode::Biquad(n), P::Gain) => n.gain(),
            (NativeNode::Osc(n), P::Frequency) => n.frequency(),
            (NativeNode::Osc(n), P::Detune) => n.detune(),
            (NativeNode::Src(n), P::PlaybackRate) => n.playback_rate(),
            (NativeNode::Src(n), P::Detune) => n.detune(),
            (NativeNode::Comp(n), P::Threshold) => n.threshold(),
            (NativeNode::Comp(n), P::Knee) => n.knee(),
            (NativeNode::Comp(n), P::Ratio) => n.ratio(),
            (NativeNode::Comp(n), P::Attack) => n.attack(),
            (NativeNode::Comp(n), P::Release) => n.release(),
            (NativeNode::Delay(n), P::DelayTime) => n.delay_time(),
            (NativeNode::Pan(n), P::Pan) => n.pan(),
            _ => return None,
        })
    }
}

/// What differs between a live and an offline context.
pub trait NativeContext: BaseAudioContext {
    fn resume_now(&self) -> Result<(), AudioError>;
    fn suspend_now(&self) -> Result<(), AudioError>;
    fn close_now(&self) -> Result<(), AudioError>;
    fn render_now(&mut self) -> Option<Vec<Vec<f32>>>;
}

fn offline_only(what: &str) -> AudioError {
    AudioError::new(
        ErrorName::InvalidStateError,
        format!("{what}() on an offline context"),
    )
}

impl NativeContext for WaContext {
    fn resume_now(&self) -> Result<(), AudioError> {
        self.resume_sync();
        Ok(())
    }

    fn suspend_now(&self) -> Result<(), AudioError> {
        self.suspend_sync();
        Ok(())
    }

    fn close_now(&self) -> Result<(), AudioError> {
        self.close_sync();
        Ok(())
    }

    fn render_now(&mut self) -> Option<Vec<Vec<f32>>> {
        None
    }
}

impl NativeContext for OfflineAudioContext {
    fn resume_now(&self) -> Result<(), AudioError> {
        Err(offline_only("resume"))
    }

    fn suspend_now(&self) -> Result<(), AudioError> {
        Err(offline_only("suspend"))
    }

    fn close_now(&self) -> Result<(), AudioError> {
        Err(offline_only("close"))
    }

    fn render_now(&mut self) -> Option<Vec<Vec<f32>>> {
        let b = self.start_rendering_sync();
        Some(
            (0..b.number_of_channels())
                .map(|c| b.get_channel_data(c).to_vec())
                .collect(),
        )
    }
}

/// The `web-audio-api` crate behind the facade.
pub struct NativeBackend<C: NativeContext> {
    ctx: C,
    nodes: HashMap<NodeId, NativeNode>,
    buffers: HashMap<BufferId, AudioBuffer>,
    waves: HashMap<WaveId, PeriodicWave>,
    tasks: Vec<Box<dyn FnOnce()>>,
}

impl NativeBackend<WaContext> {
    /// A live context. Without the `native-device` feature it processes the
    /// graph without an output device (the `"none"` sink).
    pub fn live(latency_hint: Option<&str>) -> Self {
        let opts = AudioContextOptions {
            latency_hint: match latency_hint {
                Some("playback") => AudioContextLatencyCategory::Playback,
                Some("interactive") => AudioContextLatencyCategory::Interactive,
                _ => AudioContextLatencyCategory::Balanced,
            },
            sink_id: if cfg!(feature = "native-device") {
                String::new()
            } else {
                "none".into()
            },
            ..AudioContextOptions::default()
        };
        Self::with(WaContext::new(opts))
    }
}

impl NativeBackend<OfflineAudioContext> {
    /// An offline context (`new OfflineAudioContext(channels, length,
    /// sampleRate)`).
    pub fn offline(channels: usize, length: usize, sample_rate: f32) -> Self {
        Self::with(OfflineAudioContext::new(channels, length, sample_rate))
    }
}

impl<C: NativeContext> NativeBackend<C> {
    fn with(ctx: C) -> Self {
        NativeBackend {
            ctx,
            nodes: HashMap::new(),
            buffers: HashMap::new(),
            waves: HashMap::new(),
            tasks: Vec::new(),
        }
    }

    /// The crate's own context, for what the facade does not cover.
    pub fn raw(&self) -> &C {
        &self.ctx
    }

    fn param(&self, p: ParamId) -> Option<&AudioParam> {
        self.nodes.get(&p.node).and_then(|n| n.param(p.name))
    }

    fn create(&self, kind: NodeKind, arg: Option<f64>) -> NativeNode {
        let c = &self.ctx;
        match kind {
            NodeKind::Destination => NativeNode::Destination(c.destination()),
            NodeKind::Gain => NativeNode::Gain(c.create_gain()),
            NodeKind::BiquadFilter => NativeNode::Biquad(c.create_biquad_filter()),
            NodeKind::Oscillator => NativeNode::Osc(c.create_oscillator()),
            NodeKind::BufferSource => NativeNode::Src(c.create_buffer_source()),
            NodeKind::WaveShaper => NativeNode::Shaper(c.create_wave_shaper()),
            NodeKind::DynamicsCompressor => NativeNode::Comp(c.create_dynamics_compressor()),
            NodeKind::Convolver => NativeNode::Conv(c.create_convolver()),
            NodeKind::Delay => NativeNode::Delay(c.create_delay(arg.unwrap_or(1.0))),
            NodeKind::ChannelMerger => {
                NativeNode::Merger(c.create_channel_merger(arg.unwrap_or(6.0) as usize))
            }
            NodeKind::StereoPanner => NativeNode::Pan(c.create_stereo_panner()),
            NodeKind::Analyser => NativeNode::Analyser(c.create_analyser()),
        }
    }

    fn settle_later(&mut self, done: Pending<()>, r: Result<(), AudioError>) {
        self.tasks.push(Box::new(move || done.resolve(r)));
    }
}

impl<C: NativeContext> Backend for NativeBackend<C> {
    fn sample_rate(&self) -> f64 {
        self.ctx.sample_rate() as f64
    }

    fn current_time(&self) -> f64 {
        self.ctx.current_time()
    }

    fn state(&self) -> ContextState {
        match self.ctx.state() {
            AudioContextState::Running => ContextState::Running,
            AudioContextState::Closed => ContextState::Closed,
            AudioContextState::Suspended => ContextState::Suspended,
        }
    }

    fn apply(&mut self, op: &Op) {
        match *op {
            Op::Context { .. } | Op::Resume | Op::Suspend | Op::Close => {}
            Op::New { node, kind, arg } => {
                let n = self.create(kind, arg);
                self.nodes.insert(node, n);
            }
            Op::Attr { node, attr } => {
                let buffers = &self.buffers;
                let Some(n) = self.nodes.get_mut(&node) else {
                    return;
                };
                match (n, attr) {
                    (NativeNode::Osc(o), Attr::Type(s)) => {
                        o.set_type(match OscillatorType::from_js(s) {
                            Some(OscillatorType::Square) => wn::OscillatorType::Square,
                            Some(OscillatorType::Sawtooth) => wn::OscillatorType::Sawtooth,
                            Some(OscillatorType::Triangle) => wn::OscillatorType::Triangle,
                            _ => wn::OscillatorType::Sine,
                        })
                    }
                    (NativeNode::Biquad(b), Attr::Type(s)) => {
                        b.set_type(match BiquadFilterType::from_js(s) {
                            Some(BiquadFilterType::Highpass) => wn::BiquadFilterType::Highpass,
                            Some(BiquadFilterType::Bandpass) => wn::BiquadFilterType::Bandpass,
                            Some(BiquadFilterType::Lowshelf) => wn::BiquadFilterType::Lowshelf,
                            Some(BiquadFilterType::Highshelf) => wn::BiquadFilterType::Highshelf,
                            Some(BiquadFilterType::Peaking) => wn::BiquadFilterType::Peaking,
                            Some(BiquadFilterType::Notch) => wn::BiquadFilterType::Notch,
                            Some(BiquadFilterType::Allpass) => wn::BiquadFilterType::Allpass,
                            _ => wn::BiquadFilterType::Lowpass,
                        })
                    }
                    (NativeNode::Src(s), Attr::Loop(on)) => s.set_loop(on),
                    (NativeNode::Src(s), Attr::LoopStart(t)) => s.set_loop_start(t),
                    (NativeNode::Src(s), Attr::LoopEnd(t)) => s.set_loop_end(t),
                    (NativeNode::Src(s), Attr::Buffer(Some(b))) => {
                        if let Some(b) = buffers.get(&b) {
                            s.set_buffer(b.clone());
                        }
                    }
                    (NativeNode::Conv(c), Attr::Buffer(Some(b))) => {
                        if let Some(b) = buffers.get(&b) {
                            c.set_buffer(b.clone());
                        }
                    }
                    (NativeNode::Conv(c), Attr::Normalize(on)) => c.set_normalize(on),
                    (NativeNode::Shaper(w), Attr::Curve(Some(c))) => w.set_curve(c.to_vec()),
                    (NativeNode::Shaper(w), Attr::Oversample(s)) => {
                        w.set_oversample(match OverSampleType::from_js(s) {
                            Some(OverSampleType::X2) => wn::OverSampleType::X2,
                            Some(OverSampleType::X4) => wn::OverSampleType::X4,
                            _ => wn::OverSampleType::None,
                        })
                    }
                    (NativeNode::Analyser(a), Attr::FftSize(f)) => a.set_fft_size(f as usize),
                    (NativeNode::Analyser(a), Attr::SmoothingTimeConstant(s)) => {
                        a.set_smoothing_time_constant(s)
                    }
                    // Unsetting a buffer or a curve: the crate has no way to;
                    // the game never does.
                    _ => {}
                }
            }
            Op::Value { param, v } => {
                if let Some(p) = self.param(param) {
                    p.set_value(v as f32);
                }
            }
            Op::SetValue { param, v, t } => {
                if let Some(p) = self.param(param) {
                    p.set_value_at_time(v as f32, t);
                }
            }
            Op::LinRamp { param, v, t } => {
                if let Some(p) = self.param(param) {
                    p.linear_ramp_to_value_at_time(v as f32, t);
                }
            }
            Op::ExpRamp { param, v, t } => {
                if let Some(p) = self.param(param) {
                    p.exponential_ramp_to_value_at_time(v as f32, t);
                }
            }
            Op::SetTarget { param, v, t, tc } => {
                if let Some(p) = self.param(param) {
                    p.set_target_at_time(v as f32, t, tc);
                }
            }
            Op::Cancel { param, t } => {
                if let Some(p) = self.param(param) {
                    p.cancel_scheduled_values(t);
                }
            }
            Op::Connect {
                from,
                to,
                output,
                input,
            } => {
                let Some(src) = self.nodes.get(&from) else {
                    return;
                };
                let (o, i) = (output.unwrap_or(0) as usize, input.unwrap_or(0) as usize);
                let dest: Option<&dyn AudioNode> = match to {
                    Dest::Node(d) => self.nodes.get(&d).map(|n| n.node()),
                    Dest::Param(p) => self.param(p).map(|p| p as &dyn AudioNode),
                };
                if let Some(d) = dest {
                    src.node().connect_from_output_to_input(d, o, i);
                }
            }
            Op::Disconnect { from, to } => {
                let Some(src) = self.nodes.get(&from) else {
                    return;
                };
                match to {
                    None => src.node().disconnect(),
                    Some(Dest::Node(d)) => {
                        if let Some(d) = self.nodes.get(&d) {
                            src.node().disconnect_dest(d.node());
                        }
                    }
                    Some(Dest::Param(p)) => {
                        if let Some(p) = self.param(p) {
                            src.node().disconnect_dest(p);
                        }
                    }
                }
            }
            Op::Start { node, args } => match (self.nodes.get_mut(&node), args) {
                (Some(NativeNode::Osc(o)), []) => o.start(),
                (Some(NativeNode::Osc(o)), [t, ..]) => o.start_at(*t),
                (Some(NativeNode::Src(s)), []) => s.start(),
                (Some(NativeNode::Src(s)), [t]) => s.start_at(*t),
                (Some(NativeNode::Src(s)), [t, o]) => s.start_at_with_offset(*t, *o),
                (Some(NativeNode::Src(s)), [t, o, d, ..]) => {
                    s.start_at_with_offset_and_duration(*t, *o, *d)
                }
                _ => {}
            },
            Op::Stop { node, args } => match (self.nodes.get_mut(&node), args) {
                (Some(NativeNode::Osc(o)), []) => o.stop(),
                (Some(NativeNode::Osc(o)), [t, ..]) => o.stop_at(*t),
                (Some(NativeNode::Src(s)), []) => s.stop(),
                (Some(NativeNode::Src(s)), [t, ..]) => s.stop_at(*t),
                _ => {}
            },
            Op::SetPeriodicWave { node, wave } => {
                if let Some(w) = self.waves.get(&wave).cloned()
                    && let Some(NativeNode::Osc(o)) = self.nodes.get_mut(&node)
                {
                    o.set_periodic_wave(w);
                }
            }
            Op::Wave {
                wave,
                real,
                imag,
                disable_normalization,
            } => {
                let w = self.ctx.create_periodic_wave(PeriodicWaveOptions {
                    real: Some(real.to_vec()),
                    imag: Some(imag.to_vec()),
                    disable_normalization: disable_normalization.unwrap_or(false),
                });
                self.waves.insert(wave, w);
            }
            Op::Buffer {
                buffer,
                channels,
                length,
                sample_rate,
            } => {
                let b =
                    self.ctx
                        .create_buffer(channels as usize, length as usize, sample_rate as f32);
                self.buffers.insert(buffer, b);
            }
            Op::Data { buffer, channels } => {
                if let Some(b) = self.buffers.get_mut(&buffer) {
                    for (c, d) in channels.iter().enumerate() {
                        b.copy_to_channel(d, c);
                    }
                }
            }
        }
    }

    fn param_value(&mut self, param: ParamId) -> f64 {
        self.param(param).map_or(0.0, |p| p.value() as f64)
    }

    fn resume(&mut self, done: Pending<()>) {
        let r = self.ctx.resume_now();
        self.settle_later(done, r);
    }

    fn suspend(&mut self, done: Pending<()>) {
        let r = self.ctx.suspend_now();
        self.settle_later(done, r);
    }

    fn close(&mut self, done: Pending<()>) {
        let r = self.ctx.close_now();
        self.settle_later(done, r);
    }

    fn decode(&mut self, buffer: BufferId, bytes: &[u8], done: Pending<Decoded>) {
        let r = match self
            .ctx
            .decode_audio_data_sync(std::io::Cursor::new(bytes.to_vec()))
        {
            Ok(b) => {
                let d = Decoded {
                    channels: b.number_of_channels() as u32,
                    length: b.length() as u32,
                    sample_rate: b.sample_rate() as f64,
                };
                self.buffers.insert(buffer, b);
                Ok(d)
            }
            Err(e) => Err(AudioError::new(ErrorName::EncodingError, e.to_string())),
        };
        self.tasks.push(Box::new(move || done.resolve(r)));
    }

    fn release_node(&mut self, node: NodeId) {
        self.nodes.remove(&node);
    }

    fn release_buffer(&mut self, buffer: BufferId) {
        self.buffers.remove(&buffer);
    }

    fn release_wave(&mut self, wave: WaveId) {
        self.waves.remove(&wave);
    }

    fn settle(&mut self) -> Vec<Box<dyn FnOnce()>> {
        std::mem::take(&mut self.tasks)
    }

    fn analyser_time_domain(&mut self, node: NodeId, out: &mut [f32]) {
        match self.nodes.get_mut(&node) {
            Some(NativeNode::Analyser(a)) => a.get_float_time_domain_data(out),
            _ => out.fill(0.0),
        }
    }

    fn analyser_byte_frequency(&mut self, node: NodeId, out: &mut [u8]) {
        match self.nodes.get_mut(&node) {
            Some(NativeNode::Analyser(a)) => a.get_byte_frequency_data(out),
            _ => out.fill(0),
        }
    }

    fn render(&mut self) -> Option<Vec<Vec<f32>>> {
        self.ctx.render_now()
    }
}

/// A live native context.
pub fn context(latency_hint: Option<&str>) -> super::AudioContext {
    super::AudioContext::new(
        Box::new(NativeBackend::live(latency_hint)),
        super::ContextOptions {
            latency_hint: latency_hint.map(str::to_owned),
        },
    )
}

/// An offline native context; render it with
/// [`super::AudioContext::start_rendering`].
pub fn offline_context(channels: usize, length: usize, sample_rate: f32) -> super::AudioContext {
    super::AudioContext::new(
        Box::new(NativeBackend::offline(channels, length, sample_rate)),
        super::ContextOptions::default(),
    )
}
