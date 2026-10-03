# Midnight Racer: working rules for agents

Two games live in this repo:

- **The JS game** (repo root: `index.html`, `src/`, `vendor/`, `test/`). It is
  what GitHub Pages serves from `main`, and it is **frozen** except for bug
  fixes the owner reports, and parity hooks during roadmap M0. After the
  `js-reference` tag, hooks are closed too.
- **The Rust port** (`Cargo.toml`, `crates/`, `xtask/`, `tools/parity/`,
  `parity/`). The plan is `docs/rust-port/SPEC.md` (what and how it is
  judged) and `docs/rust-port/ROADMAP.md` (order, work packages, gates). Read
  both before starting a work package, plus the inventory section for the JS
  you are porting.

## Principles (SPEC 1.1)

1. **The JS game is the oracle.** Parity is measured by tests, numeric
   traces, geometry digests and side-by-side pictures, not by "looks right".
2. **Port, don't improve.** Same structure, names, constants, order of
   operations and order of random draws as the JS. Keep the comments that
   explain why. A behaviour change is a deviation: record it in
   `docs/rust-port/DEVIATIONS.md` with the reason.
3. **The simulation is a library**: no engine, renderer, clock or global
   state; it steps a plain `SimState` by one fixed tick of 1/120 s.
4. **World generation is a library**: level in, engine-neutral scene data out.
5. **One implementation for every platform**; platform differences sit
   behind small traits.
6. **Deterministic by construction**: fixed tick, seeded streams in the
   state, and every inexact math function through `mr_math`'s kernel. Never
   call `f64::sin` and friends, `powi`, or `mul_add` in the simulation or
   world generation (`check-deps` rejects them).

A choice the spec does not cover: add it to `docs/rust-port/DECISIONS.md`
and carry on with the option that best preserves parity.

## Commands

| What | Command |
|---|---|
| Build and test the workspace | `cargo test --workspace` |
| Lints, as CI runs them | `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings` |
| Crate dependency rules (SPEC 3.2) | `cargo xtask check-deps` |
| Web build into `dist/next/` | `cargo xtask web` (add `--release` for the optimised build) |
| Wasm size report | `cargo xtask size` (after a release web build) |
| A headless race (results, `--trace`, `--state-at`, `bench`) | `cargo run --release -p mr_sim --bin mr-sim -- race --level sierra` |
| Native client | `cargo run -p mr_game` |
| JS unit tests | `npm run test:unit` (and `npm run test:unit:kernel`, with the parity kernel) |
| Rebuild and check the parity kernel | `cargo xtask kernel` (CI: `cargo xtask kernel --check`) |
| JS browser tests | `npm run test:e2e` (headless Chrome on the GPU, ~3 min) |

Toolchain: stable Rust with the `wasm32-unknown-unknown` target
(`rust-toolchain.toml`), `wasm-bindgen-cli` at exactly the version of the
`wasm-bindgen` crate in `Cargo.lock` (`cargo xtask web` checks), and
`wasm-opt` (`cargo install wasm-opt --locked`) for release builds.

## Crate rules (SPEC 3.2)

```
mr_math ← mr_track ← mr_levels ← mr_sim ← mr_net ← mr_host
mr_scene ← mr_worldgen   (also mr_canvas, mr_math, mr_track, mr_levels)
mr_game uses mr_sim, mr_net, mr_scene, mr_worldgen, mr_audio
```

Only `mr_game` depends on Bevy. The simulation crates (`mr_math`,
`mr_track`, `mr_levels`, `mr_sim`) have `#![forbid(unsafe_code)]` and no
clock, threads, `rand`, or hash maps. `xtask/src/deps.rs` holds the rule for
every crate; a new crate needs one there.

## JS semantics to keep (SPEC 4.2)

`Math.sign(-0)` is -0; `Math.round` is floor(x + 0.5) with its edge cases;
`Math.max`/`min` propagate NaN and order the zeros; `x || d` treats 0 as
missing; `Array.prototype.sort` is stable (`sort_by`, never
`sort_unstable_by`); round to `f32` wherever the JS stores into a
`Float32Array`; iterate in JS insertion order, never a sorted or hashed map.
When a trace diverges, it is a port bug until shown otherwise: find the first
differing field at the first differing tick.

## Parity data

`parity/README.md` explains the layout (`parity/golden/` in git,
`parity/cache/<js-tree-key>/` regenerated) and the hooks in `src/parity/`
that the JS game gains for its reference runs (`?kernel=1`, `?fixeddt=1`,
`?quant=1`, `?seed=N`, or `?parity=1` for all). Every capture is taken with
the kernel on.

## Viewing output

Only through the registered server: `proj up midnight-racer` (or
`./serve.sh`). It serves the repo root, so the Rust build is at `/dist/next/`
and parity reports at `/parity/report/`. Phones and other machines use the
tailnet https front (the registry's `web_url` for this project, plus
`dist/next/`), which is a secure context, so WebGPU and tilt work there.

**Never start another server, and never write a port number** into a script,
config, default or doc. A program that needs a port takes `--port`, then
`$PORT`, then `proj port`, then fails loudly. Every URL the client uses is
relative, because the game is always served under a sub-path.

## Working on a package (ROADMAP "Working rules")

1. Read the JS files named in the package in full, and the inventory section.
2. Write the parity test first where a golden exists; port until it passes.
3. Check your own work: run the tests, render headless, look at the result,
   compare with the reference.
4. Commit and push straight to `main` (no branches): imperative subject, a
   prose body. Build output (`dist/`, `target/`, `parity/report/`) stays out
   of git. GitHub Pages keeps serving the JS game until cutover (M9); never
   deploy the Rust build there or change the Pages source.

**Parallel work.** Packages run in parallel only with disjoint files. A
shared module has one owner; everyone else may add exports to it but not
change what exists.
