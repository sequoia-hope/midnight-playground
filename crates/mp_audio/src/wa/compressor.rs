//! Chrome's DynamicsCompressor for the native backend (DECISIONS D256): a
//! port of Blink's `DynamicsCompressor` kernel
//! (`third_party/blink/renderer/platform/audio/dynamics_compressor.cc`) as a
//! `web-audio-api` worklet processor. The crate's own compressor follows the
//! spec's outline of this algorithm but lets transients through several dB
//! hotter and delays by 384 frames instead of 288; every SFX and music path
//! of the game goes through two of them, so the native renders could not
//! match Chrome's band levels (SPEC 7.5) without the browser's kernel.
//!
//! Ported from Chromium's Blink (Copyright 2011/2012 Google Inc. and The
//! Chromium Authors; BSD-style licence, as the original files state).
//!
//! Stereo in (a mono input is duplicated, as Chrome up-mixes it), stereo
//! out; the params are read once per render quantum (k-rate), as Chrome's
//! handler does.

use web_audio_api::worklet::{AudioParamValues, AudioWorkletGlobalScope, AudioWorkletProcessor};
use web_audio_api::{AudioParamDescriptor, AutomationRate};

const METERING_RELEASE_TIME_CONSTANT: f32 = 0.325;
const PRE_DELAY: f32 = 0.006; // seconds
const MAX_PRE_DELAY_FRAMES: usize = 1024;
const MAX_PRE_DELAY_FRAMES_MASK: usize = MAX_PRE_DELAY_FRAMES - 1;
const DEFAULT_PRE_DELAY_FRAMES: usize = 256;

// Release zone values 0 -> 1.
const RELEASE_ZONE1: f32 = 0.09;
const RELEASE_ZONE2: f32 = 0.16;
const RELEASE_ZONE3: f32 = 0.42;
const RELEASE_ZONE4: f32 = 0.98;

// The polynomial's coefficients, computed in double as the C++ constexprs
// are, then stored as float.
const A_BASE: f32 = (0.9999999999999998 * RELEASE_ZONE1 as f64
    + 1.8432219684323923e-16 * RELEASE_ZONE2 as f64
    - 1.9373394351676423e-16 * RELEASE_ZONE3 as f64
    + 8.824516011816245e-18 * RELEASE_ZONE4 as f64) as f32;
const B_BASE: f32 = (-1.5788320352845888 * RELEASE_ZONE1 as f64
    + 2.3305837032074286 * RELEASE_ZONE2 as f64
    - 0.9141194204840429 * RELEASE_ZONE3 as f64
    + 0.1623677525612032 * RELEASE_ZONE4 as f64) as f32;
const C_BASE: f32 = (0.5334142869106424 * RELEASE_ZONE1 as f64
    - 1.272736789213631 * RELEASE_ZONE2 as f64
    + 0.9258856042207512 * RELEASE_ZONE3 as f64
    - 0.18656310191776226 * RELEASE_ZONE4 as f64) as f32;
const D_BASE: f32 = (0.08783463138207234 * RELEASE_ZONE1 as f64
    - 0.1694162967925622 * RELEASE_ZONE2 as f64
    + 0.08588057951595272 * RELEASE_ZONE3 as f64
    - 0.00429891410546283 * RELEASE_ZONE4 as f64) as f32;
const E_BASE: f32 = (-0.042416883008123074 * RELEASE_ZONE1 as f64
    + 0.1115693827987602 * RELEASE_ZONE2 as f64
    - 0.09764676325265872 * RELEASE_ZONE3 as f64
    + 0.028494263462021576 * RELEASE_ZONE4 as f64) as f32;

// Detector release time.
const SAT_RELEASE_TIME: f32 = 0.0025;

const PI_OVER_TWO_FLOAT: f32 = std::f32::consts::FRAC_PI_2;

fn ensure_finite(x: f32, default: f32) -> f32 {
    if x.is_finite() { x } else { default }
}

fn linear_to_decibels(x: f32) -> f32 {
    20.0 * x.log10()
}

fn decibels_to_linear(x: f32) -> f32 {
    10f32.powf(0.05 * x)
}

fn flush_denormal(x: f32) -> f32 {
    if x.is_subnormal() { 0.0 } else { x }
}

