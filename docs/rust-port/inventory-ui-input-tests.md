# Inventory: UI, input, platform and tests

Generated 2026-10-03 by reading the JS game at commit `7213a89`. It is a map for porting, not a substitute for the source: check each claim against the file it cites before relying on it. Paths are relative to the repository root unless a section says otherwise.

---

## 1. Screens, states and transitions

**Two kinds of state.**
- **Session `mode`** (`src/main.js:360`): `'loading' | 'menu' | 'race' | 'paused' | 'results'`. Tests read it through `window.__game.mode`.
- **Visible screen `screenNow`** (`main.js:89-97`): one of `['loading','menu','pause','results','padsetup']`, or `null` while driving.
- `showScreen()` toggles `.hidden` on the five full-screen DOM `.screen` divs.
- The touch overlay shows only when `!name && race` (`main.js:95`).
- `#rotate-hint` shows only on the menu on a touch device (`main.js:96`). CSS also hides it in landscape (`hud.css:397`).

**Inside a race, `Race.state`** (`src/game/Race.js:48`) runs `'countdown'` → `'racing'` → `'finished'`.
- The countdown starts at 3.999 s (`:49`). It shows 3/2/1 through `hud.center()` with a beep and a rumble (`:193`). At ≤0 it unlocks physics and shows `GO!` (`:194-205`).
- **Perfect start:** if throttle >0.5 was first pressed with less than 0.75 s left, the car gets +6 m/s and a `PERFECT START` toast (`:189-204`).
- **Finish:** `WINNER!` or `Nth PLACE` (`:323`), a fanfare, then a 3.2 s `finishDelay`, then `onFinish(results())`, which is `showResults` (`:326-333`).
- After the finish the player's car is auto-driven by `coolDown()` to a parking spot (`:497-527`). The race keeps updating behind the results screen (`main.js:606`).

| Screen | DOM | Entered by | Leaves to |
|---|---|---|---|
| Loading | `#loading` (logo, `#load-fill` bar, `#load-label`) `index.html:120-124` | `loadLevel()` `main.js:116-146`; progress callback `(label, f)` from `World.build` | menu, or the previous mode |
| Main menu | `#menu` `index.html:126-172`: level tabs `#level-pick`, level card (`#lvl-num/name/desc/len/best`, `#mode-pick` Race/Hot Pursuit), car picks `#car-pick`, `#btn-start`, `.menu-opts`, touch help, keyboard help | boot `main.js:629-631`; `toMenu()` `:411` | Race (`startRace` `:363`); Controller (`openPadSetup` `:484`) |
| Countdown / racing HUD | `#hud` (+ `#touch` on touch devices) | `startRace` → `mode='race'`, `showScreen(null)` `:395-396` | pause, results |
| Pause | `#pause` `index.html:174-186`: Resume, End run (cruise only, `main.js:397`), Restart, Main menu, Controller (gamepad only), Music/SFX sliders, now-playing line + Next track | Esc/P/gamepad Start/touch ⏸ (`main.js:593`); `visibilitychange` hidden (`:472`) | race (`pause(false)`), `startRace`, `toMenu`, padsetup, results (End run `:477`) |
| Controller setup | `#padsetup` `index.html:189-196` (`#pad-name`, `#pad-binds`, `#pad-hint`, `#opt-rumble`, Defaults/Done) | `openPadSetup()` from menu or pause only (`main.js:484-490`) | returns to `padReturn` (`main.js:483`) |
| Results | `#results` `index.html:198-205` (`#res-title`, `#res-table`, `#res-extra` stat tiles, `#res-best`, Race again, Main menu) | `showResults(res)` `main.js:422-470` | `startRace`, `toMenu` |
| Busted/Wrecked hold | `#hud-hold` card inside the HUD (not a screen) `HUD.js:236-245` | `pu.hold > 0` | auto release |
| Cruise score panel | `#hud-cruise` inside the HUD | level `mode==='cruise'` | — |
| Attract mode | none: the WebGL camera flies along zone 0 behind the menu (`main.js:614-622`) | any mode without a race | — |
| Debug fly cam | `?s=` hides all screens (`main.js:631`) | — | — |

**Results contents** (`main.js:422-470`):
- **Cruise:** title `New best!` or `Run over`; rows Score, Distance, Top speed, Near misses, Time.
- **Race:** a table of place, colour swatch, name and time. A `~` prefix marks an estimated time; estimates use 45 m/s for the remaining distance (`Race.js:558-565`).
- **Pursuit extra tiles:** Busted, Wrecked, Takedowns, Penalty, Top heat ★s.
- **Circuit extra tiles:** each lap's time, with ★ on the best, plus Lap record or New lap record.

**DOM/CSS vs WebGL vs canvas 2D.**
- **WebGL** (`<canvas id="game">`): the 3D world only, rendered through an EffectComposer chain: RenderPass, then UnrealBloom (0.38, 0.35, 0.92), then OutputPass (`main.js:55-66`).
- **Canvas 2D:** `#tach` (260×260) and `#minimap` (220×220, shown at 190 px) only. A sawhorse texture is also drawn on a 2D canvas but used in-world (`PursuitView.js:371`). Many world texture builders in `src/world/*` use `document.createElement('canvas')`.
- **Everything else** is DOM/CSS/inline SVG: menus, HUD text, bars, heat stars (CSS `clip-path`), speed lines (CSS conic gradient), touch controls (SVG icons), toasts.

## 2. HUD elements (`src/game/HUD.js`, `src/game/hud.css`, `index.html:41-79`)

