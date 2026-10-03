# Inventory: audio

Generated 2026-10-03 by reading the JS game at commit `7213a89`. It is a map for porting, not a substitute for the source: check each claim against the file it cites before relying on it. Paths are relative to the repository root unless a section says otherwise.

---

**Bottom line:** there is no AudioWorklet and no ScriptProcessor anywhere in `src/`, `tools/` or `music.html`. All live sound comes from built-in Web Audio nodes driven by AudioParam automation and audio-rate modulation. Sample-by-sample DSP happens only in JS, before playback: `samples.js`, the noise beds, the reverb impulse responses and the engine Fourier series.

---

## 1. Node graph, buses, volumes, persistence

The diagram is documented at `src/game/Audio.js:3-7`. `_build()` (`Audio.js:496-553`) constructs it:

- **Master:** `master` (Gain) → `limiter` → `ctx.destination` (`:498-507`).
  - The limiter is a DynamicsCompressor: threshold −1.5 dB, knee 0, ratio 20, attack 2 ms, release 120 ms.
- **SFX bus** (`:510-519`): `sfxBus` → `envLow` → `sfxComp` → `sfxVol` → `master`.
  - `envLow` is a lowshelf at 180 Hz, 0 dB normally and +3 dB in a tunnel.
  - `sfxComp` is a DynamicsCompressor: −13 dB threshold, knee 8, ratio 3, attack 5 ms, release 180 ms.
  - The `sfxVol → master` link is wrapped in `_sfxGate` (tail 0.5 s).
- **Tunnel reverb** (`:594-620`): `sfxBus` → `envSend` → Convolver → Gain 0.55 → `sfxComp`.
  - The impulse response is generated in JS: 1.4 s stereo, one-pole-darkened noise with an exp(−5.2t) decay plus 14 random early-reflection spikes.
  - It is gated with a 4 s tail. `setEnvironment('tunnel')` sets `envSend` to 0.75 and `envLow` to +3 dB, with a 0.25 s time constant (`:485-493`).
- **Music bus** (`:522-536`): `musicIn` → `musicMix` → `musicComp` → `musicGate` → `musicDuck` → `musicMoodLp` → `musicMood` → `musicVol` → `master`.
  - `musicIn` is a highpass at 36 Hz.
  - `musicComp`: −14 dB threshold, knee 12, ratio 2, attack 4 ms, release 250 ms.
  - `musicGate` is a Gain that `setMusic` fades in with a 0.3 s time constant and out with 0.25 s (`:1940-1954`).
  - `musicDuck` dips under the fanfare, busted and escaped stingers.
  - `musicMoodLp` is a lowpass at 20 kHz, ramped to 900 Hz in cooldown.
  - `musicMood` is a trim of 1 normally, 0.75 in cooldown (`:1894-1907`).
- **Internal trims** (`:244-247`): MASTER_TRIM 0.64, SFX_TRIM 1.25, MUSIC_TRIM 2.4, RUMBLE 4.
- **Volumes:** `setVolume({master,sfx,music})` takes 0..1 values and applies them with a 0.03 s time constant (`:372-387`).
  - Defaults are master 1, sfx 0.85, music 0.7 (`:307`).
  - SFX ≤ 0.001 shuts `_sfxGate`, so the whole effects graph stops rendering. Reopening it calls `_hush()`, which zeroes every voice level (`:419-425`).
- **Persistence (in `src/main.js`, not the audio code):**
  - localStorage keys `mr.musicVol` (default 0.7), `mr.sfxVol` (0.85) and `mr.track` (`'auto'`) (`main.js:30-31,43`).
  - The menu and pause sliders are kept in sync by `bindSlider` (`:277-291`).
  - `applyVolume()` calls `setVolume` and then `setMusic(music > 0.001)` (`:161-164`), so music volume 0 also stops the sequencer.
  - The M key toggles music between 0 and 0.7 and stores it (`:594-599`).
  - `music.html` uses its own keys: `mr.player.vol`, `mr.player.track`, `mr.player.repeat`.

## 2. Engine synthesis

The technique is a **wavetable built as a PeriodicWave** (header at `Audio.js:16-24`).

- **Cycle drawing:** `engineCycle()` (`:112-169`) draws one 720° four-stroke cycle (2048 samples).
  - Each cylinder contributes a gamma-like blowdown pulse, `(u·e^(1−u))^sharp`, followed by a small reflected rarefaction.
  - Pulse timing comes from the per-car `banks` firing-order map, `bankDelay` and seeded `jitter`/`ampVar`.
  - The off-throttle ("overrun") variant uses wider pulses, lower sharpness and extra random burbles.
- **Fourier step:** a naive DFT to 220 harmonics with low-order shaping (lowBoost, a taper below half-order, exp(−h/70) on the off wave). The result goes to `createPeriodicWave(real, imag)`.
- **Rumble wave:** `rumbleCycle()` (`:176-188`) builds a separate PeriodicWave up to 2× the firing order, reusing the engine's phases.
- **Profiles:** `CARS` (`:69-105`):

  | Key | Engine | Cylinders / banks | Notable fields |
  |---|---|---|---|
  | `sports` | flat-plane V8 | 8, alternating banks | pops 1.0 |
  | `muscle` | cross-plane V8 | 8, banks `[0,1,1,0,1,0,0,1]` | bankDelay 14 ms, lowBoost 1.7, pops 1.7 |
  | `super` | V10 | 10 | sharp 2.2, high formants, pops 0.6 |
  | `rally` | turbo inline-4 | 4, one bank | pops 2.2, `turbo: 1` |
  | `electric` | motor | `electric: true` | only feeds the pops code |

  Every profile also carries 3 formant triples (freq, gain dB, Q), `lpBase`, `lpRange`, `drive`, `trim`, `popFreq`, `intake`, `rasp` and `whine`.
