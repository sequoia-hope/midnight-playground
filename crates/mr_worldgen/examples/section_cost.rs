//! Times each job of a level build (or, with LEN=<m> [RT=, RS=], of a menu
//! section of it, D741) and sizes its scene: `cargo run --release -p
//! mr_worldgen --example section_cost -- sierra coast` from the repo root.
use std::sync::Arc;
use std::time::Instant;

use mr_levels::{SeasideData, level_by_id, seaside};
use mr_worldgen::scenery::scenery_factory;
use mr_worldgen::stages::{LevelSetup, level_stages};
use mr_worldgen::terrain_mesh::{TerrainSetup, seaside_ground_color};
use mr_worldgen::world::{Build, World, level_jobs};

fn main() {
    let survey =
        Arc::new(SeasideData::parse(&std::fs::read("assets/seaside/survey.bin").unwrap()).unwrap());
    let args: Vec<String> = std::env::args().skip(1).collect();
    let env = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<f64>().ok());
    let sec_len = env("LEN");
    for id in args {
        let mut l = level_by_id(&id);
        if id == "seaside" {
            seaside::prepare(&mut l, survey.clone());
        }
        let mut w = World::new(l);
        if id == "seaside" {
            w = w.with_level_data(survey.clone());
        }
        let setup = LevelSetup {
            terrain: TerrainSetup {
                plan: None,
                ground_color: (id == "seaside").then(|| seaside_ground_color(survey.clone())),
                ..TerrainSetup::default()
            },
            road: None,
        };
        if let Some(len) = sec_len {
            let t = mr_track::Track::new(&w.level).unwrap();
            let s0 = t.start_s + 60.0 - 40.0;
            println!(
                "  start_s {:.0} zone0 {:.0}..{:.0} length {:.0} loop {}",
                t.start_s, t.zones[0].s0, t.zones[0].s1, t.length, t.is_loop
            );
            w.section = Some(mr_worldgen::section::Section {
                s0,
                s1: s0 + len,
                terrain_radius: env("RT").unwrap_or(3000.0),
                scenery_radius: env("RS").unwrap_or(1500.0),
            });
        }
        let mut b = Build::new(w, level_jobs(level_stages(setup), scenery_factory(None)));
        let t0 = Instant::now();
        let mut last = String::new();
        let mut acc = 0.0;
        while let Some((label, _)) = b.progress() {
            let label = label.to_string();
            let t = Instant::now();
            b.step().unwrap();
            let dt = t.elapsed().as_secs_f64();
            if label != last && !last.is_empty() {
                println!("  {last:<28} {acc:7.3} s");
                acc = 0.0;
            }
            acc += dt;
            last = label;
        }
        println!("  {last:<28} {acc:7.3} s");
        let tc = Instant::now();
        let st = mr_worldgen::section::cut(&mut b.world);
        println!("  cut {:?} in {:.3} s", st, tc.elapsed().as_secs_f64());
        let wb = b.finish();
        let tex: usize = wb
            .scene
            .textures
            .iter()
            .map(|t| (t.width * t.height * t.channels) as usize)
            .sum();
        println!(
            "  textures {:.1} MB (shared counted twice), log {:?}",
            tex as f64 / 1e6,
            wb.log
        );
        let s = &wb.scene;
        if std::env::var("TOP").is_ok() {
            let mut per: Vec<(usize, String)> = s
                .nodes
                .iter()
                .filter_map(|n| {
                    let m = &s.meshes[n.mesh? as usize];
                    let mut b: usize = m
                        .attributes
                        .iter()
                        .map(|a| {
                            let d = &s.buffers[a.accessor as usize].data;
                            d.len() * d.component().size()
                        })
                        .sum();
                    if let Some(i) = m.index {
                        let d = &s.buffers[i as usize].data;
                        b += d.len() * d.component().size();
                    }
                    let p = n
                        .parent
                        .map(|p| s.nodes[p as usize].name.clone())
                        .unwrap_or_default();
                    Some((b, format!("{} / {} ({})", p, n.name, m.name)))
                })
                .collect();
            per.sort_by_key(|a| std::cmp::Reverse(a.0));
            for (b, n) in per.iter().take(15) {
                println!("    {:6.2} MB {n}", *b as f64 / 1e6);
            }
        }
        let bytes: usize = s
            .buffers
            .iter()
            .map(|b| b.data.len() * b.data.component().size())
            .sum();
        println!(
            "{id}: {:.2} s, {} nodes, {} meshes, {} mats, {} tex, {:.1} MB buffers, {} animators",
            t0.elapsed().as_secs_f64(),
            s.nodes.len(),
            s.meshes.len(),
            s.materials.len(),
            s.textures.len(),
            bytes as f64 / 1e6,
            wb.animators.len()
        );
    }
}
