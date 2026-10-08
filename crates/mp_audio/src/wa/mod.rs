//! `mp_audio::wa`: handles and methods shaped like Web Audio, limited to
//! what the game uses (SPEC 7.2), over a [`Backend`].
//!
//! The JS game's code ports call for call: `ctx.create_gain()`,
//! `g.gain.set_target_at_time(v, t, tc)`, `osc.connect(&lp)`,
//! `src.start_with(t, offset, None)`. Handles are reference-counted like the
//! JS objects they stand for; dropping the last handle on a node lets the
//! backend drop its own reference (the browser then keeps the node alive
//! only while it plays, as it would for an unreferenced JS node).
//!
//! **Validation.** Every call is checked before the backend sees it, the
//! way a browser checks it (SPEC 7.5): what a browser rejects with an
//! exception (non-finite values, negative times, an exponential ramp to
//! zero, a negative time constant, a second `start()`, `stop()` before
//! `start()`, disconnecting a link that is not there, mismatched periodic
//! wave arrays, connecting across contexts or to a missing input or output)
//! is not carried out and returns or records an [`AudioError`]; what a
//! browser ignores with a console warning (an invalid enum string, a value
//! outside a param's nominal range, a cycle without a delay, which browsers
//! mute) is recorded as a [`Problem`]. In strict mode
//! ([`AudioContext::set_strict`]) the first problem of either kind panics:
//! that is the null backend's validation mode, the strict fake of the JS
//! tests.

pub mod backend;
pub mod log;
pub mod timeline;

#[cfg(feature = "null")]
pub mod null;

#[cfg(all(feature = "web", target_arch = "wasm32"))]
pub mod web;

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod native;

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod compressor;

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod oscillator;

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod exhaust;

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod music;

pub use backend::{Attr, Backend, BufferId, NodeId, OfflineRender, Op, WaveId};

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::rc::{Rc, Weak};

// ── Names ──────────────────────────────────────────────────────────

/// The node types the game uses (`create<Kind>`), with the name the call
/// log gives them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Destination,
    Gain,
    BiquadFilter,
    Oscillator,
    BufferSource,
    WaveShaper,
    DynamicsCompressor,
    Convolver,
    Delay,
    ChannelMerger,
    StereoPanner,
    Analyser,
    /// The physical exhaust model (`mp_exhaust`) in an AudioWorklet: no
    /// inputs, one stereo output. Its argument is the preset's index in
    /// [`mp_exhaust::ORDER`].
    Exhaust,
    /// The radio: `mp_music`'s station player in an AudioWorklet
    /// (DECISIONS D1151): no inputs, one stereo output, no argument.
    Radio,
}

impl NodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            NodeKind::Destination => "Destination",
            NodeKind::Gain => "Gain",
            NodeKind::BiquadFilter => "BiquadFilter",
            NodeKind::Oscillator => "Oscillator",
            NodeKind::BufferSource => "BufferSource",
            NodeKind::WaveShaper => "WaveShaper",
            NodeKind::DynamicsCompressor => "DynamicsCompressor",
            NodeKind::Convolver => "Convolver",
            NodeKind::Delay => "Delay",
            NodeKind::ChannelMerger => "ChannelMerger",
            NodeKind::StereoPanner => "StereoPanner",
            NodeKind::Analyser => "Analyser",
            NodeKind::Exhaust => "Exhaust",
            NodeKind::Radio => "Radio",
        }
    }

    /// `numberOfInputs` (the merger's is its argument).
    fn inputs(self, arg: Option<f64>) -> u32 {
        match self {
            NodeKind::Oscillator | NodeKind::BufferSource | NodeKind::Exhaust | NodeKind::Radio => {
                0
            }
            NodeKind::ChannelMerger => arg.unwrap_or(6.0) as u32,
            _ => 1,
        }
    }

    /// `numberOfOutputs`.
    fn outputs(self) -> u32 {
        match self {
            // The destination has one output by the spec, but nothing can
            // usefully take it; the game never connects from it.
            NodeKind::Destination => 1,
            _ => 1,
        }
    }

    fn scheduled(self) -> bool {
        matches!(self, NodeKind::Oscillator | NodeKind::BufferSource)
    }
}

/// The AudioParams of the node types above, by their JS names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParamName {
    Gain,
    Frequency,
    Detune,
    Q,
    PlaybackRate,
    Pan,
    DelayTime,
    Threshold,
    Knee,
    Ratio,
    Attack,
    Release,
    // The exhaust node's (read once per 128-frame block).
    Rpm,
    Throttle,
    Boost,
    Speed,
    Running,
    Preset,
    // The radio node's (read once per block too).
    Station,
    WallDay,
    WallSec,
    Tune,
    Energy,
}

impl ParamName {
    pub fn as_str(self) -> &'static str {
        match self {
            ParamName::Gain => "gain",
            ParamName::Frequency => "frequency",
            ParamName::Detune => "detune",
            ParamName::Q => "Q",
            ParamName::PlaybackRate => "playbackRate",
            ParamName::Pan => "pan",
            ParamName::DelayTime => "delayTime",
            ParamName::Threshold => "threshold",
            ParamName::Knee => "knee",
            ParamName::Ratio => "ratio",
            ParamName::Attack => "attack",
            ParamName::Release => "release",
            ParamName::Rpm => "rpm",
            ParamName::Throttle => "throttle",
            ParamName::Boost => "boost",
            ParamName::Speed => "speed",
            ParamName::Running => "running",
            ParamName::Preset => "preset",
            ParamName::Station => "station",
            ParamName::WallDay => "wallDay",
            ParamName::WallSec => "wallSec",
            ParamName::Tune => "tune",
            ParamName::Energy => "energy",
        }
    }
}

/// A node's param.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ParamId {
    pub node: NodeId,
    pub name: ParamName,
}

/// Where a connection goes: a node's input or a param.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dest {
    Node(NodeId),
    Param(ParamId),
}

impl Dest {
    fn node(self) -> NodeId {
        match self {
            Dest::Node(n) => n,
            Dest::Param(p) => p.node,
        }
    }
}

/// Upper end of a "most positive single float" range.
const F32_MAX: f64 = f32::MAX as f64;
/// The detune range (1200 · log2 of the largest float).
const DETUNE_MAX: f64 = 153600.0;

/// A param's default value and nominal range, by Web Audio 1.0.
pub fn param_spec(
    kind: NodeKind,
    name: ParamName,
    sample_rate: f64,
    max_delay: f64,
) -> Option<(f64, f64, f64)> {
    let nyq = sample_rate / 2.0;
    use NodeKind as K;
    use ParamName as P;
    Some(match (kind, name) {
        (K::Gain, P::Gain) => (1.0, -F32_MAX, F32_MAX),
        (K::Delay, P::DelayTime) => (0.0, 0.0, max_delay),
        (K::StereoPanner, P::Pan) => (0.0, -1.0, 1.0),
        (K::DynamicsCompressor, P::Threshold) => (-24.0, -100.0, 0.0),
        (K::DynamicsCompressor, P::Knee) => (30.0, 0.0, 40.0),
        (K::DynamicsCompressor, P::Ratio) => (12.0, 1.0, 20.0),
        (K::DynamicsCompressor, P::Attack) => (0.003, 0.0, 1.0),
        (K::DynamicsCompressor, P::Release) => (0.25, 0.0, 1.0),
        (K::BiquadFilter, P::Frequency) => (350.0, 0.0, nyq),
        (K::BiquadFilter, P::Detune) => (0.0, -DETUNE_MAX, DETUNE_MAX),
        (K::BiquadFilter, P::Q) => (1.0, -F32_MAX, F32_MAX),
        // 40 · log10 of the largest float.
        (K::BiquadFilter, P::Gain) => (0.0, -F32_MAX, 1541.273681640625),
        (K::Oscillator, P::Frequency) => (440.0, -nyq, nyq),
        (K::Oscillator, P::Detune) => (0.0, -DETUNE_MAX, DETUNE_MAX),
        (K::BufferSource, P::PlaybackRate) => (1.0, -F32_MAX, F32_MAX),
        (K::BufferSource, P::Detune) => (0.0, -F32_MAX, F32_MAX),
        (K::Exhaust, P::Rpm) => (800.0, 0.0, 30000.0),
        (K::Exhaust, P::Throttle) => (0.0, 0.0, 1.0),
        (K::Exhaust, P::Boost) => (0.0, 0.0, 1.0),
        (K::Exhaust, P::Speed) => (0.0, -F32_MAX, F32_MAX),
        (K::Exhaust, P::Running) => (1.0, 0.0, 1.0),
        (K::Exhaust, P::Preset) => (0.0, 0.0, (mp_exhaust::ORDER.len() - 1) as f64),
        // The station is an index in `mp_music::radio::STATIONS` (-1: off);
        // the wall time at tuning comes in two parts because an f32 cannot
        // hold Unix seconds; `tune` is a serial.
        (K::Radio, P::Station) => (-1.0, -1.0, 63.0),
        (K::Radio, P::WallDay) => (0.0, 0.0, 1e6),
        (K::Radio, P::WallSec) => (0.0, 0.0, 86400.0),
        (K::Radio, P::Tune) => (0.0, 0.0, 1e9),
        (K::Radio, P::Energy) => (1.0, 0.0, 1.0),
        _ => return None,
    })
}

