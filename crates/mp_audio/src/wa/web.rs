//! The web backend (SPEC 7.1): the browser's own Web Audio nodes through
//! `web-sys`, so the web build keeps the exact sound and cost of the JS game.
//! Each facade call becomes the same call on the browser's objects; the
//! facade has validated it already, so an exception here means the browser
//! disagrees with the facade's checks: it is kept in [`WebBackend::failures`]
//! and logged to the console.
//!
//! Browser rules (SPEC 7.4) are the caller's: create the context lazily,
//! set `navigator.audioSession.type` before creating it, resume inside
//! gesture handlers, never await a resume.

use super::backend::{Attr, Backend, BufferId, NodeId, Op, WaveId};
use super::{
    AudioError, BiquadFilterType, ContextState, Decoded, Dest, ErrorName, NodeKind, OscillatorType,
    OverSampleType, ParamId, ParamName, Pending,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::{JsCast, JsValue};
use web_sys as ws;

enum WebNode {
    Destination(ws::AudioDestinationNode),
    Gain(ws::GainNode),
    Biquad(ws::BiquadFilterNode),
    Osc(ws::OscillatorNode),
    Src(ws::AudioBufferSourceNode),
    Shaper(ws::WaveShaperNode),
    Comp(ws::DynamicsCompressorNode),
    Conv(ws::ConvolverNode),
    Delay(ws::DelayNode),
    Merger(ws::ChannelMergerNode),
    Pan(ws::StereoPannerNode),
    Analyser(ws::AnalyserNode),
    /// `web/exhaust-worklet.js` running `mp_exhaust.wasm`.
    Exhaust(ws::AudioWorkletNode),
}

impl WebNode {
    fn node(&self) -> &ws::AudioNode {
        match self {
            WebNode::Destination(n) => n,
            WebNode::Gain(n) => n,
            WebNode::Biquad(n) => n,
            WebNode::Osc(n) => n,
            WebNode::Src(n) => n,
            WebNode::Shaper(n) => n,
            WebNode::Comp(n) => n,
            WebNode::Conv(n) => n,
            WebNode::Delay(n) => n,
            WebNode::Merger(n) => n,
            WebNode::Pan(n) => n,
            WebNode::Analyser(n) => n,
            WebNode::Exhaust(n) => n,
        }
    }

    fn param(&self, name: ParamName) -> Option<ws::AudioParam> {
        use ParamName as P;
        Some(match (self, name) {
            (WebNode::Gain(n), P::Gain) => n.gain(),
            (WebNode::Biquad(n), P::Frequency) => n.frequency(),
            (WebNode::Biquad(n), P::Detune) => n.detune(),
            (WebNode::Biquad(n), P::Q) => n.q(),
            (WebNode::Biquad(n), P::Gain) => n.gain(),
            (WebNode::Osc(n), P::Frequency) => n.frequency(),
            (WebNode::Osc(n), P::Detune) => n.detune(),
            (WebNode::Src(n), P::PlaybackRate) => n.playback_rate(),
            (WebNode::Src(n), P::Detune) => n.detune(),
            (WebNode::Comp(n), P::Threshold) => n.threshold(),
            (WebNode::Comp(n), P::Knee) => n.knee(),
            (WebNode::Comp(n), P::Ratio) => n.ratio(),
            (WebNode::Comp(n), P::Attack) => n.attack(),
            (WebNode::Comp(n), P::Release) => n.release(),
            (WebNode::Delay(n), P::DelayTime) => n.delay_time(),
            (WebNode::Pan(n), P::Pan) => n.pan(),
            (WebNode::Exhaust(n), p) => n.parameters().ok()?.get(p.as_str())?,
            _ => return None,
        })
    }
}

enum Ctx {
    Live(ws::AudioContext),
    Offline(ws::OfflineAudioContext),
}

impl Ctx {
    fn base(&self) -> &ws::BaseAudioContext {
        match self {
            Ctx::Live(c) => c,
            Ctx::Offline(c) => c,
        }
    }
}

/// The browser's Web Audio.
pub struct WebBackend {
    ctx: Ctx,
    nodes: HashMap<NodeId, WebNode>,
    buffers: Rc<RefCell<HashMap<BufferId, ws::AudioBuffer>>>,
    waves: HashMap<WaveId, ws::PeriodicWave>,
    failures: Rc<RefCell<Vec<String>>>,
    /// The exhaust model's compiled wasm, once [`Backend::prepare_exhaust`]
    /// has loaded it (with the worklet module).
    exhaust_module: Rc<RefCell<Option<JsValue>>>,
    /// That load, while it runs or once it has settled.
    exhaust_ready: Option<Pending<bool>>,
}

/// Where the exhaust worklet's files are, beside the game's page. Relative:
/// the game is always served under a sub-path.
const EXHAUST_WORKLET_URL: &str = "exhaust-worklet.js";
const EXHAUST_WASM_URL: &str = "mp_exhaust.wasm";
/// The engines' noise seed (the native processor's too).
const EXHAUST_SEED: u32 = 12345;

/// `ctx.audioWorklet.addModule(worklet)`, then the wasm fetched and
/// compiled: the module, or why not.
async fn load_exhaust(ctx: ws::BaseAudioContext) -> Result<JsValue, JsValue> {
    // An insecure context or an old browser has no `audioWorklet`.
    let worklet = js_sys::Reflect::get(&ctx, &JsValue::from_str("audioWorklet"))?;
    if worklet.is_undefined() || worklet.is_null() {
        return Err(JsValue::from_str("no AudioWorklet"));
    }
    let worklet: ws::AudioWorklet = worklet.unchecked_into();
    let added = worklet.add_module(EXHAUST_WORKLET_URL)?;
    let window = ws::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let fetched = window.fetch_with_str(EXHAUST_WASM_URL);
    wasm_bindgen_futures::JsFuture::from(added).await?;
    let resp: ws::Response = wasm_bindgen_futures::JsFuture::from(fetched)
        .await?
        .unchecked_into();
    if !resp.ok() {
        return Err(JsValue::from_str(&format!(
            "{EXHAUST_WASM_URL}: HTTP {}",
            resp.status()
        )));
    }
    let bytes = wasm_bindgen_futures::JsFuture::from(resp.array_buffer()?).await?;
    wasm_bindgen_futures::JsFuture::from(js_sys::WebAssembly::compile(&bytes)).await
}

fn js_err(e: &JsValue) -> String {
    e.dyn_ref::<js_sys::Error>()
        .map(|e| format!("{}: {}", String::from(e.name()), String::from(e.message())))
        .unwrap_or_else(|| format!("{e:?}"))
}

impl WebBackend {
    /// A live context (`new AudioContext({ latencyHint })`).
    pub fn new(latency_hint: Option<&str>) -> Result<WebBackend, JsValue> {
        let opts = ws::AudioContextOptions::new();
        if let Some(h) = latency_hint {
            opts.set_latency_hint(&JsValue::from_str(h));
        }
        let ctx = ws::AudioContext::new_with_context_options(&opts)?;
        Ok(Self::with(Ctx::Live(ctx)))
    }

    /// An offline context (`new OfflineAudioContext(channels, length,
    /// sampleRate)`).
    pub fn offline(channels: u32, length: u32, sample_rate: f32) -> Result<WebBackend, JsValue> {
        let ctx = ws::OfflineAudioContext::new_with_number_of_channels_and_length_and_sample_rate(
            channels,
            length,
            sample_rate,
        )?;
        Ok(Self::with(Ctx::Offline(ctx)))
    }

    fn with(ctx: Ctx) -> WebBackend {
        WebBackend {
            ctx,
            nodes: HashMap::new(),
            buffers: Rc::new(RefCell::new(HashMap::new())),
            waves: HashMap::new(),
            failures: Rc::new(RefCell::new(Vec::new())),
            exhaust_module: Rc::new(RefCell::new(None)),
            exhaust_ready: None,
        }
    }

    /// The browser's own context, for what the facade does not cover.
    pub fn raw(&self) -> &ws::BaseAudioContext {
        self.ctx.base()
    }

    /// Exceptions the browser threw on calls the facade had accepted.
    pub fn failures(&self) -> Vec<String> {
        self.failures.borrow().clone()
    }

    fn fail(&self, what: &str, e: JsValue) {
        let m = format!("{what}: {}", js_err(&e));
        ws_console(&m);
        self.failures.borrow_mut().push(m);
    }

    fn check<T>(&self, what: &str, r: Result<T, JsValue>) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(e) => {
                self.fail(what, e);
                None
            }
        }
    }

    fn param(&self, p: ParamId) -> Option<ws::AudioParam> {
        self.nodes.get(&p.node).and_then(|n| n.param(p.name))
    }

    fn create(&self, kind: NodeKind, arg: Option<f64>) -> Result<WebNode, JsValue> {
        let c = self.ctx.base();
        Ok(match kind {
            NodeKind::Destination => WebNode::Destination(c.destination()),
            NodeKind::Gain => WebNode::Gain(c.create_gain()?),
            NodeKind::BiquadFilter => WebNode::Biquad(c.create_biquad_filter()?),
            NodeKind::Oscillator => WebNode::Osc(c.create_oscillator()?),
            NodeKind::BufferSource => WebNode::Src(c.create_buffer_source()?),
            NodeKind::WaveShaper => WebNode::Shaper(c.create_wave_shaper()?),
            NodeKind::DynamicsCompressor => WebNode::Comp(c.create_dynamics_compressor()?),
            NodeKind::Convolver => WebNode::Conv(c.create_convolver()?),
            NodeKind::Delay => {
                WebNode::Delay(c.create_delay_with_max_delay_time(arg.unwrap_or(1.0))?)
            }
            NodeKind::ChannelMerger => WebNode::Merger(
                c.create_channel_merger_with_number_of_inputs(arg.unwrap_or(6.0) as u32)?,
            ),
            NodeKind::StereoPanner => WebNode::Pan(c.create_stereo_panner()?),
            NodeKind::Analyser => WebNode::Analyser(c.create_analyser()?),
            NodeKind::Exhaust => {
                let module = self
                    .exhaust_module
                    .borrow()
                    .clone()
                    .ok_or_else(|| JsValue::from_str("createExhaust before prepareExhaust"))?;
                let preset = arg.unwrap_or(0.0);
                let po = js_sys::Object::new();
                js_sys::Reflect::set(&po, &"module".into(), &module)?;
                js_sys::Reflect::set(&po, &"preset".into(), &preset.into())?;
                js_sys::Reflect::set(&po, &"seed".into(), &EXHAUST_SEED.into())?;
                let pd = js_sys::Object::new();
                js_sys::Reflect::set(&pd, &"preset".into(), &preset.into())?;
                let opts = ws::AudioWorkletNodeOptions::new();
                opts.set_number_of_inputs(0);
                opts.set_number_of_outputs(1);
                opts.set_output_channel_count(&js_sys::Array::of1(&2.into()));
                opts.set_parameter_data(&pd);
                opts.set_processor_options(Some(&po));
                WebNode::Exhaust(ws::AudioWorkletNode::new_with_options(
                    c,
                    "mp-exhaust",
                    &opts,
                )?)
            }
        })
    }

    fn promise(&self, p: Result<js_sys::Promise, JsValue>, done: Pending<()>) {
        match p {
            Ok(p) => wasm_bindgen_futures::spawn_local(async move {
                let r = wasm_bindgen_futures::JsFuture::from(p).await;
                done.resolve(
                    r.map(|_| ())
                        .map_err(|e| AudioError::new(ErrorName::InvalidStateError, js_err(&e))),
                );
            }),
            Err(e) => done.resolve(Err(AudioError::new(
                ErrorName::InvalidStateError,
                js_err(&e),
            ))),
        }
    }
}

