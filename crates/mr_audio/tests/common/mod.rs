//! The conformance script shared by the backends' tests.

use mr_audio::wa::{AudioContext, BiquadFilterType, OscillatorType, OverSampleType};
use std::cell::RefCell;
use std::rc::Rc;

/// What the script needs from the backend's driver.
pub trait Driver {
    /// Move the clock (the null backend; a real backend's runs on its own).
    fn set_now(&self, t: f64);
}

/// The Rust twin of `tools/parity/audio-facade-ref.mjs`, step for step.
/// Returns what the promise callbacks saw.
pub fn script(ctx: &AudioContext, h: &impl Driver) -> Vec<String> {
    // ── Nodes, attributes, connections ──
    let g = ctx.create_gain();
    g.connect(&ctx.destination()).unwrap();
    let f = ctx.create_biquad_filter();
    f.set_type(BiquadFilterType::Bandpass);
    f.set_type_js("band");
    f.frequency.set_value(1150.0);
    f.q.set_value(0.9);
    f.connect(&g).unwrap();
    let o = ctx.create_oscillator();
    o.set_type(OscillatorType::Square);
    o.set_type_js("saw");
    o.set_type_js("custom");
    o.frequency.set_value(220.0);
    o.detune.set_target_at_time(12.0, 0.0, 0.05);
    o.connect(&f).unwrap();
    o.start().unwrap();
    assert!(o.start().is_err());
    o.stop_at(1.5).unwrap();

    // ── Automation and value reads ──
    g.gain.set_value(0.5);
    g.gain.set_value_at_time(1.0, 0.1);
    g.gain.linear_ramp_to_value_at_time(0.2, 0.3);
    g.gain.exponential_ramp_to_value_at_time(0.8, 0.5);
    g.gain.set_target_at_time(0.0, 0.6, 0.05);
    for t in [0.05, 0.2, 1.0 / 3.0, 0.4, 0.55, 0.7, 2.0] {
        h.set_now(t);
        g.gain.value();
    }
    h.set_now(0.65);
    g.gain.cancel_scheduled_values(0.6);
    g.gain.value();
    f.frequency.set_target_at_time(400.0, 0.65, 0.1);
    f.frequency.linear_ramp_to_value_at_time(900.0, 0.9);
    h.set_now(0.8);
    f.frequency.value();
    o.frequency.value();
    g.gain.exponential_ramp_to_value_at_time(0.0, 1.0);
    g.gain.set_target_at_time(0.0, 1.0, -1.0);
    g.gain.set_value(f64::NAN);
    g.gain.set_value_at_time(1.0, -1.0);
    g.gain.linear_ramp_to_value_at_time(f64::INFINITY, 1.0);
    g.gain.cancel_scheduled_values(-0.5);

    // ── Buffers, sources, convolver ──
    h.set_now(1.0);
    let b = ctx.create_buffer(2, 4, 48000.0);
    b.with_channel_data_mut(0, |d| d.copy_from_slice(&[0.0, 0.25, -0.5, 1.0]));
    b.copy_to_channel(&[1e-7, -0.0, 3.4e38, -1.0], 1);
    let s = ctx.create_buffer_source();
    s.set_buffer(Some(&b)).unwrap();
    s.set_loop(true);
    s.set_loop_start(0.25);
    s.set_loop_end(1.0);
    s.playback_rate.set_value(0.7);
    s.connect(&g).unwrap();
    s.start_with(1.0, 0.5, Some(1.0)).unwrap();
    assert!(s.set_buffer(Some(&b)).is_err());
    let s2 = ctx.create_buffer_source();
    s2.set_buffer(Some(&b)).unwrap();
    assert!(s2.stop().is_err());
    assert!(s2.start_at(-1.0).is_err());
    s2.start_with(2.0, 0.125, None).unwrap();
    let ir = ctx.create_buffer(2, 8, 48000.0);
    ir.with_channel_data_mut(0, |d| d[0] = 1.0);
    ir.with_channel_data_mut(1, |d| d[3] = -0.5);
    let cv = ctx.create_convolver();
    cv.set_buffer(Some(&ir)).unwrap();
    cv.set_normalize(false);
    cv.connect(&g).unwrap();
    let _bad = ctx.create_buffer(0, 1, 48000.0);

    // ── Periodic waves, shapers, the other node types ──
    let w = ctx
        .create_periodic_wave(&[0.0, 1.0, 0.5], &[0.0; 3], Some(true))
        .unwrap();
    let w2 = ctx
        .create_periodic_wave(&[0.0; 2], &[0.0, 1.0], None)
        .unwrap();
    assert!(
        ctx.create_periodic_wave(&[0.0; 2], &[0.0; 3], None)
            .is_err()
    );
    let o2 = ctx.create_oscillator();
    o2.set_periodic_wave(&w);
    o2.set_periodic_wave(&w2);
    o2.frequency.set_value(7.0);
    let sh = ctx.create_wave_shaper();
    sh.set_curve(Some(&[-1.0, 0.0, 1.0]));
    sh.set_oversample(OverSampleType::X2);
    sh.set_oversample_js("8x");
    o2.connect(&sh).unwrap();
    let d = ctx.create_delay(2.0);
    d.delay_time.set_value(0.375);
    let m = ctx.create_channel_merger(2);
    let p = ctx.create_stereo_panner();
    p.pan.set_value(-0.38);
    let c = ctx.create_dynamics_compressor();
    c.threshold.set_value(-13.0);
    c.knee.set_value(8.0);
    c.ratio.set_value(3.0);
    c.attack.set_value(0.005);
    c.release.set_value(0.18);
    sh.connect(&d).unwrap();
    d.connect_with(&m, Some(0), Some(1)).unwrap();
    sh.connect_with(&m, Some(0), Some(0)).unwrap();
    m.connect(&p).unwrap();
    p.connect(&c).unwrap();
    c.connect(&g).unwrap();
    let lfo = ctx.create_oscillator();
    lfo.set_type(OscillatorType::Triangle);
    lfo.frequency.set_value(1.0 / 3.4);
    lfo.connect(&o2.detune).unwrap();
    lfo.connect(&g.gain).unwrap();
    lfo.disconnect_from(&g.gain).unwrap();
    lfo.start_at(0.0).unwrap();
    assert!(lfo.disconnect_from(&g.gain).is_err());
    assert!(sh.disconnect_from(&g).is_err());
    sh.disconnect();
    o2.start_at(0.25).unwrap();

    // ── Context state and decoding ──
    h.set_now(2.5);
    let states = Rc::new(RefCell::new(Vec::<String>::new()));
    let (st, cx) = (states.clone(), ctx.clone());
    ctx.resume()
        .then(move |_| st.borrow_mut().push(cx.state().as_str().into()));
    ctx.settle();
    let st = states.clone();
    let outcome = |r: &Result<mr_audio::wa::AudioBuffer, mr_audio::wa::AudioError>| match r {
        Ok(b) => format!("decoded b{} {}", b.id(), b.length()),
        Err(e) => format!("failed {}", e.name.as_str()),
    };
    ctx.decode_audio_data(&[1, 2, 3])
        .then(move |r| st.borrow_mut().push(outcome(r)));
    let st = states.clone();
    ctx.decode_audio_data(&[9])
        .then(move |r| st.borrow_mut().push(outcome(r)));
    ctx.settle();
    let (st, cx) = (states.clone(), ctx.clone());
    ctx.suspend()
        .then(move |_| st.borrow_mut().push(cx.state().as_str().into()));
    ctx.settle();
    let (st, cx) = (states.clone(), ctx.clone());
    ctx.close()
        .then(move |_| st.borrow_mut().push(cx.state().as_str().into()));
    ctx.settle();
    states.borrow().clone()
}
