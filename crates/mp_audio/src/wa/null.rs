//! The null backend (SPEC 7.1): plays nothing and records every call, in
//! the call log format of `tools/parity/lib/webaudio-fake.mjs`
//! (`parity/golden/audio/README.md`), on a clock its driver advances. Used
//! headless (no logging) and by the parity and strict tests (with the log).
//!
//! It keeps each param's automation timeline so `param.value` reads give
//! what the JS fake gives (DECISIONS D42), and settles promises (`resume`,
//! `suspend`, `close`, `decodeAudioData`) when the driver calls
//! [`super::AudioContext::settle`], as the fake does in a microtask.

use super::backend::{Attr, Backend, BufferId, NodeId, Op, WaveId};
use super::log::{Val, js_number, line};
use super::timeline::{Event, EventKind, Timeline};
use super::{
    AudioError, ContextState, Decoded, Dest, ErrorName, ParamId, ParamName, Pending, param_initial,
    params_of,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

/// `'#'` + the first 16 hex digits of the SHA-256 of an array's
/// little-endian float32 bytes: how the reference names arrays.
pub fn array_hash(a: &[f32]) -> String {
    crate::array_hash(a)
}

/// Arrays by content hash, in the order first seen (the fake's
/// `ArrayStore`).
#[derive(Default)]
pub struct ArrayStore {
    index: HashMap<String, usize>,
    arrays: Vec<(String, Vec<f32>)>,
}

impl ArrayStore {
    pub fn add(&mut self, a: &[f32]) -> String {
        let h = array_hash(a);
        if !self.index.contains_key(&h) {
            self.index.insert(h.clone(), self.arrays.len());
            self.arrays.push((h.clone(), a.to_vec()));
        }
        h
    }

    pub fn get(&self, hash: &str) -> Option<&[f32]> {
        self.index.get(hash).map(|&i| self.arrays[i].1.as_slice())
    }

    /// Every array, in the order first seen.
    pub fn arrays(&self) -> &[(String, Vec<f32>)] {
        &self.arrays
    }
}

/// What a clip decodes to, for the null backend's stand-in decoder
/// (DECISIONS D47): a name for the log, channels and length.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipInfo {
    pub name: String,
    pub channels: u32,
    pub length: u32,
}

type Decoder = Box<dyn Fn(&[u8]) -> Option<ClipInfo>>;
type Task = Box<dyn FnOnce()>;

/// The state a driver shares with the backend: the clock, the log, the
/// arrays, the context state.
pub struct NullShared {
    sample_rate: f64,
    now: Cell<f64>,
    state: Cell<ContextState>,
    log: Option<RefCell<Vec<String>>>,
    store: RefCell<ArrayStore>,
    params: RefCell<HashMap<NodeId, Vec<(ParamName, Timeline)>>>,
    decoder: RefCell<Option<Decoder>>,
    tasks: RefCell<Vec<Task>>,
    throws: RefCell<Vec<String>>,
}

/// The driver's handle on a null backend.
#[derive(Clone)]
pub struct NullHandle(Rc<NullShared>);

impl NullHandle {
    /// The current time in seconds.
    pub fn now(&self) -> f64 {
        self.0.now.get()
    }

    /// Move the clock (the fake's `clock.now`).
    pub fn set_now(&self, t: f64) {
        self.0.now.set(t);
    }

    /// The call log so far (empty when not logging).
    pub fn log(&self) -> Vec<String> {
        self.0
            .log
            .as_ref()
            .map(|l| l.borrow().clone())
            .unwrap_or_default()
    }

    /// Take the call log so far, leaving it empty.
    pub fn take_log(&self) -> Vec<String> {
        self.0
            .log
            .as_ref()
            .map(|l| std::mem::take(&mut *l.borrow_mut()))
            .unwrap_or_default()
    }

    /// Run `f` on the array store.
    pub fn with_store<R>(&self, f: impl FnOnce(&ArrayStore) -> R) -> R {
        f(&self.0.store.borrow())
    }

    /// Exceptions a browser would have thrown, as `"Name: message"`.
    pub fn throws(&self) -> Vec<String> {
        self.0.throws.borrow().clone()
    }

