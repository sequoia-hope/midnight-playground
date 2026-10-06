//! Every array the JS audio builds, as the audio reference lists it
//! (`parity/golden/audio/arrays.json`, roadmap WP 0.7): a `GameAudio` built
//! and `setCar` called for `sports`, `muscle`, `super`, `rally` and
//! `electric` in turn. Buffers first, in creation order, then the periodic
//! waves and wave-shaper curves in the order the JS makes them; a wave or
//! curve with the same contents as an earlier one is listed once, with the
//! later names as aliases.
//!
//! The names are the reference's: the shortest property path from the
//! `GameAudio`, or, for an array only a local variable holds, the function
//! that made it (with the JS fake's node id).

use crate::engine::{self, CARS, Fourier};
use crate::{music, noise, samples, shapes};
use mp_math::Rng;

/// What an array is.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// An AudioBuffer: one part per channel.
    Buffer { sample_rate: f64 },
    /// A PeriodicWave: parts `real`, `imag`; the options as passed.
    Wave { disable_normalization: Option<bool> },
    /// A WaveShaper curve: one part.
    Curve,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RefArray {
    pub name: String,
    pub aliases: Vec<String>,
    pub kind: Kind,
    pub parts: Vec<Vec<f32>>,
}

/// The cars `setCar` is called for, in order.
pub const CAR_ORDER: [&str; 5] = ["sports", "muscle", "super", "rally", "electric"];

fn wave(name: String, f: Fourier, disable_normalization: Option<bool>) -> RefArray {
    RefArray {
        name,
        aliases: Vec::new(),
        kind: Kind::Wave {
            disable_normalization,
        },
        parts: vec![f.real, f.imag],
    }
}

fn curve(name: &str, c: Vec<f32>) -> RefArray {
    RefArray {
        name: name.into(),
        aliases: Vec::new(),
        kind: Kind::Curve,
        parts: vec![c],
    }
}

/// The reference compares arrays by hash, so by their bits (`-0` is not
/// `0`).
fn same_bits(a: &[Vec<f32>], b: &[Vec<f32>]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| p.to_bits() == q.to_bits())
        })
}

/// Build every array in the JS order. `random` is the audio's
/// `Math.random` (`mulberry32(1)` for the reference); the noise beds draw
/// from it first, then the tunnel's impulse response.
pub fn arrays(sample_rate: f64, random: &mut impl Rng) -> Vec<RefArray> {
    let sr = sample_rate;
    let buffer = |name: String, parts: Vec<Vec<f32>>| RefArray {
        name,
        aliases: Vec::new(),
        kind: Kind::Buffer { sample_rate: sr },
        parts,
    };
    let mut out = Vec::new();
    // _build(): _makeNoise, renderSfx, _buildTunnel, ..., music.build().
    let beds = noise::make_noise(sr, random);
    out.push(buffer("audio.noise.white".into(), vec![beds.white]));
    out.push(buffer("audio.noise.pink".into(), vec![beds.pink]));
    out.push(buffer("audio.noise.brown".into(), vec![beds.brown]));
    for (k, v) in samples::sfx_data(sr) {
        out.push(buffer(format!("audio.sfxBuf.{k}"), v));
    }
    out.push(buffer(
        "audio.tunnel.buffer".into(),
        noise::tunnel_ir(sr, random),
    ));
    for (k, v) in samples::kit_data(sr) {
        out.push(buffer(format!("audio.music.kit.{k}"), v));
    }
    out.push(buffer(
        "audio.music.reverb.buffer".into(),
        music::hall_ir(sr),
    ));

    // Waves and curves, in the order the JS makes them.
    let mut shaped = vec![
        curve("audio.eng.L.shaper.curve", shapes::exhaust_curve(2.2, 2048)),
        curve("audio.eng.R.shaper.curve", shapes::exhaust_curve(2.2, 2048)),
        curve(
            "_buildEngine(): n59.curve",
            shapes::distortion_curve(2.5, 1024),
        ),
        wave(
            "pulseWave()".into(),
            shapes::pulse_wave(24).wave,
            Some(true),
        ),
        wave(
            "softSquareWave()".into(),
            shapes::soft_square_wave(15),
            None,
        ),
        curve(
            "_buildPursuit(): n241.curve",
            shapes::distortion_curve(2.6, 1024),
        ),
        wave("audio.music.pulse".into(), music::pulse_wave(), None),
    ];
    // init() calls setCar('sports'); then each car in turn. The rival wave
    // is made on the first call; a car's waves on its first call.
    let mut built: Vec<&str> = Vec::new();
    let mut rival = false;
    for kind in ["sports"].into_iter().chain(CAR_ORDER) {
        if !rival {
            shaped.push(wave("audio._rivalWave".into(), engine::rival_wave(), None));
            rival = true;
        }
        let prof = CARS.iter().find(|c| c.key == kind).expect("a car");
        if built.contains(&kind) {
            continue;
        }
        if let Some(w) = engine::car_waves(prof) {
            built.push(kind);
            let p = format!("audio._waves.{kind}");
            shaped.push(wave(format!("{p}.onL"), w.on_l, None));
            shaped.push(wave(format!("{p}.offL"), w.off_l, None));
            shaped.push(wave(format!("{p}.onR"), w.on_r, None));
            shaped.push(wave(format!("{p}.offR"), w.off_r, None));
            shaped.push(wave(format!("{p}.rumble"), w.rumble, None));
        }
    }
    // Same contents under several names: listed once.
    let mut kept: Vec<RefArray> = Vec::new();
    for a in shaped {
        match kept.iter_mut().find(|k| same_bits(&k.parts, &a.parts)) {
            Some(k) => k.aliases.push(a.name),
            None => kept.push(a),
        }
    }
    out.extend(kept);
    out
}
