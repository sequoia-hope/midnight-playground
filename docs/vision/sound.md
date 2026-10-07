# Sound: engines with weight, music with depth

Status: **draft design, for the owner to shape.** This is vision ROADMAP
M12: the first thing after multiplayer, ahead of any world building. The
owner's view: the game as it is is fun, and better sound is worth more than
more world.

The port (port SPEC section 7) reproduces today's audio exactly. Everything
here is a deliberate change made after cutover, recorded in DEVIATIONS.md
as it lands.

## 1. What is wrong today

### 1.1 Engines

The engine is clever (`Audio.js` header): one full 720° four-stroke cycle
is drawn with a pressure pulse per cylinder, timed by the bank layout,
turned into a band-limited periodic wave, crossfaded on- and off-throttle,
filtered through exhaust formants, with a separate rumble layer.

It still sounds **whiny, not rumbly**, and the reasons are structural:

- **It is perfectly periodic.** Every cycle is identical, so the ear hears
  a pitched tone that sweeps with rpm: a whine. A real engine never fires
  two cycles alike. The cycle-to-cycle variation (combustion strength,
  timing jitter, misfire-like stumbles at idle) is what reads as "engine"
  instead of "synth".
- **The low end is a filtered layer, not the physics.** A V8's rumble is
  the exhaust: pulses travelling down headers of unequal length, merging,
  and ringing in pipes, resonators and mufflers. Those resonances are fixed
  in frequency while the firing rate moves through them; that is the
  "deep" part. Formant filters approximate it, and only weakly.
- **There is too much energy in the upper harmonics** relative to the
  sub-firing orders and the pipe resonances, so the brightest part leads.
- **Small speakers lose the bass.** Phones and laptops cannot play 40 Hz,
  so a rumble mixed for headphones disappears there and the whine is all
  that is left.

### 1.2 Music

The songs' writing is decent (seven styles, arrangements, sections,
sidechain pump), but they **sound like "Claude made some synthwave"**:

- **Raw browser oscillators and biquad filters** sound thin, clean and
  static: no analogue drift, no saturation, no filter character. Every
  synth sounds like every other.
- **The drum kit is generic,** where the electronic genres the owner loves
  are defined by specific machines.
- **Arrangements are step loops** that repeat exactly; nothing evolves
  inside a section.
- **The mix is dry and digital:** no tape or bus saturation, no glue, one
  shared reverb.

## 2. Engines: a physical exhaust

### 2.1 The model

Replace the periodic wavetable with a small **physical model run per
sample**:

1. **Combustion:** each cylinder fires a pressure pulse at its point in the
   firing order. Each pulse has its own strength and timing, drawn with
   cycle-to-cycle variation that grows at idle and on overrun and shrinks
   under load.
2. **Headers:** each pulse enters its own **waveguide** (a delay line with
   loss and reflection) whose length is the header length. Unequal headers
   on a cross-plane V8 give the burble; equal ones on a flat-plane V8 or V12
   give the scream.
3. **Collector and pipe:** the header outputs merge and travel down the
   exhaust pipe (another waveguide), through resonator and muffler models
   (tuned resonant sections), to the tailpipe radiation filter.
4. **Intake** (a short waveguide with the throttle as an opening),
   **mechanical noise** (valve train ticks, a faint gear whine), and
   **overrun crackle** (unburnt fuel pops in the pipe off-throttle).

Each engine type is then a parameter set: cylinder count, firing order,
bank angle, header and pipe lengths, muffler tuning. A cross-plane V8, a
flat-plane V8, a V10, a V12, a turbo four, the 3000GT's twin-turbo V6,
and the S4's V8 come from one model with different numbers.

**Exhaust as a tuning option.** The exhaust system is its own parameter
group, so a car can carry a stock and an aftermarket one: the lab's
`audiV8awe` is the S4's V8 with an AWE Tuning cat-back (straight-through,
larger bore: less loss, more low drone, bigger tips, a little more
burble) beside the stock `audiV8`. In the game this becomes a garage
choice (cars and classes, ROADMAP).