    /// Stand in for `decodeAudioData` (null: every clip fails to decode).
    pub fn set_decoder(&self, d: impl Fn(&[u8]) -> Option<ClipInfo> + 'static) {
        *self.0.decoder.borrow_mut() = Some(Box::new(d));
    }

    /// A param's automation timeline (for tests).
    pub fn timeline(&self, p: ParamId) -> Option<Timeline> {
        self.0.params.borrow().get(&p.node).and_then(|ps| {
            ps.iter()
                .find(|(n, _)| *n == p.name)
                .map(|(_, t)| t.clone())
        })
    }
}

/// The null backend.
pub struct NullBackend(Rc<NullShared>);

/// Options for a null backend.
#[derive(Clone, Debug)]
pub struct NullOptions {
    pub sample_rate: f64,
    /// Keep the call log (the parity tests); off for plain headless use.
    pub log: bool,
    /// The state a new context starts in (a live context: suspended).
    pub state: ContextState,
}

impl Default for NullOptions {
    fn default() -> Self {
        NullOptions {
            sample_rate: 48000.0,
            log: false,
            state: ContextState::Suspended,
        }
    }
}

impl NullBackend {
    pub fn new(opts: NullOptions) -> (NullBackend, NullHandle) {
        let shared = Rc::new(NullShared {
            sample_rate: opts.sample_rate,
            now: Cell::new(0.0),
            state: Cell::new(opts.state),
            log: opts.log.then(|| RefCell::new(Vec::new())),
            store: RefCell::new(ArrayStore::default()),
            params: RefCell::new(HashMap::new()),
            decoder: RefCell::new(None),
            tasks: RefCell::new(Vec::new()),
            throws: RefCell::new(Vec::new()),
        });
        (NullBackend(shared.clone()), NullHandle(shared))
    }

    fn log(&self, op: &str, args: Vec<Val>) {
        if let Some(l) = &self.0.log {
            l.borrow_mut().push(line(self.0.now.get(), op, &args));
        }
    }

    fn logging(&self) -> bool {
        self.0.log.is_some()
    }

    fn with_timeline(&self, p: ParamId, f: impl FnOnce(&mut Timeline)) {
        if let Some(ps) = self.0.params.borrow_mut().get_mut(&p.node)
            && let Some((_, t)) = ps.iter_mut().find(|(n, _)| *n == p.name)
        {
            f(t);
        }
    }

    fn queue(&self, t: Task) {
        self.0.tasks.borrow_mut().push(t);
    }
}

fn node_id(n: NodeId) -> Val {
    Val::Str(format!("n{n}"))
}

fn dest_id(d: Dest) -> Val {
    match d {
        Dest::Node(n) => node_id(n),
        Dest::Param(p) => param_id(p),
    }
}

fn param_id(p: ParamId) -> Val {
    Val::Str(format!("n{}.{}", p.node, p.name.as_str()))
}

fn buffer_id(b: BufferId) -> Val {
    Val::Str(format!("b{b}"))
}

fn wave_id(w: WaveId) -> Val {
    Val::Str(format!("w{w}"))
}

impl Backend for NullBackend {
    fn sample_rate(&self) -> f64 {
        self.0.sample_rate
    }

    fn current_time(&self) -> f64 {
        self.0.now.get()
    }

    fn state(&self) -> ContextState {
        self.0.state.get()
    }