/// The params a node of this kind has, in the order the fake creates them.
pub fn params_of(kind: NodeKind) -> &'static [ParamName] {
    use ParamName as P;
    match kind {
        NodeKind::Gain => &[P::Gain],
        NodeKind::Delay => &[P::DelayTime],
        NodeKind::StereoPanner => &[P::Pan],
        NodeKind::DynamicsCompressor => &[P::Threshold, P::Knee, P::Ratio, P::Attack, P::Release],
        NodeKind::BiquadFilter => &[P::Frequency, P::Detune, P::Q, P::Gain],
        NodeKind::Oscillator => &[P::Frequency, P::Detune],
        NodeKind::BufferSource => &[P::PlaybackRate, P::Detune],
        NodeKind::Exhaust => &[
            P::Rpm,
            P::Throttle,
            P::Boost,
            P::Speed,
            P::Running,
            P::Preset,
        ],
        NodeKind::Radio => &[P::Station, P::WallDay, P::WallSec, P::Tune, P::Energy],
        _ => &[],
    }
}

/// The value a new node's param starts at: its default, except an exhaust
/// node's `preset`, which starts at the preset the node was made with
/// (`parameterData` in a browser), so its first block needs no rebuild.
pub fn param_initial(
    kind: NodeKind,
    name: ParamName,
    sample_rate: f64,
    arg: Option<f64>,
) -> Option<f64> {
    if (kind, name) == (NodeKind::Exhaust, ParamName::Preset) {
        return Some(arg.unwrap_or(0.0));
    }
    let max_delay = arg.unwrap_or(1.0);
    param_spec(kind, name, sample_rate, max_delay).map(|(v, _, _)| v)
}

/// The index in [`mp_exhaust::ORDER`] of an exhaust preset (`None`: not a
/// preset).
pub fn exhaust_preset_index(key: &str) -> Option<usize> {
    mp_exhaust::ORDER.iter().position(|k| *k == key)
}

/// `OscillatorType` (`'custom'` comes only from `setPeriodicWave`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OscillatorType {
    Sine,
    Square,
    Sawtooth,
    Triangle,
}

impl OscillatorType {
    pub fn as_str(self) -> &'static str {
        match self {
            OscillatorType::Sine => "sine",
            OscillatorType::Square => "square",
            OscillatorType::Sawtooth => "sawtooth",
            OscillatorType::Triangle => "triangle",
        }
    }

    pub fn from_js(s: &str) -> Option<Self> {
        Some(match s {
            "sine" => OscillatorType::Sine,
            "square" => OscillatorType::Square,
            "sawtooth" => OscillatorType::Sawtooth,
            "triangle" => OscillatorType::Triangle,
            _ => return None,
        })
    }
}

/// `BiquadFilterType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BiquadFilterType {
    Lowpass,
    Highpass,
    Bandpass,
    Lowshelf,
    Highshelf,
    Peaking,
    Notch,
    Allpass,
}

impl BiquadFilterType {
    pub fn as_str(self) -> &'static str {
        match self {
            BiquadFilterType::Lowpass => "lowpass",
            BiquadFilterType::Highpass => "highpass",
            BiquadFilterType::Bandpass => "bandpass",
            BiquadFilterType::Lowshelf => "lowshelf",
            BiquadFilterType::Highshelf => "highshelf",
            BiquadFilterType::Peaking => "peaking",
            BiquadFilterType::Notch => "notch",
            BiquadFilterType::Allpass => "allpass",
        }
    }

    pub fn from_js(s: &str) -> Option<Self> {
        Some(match s {
            "lowpass" => BiquadFilterType::Lowpass,
            "highpass" => BiquadFilterType::Highpass,
            "bandpass" => BiquadFilterType::Bandpass,
            "lowshelf" => BiquadFilterType::Lowshelf,
            "highshelf" => BiquadFilterType::Highshelf,
            "peaking" => BiquadFilterType::Peaking,
            "notch" => BiquadFilterType::Notch,
            "allpass" => BiquadFilterType::Allpass,
            _ => return None,
        })
    }
}

/// `OverSampleType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverSampleType {
    None,
    X2,
    X4,
}

impl OverSampleType {
    pub fn as_str(self) -> &'static str {
        match self {
            OverSampleType::None => "none",
            OverSampleType::X2 => "2x",
            OverSampleType::X4 => "4x",
        }
    }

    pub fn from_js(s: &str) -> Option<Self> {
        Some(match s {
            "none" => OverSampleType::None,
            "2x" => OverSampleType::X2,
            "4x" => OverSampleType::X4,
            _ => return None,
        })
    }
}

/// `AudioContextState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextState {
    Suspended,
    Running,
    Closed,
}

impl ContextState {
    pub fn as_str(self) -> &'static str {
        match self {
            ContextState::Suspended => "suspended",
            ContextState::Running => "running",
            ContextState::Closed => "closed",
        }
    }
}

// ── Errors and problems ────────────────────────────────────────────

/// The DOMException (or TypeError) a browser throws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorName {
    TypeError,
    RangeError,
    InvalidStateError,
    InvalidAccessError,
    IndexSizeError,
    NotSupportedError,
    EncodingError,
}

impl ErrorName {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorName::TypeError => "TypeError",
            ErrorName::RangeError => "RangeError",
            ErrorName::InvalidStateError => "InvalidStateError",
            ErrorName::InvalidAccessError => "InvalidAccessError",
            ErrorName::IndexSizeError => "IndexSizeError",
            ErrorName::NotSupportedError => "NotSupportedError",
            ErrorName::EncodingError => "EncodingError",
        }
    }
}

/// A call a browser rejects with an exception.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioError {
    pub name: ErrorName,
    pub message: String,
}

impl AudioError {
    pub fn new(name: ErrorName, message: impl Into<String>) -> Self {
        AudioError {
            name,
            message: message.into(),
        }
    }
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.name.as_str(), self.message)
    }
}

impl std::error::Error for AudioError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProblemKind {
    /// A browser throws: the call was not carried out.
    Throw(ErrorName),
    /// A browser ignores the call with a console warning (an invalid enum
    /// value): not carried out.
    Ignored,
    /// A browser carries the call out with a console warning (a value
    /// outside the nominal range, clamped) or silently does something the
    /// game cannot want (a cycle without a delay is muted).
    Warning,
}

/// Something the strict validation flags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub kind: ProblemKind,
    pub message: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            ProblemKind::Throw(n) => write!(f, "{} (a {} in browsers)", self.message, n.as_str()),
            ProblemKind::Ignored => write!(f, "{} (ignored by browsers)", self.message),
            ProblemKind::Warning => write!(f, "{} (a browser warning)", self.message),
        }
    }
}

enum Verdict {
    Ok,
    Warn(String),
    Ignore(String),
    Throw(AudioError),
}

// ── Pending results ────────────────────────────────────────────────

type Callback<T> = Box<dyn FnOnce(&Result<T, AudioError>)>;

struct PendingState<T> {
    result: Option<Result<T, AudioError>>,
    callbacks: Vec<Callback<T>>,
}

/// A promise: settled by the backend (the browser's promise, or the null
/// backend's [`AudioContext::settle`]), polled by the game or followed with
/// [`Pending::then`].
pub struct Pending<T>(Rc<RefCell<PendingState<T>>>);

impl<T> Clone for Pending<T> {
    fn clone(&self) -> Self {
        Pending(self.0.clone())
    }
}

