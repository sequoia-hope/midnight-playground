//! Roadmap WP 5.1's gate for the null backend (SPEC 7.5): backend
//! conformance and the strict validation mode.
//!
//! - `matches_the_js_fake`: the script of `tools/parity/audio-facade-ref.mjs`
//!   run through the facade gives the JS fake's call log line for line
//!   (`parity/golden/audio/facade-conformance.json`), its problems and its
//!   exceptions: every operation, param value reads through each kind of
//!   automation event, each exception.
//! - The facade's own rules beyond the fake (connection rules, nominal
//!   ranges, cycles, buffers changed after use), strict mode, handles
//!   released on drop, and the reference arrays built through the facade.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use mr_audio::wa::null::{self, ClipInfo, NullHandle, NullOptions};
use mr_audio::wa::{AudioContext, ContextOptions, ContextState, ProblemKind};
use serde_json::Value;

const GOLDEN: &str = include_str!("../../../parity/golden/audio/facade-conformance.json");

fn logged(latency: Option<&str>) -> (AudioContext, NullHandle) {
    null::context(
        NullOptions {
            log: true,
            ..Default::default()
        },
        ContextOptions {
            latency_hint: latency.map(str::to_owned),
        },
    )
}

impl common::Driver for NullHandle {
    fn set_now(&self, t: f64) {
        NullHandle::set_now(self, t);
    }
}

#[test]
fn matches_the_js_fake() {
    let golden: Value = serde_json::from_str(GOLDEN).unwrap();
    let (ctx, h) = logged(Some("balanced"));
    // Three bytes decode to a known clip; anything else fails.
    h.set_decoder(|b| {
        (b.len() == 3).then(|| ClipInfo {
            name: "clip.mp3".into(),
            channels: 1,
            length: 1234,
        })
    });
    let states = common::script(&ctx, &h);
    let mut log = h.log();
    log.push(serde_json::to_string(&serde_json::json!(["states", states])).unwrap());
    let want: Vec<&str> = golden["log"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l.as_str().unwrap())
        .collect();
    for (i, (a, b)) in log.iter().zip(&want).enumerate() {
        assert_eq!(a, b, "line {}", i + 1);
    }
    assert_eq!(log.len(), want.len(), "lines");
    let problems: Vec<String> = ctx
        .problems()
        .iter()
        .filter(|p| p.kind == ProblemKind::Ignored)
        .map(|p| p.message.clone())
        .collect();
    assert_eq!(serde_json::json!(problems), golden["problems"]);
    assert_eq!(serde_json::json!(h.throws()), golden["throws"]);
    // Nothing else flagged: no warnings from the facade's own rules.
    assert_eq!(
        ctx.problems().len(),
        problems.len() + h.throws().len(),
        "{:?}",
        ctx.problems()
    );
}

fn quiet() -> AudioContext {
    null::context(NullOptions::default(), ContextOptions::default()).0
}

fn last_problem(ctx: &AudioContext) -> (ProblemKind, String) {
    let p = ctx.problems().pop().expect("a problem");
    (p.kind, p.message)
}

#[test]
fn connection_rules() {
    let ctx = quiet();
    let other = quiet();
    let g = ctx.create_gain();
    let g2 = other.create_gain();
    let e = g.connect(&g2).unwrap_err();
    assert_eq!(e.name.as_str(), "InvalidAccessError");
    // A source has no input; a merger has as many as it was made with.
    let o = ctx.create_oscillator();
    assert_eq!(g.connect(&o).unwrap_err().name.as_str(), "IndexSizeError");
    let m = ctx.create_channel_merger(2);
    g.connect_with(&m, None, Some(1)).unwrap();
    assert_eq!(
        g.connect_with(&m, None, Some(2)).unwrap_err().name.as_str(),
        "IndexSizeError"
    );
    assert_eq!(
        g.connect_with(&m, Some(1), None).unwrap_err().name.as_str(),
        "IndexSizeError"
    );
    // A cycle without a delay is muted by browsers: a warning; with one, fine.
    let a = ctx.create_gain();
    let b = ctx.create_gain();
    a.connect(&b).unwrap();
    let n = ctx.problems().len();
    b.connect(&a.gain).unwrap();
    assert_eq!(ctx.problems().len(), n + 1);
    assert_eq!(last_problem(&ctx).0, ProblemKind::Warning);
    let d = ctx.create_delay(1.0);
    let x = ctx.create_gain();
    x.connect(&d).unwrap();
    d.connect(&x).unwrap();
    assert_eq!(ctx.problems().len(), n + 1);
    // Bad node arguments.
    let _ = ctx.create_delay(0.0);
    assert_eq!(
        last_problem(&ctx).0,
        ProblemKind::Throw(mr_audio::wa::ErrorName::NotSupportedError)
    );
    let _ = ctx.create_channel_merger(33);
    assert_eq!(
        last_problem(&ctx).0,
        ProblemKind::Throw(mr_audio::wa::ErrorName::IndexSizeError)
    );
}

