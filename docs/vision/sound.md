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

## 3.5 Three genres off the dance floor (2026-10-08)

The owner asked for country, classical and Peruvian chicha, specified to
the depth of 3.4, with chicha built first. Chicha is in the lab (the
`chicha` grammar, the plucked string, the latin kit; 3.5.3 describes what
was built); country and classical are specified here and wait for their
turn. Each is written the same way: the sound and its sub-styles, the
instruments (what the lab has, what it needs), rhythm, harmony, melody,
form, how the game's energy maps onto it, what the composer lacks for it,
and what makes it grab.

### 3.5.1 Country

**The sound.** Four sub-styles, picked per seed as house picks deep or
anthem:

- **Honky-tonk and Bakersfield** (Buck Owens, Merle Haggard): the
  telecaster's twang, the pedal steel, the *train beat*, a two-beat bass,
  120–136 bpm. The road-trip default.
- **The shuffle** (the two-step ballad): a triplet feel at 72–92 bpm,
  brushes on the snare, the steel's swells, the fiddle's long lines.
- **Bluegrass**: no drums. Banjo rolls, the mandolin's chop on 2 and 4,
  the fiddle, an upright bass, 140–170 bpm; everybody takes a break
  (solo) in turn.
- **Outlaw and road** (Waylon, early Steve Earle): straight 8ths at
  116–130, a baritone guitar, a Hammond, the bVII of mixolydian.

**Instruments.** Nearly all of them are the plucked string of 3.5.4 with
different numbers; the exceptions are bowed or blown:

- *Telecaster*: the string with a hard `pick`, the bridge pickup's peak
  (body 3.2 kHz, Q 2), light compression, and the chicken-pickin' pop: a
  muted ghost pluck (`decay` 0.05) between the notes, which the grammar
  writes as extra 'o' onsets. Slapback echo: the ping-pong needs a
  `slapback` mode (one mono tap at 90–120 ms, no feedback).
- *Acoustic guitar*: six strings, `strum` 12–20 ms, the body's air mode
  near 100 Hz and the top near 220 (two resonances, so the model gains a
  second `body2`); the *boom-chick*: the bass note on the beat (a bass
  part on the same instrument), the strum on the off-beat.
- *Pedal steel*: a long-ringing bright string (decay 8 s) with the volume
  pedal (a `swell` attack of 0.2–0.5 s on each note) and continuous pitch:
  a chord in which one note glides a whole step while the others hold
  (per-voice `glide` exists; the grammar writes the moving note as a slide
  '~' in a `chord` part, which needs the chord type to accept per-note
  slides). The bar's vibrato: 4.5 Hz, 20 cents.
- *Fiddle*: bowed, not plucked. A `bowed` kind: the Moog-style mono voice's
  saw as the bow, through the string's body resonances (280 and 450 Hz and
  2.8 kHz for a violin), a slow attack, bow noise, vibrato after a delay,
  double stops (two voices in thirds and fifths). The fiddle shuffle is
  the bowing's long-short-short.
- *Banjo*: the string very short (decay 0.4 s) and bright, the head's
  resonance at 1.5 kHz, little damping. Rolls (forward, backward,
  alternating thumb) are `arp` patterns over chord tones.
- *Upright bass*: the string with `damp` 0.8, decay 0.8 s, body 110 Hz.
- *Mandolin*: two strings per course 6 cents apart (two voices per note),
  short; the chop is a `chord` part on 2 and 4 with `gate` 0.25.
- *Honky-tonk piano*: the FM piano through chorus mode 3 (the detune).
- *Drums*: a `country` kit: a soft acoustic kick (longer, lower, less
  click), a snare with wires, a `brush` hit (a 60–120 ms noise swell,
  band-passed 2–6 kHz: the swish), a `sidestick` (the rim click of the
  ballads), hat, ride, floor tom.

**Rhythm.**

- The *train beat*: snare 16ths with the accents on 2 and 4
  (`x.x.X.x.x.x.X.x.`), the kick on 1 and 3 with a push, the bass in two
  (root on 1, fifth on 3) with a walk into the next chord on bar 4.
- The *shuffle* is a triplet feel: the sequencer swings 16ths, not 8ths,
  so it needs `swing8` (the second 8th of every beat delayed by a third of
  a beat). The *waltz* and 6/8 need `steps` per bar (12 instead of 16);
  the sequencer's `% 16` becomes `% steps`.
