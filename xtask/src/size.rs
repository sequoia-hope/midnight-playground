//! `cargo xtask size [--budget]`: the size of the built wasm, raw and after
//! gzip (SPEC 6.6: under 10 MB after gzip). Reads `dist/next/`, so run
//! `cargo xtask web --release` first. In GitHub Actions the table is also
//! written to the job summary.

use crate::{Result, root};
use flate2::Compression;
use flate2::write::GzEncoder;
use std::io::Write;

const BUDGET_GZIP: u64 = 10 * 1024 * 1024;

pub fn run(args: &[String]) -> Result {
    let enforce = args.iter().any(|a| a == "--budget");
    let dir = root().join("dist").join("next");
    let build = std::fs::read_to_string(dir.join("build.json"))
        .map_err(|_| "no build in dist/next/; run `cargo xtask web --release` first".to_owned())?;
    let profile = if build.contains("web-release") {
        "release"
    } else {
        "dev (not representative)"
    };

    let mut rows = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|e| format!("reading {}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "wasm" || e == "js"))
        .collect();
    entries.sort();
    for path in entries {
        let bytes = std::fs::read(&path).map_err(|e| format!("reading {}: {e}", path.display()))?;
        let mut gz = GzEncoder::new(Vec::new(), Compression::best());
        gz.write_all(&bytes).map_err(|e| e.to_string())?;
        let gz = gz.finish().map_err(|e| e.to_string())?.len() as u64;
        rows.push((
            path.file_name().unwrap().to_string_lossy().into_owned(),
            bytes.len() as u64,
            gz,
        ));
    }
    if rows.is_empty() {
        return Err("no .wasm or .js files in dist/next/".into());
    }

    let mut table = String::from("| File | Raw | Gzip |\n|---|---:|---:|\n");
    for (name, raw, gz) in &rows {
        table += &format!("| {name} | {} | {} |\n", human(*raw), human(*gz));
    }
    let wasm_gz: u64 = rows
        .iter()
        .filter(|r| r.0.ends_with(".wasm"))
        .map(|r| r.2)
        .sum();
    let verdict = format!(
        "wasm after gzip: {} of the {} budget ({profile} build)",
        human(wasm_gz),
        human(BUDGET_GZIP)
    );
    println!("{table}\n{verdict}");

    if let Ok(summary) = std::env::var("GITHUB_STEP_SUMMARY") {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(summary)
            .map_err(|e| format!("writing the job summary: {e}"))?;
        writeln!(f, "### Web build size\n\n{table}\n{verdict}\n").map_err(|e| e.to_string())?;
    }

    if enforce && wasm_gz > BUDGET_GZIP {
        return Err(format!("over budget: {verdict}"));
    }
    Ok(())
}

fn human(n: u64) -> String {
    if n >= 1024 * 1024 {
        format!("{:.2} MB", n as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} KB", n as f64 / 1024.0)
    }
}
