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
| A "Level viewer" button on the menu's level card, opening the level viewer (`?view=god`), which the JS does not have | The owner's request to review levels from any point of view; the game itself is unchanged | SPEC 8.6, WP 6.9, DECISIONS D679 |
| The menus' selected-tab glow, hover states and the native pickers are approximated (the ring, Chrome's control shapes, a dropdown list) | Bevy UI has no outer-only shadow under translucent nodes, no form controls | WP 6.1, DECISIONS D573 |
| ◂ ▸ read ← →, N₂O reads N2O; ★ ⏭ ♪ are drawn pictures | The bundled faces lack those characters | WP 6.1, D573 |
| The menu is drawn at up to 2× the CSS resolution on phones without High quality | The DOM's text was always sharp; the canvas's at 1× is not | WP 6.2, D575 |
| The music player link opens the JS page | Its Rust screen is M5's | WP 6.2, D573 |
| A tap on no control of the pause screen resumes | The M4 phone flow, kept | WP 6.2, D578 |
| The logo's gradients and glow are flat colours per line and letter | No gradient text in Bevy UI; drawing it in software cost 0.3 MB of wasm | WP 6.1, D573 |
| A barrier hit's sparks fly with the player's velocity after the hit's 3 % slowdown, not before it (`PursuitView.js:200`) | The simulation applies the slowdown at the tick (D61); the sparks are drawn after the ticks | WP 8.2, D942 |
| A level tab only selects: the menu flies over a simplified view of that level (its land, road, sky, time of day and sea, with invented stand-ins for its scenery: trees, poles, lit blocks, hoodoos, barriers, a skyline; the first 500 m from `startS + 60`, starting over at the end), prepared for every level when the menu opens; Race builds the level whole behind the loading screen (`main.js` `loadLevel` rebuilds the whole world on a tab, and the attract camera drifts along the whole first zone with the real scenery). Main menu after a race keeps the raced level for its tab, as the JS does | The owner (D676, D746): instant switching, no full level per tab, simplified views | D676, D740–D750 |
| Hot Pursuit HUD: the hold title's coloured glow, the radio pill's and the bust track's drop shadows are left out; the heat stars' drop shadow is an approximate blur; a radio line too long for its pill is cut at the edge, with no `…` | Bevy UI has no blurred text shadow, draws a box shadow under a translucent node, and has no `text-overflow: ellipsis` | WP 8.3, DECISIONS D962 |
| The bumper view does not draw the player's own car while the eye is inside it (the JS draws the inside of the body: the GT's wheel-arch liner fills the lower half of the view) | The owner's bug report; the same JS change is proposed, until it is made | D1040 |
| The headlight pools lie 1 cm over the road at their centre, tilted with it, without the polygon offset's slope term (the JS's are level quads 6 cm over their car's height, drawn over the bottom of nearby cars' tyres) | The owner's bug report (tyres sunk into the road at the start); the same JS change is proposed, until it is made | D1042 |
| A debug overlay (`debug=1`, F3): frame times over 5 s, 30 s and since it was turned on, with graphs, and the client's state; the JS has only its `?stats=1` panel | The owner's request, 2026-10-05 | DECISIONS D1020 |
| A run recording (`record=1`): a JSON-lines log of the session with every race's per-tick inputs, replayable headless by `mr-sim replay` | The owner's request, 2026-10-05 | DECISIONS D1021, `RECORDING.md` |
| The native menu ends with a Quit button (leaves the game as Ctrl+Q does); the web page has none | The owner's request (2026-10-05); a page cannot close itself | DECISIONS D1060 |
| The Ion Arc's motor whine is 7 dB quieter pulling away and 18 dB quieter from 60 % of its top speed; its wind is up to 7 dB louder (road rumble half that), rising with speed | The owner's request (2026-10-05): the whine was "really annoying at full speed"; EVs in other games are near silent with the wind played up. Other cars unchanged | DECISIONS D1061 |
| A guide line on the road (Full, Braking only, Off; Full by default on touch screens, Off otherwise): chevrons on the racing line ahead, green, orange or red by how soon the player has to brake for each point | The owner's request (2026-10-05) for phones; the JS has none | D1080, D1081, D1083 |
| A steering assist (Off, Light, Strong; Light by default on touch screens, Off otherwise) blends the player's steering toward the racing line when the car heads off the road or far off the line; applied to the controls before they are quantised, so the tick's input frame carries it | The owner's request (2026-10-05) for phones; the JS has none | D1082, D1083 |
| Bigger touch controls: sideways a larger `--b`, slider and stick throw; upright a wider and taller slider, taller pads, a longer stick throw and a bigger knob | The owner (2026-10-05): the controls are too small upright and still hard sideways | D1084 |
| The main menu plays the selected level's music as soon as the sound is up (natively at once; on the web after the first tap or key); the JS menu is silent until a level tab or Race picks a track | The owner (2026-10-06): sound should come up right away | DECISIONS D1010 |

## Known JS quirks reproduced, not fixed

Unless the owner says otherwise (SPEC 15, question 2):

- All headlight pools share one material, so one car's opacity wins
  (`Effects.js:249,273`).
- The shadow box is said to snap to texels but does not (`Sky.js:271`).
- `input.enabled` is never set false.
- Traffic and police cars always show their exhaust flames: they have no
  entry in `Race.extras`, so `ex.nitro && visible` is `undefined`, which
  three draws (`Effects.js:236-240`; DECISIONS D945).

## Dropped tests

Ported unit tests that assert on README text are dropped (SPEC 4.6). List
each here when M1 ports the tests.

| Test | JS file | Why |
|---|---|---|
| README: level lengths and rival counts match the data | `levels.test.js` | Asserts on README text (WP 1.2) |
| README: the car table lists every car (its performance claims are kept) | `physics.test.js` | Asserts on README text (WP 1.3) |
