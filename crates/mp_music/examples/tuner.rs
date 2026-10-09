//! Renders the dial turning round the stations, to hear the tuner:
//! `cargo run --release -p mp_music --example tuner -- out.wav [turns]`.
//! Each turn tunes the next station, plays it for three seconds, and
//! goes on; every turn is drawn afresh, so no two should sound alike.

use mp_music::player::Player;
use mp_music::radio::{EPOCH, STATIONS, wall_parts};

const SR: f64 = 48000.0;
const BLOCK: usize = 128;

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| "tuner.wav".into());
    let turns: usize = args.next().and_then(|n| n.parse().ok()).unwrap_or(8);
    let mut p = Player::new(1, SR);
    let wall = EPOCH + 40.0 * 86400.0 + 3333.0;
    let (day, sec) = wall_parts(wall);
    let mut data = Vec::new();
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let mut t = 0.0;
    for k in 0..turns {
        let st = (k % STATIONS.len()) as f32;
        let n = (3.5 * SR) as usize / BLOCK;
        for _ in 0..n {
            p.set_params(st, day as f32, (sec + t) as f32, k as f32, 0.8);
            p.process(&mut l, &mut r);
            for i in 0..BLOCK {
                data.push(l[i]);
                data.push(r[i]);
            }
            t += BLOCK as f64 / SR;
        }
    }
    write_wav(&out, &data);
    let peak = data.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    println!("{out}: {turns} turns, {:.1} s, peak {peak:.3}", t);
}

fn write_wav(path: &str, data: &[f32]) {
    let mut b = Vec::with_capacity(44 + data.len() * 2);
    let bytes = (data.len() * 2) as u32;
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
    for x in data {
        let s = (x.clamp(-1.0, 1.0) * 32767.0) as i16;
        b.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, b).expect("write the wav");
}
