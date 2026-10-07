//! The exhaust node (`create_exhaust`): `mp_exhaust`'s engine behind the
//! facade. On the null backend it is logged and validated like any node; on
//! the native backend it renders what the engine renders run directly.
//!
//!   cargo test -p mp_audio --test exhaust_node                    (null)
//!   cargo test -p mp_audio --features native --test exhaust_node  (both)

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
    let ready = ctx.prepare_exhaust();
    assert!(!ready.is_settled(), "settles as a promise would");
    ctx.settle();
    assert_eq!(ready.result(), Some(Ok(true)));

    let ex = ctx.create_exhaust("audiV8");
    assert_eq!(ex.kind(), NodeKind::Exhaust);
    // Its preset param starts at the preset it was made with.
    assert_eq!(ex.preset.value(), 5.0);
    assert_eq!(ex.rpm.value(), 800.0);
    assert_eq!(ex.running.value(), 1.0);
    ex.rpm.set_value_at_time(3000.0, 0.0);
    ex.throttle.set_value_at_time(0.6, 0.0);
    ex.boost.set_value_at_time(0.25, 0.0);
    ex.speed.set_value_at_time(-3.0, 0.0);
    ex.running.set_value_at_time(0.0, 1.0);
    ex.preset.set_value_at_time(2.0, 1.5);
    ex.connect(&ctx.destination()).unwrap();
    h.set_now(2.0);
    assert_eq!(ex.preset.value(), 2.0);
    let log = h.log();
    let want = [
        r#"[0,"context",{"sampleRate":48000}]"#,
        r#"[0,"new","n0","Destination"]"#,
        r#"[0,"new","n1","Exhaust",5]"#,
        r#"[0,"get","n1.preset",5]"#,
        r#"[0,"get","n1.rpm",800]"#,
        r#"[0,"get","n1.running",1]"#,
        r#"[0,"setValue","n1.rpm",3000,0]"#,
        r#"[0,"setValue","n1.throttle",0.6,0]"#,
        r#"[0,"setValue","n1.boost",0.25,0]"#,
        r#"[0,"setValue","n1.speed",-3,0]"#,
        r#"[0,"setValue","n1.running",0,1]"#,
        r#"[0,"setValue","n1.preset",2,1.5]"#,
        r#"[0,"connect","n1","n0"]"#,
        r#"[2,"get","n1.preset",2]"#,
    ];
    assert_eq!(log, want);
    assert!(ctx.problems().is_empty());
}

#[test]
fn its_params_and_connections_are_validated() {
    let ctx = null::context(NullOptions::default(), ContextOptions::default()).0;
    let ex = ctx.create_exhaust("v12");
    let last = || ctx.problems().pop().expect("a problem");
    // Nominal ranges: warnings, as for any param.
    ex.throttle.set_value_at_time(1.5, 0.0);
    let p = last();
    assert_eq!(p.kind, ProblemKind::Warning);
    assert!(
        p.message
            .contains("n1.throttle.setValueAtTime 1.5 outside the nominal range [0, 1]"),
        "{}",
        p.message
    );
    ex.rpm.set_value(31000.0);
    assert!(last().message.contains("[0, 30000]"));
    ex.preset.set_value(8.0);
    assert!(
        last()
            .message
            .contains("n1.preset.value 8 outside the nominal range [0, 7]")
    );
    let n = ctx.problems().len();
    ex.speed.set_value(-80.0);
    ex.boost.set_value(1.0);
    assert_eq!(ctx.problems().len(), n, "in range");
    // Non-finite values and negative times throw.
    ex.rpm.set_value_at_time(f64::NAN, 0.0);
    assert_eq!(last().kind, ProblemKind::Throw(ErrorName::TypeError));
    ex.rpm.set_value_at_time(1000.0, -1.0);
    assert_eq!(last().kind, ProblemKind::Throw(ErrorName::RangeError));
    // A source: no input to connect to.
    let g = ctx.create_gain();
    assert_eq!(g.connect(&ex).unwrap_err().name, ErrorName::IndexSizeError);
    g.connect(&ex.rpm).unwrap();
    // An unknown preset makes the first one, with a warning.
    let n = ctx.problems().len();
    let other = ctx.create_exhaust("nope");
    assert_eq!(other.preset.value(), 0.0);
    assert_eq!(ctx.problems().len(), n + 1);
    let p = last();
    assert_eq!(p.kind, ProblemKind::Warning);
    assert!(
        p.message.contains("createExhaust(\"nope\")"),
        "{}",
        p.message
    );
}

#[test]
#[should_panic(expected = "createExhaust(\"V12\")")]
fn strict_mode_panics_on_an_unknown_preset() {
    let ctx = null::context(NullOptions::default(), ContextOptions::default()).0;
    ctx.set_strict(true);
    ctx.create_exhaust("V12");
}

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
mod native {
    use mp_audio::wa::native::offline_context;
    use mp_exhaust::{Engine, ORDER, State, preset};

