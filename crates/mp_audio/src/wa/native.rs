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

use super::backend::{Attr, Backend, BufferId, NodeId, OfflineRender, Op, WaveId};
use super::compressor::ChromeCompressor;
use super::exhaust::ExhaustProcessor;
use super::oscillator::{BasicType, ChromeOscillator, OscMessage, WaveTables, basic_tables};
use super::timeline::{Event, EventKind, Timeline};
use super::{
    AudioError, BiquadFilterType, ContextState, Decoded, Dest, ErrorName, NodeKind, OscillatorType,
    OverSampleType, ParamId, ParamName, Pending, param_initial, params_of,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use web_audio_api::context::{
    AudioContext as WaContext, AudioContextLatencyCategory, AudioContextOptions, AudioContextState,
    BaseAudioContext, ConcreteBaseAudioContext, OfflineAudioContext,
};
use web_audio_api::node::{
    self as wn, AudioNode, AudioNodeOptions, AudioScheduledSourceNode, ChannelCountMode,
    ChannelInterpretation,
};
use web_audio_api::worklet::{AudioWorkletNode, AudioWorkletNodeOptions};
use web_audio_api::{AudioBuffer, AudioParam};

// One per live node, in a map: the size of the largest variant does not matter.
#[allow(clippy::large_enum_variant)]
enum NativeNode {
    Destination(wn::AudioDestinationNode),
    Gain(wn::GainNode),
    Biquad(wn::BiquadFilterNode),
    /// Chrome's oscillator (see [`super::oscillator`]).
    Osc(AudioWorkletNode),
    Src(wn::AudioBufferSourceNode),
    Shaper(wn::WaveShaperNode),
    /// Chrome's compressor kernel (see [`super::compressor`]).
    Comp(AudioWorkletNode),
    Conv(wn::ConvolverNode),
    Delay(wn::DelayNode),
    Merger(wn::ChannelMergerNode),
    Pan(wn::StereoPannerNode),
    Analyser(wn::AnalyserNode),
    /// The exhaust model (see [`super::exhaust`]).
    Exhaust(AudioWorkletNode),
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
            NativeNode::Exhaust(n) => n,
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
            (NativeNode::Osc(n), P::Frequency) => &n.parameters()["frequency"],
            (NativeNode::Osc(n), P::Detune) => &n.parameters()["detune"],
            (NativeNode::Src(n), P::PlaybackRate) => n.playback_rate(),
            (NativeNode::Src(n), P::Detune) => n.detune(),
            (NativeNode::Comp(n), P::Threshold) => &n.parameters()["threshold"],
            (NativeNode::Comp(n), P::Knee) => &n.parameters()["knee"],
            (NativeNode::Comp(n), P::Ratio) => &n.parameters()["ratio"],
            (NativeNode::Comp(n), P::Attack) => &n.parameters()["attack"],
            (NativeNode::Comp(n), P::Release) => &n.parameters()["release"],
            (NativeNode::Delay(n), P::DelayTime) => n.delay_time(),
            (NativeNode::Pan(n), P::Pan) => n.pan(),
            (NativeNode::Exhaust(n), p) => n.parameters().get(p.as_str())?,
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
    /// An offline context, to render steered (`Err`: a live one, given back).
    fn into_offline(self) -> Result<Box<dyn OfflineRender>, Self>
    where
        Self: Sized;
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

    fn into_offline(self) -> Result<Box<dyn OfflineRender>, Self> {
        Err(self)
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

    fn into_offline(self) -> Result<Box<dyn OfflineRender>, Self> {
        Ok(Box::new(Steered(self)))
    }
}

/// A steered render's control function.
type Control = Box<dyn FnMut(usize)>;

thread_local! {
    /// The control function of the steered render running on this thread.
    /// The crate's suspend callbacks must be `Send + 'static`; the render
    /// runs them on the calling thread, so they reach the (non-`Send`)
    /// control through here.
    static CONTROL: RefCell<Option<Control>> = RefCell::new(None);
}

/// An offline render steered every `frame` samples (`suspend(t)` then the
/// control, as the reference renders do, DECISIONS D45 and D253).
struct Steered(OfflineAudioContext);

impl OfflineRender for Steered {
    fn render(self: Box<Self>, frame: usize, control: Box<dyn FnMut(usize)>) -> Vec<Vec<f32>> {
        let mut c = self.0;
        let len = c.length();
        let sr = c.sample_rate() as f64;
        assert!(
            frame.is_multiple_of(128) && frame > 0,
            "a control frame of whole render quanta"
        );
        let mut k = 1;
        while k * frame < len {
            // The crate rounds a suspend time up to a render quantum; half a
            // quantum early lands exactly on frame k (currentTime there is
            // k * frame / sampleRate, as in Chrome).
            let t = (k * frame - 64) as f64 / sr;
            c.suspend_sync(t, move |_| {
                CONTROL.with(|cb| {
                    if let Some(f) = cb.borrow_mut().as_mut() {
                        f(k)
                    }
                })
            });
            k += 1;
        }
        CONTROL.with(|cb| *cb.borrow_mut() = Some(control));
        let b = c.start_rendering_sync();
        CONTROL.with(|cb| *cb.borrow_mut() = None);
        (0..b.number_of_channels())
            .map(|ch| b.get_channel_data(ch).to_vec())
            .collect()
    }
}

/// The `web-audio-api` crate behind the facade.
pub struct NativeBackend<C: NativeContext> {
    /// Taken out while an offline render runs (nodes are made on `base`).
    ctx: Option<C>,
    base: ConcreteBaseAudioContext,
    nodes: HashMap<NodeId, NativeNode>,
    buffers: HashMap<BufferId, AudioBuffer>,
    waves: HashMap<WaveId, Arc<WaveTables>>,
    tasks: Vec<Box<dyn FnOnce()>>,
    /// Each param's automation as the facade sent it (DECISIONS D254): the
    /// value a `setTargetAtTime` starts from.
    timelines: HashMap<ParamId, Timeline>,
    /// The graph's links by target node (a param's node for a param), and
    /// the nodes the destination pulls on (D258).
    ins: HashMap<NodeId, Vec<NodeId>>,
    reach: HashSet<NodeId>,
    /// Oscillators the facade let go of before anything pulled on them (an
    /// FM modulator wired up before its carrier): held until they are
    /// pulled, so they can be told to run.
    detached: HashMap<NodeId, NativeNode>,
    /// Live contexts only: let go of finished one-shots ([`Sweep`]).
    sweep: Option<Sweep>,
}

/// How long a released node's inputs must have been gone before it is
/// disconnected: past any filter's or reverb's tail.
const SWEEP_TAIL: f64 = 4.0;

/// Finished one-shots, let go of (DECISIONS D1009). web-audio-api 1.7.0 frees
/// a node whose handle is dropped once it reports no tail and nothing feeds
/// it, but some chains are never freed: the race's exhaust pops (a noise
/// burst through a band-pass, a shaper and a gain into the effects bus),
/// and now and then a plain gain after a buffer source, stay in the render
/// graph after their source has ended, processing silence every quantum. A
/// race added thousands (846 nodes at its start, 4,790 two minutes in) and
/// the render thread fell behind (load 0.27 at the start, 4 to 6 after two
/// and a half minutes, every callback underrunning). A browser collects
/// such a chain once nothing references it. So: a node the facade lets go
/// of is held here instead of dropped; once every node feeding it is gone
/// (a released source that has played out, or another released node let
/// go of before it) for [`SWEEP_TAIL`] seconds, it is disconnected and
/// dropped, and the crate frees it. A node fed by anything the facade
/// still holds is never touched. Offline contexts (the parity renders)
/// keep the crate's own behaviour.
#[derive(Default)]
struct Sweep {
    /// Released processing nodes, and since when their inputs were all gone.
    held: HashMap<NodeId, (NativeNode, Option<f64>)>,
    /// Released sources, until they have played out.
    sources: HashSet<NodeId>,
    /// Buffer sources that have ended (their `onended`, on the crate's
    /// event thread).
    ended: Arc<std::sync::Mutex<Vec<NodeId>>>,
    /// Oscillators' stop times.
    osc_stop: HashMap<NodeId, f64>,
}

/// Live contexts render without an output device (`set_silent`).
static SILENT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Makes the next live contexts render their graph without an output device
/// (web-audio-api's `"none"` sink): the same work, nothing heard. For runs
/// nobody listens to (pictures, smoke tests, agents' checks).
pub fn set_silent(on: bool) {
    SILENT.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// A live context's options: the latency hint, and the default device
/// (`native-device`, unless silent) or none.
fn live_options(latency_hint: Option<&str>) -> AudioContextOptions {
    AudioContextOptions {
        latency_hint: match latency_hint {
            Some("playback") => AudioContextLatencyCategory::Playback,
            Some("interactive") => AudioContextLatencyCategory::Interactive,
            _ => AudioContextLatencyCategory::Balanced,
        },
        sink_id: if cfg!(feature = "native-device")
            && !SILENT.load(std::sync::atomic::Ordering::Relaxed)
        {
            String::new()
        } else {
            "none".into()
        },
        ..AudioContextOptions::default()
    }
}

/// What the render thread reports each second: its average and peak load
/// (the share of each render quantum's time spent rendering) and the share
/// of the device's callbacks that underran.
pub type CapacityHook = Box<dyn FnMut(f64, f64, f64) + Send>;

static CAPACITY_HOOK: std::sync::Mutex<Option<CapacityHook>> = std::sync::Mutex::new(None);

/// Reports the next live context's render load to `hook`, once a second
/// (web-audio-api's `AudioRenderCapacity`).
pub fn on_render_capacity(hook: CapacityHook) {
    *CAPACITY_HOOK.lock().unwrap_or_else(|e| e.into_inner()) = Some(hook);
}

fn watch_capacity(ctx: &WaContext) {
    let Some(mut hook) = CAPACITY_HOOK
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
    else {
        return;
    };
    let cap = ctx.render_capacity();
    cap.set_onupdate(move |e| hook(e.average_load, e.peak_load, e.underrun_ratio));
    cap.start(web_audio_api::AudioRenderCapacityOptions {
        update_interval: 1.0,
    });
}

impl NativeBackend<WaContext> {
    /// A live context. Without the `native-device` feature it processes the
    /// graph without an output device (the `"none"` sink).
    pub fn live(latency_hint: Option<&str>) -> Self {
        let ctx = WaContext::new(live_options(latency_hint));
        watch_capacity(&ctx);
        let mut b = Self::with(ctx);
        b.sweep = Some(Sweep::default());
        b
    }

    /// [`NativeBackend::live`], or `None` where the output stream cannot be
    /// made (no audio device): a game without sound rather than a panic.
    pub fn try_live(latency_hint: Option<&str>) -> Option<Self> {
        let ctx = WaContext::try_new(live_options(latency_hint)).ok()?;
        watch_capacity(&ctx);
        let mut b = Self::with(ctx);
        b.sweep = Some(Sweep::default());
        Some(b)
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
            base: ctx.base().clone(),
            ctx: Some(ctx),
            nodes: HashMap::new(),
            buffers: HashMap::new(),
            waves: HashMap::new(),
            tasks: Vec::new(),
            timelines: HashMap::new(),
            ins: HashMap::new(),
            reach: HashSet::new(),
            detached: HashMap::new(),
            sweep: None,
        }
    }

    /// Lets go of what [`Sweep`] finds finished.
    fn sweep(&mut self) {
        let Some(mut sw) = self.sweep.take() else {
            return;
        };
        let now = self.base.current_time();
        let mut gone: Vec<NodeId> = Vec::new();
        {
            let mut ended = sw.ended.lock().unwrap_or_else(|e| e.into_inner());
            // One still held waits for its release.
            ended.retain(|n| {
                if sw.sources.contains(n) {
                    gone.push(*n);
                    false
                } else {
                    self.nodes.contains_key(n)
                }
            });
        }
        for (n, t) in &sw.osc_stop {
            if *t <= now && sw.sources.contains(n) {
                gone.push(*n);
            }
        }
        for n in &gone {
            sw.sources.remove(n);
            sw.osc_stop.remove(n);
        }
        // Released nodes whose inputs are all gone, to a fixed point (a
        // chain's nodes are released together).
        loop {
            let ready: Vec<NodeId> = sw
                .held
                .keys()
                .filter(|n| {
                    self.ins
                        .get(n)
                        .is_none_or(|v| v.iter().all(|u| !self.can_sound(*u, &sw)))
                })
                .copied()
                .collect();
            let mut changed = false;
            for n in ready {
                let t0 = match sw.held.get_mut(&n) {
                    Some((_, since)) => *since.get_or_insert(now),
                    None => continue,
                };
                if now - t0 >= SWEEP_TAIL
                    && let Some((node, _)) = sw.held.remove(&n)
                {
                    node.node().disconnect();
                    gone.push(n);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        // A node whose inputs came back to life (none do; a released node
        // cannot be reconnected) would start its wait again.
        if !gone.is_empty() {
            for n in &gone {
                self.ins.remove(n);
                self.reach.remove(n);
            }
            for v in self.ins.values_mut() {
                v.retain(|u| !gone.contains(u));
            }
        }
        self.sweep = Some(sw);
    }

    /// Whether `u`, feeding a released node, can still sound: held by the
    /// facade, a released source still playing, or a released node not
    /// yet let go of.
    fn can_sound(&self, u: NodeId, sw: &Sweep) -> bool {
        self.nodes.contains_key(&u)
            || self.detached.contains_key(&u)
            || sw.sources.contains(&u)
            || sw.held.contains_key(&u)
    }

    /// The crate's own context, for what the facade does not cover.
    pub fn raw(&self) -> &C {
        self.ctx.as_ref().expect("the context (not rendering)")
    }

    fn param(&self, p: ParamId) -> Option<&AudioParam> {
        self.nodes.get(&p.node).and_then(|n| n.param(p.name))
    }

    fn create(&self, kind: NodeKind, arg: Option<f64>) -> NativeNode {
        let c = &self.base;
        match kind {
            NodeKind::Destination => NativeNode::Destination(c.destination()),
            NodeKind::Gain => NativeNode::Gain(c.create_gain()),
            NodeKind::BiquadFilter => NativeNode::Biquad(c.create_biquad_filter()),
            NodeKind::Oscillator => NativeNode::Osc(AudioWorkletNode::new::<ChromeOscillator>(
                c,
                AudioWorkletNodeOptions {
                    number_of_inputs: 0,
                    number_of_outputs: 1,
                    output_channel_count: vec![1],
                    parameter_data: HashMap::new(),
                    processor_options: basic_tables(BasicType::Sine, c.sample_rate()),
                    audio_node_options: AudioNodeOptions {
                        channel_count: 1,
                        channel_count_mode: ChannelCountMode::Explicit,
                        channel_interpretation: ChannelInterpretation::Speakers,
                    },
                },
            )),
            NodeKind::BufferSource => NativeNode::Src(c.create_buffer_source()),
            NodeKind::WaveShaper => NativeNode::Shaper(c.create_wave_shaper()),
            NodeKind::DynamicsCompressor => {
                NativeNode::Comp(AudioWorkletNode::new::<ChromeCompressor>(
                    c,
                    AudioWorkletNodeOptions {
                        number_of_inputs: 1,
                        number_of_outputs: 1,
                        output_channel_count: vec![2],
                        parameter_data: HashMap::new(),
                        processor_options: c.sample_rate(),
                        audio_node_options: AudioNodeOptions {
                            channel_count: 2,
                            channel_count_mode: ChannelCountMode::ClampedMax,
                            channel_interpretation: ChannelInterpretation::Speakers,
                        },
                    },
                ))
            }
            NodeKind::Convolver => NativeNode::Conv(c.create_convolver()),
            NodeKind::Delay => NativeNode::Delay(c.create_delay(arg.unwrap_or(1.0))),
            NodeKind::ChannelMerger => {
                NativeNode::Merger(c.create_channel_merger(arg.unwrap_or(6.0) as usize))
            }
            NodeKind::StereoPanner => NativeNode::Pan(c.create_stereo_panner()),
            NodeKind::Analyser => NativeNode::Analyser(c.create_analyser()),
            NodeKind::Exhaust => {
                let preset = arg.unwrap_or(0.0) as usize;
                NativeNode::Exhaust(AudioWorkletNode::new::<ExhaustProcessor>(
                    c,
                    AudioWorkletNodeOptions {
                        number_of_inputs: 0,
                        number_of_outputs: 1,
                        output_channel_count: vec![2],
                        parameter_data: HashMap::from([("preset".into(), preset as f64)]),
                        processor_options: preset,
                        audio_node_options: AudioNodeOptions {
                            channel_count: 2,
                            channel_count_mode: ChannelCountMode::Explicit,
                            channel_interpretation: ChannelInterpretation::Speakers,
                        },
                    },
                ))
            }
        }
    }

    /// A node newly connected to what the destination pulls on: it and
    /// everything upstream of it are pulled too.
    fn pull_from(&mut self, node: NodeId) {
        let mut stack = vec![node];
        while let Some(n) = stack.pop() {
            if !self.reach.insert(n) {
                continue;
            }
            self.set_frozen(n, false);
            if let Some(v) = self.ins.get(&n) {
                stack.extend(v.iter().copied());
            }
        }
    }

    /// After a disconnect: what does the destination still pull on?
    fn recompute_reach(&mut self) {
        let mut reach = HashSet::new();
        let mut stack = vec![0];
        while let Some(n) = stack.pop() {
            if !reach.insert(n) {
                continue;
            }
            if let Some(v) = self.ins.get(&n) {
                stack.extend(v.iter().copied());
            }
        }
        let lost: Vec<NodeId> = self.reach.difference(&reach).copied().collect();
        self.reach = reach;
        for n in lost {
            self.set_frozen(n, true);
        }
    }

    /// Chrome renders only the nodes the destination pulls on: an
    /// oscillator cut off by a gate stops, phase and all, until it is
    /// connected again (D258).
    fn set_frozen(&mut self, node: NodeId, frozen: bool) {
        if let Some(NativeNode::Osc(o)) = self.nodes.get(&node) {
            o.port().post_message(OscMessage::Frozen(frozen));
        } else if !frozen && let Some(NativeNode::Osc(o)) = self.detached.remove(&node) {
            // Connected now: the crate keeps it alive without the handle.
            o.port().post_message(OscMessage::Frozen(false));
        }
    }

    /// Update a param's mirrored timeline (pruned to the present).
    fn mirror(&mut self, p: ParamId, f: impl FnOnce(&mut Timeline)) {
        let now = self.base.current_time();
        if let Some(tl) = self.timelines.get_mut(&p) {
            f(tl);
            if tl.events.len() > 16 {
                tl.prune(now);
            }
        }
    }

    /// An event just went into a param's timeline at `t`, before a target
    /// curve already sent to the crate: that curve's anchor (D254) holds
    /// the value the param had before this event, so the crate's events
    /// from `t` on are sent again from the mirror, each target anchored
    /// anew (`buzz.frequency.value = f0` after the burble's syllables are
    /// scheduled; D511).
    fn reanchor(&mut self, param: ParamId, t: f64) {
        let Some(tl) = self.timelines.get(&param) else {
            return;
        };
        let later_target = tl
            .events
            .iter()
            .any(|e| e.t > t && matches!(e.kind, EventKind::Target { .. }));
        if !later_target {
            return;
        }
        let Some(p) = self.param(param) else {
            return;
        };
        p.cancel_scheduled_values(t);
        for e in tl.events.iter().filter(|e| e.t >= t) {
            match e.kind {
                EventKind::Set => {
                    p.set_value_at_time(e.v as f32, e.t);
                }
                EventKind::Lin => {
                    p.linear_ramp_to_value_at_time(e.v as f32, e.t);
                }
                EventKind::Exp => {
                    p.exponential_ramp_to_value_at_time(e.v as f32, e.t);
                }
                EventKind::Target { tc } => {
                    p.set_value_at_time(tl.at(e.t) as f32, e.t);
                    p.set_target_at_time(e.v as f32, e.t, tc);
                }
            }
        }
    }

    /// `start(args)` on a source.
    fn start(&mut self, node: NodeId, args: &[f64]) {
        // Chrome starts a buffer at the frame nearest the offset (its read
        // index begins whole); the crate interpolates between two (D257).
        let sr = self.base.sample_rate() as f64;
        let mut a = [0.0; 3];
        let args: &[f64] = if args.len() >= 2 {
            a[..args.len()].copy_from_slice(args);
            a[1] = (a[1] * sr).round() / sr;
            &a[..args.len()]
        } else {
            args
        };
        match (self.nodes.get_mut(&node), args) {
            (Some(NativeNode::Osc(o)), []) => o.port().post_message(OscMessage::Start(0.0)),
            (Some(NativeNode::Osc(o)), [t, ..]) => o.port().post_message(OscMessage::Start(*t)),
            (Some(NativeNode::Src(s)), []) => s.start(),
            (Some(NativeNode::Src(s)), [t]) => s.start_at(*t),
            (Some(NativeNode::Src(s)), [t, o]) => s.start_at_with_offset(*t, *o),
            (Some(NativeNode::Src(s)), [t, o, d, ..]) => {
                s.start_at_with_offset_and_duration(*t, *o, *d)
            }
            _ => {}
        }
    }

    fn settle_later(&mut self, done: Pending<()>, r: Result<(), AudioError>) {
        self.tasks.push(Box::new(move || done.resolve(r)));
    }
}

impl<C: NativeContext> Backend for NativeBackend<C> {
    fn sample_rate(&self) -> f64 {
        self.base.sample_rate() as f64
    }

    fn current_time(&self) -> f64 {
        self.base.current_time()
    }

    fn state(&self) -> ContextState {
        match self.base.state() {
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
                if let (Some(sw), NativeNode::Src(s)) = (&self.sweep, &n) {
                    let ended = sw.ended.clone();
                    s.set_onended(move |_| {
                        ended.lock().unwrap_or_else(|e| e.into_inner()).push(node);
                    });
                }
                self.nodes.insert(node, n);
                if kind == NodeKind::Destination {
                    self.reach.insert(node);
                }
                let sr = self.base.sample_rate() as f64;
                for &p in params_of(kind) {
                    if let Some(v) = param_initial(kind, p, sr, arg) {
                        self.timelines
                            .insert(ParamId { node, name: p }, Timeline::new(v));
                    }
                }
            }
            Op::Attr { node, attr } => {
                let buffers = &self.buffers;
                let base = &self.base;
                let Some(n) = self.nodes.get_mut(&node) else {
                    return;
                };
                match (n, attr) {
                    (NativeNode::Osc(o), Attr::Type(s)) => {
                        let t = match OscillatorType::from_js(s) {
                            Some(OscillatorType::Square) => BasicType::Square,
                            Some(OscillatorType::Sawtooth) => BasicType::Sawtooth,
                            Some(OscillatorType::Triangle) => BasicType::Triangle,
                            _ => BasicType::Sine,
                        };
                        o.port()
                            .post_message(OscMessage::Wave(basic_tables(t, base.sample_rate())));
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
                let now = self.base.current_time();
                self.mirror(param, |tl| tl.set_value(v, now));
                if let Some(p) = self.param(param) {
                    p.set_value(v as f32);
                }
                self.reanchor(param, now);
            }
            Op::SetValue { param, v, t } => {
                self.mirror(param, |tl| {
                    tl.insert(Event {
                        kind: EventKind::Set,
                        v,
                        t,
                    })
                });
                if let Some(p) = self.param(param) {
                    p.set_value_at_time(v as f32, t);
                }
                self.reanchor(param, t);
            }
            Op::LinRamp { param, v, t } => {
                self.mirror(param, |tl| {
                    tl.insert(Event {
                        kind: EventKind::Lin,
                        v,
                        t,
                    })
                });
                if let Some(p) = self.param(param) {
                    p.linear_ramp_to_value_at_time(v as f32, t);
                }
                self.reanchor(param, t);
            }
            Op::ExpRamp { param, v, t } => {
                self.mirror(param, |tl| {
                    tl.insert(Event {
                        kind: EventKind::Exp,
                        v,
                        t,
                    })
                });
                if let Some(p) = self.param(param) {
                    p.exponential_ramp_to_value_at_time(v as f32, t);
                }
                self.reanchor(param, t);
            }
            Op::SetTarget { param, v, t, tc } => {
                // web-audio-api 1.7.0 evaluates a target curve that becomes
                // current before its start time (after a ramp, or a target
                // that another event ended) at that earlier time, where
                // e^(-(t - t0) / tc) explodes. Anchoring the curve with the
                // value the param holds at its start, in a set event at the
                // same time, makes it start where it should (D254).
                let mut hold = None;
                self.mirror(param, |tl| {
                    hold = Some(tl.at(t));
                    tl.insert(Event {
                        kind: EventKind::Target { tc },
                        v,
                        t,
                    });
                });
                if let Some(p) = self.param(param) {
                    if let Some(h) = hold {
                        p.set_value_at_time(h as f32, t);
                    }
                    p.set_target_at_time(v as f32, t, tc);
                }
                self.reanchor(param, t);
            }
            Op::Cancel { param, t } => {
                self.mirror(param, |tl| tl.cancel(t));
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
                let target = match to {
                    Dest::Node(d) => d,
                    Dest::Param(p) => p.node,
                };
                self.ins.entry(target).or_default().push(from);
                if self.reach.contains(&target) && !self.reach.contains(&from) {
                    self.pull_from(from);
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
                match to {
                    None => {
                        for v in self.ins.values_mut() {
                            v.retain(|n| *n != from);
                        }
                    }
                    Some(d) => {
                        let target = match d {
                            Dest::Node(d) => d,
                            Dest::Param(p) => p.node,
                        };
                        if let Some(v) = self.ins.get_mut(&target) {
                            v.retain(|n| *n != from);
                        }
                    }
                }
                self.recompute_reach();
            }
            Op::Start { node, args } => self.start(node, args),
            Op::Stop { node, args } => {
                if let Some(sw) = self.sweep.as_mut()
                    && matches!(self.nodes.get(&node), Some(NativeNode::Osc(_)))
                {
                    sw.osc_stop
                        .insert(node, args.first().copied().unwrap_or(0.0));
                }
                match (self.nodes.get_mut(&node), args) {
                    (Some(NativeNode::Osc(o)), []) => o.port().post_message(OscMessage::Stop(0.0)),
                    (Some(NativeNode::Osc(o)), [t, ..]) => {
                        o.port().post_message(OscMessage::Stop(*t))
                    }
                    (Some(NativeNode::Src(s)), []) => s.stop(),
                    (Some(NativeNode::Src(s)), [t, ..]) => {
                        s.stop_at(t - 0.5 / self.base.sample_rate() as f64)
                    }
                    _ => {}
                }
            }
            Op::SetPeriodicWave { node, wave } => {
                if let Some(w) = self.waves.get(&wave).cloned()
                    && let Some(NativeNode::Osc(o)) = self.nodes.get_mut(&node)
                {
                    o.port().post_message(OscMessage::Wave(w));
                }
            }
            Op::Wave {
                wave,
                real,
                imag,
                disable_normalization,
            } => {
                let w = WaveTables::new(
                    real,
                    imag,
                    disable_normalization.unwrap_or(false),
                    self.base.sample_rate(),
                );
                self.waves.insert(wave, Arc::new(w));
            }
            Op::Buffer {
                buffer,
                channels,
                length,
                sample_rate,
            } => {
                let b =
                    self.base
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
        let r = self.ctx.as_ref().map_or(Ok(()), |c| c.resume_now());
        self.settle_later(done, r);
    }

    fn suspend(&mut self, done: Pending<()>) {
        let r = self.ctx.as_ref().map_or(Ok(()), |c| c.suspend_now());
        self.settle_later(done, r);
    }

    fn close(&mut self, done: Pending<()>) {
        let r = self.ctx.as_ref().map_or(Ok(()), |c| c.close_now());
        self.settle_later(done, r);
    }

    fn decode(&mut self, buffer: BufferId, bytes: &[u8], done: Pending<Decoded>) {
        let r = match self
            .base
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

    fn prepare_exhaust(&mut self, done: Pending<bool>) {
        // The processor is compiled in: ready at the next settle.
        self.tasks.push(Box::new(move || done.resolve(Ok(true))));
    }

    fn release_node(&mut self, node: NodeId) {
        // (Its links stay in `ins`: the crate keeps a connected node alive,
        // as a browser does, until `sweep` lets go of it.)
        self.timelines.retain(|p, _| p.node != node);
        let Some(n) = self.nodes.remove(&node) else {
            return;
        };
        if let Some(sw) = self.sweep.as_mut() {
            match n {
                NativeNode::Src(_) | NativeNode::Osc(_) => {
                    sw.sources.insert(node);
                }
                NativeNode::Destination(_) => {}
                n => {
                    sw.held.insert(node, (n, None));
                    return;
                }
            }
        }
        if matches!(n, NativeNode::Osc(_)) && !self.reach.contains(&node) {
            self.detached.insert(node, n);
        }
    }

    fn release_buffer(&mut self, buffer: BufferId) {
        self.buffers.remove(&buffer);
    }

    fn release_wave(&mut self, wave: WaveId) {
        self.waves.remove(&wave);
    }

    fn settle(&mut self) -> Vec<Box<dyn FnOnce()>> {
        self.sweep();
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
        self.ctx.as_mut()?.render_now()
    }

    fn take_offline(&mut self) -> Option<Box<dyn OfflineRender>> {
        match self.ctx.take()?.into_offline() {
            Ok(r) => Some(r),
            Err(c) => {
                self.ctx = Some(c);
                None
            }
        }
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

/// A live native context, or `None` without an output device.
pub fn try_context(latency_hint: Option<&str>) -> Option<super::AudioContext> {
    NativeBackend::try_live(latency_hint).map(|b| {
        super::AudioContext::new(
            Box::new(b),
            super::ContextOptions {
                latency_hint: latency_hint.map(str::to_owned),
            },
        )
    })
}

/// An offline native context; render it with
/// [`super::AudioContext::start_rendering`].
pub fn offline_context(channels: usize, length: usize, sample_rate: f32) -> super::AudioContext {
    super::AudioContext::new(
        Box::new(NativeBackend::offline(channels, length, sample_rate)),
        super::ContextOptions::default(),
    )
}
