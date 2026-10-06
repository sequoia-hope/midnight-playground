//! The JS game's own Web Audio traffic through the facade (roadmap WP 5.1):
//! each drive's call log in the cache (`parity/cache/<key>/audio/
//! calllog-<drive>.jsonl`, rebuilt by `node tools/parity/audio-ref.mjs
//! calllog`; format in `parity/golden/audio/README.md`) is replayed call by
//! call into a facade on the null backend, which must write the same log
//! back (every line but the `api` markers, including each `param.value`
//! read, evaluated on the null backend's timelines) and flag no problem: the
//! strict validation accepts everything the game does.
//!
//! Skipped (with a note) when the cache does not hold the logs; native only.

#![cfg(not(target_arch = "wasm32"))]

use mp_audio::wa::null::{self, ClipInfo, NullHandle, NullOptions};
use mp_audio::wa::{
    AnalyserNode, AudioBuffer, AudioBufferSourceNode, AudioContext, AudioParam, BiquadFilterNode,
    ContextOptions, ConvolverNode, DelayNode, DynamicsCompressorNode, GainNode, Node,
    OscillatorNode, PeriodicWave, StereoPannerNode, WaveShaperNode,
};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

enum Any {
    Destination(Node),
    Gain(GainNode),
    Biquad(BiquadFilterNode),
    Osc(OscillatorNode),
    Src(AudioBufferSourceNode),
    Shaper(WaveShaperNode),
    Comp(DynamicsCompressorNode),
    Conv(ConvolverNode),
    Delay(DelayNode),
    Merger(Node),
    Pan(StereoPannerNode),
    #[allow(dead_code)]
    Analyser(AnalyserNode),
}

impl Any {
    fn node(&self) -> &Node {
        match self {
            Any::Destination(n) | Any::Merger(n) => n,
            Any::Gain(n) => n,
            Any::Biquad(n) => n,
            Any::Osc(n) => n,
            Any::Src(n) => n,
            Any::Shaper(n) => n,
            Any::Comp(n) => n,
            Any::Conv(n) => n,
            Any::Delay(n) => n,
            Any::Pan(n) => n,
            Any::Analyser(n) => n,
        }
    }

    fn param(&self, name: &str) -> &AudioParam {
        match (self, name) {
            (Any::Gain(n), "gain") => &n.gain,
            (Any::Biquad(n), "frequency") => &n.frequency,
            (Any::Biquad(n), "detune") => &n.detune,
            (Any::Biquad(n), "Q") => &n.q,
            (Any::Biquad(n), "gain") => &n.gain,
            (Any::Osc(n), "frequency") => &n.frequency,
            (Any::Osc(n), "detune") => &n.detune,
            (Any::Src(n), "playbackRate") => &n.playback_rate,
            (Any::Src(n), "detune") => &n.detune,
            (Any::Comp(n), "threshold") => &n.threshold,
            (Any::Comp(n), "knee") => &n.knee,
            (Any::Comp(n), "ratio") => &n.ratio,
            (Any::Comp(n), "attack") => &n.attack,
            (Any::Comp(n), "release") => &n.release,
            (Any::Delay(n), "delayTime") => &n.delay_time,
            (Any::Pan(n), "pan") => &n.pan,
            _ => panic!("no param {name} on {:?}", self.node()),
        }
    }
}

/// Arrays by hash from `calllog-<drive>.arrays.{json,bin}`.
fn arrays(dir: &Path, drive: &str) -> HashMap<String, Vec<f32>> {
    let idx: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join(format!("calllog-{drive}.arrays.json"))).unwrap(),
    )
    .unwrap();
    let bin = std::fs::read(dir.join(format!("calllog-{drive}.arrays.bin"))).unwrap();
    let mut out = HashMap::new();
    for e in idx.as_array().unwrap() {
        let off = e["offset"].as_u64().unwrap() as usize;
        let len = e["length"].as_u64().unwrap() as usize;
        let v: Vec<f32> = bin[off * 4..(off + len) * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect();
        out.insert(e["hash"].as_str().unwrap().to_owned(), v);
    }
    out
}