    fn record(&mut self, op: &Op) {
        // The array store fills even without a log: the arrays are what the
        // parity tests read back.
        let n = Val::Num;
        match *op {
            Op::Context {
                sample_rate,
                latency_hint,
            } => self.log(
                "context",
                vec![Val::Obj(vec![
                    ("sampleRate".into(), n(sample_rate)),
                    (
                        "latencyHint".into(),
                        latency_hint.map_or(Val::Undefined, Val::str),
                    ),
                ])],
            ),
            Op::New { node, kind, arg } => {
                let mut a = vec![node_id(node), Val::str(kind.as_str())];
                if let Some(x) = arg {
                    a.push(n(x));
                }
                self.log("new", a);
            }
            Op::Attr { node, attr } => {
                let v = match attr {
                    Attr::Type(s) | Attr::Oversample(s) => Val::str(s),
                    Attr::Loop(b) | Attr::Normalize(b) => Val::Bool(b),
                    Attr::LoopStart(x) | Attr::LoopEnd(x) | Attr::SmoothingTimeConstant(x) => n(x),
                    Attr::FftSize(x) => n(x as f64),
                    Attr::Buffer(b) => b.map_or(Val::Null, buffer_id),
                    Attr::Curve(c) => match c {
                        Some(c) => Val::Str(self.0.store.borrow_mut().add(c)),
                        None => Val::Null,
                    },
                };
                self.log("set", vec![node_id(node), Val::str(attr.name()), v]);
            }
            Op::Value { param, v } => self.log("value", vec![param_id(param), n(v)]),
            Op::SetValue { param, v, t } => self.log("setValue", vec![param_id(param), n(v), n(t)]),
            Op::LinRamp { param, v, t } => self.log("linRamp", vec![param_id(param), n(v), n(t)]),
            Op::ExpRamp { param, v, t } => self.log("expRamp", vec![param_id(param), n(v), n(t)]),
            Op::SetTarget { param, v, t, tc } => {
                self.log("setTarget", vec![param_id(param), n(v), n(t), n(tc)])
            }
            Op::Cancel { param, t } => self.log("cancel", vec![param_id(param), n(t)]),
            Op::Connect {
                from,
                to,
                output,
                input,
            } => {
                let mut a = vec![node_id(from), dest_id(to)];
                a.extend(output.map(|x| n(x as f64)));
                a.extend(input.map(|x| n(x as f64)));
                self.log("connect", a);
            }
            Op::Disconnect { from, to } => {
                let mut a = vec![node_id(from)];
                a.extend(to.map(dest_id));
                self.log("disconnect", a);
            }
            Op::Start { node, args } | Op::Stop { node, args } => {
                let mut a = vec![node_id(node)];
                a.extend(args.iter().map(|x| n(*x)));
                let what = if matches!(op, Op::Start { .. }) {
                    "start"
                } else {
                    "stop"
                };
                self.log(what, a);
            }
            Op::SetPeriodicWave { node, wave } => {
                self.log("setPeriodicWave", vec![node_id(node), wave_id(wave)])
            }
            Op::Wave {
                wave,
                real,
                imag,
                disable_normalization,
            } => {
                let opts = Val::Obj(vec![(
                    "disableNormalization".into(),
                    disable_normalization.map_or(Val::Undefined, Val::Bool),
                )]);
                if real.len() != imag.len() || real.len() < 2 {
                    self.log("wave", vec![wave_id(wave), Val::Null, Val::Null, opts]);
                } else {
                    let (hr, hi) = {
                        let mut s = self.0.store.borrow_mut();
                        (s.add(real), s.add(imag))
                    };
                    self.log(
                        "wave",
                        vec![wave_id(wave), Val::Str(hr), Val::Str(hi), opts],
                    );
                }
            }
            Op::Buffer {
                buffer,
                channels,
                length,
                sample_rate,
            } => self.log(
                "buffer",
                vec![
                    buffer_id(buffer),
                    n(channels as f64),
                    n(length as f64),
                    n(sample_rate),
                ],
            ),
            Op::Data { buffer, channels } => {
                let hashes: Vec<Val> = {
                    let mut s = self.0.store.borrow_mut();
                    channels.iter().map(|d| Val::Str(s.add(d))).collect()
                };
                self.log("data", vec![buffer_id(buffer), Val::List(hashes)]);
            }
            Op::Resume => self.log("resume", vec![]),
            Op::Suspend => self.log("suspend", vec![]),
            Op::Close => self.log("close", vec![]),
        }
    }

    fn record_throw(&mut self, err: &AudioError) {
        self.0
            .throws
            .borrow_mut()
            .push(format!("{}: {}", err.name.as_str(), err.message));
        self.log(
            "throw",
            vec![Val::str(err.name.as_str()), Val::str(err.message.as_str())],
        );
    }