fn ws_console(m: &str) {
    ws::console::warn_1(&JsValue::from_str(m));
}

impl Backend for WebBackend {
    fn sample_rate(&self) -> f64 {
        self.ctx.base().sample_rate() as f64
    }

    fn current_time(&self) -> f64 {
        self.ctx.base().current_time()
    }

    fn state(&self) -> ContextState {
        match self.ctx.base().state() {
            ws::AudioContextState::Running => ContextState::Running,
            ws::AudioContextState::Closed => ContextState::Closed,
            _ => ContextState::Suspended,
        }
    }

    fn apply(&mut self, op: &Op) {
        match *op {
            Op::Context { .. } | Op::Resume | Op::Suspend | Op::Close => {}
            Op::New { node, kind, arg } => {
                if let Some(n) = self.check("create", self.create(kind, arg)) {
                    self.nodes.insert(node, n);
                }
            }
            Op::Attr { node, attr } => {
                let Some(n) = self.nodes.get(&node) else {
                    return;
                };
                match (n, attr) {
                    (WebNode::Osc(o), Attr::Type(s)) => o.set_type(osc_type(s)),
                    (WebNode::Biquad(b), Attr::Type(s)) => b.set_type(filter_type(s)),
                    (WebNode::Src(s), Attr::Loop(on)) => s.set_loop(on),
                    (WebNode::Src(s), Attr::LoopStart(t)) => s.set_loop_start(t),
                    (WebNode::Src(s), Attr::LoopEnd(t)) => s.set_loop_end(t),
                    (WebNode::Src(s), Attr::Buffer(b)) => {
                        let bufs = self.buffers.borrow();
                        s.set_buffer(b.and_then(|b| bufs.get(&b)));
                    }
                    (WebNode::Conv(c), Attr::Buffer(b)) => {
                        let bufs = self.buffers.borrow();
                        c.set_buffer(b.and_then(|b| bufs.get(&b)));
                    }
                    (WebNode::Conv(c), Attr::Normalize(on)) => c.set_normalize(on),
                    (WebNode::Shaper(w), Attr::Curve(c)) => {
                        let mut v = c.map(|c| c.to_vec());
                        w.set_curve_opt_f32_slice(v.as_deref_mut());
                    }
                    (WebNode::Shaper(w), Attr::Oversample(s)) => {
                        w.set_oversample(match OverSampleType::from_js(s) {
                            Some(OverSampleType::X2) => ws::OverSampleType::N2x,
                            Some(OverSampleType::X4) => ws::OverSampleType::N4x,
                            _ => ws::OverSampleType::None,
                        })
                    }
                    (WebNode::Analyser(a), Attr::FftSize(f)) => a.set_fft_size(f),
                    (WebNode::Analyser(a), Attr::SmoothingTimeConstant(s)) => {
                        a.set_smoothing_time_constant(s)
                    }
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
                    let r = p.set_value_at_time(v as f32, t);
                    self.check("setValueAtTime", r);
                }
            }
            Op::LinRamp { param, v, t } => {
                if let Some(p) = self.param(param) {
                    let r = p.linear_ramp_to_value_at_time(v as f32, t);
                    self.check("linearRampToValueAtTime", r);
                }
            }
            Op::ExpRamp { param, v, t } => {
                if let Some(p) = self.param(param) {
                    let r = p.exponential_ramp_to_value_at_time(v as f32, t);
                    self.check("exponentialRampToValueAtTime", r);
                }
            }
            Op::SetTarget { param, v, t, tc } => {
                if let Some(p) = self.param(param) {
                    let r = p.set_target_at_time(v as f32, t, tc);
                    self.check("setTargetAtTime", r);
                }
            }
            Op::Cancel { param, t } => {
                if let Some(p) = self.param(param) {
                    let r = p.cancel_scheduled_values(t);
                    self.check("cancelScheduledValues", r);
                }
            }
            Op::Connect {
                from,
                to,
                output,
                input,
            } => {
                let Some(src) = self.nodes.get(&from).map(|n| n.node().clone()) else {
                    return;
                };
                let (o, i) = (output.unwrap_or(0), input.unwrap_or(0));
                match to {
                    Dest::Node(d) => {
                        if let Some(d) = self.nodes.get(&d) {
                            let r =
                                src.connect_with_audio_node_and_output_and_input(d.node(), o, i);
                            self.check("connect", r);
                        }
                    }
                    Dest::Param(p) => {
                        if let Some(p) = self.param(p) {
                            let r = src.connect_with_audio_param_and_output(&p, o);
                            self.check("connect", r);
                        }
                    }
                }
            }
            Op::Disconnect { from, to } => {
                let Some(src) = self.nodes.get(&from).map(|n| n.node().clone()) else {
                    return;
                };
                let r = match to {
                    None => src.disconnect(),
                    Some(Dest::Node(d)) => match self.nodes.get(&d) {
                        Some(d) => src.disconnect_with_audio_node(d.node()),
                        None => Ok(()),
                    },
                    Some(Dest::Param(p)) => match self.param(p) {
                        Some(p) => src.disconnect_with_audio_param(&p),
                        None => Ok(()),
                    },
                };
                self.check("disconnect", r);
            }
            Op::Start { node, args } => {
                let r = match (self.nodes.get(&node), args) {
                    (Some(WebNode::Osc(o)), []) => o.start(),
                    (Some(WebNode::Osc(o)), [t, ..]) => o.start_with_when(*t),
                    (Some(WebNode::Src(s)), []) => s.start(),
                    (Some(WebNode::Src(s)), [t]) => s.start_with_when(*t),
                    (Some(WebNode::Src(s)), [t, o]) => s.start_with_when_and_grain_offset(*t, *o),
                    (Some(WebNode::Src(s)), [t, o, d, ..]) => {
                        s.start_with_when_and_grain_offset_and_grain_duration(*t, *o, *d)
                    }
                    _ => Ok(()),
                };
                self.check("start", r);
            }
            Op::Stop { node, args } => {
                let r = match (self.nodes.get(&node), args) {
                    (Some(WebNode::Osc(o)), []) => o.stop(),
                    (Some(WebNode::Osc(o)), [t, ..]) => o.stop_with_when(*t),
                    (Some(WebNode::Src(s)), []) => {
                        AsRef::<ws::AudioScheduledSourceNode>::as_ref(s).stop()
                    }
                    (Some(WebNode::Src(s)), [t, ..]) => {
                        AsRef::<ws::AudioScheduledSourceNode>::as_ref(s).stop_with_when(*t)
                    }
                    _ => Ok(()),
                };
                self.check("stop", r);
            }
            Op::SetPeriodicWave { node, wave } => {
                if let (Some(WebNode::Osc(o)), Some(w)) =
                    (self.nodes.get(&node), self.waves.get(&wave))
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
                let (mut re, mut im) = (real.to_vec(), imag.to_vec());
                let c = ws::PeriodicWaveConstraints::new();
                if let Some(d) = disable_normalization {
                    c.set_disable_normalization(d);
                }
                let r = self
                    .ctx
                    .base()
                    .create_periodic_wave_with_constraints(&mut re, &mut im, &c);
                if let Some(w) = self.check("createPeriodicWave", r) {
                    self.waves.insert(wave, w);
                }
            }
            Op::Buffer {
                buffer,
                channels,
                length,
                sample_rate,
            } => {
                let r = self
                    .ctx
                    .base()
                    .create_buffer(channels, length, sample_rate as f32);
                if let Some(b) = self.check("createBuffer", r) {
                    self.buffers.borrow_mut().insert(buffer, b);
                }
            }
            Op::Data { buffer, channels } => {
                let b = self.buffers.borrow().get(&buffer).cloned();
                if let Some(b) = b {
                    for (c, d) in channels.iter().enumerate() {
                        let r = b.copy_to_channel(d, c as i32);
                        self.check("copyToChannel", r);
                    }
                }
            }
        }
    }

