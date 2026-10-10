//! Renders a patch alone or a song to a wav, to listen to and to measure:
//! `cargo run --release -p mp_music --example render -- patch surfGuitar out.wav [midi...]`
//! plays each note for a beat in turn (default: the chicha lead's register),
//! `cargo run --release -p mp_music --example render -- song chicha 3 out.wav [bar] [secs] [solo-part]`
//! renders a song from a bar, with one part soloed if named.

use mp_music::engine::Engine;
use mp_music::genres::genre;
use mp_music::instruments::{TriggerOpt, make_instrument};
use mp_music::patches::bpatch;

const SR: f64 = 48000.0;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("patch") => patch(&args[1..]),
        Some("song") => song(&args[1..]),
        _ => eprintln!(
            "render patch <name> <out.wav> [midi...] | song <genre> <seed> <out.wav> [bar] [secs] [solo]"
        ),
    }
}

fn patch(a: &[String]) {
    let lab = bpatch(&a[0]).expect("patch");
    let out = &a[1];
    let midis: Vec<f64> = if a.len() > 2 {
        a[2..].iter().map(|s| s.parse().unwrap()).collect()
    } else {
        vec![57.0, 60.0, 62.0, 64.0, 67.0, 69.0, 72.0]
    };
    let mut inst = make_instrument(lab, 7, SR);
    let beat = 0.6;
    let n = ((midis.len() as f64 + 2.0) * beat * SR) as usize;
    let (mut l, mut r) = (vec![0.0f32; n], vec![0.0f32; n]);
    let mut k = 0;
    for i in 0..n {
        if k < midis.len() && i == (k as f64 * beat * SR) as usize {
            inst.trigger(
                &[midis[k]],
                0.9,
                (beat * 0.9 * SR).round(),
                &TriggerOpt::default(),
            );
            k += 1;
        }
        let mut o = [0.0; 2];
        inst.run(&mut o);
        l[i] = o[0] as f32;
        r[i] = o[1] as f32;
    }
    write(out, &l, &r);
}

fn song(a: &[String]) {
    let g = genre(&a[0]).expect("genre");
    let seed: u32 = a[1].parse().unwrap();
    let out = &a[2];
    let bar: u32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
    let secs: f64 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(20.0);
    let mut t = (g.make)(seed, None);
    if let Some(solo) = a.get(5) {
        for (name, part) in t.parts.iter_mut() {
            if name != solo {
                part.ch.level = Some(0.0);
            }
        }
        for (_, lane) in t.drums.iter_mut() {
            lane.clear();
        }
    }
    let lead = t
        .parts
        .iter()
        .find(|(n, _)| n == "lead")
        .and_then(|(_, p)| p.lab.name.clone());
    println!("{} {:?} lead {:?}", t.id, t.style, lead);
    let mut e = Engine::new(1, SR);
    e.set_track(&t);
    e.set_energy(0.8);
    e.play(bar, 0.02);
    let n = (secs * SR) as usize / 128 * 128;
    let (mut l, mut r) = (vec![0f32; n], vec![0f32; n]);
    let mut i = 0;
    while i < n {
        e.process(&mut l[i..i + 128], &mut r[i..i + 128]);
        i += 128;
    }
    write(out, &l, &r);
}

fn write(path: &str, l: &[f32], r: &[f32]) {
    let mut b = Vec::new();
    let bytes = (l.len() * 4) as u32;
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + bytes).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&(SR as u32).to_le_bytes());
    b.extend_from_slice(&(SR as u32 * 4).to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&bytes.to_le_bytes());
    for i in 0..l.len() {
        for x in [l[i], r[i]] {
            b.extend_from_slice(&((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
    }
    std::fs::write(path, b).expect("write");
    let peak = l.iter().chain(r).fold(0.0f32, |m, x| m.max(x.abs()));
    println!("{path}: {:.1} s, peak {peak:.3}", l.len() as f64 / SR);
}
