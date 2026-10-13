//! Throttle steps at a steady rpm: how long the output level takes to
//! settle (within 2 dB of where it ends up) after a press and a lift.
//! `cargo run --release -p mp_exhaust --example throttle_step -- [PRESET RPM HEAT_RATE COMP_RELEASE]`
//! (the rates default to the lab's, `Response::LAB`; the game's are
//! `Response::GAME`, 12 and 0.0007).
use mp_exhaust::{Engine, Response, State, preset};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let key = a.get(1).cloned().unwrap_or("flatV8".into());
    let rpm: f64 = a.get(2).map_or(3500.0, |s| s.parse().unwrap());
    let heat: f64 = a
        .get(3)
        .map_or(Response::LAB.heat_rate, |s| s.parse().unwrap());
    let rel: f64 = a
        .get(4)
        .map_or(Response::LAB.comp_release, |s| s.parse().unwrap());
    let sr = 48000.0;
    let (mut press, mut lift, mut n) = (0.0, 0.0, 0);
    let (mut on_db, mut off_db) = (0.0, 0.0);
    let (mut bp, mut bl) = (0.0, 0.0);
    for seed in 1..=6u32 {
        let p = preset(&key).unwrap();
        let mut e = Engine::new(
            p,
            State {
                rpm,
                throttle: 0.0,
                ..State::default()
            },
            seed,
            sr,
        );
        e.response = Response {
            heat_rate: heat,
            comp_release: rel,
        };
        let mut l = [0f32; 128];
        let mut r = [0f32; 128];
        // 20 ms windows of level, over 0 (2 s), 1 (2 s), 0 (2 s).
        let mut lev = Vec::new();
        let mut bri = Vec::new();
        let (mut acc, mut k, mut accd, mut prev) = (0.0, 0, 0.0, 0.0f64);
        for b in 0..(sr as usize * 6 / 128) {
            let t = b as f64 * 128.0 / sr;
            let thr = if (2.0..4.0).contains(&t) { 1.0 } else { 0.0 };
            e.set_state(
                State {
                    rpm,
                    throttle: thr,
                    boost: 0.0,
                    speed: 20.0,
                },
                false,
            );
            e.process(&mut l, &mut r);
            for i in 0..128 {
                acc += (l[i] as f64).powi(2) + (r[i] as f64).powi(2);
                accd += (l[i] as f64 - prev).powi(2);
                prev = l[i] as f64;
                k += 1;
                if k == 960 {
                    lev.push(10.0 * (acc / k as f64 + 1e-12).log10());
                    bri.push(10.0 * ((2.0 * accd + 1e-12) / (acc + 1e-12)).log10());
                    acc = 0.0;
                    accd = 0.0;
                    k = 0;
                }
            }
        }
        // Smooth over 60 ms (the firing's own ripple), then settle times.
        let smooth = |v: &[f64]| -> Vec<f64> {
            (0..v.len())
                .map(|i| {
                    let w = &v[i.saturating_sub(2)..(i + 3).min(v.len())];
                    w.iter().sum::<f64>() / w.len() as f64
                })
                .collect()
        };
        let sm = smooth(&lev);
        let sb = smooth(&bri);
        let settle = |sm: &[f64], from: usize, tol: f64| {
            let fin = sm[from + 70..from + 100].iter().sum::<f64>() / 30.0;
            let last = (from..from + 70).rev().find(|&i| (sm[i] - fin).abs() > tol);
            (last.map_or(0, |i| i + 1 - from) as f64 * 20.0, fin)
        };
        let (pt, pf) = settle(&sm, 100, 2.0);
        let (lt, lf) = settle(&sm, 200, 2.0);
        bp += settle(&sb, 100, 0.5).0;
        bl += settle(&sb, 200, 0.5).0;
        press += pt;
        lift += lt;
        on_db += pf;
        off_db += lf;
        n += 1;
    }
    let n = n as f64;
    println!(
        "{key} {rpm} rpm heat {heat} release {rel}: level settles {:.0} ms after a press, {:.0} ms after a lift (on {:.1} dB, off {:.1} dB); brightness {:.0} / {:.0} ms",
        press / n,
        lift / n,
        on_db / n,
        off_db / n,
        bp / n,
        bl / n
    );
}
