//! `cargo xtask parity shots --a <dir> --b <dir> [--label name]`: compare two
//! sets of screenshots station by station (SPEC 12, roadmap WP 0.6).
//!
//! The metric is Playground Kit's `pk_xtask::imgdiff` (D1233): both images
//! box-filtered to quarter resolution in linear light, CIELAB, CIEDE2000
//! per pixel; a station passes when the mean difference is under 3 and
//! 95 % of its 16-pixel blocks are under 6. The JS game compared with
//! itself gives the noise floor.
//!
//! `<dir>` holds one directory per level of PNGs, as `tools/parity/shots.mjs`
//! writes them. The report (`parity/report/shots-<label>/`, git-ignored) shows
//! each pair with its difference map, worst first, and is viewed through the
//! registered server at `/parity/report/`.

use crate::{Result, root};
pub use pk_xtask::imgdiff::{BLOCK_LIMIT, MEAN_LIMIT};
use std::path::PathBuf;

pub fn run(args: &[String]) -> Result {
    let opt = |k: &str| pk_xtask::opt(args, k);
    let a = PathBuf::from(opt("--a").ok_or("--a <dir> is required")?);
    let b = PathBuf::from(opt("--b").ok_or("--b <dir> is required")?);
    let label = opt("--label").unwrap_or_else(|| "compare".into());
    let root = root();
    let a = if a.is_absolute() { a } else { root.join(a) };
    let b = if b.is_absolute() { b } else { root.join(b) };
    let out = root.join("parity/report").join(format!("shots-{label}"));
    let s = pk_xtask::imgdiff::compare_dirs(&a, &b, &label, &out)?;
    println!(
        "shots {label}: {} stations, {} over the limits; worst mean ΔE00 {:.3}, worst 95th-percentile block {:.3}",
        s.pictures, s.failing, s.worst_mean, s.worst_block95
    );
    println!(
        "report: parity/report/shots-{label}/index.html (on the registered server at /parity/report/)"
    );
    Ok(())
}