impl<T: Clone> Default for Pending<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> Pending<T> {
    pub fn new() -> Self {
        Pending(Rc::new(RefCell::new(PendingState {
            result: None,
            callbacks: Vec::new(),
        })))
    }

    /// Settle it (once; later calls are ignored), then run the callbacks.
    pub fn resolve(&self, r: Result<T, AudioError>) {
        let callbacks = {
            let mut s = self.0.borrow_mut();
            if s.result.is_some() {
                return;
            }
            s.result = Some(r.clone());
            std::mem::take(&mut s.callbacks)
        };
        for cb in callbacks {
            cb(&r);
        }
    }

    pub fn is_settled(&self) -> bool {
        self.0.borrow().result.is_some()
    }

    /// Run `f` once settled (at once if it is).
    pub fn then(&self, f: impl FnOnce(&Result<T, AudioError>) + 'static) {
        let done = self.0.borrow().result.clone();
        match done {
            Some(r) => f(&r),
            None => self.0.borrow_mut().callbacks.push(Box::new(f)),
        }
    }

    /// The result, once settled.
    pub fn result(&self) -> Option<Result<T, AudioError>> {
        self.0.borrow().result.clone()
    }
}

/// What a backend decoded a clip to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decoded {
    pub channels: u32,
    pub length: u32,
    pub sample_rate: f64,
}

// ── The graph the facade checks against ───────────────────────────

#[derive(Clone, Debug)]
struct Link {
    to: Dest,
    output: u32,
    input: u32,
}

#[derive(Debug)]
struct NodeInfo {
    kind: NodeKind,
    inputs: u32,
    outputs: u32,
    max_delay: f64,
    links: Vec<Link>,
    started: bool,
    has_buffer: bool,
}

#[derive(Debug)]
struct BufferInfo {
    channels: u32,
    length: u32,
    sample_rate: f64,
    data: Vec<Vec<f32>>,
    used: bool,
    decoded: bool,
}

#[derive(Default)]
struct Graph {
    nodes: HashMap<NodeId, NodeInfo>,
    buffers: HashMap<BufferId, BufferInfo>,
    waves: HashMap<WaveId, usize>,
    next_node: NodeId,
    next_buffer: BufferId,
    next_wave: WaveId,
}

/// The fake's `_check`: the first non-finite argument throws a TypeError,
/// then the first negative time a RangeError (same messages as the fake).
fn check_times(p: ParamId, what: &str, args: &[f64], times: &[f64]) -> Option<AudioError> {
    if let Some(x) = args.iter().find(|x| !x.is_finite()) {
        return Some(AudioError::new(
            ErrorName::TypeError,
            format!(
                "{}.{what}: non-finite {}",
                param_name(p),
                log::js_number(*x)
            ),
        ));
    }
    if let Some(t) = times.iter().find(|t| **t < 0.0) {
        return Some(AudioError::new(
            ErrorName::RangeError,
            format!(
                "{}.{what}: negative time {}",
                param_name(p),
                log::js_number(*t)
            ),
        ));
    }
    None
}

fn dest_name(d: Dest) -> String {
    match d {
        Dest::Node(n) => format!("n{n}"),
        Dest::Param(p) => format!("n{}.{}", p.node, p.name.as_str()),
    }
}

fn param_name(p: ParamId) -> String {
    dest_name(Dest::Param(p))
}

impl Graph {
    fn range_check(&self, p: ParamId, what: &str, v: f64, sample_rate: f64) -> Verdict {
        let Some(n) = self.nodes.get(&p.node) else {
            return Verdict::Ok;
        };
        if let Some((_, lo, hi)) = param_spec(n.kind, p.name, sample_rate, n.max_delay)
            && !(v >= lo && v <= hi)
        {
            return Verdict::Warn(format!(
                "{}.{what} {v} outside the nominal range [{lo}, {hi}]",
                param_name(p)
            ));
        }
        Verdict::Ok
    }