/// The kernel's state (`DynamicsCompressor`).
pub struct ChromeCompressor {
    sample_rate: f32,
    // Static curve parameters.
    ratio: f32,
    slope: f32,
    linear_threshold: f32,
    db_threshold: f32,
    db_knee: f32,
    knee_threshold: f32,
    db_knee_threshold: f32,
    db_yknee_threshold: f32,
    knee: f32,
    // Dynamic state.
    detector_average: f32,
    compressor_gain: f32,
    metering_gain: f32,
    metering_release_k: f32,
    db_max_attack_compression_diff: f32,
    pre_delay: [Vec<f32>; 2],
    pre_delay_read_index: usize,
    pre_delay_write_index: usize,
    last_pre_delay_frames: usize,
}

impl ChromeCompressor {
    pub fn new(sample_rate: f32) -> Self {
        let mut c = ChromeCompressor {
            sample_rate,
            ratio: -1.0,
            slope: -1.0,
            linear_threshold: -1.0,
            db_threshold: -1.0,
            db_knee: -1.0,
            knee_threshold: -1.0,
            db_knee_threshold: -1.0,
            db_yknee_threshold: -1.0,
            knee: -1.0,
            detector_average: 0.0,
            compressor_gain: 1.0,
            metering_gain: 1.0,
            metering_release_k: 0.0,
            db_max_attack_compression_diff: -1.0,
            pre_delay: [
                vec![0.0; MAX_PRE_DELAY_FRAMES],
                vec![0.0; MAX_PRE_DELAY_FRAMES],
            ],
            pre_delay_read_index: 0,
            pre_delay_write_index: DEFAULT_PRE_DELAY_FRAMES,
            last_pre_delay_frames: DEFAULT_PRE_DELAY_FRAMES,
        };
        // DiscreteTimeConstantForSampleRate.
        c.metering_release_k = (1.0
            - (-1.0 / (sample_rate as f64 * METERING_RELEASE_TIME_CONSTANT as f64)).exp())
            as f32;
        c
    }

    fn set_pre_delay_time(&mut self, t: f32) {
        let mut frames = (t * self.sample_rate) as usize;
        if frames > MAX_PRE_DELAY_FRAMES - 1 {
            frames = MAX_PRE_DELAY_FRAMES - 1;
        }
        if self.last_pre_delay_frames != frames {
            self.last_pre_delay_frames = frames;
            for b in &mut self.pre_delay {
                b.fill(0.0);
            }
            self.pre_delay_read_index = 0;
            self.pre_delay_write_index = frames;
        }
    }

    // Exponential curve for the knee. It is 1st derivative matched at
    // linear_threshold and asymptotically approaches linear_threshold + 1 / k.
    fn knee_curve(&self, x: f32, k: f32) -> f32 {
        if x < self.linear_threshold {
            return x;
        }
        self.linear_threshold + (1.0 - ((-k * (x - self.linear_threshold)) as f64).exp() as f32) / k
    }

    // Full compression curve with constant ratio after knee.
    fn saturate(&self, x: f32, k: f32) -> f32 {
        if x < self.knee_threshold {
            return self.knee_curve(x, k);
        }
        let db_x = linear_to_decibels(x);
        let db_y = self.db_yknee_threshold + self.slope * (db_x - self.db_knee_threshold);
        decibels_to_linear(db_y)
    }

    fn k_at_slope(&self, desired_slope: f32) -> f32 {
        let db_x = self.db_threshold + self.db_knee;
        let x = decibels_to_linear(db_x);
        let mut x2 = 1.0f32;
        let mut db_x2 = 0.0f32;
        if x >= self.linear_threshold {
            x2 = (x as f64 * 1.001) as f32;
            db_x2 = linear_to_decibels(x2);
        }
        // Approximate k given initial values.
        let mut min_k = 0.1f32;
        let mut max_k = 10000.0f32;
        let mut k = 5.0f32;
        let mut slope = 1.0f32;
        for _ in 0..15 {
            // Approximate 1st derivative with input and output expressed in dB.
            if x >= self.linear_threshold {
                let db_y = linear_to_decibels(self.knee_curve(x, k));
                let db_y2 = linear_to_decibels(self.knee_curve(x2, k));
                slope = (db_y2 - db_y) / (db_x2 - db_x);
            }
            if slope < desired_slope {
                max_k = k; // k is too high.
            } else {
                min_k = k; // k is too low.
            }
            // Re-calculate based on geometric mean.
            k = (min_k * max_k).sqrt();
        }
        k
    }

