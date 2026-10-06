//! The engine's characters and wavetables (`Audio.js` `CARS`,
//! `engineCycle`, `rumbleCycle`).
//!
//! The engine is a wavetable: one full 720° four-stroke cycle is drawn with a
//! pressure pulse per cylinder — timed and weighted by the engine's bank
//! layout, which is where a cross-plane V8's lumpy burble comes from — and
//! turned into a band-limited PeriodicWave played at the cycle rate
//! (rpm / 120). On- and off-throttle cycles are separate waves crossfaded by
//! load, run through exhaust formant filters, and doubled on a second,
//! slightly detuned chain for stereo width. A separate rumble layer carries
//! the low end (see [`rumble_cycle`]).
//!
//! Pure math: the coefficient arrays are bit-identical to the JS reference
//! (every inexact function through `mp_math::kernel`, every `Float32Array`
//! store rounded to `f32`).

use mp_math::Mulberry32;
use mp_math::kernel::{atan2, cos, exp, pow, sin};

/// A car's engine character (`CARS[kind]`).
///
/// `banks`: which exhaust bank each firing event in the cycle belongs to
/// (firing order mapped to banks). Cross-plane V8 (1-8-4-3-6-5-7-2) fires
/// L R R L R L L R — two same-bank pulses in a row — which is the burble.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CarProfile {
    pub key: &'static str,
    /// No combustion: the engine chains fall silent and the motor layers
    /// (`_buildElectric`) take over. Only `cyl`, `pops` and `pop_freq` are
    /// meaningful then (they feed the pops code); the rest are zero.
    pub electric: bool,
    pub cyl: u32,
    pub banks: &'static [usize],
    pub bank_amp: [f64; 2],
    pub bank_delay: [f64; 2],
    pub jitter: f64,
    pub amp_var: f64,
    pub pulse_w: f64,
    pub sharp: f64,
    pub low_boost: f64,
    pub rumble: f64,
    /// `[freq, gain dB, Q]` per formant.
    pub formants: [[f64; 3]; 3],
    pub lp_base: f64,
    pub lp_range: f64,
    pub drive: f64,
    pub trim: f64,
    pub pops: f64,
    pub pop_freq: [f64; 2],
    pub intake: f64,
    pub rasp: f64,
    pub whine: f64,
    /// `turbo: 1` on the rally car; 0 (absent) elsewhere.
    pub turbo: f64,
}

const NONE: CarProfile = CarProfile {
    key: "",
    electric: false,
    cyl: 0,
    banks: &[],
    bank_amp: [0.0; 2],
    bank_delay: [0.0; 2],
    jitter: 0.0,
    amp_var: 0.0,
    pulse_w: 0.0,
    sharp: 0.0,
    low_boost: 0.0,
    rumble: 0.0,
    formants: [[0.0; 3]; 3],
    lp_base: 0.0,
    lp_range: 0.0,
    drive: 0.0,
    trim: 0.0,
    pops: 0.0,
    pop_freq: [0.0; 2],
    intake: 0.0,
    rasp: 0.0,
    whine: 0.0,
    turbo: 0.0,
};

/// flat-plane V8: banks alternate evenly, crisp and howly
pub const SPORTS: CarProfile = CarProfile {
    key: "sports",
    cyl: 8,
    banks: &[0, 1, 0, 1, 0, 1, 0, 1],
    bank_amp: [1.0, 0.9],
    bank_delay: [0.0, 0.004],
    jitter: 0.03,
    amp_var: 0.12,
    pulse_w: 0.34,
    sharp: 1.8,
    low_boost: 1.0,
    rumble: 1.0,
    formants: [[190.0, 6.0, 1.3], [640.0, 4.0, 1.8], [1850.0, 4.0, 2.4]],
    lp_base: 520.0,
    lp_range: 5200.0,
    drive: 1.5,
    trim: 1.1,
    pops: 1.0,
    pop_freq: [650.0, 1700.0],
    intake: 0.9,
    rasp: 0.9,
    whine: 1.0,
    ..NONE
};

/// big cross-plane V8: deep, lumpy, loud pops
pub const MUSCLE: CarProfile = CarProfile {
    key: "muscle",
    cyl: 8,
    banks: &[0, 1, 1, 0, 1, 0, 0, 1],
    bank_amp: [1.0, 0.72],
    bank_delay: [0.0, 0.014],
    jitter: 0.07,
    amp_var: 0.22,
    pulse_w: 0.46,
    sharp: 1.4,
    low_boost: 1.7,
    rumble: 1.1,
    formants: [[105.0, 7.0, 1.1], [360.0, 5.0, 1.6], [920.0, 3.0, 2.2]],
    lp_base: 360.0,
    lp_range: 3300.0,
    drive: 2.1,
    trim: 0.9,
    pops: 1.7,
    pop_freq: [380.0, 1100.0],
    intake: 1.1,
    rasp: 0.7,
    whine: 1.3,
    ..NONE
};