    /// Check a call against the graph as a browser would, and commit what
    /// it changes in the graph when it goes through.
    fn admit(&mut self, op: &Op, sample_rate: f64) -> Verdict {
        use ErrorName as E;
        let throw = |name, msg: String| Verdict::Throw(AudioError::new(name, msg));
        match *op {
            Op::Value { param, v } => {
                if !v.is_finite() {
                    return throw(
                        E::TypeError,
                        format!("{}.value = {}", param_name(param), log::js_number(v)),
                    );
                }
                self.range_check(param, "value", v, sample_rate)
            }
            Op::SetValue { param, v, t }
            | Op::LinRamp { param, v, t }
            | Op::ExpRamp { param, v, t } => {
                let what = match op {
                    Op::SetValue { .. } => "setValueAtTime",
                    Op::LinRamp { .. } => "linearRampToValueAtTime",
                    _ => "exponentialRampToValueAtTime",
                };
                if let Some(e) = check_times(param, what, &[v, t], &[t]) {
                    return Verdict::Throw(e);
                }
                if matches!(op, Op::ExpRamp { .. }) && v == 0.0 {
                    return throw(
                        E::RangeError,
                        format!("{}.exponentialRampToValueAtTime to 0", param_name(param)),
                    );
                }
                self.range_check(param, what, v, sample_rate)
            }
            Op::SetTarget { param, v, t, tc } => {
                if let Some(e) = check_times(param, "setTargetAtTime", &[v, t, tc], &[t]) {
                    return Verdict::Throw(e);
                }
                if tc < 0.0 {
                    return throw(
                        E::RangeError,
                        format!(
                            "{}.setTargetAtTime time constant {}",
                            param_name(param),
                            log::js_number(tc)
                        ),
                    );
                }
                self.range_check(param, "setTargetAtTime", v, sample_rate)
            }
            Op::Cancel { param, t } => {
                match check_times(param, "cancelScheduledValues", &[t], &[t]) {
                    Some(e) => Verdict::Throw(e),
                    None => Verdict::Ok,
                }
            }
            Op::New { node, kind, arg } => {
                match kind {
                    NodeKind::Delay => {
                        let max = arg.unwrap_or(1.0);
                        if !(max > 0.0 && max < 180.0) {
                            return throw(E::NotSupportedError, format!("createDelay({max})"));
                        }
                    }
                    NodeKind::ChannelMerger => {
                        let n = arg.unwrap_or(6.0);
                        if !((1.0..=32.0).contains(&n) && n.fract() == 0.0) {
                            return throw(E::IndexSizeError, format!("createChannelMerger({n})"));
                        }
                    }
                    NodeKind::Exhaust => {
                        let i = arg.unwrap_or(0.0);
                        let n = mp_exhaust::ORDER.len() as f64;
                        if !(i >= 0.0 && i < n && i.fract() == 0.0) {
                            return throw(E::NotSupportedError, format!("createExhaust({i})"));
                        }
                    }
                    _ => {}
                }
                self.nodes.insert(
                    node,
                    NodeInfo {
                        kind,
                        inputs: kind.inputs(arg),
                        outputs: kind.outputs(),
                        max_delay: if kind == NodeKind::Delay {
                            arg.unwrap_or(1.0)
                        } else {
                            0.0
                        },
                        links: Vec::new(),
                        started: false,
                        has_buffer: false,
                    },
                );
                Verdict::Ok
            }
            Op::Attr { node, attr } => {
                let Some(n) = self.nodes.get_mut(&node) else {
                    return Verdict::Ok;
                };
                match attr {
                    Attr::Type("custom") if n.kind == NodeKind::Oscillator => {
                        return throw(E::InvalidStateError, format!("n{node}.type = 'custom'"));
                    }
                    Attr::Type(s)
                        if n.kind == NodeKind::Oscillator
                            && OscillatorType::from_js(s).is_none() =>
                    {
                        return Verdict::Ignore(format!("Oscillator.type = {s:?}"));
                    }
                    Attr::Type(s)
                        if n.kind == NodeKind::BiquadFilter
                            && BiquadFilterType::from_js(s).is_none() =>
                    {
                        return Verdict::Ignore(format!("BiquadFilter.type = {s:?}"));
                    }
                    Attr::Oversample(s) if OverSampleType::from_js(s).is_none() => {
                        return Verdict::Ignore(format!("WaveShaper.oversample = {s:?}"));
                    }
                    Attr::Curve(Some(c)) => {
                        if c.len() < 2 {
                            return throw(
                                E::InvalidStateError,
                                format!("n{node}.curve of length {}", c.len()),
                            );
                        }
                    }
                    Attr::LoopStart(x) | Attr::LoopEnd(x) | Attr::SmoothingTimeConstant(x) => {
                        if !x.is_finite() {
                            return throw(E::TypeError, format!("n{node}.{} = {x}", attr.name()));
                        }
                        if let Attr::SmoothingTimeConstant(x) = attr
                            && !(0.0..=1.0).contains(&x)
                        {
                            return throw(E::IndexSizeError, format!("smoothingTimeConstant {x}"));
                        }
                    }
                    Attr::FftSize(f) => {
                        if !(32..=32768).contains(&f) || !f.is_power_of_two() {
                            return throw(E::IndexSizeError, format!("fftSize {f}"));
                        }
                    }
                    Attr::Buffer(b) => {
                        if n.kind == NodeKind::BufferSource {
                            if n.has_buffer && b.is_some() {
                                return throw(
                                    E::InvalidStateError,
                                    format!("n{node}.buffer set twice"),
                                );
                            }
                            n.has_buffer = b.is_some();
                        } else if let Some(b) = b
                            && let Some(bi) = self.buffers.get(&b)
                        {
                            if ![1, 2, 4].contains(&bi.channels) {
                                return throw(
                                    E::NotSupportedError,
                                    format!("a convolver buffer of {} channels", bi.channels),
                                );
                            }
                            if bi.sample_rate != sample_rate {
                                return throw(
                                    E::NotSupportedError,
                                    format!("a convolver buffer at {} Hz", bi.sample_rate),
                                );
                            }
                        }
                    }
                    _ => {}
                }
                Verdict::Ok
            }
            Op::Connect {
                from,
                to,
                output,
                input,
            } => {
                let (o, i) = (output.unwrap_or(0), input.unwrap_or(0));
                let Some(src) = self.nodes.get(&from) else {
                    return Verdict::Ok;
                };
                if o >= src.outputs {
                    return throw(E::IndexSizeError, format!("n{from}.connect: output {o}"));
                }
                if let Dest::Node(d) = to {
                    let inputs = self.nodes.get(&d).map_or(1, |n| n.inputs);
                    if i >= inputs {
                        return throw(
                            E::IndexSizeError,
                            format!("n{from}.connect(n{d}): input {i} of {inputs}"),
                        );
                    }
                }
                let cycle = self.cycle_without_delay(from, to.node());
                let n = self.nodes.get_mut(&from).expect("source");
                if !n
                    .links
                    .iter()
                    .any(|l| l.to == to && l.output == o && l.input == i)
                {
                    n.links.push(Link {
                        to,
                        output: o,
                        input: i,
                    });
                }
                if cycle {
                    return Verdict::Warn(format!(
                        "n{from}.connect({}) closes a cycle without a delay (muted by browsers)",
                        dest_name(to)
                    ));
                }
                Verdict::Ok
            }
            Op::Disconnect { from, to } => {
                let Some(n) = self.nodes.get_mut(&from) else {
                    return Verdict::Ok;
                };
                match to {
                    None => n.links.clear(),
                    Some(d) => {
                        let before = n.links.len();
                        n.links.retain(|l| l.to != d);
                        if n.links.len() == before {
                            return throw(
                                E::InvalidAccessError,
                                format!("n{from}.disconnect({}): not connected", dest_name(d)),
                            );
                        }
                    }
                }
                Verdict::Ok
            }
            Op::Start { node, args } => {
                if !args.iter().all(|x| x.is_finite() && *x >= 0.0) {
                    return throw(E::RangeError, format!("n{node}.start({})", join(args)));
                }
                let Some(n) = self.nodes.get_mut(&node) else {
                    return Verdict::Ok;
                };
                if n.started {
                    return throw(E::InvalidStateError, format!("n{node}.start() twice"));
                }
                n.started = true;
                Verdict::Ok
            }
            Op::Stop { node, args } => {
                if !args.iter().all(|x| x.is_finite() && *x >= 0.0) {
                    return throw(E::RangeError, format!("n{node}.stop({})", join(args)));
                }
                if let Some(n) = self.nodes.get(&node)
                    && !n.started
                {
                    return throw(
                        E::InvalidStateError,
                        format!("n{node}.stop() before start()"),
                    );
                }
                Verdict::Ok
            }
            Op::Wave { real, imag, .. } => {
                if real.len() != imag.len() || real.len() < 2 {
                    return throw(
                        E::IndexSizeError,
                        "createPeriodicWave: arrays of different or too short lengths".into(),
                    );
                }
                Verdict::Ok
            }
            Op::Buffer {
                channels,
                length,
                sample_rate: sr,
                ..
            } => {
                if !((1..=32).contains(&channels)
                    && length >= 1
                    && (3000.0..=768000.0).contains(&sr))
                {
                    return throw(
                        E::NotSupportedError,
                        format!("createBuffer({channels}, {length}, {})", log::js_number(sr)),
                    );
                }
                Verdict::Ok
            }
            _ => Verdict::Ok,
        }
    }

    /// Would a link from `from` into `to` close a loop with no DelayNode on
    /// it? (Browsers mute such a cycle.)
    fn cycle_without_delay(&self, from: NodeId, to: NodeId) -> bool {
        let is_delay = |n: NodeId| {
            self.nodes
                .get(&n)
                .is_some_and(|i| i.kind == NodeKind::Delay)
        };
        if is_delay(from) || is_delay(to) {
            return false;
        }
        let mut seen = std::collections::HashSet::new();
        let mut stack = vec![to];
        while let Some(n) = stack.pop() {
            if n == from {
                return true;
            }
            if !seen.insert(n) {
                continue;
            }
            if let Some(info) = self.nodes.get(&n) {
                for l in &info.links {
                    let m = l.to.node();
                    if !is_delay(m) {
                        stack.push(m);
                    }
                }
            }
        }
        false
    }
}

fn join(args: &[f64]) -> String {
    args.iter()
        .map(|x| log::js_number(*x))
        .collect::<Vec<_>>()
        .join(", ")
}

// ── The context ────────────────────────────────────────────────────

struct Inner {
    backend: RefCell<Box<dyn Backend>>,
    graph: RefCell<Graph>,
    sample_rate: f64,
    strict: Cell<bool>,
    problems: RefCell<Vec<Problem>>,
    released: RefCell<Vec<Release>>,
    destination: RefCell<Option<Node>>,
}

enum Release {
    Node(NodeId),
    Buffer(BufferId),
    Wave(WaveId),
}

/// Options for a new context.
#[derive(Clone, Debug, Default)]
pub struct ContextOptions {
    /// `latencyHint` (`'balanced'`, `'playback'`, `'interactive'`).
    pub latency_hint: Option<String>,
}

/// An `AudioContext` (or an offline one, by its backend).
#[derive(Clone)]
pub struct AudioContext(Rc<Inner>);

impl AudioContext {
    /// A context on `backend`: logs the context and makes the destination
    /// (node 0), as `new AudioContext(opts)` does.
    pub fn new(backend: Box<dyn Backend>, opts: ContextOptions) -> Self {
        let sample_rate = backend.sample_rate();
        let ctx = AudioContext(Rc::new(Inner {
            backend: RefCell::new(backend),
            graph: RefCell::new(Graph::default()),
            sample_rate,
            strict: Cell::new(false),
            problems: RefCell::new(Vec::new()),
            released: RefCell::new(Vec::new()),
            destination: RefCell::new(None),
        }));
        {
            let mut g = ctx.0.graph.borrow_mut();
            g.next_buffer = 1;
            g.next_wave = 1;
        }
        let _ = ctx.call(Op::Context {
            sample_rate,
            latency_hint: opts.latency_hint.as_deref(),
        });
        let dest = ctx.new_node(NodeKind::Destination, None);
        *ctx.0.destination.borrow_mut() = Some(dest);
        ctx
    }

    /// Strict validation: panic at the first problem (SPEC 7.5).
    pub fn set_strict(&self, strict: bool) {
        self.0.strict.set(strict);
    }

    pub fn strict(&self) -> bool {
        self.0.strict.get()
    }

    /// Everything validation flagged so far.
    pub fn problems(&self) -> Vec<Problem> {
        self.0.problems.borrow().clone()
    }

    pub fn same(&self, other: &AudioContext) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    pub fn sample_rate(&self) -> f64 {
        self.0.sample_rate
    }

    pub fn current_time(&self) -> f64 {
        self.0.backend.borrow().current_time()
    }

    pub fn state(&self) -> ContextState {
        self.0.backend.borrow().state()
    }