    fn update_static_curve_parameters(
        &mut self,
        db_threshold: f32,
        db_knee: f32,
        ratio: f32,
    ) -> f32 {
        if db_threshold != self.db_threshold || db_knee != self.db_knee || ratio != self.ratio {
            self.db_threshold = db_threshold;
            self.linear_threshold = decibels_to_linear(db_threshold);
            self.db_knee = db_knee;
            self.ratio = ratio;
            self.slope = 1.0 / ratio;
            let k = self.k_at_slope(1.0 / ratio);
            self.db_knee_threshold = db_threshold + db_knee;
            self.knee_threshold = decibels_to_linear(self.db_knee_threshold);
            self.db_yknee_threshold = linear_to_decibels(self.knee_curve(self.knee_threshold, k));
            self.knee = k;
        }
        self.knee
    }

    /// `Process`: `input` (one or two channels; empty is silence) into the
    /// two `output` channels.
    #[allow(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        input: &[&[f32]],
        out_l: &mut [f32],
        out_r: &mut [f32],
        db_threshold: f32,
        db_knee: f32,
        ratio: f32,
        attack_time: f32,
        release_time: f32,
    ) {
        let frames = out_l.len();
        let sample_rate = self.sample_rate;
        let k = self.update_static_curve_parameters(db_threshold, db_knee, ratio);
        // Makeup gain with empirical/perceptual tuning.
        let linear_post_gain = (1.0 / self.saturate(1.0, k)).powf(0.6);
        // Attack parameters.
        let attack_frames = attack_time.max(0.001) * sample_rate;
        // Release parameters.
        let release_frames = sample_rate * release_time;
        let sat_release_frames = SAT_RELEASE_TIME * sample_rate;
        // y = a + b*x + c*x^2 + d*x^3 + e*x^4: adaptive release frames
        // depending on the amount of compression.
        let a = release_frames * A_BASE;
        let b = release_frames * B_BASE;
        let c = release_frames * C_BASE;
        let d = release_frames * D_BASE;
        let e = release_frames * E_BASE;
        self.set_pre_delay_time(PRE_DELAY);
        const DIVISION_FRAMES: usize = 32;
        let zero = [0f32; 0];
        let ch = |j: usize, i: usize| -> f32 {
            let c: &[f32] = match input.len() {
                0 => &zero,
                1 => input[0],
                _ => input[j.min(input.len() - 1)],
            };
            c.get(i).copied().unwrap_or(0.0)
        };

        let mut frame_index = 0;
        while frame_index < frames {
            self.detector_average = ensure_finite(self.detector_average, 1.0);
            let desired_gain = self.detector_average;
            // Pre-warp so we get desired_gain after sin() warp below.
            let scaled_desired_gain = desired_gain.asin() / PI_OVER_TWO_FLOAT;
            let is_releasing = scaled_desired_gain > self.compressor_gain;
            let mut db_compression_diff = if scaled_desired_gain == 0.0 {
                if is_releasing { -1.0 } else { 1.0 }
            } else {
                linear_to_decibels(self.compressor_gain / scaled_desired_gain)
            };
            let envelope_rate = if is_releasing {
                // Release mode - db_compression_diff should be negative dB.
                self.db_max_attack_compression_diff = -1.0;
                db_compression_diff = ensure_finite(db_compression_diff, -1.0);
                // Adaptive release - higher compression releases faster.
                let mut x = db_compression_diff.clamp(-12.0, 0.0);
                x = 0.25 * (x + 12.0);
                let x2 = x * x;
                let x3 = x2 * x;
                let x4 = x2 * x2;
                let calc_release_frames = a + b * x + c * x2 + d * x3 + e * x4;
                const DB_SPACING: f32 = 5.0;
                let db_per_frame = DB_SPACING / calc_release_frames;
                decibels_to_linear(db_per_frame)
            } else {
                // Attack mode - db_compression_diff should be positive dB.
                db_compression_diff = ensure_finite(db_compression_diff, 1.0);
                if self.db_max_attack_compression_diff == -1.0
                    || self.db_max_attack_compression_diff < db_compression_diff
                {
                    self.db_max_attack_compression_diff = db_compression_diff;
                }
                let db_eff_atten_diff = self.db_max_attack_compression_diff.max(0.5);
                let x = 0.25 / db_eff_atten_diff;
                1.0 - x.powf(1.0 / attack_frames)
            };

            // Inner loop - calculate shaped power average - apply compression.
            let mut read = self.pre_delay_read_index;
            let mut write = self.pre_delay_write_index;
            let mut detector_average = self.detector_average;
            let mut compressor_gain = self.compressor_gain;
            let loop_frames = DIVISION_FRAMES.min(frames - frame_index);
            for _ in 0..loop_frames {
                let mut compressor_input = 0f32;
                // Predelay signal, computing compression amount from the
                // un-delayed version.
                for j in 0..2 {
                    let undelayed = ch(j, frame_index);
                    self.pre_delay[j][write] = undelayed;
                    let abs = if undelayed > 0.0 {
                        undelayed
                    } else {
                        -undelayed
                    };
                    if compressor_input < abs {
                        compressor_input = abs;
                    }
                }
                let abs_input = compressor_input;
                let shaped_input = self.saturate(abs_input, k);
                let attenuation = if abs_input <= 0.0001 {
                    1.0
                } else {
                    shaped_input / abs_input
                };
                let db_attenuation = (-linear_to_decibels(attenuation)).max(2.0);
                let db_per_frame = db_attenuation / sat_release_frames;
                let sat_release_rate = decibels_to_linear(db_per_frame) - 1.0;
                let is_release = attenuation > detector_average;
                let rate = if is_release { sat_release_rate } else { 1.0 };
                detector_average += (attenuation - detector_average) * rate;
                detector_average = detector_average.min(1.0);
                detector_average = ensure_finite(detector_average, 1.0);

                // Exponential approach to desired gain.
                if envelope_rate < 1.0 {
                    // Attack - reduce gain to desired.
                    compressor_gain += (scaled_desired_gain - compressor_gain) * envelope_rate;
                } else {
                    // Release - exponentially increase gain to 1.0.
                    compressor_gain *= envelope_rate;
                    compressor_gain = compressor_gain.min(1.0);
                }
                // Warp pre-compression gain to smooth out sharp exponential
                // transition points.
                let post_warp = ((PI_OVER_TWO_FLOAT * compressor_gain) as f64).sin() as f32;
                let total_gain = linear_post_gain * post_warp;
                // Metering.
                let db_real_gain = linear_to_decibels(post_warp);
                if db_real_gain < self.metering_gain {
                    self.metering_gain = db_real_gain;
                } else {
                    self.metering_gain +=
                        (db_real_gain - self.metering_gain) * self.metering_release_k;
                }
                out_l[frame_index] = self.pre_delay[0][read] * total_gain;
                out_r[frame_index] = self.pre_delay[1][read] * total_gain;
                frame_index += 1;
                read = (read + 1) & MAX_PRE_DELAY_FRAMES_MASK;
                write = (write + 1) & MAX_PRE_DELAY_FRAMES_MASK;
            }
            self.pre_delay_read_index = read;
            self.pre_delay_write_index = write;
            self.detector_average = flush_denormal(detector_average);
            self.compressor_gain = flush_denormal(compressor_gain);
        }
    }
}

