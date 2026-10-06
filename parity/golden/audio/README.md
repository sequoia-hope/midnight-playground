# Audio reference

What the Rust port's audio (`mp_audio`, roadmap M5) is compared against
(SPEC 7.5). Produced from the JS game by `tools/parity/audio-ref.mjs`
(roadmap WP 0.7):

```
node tools/parity/audio-ref.mjs            # regenerate everything (about 10 min)
node tools/parity/audio-ref.mjs --check    # regenerate and compare with this directory
node tools/parity/audio-ref.mjs arrays calllog --check   # only some stages
```

Stages: `arrays` and `calllog` run in Node; `radio`, `drive` and `renders`
in headless Chrome through the e2e harness (request interception, no
server). Everything runs with the parity kernel: in Node it is installed
before the game's modules load, in Chrome by `?kernel=1` / `?parity=1`.

## Common rules

- **Sample rate** 48000 Hz everywhere (what desktops and iPhones run).
- **Math.random** in the audio code is replaced by `mulberry32(1)` (the
  algorithm of `src/util/math.js`) from before `GameAudio` is constructed,
  for the arrays, the call logs and every render. The audio code is the
  only thing drawing from it there, so the Rust port must draw from one
  such stream, in the same order, wherever the JS calls `Math.random`
  (noise beds, the tunnel's impulse response, loop start offsets, pops,
  misfires, one-shot variations, radio takes and burble).
- **Arrays** are float32. A hash is `#` plus the first 16 hex digits of the
  SHA-256 of the array's little-endian float32 bytes.
- **Numbers** in the JSON-lines files are JavaScript's shortest round-trip
  decimal, with `-0` kept and `"NaN"`, `"Infinity"`, `"-Infinity"` as
  strings; `"$undefined"` is an explicit `undefined` argument.
- `parity/cache/<js-tree-key>/audio/` (not in git) holds what is too big to
  commit; every stage writes it again from the same inputs.

## Files

| File | Stage | What |
|---|---|---|
| `arrays.json`, `arrays-small.bin` | arrays | Every array the audio builds (index, hashes, stats; small ones whole) |
| `radio-clips.json` | radio | Each radio MP3's SHA-256 and the length Chrome decodes it to at 48 kHz |
| `drives.json`, `drive-<name>.jsonl.gz` | drive | The game's calls on its audio facade during scripted drives |
| `calllog.json` | calllog | Digest of the Web Audio call log each drive produces |
| `renders.json` | renders | Offline renders: scenario, third-octave band levels, Chrome's run-to-run variation |
| `rust-deviated.json` | (not from the JS) | The Rust renders' own expected bands for the scenarios the port changes on purpose (`audio-bands.mjs` `DEVIATED`, DEVIATIONS.md; written by `audio-bands.mjs --update-deviated`) |

### `arrays.json`, `arrays-small.bin`

A `GameAudio` is built on the recording fake (below) and `setCar` is called
for `sports`, `muscle`, `super`, `rally`, `electric` in turn, so every
car's engine waves exist. `arrays` lists, in creation order:

- `kind: "buffer"`: every AudioBuffer: the noise beds (`audio.noise.*`),
  `samples.js`'s SFX (`audio.sfxBuf.*`) and drum kit (`audio.music.kit.*`),
  the tunnel and hall impulse responses (`audio.tunnel.buffer`,
  `audio.music.reverb.buffer`). `channels`, `length`, `sampleRate`.
- `kind: "wave"`: every `createPeriodicWave(real, imag, options)`: the
  engine cycles per car (`audio._waves.<car>.{onL,offL,onR,offR}`: the
  Fourier series of `engineCycle`; `.rumble`: `rumbleCycle`), the rival
  wave, the music's pulse wave, `pulseWave()` and `softSquareWave()`.
  `parts` is `["real", "imag"]`; `options` as passed.
- `kind: "curve"`: every WaveShaper curve set during the build.