    pub fn destination(&self) -> Node {
        self.0
            .destination
            .borrow()
            .clone()
            .expect("the destination")
    }

    /// Run a closure on the backend (a test's or driver's access to it).
    pub fn with_backend<R>(&self, f: impl FnOnce(&mut dyn Backend) -> R) -> R {
        f(&mut **self.0.backend.borrow_mut())
    }

    fn problem(&self, kind: ProblemKind, message: String) {
        let p = Problem { kind, message };
        if self.0.strict.get() {
            panic!("Web Audio: {p}");
        }
        self.0.problems.borrow_mut().push(p);
    }

    fn flush_released(&self) {
        let released = std::mem::take(&mut *self.0.released.borrow_mut());
        if released.is_empty() {
            return;
        }
        let mut b = self.0.backend.borrow_mut();
        let mut g = self.0.graph.borrow_mut();
        for r in released {
            match r {
                Release::Node(n) => {
                    g.nodes.remove(&n);
                    b.release_node(n);
                }
                Release::Buffer(id) => {
                    g.buffers.remove(&id);
                    b.release_buffer(id);
                }
                Release::Wave(id) => {
                    g.waves.remove(&id);
                    b.release_wave(id);
                }
            }
        }
    }

    /// Record, validate and carry out one call.
    fn call(&self, op: Op) -> Result<(), AudioError> {
        self.call_using(op, None)
    }

    /// The same, for a call that gives a buffer to a node: the buffer's
    /// contents are recorded after the call (the fake's order) but reach the
    /// backend before it, so a node never takes a buffer still empty (a
    /// convolver computes its response when given one).
    fn call_using(&self, op: Op, uses: Option<BufferId>) -> Result<(), AudioError> {
        self.flush_released();
        self.0.backend.borrow_mut().record(&op);
        let verdict = self.0.graph.borrow_mut().admit(&op, self.0.sample_rate);
        match verdict {
            Verdict::Ok => {
                if let Some(b) = uses {
                    self.buffer_used(b);
                }
                self.0.backend.borrow_mut().apply(&op);
                Ok(())
            }
            Verdict::Warn(m) => {
                if let Some(b) = uses {
                    self.buffer_used(b);
                }
                self.0.backend.borrow_mut().apply(&op);
                self.problem(ProblemKind::Warning, m);
                Ok(())
            }
            Verdict::Ignore(m) => {
                self.problem(ProblemKind::Ignored, m);
                Ok(())
            }
            Verdict::Throw(e) => {
                self.0.backend.borrow_mut().record_throw(&e);
                self.problem(ProblemKind::Throw(e.name), e.message.clone());
                Err(e)
            }
        }
    }

    fn new_node(&self, kind: NodeKind, arg: Option<f64>) -> Node {
        let id = {
            let mut g = self.0.graph.borrow_mut();
            let id = g.next_node;
            g.next_node += 1;
            id
        };
        // An invalid argument throws in a browser; the node is still made
        // here (inert to the backend) so the caller gets a handle.
        let _ = self.call(Op::New {
            node: id,
            kind,
            arg,
        });
        Node(Rc::new(NodeRef {
            ctx: self.clone(),
            id,
            kind,
        }))
    }

    fn param(&self, node: &Node, name: ParamName) -> AudioParam {
        AudioParam {
            node: node.clone(),
            id: ParamId {
                node: node.id(),
                name,
            },
        }
    }

    pub fn create_gain(&self) -> GainNode {
        let node = self.new_node(NodeKind::Gain, None);
        GainNode {
            gain: self.param(&node, ParamName::Gain),
            node,
        }
    }

    pub fn create_biquad_filter(&self) -> BiquadFilterNode {
        let node = self.new_node(NodeKind::BiquadFilter, None);
        BiquadFilterNode {
            frequency: self.param(&node, ParamName::Frequency),
            detune: self.param(&node, ParamName::Detune),
            q: self.param(&node, ParamName::Q),
            gain: self.param(&node, ParamName::Gain),
            node,
        }
    }

    pub fn create_oscillator(&self) -> OscillatorNode {
        let node = self.new_node(NodeKind::Oscillator, None);
        OscillatorNode {
            frequency: self.param(&node, ParamName::Frequency),
            detune: self.param(&node, ParamName::Detune),
            node,
        }
    }

    pub fn create_buffer_source(&self) -> AudioBufferSourceNode {
        let node = self.new_node(NodeKind::BufferSource, None);
        AudioBufferSourceNode {
            playback_rate: self.param(&node, ParamName::PlaybackRate),
            detune: self.param(&node, ParamName::Detune),
            buffer: RefCell::new(None),
            node,
        }
    }

    pub fn create_wave_shaper(&self) -> WaveShaperNode {
        WaveShaperNode {
            node: self.new_node(NodeKind::WaveShaper, None),
        }
    }

    pub fn create_dynamics_compressor(&self) -> DynamicsCompressorNode {
        let node = self.new_node(NodeKind::DynamicsCompressor, None);
        DynamicsCompressorNode {
            threshold: self.param(&node, ParamName::Threshold),
            knee: self.param(&node, ParamName::Knee),
            ratio: self.param(&node, ParamName::Ratio),
            attack: self.param(&node, ParamName::Attack),
            release: self.param(&node, ParamName::Release),
            node,
        }
    }

    pub fn create_convolver(&self) -> ConvolverNode {
        ConvolverNode {
            node: self.new_node(NodeKind::Convolver, None),
            buffer: RefCell::new(None),
        }
    }

    /// `createDelay(maxDelayTime = 1)`.
    pub fn create_delay(&self, max_delay_time: f64) -> DelayNode {
        let node = self.new_node(NodeKind::Delay, Some(max_delay_time));
        DelayNode {
            delay_time: self.param(&node, ParamName::DelayTime),
            node,
        }
    }

    /// `createChannelMerger(numberOfInputs = 6)`.
    pub fn create_channel_merger(&self, inputs: u32) -> Node {
        self.new_node(NodeKind::ChannelMerger, Some(inputs as f64))
    }

    pub fn create_stereo_panner(&self) -> StereoPannerNode {
        let node = self.new_node(NodeKind::StereoPanner, None);
        StereoPannerNode {
            pan: self.param(&node, ParamName::Pan),
            node,
        }
    }

    /// Starts loading what an exhaust node needs (the web backend's worklet
    /// module and the model's wasm); settles `true` when
    /// [`AudioContext::create_exhaust`] may be called, `false` if it never
    /// can (no AudioWorklet: an old browser or an insecure context).
    /// Settles at the next [`AudioContext::settle`] on a virtual clock, by
    /// itself in a browser. Not a Web Audio call: not in the call log.
    pub fn prepare_exhaust(&self) -> Pending<bool> {
        self.flush_released();
        let p = Pending::new();
        self.0.backend.borrow_mut().prepare_exhaust(p.clone());
        p
    }

    /// An exhaust node for `preset`, a key in [`mp_exhaust::ORDER`]. An
    /// unknown key makes the first preset, with a warning (a problem in
    /// strict mode). On the web, only once [`AudioContext::prepare_exhaust`]
    /// has settled `true` (before that the browser throws and the node is
    /// inert, as the backend's failures record).
    pub fn create_exhaust(&self, preset: &str) -> ExhaustNode {
        let index = exhaust_preset_index(preset).unwrap_or_else(|| {
            self.problem(
                ProblemKind::Warning,
                format!(
                    "createExhaust({preset:?}): not a preset, {:?} instead",
                    mp_exhaust::ORDER[0]
                ),
            );
            0
        });
        let node = self.new_node(NodeKind::Exhaust, Some(index as f64));
        ExhaustNode {
            rpm: self.param(&node, ParamName::Rpm),
            throttle: self.param(&node, ParamName::Throttle),
            boost: self.param(&node, ParamName::Boost),
            speed: self.param(&node, ParamName::Speed),
            running: self.param(&node, ParamName::Running),
            preset: self.param(&node, ParamName::Preset),
            node,
        }
    }

    /// Starts loading what a radio node needs (the web backend's worklet
    /// module and `mp_music`'s wasm); settles `true` when
    /// [`AudioContext::create_radio`] may be called, `false` if it never can
    /// (no AudioWorklet). As [`AudioContext::prepare_exhaust`]: settles at
    /// the next [`AudioContext::settle`] on a virtual clock, by itself in a
    /// browser; not a Web Audio call, so not in the call log.
    pub fn prepare_radio(&self) -> Pending<bool> {
        self.flush_released();
        let p = Pending::new();
        self.0.backend.borrow_mut().prepare_radio(p.clone());
        p
    }