    fn param_value(&mut self, param: ParamId) -> f64 {
        self.param(param).map_or(0.0, |p| p.value() as f64)
    }

    fn resume(&mut self, done: Pending<()>) {
        let p = match &self.ctx {
            Ctx::Live(c) => c.resume(),
            Ctx::Offline(c) => c.resume(),
        };
        self.promise(p, done);
    }

    fn suspend(&mut self, done: Pending<()>) {
        match &self.ctx {
            Ctx::Live(c) => {
                let p = c.suspend();
                self.promise(p, done);
            }
            // An offline context suspends at a time (suspend(t)), which the
            // facade does not model yet.
            Ctx::Offline(_) => done.resolve(Err(AudioError::new(
                ErrorName::InvalidStateError,
                "suspend() on an offline context",
            ))),
        }
    }

    fn close(&mut self, done: Pending<()>) {
        match &self.ctx {
            Ctx::Live(c) => {
                let p = c.close();
                self.promise(p, done);
            }
            Ctx::Offline(_) => done.resolve(Err(AudioError::new(
                ErrorName::InvalidStateError,
                "close() on an offline context",
            ))),
        }
    }

    fn decode(&mut self, buffer: BufferId, bytes: &[u8], done: Pending<Decoded>) {
        let ab = js_sys::Uint8Array::from(bytes).buffer();
        let p = match self.ctx.base().decode_audio_data(&ab) {
            Ok(p) => p,
            Err(e) => {
                done.resolve(Err(AudioError::new(ErrorName::EncodingError, js_err(&e))));
                return;
            }
        };
        let buffers = self.buffers.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match wasm_bindgen_futures::JsFuture::from(p).await {
                Ok(v) => {
                    let b: ws::AudioBuffer = v.unchecked_into();
                    let d = Decoded {
                        channels: b.number_of_channels(),
                        length: b.length(),
                        sample_rate: b.sample_rate() as f64,
                    };
                    buffers.borrow_mut().insert(buffer, b);
                    done.resolve(Ok(d));
                }
                Err(e) => done.resolve(Err(AudioError::new(ErrorName::EncodingError, js_err(&e)))),
            }
        });
    }

    fn prepare_exhaust(&mut self, done: Pending<bool>) {
        // One load per context; a second call follows the first.
        if let Some(ready) = &self.exhaust_ready {
            ready.then(move |r| done.resolve(r.clone()));
            return;
        }
        self.exhaust_ready = Some(done.clone());
        let ctx = self.ctx.base().clone();
        let module = self.exhaust_module.clone();
        let failures = self.failures.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match load_exhaust(ctx).await {
                Ok(m) => {
                    *module.borrow_mut() = Some(m);
                    done.resolve(Ok(true));
                }
                Err(e) => {
                    let m = format!("prepareExhaust: {}", js_err(&e));
                    ws_console(&m);
                    failures.borrow_mut().push(m);
                    done.resolve(Ok(false));
                }
            }
        });
    }

    fn release_node(&mut self, node: NodeId) {
        self.nodes.remove(&node);
    }

    fn release_buffer(&mut self, buffer: BufferId) {
        self.buffers.borrow_mut().remove(&buffer);
    }

    fn release_wave(&mut self, wave: WaveId) {
        self.waves.remove(&wave);
    }

    fn js_node(&self, node: NodeId) -> Option<JsValue> {
        self.nodes
            .get(&node)
            .map(|n| JsValue::from(n.node().clone()))
    }

    fn analyser_time_domain(&mut self, node: NodeId, out: &mut [f32]) {
        match self.nodes.get(&node) {
            Some(WebNode::Analyser(a)) => a.get_float_time_domain_data(out),
            _ => out.fill(0.0),
        }
    }

    fn analyser_byte_frequency(&mut self, node: NodeId, out: &mut [u8]) {
        match self.nodes.get(&node) {
            Some(WebNode::Analyser(a)) => a.get_byte_frequency_data(out),
            _ => out.fill(0),
        }
    }
}

