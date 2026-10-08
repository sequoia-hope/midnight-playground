//! The radio node (`create_radio`): `mp_music`'s station player behind the
//! facade (DECISIONS D1151). On the null backend it is logged and validated
//! like any node; on the native backend it renders the station.
//!
//!   cargo test -p mp_audio --test radio_node                    (null)
//!   cargo test -p mp_audio --features native --test radio_node  (both)

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

use mp_audio::wa::null::{self, NullOptions};
use mp_audio::wa::{ContextOptions, ErrorName, NodeKind, ProblemKind};

#[test]
fn the_null_backend_logs_it_like_any_node() {
    let (ctx, h) = null::context(
        NullOptions {
            log: true,
            ..Default::default()
        },
        ContextOptions::default(),
    );
    ctx.set_strict(true);
    let ready = ctx.prepare_radio();
    assert!(!ready.is_settled(), "settles as a promise would");
    ctx.settle();
    assert_eq!(ready.result(), Some(Ok(true)));

    let r = ctx.create_radio();
    assert_eq!(r.kind(), NodeKind::Radio);
    // Off until tuned, at full energy.
    assert_eq!(r.station.value(), -1.0);
    assert_eq!(r.energy.value(), 1.0);
    r.station.set_value_at_time(1.0, 0.0);
    r.wall_day.set_value_at_time(20543.0, 0.0);
    r.wall_sec.set_value_at_time(84800.0, 0.0);
    r.tune.set_value_at_time(1.0, 0.0);
    r.energy.set_target_at_time(0.5, 0.5, 0.5);
    r.station.set_value_at_time(-1.0, 2.0);
    r.connect(&ctx.destination()).unwrap();
    h.set_now(2.0);
    assert_eq!(r.station.value(), -1.0);
    let log = h.log();
    let want = [
        r#"[0,"context",{"sampleRate":48000}]"#,
        r#"[0,"new","n0","Destination"]"#,
        r#"[0,"new","n1","Radio"]"#,
        r#"[0,"get","n1.station",-1]"#,
        r#"[0,"get","n1.energy",1]"#,
        r#"[0,"setValue","n1.station",1,0]"#,
        r#"[0,"setValue","n1.wallDay",20543,0]"#,
        r#"[0,"setValue","n1.wallSec",84800,0]"#,
        r#"[0,"setValue","n1.tune",1,0]"#,
        r#"[0,"setTarget","n1.energy",0.5,0.5,0.5]"#,
        r#"[0,"setValue","n1.station",-1,2]"#,
        r#"[0,"connect","n1","n0"]"#,
        r#"[2,"get","n1.station",-1]"#,
    ];
    assert_eq!(log, want);
    assert!(ctx.problems().is_empty());
}

#[test]
fn its_params_and_connections_are_validated() {
    let ctx = null::context(NullOptions::default(), ContextOptions::default()).0;
    let r = ctx.create_radio();
    let last = || ctx.problems().pop().expect("a problem");
    // Nominal ranges: warnings, as for any param.
    r.station.set_value_at_time(64.0, 0.0);
    let p = last();
    assert_eq!(p.kind, ProblemKind::Warning);
    assert!(
        p.message
            .contains("n1.station.setValueAtTime 64 outside the nominal range [-1, 63]"),
        "{}",
        p.message
    );
    r.wall_sec.set_value(90000.0);
    assert!(last().message.contains("[0, 86400]"));
    r.energy.set_value(1.5);
    assert!(last().message.contains("[0, 1]"));
    let n = ctx.problems().len();
    r.station.set_value(-1.0);
    r.wall_day.set_value(20543.0);
    r.tune.set_value(12.0);
    assert_eq!(ctx.problems().len(), n, "in range");
    // Non-finite values and negative times throw.
    r.tune.set_value_at_time(f64::NAN, 0.0);
    assert_eq!(last().kind, ProblemKind::Throw(ErrorName::TypeError));
    r.tune.set_value_at_time(1.0, -1.0);
    assert_eq!(last().kind, ProblemKind::Throw(ErrorName::RangeError));
    // A source: no input to connect to, but its params take a connection.
    let g = ctx.create_gain();
    assert_eq!(g.connect(&r).unwrap_err().name, ErrorName::IndexSizeError);
    g.connect(&r.energy).unwrap();
}

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
mod native {
    use mp_audio::wa::native::offline_context;

    const SR: f32 = 48000.0;

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / x.len() as f64).sqrt()
    }

    /// `secs` of the radio through an offline context, tuned (or not) from
    /// the start.
    fn render(station: f64, secs: f64) -> Vec<Vec<f32>> {
        let ctx = offline_context(2, (SR * secs as f32) as usize, SR);
        ctx.set_strict(true);
        let ready = ctx.prepare_radio();
        ctx.settle();
        assert_eq!(ready.result(), Some(Ok(true)));
        let r = ctx.create_radio();
        r.station.set_value_at_time(station, 0.0);
        r.wall_day.set_value_at_time(20543.0, 0.0);
        r.wall_sec.set_value_at_time(84800.0, 0.0);
        r.tune.set_value_at_time(1.0, 0.0);
        r.connect(&ctx.destination()).unwrap();
        ctx.start_rendering().expect("an offline render")
    }

    #[test]
    fn it_renders_the_station() {
        let out = render(0.0, 4.0);
        assert_eq!(out.len(), 2);
        assert!(out.iter().flatten().all(|v| v.is_finite()));
        // Past the tuner's sweep the station is up.
        let from = (SR * 2.5) as usize;
        let (l, r) = (&out[0][from..], &out[1][from..]);
        assert!(rms(l) > 0.005 && rms(r) > 0.005, "{} {}", rms(l), rms(r));
    }

    #[test]
    fn off_it_is_silent() {
        let out = render(-1.0, 1.0);
        for c in &out {
            let peak = c.iter().fold(0f32, |m, v| m.max(v.abs()));
            assert!(peak < 1e-6, "{peak}");
        }
    }
}
