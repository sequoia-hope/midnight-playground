//! Chrome's OscillatorNode for the native backend (DECISIONS D257): a port
//! of Blink's `PeriodicWaveHandler` (band-limited wave tables) and
//! `OscillatorHandler` (table lookup, scheduling) as a `web-audio-api`
//! worklet processor.
//!
//! The crate's oscillator draws square and sawtooth with polyBLEP at full
//! scale, the triangle and every periodic wave without band-limiting, and it
//! ignores a built-in type set after a periodic wave. Chrome plays every
//! waveform from tables that keep only the partials below Nyquist for the
//! pitch (three tables per octave, crossfaded), normalised to the full
//! table's peak. The difference is several dB in the top bands and in
//! level, past SPEC 7.5's 1.5 dB, so the native backend runs Chrome's.
//!
//! Ported from Chromium's Blink (Copyright 2011/2012 Google Inc. and The
//! Chromium Authors; BSD-style licence, as the original files state).
//!
//! Also Chrome's: start and end frames rounded up (`TimeToSampleFrame`), and
//! in the render quantum an oscillator starts in, its a-rate params read
//! from the quantum's start (a start `off` frames in hears them `off` frames
//! early), which one-shots that automate from their start time can hear.

use std::f64::consts::PI;
use std::sync::{Arc, Mutex, OnceLock};
use web_audio_api::worklet::{AudioParamValues, AudioWorkletGlobalScope, AudioWorkletProcessor};
use web_audio_api::{AudioParamDescriptor, AutomationRate};

/// The number of bands per octave.
const NUMBER_OF_OCTAVE_BANDS: u32 = 3;
const CENTS_PER_RANGE: f32 = 1200.0 / NUMBER_OF_OCTAVE_BANDS as f32;
const INTERPOLATE_2_POINT: f32 = 0.3;
const INTERPOLATE_3_POINT: f32 = 0.16;
const QUANTUM: usize = 128;

/// `PeriodicWaveSize()` for a sample rate.
fn periodic_wave_size(sample_rate: f32) -> usize {
    if sample_rate <= 24000.0 {
        2048
    } else if sample_rate <= 88200.0 {
        4096
    } else {
        16384
    }
}

/// A periodic wave's band-limited tables (`PeriodicWaveHandler`).
pub struct WaveTables {
    size: usize,
    tables: Vec<Arc<Vec<f32>>>,
    lowest_fundamental: f32,
    rate_scale: f32,
}

/// In-place iterative radix-2 FFT with the `e^{+i...}` kernel, unscaled.
fn inverse_fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = 2.0 * PI / len as f64;
        for i in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (wr, wi) = ((ang * k as f64).cos(), (ang * k as f64).sin());
                let (a, b) = (i + k, i + k + len / 2);
                let xr = re[b] * wr - im[b] * wi;
                let xi = re[b] * wi + im[b] * wr;
                re[b] = re[a] - xr;
                im[b] = im[a] - xi;
                re[a] += xr;
                im[a] += xi;
            }
        }
        len <<= 1;
    }
}