`name` is the shortest property path from the `GameAudio` to the object,
or, for one only a local variable holds, the function that made it
(`pulseWave()`, `_buildEngine(): n59.curve`, with the fake's node id). A
wave or curve with the same contents under several names is listed once,
with the other names in `aliases` (the rival wave is the sports car's
`onR`). Per channel or part: `hashes`, `stats` (`rms`, `peak`), `cache`
(offset in floats into the cache's `arrays.bin`, which holds every array
whole) and, for arrays of at most 16384 floats, `small` (offset in floats
into `arrays-small.bin`). The noise beds, impulse responses and long SFX
are not committed whole (12 MB at 48 kHz, and noise does not compress):
the M5 test reads the cache, which this stage regenerates in a second.

### `radio-clips.json`

`{ sampleRate, clips: [{ file, sha256, channels, length }] }`: every file in
`audio/radio/`, decoded by Chrome's `decodeAudioData` on a 48 kHz context.
The call log's playback uses it to stand in for decoding.

### `drives.json`, `drive-<name>.jsonl.gz`

The real game in headless Chrome with `?parity=1` (kernel, fixed 1/120 s
ticks, two per frame, quantised input, seeded streams) and the autopilot:

| Name | Query | Ticks |
|---|---|---|
| `race` | `parity=1&level=sierra&autostart=sports&autodrive=1` | 5400 (45 s) |
| `pursuit` | `parity=1&level=sierra&autostart=sports&autodrive=1&pursuit=1&heat=2` | 7200 (60 s) |

A script installed before the page's own (`facadeRecorder` in
`tools/parity/lib/audio-facade.mjs`) wraps every public method of the
`GameAudio` the game publishes as `window.__audio`, and
`radioVoice.prefetch`, and logs each call the game makes on them from
outside the audio modules. One line per call:

```
[tick, "method", ...args]
```

`tick` is the race tick the call happens in (the number of
`parity.onTick` calls before it; calls before the first tick, from
`startRace`, have tick 0). The log ends before tick `ticks`. `drives.json`
has each drive's query, ticks, SHA-256 of the uncompressed log and the
number of calls per method.

The audio's own calls on itself (`init` calling `setCar`, `radioLine`
calling `radio`) are not in it: they are what playing the log back
produces. Nothing the audio does feeds back into the race, so the log is
the same whatever the audio does with it.

### `calllog.json` (and the logs in the cache)

Each drive log is played back in Node into a `GameAudio` whose
`window.AudioContext` is the recording fake
(`tools/parity/lib/webaudio-fake.mjs`), on a virtual clock:

- A call in tick `k` is made at `currentTime = k / 120`. Before it, every
  `setTimeout` callback due at or before that time runs, in order of due
  time (ties in the order they were set), with `currentTime` set to its due
  time (`now + ms / 1000` when it was set). So the music's 25 ms scheduler,
  the gate tails, the retire timers and the radio's 700 ms wait all run on
  game time. After each call and each callback every pending promise
  settles.
- The context is a live one (`_realtime` is true): `init()` creates it,
  `unlock()` resumes it; `resume()` and `suspend()` change `state` one
  microtask later.
- `fetch` reads `audio/radio/` from disk; `decodeAudioData` resolves at
  once with a buffer of the length in `radio-clips.json` (the contents are
  not modelled), so a radio line's clips always beat its 700 ms wait.

The cache gets `calllog-<name>.jsonl` (one line per Web Audio call, below),
`calllog-<name>.arrays.json` (`[{ hash, offset, length }]`) and
`calllog-<name>.arrays.bin` (every array the log names by hash, float32).
`calllog.json` has, per drive: `lines`, `bytes`, `sha256` of the whole log,
`ops` (count per operation), `seconds` (`[{ second, lines, sha256 }]`, the
first 16 hex digits of the SHA-256 of the lines whose time is in that whole
second, so a mismatch can be found without the cache), `arrays`, and
`throws` (exceptions a browser would have thrown, which the game caught).

#### The Web Audio call log

One JSON array per line: `[currentTime, op, ...]`. Nodes are `n<k>`
(`n0` is the destination), buffers `b<k>`, periodic waves `w<k>`, numbered
in creation order; a param is `<node>.<name>`.

