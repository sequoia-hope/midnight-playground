//! `cargo xtask <command>`: build, parity and size tooling (SPEC 11).
//!
//! Commands:
//!   check-deps        enforce the crate dependency rules (SPEC 3.2)
//!   web [--release]   build the web client into dist/next/
//!   size              report the size of the built wasm (raw and gzip)
//!   kernel [--check]  build the math kernel's wasm for the JS oracle; check its bits
//!   parity shots      compare two sets of screenshots (CIEDE2000), write a report
//!   parity scene-check [files]  read exported scenes back and check their digests
//!   parity materials  the material test scenes, JS and Rust, compared (WP 2.3)
//!   parity stations   the screenshot stations, JS and Rust, compared (WP 2.5)

mod deps;
mod kernel;
mod materials;
mod parity_scene;
mod shots;
mod size;
mod stations;
mod web;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// An error that ends the command with a message and a non-zero exit.
type Result<T = ()> = std::result::Result<T, String>;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else {
        eprintln!("{}", usage());
        return ExitCode::FAILURE;
    };
    let rest = &args[1..];
    let result = match cmd.as_str() {
        "check-deps" => deps::run(rest),
        "web" => web::run(rest),
        "size" => size::run(rest),
        "kernel" => kernel::run(rest),
        "parity" => match rest.first().map(String::as_str) {
            Some("shots") => shots::run(&rest[1..]),
            Some("scene-check") => parity_scene::run(&rest[1..]),
            Some("materials") => materials::run(&rest[1..]),
            Some("stations") => stations::run(&rest[1..]),
            _ => Err(format!("parity: which comparison?\n\n{}", usage())),
        },
        "-h" | "--help" | "help" => {
            println!("{}", usage());
            Ok(())
        }
        other => Err(format!("unknown command `{other}`\n\n{}", usage())),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask {cmd}: {e}");
            ExitCode::FAILURE
        }
    }
}

fn usage() -> &'static str {
    "usage: cargo xtask <command>\n\
     \n\
     commands:\n\
     \x20 check-deps        enforce the crate dependency rules (SPEC 3.2)\n\
     \x20 web [--release]   build the web client into dist/next/\n\
     \x20 size [--budget]   report the built wasm's size; --budget fails over 10 MB gzip\n\
     \x20 kernel [--check]  build the math kernel's wasm for the JS oracle; check its bits\n\
     \x20 parity shots --a <dir> --b <dir> [--label name]\n\
     \x20                   compare two sets of screenshots; report in parity/report/\n\
     \x20 parity scene-check [files]  read exported .mrscene files back and check their digests\n\
     \x20 parity materials [--only all|every|<names>] [--js-run a] [--rerun-js]\n\
     \x20                   the material test scenes rendered by the JS and the Rust client, compared\n\
     \x20 parity stations [--levels a,b] [--js-run a] [--rerun-js] [--label name]\n\
     \x20                 [--rust-run rust] [--base] [--gate level/name.png,...]\n\
     \x20                   the screenshot stations of the JS and the Rust client, compared;\n\
     \x20                   --base: terrain, road and sky only (WP 2.4's gate on Sierra)"
}

/// The repository root (the workspace root).
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level below the workspace root")
        .to_path_buf()
}

/// Runs a command in the repo root, failing on a non-zero exit.
fn exec(cmd: &mut Command) -> Result {
    let status = cmd
        .current_dir(root())
        .status()
        .map_err(|e| format!("could not run {cmd:?}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{cmd:?} failed ({status})"))
    }
}

/// Runs a command in the repo root and returns its stdout.
fn output(cmd: &mut Command) -> Result<String> {
    let out = cmd
        .current_dir(root())
        .output()
        .map_err(|e| format!("could not run {cmd:?}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{cmd:?} failed ({}):\n{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    String::from_utf8(out.stdout).map_err(|e| format!("{cmd:?}: output is not UTF-8: {e}"))
}

fn cargo() -> Command {
    Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
}