fn cache_dir() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../parity/cache");
    let golden: Value =
        serde_json::from_str(include_str!("../../../parity/golden/audio/calllog.json")).unwrap();
    for d in std::fs::read_dir(root).ok()?.flatten() {
        let dir = d.path().join("audio");
        // The cache entry whose logs are the golden's (by size and hash).
        let ok = golden["drives"].as_array().unwrap().iter().all(|g| {
            let p = dir.join(format!("calllog-{}.jsonl", g["name"].as_str().unwrap()));
            std::fs::metadata(&p).is_ok_and(|m| m.len() == g["bytes"].as_u64().unwrap())
        });
        if ok {
            return Some(dir);
        }
    }
    None
}

fn num(v: &Value) -> f64 {
    match v {
        Value::String(s) => match s.as_str() {
            "NaN" => f64::NAN,
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            _ => panic!("not a number: {s}"),
        },
        v => v.as_f64().unwrap_or_else(|| panic!("not a number: {v}")),
    }
}

fn id(v: &Value, prefix: char) -> u32 {
    v.as_str()
        .unwrap()
        .strip_prefix(prefix)
        .unwrap()
        .parse()
        .unwrap()
}

struct Replay {
    ctx: AudioContext,
    h: NullHandle,
    nodes: HashMap<u32, Any>,
    buffers: HashMap<u32, AudioBuffer>,
    waves: HashMap<u32, PeriodicWave>,
    arrays: HashMap<String, Vec<f32>>,
    /// Buffer contents by buffer id, from the log's `data` lines.
    data: HashMap<u32, Vec<String>>,
    next_decode: Rc<RefCell<Option<ClipInfo>>>,
}

impl Replay {
    fn dest(&self, v: &Value) -> Result<&Any, (&Any, String)> {
        let s = v.as_str().unwrap();
        match s.split_once('.') {
            None => Ok(&self.nodes[&s[1..].parse::<u32>().unwrap()]),
            Some((n, p)) => Err((&self.nodes[&n[1..].parse::<u32>().unwrap()], p.to_owned())),
        }
    }

    fn param(&self, v: &Value) -> &AudioParam {
        match self.dest(v) {
            Err((n, p)) => n.param(&p),
            Ok(_) => panic!("not a param: {v}"),
        }
    }

