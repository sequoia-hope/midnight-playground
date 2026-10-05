# Deviations from the JS game

Every place where the Rust port behaves differently from the JS game, on
purpose. A deviation needs a reason. Seeded from SPEC section 13; add a row
when a work package introduces one, with the package and the JS it concerns.

| Deviation | Reason | Where |
|---|---|---|
| Fixed 120 Hz tick with interpolation, for all subsystems | Determinism, multiplayer, RL. JS steps AI and traffic once per frame. | SPEC 4.1 |
| Seeded random streams instead of `Math.random` | Determinism | SPEC 4.3 |
| Per-frame random effects become rates at 60 Hz | Same look at any display rate | SPEC 6.5 |
| Points drawn as instanced quads | No point size in WebGPU | SPEC 6.2 |
| Bundled fonts for generated textures and HUD | Same signs on every device; no web font request | SPEC 5.3 |
| UI drawn in-engine; some CSS effects approximated | One UI for native and web | SPEC 8.1 |
| Native audio through a Rust Web Audio implementation | One graph definition; small differences in compressor and oscillator behaviour | SPEC 7.1 |
| Music player is a screen in the game, not a second page | One wasm app | SPEC 7.3 |
| Seaside survey data in a binary file | No base64-in-JS loader | SPEC 4.4 |
| The menus' selected-tab glow, hover states and the native pickers are approximated (the ring, Chrome's control shapes, a dropdown list) | Bevy UI has no outer-only shadow under translucent nodes, no form controls | WP 6.1, DECISIONS D573 |
| ◂ ▸ read ← →, N₂O reads N2O; ★ ⏭ ♪ are drawn pictures | The bundled faces lack those characters | WP 6.1, D573 |
| The menu is drawn at up to 2× the CSS resolution on phones without High quality | The DOM's text was always sharp; the canvas's at 1× is not | WP 6.2, D575 |
| The music player link opens the JS page | Its Rust screen is M5's | WP 6.2, D573 |
| A tap on no control of the pause screen resumes | The M4 phone flow, kept | WP 6.2, D578 |
| The logo's gradients and glow are flat colours per line and letter | No gradient text in Bevy UI; drawing it in software cost 0.3 MB of wasm | WP 6.1, D573 |
| A level tab only selects: the menu flies over a simplified view of that level (its land, road, sky, time of day and sea, with invented stand-ins for its scenery: trees, poles, lit blocks, hoodoos, barriers, a skyline; the first 500 m from `startS + 60`, starting over at the end), prepared for every level when the menu opens; Race builds the level whole behind the loading screen (`main.js` `loadLevel` rebuilds the whole world on a tab, and the attract camera drifts along the whole first zone with the real scenery). Main menu after a race keeps the raced level for its tab, as the JS does | The owner (D676, D746): instant switching, no full level per tab, simplified views | D676, D740–D750 |

## Known JS quirks reproduced, not fixed

Unless the owner says otherwise (SPEC 15, question 2):

- All headlight pools share one material, so one car's opacity wins
  (`Effects.js:249,273`).
- The shadow box is said to snap to texels but does not (`Sky.js:271`).
- `input.enabled` is never set false.

## Dropped tests

Ported unit tests that assert on README text are dropped (SPEC 4.6). List
each here when M1 ports the tests.

| Test | JS file | Why |
|---|---|---|
| README: level lengths and rival counts match the data | `levels.test.js` | Asserts on README text (WP 1.2) |
| README: the car table lists every car (its performance claims are kept) | `physics.test.js` | Asserts on README text (WP 1.3) |