    /// The radio node (DECISIONS D1151): off until its `station` param
    /// names a station. On the web, only once [`AudioContext::prepare_radio`]
    /// has settled `true`.
    pub fn create_radio(&self) -> RadioNode {
        let node = self.new_node(NodeKind::Radio, None);
        RadioNode {
            station: self.param(&node, ParamName::Station),
            wall_day: self.param(&node, ParamName::WallDay),
            wall_sec: self.param(&node, ParamName::WallSec),
            tune: self.param(&node, ParamName::Tune),
            energy: self.param(&node, ParamName::Energy),
            node,
        }
    }

    pub fn create_analyser(&self) -> AnalyserNode {
        AnalyserNode {
            node: self.new_node(NodeKind::Analyser, None),
            fft_size: Cell::new(2048),
        }
    }

    /// `createPeriodicWave(real, imag, { disableNormalization })`. The
    /// options are passed on as given (`None`: `{}`).
    pub fn create_periodic_wave(
        &self,
        real: &[f32],
        imag: &[f32],
        disable_normalization: Option<bool>,
    ) -> Result<PeriodicWave, AudioError> {
        let id = {
            let mut g = self.0.graph.borrow_mut();
            let id = g.next_wave;
            g.next_wave += 1;
            id
        };
        self.call(Op::Wave {
            wave: id,
            real,
            imag,
            disable_normalization,
        })?;
        self.0.graph.borrow_mut().waves.insert(id, real.len());
        Ok(PeriodicWave(Rc::new(WaveRef {
            ctx: self.clone(),
            id,
        })))
    }

    /// `createBuffer(channels, length, sampleRate)`; zero-filled. An
    /// invalid shape throws in a browser: it is recorded, and the handle
    /// returned is inert.
    pub fn create_buffer(&self, channels: u32, length: u32, sample_rate: f64) -> AudioBuffer {
        let id = {
            let mut g = self.0.graph.borrow_mut();
            let id = g.next_buffer;
            g.next_buffer += 1;
            id
        };
        if self
            .call(Op::Buffer {
                buffer: id,
                channels,
                length,
                sample_rate,
            })
            .is_ok()
        {
            self.0.graph.borrow_mut().buffers.insert(
                id,
                BufferInfo {
                    channels,
                    length,
                    sample_rate,
                    data: vec![vec![0.0; length as usize]; channels as usize],
                    used: false,
                    decoded: false,
                },
            );
        }
        AudioBuffer(Rc::new(BufferRef {
            ctx: self.clone(),
            id,
            channels,
            length,
            sample_rate,
        }))
    }

    /// `decodeAudioData(bytes)`. Settles when the backend has decoded it
    /// (the null backend: at the next [`AudioContext::settle`]).
    pub fn decode_audio_data(&self, bytes: &[u8]) -> Pending<AudioBuffer> {
        self.flush_released();
        let id = {
            let mut g = self.0.graph.borrow_mut();
            let id = g.next_buffer;
            g.next_buffer += 1;
            id
        };
        let out = Pending::new();
        let inner: Pending<Decoded> = Pending::new();
        let weak: Weak<Inner> = Rc::downgrade(&self.0);
        let o = out.clone();
        inner.then(move |r| {
            let Some(rc) = weak.upgrade() else {
                return;
            };
            let ctx = AudioContext(rc);
            match r {
                Ok(d) => {
                    ctx.0.graph.borrow_mut().buffers.insert(
                        id,
                        BufferInfo {
                            channels: d.channels,
                            length: d.length,
                            sample_rate: d.sample_rate,
                            data: Vec::new(),
                            used: true,
                            decoded: true,
                        },
                    );
                    o.resolve(Ok(AudioBuffer(Rc::new(BufferRef {
                        ctx: ctx.clone(),
                        id,
                        channels: d.channels,
                        length: d.length,
                        sample_rate: d.sample_rate,
                    }))));
                }
                Err(e) => o.resolve(Err(e.clone())),
            }
        });
        self.0.backend.borrow_mut().decode(id, bytes, inner);
        out
    }

    fn state_change(&self, op: Op) -> Pending<()> {
        self.flush_released();
        let p = Pending::new();
        let mut b = self.0.backend.borrow_mut();
        b.record(&op);
        match op {
            Op::Resume => b.resume(p.clone()),
            Op::Suspend => b.suspend(p.clone()),
            _ => b.close(p.clone()),
        }
        p
    }

    /// `resume()`: the state changes when the promise settles.
    pub fn resume(&self) -> Pending<()> {
        self.state_change(Op::Resume)
    }

    pub fn suspend(&self) -> Pending<()> {
        self.state_change(Op::Suspend)
    }

    pub fn close(&self) -> Pending<()> {
        self.state_change(Op::Close)
    }

    /// Let every pending promise settle (backends on a virtual clock; a
    /// browser does this itself between tasks).
    pub fn settle(&self) {
        loop {
            let tasks = self.0.backend.borrow_mut().settle();
            if tasks.is_empty() {
                break;
            }
            for t in tasks {
                t();
            }
        }
    }

    /// An offline context's render (`startRendering()`), channel by channel.
    pub fn start_rendering(&self) -> Option<Vec<Vec<f32>>> {
        self.flush_released();
        self.0.backend.borrow_mut().render()
    }

    /// An offline render suspended every `frame` samples (a multiple of the
    /// 128-sample render quantum) to run `control(k)` at frame k, k >= 1:
    /// `oc.suspend(k * frame / sampleRate).then(() => { control(k);
    /// oc.resume(); })` for every k, then `startRendering()`. `None` for a
    /// backend that cannot (a live context, the null backend).
    pub fn start_rendering_steered(
        &self,
        frame: usize,
        control: Box<dyn FnMut(usize)>,
    ) -> Option<Vec<Vec<f32>>> {
        self.flush_released();
        let r = self.0.backend.borrow_mut().take_offline()?;
        Some(r.render(frame, control))
    }

    /// A buffer is given to a node: its contents go to the backend once.
    fn buffer_used(&self, id: BufferId) {
        let data = {
            let mut g = self.0.graph.borrow_mut();
            let Some(b) = g.buffers.get_mut(&id) else {
                return;
            };
            if b.used || b.decoded {
                return;
            }
            b.used = true;
            std::mem::take(&mut b.data)
        };
        {
            let op = Op::Data {
                buffer: id,
                channels: &data,
            };
            let mut b = self.0.backend.borrow_mut();
            b.record(&op);
            b.apply(&op);
        }
        if let Some(b) = self.0.graph.borrow_mut().buffers.get_mut(&id) {
            b.data = data;
        }
    }
}

// ── Nodes ──────────────────────────────────────────────────────────

struct NodeRef {
    ctx: AudioContext,
    id: NodeId,
    kind: NodeKind,
}

impl Drop for NodeRef {
    fn drop(&mut self) {
        if let Ok(mut r) = self.ctx.0.released.try_borrow_mut() {
            r.push(Release::Node(self.id));
        }
    }
}

/// An `AudioNode` handle: connect and disconnect.
#[derive(Clone)]
pub struct Node(Rc<NodeRef>);

impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "n{} ({})", self.0.id, self.0.kind.as_str())
    }
}

/// Something a node can connect to.
pub trait Connectable {
    fn dest(&self) -> (Dest, &AudioContext);
}

impl Connectable for Node {
    fn dest(&self) -> (Dest, &AudioContext) {
        (Dest::Node(self.0.id), &self.0.ctx)
    }
}

impl Connectable for AudioParam {
    fn dest(&self) -> (Dest, &AudioContext) {
        (Dest::Param(self.id), &self.node.0.ctx)
    }
}

impl<T: std::ops::Deref<Target = Node>> Connectable for T {
    fn dest(&self) -> (Dest, &AudioContext) {
        (**self).dest()
    }
}

impl Node {
    pub fn id(&self) -> NodeId {
        self.0.id
    }

    pub fn kind(&self) -> NodeKind {
        self.0.kind
    }

    pub fn context(&self) -> &AudioContext {
        &self.0.ctx
    }

    fn call(&self, op: Op) -> Result<(), AudioError> {
        self.0.ctx.call(op)
    }