fn osc_type(s: &str) -> ws::OscillatorType {
    match OscillatorType::from_js(s) {
        Some(OscillatorType::Square) => ws::OscillatorType::Square,
        Some(OscillatorType::Sawtooth) => ws::OscillatorType::Sawtooth,
        Some(OscillatorType::Triangle) => ws::OscillatorType::Triangle,
        _ => ws::OscillatorType::Sine,
    }
}

fn filter_type(s: &str) -> ws::BiquadFilterType {
    match BiquadFilterType::from_js(s) {
        Some(BiquadFilterType::Highpass) => ws::BiquadFilterType::Highpass,
        Some(BiquadFilterType::Bandpass) => ws::BiquadFilterType::Bandpass,
        Some(BiquadFilterType::Lowshelf) => ws::BiquadFilterType::Lowshelf,
        Some(BiquadFilterType::Highshelf) => ws::BiquadFilterType::Highshelf,
        Some(BiquadFilterType::Peaking) => ws::BiquadFilterType::Peaking,
        Some(BiquadFilterType::Notch) => ws::BiquadFilterType::Notch,
        Some(BiquadFilterType::Allpass) => ws::BiquadFilterType::Allpass,
        _ => ws::BiquadFilterType::Lowpass,
    }
}

/// A web-backed context: `new AudioContext({ latencyHint })`.
pub fn context(latency_hint: Option<&str>) -> Result<super::AudioContext, JsValue> {
    let b = WebBackend::new(latency_hint)?;
    Ok(super::AudioContext::new(
        Box::new(b),
        super::ContextOptions {
            latency_hint: latency_hint.map(str::to_owned),
        },
    ))
}

/// The browser's own node behind a facade node (`None` off the web
/// backend), for the test bridge (`__audio._radioCur.srcs`).
pub fn js_node(node: &super::Node) -> Option<JsValue> {
    node.0.ctx.flush_released();
    node.0.ctx.0.backend.borrow().js_node(node.0.id)
}