/// The params, with the spec's defaults and ranges.
pub const PARAMS: [(&str, f32, f32, f32); 5] = [
    ("threshold", -24.0, -100.0, 0.0),
    ("knee", 30.0, 0.0, 40.0),
    ("ratio", 12.0, 1.0, 20.0),
    ("attack", 0.003, 0.0, 1.0),
    ("release", 0.25, 0.0, 1.0),
];

impl AudioWorkletProcessor for ChromeCompressor {
    type ProcessorOptions = f32;

    fn constructor(sample_rate: f32) -> Self {
        ChromeCompressor::new(sample_rate)
    }

    fn parameter_descriptors() -> Vec<AudioParamDescriptor> {
        PARAMS
            .iter()
            .map(
                |&(name, default_value, min_value, max_value)| AudioParamDescriptor {
                    name: name.into(),
                    automation_rate: AutomationRate::K,
                    default_value,
                    min_value,
                    max_value,
                },
            )
            .collect()
    }

    fn process<'a, 'b>(
        &mut self,
        inputs: &'b [&'a [&'a [f32]]],
        outputs: &'b mut [&'a mut [&'a mut [f32]]],
        params: AudioParamValues<'b>,
        _scope: &'b AudioWorkletGlobalScope,
    ) -> bool {
        let p = |n: &str| params.get(n)[0];
        let (thr, knee, ratio, att, rel) = (
            p("threshold"),
            p("knee"),
            p("ratio"),
            p("attack"),
            p("release"),
        );
        let input: &[&[f32]] = inputs.first().copied().unwrap_or(&[]);
        let out = &mut outputs[0];
        let (l, r) = out.split_at_mut(1);
        self.process(input, l[0], r[0], thr, knee, ratio, att, rel);
        // A compressor keeps running (its pre-delay and release tail).
        true
    }
}