    /// `connect(dest)`. Connecting to another context's node throws in a
    /// browser (recorded; returns the error).
    pub fn connect(&self, dest: &impl Connectable) -> Result<(), AudioError> {
        self.connect_with(dest, None, None)
    }

    /// `connect(dest, output, input)`.
    pub fn connect_with(
        &self,
        dest: &impl Connectable,
        output: Option<u32>,
        input: Option<u32>,
    ) -> Result<(), AudioError> {
        let (to, ctx) = dest.dest();
        if !ctx.same(&self.0.ctx) {
            self.0.ctx.0.backend.borrow_mut().record(&Op::Connect {
                from: self.0.id,
                to,
                output,
                input,
            });
            let e = AudioError::new(
                ErrorName::InvalidAccessError,
                format!("n{}.connect: a node of another context", self.0.id),
            );
            self.0.ctx.0.backend.borrow_mut().record_throw(&e);
            self.0
                .ctx
                .problem(ProblemKind::Throw(e.name), e.message.clone());
            return Err(e);
        }
        self.call(Op::Connect {
            from: self.0.id,
            to,
            output,
            input,
        })
    }

    /// `disconnect()`: every outgoing link.
    pub fn disconnect(&self) {
        let _ = self.call(Op::Disconnect {
            from: self.0.id,
            to: None,
        });
    }

    /// `disconnect(dest)`: throws in a browser if there is no such link.
    pub fn disconnect_from(&self, dest: &impl Connectable) -> Result<(), AudioError> {
        let (to, _) = dest.dest();
        self.call(Op::Disconnect {
            from: self.0.id,
            to: Some(to),
        })
    }
}

macro_rules! node_deref {
    ($($t:ty),*) => {$(
        impl std::ops::Deref for $t {
            type Target = Node;
            fn deref(&self) -> &Node {
                &self.node
            }
        }
    )*};
}

/// An `AudioParam` handle.
#[derive(Clone)]
pub struct AudioParam {
    node: Node,
    id: ParamId,
}

impl fmt::Debug for AudioParam {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&param_name(self.id))
    }
}

impl AudioParam {
    pub fn id(&self) -> ParamId {
        self.id
    }

    fn call(&self, op: Op) -> &Self {
        let _ = self.node.0.ctx.call(op);
        self
    }

    /// `param.value`: the value now (the null backend: the automation
    /// timeline at the current time, DECISIONS D42).
    pub fn value(&self) -> f64 {
        let ctx = &self.node.0.ctx;
        ctx.flush_released();
        ctx.0.backend.borrow_mut().param_value(self.id)
    }

    /// `param.value = v`.
    pub fn set_value(&self, v: f64) {
        self.call(Op::Value { param: self.id, v });
    }

    pub fn set_value_at_time(&self, v: f64, t: f64) -> &Self {
        self.call(Op::SetValue {
            param: self.id,
            v,
            t,
        })
    }

    pub fn linear_ramp_to_value_at_time(&self, v: f64, t: f64) -> &Self {
        self.call(Op::LinRamp {
            param: self.id,
            v,
            t,
        })
    }

    pub fn exponential_ramp_to_value_at_time(&self, v: f64, t: f64) -> &Self {
        self.call(Op::ExpRamp {
            param: self.id,
            v,
            t,
        })
    }

    pub fn set_target_at_time(&self, v: f64, t: f64, tc: f64) -> &Self {
        self.call(Op::SetTarget {
            param: self.id,
            v,
            t,
            tc,
        })
    }

    pub fn cancel_scheduled_values(&self, t: f64) -> &Self {
        self.call(Op::Cancel { param: self.id, t })
    }
}

pub struct GainNode {
    node: Node,
    pub gain: AudioParam,
}

pub struct BiquadFilterNode {
    node: Node,
    pub frequency: AudioParam,
    pub detune: AudioParam,
    pub q: AudioParam,
    pub gain: AudioParam,
}

impl BiquadFilterNode {
    pub fn set_type(&self, t: BiquadFilterType) {
        self.set_type_js(t.as_str());
    }

    /// `filter.type = s` with a JS string: an invalid one is ignored, as
    /// browsers do (a problem in strict mode).
    pub fn set_type_js(&self, s: &str) {
        let _ = self.call(Op::Attr {
            node: self.id(),
            attr: Attr::Type(s),
        });
    }
}

pub struct OscillatorNode {
    node: Node,
    pub frequency: AudioParam,
    pub detune: AudioParam,
}

impl OscillatorNode {
    pub fn set_type(&self, t: OscillatorType) {
        self.set_type_js(t.as_str());
    }

    /// `osc.type = s` with a JS string: an invalid one is ignored, as
    /// browsers do; `'custom'` throws.
    pub fn set_type_js(&self, s: &str) {
        let _ = self.call(Op::Attr {
            node: self.id(),
            attr: Attr::Type(s),
        });
    }

    pub fn set_periodic_wave(&self, w: &PeriodicWave) {
        let _ = self.call(Op::SetPeriodicWave {
            node: self.id(),
            wave: w.0.id,
        });
    }

    /// `start()`.
    pub fn start(&self) -> Result<(), AudioError> {
        self.call(Op::Start {
            node: self.id(),
            args: &[],
        })
    }

    /// `start(when)`.
    pub fn start_at(&self, when: f64) -> Result<(), AudioError> {
        self.call(Op::Start {
            node: self.id(),
            args: &[when],
        })
    }

    pub fn stop(&self) -> Result<(), AudioError> {
        self.call(Op::Stop {
            node: self.id(),
            args: &[],
        })
    }

    pub fn stop_at(&self, when: f64) -> Result<(), AudioError> {
        self.call(Op::Stop {
            node: self.id(),
            args: &[when],
        })
    }
}

pub struct AudioBufferSourceNode {
    node: Node,
    pub playback_rate: AudioParam,
    pub detune: AudioParam,
    buffer: RefCell<Option<AudioBuffer>>,
}

impl AudioBufferSourceNode {
    /// `src.buffer = b`. Setting a second buffer throws in browsers.
    pub fn set_buffer(&self, b: Option<&AudioBuffer>) -> Result<(), AudioError> {
        let id = b.map(|b| b.0.id);
        self.context().call_using(
            Op::Attr {
                node: self.id(),
                attr: Attr::Buffer(id),
            },
            id,
        )?;
        *self.buffer.borrow_mut() = b.cloned();
        Ok(())
    }

    pub fn buffer(&self) -> Option<AudioBuffer> {
        self.buffer.borrow().clone()
    }

    pub fn set_loop(&self, on: bool) {
        let _ = self.call(Op::Attr {
            node: self.id(),
            attr: Attr::Loop(on),
        });
    }

    pub fn set_loop_start(&self, t: f64) {
        let _ = self.call(Op::Attr {
            node: self.id(),
            attr: Attr::LoopStart(t),
        });
    }

    pub fn set_loop_end(&self, t: f64) {
        let _ = self.call(Op::Attr {
            node: self.id(),
            attr: Attr::LoopEnd(t),
        });
    }

    /// `start(...args)` with up to three arguments (`when`, `offset`,
    /// `duration`), as called: arguments left out are not passed on.
    pub fn start_args(&self, args: &[f64]) -> Result<(), AudioError> {
        assert!(args.len() <= 3, "start takes at most three arguments");
        // (The buffer went to the backend when it was set.)
        self.call(Op::Start {
            node: self.id(),
            args,
        })
    }

    pub fn start(&self) -> Result<(), AudioError> {
        self.start_args(&[])
    }

    pub fn start_at(&self, when: f64) -> Result<(), AudioError> {
        self.start_args(&[when])
    }

    /// `start(when, offset[, duration])`.
    pub fn start_with(
        &self,
        when: f64,
        offset: f64,
        duration: Option<f64>,
    ) -> Result<(), AudioError> {
        match duration {
            Some(d) => self.start_args(&[when, offset, d]),
            None => self.start_args(&[when, offset]),
        }
    }

    pub fn stop(&self) -> Result<(), AudioError> {
        self.call(Op::Stop {
            node: self.id(),
            args: &[],
        })
    }

    pub fn stop_at(&self, when: f64) -> Result<(), AudioError> {
        self.call(Op::Stop {
            node: self.id(),
            args: &[when],
        })
    }
}

pub struct WaveShaperNode {
    node: Node,
}

impl WaveShaperNode {
    /// `shaper.curve = c` (a `Float32Array`).
    pub fn set_curve(&self, c: Option<&[f32]>) {
        let _ = self.call(Op::Attr {
            node: self.id(),
            attr: Attr::Curve(c),
        });
    }