**The reference** that this approach can sound real: AngeTheGreat's
open-source Engine Simulator, which simulates combustion and exhaust
acoustics and produces convincingly real engines. It is far too heavy to
run as it is (it is a full gas-dynamics simulation); the waveguide model
above is the cheap version of the same idea.

### 2.2 Making the rumble survive small speakers

- **Psychoacoustic bass:** generate harmonics of the sub-bass so the ear
  infers the missing fundamental. Phones then "hear" a rumble they cannot
  play.
- **Mix priority:** the engine's low end ducks the music's bass, not the
  other way round, in the 40–150 Hz band only.
- A **speaker profile** setting (headphones, laptop, phone, big speakers)
  that sets how much of each is applied.

### 2.3 Cost

A few waveguides and filters per engine at 48 kHz: modest for the player's
car. Rival engines stay on the cheap wavetable voice, which is fine at a
distance; the nearest rival can switch to the full model.

## 3. Music: real instruments, evolving arrangements

### 3.1 Instruments with character

Two sources, both open:

- **Emulations of the classic machines** that define the genres:
  - Roland **TR-808** (deep, booming kick; the sound of hip-hop and much
    house) and **TR-909** (the house and techno kick, hats and claps);
  - the **TB-303** bassline (acid; the `acid` instrument is a start);
  - **Juno-style** pads with their chorus, **Moog-style** ladder-filter
    basses and leads, **DX7-style** FM keys and bells (the `ep` and
    `bell` instruments are a start).
  Built as proper virtual-analogue DSP: band-limited oscillators that
  drift, zero-delay-feedback ladder and state-variable filters that
  saturate, and per-voice analogue variation.
- **Mutable Instruments' modules.** The company is defunct, but its
  Eurorack firmware is published under the MIT licence. The modules were
  microcontrollers running DSP code, so **the code is the instrument**:
  running it reproduces the module exactly, deterministically. And
  because it was written for 72 MHz embedded chips, it is cheap enough
  for phones. The most useful:
  - **Plaits:** a macro oscillator with many synthesis models (virtual
    analogue, FM, wavetable, chords, speech, drums). I believe a Rust port
    of its DSP already exists, which would be the place to start.
  - **Rings:** a resonator (strings, plates, sympathetic strings).
  - **Elements:** modal physical modelling (bowed, blown, struck).
  - **Clouds:** granular texture: pads, ambience, freezes.
  - **Marbles:** a random generator built for **controlled randomness in
    sequences**: the core of generative variation (section 3.2).

### 3.2 Arrangements that evolve

- **Genre grammars,** not fixed loops: house (four-on-the-floor, offbeat
  hats, chord stabs, 8- and 16-bar phrases, breakdowns and builds),
  techno (hypnotic loops that evolve through slow filter and parameter
  motion), drum & bass, breakbeat, ambient for the crawler trails,
  synthwave kept as one style among many.
- **Controlled randomness** (Marbles style), seeded, so a track varies
  each time without losing its identity.
- **Music follows the game:** energy rises in a pursuit or a final lap,
  opens up on a straight, drops to a breakdown at a standstill.
- **Radio stations are genres** (WORLD 5.2): a house station, a techno
  station, a chill station, each with its own DJ voice and sound.

### 3.3 A real mix

Tape and bus saturation, a glue compressor per bus, better reverbs
(generated impulse responses per space: tunnel, forest, city canyon),
proper stereo, and loudness targets per station.

### 3.4 What makes a generated track grab (the composer)

The first grammars (2026-10-07) had real instruments and a real mix and
still did not grab, and the reasons were structural, not sonic:

- **Every step was a coin flip.** A lane made step by step from independent
  probabilities has no shape: no motif, no question and answer, no pickup
  into the downbeat. It is statistically uniform, and the ear hears that.
- **No melody.** Random arpeggio indexes and a random acid lane; nothing
  to sing back.
- **Harmony with no pull.** The chords were pleasant and static, and the
  bass only ever played the current chord, never led into the next one.
- **Déjà vu was a random walk.** Each block mutated the previous one, so the
  loop drifted away from its identity. Marbles locks; it does not walk.
