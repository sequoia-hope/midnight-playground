//! The player's engine on the physical exhaust model (DECISIONS D1110), in
//! a whole `GameAudio` rendered offline on the native backend:
//!
//!   cargo test -p mp_audio --features native --test exhaust_game
//!
//! Off (the default), the game is the JS game's: no exhaust node exists. On,
//! each combustion car sounds about as loud as its wavetable engine did, so
//! the mix around it (music, tyres, wind) still fits; the electric car is
//! untouched; a car change moves the one node's preset; engine off fades it.

#![cfg(all(feature = "native", not(target_arch = "wasm32")))]

use mp_audio::game::{CarState, GameAudio, InitOptions, Platform, Volume};
use mp_audio::wa::native;
use std::cell::RefCell;
use std::rc::Rc;

const SR: f64 = 48000.0;
const FRAME: usize = 384;

fn state(rpm: f64, throttle: f64) -> CarState {
    CarState {
        rpm: Some(rpm),
        rpm_max: Some(7800.0),
        throttle: Some(throttle),
        speed: Some(0.0),
        on_ground: Some(true),
        gear: Some(0.0),
        boost: Some(0.0),
        ..Default::default()
    }
}

/// `secs` of the game's sound for `car`, steered every 8 ms with
/// `control(t)`'s state, the model on or off. Returns the left channel and
/// the GameAudio.
fn render(
    car: &str,
    model: bool,
    secs: f64,
    control: impl Fn(f64) -> CarState + 'static,
) -> (Vec<f32>, Rc<RefCell<GameAudio>>) {
    render_switching(car, model, secs, None, control)
}

/// [`render`], with the car changed to `switch.1` at `switch.0` s.
fn render_switching(
    car: &str,
    model: bool,
    secs: f64,
    switch: Option<(f64, &'static str)>,
    control: impl Fn(f64) -> CarState + 'static,
) -> (Vec<f32>, Rc<RefCell<GameAudio>>) {
    let frames = (secs * SR / FRAME as f64).round() as usize * FRAME;
    let ctx = native::offline_context(2, frames, SR as f32);
    let a = Rc::new(RefCell::new(GameAudio::new(Platform::headless(1))));
    {
        let mut a = a.borrow_mut();
        a.init(InitOptions {
            context: Some(ctx.clone()),
            latency_hint: None,
        });
        a.set_volume(Volume {
            master: Some(1.0),
            sfx: Some(0.85),
            music: Some(0.0),
        });
        a.set_car(car);
        a.set_engine_model(model);
    }
    let dt = FRAME as f64 / SR;
    let aa = a.clone();
    let step = move |k: usize| {
        let mut a = aa.borrow_mut();
        let t = k as f64 * dt;
        if let Some((at, to)) = switch
            && t >= at
            && t < at + dt
        {
            a.set_car(to);
        }
        a.settle();
        a.update(dt, &control(t));
    };
    step(0);
    let out = ctx
        .start_rendering_steered(FRAME, Box::new(step))
        .expect("an offline native context");
    assert!(ctx.problems().is_empty(), "{:?}", ctx.problems());
    (out[0].clone(), a)
}

fn db(x: &[f32]) -> f64 {
    let e: f64 = x.iter().map(|&v| v as f64 * v as f64).sum::<f64>() / x.len() as f64;
    10.0 * (e + 1e-20).log10()
}

#[test]
fn off_by_default_no_exhaust_node() {
    let (_, a) = render("muscle", false, 0.5, |_| state(3000.0, 0.5));
    assert!(!a.borrow().engine_model_active());
}

#[test]
fn each_car_sits_where_its_wavetable_engine_did() {
    let mut report = Vec::new();
    for car in ["sports", "muscle", "super", "rally"] {
        for (rpm, thr) in [(900.0, 0.0), (3500.0, 0.4), (6500.0, 1.0)] {
            let (old, _) = render(car, false, 1.5, move |_| state(rpm, thr));
            let (new, a) = render(car, true, 1.5, move |_| state(rpm, thr));
            assert!(a.borrow().engine_model_active(), "{car}: the model plays");
            let skip = (0.7 * SR) as usize;
            let (o, n) = (db(&old[skip..]), db(&new[skip..]));
            report.push(format!(
                "{car} {rpm} {thr}: classic {o:.1} dB, model {n:.1} dB ({:+.1})",
                n - o
            ));
        }
    }
    println!("{}", report.join("\n"));
    for line in &report {
        let d: f64 = line
            .rsplit('(')
            .next()
            .unwrap()
            .trim_end_matches(')')
            .parse()
            .unwrap();
        assert!(d.abs() <= 4.0, "{line}");
    }
}

#[test]
fn the_electric_car_keeps_its_motor() {
    let (_, a) = render("electric", true, 0.5, |_| CarState {
        motor: Some(0.4),
        ..state(3000.0, 0.5)
    });
    assert!(!a.borrow().engine_model_active());
}

#[test]
fn engine_off_fades_the_model_to_silence() {
    let (out, a) = render("sports", true, 2.5, |t| {
        if t < 0.8 {
            state(3000.0, 0.5)
        } else {
            state(0.0, 0.0)
        }
    });
    assert!(a.borrow().engine_model_active());
    let on = db(&out[(0.4 * SR) as usize..(0.8 * SR) as usize]);
    let off = db(&out[(2.0 * SR) as usize..]);
    assert!(on > -40.0, "playing: {on:.1} dB");
    assert!(off < on - 40.0, "faded: {off:.1} dB against {on:.1} dB");
}

#[test]
fn a_car_change_keeps_playing_on_the_new_preset() {
    // The flat-plane V8 at 3000 rpm, then the cross-plane V8: the same node
    // plays on, and the new engine fires at its own rate.
    let (out, a) = render_switching("sports", true, 2.0, Some((1.0, "muscle")), |_| {
        state(3000.0, 0.5)
    });
    assert!(a.borrow().engine_model_active());
    let before = db(&out[(0.5 * SR) as usize..(1.0 * SR) as usize]);
    let after = db(&out[(1.4 * SR) as usize..]);
    assert!(
        before > -40.0 && after > -40.0,
        "{before:.1} dB, then {after:.1} dB"
    );
    assert_eq!(
        a.borrow().exhaust_nodes(),
        1,
        "one node for the graph's life"
    );
}