    const SR: f32 = 48000.0;
    const BLOCK: usize = 128;

    /// Equal sample for sample (the first difference, if not).
    fn same(a: &[f32], b: &[f32], what: &str) {
        assert_eq!(a.len(), b.len(), "{what}: lengths");
        if let Some(i) = (0..a.len()).find(|&i| a[i].to_bits() != b[i].to_bits()) {
            panic!("{what}: first difference at {i}: {} vs {}", a[i], b[i]);
        }
    }

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / x.len() as f64).sqrt()
    }

    /// One second of preset `key` at 3000 rpm and 60 % throttle, `running`
    /// from the start, through an offline context.
    fn render(key: &str, running: f64) -> Vec<Vec<f32>> {
        let ctx = offline_context(2, SR as usize, SR);
        ctx.set_strict(true);
        let ready = ctx.prepare_exhaust();
        ctx.settle();
        assert_eq!(ready.result(), Some(Ok(true)));
        let ex = ctx.create_exhaust(key);
        ex.rpm.set_value_at_time(3000.0, 0.0);
        ex.throttle.set_value_at_time(0.6, 0.0);
        ex.running.set_value_at_time(running, 0.0);
        ex.connect(&ctx.destination()).unwrap();
        ctx.start_rendering().expect("an offline render")
    }

    /// The engine run directly, as the processor runs it: built idling,
    /// then the params' values as targets at every block.
    fn direct(key: &str, blocks: usize) -> (Vec<f32>, Vec<f32>) {
        let p = preset(key).unwrap();
        let idle = p.idle;
        let mut e = Engine::new(
            p,
            State {
                rpm: idle,
                ..State::default()
            },
            12345,
            SR as f64,
        );
        let (mut l, mut r) = (vec![0.0; blocks * BLOCK], vec![0.0; blocks * BLOCK]);
        for b in 0..blocks {
            e.set_state(
                State {
                    rpm: 3000.0,
                    // The param holds a float32.
                    throttle: 0.6f32 as f64,
                    boost: 0.0,
                    speed: 0.0,
                },
                false,
            );
            e.set_running(true);
            let s = b * BLOCK..(b + 1) * BLOCK;
            e.process(&mut l[s.clone()], &mut r[s]);
        }
        (l, r)
    }

    #[test]
    fn it_renders_the_engine() {
        let out = render("audiV8", 1.0);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].len(), SR as usize);
        assert!(out.iter().flatten().all(|v| v.is_finite()));
        let (l, r) = (&out[0], &out[1]);
        assert!(rms(l) > 0.01 && rms(r) > 0.01, "{} {}", rms(l), rms(r));
        assert!(l != r, "stereo");
        // Sample for sample what the engine renders run directly.
        let (dl, dr) = direct("audiV8", SR as usize / BLOCK);
        let n = dl.len();
        same(&l[..n], &dl, "left");
        same(&r[..n], &dr, "right");
    }

    #[test]
    fn not_running_it_is_silent() {
        let out = render("audiV8", 0.0);
        // A new engine starts faded out: told not to run from the first
        // block, it never fades in.
        for c in &out {
            let peak = c.iter().fold(0f32, |m, v| m.max(v.abs()));
            assert!(peak < 1e-6, "{peak}");
        }
    }

    #[test]
    fn a_preset_change_rebuilds_the_engine() {
        let ctx = offline_context(2, SR as usize / 2, SR);
        ctx.set_strict(true);
        let ex = ctx.create_exhaust("crossV8");
        ex.rpm.set_value_at_time(2500.0, 0.0);
        ex.throttle.set_value_at_time(0.5, 0.0);
        // On a block boundary (k-rate params change only there).
        let at = (96 * BLOCK) as f64 / SR as f64;
        let i = ORDER.iter().position(|k| *k == "i4turbo").unwrap();
        ex.preset.set_value_at_time(i as f64, at);
        ex.connect(&ctx.destination()).unwrap();
        let out = ctx.start_rendering().unwrap();
        let k = 96 * BLOCK;
        // From that block on: a new i4 engine started at the current state.
        let p = preset("i4turbo").unwrap();
        let s = State {
            rpm: 2500.0,
            throttle: 0.5,
            boost: 0.0,
            speed: 0.0,
        };
        let mut e = Engine::new(p, s, 12345, SR as f64);
        let (mut l, mut r) = (vec![0.0; BLOCK * 4], vec![0.0; BLOCK * 4]);
        for b in 0..4 {
            e.set_state(s, false);
            e.set_running(true);
            let sl = b * BLOCK..(b + 1) * BLOCK;
            e.process(&mut l[sl.clone()], &mut r[sl]);
        }
        same(&out[0][k..k + 4 * BLOCK], &l, "left");
        same(&out[1][k..k + 4 * BLOCK], &r, "right");
        assert!(rms(&out[0][..k]) > 0.0 && rms(&out[0][k..]) > 0.0);
    }
}