#[test]
fn nominal_ranges_are_warnings() {
    let ctx = quiet();
    let p = ctx.create_stereo_panner();
    p.pan.set_value(1.5);
    let (k, m) = last_problem(&ctx);
    assert_eq!(k, ProblemKind::Warning);
    assert!(
        m.contains("n1.pan.value 1.5 outside the nominal range [-1, 1]"),
        "{m}"
    );
    let f = ctx.create_biquad_filter();
    f.frequency.set_target_at_time(30000.0, 0.0, 0.1);
    assert!(
        last_problem(&ctx)
            .1
            .contains("outside the nominal range [0, 24000]")
    );
    let c = ctx.create_dynamics_compressor();
    c.ratio.set_value(21.0);
    assert!(last_problem(&ctx).1.contains("n3.ratio"));
    let d = ctx.create_delay(0.5);
    d.delay_time.linear_ramp_to_value_at_time(0.6, 1.0);
    assert!(last_problem(&ctx).1.contains("[0, 0.5]"));
    let n = ctx.problems().len();
    let g = ctx.create_gain();
    g.gain.set_value(-40.0);
    let o = ctx.create_oscillator();
    o.frequency.set_value(-20.0);
    assert_eq!(ctx.problems().len(), n, "in range");
}

#[test]
fn buffers_go_to_the_backend_once_and_stay() {
    let (ctx, h) = logged(None);
    let b = ctx.create_buffer(1, 3, 48000.0);
    b.copy_to_channel(&[1.0, 2.0, 3.0], 0);
    assert_eq!(b.duration(), 3.0 / 48000.0);
    let s = ctx.create_buffer_source();
    s.set_buffer(Some(&b)).unwrap();
    let datas = h.log().iter().filter(|l| l.contains("\"data\"")).count();
    assert_eq!(datas, 1);
    // Changing it after use is flagged (the reference forbids it).
    b.copy_to_channel(&[0.0], 0);
    let (k, m) = last_problem(&ctx);
    assert_eq!(k, ProblemKind::Warning);
    assert!(m.contains("changed after use"));
    assert_eq!(b.get_channel_data(0), vec![0.0, 2.0, 3.0]);
    let hash = mr_audio::array_hash(&[1.0, 2.0, 3.0]);
    assert!(h.with_store(|s| s.get(&hash).is_some()));
    // A convolver's buffer must match the context's rate.
    let ir = ctx.create_buffer(2, 8, 44100.0);
    let cv = ctx.create_convolver();
    assert_eq!(
        cv.set_buffer(Some(&ir)).unwrap_err().name.as_str(),
        "NotSupportedError"
    );
}

#[test]
fn context_state_changes_when_promises_settle() {
    let ctx = quiet();
    assert_eq!(ctx.state(), ContextState::Suspended);
    let p = ctx.resume();
    assert_eq!(ctx.state(), ContextState::Suspended);
    assert!(!p.is_settled());
    ctx.settle();
    assert_eq!(ctx.state(), ContextState::Running);
    assert_eq!(p.result(), Some(Ok(())));
    ctx.close();
    ctx.settle();
    ctx.resume();
    ctx.settle();
    assert_eq!(ctx.state(), ContextState::Closed);
}

#[test]
fn dropped_handles_release_the_backend() {
    let (ctx, h) = logged(None);
    let g = ctx.create_gain();
    let id = g.gain.id();
    g.gain.set_value_at_time(0.5, 0.0);
    assert!(h.timeline(id).is_some());
    drop(g);
    // The release is applied at the next call.
    let _ = ctx.create_gain();
    assert!(h.timeline(id).is_none());
    // A param handle keeps its node.
    let g = ctx.create_gain();
    let p = g.gain.clone();
    drop(g);
    let _ = ctx.create_gain();
    assert!(h.timeline(p.id()).is_some());
}

#[test]
#[should_panic(expected = "exponentialRampToValueAtTime to 0")]
fn strict_mode_panics_on_a_throw() {
    let ctx = quiet();
    ctx.set_strict(true);
    let g = ctx.create_gain();
    g.gain.exponential_ramp_to_value_at_time(0.0, 1.0);
}

#[test]
#[should_panic(expected = "Oscillator.type = \"saw\"")]
fn strict_mode_panics_on_an_invalid_type() {
    let ctx = quiet();
    ctx.set_strict(true);
    ctx.create_oscillator().set_type_js("saw");
}

#[test]
#[should_panic(expected = "outside the nominal range")]
fn strict_mode_panics_on_a_range_warning() {
    let ctx = quiet();
    ctx.set_strict(true);
    ctx.create_stereo_panner().pan.set_value(-2.0);
}

/// The reference's buffers built the way the game builds them: through the
/// facade, given to nodes, hashed by the null backend.
#[test]
fn the_reference_buffers_through_the_facade() {
    let golden: Value =
        serde_json::from_str(include_str!("../../../parity/golden/audio/arrays.json")).unwrap();
    let want = |name: &str| -> Vec<String> {
        golden["arrays"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["name"] == name)
            .unwrap()["hashes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h.as_str().unwrap().to_owned())
            .collect()
    };
    let (ctx, h) = logged(None);
    ctx.set_strict(true);
    for (name, buf) in mr_audio::samples::render_sfx(&ctx)
        .into_iter()
        .map(|(k, b)| (format!("audio.sfxBuf.{k}"), b))
        .chain(
            mr_audio::samples::render_kit(&ctx)
                .into_iter()
                .map(|(k, b)| (format!("audio.music.kit.{k}"), b)),
        )
    {
        let s = ctx.create_buffer_source();
        s.set_buffer(Some(&buf)).unwrap();
        let line = h.log().pop().unwrap();
        let hashes: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(hashes[1], "data", "{name}");
        assert_eq!(hashes[3], serde_json::json!(want(&name)), "{name}");
    }
    let curve = mr_audio::shapes::exhaust_curve(2.2, 2048);
    let sh = ctx.create_wave_shaper();
    sh.set_curve(Some(&curve));
    let line: Value = serde_json::from_str(&h.log().pop().unwrap()).unwrap();
    assert_eq!(line[4], want("audio.eng.L.shaper.curve")[0]);
}
