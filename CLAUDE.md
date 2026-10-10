# Midnight Playground (formerly Midnight Racer): working rules for agents

The game is **Midnight Playground**, in Rust; its crates are `mp_*`
(DECISIONS D1100). The repository is `sequoia-hope/midnight-playground` and
Pages serves the Rust game at https://sequoia-hope.github.io/midnight-playground/
(the cutover, D1112). The JS game keeps the name Midnight Racer and is the
legacy version, under `legacy/` on Pages. The local addresses keep the old
name: the `proj` name and the tailnet path are still `midnight-racer`.

Two games live in this repo:

- **The JS game** (repo root: `index.html`, `src/`, `vendor/`, `test/`). It is
  the legacy version (Pages serves it under `legacy/`), and it is **frozen** except for bug
  fixes the owner reports, and parity hooks during roadmap M0. After the
  `js-reference` tag, hooks are closed too.
- **The Rust port** (`Cargo.toml`, `crates/`, `xtask/`, `tools/parity/`,
  `parity/`). The plan is `docs/rust-port/SPEC.md` (what and how it is
  judged) and `docs/rust-port/ROADMAP.md` (order, work packages, gates). Read
  both before starting a work package, plus the inventory section for the JS
  you are porting.

## Principles (SPEC 1.1)

**Since D1111 (2026-10-07) parity with the JS game is no longer the goal.**
The owner judges the Rust game on its own; principles 1 and 2 below now
apply only as history and as regression checks that still pass. Principles
3 to 6 stand: multiplayer relies on determinism.

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
   state, and every inexact math function through `mp_math`'s kernel. Never
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
| A headless race (results, `--trace`, `--state-at`, `bench`) | `cargo run --release -p mp_sim --bin mp-sim -- race --level sierra` |
| Native client | `cargo run -p mp_game` |
| The open world's plan, measured (`resolve` rewrites what the planner draws) | `cargo run --release -p mp_atlas -- report` |
| A patch over a scale, or a song from a bar with one part soloed, to a wav | `cargo run --release -p mp_music --example render -- patch surfGuitar out.wav` / `-- song chicha 3 out.wav 24 20 lead` |
| Rebuild the atlas from real data (python3 with numpy, Pillow, shapely, pyarrow) | `python3 tools/atlas/build.py` |
| JS unit tests | `npm run test:unit` (and `npm run test:unit:kernel`, with the parity kernel) |
| Rebuild and check the parity kernel | `cargo xtask kernel` (CI: `cargo xtask kernel --check`) |
| JS browser tests | `npm run test:e2e` (headless Chrome on the GPU, ~3 min) |
| The same browser suites against the Rust build | `cargo xtask web --release && npm run test:e2e:rust` (one Chrome at a time; prints the suite table) |

Toolchain: stable Rust with the `wasm32-unknown-unknown` target
(`rust-toolchain.toml`), `wasm-bindgen-cli` at exactly the version of the
`wasm-bindgen` crate in `Cargo.lock` (`cargo xtask web` checks), and
`wasm-opt` (`cargo install wasm-opt --locked`) for release builds.

## Crate rules (SPEC 3.2)

```
mp_math ← mp_track ← mp_levels ← mp_sim ← mp_net ← mp_host
mp_math ← mp_vdyn ← mp_sim   (vehicle dynamics: the sim car's body, tyres, drivetrain)
mp_scene ← mp_worldgen   (also mp_canvas, mp_math, mp_track, mp_levels)
mp_exhaust ← mp_audio   (mp_exhaust has no dependencies; it is also its own wasm)
mp_atlas   (real geography and the open world's plan; no dependencies yet)
mp_game uses mp_sim, mp_net, mp_scene, mp_worldgen, mp_audio
```

Only `mp_game` depends on Bevy. The simulation crates (`mp_math`,
`mp_track`, `mp_levels`, `mp_vdyn`, `mp_sim`) have `#![forbid(unsafe_code)]` and no
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
   of git. `.github/workflows/pages.yml` builds and publishes on every push
   to `main` (D1112): the Rust build at the site root, the JS game's tracked
   files under `legacy/`, and `dist/next/` forwarding to the root. The web
   build is self-contained (`cargo xtask web` copies the fonts, Seaside's
   survey and photo and the radio clips into it), so it works at
   `dist/next/` locally and at the root on Pages: never reach above it with
   `../../` from the Rust page.

**Parallel work.** Packages run in parallel only with disjoint files. A
shared module has one owner; everyone else may add exports to it but not
change what exists.