- **A flat tension curve.** Nothing changed every 4 or 8 bars, and the
  drop was the groove plus a boom instead of the fullest, brightest point.

The composer (`music-lab/compose.js`) is what the grammars now share, and
each point above has its answer:

- **One hook per track.** A motif is a rhythm (per-beat cells or a whole-bar
  figure such as the tresillo) plus a contour (intervals in scale steps,
  shaped as a rise, fall, arch, valley or wave). Realising it over a chord
  snaps the strong beats to chord tones, so the same figure follows the
  harmony and is recognisable when it comes back over a different chord.
  Eight bars are two phrases: the first ends open (3rd or 5th), the second
  closed (the root). The hook is stated **thinned** in the breakdown (the
  downbeat notes only, same shape), **in full** at the drop, and
  **answered** (a third up) in the drop's second half.
- **Keys and chords, not chord names.** A scale (minor, dorian, phrygian,
  harmonic minor, major, mixolydian), diatonic chords on its degrees with
  the extensions a genre wants, and progression families: the axis loops
  (i VI III VII, i VII VI VII, VI VII i) that carry trance, eurobeat and
  big house; still verses; deep two-chord vamps; drones for techno and psy.
- **The bass leads.** A lane degree `n` plays the next chord's root, so a
  bass line picks up into every change.
- **Rhythm with a shape.** Euclidean patterns (E(5,16) is the "x..x..x."
  family, E(3,8) the tresillo), cells chosen once per bar and repeated,
  metric accents, 4-bar phrases with a turnaround, 8-bar blocks with the
  kick out before the crash, odd-length percussion that phases.
- **Déjà vu that returns.** Variations are made from the core once; each
  8-bar block plays the core (probability déjà vu) or a variation, and the
  first block of every section is the core.
- **A form.** Intro, groove, break, build, drop, and the seams placed by
  it: crash and sub boom into a rise, downlifter into a fall, riser,
  snare build (beats, 8ths, 16ths, roll) and gap before a drop.

**Genres**, each a vocabulary over the same composer: house (deep: dorian
9ths, organ or EP riff; anthem: axis chords, piano stabs, a choir, a hook),
techno (acid: phrygian drone, a 303 motif under slow filter motion, dub
chords, phasing percussion; melodic: a 16th sequence on a pluck, chords
every two bars, a wide pad), trance (the rolling off-beat bass from bar
one, a pluck arpeggio, the supersaw hook alone over pads in a long break,
the snare build, the drop), **eurobeat** (the touge: octave bass on the
8ths, gated snare, a dense saw-lead chorus, brass hits, a 16th riff, and
the last chorus a whole step up), psytrance (the rolling root bass between
the kicks, a 303 squelch, FM zaps in phrygian), drum & bass (liquid: 2-step
with ghost snares, sub and reese on long notes, 7ths on EP, a pretty hook;
roller: darker, bass on the 8ths) and UK garage (the kick skips, snare on 2
and 4, swung skippy hats, organ bass with pickups, chopped chords, vowel
stabs).

**What gets someone in the zone.** On the dance floor it is a steady pulse
with slow, directed change: the hypnotic genres (techno, psy, trance) hold
one idea and move the filter, and the drop is a release that was earned by
the break. On twisty mountain roads it is eurobeat and trance: a tempo
above 150, an off-beat engine of a bass, a lead that sings, a key change at
the end. The instruments added for these: a seven-oscillator supersaw with
the JP-8000 spread, a formant choir (vowel band-passes after the voices),
trance and psy basses, an FM piano, a zap.

## 4. Architecture

Port SPEC 7.1 kept the browser's own Web Audio nodes and rejected one Rust
DSP engine in an AudioWorklet: the engines available needed nightly Rust,
shared memory and cross-origin isolation headers that GitHub Pages cannot
set. Everything in sections 2 and 3 needs custom per-sample DSP, so that
decision is revisited, narrowly:

- **A small, separate DSP module** in Rust, compiled to its own wasm and
  instantiated **inside the AudioWorklet**: no threads and no shared
  memory, so stable Rust and no special headers. Parameters arrive by
  message port and audio parameters. Natively, the same Rust code runs
  directly in the native backend.
