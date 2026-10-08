//! The lab's engine tests (`tools/music-lab/test/lab.test.js`, the Engine
//! section), ported, plus `cue`, which the lab lacks. The `renderEngine`
//! harness: 48 kHz, 128-sample blocks, the output stored to `f32` as the
//! lab's `Float32Array`s are. The lab's first test also plays the JS game's
//! tracks (`voiceTrack`), which this crate does not hold: generated tracks
//! stand in.

// The lab's index loops stay index loops, as in the crate.
#![allow(clippy::needless_range_loop)]

use mp_music::engine::{Engine, EngineEvent};
use mp_music::genres::genre;
use mp_music::js_round;
use mp_music::track::{Track, lookup};

const RATE: f64 = 48000.0;

fn rms(x: &[f32]) -> f64 {
    let mut s = 0.0;
    for &v in x {
        s += v as f64 * v as f64;
    }
    (s / x.len() as f64).sqrt()
}

fn peak(x: &[f32]) -> f64 {
    let mut p = 0.0f64;
    for &v in x {
        p = p.max((v as f64).abs());
    }
    p
}

fn finite(x: &[f32]) -> bool {
    x.iter().all(|v| v.is_finite())
}

/// Power in a band, by direct DFT over a few bins (small n only).
fn band_power(x: &[f32], lo: f64, hi: f64) -> f64 {
    let n = x.len();
    let mut p = 0.0;
    let mut f = lo;
    while f < hi {
        let mut re = 0.0;
        let mut im = 0.0;
        let w = (2.0 * std::f64::consts::PI * f) / RATE;
        for i in 0..n {
            re += x[i] as f64 * (w * i as f64).cos();
            im -= x[i] as f64 * (w * i as f64).sin();
        }
        p += re * re + im * im;
        f += RATE / n as f64;
    }
    p
}

fn make(g: &str, seed: u32) -> Track {
    (genre(g).unwrap_or_else(|| panic!("genre {g}")).make)(seed, None)
}

struct Render {
    l: Vec<f32>,
    r: Vec<f32>,
    e: Engine,
}

/// `renderEngine(T, { secs, bar, energy })`, with the lab's seed 5.
fn render_engine(t: &Track, secs: f64, bar: u32, energy: f64) -> Render {
    let mut e = Engine::new(5, RATE);
    e.set_track(t);
    e.set_energy(energy);
    e.play(bar, 0.02);
    let (l, r) = run(&mut e, secs);
    Render { l, r, e }
}

fn run(e: &mut Engine, secs: f64) -> (Vec<f32>, Vec<f32>) {
    let n = (js_round((secs * RATE) / 128.0) * 128.0) as usize;
    let mut l = vec![0f32; n];
    let mut r = vec![0f32; n];
    let mut i = 0;
    while i < n {
        e.process(&mut l[i..i + 128], &mut r[i..i + 128]);
        i += 128;
    }
    (l, r)
}

#[test]
fn every_song_renders_finite_audible_and_under_the_ceiling_the_same_every_time() {
    let songs = [
        make("chicha", 1),
        make("dnb", 3),
        make("house", 1),
        make("techno", 2),
    ];
    for (i, t) in songs.iter().enumerate() {
        let bar = t.sections[0].bars;
        let a = render_engine(t, 3.0, bar, 1.0);
        assert!(finite(&a.l) && finite(&a.r), "{}", t.id);
        assert!(rms(&a.l) > 0.01, "{} rms {}", t.id, rms(&a.l));
        assert!(
            peak(&a.l) <= 0.951 && peak(&a.r) <= 0.951,
            "{} peak {} {}",
            t.id,
            peak(&a.l),
            peak(&a.r)
        );
        if i == 0 || i == songs.len() - 1 {
            let b = render_engine(t, 3.0, bar, 1.0);
            assert_eq!(a.l, b.l, "{} deterministic", t.id);
        }
    }
}

#[test]
fn energy_closes_the_mix_down() {
    let t = make("house", 5);
    let full = render_engine(&t, 3.0, 48, 1.0).l;
    let low = render_engine(&t, 3.0, 48, 0.15).l;
    let hi = |x: &[f32]| band_power(&x[48000..48000 + 8192], 3000.0, 8000.0);
    // What is left up there at low energy is the kick's click through the
    // 480 Hz low-pass; the hats and the lead are out (lay) or filtered.
    assert!(
        hi(&low) < hi(&full) / 4.0,
        "high band {} dB down",
        10.0 * (hi(&full) / hi(&low)).log10()
    );
}