| Element | Id / class | Rendering | Driven by |
|---|---|---|---|
| Position "1st/6" | `#hud-pos`, `#hud-pos-suf`, `#hud-of` | DOM text; ordinal function `ORD` `HUD.js:4` | `st.position`; hidden in cruise `:49` |
| Race clock | `#hud-time` | DOM, `fmtTime` m:ss.hh, rounds to cs first `:6-11` | `st.time` |
| Lap counter, lap time, best lap | `#hud-lap`, `#hud-lap-n`, `#hud-lap-time`, `#hud-lap-best` | DOM; circuits only `:26,172-177` | `st.laps {lap, of, time, best}` |
| Penalty served "+x.x s" | `#hud-pen` | DOM | `pu.penalties` `:233-235` |
| Zone name | `#hud-zone` | DOM | `track.zone[idx(s)]` `:163-170` |
| Zone banner card | `#zone-card .zc-name/.zc-sub` | DOM + CSS 3.6 s keyframe `hud.css:121-125` | zone change once started; on circuits only on the first lap `:169` |
| Route bar with racer dots | `#route-bar` (`.route-seg` flex sized by zone length, zone colour, name), `#route-dots .rdot` | DOM, `left:%` | `st.racers[].s / L` `:180-184`; hidden in cruise |
| Minimap | `#minimap` | Canvas 2D | Details below the table |
| Tachometer | `#tach` | Canvas 2D | Details below the table |
| EV power meter | `#tach` | Canvas 2D | Details below the table |
| Speed | `#hud-speed`, `#hud-unit` | DOM; m/s × 2.23694 (MPH) or × 3.6 (KM/H) `:155-157` | `hud.mph` |
| Gear | `#hud-gear` | DOM: `R`, `N`, `D` (electric) or a number `:158` | |
| Nitro bar | `#hud-nitro` (width %), `.nitro.active` pulse | DOM/CSS `:159-160` | |
| Damage bar | `#hud-dmg`, `#hud-dmg-fill` | DOM; hsl(125·(1−d)), `.crit` >0.75 `:227-231` | pursuit only |
| Speed lines | `#speedlines` | CSS repeating-conic-gradient with a mask; opacity = clamp((v−45)/35) × (nitro ? 1 : 0.6) `:161` | |
| Center pop text | `#hud-center` classes `pop`, `go`, `warn` | DOM + CSS `pop` animation `hud.css:113-117`; `center(text, cls, dur)` `:122-129` | 3/2/1/GO!, WINNER!, WRONG WAY, LAP n/N, FINAL LAP, PURSUIT, ESCAPED, TAKEDOWN, SPIKED!, BUSTED, WRECKED |
| Toast | `#hud-toast` | DOM, opacity fade; `toast(text, dur=1.6)` `:131-135` | Listed below the table |
| Heat stars | `#pz-stars` (5 × `.pz-star` with `<i>` fill) | DOM + CSS clip-path star; whole stars up to heat, the next one filling with `heatMeter`; `.max` at 5 `:211-216` | |
| Bust/Evade bar | `#pz-bar`, `#pz-label`, `#pz-fill` | DOM; BUST while `pu.bust>0`, EVADE in cooldown, hidden in patrol; blinks if flash `:218-225` | |
| Radio/dispatch line | `#hud-radio`, `#hud-radio-text` | DOM pill, slide-in; `radio(text, dur)` `:101-108`; duration max(3, len/14) `PursuitView.js:238` | |
| Busted/Wrecked hold card | `#hud-hold` (`.wrecked`), `#hold-title`, `#hold-sub` "+x.x s PENALTY", `#hold-fill` | DOM `:237-245` | |
| Cruise panel | `#hud-score`, `#hud-mult` "×N", `#hud-mult-fill` (multTimer/6), `#hud-dist`, `#hud-best` | DOM `:186-194` | |
| Now-playing pill | `#np-toast` (outside `#hud`) | DOM, 4 s `main.js:177-186` | track change |
| Stats overlay | `#stats` created dynamically | DOM, `?stats=1` `main.js:548-563` | |

**Canvas 2D details.**
- **Minimap** (`:321-372`): rotates with the player's yaw, 0.28 px/m, offset 30 px down. It draws the road polyline from s−500 to s+900 in two strokes, a green finish dot, traffic dots, rival dots in their colours, and police (`drawPolice` `:377-397`: roadblocks as red bars, spikes as amber bars, units as red/blue dots that blink at 4 Hz unless flash is off, grey when disabled). The player arrow is drawn in screen space.
- **Tachometer** (`drawTach` `:248-280`): arc from 0.75π to 2.25π, 0–8000 rpm, redline from 7000, gradient fill, ticks 0–8, red needle. The font is Rajdhani 600.
- **EV power meter** (`drawPower` `:284-319`): replaces the tach when `st.electric`. Range −250..1000 kW, a green REGEN segment, "kW ×100" labels.

**Toast texts:**
- Race: PERFECT START, `DRIFT x.xs`, `AIR x.xs`, NEAR MISS (+N₂O, or +points in cruise), `LAP m:ss.hh  BEST`, `STUCK? PRESS R/TAP ↺/<pad label> TO RESET`, CRASH — MULTIPLIER LOST.
- Pursuit (`PursuitView.js:133-215`): SPOTTED, COOLDOWN…, HEAT LEVEL n, (HEAVY) ROADBLOCK AHEAD, SPIKE STRIP AHEAD, X HIT THE SPIKES, SPIKES/ROADBLOCK DODGED, X BUSTED, BACK IN THE RACE.

**Other HUD facts.**
- **Wrong way:** facing backwards (along < −0.3) at more than 4 m/s for over 1.5 s shows WRONG WAY (`Race.js:305-309`).
- **No subtitles element** exists. The radio line is the only spoken-text display.
- **Pursuit HUD input** comes from `Pursuit.hud(damage)` (`src/game/Pursuit.js:783-795`). It returns `{heat, heatMeter, state, bust, evade, damage, hold, holdReason, holdTotal, units[{x,z,disabled}], roadblocks[{x,z,yaw,width}], spikes[...], flash}`. `PursuitView.hudState` adds `penalties` (`PursuitView.js:343-347`).
- **HUD update payload** (`Race.js:399-414`): `{position, time, speed, gear, rpm, nitro, nitroActive, electric, power, s, started, racers[{s}], laps, player, traffic[], racersFull, cruise{score,mult,multTimer,dist,top}, pursuit}`.
- **Responsive layouts:** media queries at max-width 720, max-height 780/500, a two-column landscape menu, and portrait variants. Touch devices get a compact digital speedo box in place of the tach (`hud.css:337-396`).

## 3. Menu options and localStorage

**Store helper** (`main.js:22-25`): key prefix `mr.`, JSON-encoded, wrapped in try/catch for private mode. The same helper is copied into `music.html:208-209`.

