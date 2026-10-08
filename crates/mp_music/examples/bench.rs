//! `cargo run --release -p mp_music --example bench`: the station player's
//! cost, as seconds of audio rendered per second of CPU, per station.
use mp_music::player::Player;
use mp_music::radio::{EPOCH, STATIONS, wall_parts};
use std::time::Instant;

fn main() {
    let sr = 48000.0;
    for (i, st) in STATIONS.iter().enumerate() {
        let mut p = Player::new(2026, sr);
        let (day, sec) = wall_parts(EPOCH + 86400.0 * 11.0 + 300.0);
        p.set_params(i as f32, day as f32, sec as f32, 1.0, 1.0);
        let mut l = vec![0.0f32; 128];
        let mut r = vec![0.0f32; 128];
        let secs = 60.0;
        let blocks = (secs * sr / 128.0) as usize;
        let t0 = Instant::now();
        for _ in 0..blocks {
            p.process(&mut l, &mut r);
        }
        let dt = t0.elapsed().as_secs_f64();
        println!(
            "{:16} {:5.1} s of audio in {:6.3} s: {:5.1} % of one core",
            st.name,
            secs,
            dt,
            100.0 * dt / secs
        );
    }
}