impl WaveTables {
    /// `CreateBandLimitedTables(real, imag, disableNormalization)`.
    pub fn new(real: &[f32], imag: &[f32], disable_normalization: bool, sample_rate: f32) -> Self {
        let size = periodic_wave_size(sample_rate);
        let half = size / 2;
        let number_of_ranges =
            (0.5 + NUMBER_OF_OCTAVE_BANDS as f32 * (size as f32).log2()) as usize;
        let components = real.len().min(imag.len()).min(half);
        let mut tables: Vec<Arc<Vec<f32>>> = Vec::with_capacity(number_of_ranges);
        let mut last_kept = usize::MAX;
        let mut scale = 1.0f32;
        for range in 0..number_of_ranges {
            // NumberOfPartialsForRange.
            let cents_to_cull = range as f32 * CENTS_PER_RANGE;
            let culling_scale = 2f64.powf((-cents_to_cull / 1200.0) as f64);
            let partials = (culling_scale * half as f64) as usize;
            // Bins kept: 1 .. min(components, partials + 1).
            let kept = components.min(partials + 1);
            if kept == last_kept
                && let Some(t) = tables.last()
            {
                tables.push(t.clone());
                continue;
            }
            last_kept = kept;
            let mut re = vec![0f64; size];
            let mut im = vec![0f64; size];
            for k in 1..kept {
                // x[n] = sum a_k cos + b_k sin = Re sum (a_k - i b_k) e^{+i...}
                re[k] = real[k] as f64;
                im[k] = -(imag[k] as f64);
            }
            inverse_fft(&mut re, &mut im);
            let mut t: Vec<f32> = re.iter().map(|x| *x as f32).collect();
            if !disable_normalization && range == 0 {
                let max = t.iter().fold(0f32, |m, x| m.max(x.abs()));
                if max != 0.0 {
                    scale = 1.0 / max;
                }
            }
            if !disable_normalization {
                for x in &mut t {
                    *x *= scale;
                }
            }
            tables.push(Arc::new(t));
        }
        WaveTables {
            size,
            tables,
            lowest_fundamental: 0.5 * sample_rate / half as f32,
            rate_scale: size as f32 / sample_rate,
        }
    }

    /// `WaveDataForFundamentalFrequency`: (lower, higher, factor).
    fn wave_data(&self, fundamental: f32) -> (&[f32], &[f32], f32) {
        let f = fundamental.abs();
        let ratio = if f > 0.0 {
            f / self.lowest_fundamental
        } else {
            0.5
        };
        let cents = ratio.log2() * 1200.0;
        let ranges = self.tables.len();
        let pitch_range = (1.0 + cents / CENTS_PER_RANGE).clamp(0.0, (ranges - 1) as f32);
        let i1 = pitch_range as usize;
        let i2 = if i1 < ranges - 1 { i1 + 1 } else { i1 };
        (
            &self.tables[i2][..],
            &self.tables[i1][..],
            pitch_range - i1 as f32,
        )
    }
}