- The existing Web Audio graph stays for everything that sounds fine,
  and becomes the mixer around the new voices.
- **Deterministic:** the DSP is plain Rust on `mp_math`'s kernel, so a
  rendered song or engine sweep is the same on every platform, and
  offline renders become regression tests.
- **Budgets measured on the phones** before each new voice ships, as
  port SPEC 6.6 does for rendering.

## 5. Milestones

| | What | Gate |
|---|---|---|
| **S0. Listening kit** | Offline renders of every engine and song; spectrum and loudness reports; a sound test screen with the speaker profiles | The owner can A/B any change on headphones, laptop and phone |
| **S1. DSP worklet** | The separate wasm DSP module in the worklet and natively, one test voice, measured on the phones | Works on GitHub Pages; cost known |
| **S2. Engines** | The physical exhaust model, cycle variation, small-speaker bass, parameter sets for every car | The owner hears a deep rumble on a V8, on headphones and on a phone |
| **S3. Instruments** | 808, 909, 303 and Juno-style voices; Plaits first among the Mutable Instruments | One existing song re-voiced sounds "real", in the owner's judgement |
| **S4. Arrangements** | Genre grammars, seeded variation, game-driven energy | A house track and a techno track the owner would listen to outside the game |
| **S5. Stations** | Stations by genre, DJ voices, the mix | The owner leaves the radio on for a session (vision ROADMAP M16) |

## 5.1 Where it stands (2026-10-07)

- **S2, engines:** the engine lab (`tools/engine-lab.html`) and its Rust
  port `mp_exhaust`, which plays the player's car in the game (D1110).
- **S3 and S4, music: the Music Lab** (`tools/music-lab.html`), a listening
  prototype in the same style. Per sample in an AudioWorklet:
  - instruments (`music-lab/instruments.js`): a TB-303 (accent with the
    accent capacitor's build-up, fixed-time slide, the 18 dB filter), a
    Juno-style poly (DCO saw, PWM pulse, sub, noise, HPF, saturating
    ladder, chorus I / II / I+II), a Moog-style mono (three drifting
    oscillators, ladder with drive, glide, vibrato), a 4-operator FM voice
    (EP, bell, stack, organ algorithms), and TR-808 and TR-909 kits
    synthesised per hit. Every oscillator drifts a little, per voice.
    Not yet: the Mutable Instruments ports (Plaits first).
  - the game's seven songs, re-voiced: each game patch maps to a lab
    patch (`BPATCH`), the sequencer plays the game's song format as
    Music.js does, and A (the game's music today) plays in step for
    comparison. Every part was levelled against A by soloing it in both,
    so A/B compares sound, not loudness.
  - genre grammars (`music-lab/gen.js`) over the composer
    (`music-lab/compose.js`, section 3.4): house, techno, trance, eurobeat,
    psytrance, drum & bass and UK garage from a seed, each with one hook
    stated sparse in the break and in full at the drop, bass pickups into
    the next chord, 8-bar blocks with a turnaround, a Marbles-style déjà
    vu control over how often a block plays the core pattern, section
    automation, and a live energy control (layers drop out and the mix
    closes down) for the game to drive.
  - the mix (`music-lab/engine.js`): channels with drive, high-pass, pan
    and sends, sidechain pump, a feedback-delay-network reverb, ping-pong
    delay, glue compressor and tape saturation on the bus.
- **Cost:** a busy song takes 15 to 35 % of one desktop core in the
  worklet's JavaScript (the page shows the audio thread's load). The Rust
  port, once the owner has picked the sound, is the step that makes it
  cheap enough for phones in the game, as `mp_exhaust` did for engines.

## 6. Questions for the owner

1. **Reference music:** which artists or tracks in house and techno (and
   anything else) are the target sound? A short list sets the grammars
   and the instrument choices better than any description.
2. **Reference engines:** which cars' sounds are the target? A list of
   cars to study (by ear, from recordings) for each engine type.
3. **Recorded sound:** the game is generated-only today. Is that a rule
   to keep, or are recorded samples allowed where they are clearly better
   (for example the owner recording real cars)?
