//! Where a streamed section's time goes, job by job (D1215's measure):
//! `cargo test --release -p mp_worldgen --test section_timing -- --ignored --nocapture`.
mod common;
use std::sync::Arc;
use std::time::Instant;

use mp_worldgen::section;
use mp_worldgen::stages::LevelSetup;
use mp_worldgen::stream::Planned;
use mp_worldgen::terrain_mesh::TerrainSetup;

#[test]
#[ignore]
fn timing() {
    let setup = LevelSetup {
        terrain: TerrainSetup {
            plan: None,
            ..TerrainSetup::default()
        },
        road: None,
    };
    let t0 = Instant::now();
    let p = Planned::new(
        common::level("peninsula"),
        None,
        setup,
        Arc::new(mp_worldgen::scenery::open_world),
        Arc::new(|_| Vec::new()),
    )
    .unwrap();
    eprintln!(
        "planned {:.2} s, {} sections",
        t0.elapsed().as_secs_f64(),
        p.sections()
    );
    for k in [1i64, 6, 9, 14, 20, 27, 33] {
        let mut b = p.build(p.section(k));
        let ts = Instant::now();
        while !b.is_done() {
            let label = b.progress().map(|(l, _)| l.to_string()).unwrap_or_default();
            let t = Instant::now();
            b.step().unwrap();
            eprintln!(
                "  {k:3} {label:28} {:7.1} ms",
                t.elapsed().as_secs_f64() * 1e3
            );
        }
        let (wb, stats) = section::finish(b);
        let bytes = section::pack(&wb, stats).unwrap();
        eprintln!(
            "section {k}: {:.2} s, {} animators, packed {} kB",
            ts.elapsed().as_secs_f64(),
            wb.animators.len(),
            bytes.len() / 1024
        );
    }
}
