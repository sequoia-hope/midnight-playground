//! `cargo xtask kernel [--check]`: build `mp_math`'s kernel to wasm for the
//! JS reference run, and check that the wasm, called through the JS `Math`
//! patch, gives the same bits as native Rust on a million inputs per
//! function (roadmap WP 0.3).
//!
//! Without `--check` the fresh build replaces the committed
//! `tools/parity/kernel/mr_kernel.wasm`. With it, nothing is written: the
//! fresh build and the committed file are both checked (CI).

use crate::{Result, cargo, exec, output, root};
use std::process::Command;

const N: u32 = 1_000_000;

pub fn run(args: &[String]) -> Result {
    let check_only = args.iter().any(|a| a == "--check");
    let root = root();

    exec(cargo().args([
        "build",
        "-p",
        "mr_kernel",
        "--lib",
        "--target",
        "wasm32-unknown-unknown",
        "--profile",
        "web-release",
    ]))?;
    let fresh = root.join("target/wasm32-unknown-unknown/web-release/mr_kernel.wasm");
    let committed = root.join("tools/parity/kernel/mr_kernel.wasm");
    if !check_only {
        std::fs::copy(&fresh, &committed).map_err(|e| format!("copying the kernel: {e}"))?;
        println!(
            "kernel: wrote {}",
            committed.strip_prefix(&root).unwrap().display()
        );
    }

    let native = native_hashes();
    let mut failures = 0;
    let files = if check_only {
        vec![&fresh, &committed]
    } else {
        vec![&committed]
    };
    for wasm in files {
        let js = output(
            Command::new("node")
                .arg("tools/parity/kernel/check.mjs")
                .arg(wasm)
                .arg(N.to_string()),
        )?;
        let js: serde_json::Value =
            serde_json::from_str(js.trim()).map_err(|e| format!("check.mjs output: {e}"))?;
        let label = wasm.strip_prefix(&root).unwrap_or(wasm).display();
        for (name, want) in &native {
            let got = js[name].as_u64();
            if got != Some(*want as u64) {
                failures += 1;
                eprintln!("  {label}: {name}: wasm {got:?}, native {want}");
            }
        }
        println!(
            "kernel: {label}: {} functions × {N} inputs, {}",
            native.len(),
            if failures == 0 {
                "bit-identical to native"
            } else {
                "DIFFERS"
            }
        );
    }
    if failures > 0 {
        return Err(format!(
            "{failures} function(s) differ between wasm and native"
        ));
    }
    Ok(())
}

fn native_hashes() -> Vec<(&'static str, u32)> {
    mr_kernel::FUNCTIONS
        .iter()
        .enumerate()
        .map(|(f, (name, _))| (*name, mr_kernel::native_hash(f, N)))
        .collect()
}
