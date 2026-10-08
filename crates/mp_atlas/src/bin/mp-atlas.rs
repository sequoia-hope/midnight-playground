//! `mp-atlas`: the open world's plan, measured over the real ground.
//!
//!   cargo run -p mp_atlas -- report [world]        the plan, measured, as text
//!   cargo run -p mp_atlas -- resolve [world]       writes assets/atlas/<world>/resolved.json
//!   cargo run -p mp_atlas -- profile <id> [world]  a route's or event's profile as CSV
//!
//! The world defaults to `peninsula`.

use mp_atlas::Atlas;
use mp_atlas::report::{json, measure, text};
use std::process::ExitCode;

fn run(args: &[String]) -> Result<(), String> {
    let cmd = args.first().map(String::as_str).unwrap_or("report");
    let world_at = |i: usize| args.get(i).map(String::as_str).unwrap_or("peninsula");
    match cmd {
        "report" | "resolve" => {
            let dir = Atlas::dir(world_at(1));
            let a = Atlas::load(&dir)?;
            let m = measure(&a)?;
            if cmd == "report" {
                print!("{}", text(&a, &m));
            } else {
                let out = dir.join("resolved.json");
                let s = serde_json::to_string(&json(&a, &m)).map_err(|e| e.to_string())? + "\n";
                std::fs::write(&out, s).map_err(|e| format!("{}: {e}", out.display()))?;
                eprintln!("wrote {}", out.display());
            }
            Ok(())
        }
        "profile" => {
            let id = args.get(1).ok_or("profile needs a route or event id")?;
            let a = Atlas::load(&Atlas::dir(world_at(2)))?;
            let m = measure(&a)?;
            let r = m
                .routes
                .iter()
                .find(|r| r.id == *id)
                .ok_or_else(|| format!("no route or event `{id}`"))?;
            println!("d_m,lat,lon,h_m,cover");
            for s in &r.profile.samples {
                let ll = a.geo.projection.to_latlon(s.at);
                println!(
                    "{:.0},{:.6},{:.6},{:.1},{}",
                    s.d,
                    ll.lat,
                    ll.lon,
                    s.h,
                    a.terrain.cover_at(s.at).name()
                );
            }
            Ok(())
        }
        other => Err(format!(
            "unknown command `{other}`: report, resolve or profile <id> [world]"
        )),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mp-atlas: {e}");
            ExitCode::FAILURE
        }
    }
}