/// The built-in types (`GenerateBasicWaveform`), made once per type.
pub fn basic_tables(kind: BasicType, sample_rate: f32) -> Arc<WaveTables> {
    type Cache = Mutex<Vec<((BasicType, u32), Arc<WaveTables>)>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let key = (kind, sample_rate.to_bits());
    let cache = CACHE.get_or_init(|| Mutex::new(Vec::new()));
    if let Some((_, t)) = cache.lock().expect("cache").iter().find(|(k, _)| *k == key) {
        return t.clone();
    }
    let half = periodic_wave_size(sample_rate) / 2;
    let real = vec![0f32; half];
    let mut imag = vec![0f32; half];
    for (n, b) in imag.iter_mut().enumerate().skip(1) {
        let pi_factor = 2.0 / (n as f32 * std::f32::consts::PI);
        let odd = n & 1 == 1;
        *b = match kind {
            BasicType::Sine => {
                if n == 1 {
                    1.0
                } else {
                    0.0
                }
            }
            BasicType::Square => {
                if odd {
                    2.0 * pi_factor
                } else {
                    0.0
                }
            }
            BasicType::Sawtooth => pi_factor * if odd { 1.0 } else { -1.0 },
            BasicType::Triangle => {
                if odd {
                    2.0 * (pi_factor * pi_factor) * if ((n - 1) >> 1) & 1 == 1 { -1.0 } else { 1.0 }
                } else {
                    0.0
                }
            }
        };
    }
    let t = Arc::new(WaveTables::new(&real, &imag, false, sample_rate));
    cache.lock().expect("cache").push((key, t.clone()));
    t
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BasicType {
    Sine,
    Square,
    Sawtooth,
    Triangle,
}

/// Messages to the processor.
pub enum OscMessage {
    Start(f64),
    Stop(f64),
    Wave(Arc<WaveTables>),
    /// Not pulled by the destination: not processed (no output, no phase
    /// advance, no scheduling), as Chrome leaves a disconnected node.
    Frozen(bool),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Unscheduled,
    Scheduled,
    Playing,
    Finished,
}

/// `OscillatorHandler`.
pub struct ChromeOscillator {
    wave: Arc<WaveTables>,
    virtual_read_index: f64,
    start_time: f64,
    end_time: Option<f64>,
    state: State,
    phase_increments: Vec<f32>,
    frozen: bool,
}

/// `TimeToSampleFrame(t, sr, kRoundUp)`.
fn frame_up(t: f64, sample_rate: f64) -> u64 {
    (t * sample_rate).ceil().max(0.0) as u64
}

#[allow(clippy::too_many_arguments)]
fn do_interpolation(
    virtual_read_index: f64,
    incr: f32,
    mask: usize,
    table_factor: f32,
    lower: &[f32],
    higher: &[f32],
) -> f32 {
    let mut s_lower = 0f64;
    let mut s_higher = 0f64;
    let read_index_0 = virtual_read_index as usize;
    if incr >= INTERPOLATE_2_POINT {
        let r0 = read_index_0 & mask;
        let r2 = (read_index_0 + 1) & mask;
        let f = ((virtual_read_index as f32) - read_index_0 as f32) as f64;
        s_higher = (1.0 - f) * higher[r0] as f64 + f * higher[r2] as f64;
        s_lower = (1.0 - f) * lower[r0] as f64 + f * lower[r2] as f64;
    } else if incr >= INTERPOLATE_3_POINT {
        let t = virtual_read_index - read_index_0 as f64;
        let a = [0.5 * t * (t - 1.0), 1.0 - t * t, 0.5 * t * (t + 1.0)];
        for (k, ak) in a.iter().enumerate() {
            let i = (read_index_0.wrapping_add(k).wrapping_sub(1)) & mask;
            s_lower += ak * lower[i] as f64;
            s_higher += ak * higher[i] as f64;
        }
    } else {
        let t = virtual_read_index - read_index_0 as f64;
        let t2 = t * t;
        let a = [
            t * (t2 - 1.0) * (t - 2.0) / 24.0,
            -t * (t - 1.0) * (t2 - 4.0) / 6.0,
            (t2 - 1.0) * (t2 - 4.0) / 4.0,
            -t * (t + 1.0) * (t2 - 4.0) / 6.0,
            t * (t2 - 1.0) * (t + 2.0) / 24.0,
        ];
        for (k, ak) in a.iter().enumerate() {
            let i = (read_index_0.wrapping_add(k).wrapping_sub(2)) & mask;
            s_lower += ak * lower[i] as f64;
            s_higher += ak * higher[i] as f64;
        }
    }
    ((1.0 - table_factor as f64) * s_higher + table_factor as f64 * s_lower) as f32
}

fn clamp_frequency(f: f32, nyquist: f32) -> f32 {
    if f.is_nan() {
        nyquist
    } else {
        f.clamp(-nyquist, nyquist)
    }
}

impl ChromeOscillator {
    fn wrap(&self, v: f64) -> f64 {
        let size = self.wave.size as f64;
        v - (v * (1.0 / size)).floor() * size
    }

    /// One render quantum into `out` (128 frames).
    fn render(
        &mut self,
        out: &mut [f32],
        freq: &[f32],
        detune: &[f32],
        quantum_start: u64,
        sample_rate: f32,
    ) -> bool {
        out.fill(0.0);
        let sr = sample_rate as f64;
        let frames = out.len();
        let quantum_end = quantum_start + frames as u64;
        let start_frame = frame_up(self.start_time, sr);
        let end_frame = self.end_time.map(|t| frame_up(t, sr));
        if let Some(e) = end_frame
            && e <= quantum_start
            && self.state != State::Unscheduled
        {
            self.state = State::Finished;
        }
        if matches!(self.state, State::Unscheduled | State::Finished) || start_frame >= quantum_end
        {
            return self.state != State::Finished;
        }
        let start_frame_offset = if self.state == State::Scheduled {
            self.state = State::Playing;
            self.start_time * sr - start_frame as f64
        } else {
            0.0
        };
        let quantum_frame_offset = (start_frame.saturating_sub(quantum_start) as usize).min(frames);
        let mut n = frames - quantum_frame_offset;
        if let Some(e) = end_frame
            && e >= quantum_start
            && e <= quantum_end
        {
            if e < quantum_end {
                let zero_from = (e - quantum_start) as usize;
                let to_zero = frames - zero_from;
                n = n.saturating_sub(to_zero);
            }
            self.state = State::Finished;
        }
        if n == 0 {
            return self.state != State::Finished;
        }
        let nyquist = sample_rate / 2.0;
        let wave = self.wave.clone();
        let size = wave.size;
        let mask = size - 1;
        let rate_scale = wave.rate_scale;
        let mut vri = self.virtual_read_index;

        // CalculateSampleAccuratePhaseIncrements.
        let freq_changes = freq.len() > 1;
        let detune_changes = detune.len() > 1;
        let sample_accurate = freq_changes || detune_changes;
        let mut frequency = 0f32;
        if sample_accurate {
            let mut final_scale = rate_scale;
            let pi = &mut self.phase_increments;
            if freq_changes {
                pi[..frames].copy_from_slice(&freq[..frames]);
            } else {
                final_scale *= freq[0];
            }
            if detune_changes {
                for i in 0..frames {
                    let d = (detune[i] * (1.0 / 1200.0)).exp2();
                    pi[i] = if freq_changes { d * pi[i] } else { d };
                }
            } else {
                final_scale *= (detune[0] / 1200.0).exp2();
            }
            for p in pi[..frames].iter_mut() {
                *p = clamp_frequency(*p, nyquist) * final_scale;
            }
        } else {
            frequency = clamp_frequency(freq[0] * (detune[0] / 1200.0).exp2(), nyquist);
        }

        let mut dest = quantum_frame_offset;
        if start_frame_offset > 0.0 {
            dest += 1;
            n -= 1;
            vri += (1.0 - start_frame_offset) * frequency as f64 * rate_scale as f64;
        } else if start_frame_offset < 0.0 {
            vri = -start_frame_offset * frequency as f64 * rate_scale as f64;
        }

        if sample_accurate {
            // ProcessARate: groups of four, then the rest; the increments
            // are read from the quantum's start.
            let pi = &self.phase_increments;
            let mut k = 0;
            while k < n {
                let group = if k + 4 <= n { 4 } else { 1 };
                let big = group == 4 && pi[k..k + 4].iter().all(|x| x.abs() >= INTERPOLATE_2_POINT);
                for m in k..k + group {
                    let incr = pi[m];
                    let (lower, higher, tf) = wave.wave_data(incr / rate_scale);
                    let s = if big {
                        let r0 = (vri as usize) & mask;
                        let r1 = (r0 + 1) & mask;
                        let f = (vri as f32) - (vri as usize) as f32;
                        let sh = higher[r0] + f * (higher[r1] - higher[r0]);
                        let sl = lower[r0] + f * (lower[r1] - lower[r0]);
                        sh + tf * (sl - sh)
                    } else {
                        do_interpolation(vri, incr.abs(), mask, tf, lower, higher)
                    };
                    out[dest + m] = s;
                    vri += incr as f64;
                    vri = self.wrap(vri);
                }
                k += group;
            }
        } else {
            // ProcessKRate.
            let (lower, higher, tf) = wave.wave_data(frequency);
            let incr = frequency * rate_scale;
            if incr >= INTERPOLATE_2_POINT {
                let mut v = vri;
                for m in 0..n {
                    let r0 = (v as usize) & mask;
                    let r1 = (r0 + 1) & mask;
                    let f = (v as f32) - r0 as f32;
                    let sh = higher[r0] + f * (higher[r1] - higher[r0]);
                    let sl = lower[r0] + f * (lower[r1] - lower[r0]);
                    out[dest + m] = sh + tf * (sl - sh);
                    v += incr as f64;
                    v = self.wrap(v);
                }
                // Recomputed to reduce round-off.
                vri += n as f64 * incr as f64;
                vri = self.wrap(vri);
            } else {
                for m in 0..n {
                    out[dest + m] = do_interpolation(vri, incr.abs(), mask, tf, lower, higher);
                    vri += incr as f64;
                    vri = self.wrap(vri);
                }
            }
        }
        self.virtual_read_index = vri;
        self.state != State::Finished
    }
}

impl AudioWorkletProcessor for ChromeOscillator {
    type ProcessorOptions = Arc<WaveTables>;

    fn constructor(wave: Arc<WaveTables>) -> Self {
        ChromeOscillator {
            wave,
            virtual_read_index: 0.0,
            start_time: 0.0,
            end_time: None,
            state: State::Unscheduled,
            phase_increments: vec![0.0; QUANTUM],
            // Nothing pulls on a new node until it is connected.
            frozen: true,
        }
    }

    fn parameter_descriptors() -> Vec<AudioParamDescriptor> {
        // The nominal ranges (Nyquist at 48 kHz; the processor clamps to the
        // context's own).
        vec![
            AudioParamDescriptor {
                name: "frequency".into(),
                automation_rate: AutomationRate::A,
                default_value: 440.0,
                min_value: f32::MIN,
                max_value: f32::MAX,
            },
            AudioParamDescriptor {
                name: "detune".into(),
                automation_rate: AutomationRate::A,
                default_value: 0.0,
                min_value: -153600.0,
                max_value: 153600.0,
            },
        ]
    }

    fn process<'a, 'b>(
        &mut self,
        _inputs: &'b [&'a [&'a [f32]]],
        outputs: &'b mut [&'a mut [&'a mut [f32]]],
        params: AudioParamValues<'b>,
        scope: &'b AudioWorkletGlobalScope,
    ) -> bool {
        let freq = params.get("frequency");
        let detune = params.get("detune");
        let out = &mut outputs[0][0];
        if self.frozen {
            out.fill(0.0);
            return self.state != State::Finished;
        }
        self.render(out, &freq, &detune, scope.current_frame, scope.sample_rate)
    }

    fn onmessage(&mut self, msg: &mut dyn std::any::Any) {
        let Some(m) = msg.downcast_mut::<OscMessage>() else {
            return;
        };
        match m {
            OscMessage::Start(t) => {
                if self.state == State::Unscheduled {
                    self.start_time = *t;
                    self.state = State::Scheduled;
                }
            }
            OscMessage::Stop(t) => self.end_time = Some(*t),
            OscMessage::Wave(w) => self.wave = w.clone(),
            OscMessage::Frozen(f) => self.frozen = *f,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_square_peaks_at_one() {
        let t = basic_tables(BasicType::Square, 48000.0);
        assert_eq!(t.tables.len(), 36);
        let max = t.tables[0].iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!((max - 1.0).abs() < 1e-6);
        // The unnormalised square peaks at its Gibbs overshoot, ~1.179.
        let raw = WaveTables::new(
            &[0.0; 2048],
            &{
                let mut v = vec![0f32; 2048];
                for (n, b) in v.iter_mut().enumerate().skip(1) {
                    if n & 1 == 1 {
                        *b = 4.0 / (n as f32 * std::f32::consts::PI);
                    }
                }
                v
            },
            true,
            48000.0,
        );
        let peak = raw.tables[0].iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!((peak - 1.179).abs() < 1e-3, "{peak}");
    }

    #[test]
    fn sine_at_a_frequency() {
        let mut o = ChromeOscillator::constructor(basic_tables(BasicType::Sine, 48000.0));
        o.start_time = 0.0;
        o.state = State::Scheduled;
        let mut out = vec![0f32; 128];
        o.render(&mut out, &[1000.0], &[0.0], 0, 48000.0);
        for (i, x) in out.iter().enumerate() {
            let want = (2.0 * PI * 1000.0 * i as f64 / 48000.0).sin() as f32;
            assert!((x - want).abs() < 1e-3, "{i}: {x} vs {want}");
        }
    }
}
