//! `cargo xtask check-deps`: the dependency rules of SPEC 3.2, checked from
//! `cargo tree` and from the crates' sources.
//!
//! Every workspace member needs a rule here, so a new crate cannot slip in
//! unchecked.

use crate::{Result, cargo, output, root};
use std::collections::BTreeSet;
use std::path::Path;

struct Rule {
    krate: &'static str,
    /// Workspace crates this one may depend on directly.
    allowed: &'static [&'static str],
    /// Crates that must not appear anywhere in its normal dependency tree,
    /// for any target, with default features. A trailing `*` is a prefix.
    banned: &'static [&'static str],
    /// Simulation rules: `#![forbid(unsafe_code)]`, and no clock, threads or
    /// hash maps in the source (SPEC 3.2, 4.2).
    pure: bool,
    /// No float methods that call the platform's math library (`x.sin()`,
    /// `f64::exp`, `mul_add`, ...): the kernel in `mr_math` is used instead
    /// (SPEC 4.2, 5.4).
    exact_math: bool,
}

/// No engine, no browser, no ambient randomness, no threads.
const SIM_BANNED: &[&str] = &[
    "bevy*",
    "wgpu*",
    "web-sys",
    "js-sys",
    "wasm-bindgen*",
    "rand*",
    "getrandom",
    "rayon*",
    "crossbeam*",
];
/// World generation: no engine, no GPU; rayon only behind a non-default
/// feature, so it must be absent from the default tree.
const WORLD_BANNED: &[&str] = &["bevy*", "wgpu*", "rayon*"];
const NO_ENGINE: &[&str] = &["bevy*", "wgpu*"];
const NO_BEVY: &[&str] = &["bevy*"];

const RULES: &[Rule] = &[
    Rule {
        krate: "mr_math",
        allowed: &[],
        banned: SIM_BANNED,
        pure: true,
        exact_math: true,
    },
    Rule {
        krate: "mr_track",
        allowed: &["mr_math"],
        banned: SIM_BANNED,
        pure: true,
        exact_math: true,
    },
    Rule {
        krate: "mr_levels",
        allowed: &["mr_math", "mr_track"],
        banned: SIM_BANNED,
        pure: true,
        exact_math: true,
    },
    Rule {
        krate: "mr_sim",
        allowed: &["mr_math", "mr_track", "mr_levels"],
        banned: SIM_BANNED,
        pure: true,
        exact_math: true,
    },
    Rule {
        krate: "mr_scene",
        allowed: &[],
        banned: NO_ENGINE,
        pure: false,
        exact_math: false,
    },
    Rule {
        krate: "mr_canvas",
        allowed: &["mr_math"],
        banned: WORLD_BANNED,
        pure: false,
        exact_math: false,
    },
    Rule {
        krate: "mr_worldgen",
        allowed: &["mr_scene", "mr_canvas", "mr_math", "mr_track", "mr_levels"],
        banned: WORLD_BANNED,
        pure: false,
        exact_math: true,
    },
    Rule {
        krate: "mr_audio",
        allowed: &["mr_math"],
        banned: NO_BEVY,
        pure: false,
        exact_math: false,
    },
    Rule {
        krate: "mr_net",
        allowed: &["mr_math", "mr_track", "mr_levels", "mr_sim"],
        banned: NO_BEVY,
        pure: false,
        exact_math: false,
    },
    Rule {
        krate: "mr_host",
        allowed: &["mr_math", "mr_track", "mr_levels", "mr_sim", "mr_net"],
        banned: NO_BEVY,
        pure: false,
        exact_math: false,
    },
    Rule {
        krate: "mr_game",
        allowed: &[
            "mr_math",
            "mr_track",
            "mr_levels",
            "mr_sim",
            "mr_net",
            "mr_scene",
            "mr_canvas",
            "mr_worldgen",
            "mr_audio",
        ],
        banned: &[],
        pure: false,
        exact_math: false,
    },
    // The kernel's wasm build for the JS oracle (WP 0.3): mr_math only.
    Rule {
        krate: "mr_kernel",
        allowed: &["mr_math"],
        banned: SIM_BANNED,
        pure: false,
        exact_math: true,
    },
    Rule {
        krate: "xtask",
        allowed: &["*"],
        banned: &[],
        pure: false,
        exact_math: false,
    },
];

/// Float methods that are not exact on every platform, plus `mul_add`, which
/// changes rounding.
const INEXACT: &[&str] = &[
    "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sin_cos", "exp", "exp2", "exp_m1", "ln",
    "ln_1p", "log", "log2", "log10", "powf", "powi", "sinh", "cosh", "tanh", "asinh", "acosh",
    "atanh", "hypot", "cbrt", "mul_add",
];

/// Source tokens the pure crates must not use.
const IMPURE: &[&str] = &[
    "std::time",
    "std::thread",
    "HashMap",
    "HashSet",
    "Instant",
    "SystemTime",
];