/// V10: high, hard-edged scream
pub const SUPER: CarProfile = CarProfile {
    key: "super",
    cyl: 10,
    banks: &[0, 1, 0, 1, 0, 1, 0, 1, 0, 1],
    bank_amp: [1.0, 0.94],
    bank_delay: [0.0, 0.003],
    jitter: 0.025,
    amp_var: 0.09,
    pulse_w: 0.28,
    sharp: 2.2,
    low_boost: 0.8,
    rumble: 0.75,
    formants: [[300.0, 4.0, 1.4], [1150.0, 5.0, 2.0], [3100.0, 5.0, 2.6]],
    lp_base: 900.0,
    lp_range: 7800.0,
    drive: 1.35,
    trim: 1.2,
    pops: 0.6,
    pop_freq: [1100.0, 2600.0],
    intake: 1.2,
    rasp: 1.2,
    whine: 0.8,
    ..NONE
};

/// turbo inline-4: even, buzzy and raspy, anti-lag crackle, turbo whistle
pub const RALLY: CarProfile = CarProfile {
    key: "rally",
    cyl: 4,
    banks: &[0, 0, 0, 0],
    bank_amp: [1.0, 1.0],
    bank_delay: [0.0, 0.0],
    jitter: 0.045,
    amp_var: 0.16,
    pulse_w: 0.4,
    sharp: 1.7,
    low_boost: 1.15,
    rumble: 0.7,
    formants: [[170.0, 5.0, 1.2], [560.0, 5.0, 1.7], [1650.0, 5.0, 2.3]],
    lp_base: 640.0,
    lp_range: 5600.0,
    drive: 2.0,
    trim: 1.05,
    pops: 2.2,
    pop_freq: [800.0, 2300.0],
    intake: 1.3,
    rasp: 1.35,
    whine: 0.9,
    turbo: 1.0,
    ..NONE
};

/// No combustion: the engine chains fall silent and the motor layers
/// (_buildElectric) take over. The fields below only feed the pops code.
pub const ELECTRIC: CarProfile = CarProfile {
    key: "electric",
    electric: true,
    cyl: 8,
    pops: 0.0,
    pop_freq: [800.0, 2000.0],
    ..NONE
};

/// `CARS`, in the JS object's key order.
pub const CARS: [&CarProfile; 5] = [&SPORTS, &MUSCLE, &SUPER, &RALLY, &ELECTRIC];

/// `CARS[kind]`.
pub fn car(kind: &str) -> Option<&'static CarProfile> {
    CARS.iter().copied().find(|c| c.key == kind)
}

pub const WAVE_SAMPLES: usize = 2048;
pub const WAVE_HARMONICS: usize = 220;

/// A PeriodicWave's coefficients, as the JS hands them to
/// `createPeriodicWave` (`Float32Array`s).
#[derive(Clone, Debug, PartialEq)]
pub struct Fourier {
    pub real: Vec<f32>,
    pub imag: Vec<f32>,
}

/// The `COS` and `SIN` tables (`Float32Array`s, built once in the JS).
struct Tables {
    cos: Vec<f32>,
    sin: Vec<f32>,
}

fn tables() -> &'static Tables {
    static T: std::sync::OnceLock<Tables> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let m = WAVE_SAMPLES;
        let mut cos_t = vec![0f32; m];
        let mut sin_t = vec![0f32; m];
        for i in 0..m {
            let a = (2.0 * std::f64::consts::PI * i as f64) / m as f64;
            cos_t[i] = cos(a) as f32;
            sin_t[i] = sin(a) as f32;
        }
        Tables {
            cos: cos_t,
            sin: sin_t,
        }
    })
}

