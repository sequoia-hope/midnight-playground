//! `test/unit/pursuit-audio.test.js` (roadmap WP 5.7, L1): the Hot Pursuit
//! sounds' pure parts. The siren patterns span the right pitches, doppler
//! raises the pitch of a closing unit and lowers a receding one, the siren
//! level falls with distance to silence at 350 m, and every pursuit method
//! is a no-op before the audio has started. (The sound itself is measured
//! by the L4 renders: `voice-siren-*`, `shot-*`.)

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

use mp_audio::game::{GameAudio, Platform, SirenUnit, siren_doppler, siren_level, siren_pattern};

#[test]
fn siren_patterns_wail_and_yelp_sweep_650_1450_hz_hi_lo_alternates_960_770_hz() {
    let wail = siren_pattern("wail").unwrap();
    let yelp = siren_pattern("yelp").unwrap();
    let hilo = siren_pattern("hilo").unwrap();
    for p in [wail, yelp] {
        assert_eq!((p.lo, p.hi, p.shape), (650.0, 1450.0, "tri"));
    }
    assert_eq!(wail.period / 2.0, 1.7, "wail takes 1.7 s each way");
    assert!(
        yelp.period < wail.period / 4.0,
        "yelp is much faster than wail"
    );
    assert_eq!(
        (hilo.lo, hilo.hi, hilo.shape, hilo.period / 2.0),
        (770.0, 960.0, "square", 0.5)
    );
}

#[test]
fn doppler_closing_raises_the_pitch_receding_lowers_it_standing_still_leaves_it() {
    assert_eq!(siren_doppler(Some(0.0)), 1.0);
    assert!((siren_doppler(Some(30.0)) - 343.0 / 313.0).abs() < 1e-9);
    assert!((siren_doppler(Some(-30.0)) - 343.0 / 373.0).abs() < 1e-9);
    assert!(
        siren_doppler(Some(1e6)) < 2.0 && siren_doppler(Some(-1e6)) > 0.5,
        "clamped for silly speeds"
    );
    assert_eq!(siren_doppler(None), 1.0);
}

#[test]
fn siren_level_falls_with_distance_and_is_silent_from_350_m() {
    let d = [0.0, 5.0, 20.0, 50.0, 100.0, 200.0, 300.0, 349.0];
    for i in 1..d.len() {
        assert!(
            siren_level(Some(d[i])) < siren_level(Some(d[i - 1])),
            "{} m quieter than {} m",
            d[i],
            d[i - 1]
        );
    }
    assert!(
        siren_level(Some(0.0)) > 0.2 && siren_level(Some(0.0)) < 0.5,
        "loud right behind, not painful"
    );
    assert!(
        siren_level(Some(100.0)) > 0.02,
        "still clearly audible at 100 m"
    );
    assert_eq!(siren_level(Some(350.0)), 0.0);
    assert_eq!(siren_level(Some(1000.0)), 0.0);
}

#[test]
fn pursuit_methods_are_no_ops_before_init() {
    let mut a = GameAudio::new(Platform::headless(1));
    assert!(!a.ready());
    a.set_sirens(&[SirenUnit {
        id: None,
        dist: Some(5.0),
        pan: Some(0.0),
        rel_speed: Some(10.0),
        mode: Some("wail".into()),
    }]);
    a.set_sirens(&[]);
    a.siren_horn(0.5);
    a.radio(2.0, -0.5, None);
    a.radio_line(&["Suspect in custody.".to_string()], 0.0);
    a.busted();
    a.escaped();
    a.takedown(1.0, 0.0);
    a.spike_pop(0.0);
    a.wrecked();
    a.set_spiked_tyres(true, 30.0);
    a.set_pursuit_mood("cooldown");
    a.set_damage(0.9);
    assert!(a.ctx().is_none());
}
