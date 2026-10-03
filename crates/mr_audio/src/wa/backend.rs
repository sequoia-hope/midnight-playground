//! What a backend implements (SPEC 7.1): the browser's Web Audio
//! (`web`), the `web-audio-api` crate (`native`), or a recorder (`null`).
//!
//! The facade ([`super::AudioContext`]) hands every call to the backend as an
//! [`Op`], twice: [`Backend::record`] before validation (the null backend's
//! call log, which logs calls a browser would reject too, as the JS fake
//! does) and [`Backend::apply`] once the call is valid. A backend never sees
//! an invalid call, so the native one (which panics on many) and the web one
//! (which throws) need no checks of their own.

use super::{AudioError, ContextState, Decoded, Dest, NodeKind, ParamId, Pending};

pub type NodeId = u32;
pub type BufferId = u32;
pub type WaveId = u32;

/// A node attribute assignment (`node.type = ...`, `src.loop = ...`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Attr<'a> {
    /// `type` of an Oscillator or BiquadFilter, as the JS string. Always a
    /// valid value of the node's enum by the time a backend applies it.
    Type(&'a str),
    Loop(bool),
    LoopStart(f64),
    LoopEnd(f64),
    /// `'none'`, `'2x'` or `'4x'` once applied.
    Oversample(&'a str),
    Normalize(bool),
    /// A BufferSource's or Convolver's buffer.
    Buffer(Option<BufferId>),
    /// A WaveShaper's curve.
    Curve(Option<&'a [f32]>),
    FftSize(u32),
    SmoothingTimeConstant(f64),
}

impl Attr<'_> {
    /// The JS attribute name.
    pub fn name(&self) -> &'static str {
        match self {
            Attr::Type(_) => "type",
            Attr::Loop(_) => "loop",
            Attr::LoopStart(_) => "loopStart",
            Attr::LoopEnd(_) => "loopEnd",
            Attr::Oversample(_) => "oversample",
            Attr::Normalize(_) => "normalize",
            Attr::Buffer(_) => "buffer",
            Attr::Curve(_) => "curve",
            Attr::FftSize(_) => "fftSize",
            Attr::SmoothingTimeConstant(_) => "smoothingTimeConstant",
        }
    }
}

/// One call on the Web Audio API, as the facade passes it on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op<'a> {
    /// `new AudioContext({ latencyHint })` (or an offline context).
    Context {
        sample_rate: f64,
        latency_hint: Option<&'a str>,
    },
    /// `create<Kind>(arg)`; `arg` is the Delay's maximum delay or the
    /// ChannelMerger's input count. The destination is node 0.
    New {
        node: NodeId,
        kind: NodeKind,
        arg: Option<f64>,
    },
    Attr {
        node: NodeId,
        attr: Attr<'a>,
    },
    /// `param.value = v`.
    Value {
        param: ParamId,
        v: f64,
    },
    SetValue {
        param: ParamId,
        v: f64,
        t: f64,
    },
    LinRamp {
        param: ParamId,
        v: f64,
        t: f64,
    },
    ExpRamp {
        param: ParamId,
        v: f64,
        t: f64,
    },
    SetTarget {
        param: ParamId,
        v: f64,
        t: f64,
        tc: f64,
    },
    Cancel {
        param: ParamId,
        t: f64,
    },
    Connect {
        from: NodeId,
        to: Dest,
        output: Option<u32>,
        input: Option<u32>,
    },
    /// `disconnect()` (`to: None`) or `disconnect(to)`.
    Disconnect {
        from: NodeId,
        to: Option<Dest>,
    },
    /// `start(...args)` / `stop(...args)` with the arguments as called.
    Start {
        node: NodeId,
        args: &'a [f64],
    },
    Stop {
        node: NodeId,
        args: &'a [f64],
    },
    SetPeriodicWave {
        node: NodeId,
        wave: WaveId,
    },
    /// `createPeriodicWave(real, imag, { disableNormalization })`.
    Wave {
        wave: WaveId,
        real: &'a [f32],
        imag: &'a [f32],
        disable_normalization: Option<bool>,
    },
    /// `createBuffer(channels, length, sampleRate)`.
    Buffer {
        buffer: BufferId,
        channels: u32,
        length: u32,
        sample_rate: f64,
    },
    /// A buffer's contents, passed on when it is first given to a node (the
    /// fake's `data` line). Not sent for decoded buffers.
    Data {
        buffer: BufferId,
        channels: &'a [Vec<f32>],
    },
    Resume,
    Suspend,
    Close,
}

/// A Web Audio implementation behind the facade.
pub trait Backend {
    fn sample_rate(&self) -> f64;
    fn current_time(&self) -> f64;
    fn state(&self) -> ContextState;

    /// Every call, before it is validated.
    fn record(&mut self, _op: &Op) {}
    /// The call just recorded threw (a browser would have thrown).
    fn record_throw(&mut self, _err: &AudioError) {}
    /// Carry out a valid call. `Resume`, `Suspend` and `Close` come through
    /// [`Backend::resume`] and friends instead.
    fn apply(&mut self, op: &Op);

    /// `param.value`: what the param holds now.
    fn param_value(&mut self, param: ParamId) -> f64;

    /// Context state changes; `done` settles when the browser's promise does.
    fn resume(&mut self, done: Pending<()>);
    fn suspend(&mut self, done: Pending<()>);
    fn close(&mut self, done: Pending<()>);

    /// `decodeAudioData(bytes)` into buffer `buffer`.
    fn decode(&mut self, buffer: BufferId, bytes: &[u8], done: Pending<Decoded>);

    /// The facade dropped its last handle on a node, buffer or wave.
    fn release_node(&mut self, _node: NodeId) {}
    fn release_buffer(&mut self, _buffer: BufferId) {}
    fn release_wave(&mut self, _wave: WaveId) {}

    /// Work a browser would do in a microtask (settling promises), for
    /// backends that run on a virtual clock: returned to the facade, which
    /// runs it with the backend free.
    fn settle(&mut self) -> Vec<Box<dyn FnOnce()>> {
        Vec::new()
    }

    /// An AnalyserNode's current data.
    fn analyser_time_domain(&mut self, _node: NodeId, out: &mut [f32]) {
        out.fill(0.0);
    }
    fn analyser_byte_frequency(&mut self, _node: NodeId, out: &mut [u8]) {
        out.fill(0);
    }

    /// An offline context: render it whole, channel by channel.
    fn render(&mut self) -> Option<Vec<Vec<f32>>> {
        None
    }

    /// An offline context, handed over to render steered (the backend keeps
    /// taking calls meanwhile, from the control function).
    fn take_offline(&mut self) -> Option<Box<dyn OfflineRender>> {
        None
    }
}

/// An offline render that suspends every `frame` samples to run
/// `control(k)` (frame k), as `OfflineAudioContext.suspend(t)` lets a page
/// steer one (DECISIONS D45). The control's calls on the context go through
/// the facade as usual.
pub trait OfflineRender {
    fn render(self: Box<Self>, frame: usize, control: Box<dyn FnMut(usize)>) -> Vec<Vec<f32>>;
}