/// Draw one engine cycle and return its Fourier series (`engineCycle`).
pub fn engine_cycle(prof: &CarProfile, seed: u32, off: bool) -> Fourier {
    let m = WAVE_SAMPLES;
    let tb = tables();
    let mut rng = Mulberry32::new(seed);
    let mut x = vec![0f32; m];
    let n = prof.cyl as f64;
    let w = (prof.pulse_w / n) * (if off { 1.7 } else { 1.0 });
    let sharp = if off { 0.9 } else { prof.sharp };
    let amp_var = prof.amp_var * (if off { 2.2 } else { 1.0 });
    for k in 0..prof.cyl as usize {
        let bank = prof.banks[k];
        let t0 = k as f64 / n + prof.bank_delay[bank] + (rng.next_f64() - 0.5) * prof.jitter / n;
        let amp = prof.bank_amp[bank]
            * (1.0 + (rng.next_f64() - 0.5) * amp_var)
            * (if off { 0.6 } else { 1.0 });
        for (i, xi) in x.iter_mut().enumerate() {
            let mut tau = i as f64 / m as f64 - t0;
            tau -= tau.floor();
            let u = tau / w;
            if u > 7.0 {
                continue;
            }
            // Gamma-like blowdown pulse, then a small reflected rarefaction.
            let p = pow(u * exp(1.0 - u), sharp);
            let refl = if u > 1.2 {
                -0.22 * exp(-(u - 2.6) * (u - 2.6))
            } else {
                0.0
            };
            *xi = (*xi as f64 + amp * (p + refl)) as f32;
        }
    }
    if off {
        // Overrun: irregular soft burbles between pulses.
        for _b in 0..prof.cyl {
            let t0 = rng.next_f64();
            let a = 0.15 + rng.next_f64() * 0.25;
            let bw = w * (0.6 + rng.next_f64());
            for (i, xi) in x.iter_mut().enumerate() {
                let mut tau = i as f64 / m as f64 - t0;
                tau -= tau.floor();
                let u = tau / bw;
                if u < 5.0 {
                    *xi = (*xi as f64 + a * u * exp(1.0 - u)) as f32;
                }
            }
        }
    }
    let hh = WAVE_HARMONICS;
    let mut real = vec![0f32; hh + 1];
    let mut imag = vec![0f32; hh + 1];
    for h in 1..=hh {
        let (mut re, mut im, mut idx) = (0.0, 0.0, 0usize);
        for &xi in x.iter() {
            re += xi as f64 * tb.cos[idx] as f64;
            im += xi as f64 * tb.sin[idx] as f64;
            idx += h;
            if idx >= m {
                idx -= m;
            }
        }
        let mut g = 2.0 / m as f64;
        // Orders below the firing frequency carry the burble; the very lowest
        // (below half-order) are mostly sub-bass mud, so taper them off.
        let hf = h as f64;
        if hf < n {
            g *= prof.low_boost;
        }
        if hf < n / 2.0 {
            g *= pow(hf / (n / 2.0), 1.5);
        }
        if off {
            g *= exp(-hf / 70.0);
        }
        real[h] = (re * g) as f32;
        imag[h] = (im * g) as f32;
    }
    Fourier { real, imag }
}

/// The low end, drawn separately (`rumbleCycle`): every order up to twice
/// the firing order, tilted toward the bottom, with extra weight on the half
/// and full firing orders, and the engine's own phases so it lines up with
/// the exhaust pulses. Through a fixed low-pass that leaves the sub-firing
/// lope at racing revs and the firing thump near idle.
pub fn rumble_cycle(prof: &CarProfile, cycle: &Fourier) -> Fourier {
    let n = prof.cyl as usize;
    let hh = 2 * n;
    let mut re = vec![0f32; hh + 1];
    let mut im = vec![0f32; hh + 1];
    for h in 1..=hh {
        // order 1: the deepest, felt more than heard
        let mut w = if h == 1 { 0.9 } else { 1.0 / (h as f64).sqrt() };
        // (n is even for every car, so `h === n / 2` is an integer test.)
        if 2 * h == n || h == n {
            w *= 1.6;
        }
        if (h as f64) < n as f64 / 2.0 {
            w *= prof.low_boost; // cross-plane lope
        }
        let ph = atan2(cycle.imag[h] as f64, cycle.real[h] as f64);
        re[h] = (w * cos(ph)) as f32;
        im[h] = (w * sin(ph)) as f32;
    }
    Fourier { real: re, imag: im }
}

/// The five waves `setCar` builds and caches for a combustion car, in the
/// order it computes them: the rumble's source cycle first (seed 11, not a
/// wave itself), then `onL`, `offL`, `onR`, `offR`, `rumble`.
#[derive(Clone, Debug, PartialEq)]
pub struct CarWaves {
    pub on_l: Fourier,
    pub off_l: Fourier,
    pub on_r: Fourier,
    pub off_r: Fourier,
    pub rumble: Fourier,
}

/// `setCar`'s wave set for a combustion car (`None` for the electric car,
/// which returns before building any).
pub fn car_waves(prof: &CarProfile) -> Option<CarWaves> {
    if prof.electric {
        return None;
    }
    let rum = rumble_cycle(prof, &engine_cycle(prof, 11, false));
    Some(CarWaves {
        on_l: engine_cycle(prof, 11, false),
        off_l: engine_cycle(prof, 12, true),
        on_r: engine_cycle(prof, 21, false),
        off_r: engine_cycle(prof, 22, true),
        rumble: rum,
    })
}

/// Rivals are all combustion cars, whatever the player drives: the sports
/// car's seed-21 on-throttle cycle (`_rivalWave`).
pub fn rival_wave() -> Fourier {
    engine_cycle(&SPORTS, 21, false)
}