    fn apply(&mut self, op: &Op) {
        let now = self.0.now.get();
        match *op {
            Op::New { node, kind, arg } => {
                let ps = params_of(kind)
                    .iter()
                    .map(|&p| {
                        let v = param_initial(kind, p, self.0.sample_rate, arg)
                            .expect("a param of this kind");
                        (p, Timeline::new(v))
                    })
                    .collect();
                self.0.params.borrow_mut().insert(node, ps);
            }
            Op::Value { param, v } => self.with_timeline(param, |t| t.set_value(v, now)),
            Op::SetValue { param, v, t } => self.with_timeline(param, |tl| {
                tl.insert(Event {
                    kind: EventKind::Set,
                    v,
                    t,
                })
            }),
            Op::LinRamp { param, v, t } => self.with_timeline(param, |tl| {
                tl.insert(Event {
                    kind: EventKind::Lin,
                    v,
                    t,
                })
            }),
            Op::ExpRamp { param, v, t } => self.with_timeline(param, |tl| {
                tl.insert(Event {
                    kind: EventKind::Exp,
                    v,
                    t,
                })
            }),
            Op::SetTarget { param, v, t, tc } => self.with_timeline(param, |tl| {
                tl.insert(Event {
                    kind: EventKind::Target { tc },
                    v,
                    t,
                })
            }),
            Op::Cancel { param, t } => self.with_timeline(param, |tl| tl.cancel(t)),
            _ => {}
        }
    }

    fn param_value(&mut self, param: ParamId) -> f64 {
        let now = self.0.now.get();
        let mut v = 0.0;
        self.with_timeline(param, |t| v = t.at(now) as f32 as f64);
        if self.logging() {
            self.log("get", vec![param_id(param), Val::Num(v)]);
        }
        v
    }

    fn resume(&mut self, done: Pending<()>) {
        let sh = self.0.clone();
        self.queue(Box::new(move || {
            if sh.state.get() != ContextState::Closed {
                sh.state.set(ContextState::Running);
            }
            done.resolve(Ok(()));
        }));
    }

    fn suspend(&mut self, done: Pending<()>) {
        let sh = self.0.clone();
        self.queue(Box::new(move || {
            if sh.state.get() != ContextState::Closed {
                sh.state.set(ContextState::Suspended);
            }
            done.resolve(Ok(()));
        }));
    }

    fn close(&mut self, done: Pending<()>) {
        let sh = self.0.clone();
        self.queue(Box::new(move || {
            sh.state.set(ContextState::Closed);
            done.resolve(Ok(()));
        }));
    }

    fn decode(&mut self, buffer: BufferId, bytes: &[u8], done: Pending<Decoded>) {
        let info = self.0.decoder.borrow().as_ref().and_then(|d| d(bytes));
        let sr = self.0.sample_rate;
        match info {
            Some(c) => {
                self.log(
                    "decode",
                    vec![
                        buffer_id(buffer),
                        Val::Str(c.name.clone()),
                        Val::Num(c.channels as f64),
                        Val::Num(c.length as f64),
                        Val::Num(sr),
                    ],
                );
                self.queue(Box::new(move || {
                    done.resolve(Ok(Decoded {
                        channels: c.channels,
                        length: c.length,
                        sample_rate: sr,
                    }))
                }));
            }
            None => {
                self.log("decode", vec![buffer_id(buffer), Val::Null]);
                self.queue(Box::new(move || {
                    done.resolve(Err(AudioError::new(
                        ErrorName::EncodingError,
                        "decodeAudioData: unknown clip",
                    )))
                }));
            }
        }
    }

    fn prepare_exhaust(&mut self, done: Pending<bool>) {
        // Nothing to load: ready at the next settle, as a promise would be.
        self.queue(Box::new(move || done.resolve(Ok(true))));
    }

    fn release_node(&mut self, node: NodeId) {
        self.0.params.borrow_mut().remove(&node);
    }

    fn settle(&mut self) -> Vec<Box<dyn FnOnce()>> {
        std::mem::take(&mut *self.0.tasks.borrow_mut())
    }
}

/// A null-backed context: `(context, handle)`.
pub fn context(
    opts: NullOptions,
    ctx_opts: super::ContextOptions,
) -> (super::AudioContext, NullHandle) {
    let (b, h) = NullBackend::new(opts);
    (super::AudioContext::new(Box::new(b), ctx_opts), h)
}

/// `js_number` re-exported for drivers that format times as the log does.
pub fn fmt_num(x: f64) -> String {
    js_number(x)
}
