//! Roadmap WP 5.1: conformance of the native backend (the `web-audio-api`
//! crate), rendered offline. Runs with `--features native`:
//!
//!   cargo test -p mr_audio --features native --test native_backend
//!
//! The conformance script runs on it (everything the facade accepts, the
//! crate accepts: it panics on what browsers reject), and small graphs
//! render as Web Audio defines: gain, automation, oscillators, periodic
//! waves, buffer sources, decoding.

#![cfg(all(feature = "native", not(target_arch = "wasm32")))]

mod common;

use mr_audio::wa::native::offline_context;
use mr_audio::wa::{AudioContext, OscillatorType, ProblemKind};

const SR: f32 = 48000.0;

struct Free;

impl common::Driver for Free {
    fn set_now(&self, _t: f64) {}
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / x.len() as f64).sqrt()
}

#[test]
fn the_conformance_script_runs_and_renders() {
    let ctx = offline_context(2, 3 * SR as usize, SR);
    let states = common::script(&ctx, &Free);
    // An offline context has no resume/suspend/close of its own; the clip
    // bytes are not audio.
    assert!(
        states.iter().any(|s| s == "failed EncodingError"),
        "{states:?}"
    );
    // The facade flagged what the fake does (3 ignored, 15 exceptions), and
    // nothing reached the crate to make it panic.
    let p = ctx.problems();
    assert_eq!(
        p.iter().filter(|p| p.kind == ProblemKind::Ignored).count(),
        3
    );
    assert_eq!(
        p.iter()
            .filter(|p| matches!(p.kind, ProblemKind::Throw(_)))
            .count(),
        15
    );
    let out = ctx.start_rendering().expect("an offline render");
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].len(), 3 * SR as usize);
    assert!(rms(&out[0]) > 1e-4, "the script's graph is audible");
}

fn ones(ctx: &AudioContext, len: u32) -> mr_audio::wa::AudioBufferSourceNode {
    let b = ctx.create_buffer(1, len, SR as f64);
    b.copy_to_channel(&vec![1.0; len as usize], 0);
    let s = ctx.create_buffer_source();
    s.set_buffer(Some(&b)).unwrap();
    s
}

#[test]
fn gain_automation_renders_as_scheduled() {
    let n = SR as usize;
    let ctx = offline_context(1, n, SR);
    ctx.set_strict(true);
    let s = ones(&ctx, n as u32);
    let g = ctx.create_gain();
    g.gain.set_value_at_time(0.0, 0.0);
    g.gain.linear_ramp_to_value_at_time(1.0, 0.5);
    g.gain.set_target_at_time(0.25, 0.5, 0.05);
    s.connect(&g).unwrap();
    g.connect(&ctx.destination()).unwrap();
    s.start().unwrap();
    let out = ctx.start_rendering().unwrap();
    let at = |t: f64| out[0][(t * SR as f64) as usize] as f64;
    assert!((at(0.25) - 0.5).abs() < 1e-3, "{}", at(0.25));
    let target = 0.25 + 0.75 * (-(0.1f64) / 0.05).exp();
    assert!((at(0.6) - target).abs() < 1e-3, "{} vs {target}", at(0.6));
    assert!((at(0.99) - 0.25).abs() < 1e-3);
}

#[test]
fn a_sine_has_the_rms_of_a_sine() {
    let n = SR as usize / 2;
    let ctx = offline_context(1, n, SR);
    ctx.set_strict(true);
    let o = ctx.create_oscillator();
    o.set_type(OscillatorType::Sine);
    o.frequency.set_value(1000.0);
    o.connect(&ctx.destination()).unwrap();
    o.start().unwrap();
    let out = ctx.start_rendering().unwrap();
    assert!(
        (rms(&out[0]) - 0.5f64.sqrt()).abs() < 1e-3,
        "{}",
        rms(&out[0])
    );
}

#[test]
fn an_engine_wave_plays_at_the_cycle_rate() {
    let n = SR as usize / 2;
    let ctx = offline_context(1, n, SR);
    ctx.set_strict(true);
    let f = mr_audio::engine::engine_cycle(&mr_audio::engine::SPORTS, 11, false);
    let w = ctx.create_periodic_wave(&f.real, &f.imag, None).unwrap();
    let o = ctx.create_oscillator();
    o.set_periodic_wave(&w);
    o.frequency.set_value(50.0);
    o.connect(&ctx.destination()).unwrap();
    o.start().unwrap();
    let out = ctx.start_rendering().unwrap();
    let x = &out[0];
    // Periodic at 50 Hz: 960 samples a cycle.
    let p = 960;
    let diff: f64 = (p..x.len())
        .map(|i| ((x[i] - x[i - p]) as f64).abs())
        .fold(0.0, f64::max);
    assert!(rms(x) > 0.05, "{}", rms(x));
    assert!(diff < 1e-2, "{diff}");
}

#[test]
fn param_values_read_back() {
    let ctx = offline_context(1, 128, SR);
    let g = ctx.create_gain();
    g.gain.set_value(0.25);
    assert_eq!(g.gain.value(), 0.25);
}

#[test]
fn a_radio_clip_decodes() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let clips: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("parity/golden/audio/radio-clips.json")).unwrap(),
    )
    .unwrap();
    let clip = &clips["clips"][0];
    let file = clip["file"].as_str().unwrap();
    let bytes = std::fs::read(root.join("audio/radio").join(file)).unwrap();
    let ctx = offline_context(1, 128, SR);
    let p = ctx.decode_audio_data(&bytes);
    ctx.settle();
    let b = p.result().expect("settled").expect("decoded");
    let chrome = clip["length"].as_u64().unwrap() as f64;
    assert_eq!(
        b.number_of_channels() as u64,
        clip["channels"].as_u64().unwrap()
    );
    // Chrome's decoder and the crate's (symphonia, resampled to 48 kHz)
    // agree on the duration to within a frame of MP3 padding.
    let diff = (b.length() as f64 - chrome).abs();
    assert!(diff <= 2400.0, "{file}: {} vs Chrome {chrome}", b.length());
}