    fn run(&mut self, r: &[Value]) {
        let op = r[1].as_str().unwrap();
        match op {
            "new" => {
                let n = id(&r[2], 'n');
                let c = &self.ctx;
                let node = match r[3].as_str().unwrap() {
                    "Destination" => {
                        assert_eq!(n, 0);
                        Any::Destination(c.destination())
                    }
                    "Gain" => Any::Gain(c.create_gain()),
                    "BiquadFilter" => Any::Biquad(c.create_biquad_filter()),
                    "Oscillator" => Any::Osc(c.create_oscillator()),
                    "BufferSource" => Any::Src(c.create_buffer_source()),
                    "WaveShaper" => Any::Shaper(c.create_wave_shaper()),
                    "DynamicsCompressor" => Any::Comp(c.create_dynamics_compressor()),
                    "Convolver" => Any::Conv(c.create_convolver()),
                    "Delay" => Any::Delay(c.create_delay(num(&r[4]))),
                    "ChannelMerger" => Any::Merger(c.create_channel_merger(num(&r[4]) as u32)),
                    "StereoPanner" => Any::Pan(c.create_stereo_panner()),
                    k => panic!("node kind {k}"),
                };
                assert_eq!(node.node().id(), n, "node ids");
                self.nodes.insert(n, node);
            }
            "set" => {
                let node = &self.nodes[&id(&r[2], 'n')];
                let v = &r[4];
                match (node, r[3].as_str().unwrap()) {
                    (Any::Osc(o), "type") => o.set_type_js(v.as_str().unwrap()),
                    (Any::Biquad(f), "type") => f.set_type_js(v.as_str().unwrap()),
                    (Any::Src(s), "loop") => s.set_loop(v.as_bool().unwrap()),
                    (Any::Src(s), "loopStart") => s.set_loop_start(num(v)),
                    (Any::Src(s), "loopEnd") => s.set_loop_end(num(v)),
                    (Any::Src(s), "buffer") => s
                        .set_buffer(v.as_str().map(|_| &self.buffers[&id(v, 'b')]))
                        .unwrap(),
                    (Any::Conv(c), "buffer") => c
                        .set_buffer(v.as_str().map(|_| &self.buffers[&id(v, 'b')]))
                        .unwrap(),
                    (Any::Conv(c), "normalize") => c.set_normalize(v.as_bool().unwrap()),
                    (Any::Shaper(s), "oversample") => s.set_oversample_js(v.as_str().unwrap()),
                    (Any::Shaper(s), "curve") => {
                        s.set_curve(v.as_str().map(|h| self.arrays[h].as_slice()))
                    }
                    (_, a) => panic!("attribute {a}"),
                }
            }
            "value" => self.param(&r[2]).set_value(num(&r[3])),
            "get" => {
                self.param(&r[2]).value();
            }
            "setValue" => {
                self.param(&r[2]).set_value_at_time(num(&r[3]), num(&r[4]));
            }
            "linRamp" => {
                self.param(&r[2])
                    .linear_ramp_to_value_at_time(num(&r[3]), num(&r[4]));
            }
            "expRamp" => {
                self.param(&r[2])
                    .exponential_ramp_to_value_at_time(num(&r[3]), num(&r[4]));
            }
            "setTarget" => {
                self.param(&r[2])
                    .set_target_at_time(num(&r[3]), num(&r[4]), num(&r[5]));
            }
            "cancel" => {
                self.param(&r[2]).cancel_scheduled_values(num(&r[3]));
            }
            "connect" => {
                let from = self.nodes[&id(&r[2], 'n')].node();
                let o = r.get(4).map(|x| num(x) as u32);
                let i = r.get(5).map(|x| num(x) as u32);
                match self.dest(&r[3]) {
                    Ok(d) => from.connect_with(d.node(), o, i).unwrap(),
                    Err((n, p)) => from.connect_with(n.param(&p), o, i).unwrap(),
                }
            }
            "disconnect" => {
                let from = self.nodes[&id(&r[2], 'n')].node();
                match r.get(3) {
                    None => from.disconnect(),
                    Some(d) => match self.dest(d) {
                        Ok(d) => from.disconnect_from(d.node()).unwrap(),
                        Err((n, p)) => from.disconnect_from(n.param(&p)).unwrap(),
                    },
                }
            }
            "start" | "stop" => {
                let args: Vec<f64> = r[3..].iter().map(num).collect();
                let start = op == "start";
                match &self.nodes[&id(&r[2], 'n')] {
                    Any::Osc(o) => match (start, args.as_slice()) {
                        (true, []) => o.start().unwrap(),
                        (true, [t]) => o.start_at(*t).unwrap(),
                        (false, []) => o.stop().unwrap(),
                        (false, [t]) => o.stop_at(*t).unwrap(),
                        _ => panic!("{op} {args:?}"),
                    },
                    Any::Src(s) => {
                        if start {
                            s.start_args(&args).unwrap()
                        } else {
                            match args.as_slice() {
                                [] => s.stop().unwrap(),
                                [t] => s.stop_at(*t).unwrap(),
                                _ => panic!("stop {args:?}"),
                            }
                        }
                    }
                    _ => panic!("{op} on a node that is not a source"),
                }
            }
            "setPeriodicWave" => match &self.nodes[&id(&r[2], 'n')] {
                Any::Osc(o) => o.set_periodic_wave(&self.waves[&id(&r[3], 'w')]),
                _ => panic!("setPeriodicWave"),
            },
            "wave" => {
                let dn = r[5].get("disableNormalization").and_then(Value::as_bool);
                let w = self
                    .ctx
                    .create_periodic_wave(
                        &self.arrays[r[3].as_str().unwrap()],
                        &self.arrays[r[4].as_str().unwrap()],
                        dn,
                    )
                    .unwrap();
                assert_eq!(w.id(), id(&r[2], 'w'));
                self.waves.insert(w.id(), w);
            }
            "buffer" => {
                let b = self
                    .ctx
                    .create_buffer(num(&r[3]) as u32, num(&r[4]) as u32, num(&r[5]));
                assert_eq!(b.id(), id(&r[2], 'b'));
                if let Some(hs) = self.data.get(&b.id()) {
                    for (c, h) in hs.iter().enumerate() {
                        b.copy_to_channel(&self.arrays[h], c as u32);
                    }
                }
                self.buffers.insert(b.id(), b);
            }
            "data" => {} // written by the facade when the buffer is first used
            "decode" => {
                *self.next_decode.borrow_mut() = r[3].as_str().map(|name| ClipInfo {
                    name: name.to_owned(),
                    channels: num(&r[4]) as u32,
                    length: num(&r[5]) as u32,
                });
                let p = self.ctx.decode_audio_data(&[]);
                self.ctx.settle();
                if let Some(Ok(b)) = p.result() {
                    assert_eq!(b.id(), id(&r[2], 'b'));
                    self.buffers.insert(b.id(), b);
                }
            }
            "resume" => {
                self.ctx.resume();
                self.ctx.settle();
            }
            "suspend" => {
                self.ctx.suspend();
                self.ctx.settle();
            }
            "close" => {
                self.ctx.close();
                self.ctx.settle();
            }
            o => panic!("op {o}"),
        }
    }
}