pub fn run(_args: &[String]) -> Result {
    let members = members()?;
    let mut problems = Vec::new();

    for m in &members {
        if !RULES.iter().any(|r| r.krate == m) {
            problems.push(format!(
                "{m}: workspace member with no rule in xtask/src/deps.rs"
            ));
        }
    }

    for rule in RULES.iter().filter(|r| members.contains(r.krate)) {
        check_tree(rule, &members, &mut problems)?;
        if rule.pure || rule.exact_math {
            check_source(rule, &mut problems)?;
        }
    }

    if problems.is_empty() {
        println!("check-deps: {} crates follow the rules", members.len());
        Ok(())
    } else {
        for p in &problems {
            eprintln!("  {p}");
        }
        Err(format!("{} violation(s)", problems.len()))
    }
}

fn members() -> Result<BTreeSet<String>> {
    let json = output(cargo().args(["metadata", "--no-deps", "--format-version", "1"]))?;
    let meta: serde_json::Value =
        serde_json::from_str(&json).map_err(|e| format!("cargo metadata: {e}"))?;
    Ok(meta["packages"]
        .as_array()
        .ok_or("cargo metadata: no packages")?
        .iter()
        .filter_map(|p| p["name"].as_str().map(str::to_owned))
        .collect())
}

/// Crate names in `cargo tree` output, one per line, first token.
fn tree(krate: &str, depth: Option<u32>) -> Result<Vec<String>> {
    let mut cmd = cargo();
    cmd.args([
        "tree", "-p", krate, "-e", "normal", "--target", "all", "--prefix", "none",
    ]);
    if let Some(d) = depth {
        cmd.args(["--depth", &d.to_string()]);
    }
    let text = output(&mut cmd)?;
    Ok(text
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .map(str::to_owned)
        .collect())
}

fn check_tree(rule: &Rule, members: &BTreeSet<String>, problems: &mut Vec<String>) -> Result {
    if !rule.allowed.contains(&"*") {
        for dep in tree(rule.krate, Some(1))?.iter().skip(1) {
            if members.contains(dep) && !rule.allowed.contains(&dep.as_str()) {
                problems.push(format!(
                    "{}: may not depend on {dep} (SPEC 3.2)",
                    rule.krate
                ));
            }
        }
    }
    let all: BTreeSet<String> = tree(rule.krate, None)?.into_iter().skip(1).collect();
    for dep in &all {
        if rule.banned.iter().any(|b| matches(b, dep)) {
            problems.push(format!(
                "{}: {dep} is in its dependency tree (banned)",
                rule.krate
            ));
        }
    }
    Ok(())
}

fn matches(pattern: &str, name: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => name == pattern,
    }
}

fn check_source(rule: &Rule, problems: &mut Vec<String>) -> Result {
    let dir = root().join("crates").join(rule.krate).join("src");
    if !dir.is_dir() {
        // Tooling crates outside crates/ have no source rules.
        return Ok(());
    }
    if rule.pure {
        let lib = std::fs::read_to_string(dir.join("lib.rs"))
            .map_err(|e| format!("{}: reading lib.rs: {e}", rule.krate))?;
        if !lib.contains("#![forbid(unsafe_code)]") {
            problems.push(format!(
                "{}: lib.rs lacks #![forbid(unsafe_code)]",
                rule.krate
            ));
        }
    }
    let mut files = Vec::new();
    rust_files(&dir, &mut files)?;
    for file in files {
        let text = std::fs::read_to_string(&file)
            .map_err(|e| format!("reading {}: {e}", file.display()))?;
        let rel = file
            .strip_prefix(root())
            .unwrap_or(&file)
            .display()
            .to_string();
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            if rule.pure {
                for tok in IMPURE {
                    if code.contains(tok) {
                        problems.push(format!("{rel}:{}: `{tok}` in a simulation crate", n + 1));
                    }
                }
            }
            if rule.exact_math {
                for m in INEXACT {
                    if calls_method(code, m) {
                        problems.push(format!(
                            "{rel}:{}: `{m}` calls the platform's math; use mr_math's kernel",
                            n + 1
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

/// `x.sin(` or `f64::sin(` (also `f32::`), but not `libm::sin(` or `kernel::sin(`.
fn calls_method(code: &str, name: &str) -> bool {
    let method = format!(".{name}(");
    let assoc64 = format!("f64::{name}(");
    let assoc32 = format!("f32::{name}(");
    code.contains(&method) || code.contains(&assoc64) || code.contains(&assoc32)
}

fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("reading {}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() {
            rust_files(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns() {
        assert!(matches("bevy*", "bevy_render"));
        assert!(matches("rand*", "rand_core"));
        assert!(!matches("web-sys", "web-sys-extra"));
        assert!(matches("getrandom", "getrandom"));
    }

    #[test]
    fn method_calls() {
        assert!(calls_method("let y = x.sin();", "sin"));
        assert!(calls_method("f64::exp(x)", "exp"));
        assert!(calls_method("a.mul_add(b, c)", "mul_add"));
        assert!(!calls_method("libm::sin(x)", "sin"));
        assert!(!calls_method("kernel::sin(x)", "sin"));
        assert!(!calls_method("x.sinh_like()", "sin"));
    }

    #[test]
    fn every_sim_crate_is_pure() {
        for k in ["mr_math", "mr_track", "mr_levels", "mr_sim"] {
            let r = RULES.iter().find(|r| r.krate == k).unwrap();
            assert!(r.pure && r.exact_math, "{k}");
        }
    }
}