    pub fn set_oversample(&self, o: OverSampleType) {
        self.set_oversample_js(o.as_str());
    }

    pub fn set_oversample_js(&self, s: &str) {
        let _ = self.call(Op::Attr {
            node: self.id(),
            attr: Attr::Oversample(s),
        });
    }
}

pub struct DynamicsCompressorNode {
    node: Node,
    pub threshold: AudioParam,
    pub knee: AudioParam,
    pub ratio: AudioParam,
    pub attack: AudioParam,
    pub release: AudioParam,
}

pub struct ConvolverNode {
    node: Node,
    buffer: RefCell<Option<AudioBuffer>>,
}

impl ConvolverNode {
    pub fn set_buffer(&self, b: Option<&AudioBuffer>) -> Result<(), AudioError> {
        let id = b.map(|b| b.0.id);
        self.context().call_using(
            Op::Attr {
                node: self.id(),
                attr: Attr::Buffer(id),
            },
            id,
        )?;
        *self.buffer.borrow_mut() = b.cloned();
        Ok(())
    }

    pub fn buffer(&self) -> Option<AudioBuffer> {
        self.buffer.borrow().clone()
    }

    pub fn set_normalize(&self, on: bool) {
        let _ = self.call(Op::Attr {
            node: self.id(),
            attr: Attr::Normalize(on),
        });
    }
}

pub struct DelayNode {
    node: Node,
    pub delay_time: AudioParam,
}

pub struct StereoPannerNode {
    node: Node,
    pub pan: AudioParam,
}

pub struct AnalyserNode {
    node: Node,
    fft_size: Cell<u32>,
}

impl AnalyserNode {
    pub fn set_fft_size(&self, n: u32) {
        if self
            .call(Op::Attr {
                node: self.id(),
                attr: Attr::FftSize(n),
            })
            .is_ok()
        {
            self.fft_size.set(n);
        }
    }

    pub fn fft_size(&self) -> u32 {
        self.fft_size.get()
    }

    pub fn frequency_bin_count(&self) -> u32 {
        self.fft_size.get() / 2
    }

    pub fn set_smoothing_time_constant(&self, s: f64) {
        let _ = self.call(Op::Attr {
            node: self.id(),
            attr: Attr::SmoothingTimeConstant(s),
        });
    }

    pub fn get_float_time_domain_data(&self, out: &mut [f32]) {
        let ctx = self.context();
        ctx.flush_released();
        ctx.0
            .backend
            .borrow_mut()
            .analyser_time_domain(self.id(), out);
    }

    pub fn get_byte_frequency_data(&self, out: &mut [u8]) {
        let ctx = self.context();
        ctx.flush_released();
        ctx.0
            .backend
            .borrow_mut()
            .analyser_byte_frequency(self.id(), out);
    }
}

/// The exhaust model's node ([`AudioContext::create_exhaust`]). Its params
/// are read once per 128-frame block (k-rate): drive them with
/// `set_value_at_time`. `running` below 0.5 fades the engine out (about
/// 40 ms), at or above fades it in; `preset` (an index in
/// [`mp_exhaust::ORDER`], rounded) rebuilds the engine for another car when
/// it changes, so one node can serve a whole session.
pub struct ExhaustNode {
    node: Node,
    pub rpm: AudioParam,
    pub throttle: AudioParam,
    pub boost: AudioParam,
    pub speed: AudioParam,
    pub running: AudioParam,
    pub preset: AudioParam,
}

/// The radio's node ([`AudioContext::create_radio`]): `mp_music`'s station
/// player, driven by k-rate params read once per block (drive them with
/// `set_value_at_time`). `station` is an index in
/// `mp_music::radio::STATIONS` (-1: off); `wall_day` and `wall_sec` are
/// the wall time at the moment of tuning (days since the Unix epoch and
/// seconds into the day: an f32 cannot hold Unix seconds); a change of
/// `tune` (a serial) re-syncs the player to that time, even on the same
/// station; `energy` (0..1) is the game's intensity. From then on the
/// processor keeps the station's schedule on its own clock.
pub struct RadioNode {
    node: Node,
    pub station: AudioParam,
    pub wall_day: AudioParam,
    pub wall_sec: AudioParam,
    pub tune: AudioParam,
    pub energy: AudioParam,
}

node_deref!(
    GainNode,
    BiquadFilterNode,
    OscillatorNode,
    AudioBufferSourceNode,
    WaveShaperNode,
    DynamicsCompressorNode,
    ConvolverNode,
    DelayNode,
    StereoPannerNode,
    AnalyserNode,
    ExhaustNode,
    RadioNode
);

// ── Buffers and waves ──────────────────────────────────────────────

struct BufferRef {
    ctx: AudioContext,
    id: BufferId,
    channels: u32,
    length: u32,
    sample_rate: f64,
}

impl Drop for BufferRef {
    fn drop(&mut self) {
        if let Ok(mut r) = self.ctx.0.released.try_borrow_mut() {
            r.push(Release::Buffer(self.id));
        }
    }
}

/// An `AudioBuffer` handle. Its contents live in the facade until the
/// buffer is first given to a node, when they go to the backend; changing
/// them after that is a problem (the reference capture forbids it, and a
/// browser may or may not hear the change).
#[derive(Clone)]
pub struct AudioBuffer(Rc<BufferRef>);

impl fmt::Debug for AudioBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "b{} ({} × {} at {} Hz)",
            self.0.id, self.0.channels, self.0.length, self.0.sample_rate
        )
    }
}

impl AudioBuffer {
    pub fn id(&self) -> BufferId {
        self.0.id
    }

    pub fn number_of_channels(&self) -> u32 {
        self.0.channels
    }

    pub fn length(&self) -> u32 {
        self.0.length
    }

    pub fn sample_rate(&self) -> f64 {
        self.0.sample_rate
    }

    /// `duration`: `length / sampleRate`.
    pub fn duration(&self) -> f64 {
        self.0.length as f64 / self.0.sample_rate
    }

    /// Change channel `c`'s samples in place (`getChannelData(c)` written
    /// to).
    pub fn with_channel_data_mut<R>(&self, c: u32, f: impl FnOnce(&mut [f32]) -> R) -> R {
        let ctx = &self.0.ctx;
        let (r, used) = {
            let mut g = ctx.0.graph.borrow_mut();
            let b = g
                .buffers
                .get_mut(&self.0.id)
                .expect("a buffer the context made");
            assert!(c < b.channels, "getChannelData({c}): IndexSizeError");
            let used = b.used;
            if b.data.is_empty() {
                b.data = vec![vec![0.0; b.length as usize]; b.channels as usize];
            }
            (f(&mut b.data[c as usize]), used)
        };
        if used {
            ctx.problem(
                ProblemKind::Warning,
                format!("buffer b{} changed after use", self.0.id),
            );
        }
        r
    }

    /// `copyToChannel(src, c)`.
    pub fn copy_to_channel(&self, src: &[f32], c: u32) {
        self.with_channel_data_mut(c, |d| {
            let n = src.len().min(d.len());
            d[..n].copy_from_slice(&src[..n]);
        });
    }

    /// A copy of channel `c` (zeros for a decoded buffer, whose samples
    /// stay with the backend).
    pub fn get_channel_data(&self, c: u32) -> Vec<f32> {
        let g = self.0.ctx.0.graph.borrow();
        match g.buffers.get(&self.0.id) {
            Some(b) if !b.data.is_empty() => b.data[c as usize].clone(),
            _ => vec![0.0; self.0.length as usize],
        }
    }
}

struct WaveRef {
    ctx: AudioContext,
    id: WaveId,
}

impl Drop for WaveRef {
    fn drop(&mut self) {
        if let Ok(mut r) = self.ctx.0.released.try_borrow_mut() {
            r.push(Release::Wave(self.id));
        }
    }
}

/// A `PeriodicWave` handle.
#[derive(Clone)]
pub struct PeriodicWave(Rc<WaveRef>);

impl fmt::Debug for PeriodicWave {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "w{}", self.0.id)
    }
}

impl PeriodicWave {
    pub fn id(&self) -> WaveId {
        self.0.id
    }
}

impl NodeKind {
    /// Whether nodes of this kind are scheduled sources (`start`/`stop`).
    pub fn is_scheduled(self) -> bool {
        self.scheduled()
    }
}
