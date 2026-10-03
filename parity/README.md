# Parity data

What the Rust port is compared against (SPEC 4.6, 5.7, 7.5, 12). Everything
here is produced from the JS game by the tools in `tools/parity/`, with the
parity hooks on (`src/parity/`).

| Path | In git | What |
|---|---|---|
| `golden/` | yes | Small goldens: simulation traces of staged scenarios, digests, audio arrays, the kernel. Regenerating one must give an identical file. |
| `cache/<key>/` | no | Large outputs regenerated on demand: whole-race recordings, scene exports, Track and terrain dumps, screenshots. `<key>` is a hash of the JS tree (`node tools/parity/lib/jstree.mjs`), so a changed game never reuses a stale capture. |
| `report/` | no | The comparison report (`cargo xtask parity ...`), viewed through the registered server at `/parity/report/`. |
| `trace-format.md` | yes | The trace record both sides produce (WP 0.4). |
| `scenarios.md` | yes | The catalogue of staged simulation scenarios (WP 0.4). |
| `golden/world/` | yes | Per level: SHA-256 of every Track array and of the terrain heights at 10,000 points, and the scene's counts, material kinds and digest hash; `models.json` likewise (WP 0.5). |
| `cache/<key>/scenes/` | no | `<level>.mrscene` and `models.mrscene` scene exports (`crates/mr_scene/FORMAT.md`), each with the digest of the live scene; `<level>.base.mrscene` with `--base` (WP 0.5). |
| `cache/<key>/world/<level>/` | no | `track.{json,bin}` and `terrain.{json,bin}`: the Track arrays and terrain heights (WP 0.5). |

## The hooks

`src/parity/hooks.js` is the first module the game loads. Each hook changes
nothing unless its URL parameter is given:

| Parameter | Effect |
|---|---|
| `kernel=1` | Every inexact `Math` function comes from `mr_math`'s kernel, compiled to wasm (`tools/parity/kernel/mr_kernel.wasm`, rebuilt by `cargo xtask kernel`). Any other inexact one throws. |
| `fixeddt=1` | The race steps in fixed ticks of 1/120 s, `ticks=N` per rendered frame (default 2). Game time follows the ticks, not the clock. |
| `quant=1` | The player's input is quantised as the Rust `InputFrame` is. |
| `seed=N` | Rivals, police and the pursuit draw from seeded streams (`src/parity/sim.js`). |
| `parity=1` | All of the above, seed 1 unless `seed` is given. |
| `freeze=1` | Scenery animation holds still (`world.update` gets dt 0), for scene exports and screenshots. Not part of `parity=1`. |

In Node, `NODE_OPTIONS=--import=./tools/parity/kernel/register.mjs` installs
the kernel before anything runs (`npm run test:unit:kernel`).

Every capture is taken with the kernel on.

## Scene export (WP 0.5)

```
node tools/parity/scene-export.mjs            # all six levels and the models, ~1 min
node tools/parity/scene-export.mjs --base     # terrain, road and sky only
node tools/parity/scene-export.mjs --check    # again, and compare with the goldens and the last run
cargo xtask parity scene-check                # read every scene back with mr_scene, check its digest
```

Each JS material is tagged with its `MaterialKind` (`material.userData.kind`);
the exporter refuses a custom material without one. The export seeds
`Math.random` and uses a fresh Chrome per level, so two runs give identical
files (DECISIONS D20 to D29).