| op | Fields | Call |
|---|---|---|
| `api` | method, ...args | A facade call from the drive log (a marker; the calls below are what it did) |
| `context` | `{ sampleRate, latencyHint }` | `new AudioContext(...)` |
| `new` | node, type, ...args | `create<Type>(...args)`: `Gain`, `BiquadFilter`, `Oscillator`, `BufferSource`, `WaveShaper`, `DynamicsCompressor`, `Convolver`, `Delay` (max delay), `ChannelMerger` (inputs), `StereoPanner`; `Destination` once |
| `set` | node, attribute, value | `type`, `loop`, `loopStart`, `loopEnd`, `oversample`, `normalize`; `buffer` (a buffer id or null); `curve` (a hash) |
| `value` | param, v | `param.value = v` |
| `get` | param, v | `param.value` read: the automation timeline evaluated at `currentTime` (Web Audio 1.0: a ramp runs from the previous event, a target curve from the value at its start), rounded to float32 as Chrome stores it |
| `setValue` | param, v, t | `setValueAtTime(v, t)` |
| `linRamp` / `expRamp` | param, v, t | `linearRampToValueAtTime` / `exponentialRampToValueAtTime` |
| `setTarget` | param, v, t, tc | `setTargetAtTime(v, t, tc)` |
| `cancel` | param, t | `cancelScheduledValues(t)` |
| `connect` | node, destination[, output, input] | `connect(...)`; the destination is a node or a param |
| `disconnect` | node[, destination] | `disconnect(...)` |
| `start` / `stop` | node, ...args | as called (arguments left out are not written) |
| `setPeriodicWave` | node, wave | |
| `wave` | wave, real hash, imag hash, options | `createPeriodicWave` |
| `buffer` | buffer, channels, length, sampleRate | `createBuffer` |
| `data` | buffer, [hash per channel] | the buffer's contents, logged when it is first given to a node (the fake checks that no buffer changes afterwards) |
| `decode` | buffer, file, channels, length, sampleRate | `decodeAudioData` of a radio clip |
| `resume` / `suspend` / `close` | | context state calls |
| `throw` | error name, message | the call just above threw, as Chrome would have |

The fake is strict where Chrome is: a second `start()`, `stop()` before
`start()`, disconnecting a link that does not exist, an exponential ramp to
zero, negative or non-finite times and values throw; an invalid enum value
(an oscillator type `'saw'`) is ignored and fails the capture.

### `renders.json` (WAVs in the cache)

Every scenario of `tools/parity/lib/audio-scenarios.mjs`, rendered by
`tools/parity/audio-ref.html` on a 48 kHz stereo OfflineAudioContext with a
whole `GameAudio` on it (`init({ context })`), controlled every 384 samples
(8 ms) through `suspend()`; that file documents the scenario fields. They
are: each combustion car at rpm 900, 2000, 3500, 5000, 6500, 7700 and
throttle 0 and 1, the electric motor at five speeds, engine transitions
(lift-off, shifts, blow-off), every one-shot, the continuous voices, and the
first thirty seconds of each song (SFX at 0, music 0.7).

Per render: `id`, the `scenario` as run, `frames`, `sha256` of the float32
samples (channel 0 then 1), `jitter`, and per channel `rms` (dB), `peak` and
`bands`: 31 third-octave levels in dB, 20 Hz to 20 kHz, over the
scenario's `analyse` window, rounded to 0.01 dB. `tools/parity/lib/bands.mjs`
is the analyser (`analyser` in the file records its settings); the Rust
port's native renders go through the same file
(`node tools/parity/lib/bands.mjs render.wav [from to]`). The cache holds
each render as a 32-bit float WAV.

**Chrome's offline renders are not bit-reproducible.** Rendering the same
scenario twice, even in one page, gives samples that differ: by about 1e-7
where several connections sum into one input (Chrome mixes them in an order
that changes between runs), growing to about 1e-3 where that difference
reaches an oscillator's frequency and accumulates in its phase (the
engine's noise-modulated detune). Each render is therefore made twice when
the reference is written; `jitter` is the largest difference between the
two in any band louder than -90 dB. The `sha256` is that of the first
render and is expected to differ on every run. Measured when this
reference was taken: `jitter` at most 0.21 dB (`engine-rally-6500-1`), and
a later run against the golden at most 0.38 dB, all of it on the engines;
renders with no audio-rate modulation of an oscillator (most one-shots)
come out identical or within 1e-7. `--check` compares band levels with the
golden instead, within 1.0 dB, ignoring bands quieter than -90 dB in both.
The comparison with the Rust port (SPEC 7.5) allows 1.5 dB, above this
noise floor.
