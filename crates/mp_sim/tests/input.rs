//! `mp_sim::input`: the quantised controls (`InputFrame`, `quantiseInput` in
//! `src/parity/sim.js`). Out-of-range controls clamp, halves round as JS's
//! `Math.round` does (up), every frame survives a round trip through the
//! values physics reads, and the flag bits are distinct and kept apart from
//! the controls.

use mp_sim::input::{
    ANALOG, AUTOPILOT, AWAY, HANDBRAKE, Input, InputFrame, NITRO, RESET, SHIFT_DOWN, SHIFT_UP,
    quantise,
};

fn inp(steer: f64, throttle: f64, brake: f64) -> Input {
    Input {
        steer,
        throttle,
        brake,
        ..Input::default()
    }
}

#[test]
fn out_of_range_controls_clamp_to_the_ends() {
    let cases: [(f64, f64, f64, i16, u8, u8); 6] = [
        (2.0, 1.5, 7.0, 32767, 255, 255),
        (-5.0, -1.0, -0.1, -32767, 0, 0),
        (1.0, 1.0, 1.0, 32767, 255, 255),
        (-1.0, 0.0, 0.0, -32767, 0, 0),
        (f64::INFINITY, f64::INFINITY, f64::INFINITY, 32767, 255, 255),
        (
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
            -32767,
            0,
            0,
        ),
    ];
    for (s, t, b, qs, qt, qb) in cases {
        let f = InputFrame::quantise(&inp(s, t, b));
        assert_eq!((f.steer, f.throttle, f.brake), (qs, qt, qb), "{s} {t} {b}");
    }
}

/// `Math.round` rounds halves up (towards +infinity), so a negative half
/// goes towards zero; Rust's `f64::round` would go away from it.
#[test]
fn halves_round_up_as_in_js() {
    // 0.5 * 255 = 127.5 -> 128.
    assert_eq!(InputFrame::quantise(&inp(0.0, 0.5, 0.5)).throttle, 128);
    assert_eq!(InputFrame::quantise(&inp(0.0, 0.5, 0.5)).brake, 128);
    // +-0.5 * 32767 = +-16383.5 -> 16384 and -16383.
    assert_eq!(InputFrame::quantise(&inp(0.5, 0.0, 0.0)).steer, 16384);
    assert_eq!(InputFrame::quantise(&inp(-0.5, 0.0, 0.0)).steer, -16383);
    // -0 steers nowhere.
    assert_eq!(InputFrame::quantise(&inp(-0.0, 0.0, 0.0)).steer, 0);
}

#[test]
fn every_frame_survives_a_round_trip_through_its_input() {
    for steer in -32767..=32767i16 {
        let f = InputFrame {
            steer,
            ..InputFrame::default()
        };
        assert_eq!(InputFrame::quantise(&f.input()), f, "steer {steer}");
    }
    for x in 0..=255u8 {
        let f = InputFrame {
            throttle: x,
            brake: 255 - x,
            flags: HANDBRAKE | NITRO | ANALOG,
            ..InputFrame::default()
        };
        assert_eq!(InputFrame::quantise(&f.input()), f, "throttle {x}");
    }
}

#[test]
fn quantising_twice_changes_nothing_and_stays_within_half_a_step() {
    // A spread of values, including some just off the steps.
    let mut x = 0.123_456_789_f64;
    for _ in 0..2000 {
        x = (x * 997.0 + 0.317).fract();
        let i = Input {
            steer: x * 2.0 - 1.0,
            throttle: x,
            brake: 1.0 - x,
            handbrake: x > 0.5,
            nitro: x < 0.3,
            analog: x > 0.8,
            cruise: x > 0.9,
        };
        let q = quantise(&i);
        assert_eq!(quantise(&q), q);
        assert!((q.steer - i.steer).abs() <= 0.5 / 32767.0 + 1e-15);
        assert!((q.throttle - i.throttle).abs() <= 0.5 / 255.0 + 1e-15);
        assert!((q.brake - i.brake).abs() <= 0.5 / 255.0 + 1e-15);
        assert_eq!(
            (q.handbrake, q.nitro, q.analog, q.cruise),
            (i.handbrake, i.nitro, i.analog, i.cruise)
        );
    }
}

#[test]
fn the_flag_bits_are_distinct_single_bits() {
    let flags = [
        HANDBRAKE, NITRO, ANALOG, RESET, AUTOPILOT, AWAY, SHIFT_UP, SHIFT_DOWN,
    ];
    let mut all = 0u8;
    for f in flags {
        assert_eq!(f.count_ones(), 1, "{f}");
        assert_eq!(all & f, 0, "{f} overlaps another flag");
        all |= f;
    }
}

/// The edge and multiplayer flags are the network's: physics never sees
/// them, and quantising the controls never sets them.
#[test]
fn reset_autopilot_and_away_are_not_controls() {
    let base = InputFrame {
        steer: -1234,
        throttle: 200,
        brake: 3,
        flags: NITRO,
        clutch: 0,
    };
    // The sim car's controls (the clutch, the shifts) are not the arcade's
    // either.
    let sim = SHIFT_UP | SHIFT_DOWN;
    for extra in [RESET, AUTOPILOT, AWAY, RESET | AUTOPILOT | AWAY, sim] {
        let f = InputFrame {
            flags: base.flags | extra,
            ..base
        };
        assert_eq!(f.input(), base.input(), "flags {extra}");
    }
    let pressed = InputFrame {
        clutch: 255,
        ..base
    };
    assert_eq!(pressed.input(), base.input());
    assert_eq!(pressed.clutch(), 1.0);
    let every = Input {
        steer: 1.0,
        throttle: 1.0,
        brake: 1.0,
        handbrake: true,
        nitro: true,
        analog: true,
        cruise: true,
    };
    assert_eq!(
        InputFrame::quantise(&every).flags,
        HANDBRAKE | NITRO | ANALOG
    );
}

#[test]
fn the_default_frame_is_hands_off() {
    let i = InputFrame::default().input();
    assert_eq!(i, Input::default());
    assert_eq!(
        InputFrame::quantise(&Input::default()),
        InputFrame::default()
    );
}
