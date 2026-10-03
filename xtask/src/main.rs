//! `cargo xtask <command>`: build, parity and size tooling (SPEC 11).
//!
//! Commands:
//!   check-deps        enforce the crate dependency rules (SPEC 3.2)
//!   web [--release]   build the web client into dist/next/
//!   size              report the size of the built wasm (raw and gzip)
//!   kernel [--check]  build the math kernel's wasm for the JS oracle; check its bits

mod deps;
mod kernel;
mod size;
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
     \x20 kernel [--check]  build the math kernel's wasm for the JS oracle; check its bits"
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