- **Per-car wave cache:** `setCar()` (`:445-482`) builds and caches 5 PeriodicWaves per car (onL, offL, onR, offR with different seeds, plus rumble). It swaps them in with `setPeriodicWave` and retunes the formant peaking filters (R side ×1.04).
- **Graph:** `_buildEngine()` (`:622-727`):
  - **Two chains**, L (pan −0.38, detune −4 cents) and R (pan +0.38, detune +5). Each chain is: `on` Osc and `off` Osc → `gOn`/`gOff` → `am` Gain → `drive` Gain → WaveShaper (`exhaustCurve(2.2)`, an asymmetric tanh, 2x oversampling) → DC-blocking highpass at 28 Hz → 3× peaking Biquad (formants) → lowpass (Q 0.9) → StereoPanner → `e.sum`.
  - **Jitter:** a looped brown-noise buffer (rate 0.7) → lowpass at 16 Hz. It feeds `jitAmp` (0.35, into every `am.gain`) and `jitPitch` (14 cents, into every oscillator's `detune`). This is audio-rate AudioParam modulation.
  - **Rumble:** Osc → highpass 26 Hz → lowpass 170 Hz → AM → WaveShaper (tanh 2.5, 2x) → lowpass 520 Hz → `rumG` → sum.
  - **Intake:** looped pink noise → 2 bandpass filters (Q 1.6 and 2.4) → `inG`.
  - **Rasp:** looped white noise → bandpass at 2600 Hz → `raspG`. The `L.on` oscillator also feeds `raspDepth` → `raspG.gain`, so the engine waveform amplitude-modulates the noise.
  - **Output chain:** sum (0.105) → highpass 28 Hz → `body` lowshelf (140 Hz, +6 dB) → `shiftG` → `limG` → `misG` → `out` → `sfxBus`.
  - **Rev limiter:** a square LFO at 14 Hz → `limDepth` → `limG.gain`.
  - **Transmission whine:** sine plus triangle (×0.35) → `whineG` → `sfxBus`, outside the engine chain.
- **Per-frame steering:** `_steerEngine` (`:1065-1106`), all via `setTargetAtTime`:
  - Pitch: `fc = rpm/120`, time constant 0.022 s, on all 5 engine oscillators.
  - Load: `load = thr^0.7`. Crossfade gOn = 0.12+0.88·load, gOff = 0.75(1−load)+0.08.
  - Drive: `prof.drive·(0.65+0.7load+0.35rn)`.
  - Lowpass: `lpBase + lpRange·(0.25rn + 0.45·load·(0.4+0.6rn))`, R side ×1.06.
  - Intake bandpasses at firing frequency ×1.1 and ×2.3, rasp bandpass 1800+2600·rn, rasp depth, rumble gain.
  - Output level: `trim·(0.45+0.27load+0.2rn)`.
  - The limiter engages when rpm ≥ 0.975·rpmMax, thr > 0.6 and the car is on the ground: `limG` → 0.7 and `limDepth` → 0.3.
- **Whine** (`:1041-1048`): frequency 90+26·speed (reverse: 250+90·speed). Gain `prof.whine·0.016·…`, or 0.05 in reverse.
- **Decel pops** (`:1050-1058`):
  - A burst of 3–7 × `prof.pops` pops fires on throttle lift (prev > 0.5 → now < 0.15) above rn 0.5.
  - Otherwise there are random pops at `dt·1.6·pops` probability when thr < 0.1 and rn > 0.42, with a 0.12 s cooldown.
  - `_pop()` (`:1274-1299`) is a one-shot: white-noise BufferSource (random rate) → bandpass in the `popFreq` range → WaveShaper (`distortionCurve(6)`) → gain envelope (linear 2 ms attack, exponential decay ~50–80 ms). A sine thud sweeping down to 40 Hz is layered under it.
- **Gear shifts:** `shift(up)` (`:1302-1321`):
  - Dips `shiftG` (to 0.3 up / 0.55 down over 25 ms, then recovers with a 0.05 s time constant).
  - Plays the pre-rendered `clunk` buffer.
  - Downshift adds a rev-match pop. Upshift adds 2 pops if the previous throttle was > 0.6, and a blow-off if the car is turbo and boost > 0.5.
- **Turbo** (`:731-750`, `:1133-1158`):
  - Two sines (ratio 1.505) plus white noise → bandpass "hiss".
  - Whistle frequency 1900+3600·boost+900·rn; gain 0.045·boost²·(0.4+0.6thr). Hiss bandpass 1400+2600·boost.
  - **Blow-off** fires when the previous throttle was > 0.5, now < 0.2 and boost > 0.35. It is three `_noiseBurst` layers: a bandpass sweeping 3600→1300 Hz, a 5.2 kHz highpass, and 3 bursts at 520 Hz ("chu-tu-tu").
- **Electric** (`:757-793`, `:1160-1195`). All combustion chains are gated off; the motor layers take over.
  - **Tones:** f1 (sine), f2 (×2.003), f3 (triangle, ×3.02), mesh (×4.37), regen (triangle ×1.5 → lowpass 3 kHz), saw (×0.5 → resonant lowpass Q 7, cutoff 120+f1·(2.2+1.4load)), growl (square ×0.25 → bandpass, only under nitro), hum 100/200 Hz at idle, air (pink noise → bandpass 500+1.7·f1).
  - A shared 4.3 Hz vibrato (4 cents) feeds every tone's detune.
  - Base frequency: `f1 = 45 + motor·1450`.
  - Inputs: `s.motor` (0..1.05), `s.power/600` as load, `s.regen`, `s.nitro`, speed. Output gain 3.4.
- **Damage** (`:964-986`, `:1110-1131`). Silent below damage 0.5.
  - A PeriodicWave pulse train at crank rate rings two bandpasses (1300 Hz Q 5, 420 Hz Q 3) for knock, and gates bandpassed white noise for rattle.
  - Steam is highpassed noise above damage 0.8.
  - Random misfires automate `misG` down to 0.2 for 40–120 ms, with a 40% chance of a pop.
- **Wrecked engine run-down** (`:1858-1876`): a one-shot oscillator on the car's own `onL` PeriodicWave, swept from 2600/120 to 500/120 Hz over 2.2 s, with a square-LFO amplitude stutter.

## 3. Other SFX voices

**Continuous voices.** All are built once and steered in `_updateEnvironment` (`:1199-1262`).

| Voice | Technique | Driven by |
|---|---|---|
| Wind (`:815`) | looped pink noise → lowpass 500 Hz → gain | gain 0.2·(speed/80)²·(1.35 in the air); cutoff 350+22·speed |
| Wind side bands (`:816-824`) | 2× pink noise → bandpass 1400 Hz → gust Gain → StereoPanner ±0.6; gust gain modulated by brown noise lowpassed at 0.45 Hz | speed |
| Road rumble (`:825`) | brown noise → lowpass 130 Hz | speed, offroad; 0 in the air |
| Tyre squeal (`:829-848`) | triangle 950 + sine 1930 with ±70-cent noise jitter (brown noise lowpassed at 45 Hz → detune) → bandpass; plus white noise → 2 bandpasses (Q 9, Q 2.5); all → AM gain chattered by noise lowpassed at 28 Hz | `skid`, `slip`, speed; pitch 780+520·slip+3.5·speed+120·skid; tone/noise balance from skid; muted off-road |
| Gravel (`:852-856`) | pre-rendered 2 s loop `sfxBuf.gravel` → highshelf −4 dB at 2.5 kHz | gain from offroad·speed; **playbackRate** 0.55+speed/45 |
| Wall scrape (`:859-865`) | pre-rendered 1.5 s loop `scrape` plus white noise → bandpass 2600 Hz → StereoPanner | `scrape`, playbackRate 0.7+speed/60, pan ±0.55 from `scrapeSide` |
| Nitro bed (`:868-874`) | white noise → highpass 3200 (hiss), brown → lowpass 90 (rumble), pink → bandpass 420 (flame, with a 23 Hz sine flutter on its gain) | `s.nitro`; the electric car gets hiss only |
| Rival engines ×3 (`:887-903`, `:1538-1559`) | Osc on the sports PeriodicWave (rivals are always combustion) → lowpass → gain → pan; an electric rival switches the oscillator to sawtooth | `[{dist, pan, rpmNorm, electric}]`; audible within 70 m; frequency (1200+6000·rn)/120 |
| Sirens ×3 (`:919-946`, `:1567-1606`) | square + saw (+18 cents) → bandpass 1150 Hz Q 0.9 → distance lowpass → gain → pan; pitch comes from two always-running LFOs (triangle, and a soft-square PeriodicWave) → depth gains → both oscillators' detune; per-voice drift ±4.5% | `[{id, dist, pan, relSpeed, mode}]`; doppler 343/(343−rel), rel clamped to ±120; level `sirenLevel()` (`:237-241`), 0 at 350 m; lowpass 900+9000·30/(30+d); voices allocated by `id` |
| Spiked tyres (`:950-962`, `:1881-1890`) | pulse PeriodicWave at the flap rate (0.95·speed Hz) gates bandpassed pink noise at 340 Hz, plus a lowpassed thump | `setSpikedTyres(on, speed)` |
| Transmission whine, turbo, damage | covered in section 2 | |

**One-shots.** Each creates short-lived nodes per call. The shared helpers are `_play(buf, {time, gain, rate, pan})` (`:1324`) and `_noiseBurst({time, dur, type, freq, q, gain, rate, sweepTo, pan, dest})` (`:1336`), which plays a white-noise slice through a Biquad with a linear attack, an exponential decay and an optional exponential frequency sweep.

- **impact(strength, pan)** (`:1359-1374`):
  - Pre-rendered `thud` + a random `metal1/2/3` (rate falls as strength rises) + a second metal layer above 0.45 + a noise crunch + `glass1/2` above 0.55.
  - Hits within 70 ms are thinned unless they are at least 0.2 stronger.
- **landing(strength)** (`:1376-1395`): a sine sweeping 75→32 Hz, a `thud` buffer, a noise burst, a tyre chirp above 0.25 (bandpass Q 7 sweeping 1300→1000 Hz) and `metal3` above 0.6.
- **beep(final)** (`:1398-1422`): square + sine pairs per note → shared lowpass with a decaying cutoff. Normal: 440 Hz plus its sub-octave. GO: 880 Hz chord plus a rising noise burst.
- **whoosh(pan, strength)** (`:1426-1458`), for traffic and rival pass-bys: a bandpass noise burst sweeping 380→1700→280 Hz, a StereoPanner ramp, and a saw → lowpass 700 Hz with a falling pitch (pseudo-doppler).
- **nitroBurst()** (`:1460-1499`):
  - Combustion: highpass "pssh", a bandpass rush sweeping 600→4500 Hz, a sine whump 95→38 Hz, a saw 55→110 Hz → lowpass, and a pop.
  - Electric: a saw zap sweeping 180→2600 Hz through a sweeping bandpass, plus 6 highpass crackles.
- **uiClick('click'|'start')** (`:1502-1517`): triangle blips with a short downward pitch drop, plus a 6 kHz noise tick.
- **finishFanfare()** (`:1520-1536`): uses `Music.note()` with inline brass (saw) and bell (FM) patches, the kit's `crash` and `kickPunch` buffers, and ducks the music to 0.35 for 2.4 s.
- **sirenHorn(pan)** / `_sirenBlip` (`:1609-1651`): two siren whoops (exponential frequency ramps) plus an air horn (saws at 277/330 Hz → lowpass).
- **busted()** (`:1757-1776`) and **escaped()** (`:1780-1791`): `Music.note` chords, kit hits and a sine drop; `_duckMusic`.
- **takedown(strength, pan)** (`:1802-1820`): impact + 3 slowed metal buffers + brown noise → bandpass → distortion.
- **spikePop(pan)** (`:1824-1840`): noise cracks, a sine thump and a 1.4 s air-hiss sweep.
- **wrecked()** (`:1844-1877`): impact(1), thud, metal3, 3.2 s of steam and the engine run-down.

**Pre-rendered SFX buffers.** `renderSfx()` (`samples.js:406-416`) does plain JS sample math with its own RBJ biquad class (`samples.js:20-37`). It is not an OfflineAudioContext.

- `metal1/2/3`: 14 inharmonic modal bandpass resonators, excited by noise with crumple crackle.
- `glass1/2`: a highpassed shatter plus 55 sine tinkles.
- `gravel`: a 2 s seamless loop of ~1800 bandpassed grains plus low crunch.
- `scrape`: a 1.5 s loop through 5 high-Q modes.
- `clunk`: 3 decaying sines plus a tick.
- `thud`: a falling sine, lowpassed noise and an 18-Q ring.

Loops use `loopTexture()`: render 2× the length, keep the second half, crossfade the seam.

**Noise beds.** `_makeNoise` (`Audio.js:555-583`) generates 2 s mono white, pink (Paul Kellet filter) and brown buffers with a 2048-sample seam crossfade. `_loop()` (`:585-592`) starts each loop at a random offset.

## 4. The Gate, and other CPU savings

- **`Gate` class** (`Audio.js:249-301`). The rationale is in the comment at `:250-256`: Chrome renders every node the destination pulls on, even behind a zero gain.
  - A gate holds `[node, destination]` links and *disconnects* them when the voice is idle.
  - `set(active, t)` reconnects immediately when the voice becomes active. On going idle it starts a tail (default GATE_TAIL 1.2 s, "8 time constants of the slowest fade") and shuts once the tail has run out.
  - It returns whether the voice is connected. Callers only automate params when it returns true, because a node that isn't rendered never retires its automation events.
- **Gated voices:** `_vg` holds `tunnel` (4 s), `eng`, `whine`, `turbo`, `ev`, `road`, `squeal`, `gravel`, `scrape`, `nitro` (1.6 s), `tyres`, `dmg`, `radio`. Each rival and siren voice has its own gate. `_sfxGate` (0.5 s) covers the whole SFX chain.
- **Timers:** `_gateLater` / `_gateTick` (`:400-414`) use `setTimeout` to shut gates whose tails run out while nothing calls `update()` (the menu). This only applies on a realtime context.
- **Startup state:** every gate is shut at the end of `_build` (`:551`).
- **Sources keep running:** oscillators and buffer sources are started once and never stopped. Only the output links are cut.
- **No steering before the context has run:** `_running()` / `_steer()` / `_sfxOn()` (`:430-440`) skip steering until the context has been `running` at least once, so automation events can't pile up.
- **Other savings:**
  - Drums and crash sounds are pre-rendered buffers: one source + gain per hit (`samples.js:1-6`).
  - A chord shares one filter and amp across its oscillators (`Music.js:11-16`).
  - The default latencyHint is `'balanced'`; the music player uses `'playback'` (`:333-336`).
  - Decoded radio clips are not cached (`RadioVoice.js:1-5`).
  - Music drops late steps instead of playing them late (`Music.js:310`).
  - Retired songs are disconnected via `setTimeout` (`Music.js:372-381`).

## 5. Music

**Sequencer** (`src/game/audio/Music.js`):

- **Scheduling:** a lookahead scheduler. `setTimeout` runs every 25 ms (TICK_MS) and `pumpUntil(currentTime + 0.2)` places events on the audio clock (`:18-31`, `:299-328`).
- **Late steps:** a step more than 30 ms in the past is dropped (realtime only). The loop has a guard of 256 steps per pump.
- **Offline use:** `pumpUntil` is public so an OfflineAudioContext can drive it.
- **Grid:** step = 16th note, `stepDur = 60/bpm/4`. Odd steps are delayed by `swing·stepDur` (`:460`).
- **Position:** `{sec, bar, step}`.
- **End of song:** the old song is retired at `nextT+2.5` with a 0.8 s time constant, and the next playlist entry begins at `nextT+1.2` (`:316-324`).
- **Transport:**
  - `play(id, {bar, fade})`: if the same song is already playing, nothing changes (a race restart doesn't restart the music). Otherwise the old song fades out with a 0.12 s time constant and the new one starts 0.55 s later.
  - `seek(id, bar)`: a hard cut.
  - `next()`: advances through `this.playlist`, wrapping.
  - `start()` / `stop()`: stop keeps the position; start resumes from the start of the bar.
  - `onResume()` pushes `nextT` forward.
  - `onTrack(info)` fires via `setTimeout` at the song's start time (`:366-369`).
- **Per-song graph** (`_begin`, `:331-370`):
  - bus (linear fade-in) ← song lowpass filter ← pump Gain.
  - rev send → shared Convolver.
  - Ping-pong delay: 2 DelayNodes, highpass 280 Hz in, lowpass 3200 Hz in the feedback path, ChannelMerger, delay time `T.delay·60/bpm` (default 0.75 = dotted eighth), feedback `T.delayFb` (0.38).
  - One channel per part.
- **Channels** (`_channel`, `:385-421`):
  - input → [WaveShaper tanh(drive) 2x + lowpass `driveLp`] → [highpass `hp`] → [chorus: 2 DelayNodes with LFO-modulated delayTime, panned ±0.8] → StereoPanner → level → (pump or filter).
  - Plus rev and dly sends.
- **Sidechain pump** (`_duck`, `:447-453`): every kick automates the pump gain down to 1−depth over 6 ms, then recovers with `setTargetAtTime`.
- **Shared reverb** (`build`, `:200-232`): JS-generated 2.6 s stereo impulse response (xorshift noise, a closing one-pole lowpass, 18 ms pre-delay, 10 early reflections), return gain 0.42.
- **Pulse wave:** a 25% duty PeriodicWave with 64 harmonics for the `pulse` patches.
- **Sections** (`_barStart`, `:517-538`):
  - `crash` and `drop` (the `boom` buffer) on bar 0.
  - `lp: [from, to]`: an exponential ramp of the song filter across the whole section.
  - `riser`: a noise bandpass sweeping 350→7.5 kHz over N bars (`:541-554`).
  - `down`: a downlifter sweeping 6 kHz→250 Hz.
  - `swell`: `revCrash` timed to end on the next downbeat (it may start with an offset).
  - `gap`: the last N steps of the section are silent.
  - `fill`: one of 6 FILLS, replacing the snare/hat/tom lanes from a given step (`:151-160`).
- **Humanisation:** `_human()` applies a deterministic ±6% hash to drum velocities (`:511-515`).
- **Harmony:**
  - Chord parser with 19 qualities and slash bass (`:48-59`).
  - `voice()` picks the inversion with minimal voice movement from the previous voicing (`:63-78`).
  - Bass degrees r/o/f/t/s/l/u with `~` slide/glide; arp indexes wrap up an octave (`:574-603`).
- **Instruments** (`note()`, `:607-702`):
  - Envelope: linear attack, `setTargetAtTime` for decay to sustain and for release, gain normalised by 1/√(number of notes).
  - **FM** (`type:'fm'`): per note, a sine carrier plus modulators. Each modulator → gain → `carrier.frequency`, depth `index·f·ratio`, decaying to `sus`. Optional ±`fmDetune` doubled carriers.
  - **Subtractive:** N detuned oscillators (`voices`, `detune` spread), optionally split into two StereoPanners at ±`width` → one lowpass with `fenv`/`fdec` (decaying from a peak), `fattack` or `keytrack` → amp.
  - Optional `sub` oscillator (octave down unless that is below 40 Hz).
  - Optional delayed vibrato LFO into `detune`.
  - `bend` / `glide` as exponential frequency ramps (`_bend`, `:704-713`).
- **Kit:** `KIT` (`:166-182`) maps 15 voices to buffers, levels and reverb sends; tracks can override them. DRUM_TRIM is 0.16.

**Song data** (`tracks.js`, format documented at `:1-26`):

- 22 patches in `P` (`:29-53`), also exported as `PATCHES`.
- Each song has `{id, title, style, bpm, swing?, gain, delay?, delayFb?, pump?{depth, release}, kit?{voice:{s, g, rev, rate}}, prog{key: 'chord per bar, commas within a bar'}, drums{name:{voice: '16-char lanes X/x/o/.'}}, parts{name:{type: bass|chord|arp|mel, inst, lo, res, legato, gate, ch{level, pan, rev, dly, pump, drive, driveLp, hp, chorus}, pat{key: string}}}, sections[{bars, prog, drums, p{part: patKey}, crash, drop, fill, riser, swell, down, gap, lp}]}`.

| Song | Style | BPM | Line |
|---|---|---|---|
| Midnight Run | Outrun | 110 | `:60` |
| Seabright Dawn | Chill drive | 92 | `:99` |
| Neon Rush | Drum & bass | 172 | `:133` |
| Mirage Highway | Desert western | 96 | `:173` |
| Interstate Nights | Night drive house | 122 | `:206` |
| Chrome Heart | Darksynth | 118 | `:242` |
| Afterburner | Breakbeat acid | 128 | `:279` |

- **`LEVEL_TRACK`** (`:318-325`): sierra→midnight-run, coast→seabright, streets→neon-rush, desert→mirage, seaside→afterburner, cruise→interstate.
- **`PLAYLIST`** (`:326`) sets the next-track order. `levelTrack(unknown)` returns `PLAYLIST[0]`.
- **How the game picks music** (`main.js:168-175`): `pickMusic` plays `settings.track` or the level's own track, and only switches when the level|track key changes. T and the pause screen's Next track button call `nextTrack()`.

**Drum kit** (`samples.js:228-260`): rendered once with pure JS math into AudioBuffers. It is **not** an OfflineAudioContext.

- 5 kicks: a sine with a pitch drop, a click transient and tanh saturation.
- 4 snares: tuned body, noise, crack bandpass, and an optional gated "plate" tail.
- 2 claps: 4 bursts plus a tail.
- Hats, open hat, ride and crash: 808-style six detuned squares plus noise through highpass/bandpass filters; the ride has bell partials and the crash a darkening lowpass.
- `revCrash` (the crash reversed), shaker, rim, snap, 3 toms, and `boom` (a 2.4 s sub drop).
- All are peak-normalised by `finish()`.

**OfflineAudioContext** appears only in:
- `tools/audio-test.html:144-176` (`renderTrack`): a whole GameAudio on an offline context, sfx 0, driven with `oc.suspend(t)` → `pumpUntil` → resume. It produces a 16-bit WAV plus RMS, peak and RMS per 5 s.
- `Music.realtime`, which detects an offline context (`Music.js:191`).
- The `init({context})` option (`Audio.js:332-351`).

**Music player** (`music.html`):
- Calls `new GameAudio().init({latencyHint:'playback'})` and `setVolume({master:1, sfx:0, music:vol})`. With SFX at 0 the effects graph is fully gated off (`:247-265`).
- An AnalyserNode (fftSize 4096, smoothing 0.7) is tapped off `audio.limiter` (`:256-261`).
  - Meter: RMS and peak from `getFloatTimeDomainData`, with a 1.2 s peak hold (`:449-465`).
  - Spectrum: 56 log bands from 30 Hz to 16 kHz from `getByteFrequencyData` (`:427-447`).
- Solo sets `audio.music.solo` to a part name or `'drums'`; `_step` honours it (`Music.js:478-499`).
- Seek and section jump call `audio.music.seek(id, bar)` (`:271-282`).
- Repeat sets `audio.music.playlist = [id]` (`:275, 507-512`).
- The playhead comes from `music.pos`, which runs about 0.2 s ahead of what is heard (`:332-337`).
- Pause and resume use `setPaused`. Every gesture calls `unlock()` (`:266-268`).
- Test hooks are `window.__audio` and `window.__player`.

## 6. Police radio

- **Lines** (`radioLines.js`):
  - The `RADIO` builders (`:24-42`) return `{text, parts}`. Parts usually equal the text; `intercept` splits off the callsign.
  - `clipId(words)` lowercases and joins with hyphens (`:20`).
  - `radioClips({zones, units, names})` enumerates every clip; fixed lines get `TAKES = 2` (`:46-70`).
  - `levelsRadioClips(LEVELS)` (`:74-80`).
- **Assets:** `audio/radio/` holds 202 entries (about 201 MP3s plus `index.json`), 3.0 MB, 16 kHz mono (README `:116-124`). `index.json` has the form `{clips: {id: takes}}`. Takes after the first are named `id.N.mp3`.
- **Loader** (`RadioVoice.js`):
  - `load()` fetches `index.json`. If it fails, the index becomes `{}`.
  - `prefetch(ids)` fetches every take's ArrayBuffer using 3 concurrent workers. The bytes are cached; decoded audio is not.
  - `buffers(ctx, parts, rng)` picks a random take per part and returns null if any part is missing. It calls `decodeAudioData` on a `slice(0)` copy each time.
  - PursuitView prefetches the race's clips at race start (`PursuitView.js:51-53`).
- **Line flow:**
  - `PursuitView.say(line, force)` (`:235-240`) rate-limits to one line per 4 s (RADIO_GAP, `:16`) unless forced. It shows the subtitle with `hud.radio(text, max(3, len/14))` and calls `audio.radioLine(parts)`.
  - `HUD.radio` (`HUD.js:101-108`) replaces the current subtitle; it has no queue.
  - `radioLine(parts, pan)` (`Audio.js:1656-1665`) races the decode against a 700 ms timeout (RADIO_WAIT). A sequence number makes a newer line win. On timeout it falls back to the burble, with a length based on the text.
- **Radio bus** (`Audio.js:988-1001`): `rd.in` (Gain 1.6) → WaveShaper (`distortionCurve(2.6)`, no oversampling) → 2× highpass 340 Hz → 2× lowpass 3000 Hz (cascaded 12 dB/oct stages, Q 0.7) → out (0.22) → StereoPanner → `sfxBus`. It is gated.
- **Playing a line** (`radio()`, `:1671-1703`): one channel. A new call cuts the current one with `_radioCut` (a 12 ms fade, sources stopped).
  - Per line: squelch-open click (12 ms highpass noise) and rush (70 ms bandpass), then carrier hiss (looped white noise → bandpass 1700 Hz, gain 0.045).
  - Then either the voice buffers in sequence (gain RADIO_VOICE 1.6, 0.14 s gaps) or the burble.
  - Then squelch close: a 160 ms bandpass "kssht" plus a click.
- **Fallback burble** (`_burble`, `:1706-1744`):
  - A sawtooth "buzz" plus breath noise → 3 bandpass formants (Q 5/8/10).
  - Each syllable (60–210 ms) sets the formants to a random vowel from a 7-vowel table, with F1 floored at 380 Hz.
  - The pitch sags across the phrase with per-syllable inflection, `vox.gain` gives syllable envelopes, and random consonant noise bursts are added.
- **Pursuit reset:** `_pursuitReset` (`:1915-1922`) runs on `setCar` and whenever `update` sees the engine off with no motor field. It clears sirens, spiked tyres, mood, damage and the radio.

## 7. Browser handling

- **iOS audio session:** `askForPlayback(nav)` (`Audio.js:265-269`) sets `navigator.audioSession.type = 'playback'` if the API exists and the type differs, inside a try/catch. `init` calls it *before* creating the context, and not when a test context is passed in (`:347`).
- **Context creation:** `init(opts)` (`:342-360`) is memoised. It uses `window.AudioContext || webkitAudioContext` with `latencyHint` defaulting to `'balanced'`. It builds the graph on a possibly suspended context and never awaits resume.
- **Unlock:** `unlock()` (`:365-369`) calls `ctx.resume()` unless the game is paused, the context is running, or it is closed.
  - `main.js:199-204` calls `init()` + `unlock()` on every `pointerdown`, `pointerup`, `touchend`, `click` and `keydown` (capture, passive).
  - This also restarts a context iOS has interrupted (e.g. by a phone call).
- **Pause:** `setPaused(p)` (`:1924-1932`) calls `ctx.suspend()` or `ctx.resume()` and then `music.onResume()`. `_paused` blocks `update()` and `unlock()`.
- **Tab hide:** `main.js:472` pauses the race on `visibilitychange` when the document is hidden, which suspends the audio. Audio.js has no visibility handling of its own.
- **OfflineAudioContext:** covered in section 5. There are no other uses.

## 8. The `update()` interface and event calls

**`update(dt, s)`** (`Audio.js:1008-1062`) reads these fields of `s`:

- `rpm`: < 100 means the engine is off.
- `rpmMax`: default 7800.
- `throttle`.
- `speed`: m/s, abs taken.
- `onGround`.
- `gear`: −1 means reverse.
- `boost`: turbo, 0..1.
- `skid`, `slip` (rad), `offroad` (0..1), `scrape` (0..1), `scrapeSide` (±1).
- `nitro`: boolean.
- Electric only: `motor` (0..1.05; its presence also means "running"), `power` (kW; /600 = load), `regen`.

The caller is `Race.js:375-380`. The menu calls `update` with a stopped engine (`main.js:417`).

**Per-frame setters:**
- `setRivalEngines([{dist, pan, rpmNorm, electric}])` (Race.js:388).
- `setSirens([{id, dist, pan, relSpeed, mode}])`, `setPursuitMood('off'|'pursuit'|'cooldown')`, `setDamage(0..1)` and `setSpikedTyres(on, speed)` (`PursuitView.js:323-341`).

**Events:**
- `shift(up)`, `impact(strength, pan)`, `landing(strength)`, `beep(final)`, `whoosh(pan, strength)`, `nitroBurst()` (on the nitro rising edge), `finishFanfare()` (Race.js:193-324, 381).
- `setEnvironment('tunnel'|'open')` (Race.js:395).
- `sirenHorn(pan)`, `escaped()`, `takedown(strength, pan)`, `spikePop(pan)`, `busted()`, `wrecked()`, `radioLine(parts, pan)`, `radio(duration, pan, voice)`.
- `uiClick('click'|'start')` (main.js:208).

**Control and transport:**
- `init(opts)`, `unlock()`, `setVolume({master, sfx, music})`, `setMusic(on)`, `playTrack(id)`, `nextTrack()` (returns info), `setCar(kind)`, `setPaused(p)`.
- Getters: `trackInfo`, `ready`, `car`, `environment`. Callback: `onTrackChange(info)`.
- Static: `GameAudio.tracks`, `GameAudio.levelTrack(id)`.
- Exports: `SIREN_PATTERNS`, `sirenDoppler`, `sirenLevel`, `askForPlayback`.
- `radioVoice.prefetch(ids)` is called directly.

## 9. Worklets and ScriptProcessors

None, as stated at the top. Node types in use:
- Oscillator (built-in types and PeriodicWave), AudioBufferSource (looped and one-shot, with playbackRate).
- Gain, BiquadFilter (lowpass, highpass, bandpass, peaking, lowshelf, highshelf), WaveShaper.
- DynamicsCompressor (×3), Convolver (×2), Delay, ChannelMerger, StereoPanner.
- Analyser (`music.html` only).

All automation uses `setValueAtTime`, `linearRampToValueAtTime`, `exponentialRampToValueAtTime`, `setTargetAtTime` and `cancelScheduledValues`. There is extensive audio-rate modulation, where node outputs connect into AudioParams such as `detune`, `gain`, `frequency` and `delayTime`.

## 10. Rough counts

**Persistent nodes**, counted by hand from the build code:

| Area | Nodes |
|---|---|
| Master + SFX bus | ~6 |
| Music bus | ~8 |
| Tunnel | 3 |
| Engine | ~59 |
| Turbo | 8 |
| Electric | ~29 |
| Environment | ~59 |
| Rivals | 12 |
| Pursuit (sirens 36, tyres 7, damage 12, radio bus 8) | ~63 |
| Music reverb | 2 |
| **Total** | **~250** |

- **Always-running sources:** about 40 oscillators and about 23 looped buffer sources, all started at build time. Gates decide which ones actually render.
- **Music, per song:** 12 bus/delay nodes, plus 4–15 per channel, plus a drum gain per voice. Per note there are roughly 3–15 nodes (oscillators × voices, sub, lowpass, amp, vibrato); per drum hit, 2. The file comment says a busy 16th note costs "a couple of dozen nodes".
- **One-shots:** 3–30 nodes each (an impact is about 5 sources with gains and panners; a GO beep is 8 oscillators plus a filter, gains and a noise burst).

**Lines of code**, `Audio.js` (1973 total):

| Range | Subsystem | Approx. lines |
|---|---|---|
| 1-241 | helpers, car profiles, wave generation, siren math | 240 |
| 249-440 | Gate, init, volumes | 190 |
| 442-553 | setCar, setEnvironment, `_build` | 110 |
| 555-727 | noise, tunnel, engine build | 170 |
| 729-1005 | turbo, electric, environment, rivals, pursuit builds | 275 |
| 1007-1299 | update and steering, pops | 290 |
| 1301-1559 | one-shots, rivals | 260 |
| 1561-1932 | pursuit API (radio about 100 of these) | 370 |
| 1934-1973 | music facade | 40 |

Other files: `Music.js` 714, `tracks.js` 326 (about 290 of it data), `samples.js` 416, `radioLines.js` 80, `RadioVoice.js` 63, `music.html` 541 (about 340 JS), `tools/audio-test.html` 306.

## 11. What the tests assert

- **`test/unit/music.test.js`** builds a strict `FakeContext` (`:21-107`). It flags:
  - invalid oscillator or filter types;
  - non-finite or negative times;
  - `exponentialRamp` to 0;
  - a bad `setTarget` time constant;
  - `connect(undefined)`, a bad `oversample` value, bad `start`/`stop` arguments, and PeriodicWave arrays of different lengths.

  Tests:
  - `noteToMidi` values, including Cb4=59 and B#3=60, and rejection of malformed tokens.
  - Track ids are unique; PLAYLIST equals the set of TRACKS; every level has an existing LEVEL_TRACK; `levelTrack('nope')` is `PLAYLIST[0]`; the README's word for the track count matches `TRACKS.length`; `Music.tracks` exposes exactly `{bpm, id, style, title}`.
  - Per track: it compiles and caches; chord names match a regex; drum lanes are multiples of 16 and use only `Xxo.`; sections reference real progressions, drums, parts and patterns; a riser fits its section; 0 < gap < 16; bpm is within 60–200.
  - Each patch creates `voices` oscillators of the expected type (`pulse` becomes `custom`) plus a sub oscillator if the patch has one.
  - Per track: it plays start to finish by pumping in 0.5 s steps; there are no problems; every drum hit has a buffer; every part named in each section played a note; `onTrack` fires `[id, nextInPlaylist]`.
  - `next()` while stopped queues the next song; `play()` of the current song keeps the same song object; `next()` wraps; `stop()` sets `on=false`.
- **`test/unit/radio.test.js`:**
  - `clipId` and `placeName` formatting.
  - The clip list covers exactly every part of every line, once.
  - The intercept line's split.
  - Fixed lines have `TAKES` takes.
  - Every callsign has a recording.
  - Each level's preload list is a subset of the recordings.
  - `audio/radio/index.json` matches the takes, every MP3 exists and is ≥ 1000 bytes, and there are no extra MP3s.
  - `RadioVoice` (fake fetch and decode): picks a take by rng, returns null for a missing part, prefetch fetches each take exactly once and later lines need no new fetches, and a missing `index.json` gives null without throwing.
- **`test/unit/pursuit-audio.test.js`:**
  - SIREN_PATTERNS values.
  - `sirenDoppler(0)=1`, `(30)=343/313`, `(−30)=343/373`, clamped to (0.5, 2), `undefined` gives 1.
  - `sirenLevel` is strictly decreasing, between 0.2 and 0.5 at 0 m, above 0.02 at 100 m, and 0 at 350 m and beyond.
  - Every pursuit method is a no-op before `init` (`ctx` stays null).
- **`test/unit/audio-session.test.js`:** `askForPlayback` sets `'playback'`; it doesn't re-set when already `'playback'`; and it doesn't throw with no API, an undefined navigator, or a throwing setter.
- **`test/e2e/audio.test.js`** (Puppeteer, real Chrome):
  - **Phone:** there is no context before the first touch; the Race tap makes the context run; `musicGate` rises above 0.5; default music volume is 0.7.
  - **iOS session:** `audioSession` is set to `'playback'` once, before the context exists.
  - **Pause:** suspends the context; a non-Resume tap leaves it suspended; Resume makes it run.
  - **Interruption:** an external `ctx.suspend()` recovers on the next touch (phone) or key (desktop).
  - **M key:** mutes (localStorage `musicVol` = 0, `_musicOn` false, both sliders at 0), unmutes to 0.7, and the mute survives a reload (a race then starts without music).
  - **Tracks:** the level starts on `LEVEL_TRACK.sierra`; T and the pause screen's Next track change the track; the pause screen names it.
  - **Sliders:** menu and pause sliders stay in sync, persist, and `_vol` matches.
  - **Track picker:** options are `auto` plus every track; the choice persists and plays; going back to `auto` returns to the level's own track.
  - **Hard driving:** no console warnings or errors.

---

## What ports to sample-level DSP and what leans on Web Audio built-ins

**Ports cleanly (already sample math, or simple per-sample structures):**
- All of `samples.js`. It already has its own RBJ biquad, tanh saturation and modal resonators, and is pure functions of (sampleRate, seed).
- Noise bed generation and the reverb impulse-response generators.
- The engine Fourier series in `engineCycle` and `rumbleCycle`. The output is just harmonic arrays, so a Rust port needs its own band-limited additive or wavetable oscillator to stand in for PeriodicWave.
- The sequencer logic: pattern compilation, voicing, `pumpUntil` stepping and humanisation. It is pure and deterministic, though it schedules by audio-clock time.
- The siren, doppler and level math, the Gate idea, and all of `radioLines.js` and `RadioVoice.js`'s selection logic (MP3 decoding is a separate need).

**Depends heavily on Web Audio built-ins and their exact behaviour:**
- **BiquadFilter:** the most-used node type, everywhere — engine formants (peaking), lowshelf/highshelf, every noise voice, the radio band. Many cutoffs are automated per frame.
- **WaveShaper:** engine exhaust and rumble use 2x oversampling; the music drive is 2x; pops, takedown and the radio crunch use none.
- **DynamicsCompressor ×3:** master limiter, SFX glue and music glue. Their exact knee, lookahead and release behaviour is the browser's own, and the trims were calibrated against them (`:243`).
- **Convolver ×2:** the 1.4 s tunnel and 2.6 s hall impulse responses, so the port needs convolution (or an equivalent reverb).
- **PeriodicWave:** engine on/off/rumble waves, rival waves, the siren soft-square LFO, the damage and tyre pulse trains, and the music pulse lead. These rely on browser-side band-limiting and the `disableNormalization` semantics.
- **AudioParam automation semantics:** `setTargetAtTime` is the primary smoothing mechanism for every steered parameter. The code also uses `exponentialRamp` sweeps, `cancelScheduledValues`, and the timeline ordering of overlapping events (envelopes in `Music.note`, ducking, misfires).
- **Audio-rate AudioParam modulation:** noise into oscillator detune and gain, the engine oscillator into rasp gain, FM modulators into `carrier.frequency`, LFOs into `delayTime` (chorus) and siren detune, pulse waves into gain.
- **DelayNode with feedback loops** (ping-pong, sub-block feedback) and **StereoPanner's equal-power law**.
- **Scheduling model:** fire-and-forget nodes started at future `ctx.currentTime` instants (one-shots and every music note), plus `AudioBufferSource.playbackRate` resampling for gravel, scrape, pops and the kit's rate overrides.
- **Graph connect/disconnect** as the CPU-saving mechanism (the Gate). This is specific to how Chrome renders the graph.
- **Platform APIs:** `decodeAudioData` for the radio MP3s, AnalyserNode FFT for the music player, `navigator.audioSession`, and context suspend/resume/unlock.