| Key | Default | Meaning / where |
|---|---|---|
| `mr.musicVol` | 0.7 | Music slider (`.vol-music`, menu and pause kept in sync) `main.js:30,290`; M toggles 0 ↔ 0.7 `:594-599` |
| `mr.sfxVol` | 0.85 | SFX slider |
| `mr.mph` | true | `#opt-mph` |
| `mr.hq` | `!touchUI` | `#opt-hq`: pixel ratio min(dpr, 1.5) vs 1, shadows on/off `main.js:70-76`; also passed to pursuit (shared flashing light) |
| `mr.autogas` | false | `#opt-autogas` (touch only) |
| `mr.steering` | `'stick'` (`'tilt'` if legacy `mr.tilt` is true) | `#opt-steer`: stick, buttons or tilt (touch only) |
| `mr.tilt` | — | legacy boolean, read only `main.js:37` |
| `mr.tiltSens` | 0.5 | `#opt-tilt-sens` 0..100 → 0..1 (shown only with tilt) |
| `mr.pedals` | `'slider'` | `#opt-pedals`: slider or buttons |
| `mr.fullscreen` | true | `#opt-fullscreen` (touch only) |
| `mr.car` | `'sports'` | car pick: CAR_SPECS keys sports, muscle, super, rally, electric |
| `mr.level` | `'sierra'` | level tab (sierra, coast, streets, desert, seaside, cruise); `?level=` overrides without saving |
| `mr.track` | `'auto'` | `#opt-track` music picker (`'auto'` = the level's own track, else a track id) |
| `mr.flash` | true | `#opt-flash`: police lights strobe |
| `mr.rumble` | true | `#opt-rumble` on the Controller screen; toggling plays `kick(0.5,0.7,300)` |
| `mr.padMaps` | `{}` | `{ [gamepad.id]: { action: Binding[] } }` `main.js:155` |
| `mr.mode.<levelId>` | `'race'` | `'race'` or `'pursuit'`; only levels with `police` show `#mode-pick` |
| `mr.best.<levelId>` | null | best *winning* time in seconds (saved only if place 1 and lower) `main.js:443-447` |
| `mr.best.<levelId>.pursuit` | null | the same for Hot Pursuit |
| `mr.bestScore.<levelId>` | 0 | cruise best score (integer) `:425-427`; also fed to `hud.bestScore` |
| `mr.bestLap.<levelId>` | null | circuit lap record in seconds `:460-461` |
| `mr.player.track` / `.repeat` / `.vol` | PLAYLIST[0] / false / musicVol | music.html only |

**Other menu items.**
- The level card shows `levelStats()` (`main.js:212-217`) and a best line: "Best winning time m:ss.hh · Lap record …" or "Best score N".
- The start button label is Race, Hot Pursuit or Cruise (`:233`).
- Also on the menu: a link to `music.html`, the Controller setup button (`.pad-only`), touch help (`.touch-only`) and keyboard help.
- Tilt notes for states insecure, none, ask and denied are listed at `main.js:307-317`.
- `input.enabled` is set true at race start (`:394`) and never set false anywhere in `src/`. Only `test/unit/input.test.js:177` exercises it.

## 4. Input

**Keyboard** (`src/game/Input.js:10-19`):

| Action | Keys |
|---|---|
| throttle | KeyW, ArrowUp |
| brake | KeyS, ArrowDown |
| left | KeyA, ArrowLeft |
| right | KeyD, ArrowRight |
| handbrake | Space |
| nitro | ShiftLeft, ShiftRight, KeyN |
| lookBack | KeyB |
| one-shot | camera = KeyC, reset = KeyR, pause = Escape/KeyP, music = KeyM |

- KeyT (next track) is a separate listener in `main.js:190`.
- Game keys `preventDefault`. Repeats are ignored. Window blur clears held keys (`:29-36`).
- One-shot actions go into the `pressed` Set and are read once with `consume(name)` (`:47-51`). Touch taps add `reset`, `camera` and `pause`. Gamepad edges add `camera`, `reset` and `pause`.
- `pressed.clear()` and `pads.hush()` run on race start and on resume (`main.js:392-393, 406`).

**State object** (`Input.js:26`): `{ throttle 0..1, brake 0..1, steer −1..1, analog bool, handbrake bool, nitro bool, lookBack bool }`.

**Merge order** (`:53-93`):
1. Keys set throttle/brake to 1, or fall back to the touch values.
2. Digital steering (keys, a pad bound to buttons, or ◂ ▸) is ramped: the rate is 7/s returning to 0, 9/s counter-steering, 3.6/s turning in (`:72-73`).
3. Touch stick or tilt passes through unramped with `analog=true`, unless a key is held.
4. A gamepad axis with |ax|>0.12 maps to `sign·((|ax|−0.12)/0.88)^1.4` and sets `analog=true` (`:82`).
5. Pad triggers combine by max (above 0.05). Pad buttons OR into nitro, handbrake and lookBack.

**Consumed downstream.**
- `CarPhysics.step` reads `throttle`, `brake`, `steer`, `analog`, `handbrake`, `nitro`, and `cruise` (set only by Race's coolDown) (`src/vehicles/CarPhysics.js:129-272`).
- With `analog`, steer lock is capped at `ANALOG_LOCK(1.25)·grip·1.5/speed` (`:128-132`).
- `lookBack` goes to `CameraRig` (`Race.js:360`).
- Race reads `inp.throttle` during the countdown (perfect start, launch FX, audio revs).
- While held for a penalty, input is replaced by `holdControls()` (`PursuitView.js:80-84`).

**Gamepad** (`src/game/Gamepad.js`):
- **Bindings:** `{button:i}` or `{axis:j, dir:±1, rest:0|±1}`. `bindingValue` gives 0..1 (`:43-50`).
- `ACTIONS` (`:13-19`): left, right, throttle, brake, nitro, handbrake, lookBack, camera, reset, pause.
- `DEFAULT_MAP` (`:20-26`): stick axis 0 ±, RT=7, LT=6, A=0 nitro, X=2/RB=5 handbrake, B=1 look back, Y=3 camera, Back=8 reset, Start=9 pause.
- Menu navigation is hard-wired to the standard layout (`:31`): D-pad 12–15, A confirm, B back, Start, plus the left stick at ±0.5.
- Stick dead zone 0.12 applies to axis bindings with rest 0 (`:53-54`).
- **`poll()` returns** `state = {connected, value{}, held{} (>0.5), nav{}, steerAxis, digital{left,right}, edges[]}` (`:86-138`).
- **Active pad:** whichever was last touched (a new press or axis change >0.3) (`:148-153`).
- **Muting:** `hush()` and a mute set keep a held control quiet until it is released (`:115-145`).
- **Remapping:** `startCapture(action, done)` waits until everything is released, then binds the first button or axis that moves past 0.6 (`listen` `:167-202`). It times out after 8 s. `bind()` removes the same binding from other actions and saves the map per `p.id` (`:204-210`). `resetMap` deletes that pad's map (`:211-215`).
- **Labels:** `bindingLabel` uses STD names, or "Button n" / "Axis n ±" for non-standard pads (`:34-41`).
- **Rumble API:**
  - `kick(strong, weak, ms)` is a linear-fading jolt.
  - `feel(strong, weak)` is a steady buzz that lapses 150 ms after the last call.
  - `rumbleLevel` takes the max of active effects.
  - `flushRumble` calls `vibrationActuator.playEffect('dual-rumble', {duration:140, strongMagnitude, weakMagnitude})` and re-sends when 80 ms have passed or a value changed by more than 0.08. It calls `reset()` below 0.02 and falls back to `hapticActuators[0].pulse` (`:220-252`).
- **Rumble call sites:**
  - Race (`Race.js`): countdown beeps (0, 0.25, 90), GO (0.3, 0.6, 200), shift (0, 0.2, 70), landing, impact `jolt` (`:441-443`), nitro start (0.45, 0.6, 260), and continuous `feel` for scrape, offroad, skid, nitro and spiked tyres (`:444-453`).
  - Pursuit (`PursuitView.js:172,185,192,198`): takedown, spiked, barrier hit, busted.
- **PadSetup** (`src/game/PadSetup.js`): one row per action (the first has `data-nav-first`). Rows light (`.on`) while held and show `.listening` during capture. The pad name is the id stripped of "(STANDARD GAMEPAD Vendor…)", with "· remapped" or a non-standard warning appended. Esc, P or Start cancels a capture or closes the screen (`main.js:588`).
- **MenuNav** (`src/game/MenuNav.js`):
  - Spatial focus: the nearest control in the pushed direction, scored as along + 3×off-axis (`:119-136`).
  - Repeat after 380 ms, then every 110 ms (`:11`).
  - Sliders and selects enter an edit mode on A; ◂ ▸ then change them in steps of 5 or one option (`:138-151`).
  - B and Start are per-screen callbacks (`main.js:494-502`): B resumes from pause, goes to the menu from results, and closes padsetup. Start races from the menu or results.
  - Body classes `pad` (connected) and `pad-nav` (highlight visible) control the `.pad-focus` and `.pad-edit` outlines. A pointerdown or mouse move hides the highlight. The last focus is remembered per screen.

**Touch** (`src/game/TouchControls.js`):
- **Steering modes:**
  - `stick` (default): a thumb anywhere in the left 45 % of the screen sets the centre, clamped at least `stickR + 4` from the edge. Past full lock the centre follows the thumb (`:173-192`). `stickRange = clamp(min(w,h)·0.15, 44, 84)` px (`:50`). `stickSteer`: dead zone 0.06, end zone 0.04, exponent 1.25 (`:43-47`).
  - `buttons`: ◂ ▸ pads, ramped like keys through `touch.steer`.
  - `tilt`: falls back to the stick until the sensor goes live. A wheel SVG rotates s×90° (`:300-307`).
- **Pedal modes:**
  - `slider` (default): bands from the bottom are brakeFull 0.06, brakeTop 0.30, gasBottom 0.36, gasFull 0.62, nitro ≥0.82, with the lightest pedal at 0.15 (`:37-59`). `--u`, `--gas` and `--brk` CSS variables draw the knob and fills. A thumb more than 3 px past the slider's right edge is DRIFT (handbrake) (`:268-281`).
  - `buttons`: GAS, BRAKE, DRIFT and N₂O pads.
- **Hit testing:** pads use 14 px slop and take the nearest centre. Fingers can slide between pads (`:234-244`).
- **Tap buttons** `[data-tap]` (reset, camera, pause) post one-shots and call `navigator.vibrate(8)` (`:167-172`).
- **Getters:** `throttle` is 1 under nitro. Auto gas gives 1 while the overlay is visible, unless braking or a thumb is on the slider (`:313-319`).
- **Events:** pointer events with `setPointerCapture`. `touchstart` and `contextmenu` are prevented. Blur releases everything.
- **Detection:** `isTouchDevice(params)` is true for `?touch=1`, false for `?touch=0`, otherwise `(pointer: coarse)` with `maxTouchPoints > 0` (`:323-327`).

**Tilt** (`src/game/TiltSteer.js`):
- **Roll:** `screenRoll(beta, gamma, angle)`: `up = (−cosβ·sinγ, sinβ)`, `right = (cos a, −sin a)`, `roll = asin(−(up·right))` (`:24-31`). The angle comes from `window.orientation`, else `screen.orientation.angle` (`:112`).
- **Steering curve:** `rollToSteer` uses a 2° dead zone and exponent 1.3 up to `fullLock` (`:35-39`). `fullLockFor(k) = (40 − 28k)°` (`:42`). Smoothing is exponential with τ=0.05 s and snaps to the target within 1e-3 (`:118-123`).
- **States:** off, waiting, live, none, insecure, ask, denied.
- **Permission flow:** iOS `DeviceOrientationEvent.requestPermission()` is called from `enable()`. `enable()` runs in the menu select's change handler and again in the Race tap (`main.js:325, 368`). An `isSecureContext===false` page shows `insecure`. With no reading after 2 s the state becomes `none`; a null beta/gamma event also means `none` (`:70-115`).

## 5. Platform integration

- **Fullscreen and orientation** (`main.js:347-357`): touch devices only, when the setting is on. Inside the Race tap it calls `requestFullscreen` or `webkitRequestFullscreen` with `{navigationUI:'hide'}`, then `screen.orientation.lock('landscape')`. Every step is best-effort.
- **Visibility:** `visibilitychange` with `document.hidden` pauses when mode is race (`main.js:472`). Pause suspends the AudioContext (`Audio.js:1924-1932`). The render loop keeps running while paused.
- **Audio unlock:** `wakeAudio()` (`audio.init()` + `unlock()`) runs on capture-phase pointerdown, pointerup, touchend, click and keydown (`main.js:201-204`). `askForPlayback()` sets `navigator.audioSession.type='playback'` for iOS Silent mode (`Audio.js:265`). Button clicks play UI sounds (`main.js:206-209`).
- **PWA:**
  - Manifest (`manifest.webmanifest`): `display: fullscreen`, `orientation: landscape`, `start_url ./index.html`, `scope ./`, background and theme `#05060a`, icons `desktop/icon.png` 256 and `icon.svg`.
  - No service worker.
  - Meta tags: theme-color, mobile-web-app-capable, apple-mobile-web-app-capable, black-translucent status bar, a viewport with `user-scalable=no, viewport-fit=cover`, apple-touch-icon, an inline SVG data-URI favicon (`index.html:5-26`).
- **og/twitter tags** (`index.html:15-25`): absolute URLs on `https://sequoia-hope.github.io/midnight-racer/`, `og-image.png` 1200×630, `summary_large_image`.
- **Fonts:** Google Fonts Rajdhani 500/600/700 with preconnect (`index.html:27-29`). The CSS fallback stack is Barlow Condensed, Arial Narrow, system (`hud.css:8`). The canvas tach also names Rajdhani.
- **Import map:** `three` → `vendor/three/build/three.module.js`, `three/addons/` (`index.html:31-36`).
- **Safe areas:** `env(safe-area-inset-*)` in the touch layout.
- **Electron** (`desktop/main.cjs`):
  - Privileged `app://` scheme (standard, secure, fetch, CORS, stream). `app://game/<path>` maps to the project root, with a path-escape check (403), a MIME table, `cache-control: no-cache`, and a CSP on HTML that allows self plus Google Fonts (`:28-119`).
  - Chromium switches: autoplay without a gesture, no renderer backgrounding or timer throttling, ignore-gpu-blocklist (`:74-77`).
  - **Window state:** `userData/window-state.json` stores `{x,y,width,height,fullscreen,maximized}`, saved on close. Defaults are 1600×900, minimum 800×450 (`:83-150`).
  - Keys: F11 toggles fullscreen, Ctrl/Cmd+Q quits, F12 opens devtools only with `--dev` (`:152-157`).
  - Navigation away from `app://` is blocked and popups are denied.
  - `--url-query <qs>` is appended to `index.html` (`:163-164`).
  - `--smoke-test` polls `window.__ready` for up to 60 s, waits `--wait` ms (default 2500), reads the WebGL renderer string, optionally saves `capturePage()` to `--shot <png>`, filters console errors and warnings (ignoring favicon, 404, Autofill, fonts), prints JSON, and exits 0 or 1 (`:168-208`).
  - `play.sh` installs Electron on first run and adds `--no-sandbox` when AppArmor blocks the sandbox (override with `MP_SANDBOX`). `tools/install-desktop-entry.sh` writes an XDG `.desktop` file.
- **serve.py** (`tools/serve.py`): ThreadingHTTPServer on `--port` (required) and `--bind` (default 0.0.0.0). `/vendor/` and `/audio/` are sent `no-cache`; everything else is `no-store`, with `If-Modified-Since` stripped so non-pinned files never get a 304. `serve.sh` gets the port from `$PORT` or `proj port`.
- **Deploy:** GitHub Pages serves `main` as-is with no build step (`README.md:199-201`). `.nojekyll` is present and there is no `.github/` workflow. Tilt needs https, so it only works on the Pages build (`README:317`).

## 6. URL parameters and `window.__*` globals

**URL parameters** (`src/main.js` unless noted):

| Param | Effect |
|---|---|
| `level` | sets the level, not saved (`:51`) |
| `pursuit=1/0` | forces Hot Pursuit on or off (`:50`) |
| `heat` | starting heat 1–5 (`:378`) |
| `cops` | caps police units (default 6; 0 gives no police) (`:378`) |
| `t` | fixed time of day (`sky.override`) (`:139`) |
| `s, h, back, lat, v, yaw, pitch` | debug fly camera (`:506-523`) |
| `timescale` | multiplies dt (`:545`) |
| `stats` | stats overlay (`:548`) |
| `autodrive=1` | autopilot replaces input (`:526-540, 607`) |
| `autostart[=carKind]` | starts a race at boot (`:632`) |
| `touch=1/0` | forces the touch UI (`TouchControls.js:324`) |

**Globals:**

| Global | Exposes |
|---|---|
| `__world` | the World (`:133`) |
| `__audio` | GameAudio (`:156`; tests read `.ctx.state` and `.trackInfo`) |
| `__pads` | the Pads instance (`:157`) |
| `__camera` | the THREE camera (`:159`) |
| `__THREE` | the THREE namespace (`:160`) |
| `__game` | `{ get mode() }` (`:361`) |
| `__race` | the live Race (`:398`; tests poke `.player`, `.phys`, `.track`, `.input`, `.cam`, `.ais`, `.time`, `.state`) |
| `__dbg` | the fly-camera object or null (`:511`) |
| `__stats` | `{fps, cpuMs, calls, tris, geos, tex}` every 500 ms (`:554-561`) |
| `__ready` | true after first level load (`:633`) |
| `__pursuit` | the live Pursuit (`PursuitView.js:43, 360`) |

- `music.html:530-538` exposes `__audio`, `__player` and `__ready`.
- `tools/car-test.html` exposes `__tris` and `__ready`.
- `tools/city-view.html` exposes `__world`, `__camera`, `__THREE`, `__stats` and `__ready`.

## 7. Main loop (`main.js:566-626`)

**Timing.**
- Driven by `requestAnimationFrame`. dt comes from the rAF timestamp; the first frame gets 0 (`:571-575`).
- Clamp: `dt = min(frameDt, 1/20) × timescale` (`:583`).
- Exceptions are caught each frame and each distinct error is logged once (`:577-580`).
- Car physics substeps: `n = min(12, ceil(dt/(1/120·1.1)))` equal steps that add up to exactly dt. There is no accumulator (`CarPhysics.js:86-92`). AI, traffic and pursuit run once per frame on the frame dt.

**`tick` order:**
1. `input.update(dt)`, which polls the pads.
2. Toggle the `body.pad` class.
3. Padsetup escape check.
4. `menuNav.update(nav)` and `padSetup.update()`.
5. Return early if there is no world or mode is loading.
6. Consume pause and toggle `pause()`; consume music (mute toggle).
7. One of four branches:
   - fly camera;
   - race or results: `race.update(dt, inp)`;
   - paused: no sim, only the focus is kept;
   - attract camera.
8. `world.update(dt, s, focus, camera)`: sky, night materials, updaters.
9. `refreshEnv` (PMREM of the sky dome when the time of day moves by ≥0.025).
10. `composer.render()`.

**`Race.update` order** (`Race.js:182-416`):
1. Countdown state machine; race time; `world.onCountdown`.
2. Reset key.
3. Build the agents list.
4. Player control: coolDown, hold, or the player's input. Then `phys.update`.
5. `ai.update` and park spots.
6. Circuit progress.
7. Distance and odometer.
8. `traffic.update`.
9. `pv.update` (Pursuit).
10. `resolveCollisions` and `writePos`.
11. Hit reactions: audio, rumble, camera bump, sparks, crash; then physics events (impact, shift, land).
12. Scrape sparks.
13. Bonuses: drift, near miss.
14. Wrong-way and stuck checks.
15. Cruise score, lap check, finish detection, finish delay.
16. Visual sync: vehicles, traffic LOD, lights, boost.
17. `pv.sync`: pursuit events turn into HUD and radio.
18. `effects.update`.
19. Camera: cycle, update, intro camera during the countdown.
20. Offroad and rumble.
21. Audio update, rival engines, sirens.
22. Tunnel reverb.
23. `hud.update`.

## 8. Tests

Run with `npm test`. Unit tests: `node --test 'test/unit/*.test.js'`. E2E tests: `node --test --test-concurrency=2 'test/e2e/*.test.js'` (`package.json`). The counts below include tests generated in loops: 6 levels, 4 police levels, 5 cars, 7 music tracks.

**Unit (plain Node):**

| File | Tests | Covers |
|---|---|---|
| physics.test.js | 37 | CarPhysics per car plus general behaviour (assertions below) |
| track.test.js | 73 | Track geometry on all 6 levels + tags |
| ai.test.js | 16 | AIDriver and KinematicCar |
| traffic.test.js | 14 | Traffic spawning and rules; Collisions |
| pursuit.test.js | 13 | Pursuit and PoliceDriver |
| levels.test.js | 38 | level data schema; README claims |
| seaside.test.js | 9 | survey-built circuit |
| hud.test.js | 14 | `fmtTime`; HUD pursuit furniture on a hand-rolled `FakeEl` DOM with a Proxy canvas context |
| input.test.js | 9 | keymap, consume, steering ramp (0.36 after 0.1 s; 7/s back; 9/s counter), blur, touch merge, gamepad, analog flag, enabled; fake `window = new EventTarget()` |
| gamepad.test.js | 12 | binding values and labels, nav, hush, capture, dead zone, rumble; fake `navigator.getGamepads` |
| touch.test.js | 3 | `stickSteer`, `stickRange` (915×412 → 61.8), `sliderAt` |
| tilt.test.js | 8 | `screenRoll`, `rollToSteer`, sensor and permission states, Input merge |
| page-meta.test.js | 2 | og tags; the PNG is 1200×630 |
| audio-session.test.js | 3 | `askForPlayback` |
| pursuit-audio.test.js | 4 | siren patterns, doppler, level vs distance, no-ops before init |
| radio.test.js | 10 | radio lines, clip coverage, RadioVoice fetch and decode |
| music.test.js | 19 | `noteToMidi`, track compile, a strict fake AudioContext playing each song |
| math.test.js | 10 | clamp/lerp/smoothstep/damp/wrapAngle/stopSpeed/mulberry32/hash/noise |

**Unit shims** (`test/unit/support/`):
- `three.js` and `three-resolve.js`: a Node `module.register` resolve hook that maps `three` and `three/addons/*` to `vendor/three`.
- `sim.js`: `makeVehicle(kind, mass)` (a Vehicle with dims and no model), `straightTrack(len, road)` (flat, along +x), `withSeededRandom(seed, fn)` (swaps `Math.random` for mulberry32), and DIMS for the 5 cars plus a rival.
- `levels.js`: LEVELS with `prepare()` awaited (Seaside survey data).
- `pose.js`: a beta/gamma generator for a phone held at a given angle, turn and lean, plus `expectedRoll`.

**Parity assertions from the pure-simulation tests:**

*physics.test.js* (dt 1/60, 14 km straight):
- For each car:
  - 0–100 km/h takes 1.5–6 s; top speed is 200–320 km/h; the car stays on the centreline with yaw 0; the `vmax` limiter holds; it ends in gear 6 (electric: 1).
  - Braking from 30 m/s stops in under 3 s and under 45 m, and the brake light goes off.
  - Holding brake from rest reverses: speed < −2 m/s after 2 s in gear −1, capped above −14 m/s, and throttle goes forward again.
  - steer +1 at 20 m/s gives yaw > 0.1 and lat > 0.5. Left mirrors right exactly (|yawL + yawR| < 1e-6). Steering at standstill gives yaw 0.
  - Nitro gives more than 5 m/s extra in 2 s and drains the tank. An empty tank does nothing; nitro without throttle stays inactive.
- Analogue steering at 30/45/60 m/s yields 0.2·ANALOG_LOCK (±0.06) and 0.5·ANALOG_LOCK (±0.1) of the grip-limited yaw rate (grip·1.5/v); full stick exceeds 0.9 of it. Keys at 0.2 already exceed 0.9 of the limit. At 5 m/s analogue equals keys.
- Handbrake at 30 m/s gives drifting, |slip| > 0.18 and skid > 0.3; drifting fills nitro.
- `locked` holds the car still with rpm > 5000, and it moves at GO.
- Coasting from 20 m/s gives 5–18 m/s after 3 s, then a stop with no creeping backwards.
- Walls: lat ≤ wallR − halfW + 0.01, an `impact` event fires, all values stay finite.
- Steering off the wall keeps yaw rate < −0.5; a tail slap brings it above −1.
- The car never backs off the start of a point-to-point road (s ≥ 0.5).
- Electric: gear 1, rpm ≤ MOTOR_MAX·1.02, powerOut > 0; braking gives regen > 0.2 with powerOut < 0 and charges nitro.
- Rally turbo boost > 0.5 under throttle and < 0.1 0.5 s after lifting.
- Deterministic: identical inputs give identical state.
- Per-frame displacement is within 1 % of v·dt at 60/90/120/144 Hz with ±3 % jitter.
- README car claims hold: electric has the quickest 0–100; rally launches faster but tops out lower than sports, muscle and super.

*track.test.js* (each of 6 levels):
- Loops have length = n and finishS = roadEnd = ∞. Point-to-point roads have length = Σ segment lengths, n = length + 1, finishS = length − finishRunoff (default 180), and finishS > 3000.
- startS is in (0, 200), or [0, 200) for circuits.
- 16 per-sample arrays are finite. 3 < hw < 12, walls ≥ hw, |f| = 1.
- Samples are 1 m ± 0.02 apart.
- Zones are contiguous and cover the route; `zoneBlend` weights sum to 1.
- `frame` and `pointAt` geometry holds: right = (−fz, fx), pointAt sits on `surfaceY`.
- `project` inverts `pointAt` within 0.15 m in both s and lat.
- `idx`, `wrap` and `ds` semantics hold on loops and lines, and the runout continues straight past the end.
- The racing line stays within hw − 1.6.
- The speed profile stays in (10, 69.5] and is brakeable at 11 m/s².
- No two parts of the road overlap (gap 300 m, clearance 6 m vertical).
- `nearest` and `distanceToRoad` work.
- Terrain never rises more than 0.03 m above the road.
- Tags are well formed, with exactly one `start` on point-to-point roads.

*ai.test.js*:
- Each level's field finishes, or laps a loop in under 300 s, at an average of 25–69.5 m/s. Car centres stay on the tarmac and bodies never pass more than 0.01 m into a wall.
- Higher skill finishes sooner (0.93 > 0.96 > 0.99 in time).
- Rivals stay on the grid until `started`.
- Rubber band: 400 m ahead is more than 2 m/s slower; 350 m behind is more than 2 m/s faster.
- Overtakes at 30 and 60 m/s never overlap bodies.
- A rival alongside makes 0 contacts.
- A rival boxed against the wall drops behind with fewer than 10 contacts.
- A player pinned by a rival gets 2 m off the wall in under 1.5 s for every car.
- KinematicCar: velocity round trip; wall clamp and bounce; spin decays (|spin| < 0.05, stunned 0); `translate`; oncoming cars have yaw ≈ π; clamps at the road end; wraps on a loop.

*traffic.test.js*:
- Per level, spawned cars stay inside the walls and on the tarmac with 0 ≤ speed < 45; more than 10 cars come and go (0 on a closed circuit); at most 22 are active.
- No spawning in the grid zone (< startS + 250) or past finish + 110.
- No traffic on the desert lake bed.
- A car slows to under 12 m/s behind a slower one, keeps a gap over 5 m, and shows brake lights.
- Collisions: a `carhit` event with strength in (0.3, 1]; momentum is conserved for equal masses; cars separate; a second pass finds no contact.
- An off-centre hit spins the car mirror-symmetrically and stuns it.
- Leaning contact pushes without spin.
- Collisions are skipped for cars far apart, cars more than 3 m vertically apart, and traffic against traffic (`kinematicOnly`).
- PhysicsBody (the player) collides: loses speed, shoves the other car, and gets a yaw rate.

*pursuit.test.js*:
- `topSpeed(sports)` is in (65, 80) m/s; rally ≤ 71.5.
- Line of sight reaches 300 m, is blocked by a 90° corner unless `losOpenGround`, and a tunnel hides you unless both cars are inside.
- A parked unit spots a 40 m/s racer and starts a pursuit in chase mode.
- Two stopped units beside you bust you within 5 s. The hold equals `bustPenalty(heat)` ± 0.1 s. The release spot is ahead of the units, the state returns to patrol, and a grace period follows.
- Out of sight, escape takes `EVADE_TIME[heat]` ± 1 s after cooldown starts.
- Heat rises with the meter, is capped by the zone `heatCap`, and does not rise outside a pursuit.
- Active units never exceed `HEAT[heat].units`, and at least min(2, cap) arrive.
- Crossing spikes sets `phys.spiked > 0` and emits a `spiked` event.
- A roadblock has 2 or more cars, a gap on the road with every car more than halfW + 1 from it, cars inside the walls, and sawhorses only at heat 5.
- On all 4 police levels a full race finishes, a pursuit starts, units stay within 0.3 m of the walls, and heat stays ≤ the cap.

*levels.test.js*:
- The id order is sierra, coast, streets, desert, seaside, cruise, numbered LEVEL 1–5 and ENDLESS. `levelById` falls back to the first level.
- Required fields; segments xor loop. Cruise is a loop with no rivals. Circuits are loops with integer laps, have rivals, and have no police.
- Zones have unique keys, a known landform, and a `src/world/<scenery>.js` file that exists, plus a hex colour.
- Sky keyframes run from s 0 to 1 with identical fields and night in [0, 1].
- Segments: integer length, |turn| ≤ 270, grade < 0.3, valid road types, zones only move forward.
- Rivals have unique names, a CAR_SPECS kind, skill in (0.8, 1], power in (300, 700).
- One traffic rule per zone, with mix weights summing to 1 and speed < 45.
- README lengths and rival counts match the data. Seaside `lapLength` matches the survey.

*seaside.test.js*:
- The lap is 3602 ± 15 m, equal to the survey, with 3 laps, startS 0, finishS ∞. It runs anticlockwise with a net turn of 2π.
- The height range is 54.9 ± 1 m. The highest point is at the Corkscrew tag, followed by a 16–21 m drop over 150 m. |bank| < 0.16.
- Walls stand between hw + 1.5 and 34.5 m, with more than 30 % of sides having over 10 m of run-off.
- No surface step at the tarmac edge or at RUNOFF_FLAT. Ground vs road < 0.6 m.
- Terrain vs run-off surface < 0.5 m.
- After 1 s from 40 m/s, a car on loose run-off has `offTrack` > 0.95 and is more than 4 m/s slower than on tarmac. Paved run-off has `offTrack` 0 and is within 0.6 m/s of tarmac.
- 30–75 % of the run-off is paved, and the photo covers the lap.
- Half-width is 5.2–7.8 m, narrowest at s 1700–2050; the pit straight is wider than 14 m.
- Rubber-banding uses `prog`, not lap position.

**E2E harness** (`test/e2e/harness.js`):
- `launch()` starts puppeteer-core Chrome (`CHROME_PATH`, default `/usr/bin/google-chrome`) headless unless `MP_HEADFUL`, with flags `--use-angle=vulkan --enable-gpu --ignore-gpu-blocklist --autoplay-policy=document-user-activation-required`.
- `DEVICES`: desktop 1280×800; phone 915×412 (mobile, touch, landscape, Pixel UA); phonePortrait 412×915.
- `openGame(browser, {device, query, storage, path, init, initArgs})`:
  - creates a fresh browser context;
  - uses request interception to serve the working tree at `https://midnight-racer.test` (403 if a path escapes, 404 otherwise, all other origins aborted), or `MP_BASE_URL` to test a running copy;
  - collects page errors, console errors and warnings, and HTTP ≥ 400;
  - emulates the device;
  - seeds `localStorage` through `evaluateOnNewDocument`;
  - runs `init` (fake gamepad, etc.) before page scripts;
  - navigates and waits for `__ready`.
- **`Game` methods:**
  - `eval(fn|expr, ...args)` uses CDP `Runtime.evaluate` with `userGesture:false`, so tests don't grant user activation.
  - `waitFor(fn, {timeout, interval, what})` dumps a snapshot on timeout.
  - `snapshot()` returns `{screen, mode, race state, audio ctx state, fullscreen, touchUI}`.
  - `screen()`.
  - `center(sel)` scrolls the element into view and fails if another element covers it.
  - `tap(sel)` uses the touchscreen; `click(sel)` uses the mouse.
  - `key(code, holdMs)`.
  - `touch(type, points)` sends raw CDP `Input.dispatchTouchEvent`.
  - `reload()`, `waitReady()`, `close()`.
- **Helpers:**
  - `flow-helpers.js`: `raceStarted`, `isShown`, `stored(key)`, `startFromMenu`, `markRace`/`newRaceStarted`, `waitRacing`, `expectScreen`.
  - `controls-helpers.js`: `simWait` (waits on race time), `startRace`, `car()` snapshot, `placeCar`, `cameraRight`, `pad`/`press`/`lift`/`hold`, `holdKeys`/`holdSim`, `choose(select)`, `stickDown`/`stickTo`, `sliderPoint`.
- E2E tests drive the game through `window.__race`, `__camera`, `__audio`, `__pads`, `__pursuit` and `__stats`. Tilt uses CDP `DeviceOrientation.setDeviceOrientationOverride`. The gamepad tests replace `navigator.getGamepads` with a fake pad that records rumble effects.

**E2E files:**

| File | Tests | Covers |
|---|---|---|
| analog-controls | 5 | thumb stick proportional and re-centring; stick + gas; slider bands and fills; DRIFT strip; ◂ ▸ and pedal-button modes |
| audio | 10 | Race tap starts audio; iOS audioSession; pause suspends and resume restarts; an interrupted context recovers on touch or key; M mute persists; T and Next track; slider sync and persistence; track picker; no warnings while driving |
| circuit | 4 | Seaside card; start lights; 3 laps with results laps; no lap counter elsewhere |
| gamepad | 5 | menu navigation, Start, pause and back; held A doesn't fire nitro; slider/select editing; remapping LB → throttle saved per id; wall rumble and the off switch |
| keyboard | 6 | menu keys don't leak; W/↑ and S/↓; steering; Space, Shift, N; C, B, R; Esc/P pause freezes the race |
| levels | 6 | each level loads and makes progress under autodrive |
| menu | 9 | tabs and picks; car and level persistence; checkboxes and track picker; desktop vs phone option visibility; rotate hint; every menu control reachable and uncovered on phone and portrait |
| music-player | 11 | music.html: play/pause, every track, next/prev wrap, seek, solo, repeat, persistence, meter, keyboard, back link |
| pursuit | 7 | mode toggle per level; race mode has no pursuit HUD; units, stars and siren; radio voice prefetch and fallback; busted flow (R blocked); wrecked flow; pursuit results and separate best time |
| race-button | 7 | phone landscape and portrait Race; after other controls; fullscreen off; a double tap starts one race; desktop click; Tab + Enter |
| race-flow | 11 | countdown → racing; Esc and P pause; touch pause; visibilitychange; Restart (with sound); Main menu; win saves best; 2nd place saves none; cruise End run saves the score |
| tilt | 5 | tilt option, saving and the no-sensor note; steering direction and wheel; sensitivity; both landscapes and portrait; none on desktop |
| touch-controls | 10 | pads shown only while racing; GAS, BRAKE, steer, N₂O; sliding between pads; auto gas; reset, camera and pause pads; 4 layout and device combinations; `?touch` forcing |
| traffic-lod | 2 | far traffic in 4 draw calls; LOD switches at about 90 m |

## 9. tools/

- `serve.py`: dev static server with cache headers (section 5).
- `og-image.mjs`: uses the harness's interception to open `?level=sierra&autostart=super&pursuit=1&heat=3&cops=3`. It stages a pursuit scene through `__race`/`__pursuit`, overrides `cam.update`, hides the HUD, overlays the logo and the tagline "Do you have what it takes?", then screenshots 1200×630 to `og-image.png` (`--out`, `--scale`).
- `check-track.js`: Node CLI taking a level id (default sierra; `npm run check-track`). It prints length, finish, and per-zone s-range and heights; the tightest radius, smoothed and raw; the steepest grade; bounds; CLOSE overlaps (gap 300 m, threshold hw+hw+18); and `--dump file.json` writes samples.
- `check-terrain.js`: terrain vs road check and field-build timing.
- `car-test.html`: browser lineup of every vehicle kind with `?view=` lineup, close, side, rear, chase, front or orbit, plus car, kinds, lod, noenv, noshadow, lights, brake, spin, solo, night, steer, siren and t parameters. It exposes `__tris` and `__ready`.
- `city-view.html`: city-zone scenery preview with a fly camera.
- `audio-test.html`: Web Audio test page.
- `radio-voice.html` and `radio-voice/` (design.py, render.py, lines.mjs, voices.json): police radio voice design, recording and audition (Qwen3-TTS).
- `seaside/build.py`: builds Level 5 data (circuit.js, ground.js, photo.jpg) from OSM and USGS lidar.
- `install-desktop-entry.sh`: writes the XDG `.desktop` entry.

---

## Inherently DOM/browser-bound pieces

- **All screens and menus** (loading, menu, pause, padsetup, results) are HTML. They use native `<input type=range>`, `<select>` and checkboxes, CSS layout and media queries (portrait/landscape, height breakpoints, two-column menu), `backdrop-filter`, gradient text and `scrollIntoView`.
- **The HUD** is DOM text and CSS: animations (pop, zoneCard, nitro pulse, blink), clip-path stars, conic-gradient speed lines, a gradient-masked hold card, and transitions for the toast, radio and now-playing pill.
- **Canvas 2D drawing:** the tach/power meter and minimap. Many world texture generators also use `document.createElement('canvas')` (`src/world/*textures*`, `CarModel.js`, `PursuitView.js:371`).
- **MenuNav** depends on `getBoundingClientRect`, `getComputedStyle` and `click()`/`dispatchEvent` on DOM controls. **PadSetup** builds its DOM rows.
- **TouchControls** depends on pointer events, `setPointerCapture`, `getBoundingClientRect` hit-testing on DOM pads, CSS custom properties, inline SVG icons, `navigator.vibrate`, `env(safe-area-inset-*)` and `matchMedia('(pointer: coarse)')`.
- **Tilt:** `deviceorientation`, iOS `DeviceOrientationEvent.requestPermission()` inside a user gesture, `isSecureContext`, `window.orientation`/`screen.orientation.angle`.
- **Gamepad:** `navigator.getGamepads()`, `vibrationActuator.playEffect('dual-rumble')`/`reset()`, `hapticActuators`.
- **Keyboard:** `KeyboardEvent.code`, `preventDefault`, `blur`.
- **Fullscreen** (`requestFullscreen`/`webkitRequestFullscreen`, `navigationUI:'hide'`) and `screen.orientation.lock` inside a user gesture.
- **Page lifecycle:** `visibilitychange`, `resize`, `devicePixelRatio`.
- **Audio:** AudioContext creation and unlock gated on user-gesture events, `navigator.audioSession` (iOS), `fetch` of radio clips (`RadioVoice.js`).
- **Storage:** `localStorage` (`mr.*` JSON).
- **URL:** `location.search` parameters; `window.__*` globals for the CDP-driven e2e tests.
- **Page head:** Google Fonts stylesheet, import map, PWA manifest and meta tags, og/twitter tags (checked by `page-meta.test.js`).
- **Separate page:** `music.html` (its own DOM UI and storage keys).
- **Electron shell:** `app://` protocol, CSP, window-state file, smoke test using `executeJavaScript('window.__ready')` and `capturePage`.