fn replay(dir: &Path, drive: &str) {
    let text = std::fs::read_to_string(dir.join(format!("calllog-{drive}.jsonl"))).unwrap();
    let lines: Vec<&str> = text.lines().filter(|l| !l.contains(",\"api\",")).collect();
    let rows: Vec<Vec<Value>> = lines
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let mut data = HashMap::new();
    for r in &rows {
        if r[1] == "data" {
            let hs = r[3]
                .as_array()
                .unwrap()
                .iter()
                .map(|h| h.as_str().unwrap().to_owned())
                .collect();
            data.insert(id(&r[2], 'b'), hs);
        }
    }
    let first = &rows[0];
    assert_eq!(first[1], "context");
    let (ctx, h) = null::context(
        NullOptions {
            sample_rate: num(&first[2]["sampleRate"]),
            log: true,
            ..Default::default()
        },
        ContextOptions {
            latency_hint: first[2]["latencyHint"].as_str().map(str::to_owned),
        },
    );
    let next_decode: Rc<RefCell<Option<ClipInfo>>> = Rc::new(RefCell::new(None));
    let nd = next_decode.clone();
    h.set_decoder(move |_| nd.borrow_mut().take());
    let mut rp = Replay {
        ctx,
        h,
        nodes: HashMap::new(),
        buffers: HashMap::new(),
        waves: HashMap::new(),
        arrays: arrays(dir, drive),
        data,
        next_decode,
    };
    // The context line and the destination are written by the constructor.
    assert_eq!(rows[1][1], "new");
    rp.nodes.insert(0, Any::Destination(rp.ctx.destination()));
    for r in rows.iter().skip(2) {
        rp.h.set_now(num(&r[0]));
        rp.run(r);
    }
    let ours = rp.h.take_log();
    let mut first_diff = None;
    for (i, (a, b)) in ours.iter().zip(&lines).enumerate() {
        if a != b {
            first_diff = Some(format!("line {}: ours {a}\n            JS   {b}", i + 1));
            break;
        }
    }
    assert!(first_diff.is_none(), "{drive}: {}", first_diff.unwrap());
    assert_eq!(ours.len(), lines.len(), "{drive}: lines");
    let problems = rp.ctx.problems();
    assert!(
        problems.is_empty(),
        "{drive}: {} problems, first {:?}",
        problems.len(),
        &problems[..problems.len().min(10)]
    );
    println!(
        "{drive}: {} calls replayed, the same log back, no problems",
        ours.len()
    );
}

#[test]
fn the_games_call_logs_replay_through_the_facade() {
    let Some(dir) = cache_dir() else {
        eprintln!(
            "no cached call logs matching calllog.json: run `node tools/parity/audio-ref.mjs calllog`"
        );
        return;
    };
    for drive in ["race", "pursuit"] {
        replay(&dir, drive);
    }
}