/// The first section with an `acid.cutoff` automation, and the bar it
/// starts at.
fn auto_section(t: &Track) -> (usize, u32) {
    let mut bar = 0;
    for (i, s) in t.sections.iter().enumerate() {
        if s.auto
            .as_ref()
            .is_some_and(|a| lookup(a, "acid.cutoff").is_some())
        {
            return (i, bar);
        }
        bar += s.bars;
    }
    panic!("no acid.cutoff automation");
}

#[test]
fn section_automation_moves_the_patch() {
    let t = make("psytrance", 4);
    let (sec, bar) = auto_section(&t);
    let r = render_engine(&t, 4.0, bar, 1.0);
    let [from, to] = *lookup(t.sections[sec].auto.as_ref().unwrap(), "acid.cutoff").unwrap();
    let c =
        r.e.part_lab("acid")
            .expect("the acid part")
            .cutoff
            .expect("a cutoff");
    assert!(
        c != from && (c - from) * (to - from) > 0.0,
        "cutoff {c} on the way from {from} to {to}"
    );
}

#[test]
fn section_events_are_taken_in_order() {
    let t = make("house", 1);
    let bars = t.sections[0].bars;
    let mut e = Engine::new(5, RATE);
    e.set_track(&t);
    e.play(0, 0.02);
    // Through the first section's end into the second.
    let secs = bars as f64 * 16.0 * e.seq().unwrap().step_dur + 0.5;
    run(&mut e, secs);
    assert_eq!(
        e.take_events(),
        vec![EngineEvent::Section(0), EngineEvent::Section(1)]
    );
    assert!(e.take_events().is_empty());
    // The song's end wraps to the start with an End.
    let last = e.seq().unwrap().bars - 1;
    e.play(last, 0.02);
    let bar_secs = 16.0 * e.seq().unwrap().step_dur;
    run(&mut e, bar_secs + 0.5);
    let ev = e.take_events();
    assert!(ev.contains(&EngineEvent::End), "{ev:?}");
    assert!(ev.contains(&EngineEvent::Section(0)), "{ev:?}");
}

/// The first section with an lp sweep, and the bar it starts at.
fn lp_section(t: &Track) -> (usize, u32) {
    let mut bar = 0;
    for (i, s) in t.sections.iter().enumerate() {
        if s.lp.is_some() {
            return (i, bar);
        }
        bar += s.bars;
    }
    panic!("no lp section");
}

#[test]
fn cue_at_a_bars_start_is_play() {
    let t = make("techno", 2);
    let (_, bar) = lp_section(&t);
    let mut a = Engine::new(5, RATE);
    a.set_track(&t);
    a.play(bar, 0.02);
    let mut b = Engine::new(5, RATE);
    b.set_track(&t);
    b.cue(16 * bar, 0.02);
    let (al, ar) = run(&mut a, 2.0);
    let (bl, br) = run(&mut b, 2.0);
    assert_eq!(al, bl);
    assert_eq!(ar, br);
    assert_eq!(a.position(), b.position());
    assert_eq!(a.take_events(), b.take_events());
}

#[test]
fn cue_mid_bar_reports_the_step_and_plays() {
    let t = make("house", 1);
    let (sec, bar) = lp_section(&t);
    let mut e = Engine::new(5, RATE);
    e.set_track(&t);
    e.cue(16 * bar + 8, 0.02);
    let p = e.position().expect("a position");
    assert_eq!((p.sec, p.bar, p.step, p.bar_index), (sec, 0, 8, bar));
    assert!(p.playing);
    // The section's start was reported, though its drums and notes were
    // skipped.
    assert_eq!(e.take_events(), vec![EngineEvent::Section(sec)]);
    let (l, r) = run(&mut e, 1.0);
    assert!(finite(&l) && finite(&r));
    assert!(rms(&l) > 0.01, "rms {}", rms(&l));
    assert!(peak(&l) <= 0.951 && peak(&r) <= 0.951);
    // Half a bar on, the position has moved on from where it was cued.
    let q = e.position().expect("a position");
    assert!(q.bar_index > bar || q.step > 8, "{q:?}");
}