- Bluegrass: the mandolin's chop and the banjo's rolls carry the beat; the
  bass walks.

**Harmony.** Major keys. I–IV–V with the dominant as 'dom', the 12-bar for
honky-tonk, I–V–vi–IV for the modern side, ii–V turnarounds, the bVII in
the outlaw side (mixolydian), two-chord verses (I–V). A `PROGS.country`
family. The last chorus a half or whole step up (eurobeat's trick is
country's too).

**Melody.** Major pentatonic (`avoid: [3, 6]` in major) with the blue
third as colour on the tele. There is no singer, so the lead instruments
trade: fiddle on the verse, steel on the chorus, tele on the break. The
hook is 3.4's motif; the ache is the 6th or the 2nd held over the IV
chord, which the realiser can be asked for (`home` on the IV bar).

**Form.** Intro (the hook on the steel or tele over the band), verse,
chorus, verse, chorus, break (the tele's or fiddle's solo over the verse
chords, with the turnaround), chorus, tag (the last line twice). The
ballads take the 32-bar AABA.

**Energy.** The band thins instead of a filter closing: at low energy the
steel and the fiddle drop out and the train beat becomes brushes; at the
top the whole band plays the train beat with the tele's fills.

**What the composer lacks.** `swing8`, `steps` per bar, per-note slides
in chord parts, the walking bass (an approach degree 'p', a half step
below the next root), the ghost pluck, and `PROGS.country`.

**What makes it grab.** The swing and the two-beat bass's drive; the
articulations (the pop, the bend into a note, the steel's swell into the
chorus); a melody that sits on the 3rd and 6th; the turnaround before
every chorus.

### 3.5.2 Classical

**The sound.** An instrumental tradition with no drums, no bass lane and no
drop; harmony, counterpoint and orchestration carry the arc. Three styles
that generate well:

- **Baroque**: a string ensemble with harpsichord continuo over a *ground
  bass* (the passacaglia and chaconne: Pachelbel, Purcell), or the
  ritornello of a fast Vivaldi movement at 100–140 with motoric 8ths.
- **Classical and early romantic chamber music**: a string quartet or a
  piano, periodic phrases of 4 + 4 bars, melody and accompaniment (the
  Alberti bass), cadences, the minuet and trio, the rondo.
- **Minimalist and neo-classical** (Satie, Glass, Richter, Arnalds): piano
  ostinatos, a slow harmonic rhythm, long crescendos. The night-drive
  station.

**Instruments.**

- *Bowed strings*, solo and in sections: the `bowed` kind of 3.5.1 with
  body resonances per size (violin, viola, cello, bass), slow attacks,
  vibrato after a delay, bow noise, and an `ensemble` mode (several voices
  per note, a few cents and a few milliseconds apart: the section sound).
  Articulations per note: legato (no retrigger), détaché, staccato (`gate`
  0.5), tremolo (repeated 16ths), pizzicato (the plucked string), con
  sordino (a darker body).
- *Harpsichord*: the plucked string with the hardest pluck, two 8'
  registers two cents apart (two voices per note), and the jack's click at
  note-off (a tiny noise burst on release).
- *Piano*: two or three strings per note a cent or two apart on the string
  model with a hammer excitation (a pulse plus filtered noise, shorter and
  brighter with velocity), a soundboard (two low-Q resonances near 200 Hz
  and 1.2 kHz), decay falling with pitch (`decayKey`: 8 s at the bottom,
  under a second at the top), the sustain pedal as a long `r`.
- *Woodwinds and horns* for the orchestral side: the flute exists; the
  oboe is a pulse through two formants (1 and 3 kHz), the clarinet a square
  through a dark body, the horn a saw with a slow attack through a
  low-pass (the brass lead is close). *Timpani*: the tom type at 60–100 Hz
  with a long decay, the roll a fill.

**Harmony.** Functional tonality: tonic, subdominant, dominant, tonic.
Cadence formulas (ii–V–I, IV–V–I, the half cadence on V that ends an
antecedent, the deceptive V–vi), the circle of fifths (vi–ii–V–I, the
descending fifths with 7ths), secondary dominants (V/V), the Neapolitan in
minor, modulation to the dominant or the relative major and back, and the
ground bass (a fixed 4- or 8-bar bass the harmony is built on:
Pachelbel's D A B F# G D G A). The composer needs a **cadence-aware
progression generator**: a 4-bar phrase ends open (on V) or closed (on I)
as 3.4's melodies already do, and a sequence is a progression transposed
down a step each bar. Voice leading: `voice()` already moves the least;
the bass becomes an independent voice (the slash chord syntax exists), and
contrary motion between bass and melody is preferred.

**Counterpoint.** A second voice against the melody by first-species
rules: consonances on strong beats, contrary motion preferred, no parallel
fifths or octaves (`counter(melody, chords)`). Imitation: the melody again
a bar later a fifth down (the fugato); over a ground bass a canon falls out
of pattern offsets (Pachelbel's three violins are one line three bars
apart).

**Rhythm.** Meters 4/4, 3/4 (the minuet), 6/8 and 2/4 (`steps` per bar);
no 16th-grid drums; *rubato* (a tempo curve per phrase, ±3 %, slowing into
cadences: a `rubato` field the engine's step clock follows) and fermatas.

**Melody.** The period (antecedent, consequent) is 3.4's open and closed
phrases. Motivic development: the motif transposed (the composer's
`answer`), thinned (`thin`), and new: inverted (the contour negated),
in sequence (a step lower each bar), augmented (the rhythm at half speed).

**Form.** Ternary (minuet, trio, minuet), rondo (A B A C A), theme and
variations (the composer's `variants` plus a change of orchestration per
variation: déjà vu's perfect fit), the passacaglia (continuous variations
over the ground), and sonata-allegro for the ambitious (two themes, a
development, the recapitulation).

**Energy.** Dynamics and orchestration: a solo cello at the bottom, the
tutti at the top. A pursuit is the storm (Vivaldi's Summer, the Fifth):
tremolo strings and timpani; the cruise is the minimalist piano.

**What the composer lacks.** The harmony generator with cadences and
sequences, counterpoint, inversion and sequence of motifs, `steps` per
bar and rubato, the `bowed` kind with `ensemble`, the harpsichord and
piano excitations, `decayKey`. The largest of the three.

**What makes it grab.** A theme one can hum (the period), harmonic rhythm
(tension held on V, released on I), and a climax prepared over eight bars
by voices joining (the "build" of 3.4 with other means).

### 3.5.3 Chicha (built)

**The sound.** Peruvian cumbia of the late sixties onward: Colombian
cumbia's rhythm played on surf rock's instruments, the melodies of the
Andes in them. Two sub-styles, picked per seed:

- **Costeña** (Los Destellos, Los Mirlos, Los Shapis): the electric guitar
  with tremolo carries the melody, a second guitar a third above it in
  the chorus, a combo organ underneath, 96–104 bpm.
- **Amazónica** (Juaneco y su Combo): the lead through a wah pedal rocked
  once a beat, the organ up front, 90–98 bpm.

**Instruments** (`instruments.js`): the plucked string of 3.5.4 as
`surfGuitar` (a clean single coil, the pickup's peak near 3 kHz, light
drive, Fender-style tremolo at 5–6.5 Hz, a quarter-tone slide up into each
note), `wahGuitar` (the same through a wah of two octaves from 380 Hz,
rocked at the beat's rate), `rhythmGuitar` (muted strums, damped in
60 ms), `fingerBass` (a round string with a soft top), and `comboOrgan`
(a Farfisa-like: four drawbars on the FM organ algorithm through the
chorus's vibrato mode). The **latin kit** (`KITS.latin`): congas (open
tone, slap, the low tumba) and bongos on a new `conga` hit (a head tone
with a short pitch drop and a second partial; the slap a bright burst that
chokes it), timbales on the tom hit with metallic noise, the shell's
cáscara and a clave on the rim hit, the campana (cowbell), maracas (the
shaker), and the **güiro** on a new `guiro` hit: band-passed noise rasped
once per ridge, the stroke speeding up over its length and stopping dead,
in a long (170 ms) and a short (40 ms) stroke. No snare and no boom: a
section's `drop` is silent on this kit and only marks the chorus.

**Rhythm.** The güiro's long-short-short on every beat (`guiroL`
`x...x...x...x...`, `guiroS` on the 16ths between); the congas' open tones
on the "and" of 2 and the end of the bar, the slap on 2 and 4, the tumba
on 4; the bongos' martillo in the verse; the cáscara (`x.x.xx.x.x.xx.x.`)
and the bell on the beat in the chorus; the kick lightly on 1 and 3. The
fill into every chorus is the timbalero's *abanico* (`FILLS.abanico`: a
roll on the high drum opening onto the low one and the bell), under which
the congas and güiro keep playing.

**The bass** is the tumbao: the root on beats 1 and 3, the fifth on the
off-beat 8th before the next beat (`r.....f.r.....f.`), the pickup 'n'
into every chord change, and now and then the bordoneo (a repeated root)
or the octave.

**Harmony.** Minor keys; i, iv and the dominant V7 (`chordOn`'s new 'dom'
extension writes the V7 whatever the scale says), VII and VI on the Andean
side; `PROGS.cumbia` for the verse, a tighter i–V7 or i–iv–V7 vamp for the
chorus.

**Melody.** Minor pentatonic (the huayno in it): `realise` gained `avoid`,
the scale degrees a line steps over when it is not on a chord tone
([1, 5] in minor), so the 2nd and 6th appear only as chord tones (the
V7's 7th). The chorus hook (3.4's motif) is stated by the lead guitar in
full at both choruses and *at the same time* by the second guitar a third
up (the `answer`, which is what twin guitars in thirds are), thinned in
the pre-chorus, and alone over the güiro to open the song. The verse has
its own calmer, lower melody; the organ solo its own dense figure.

**Form** (about four minutes at 98 bpm, 120 bars): the hook alone over
güiro and clave (8), verse (16), chorus (16), verse (16), the organ solo
over bass and percussion (16), the pre-chorus with the hook thinned and
the abanico (8), chorus (16), chorus with the organ in unison and the
guitars answering (16), coda (8). Déjà vu plays over the verse's bass and
the rhythm guitar's strums as in every genre.

**Energy.** The shorts of the güiro, the cáscara, the maracas, the bongos
and the bell drop out first, then the second guitar and the strums; what
remains at the bottom is bass, kick, the long güiro stroke, the congas
and the lead.

**Levelling.** Each part soloed over the chorus and matched to 3.4's
rule against the drums (bass under by 3 dB, the lead by 3–4, the organ pad
by 6, the strums by 12). The owner's ear decides the rest, as always.

### 3.5.4 The plucked string

One model behind every guitar, bass, banjo and harpsichord above
(`instruments.js` `StringVoice`, after Karplus and Strong as Jaffe and
Smith extended it): a noise burst one period long (`pick` sets how bright,
and harder notes are brighter) enters a delay loop whose length is the
period, read fractionally for tuning; a one-zero average in the loop damps
the upper partials faster than the lower (`damp`); a loss per period sets
the ring time (`decay`, RT60) and a second loss the muting when the note
ends (`r`); the string is heard through a resonance (`body`, `bodyQ`,
`bodyMix`: the pickup's peak or the top) and a tone control. Per note:
`bend` / `bendT` slide up into the note, `glide` slides from the last when
legato, `strum` spaces a chord's strings. The amp is the instrument's:
`drive` (a soft clip), the wah (`wah` octaves from `wahHz`, rocked at
`wahRate`), the tremolo (`trem`, `tremRate`), and the chorus. Voices are
polyphonic through `Poly` like the Juno's.

What the specs above add to it, when they are built: `swell` (the volume
pedal), `body2` (a second resonance for acoustic bodies), `decayKey`
(decay falling with pitch, for the piano), the hammer and the jack click
as alternative excitations, two or three strings per note (`strings`,
cents apart), and the `bowed` excitation as its own kind sharing the body
chain.

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
    psytrance, drum & bass, UK garage and chicha (3.5.3, with the plucked
    string and the latin kit) from a seed, each with one hook
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
- **The port and the radio (2026-10-08):** `crates/mp_music` is the lab
  ported line for line and checked against `parity/golden/music/` (written
  by `tools/music-lab/test/golden.mjs`: every grammar's tracks and the
  sequencer's events to the bit, the renders to the last bit of an f32);
  it is the music of the radio stations (radio.md 5 and 7), which play it
  as persistent streams on the wall clock through a worklet node in
  `mp_audio`. A station costs 2 to 3 % of a desktop core natively; the
  phones are S1's measurement still to make.

## 6. Questions for the owner

1. **Reference music:** which artists or tracks in house and techno (and
   anything else) are the target sound? A short list sets the grammars
   and the instrument choices better than any description.
2. **Reference engines:** which cars' sounds are the target? A list of
   cars to study (by ear, from recordings) for each engine type.
3. **Recorded sound:** the game is generated-only today. Is that a rule
   to keep, or are recorded samples allowed where they are clearly better
   (for example the owner recording real cars)?
