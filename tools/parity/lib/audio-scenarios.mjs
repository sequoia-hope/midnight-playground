// The offline renders of the audio reference (SPEC 7.5: each car at a table
// of rpm and throttle, each song's first thirty seconds, each one-shot),
// plus the continuous voices. tools/parity/audio-ref.html renders them in
// Chrome; the Rust port's native backend renders the same table in M5 and
// both go through tools/parity/lib/bands.mjs.
//
// A scenario, as the renderer reads it (48 kHz stereo, control frames of
// 384 samples = 8 ms; frame k is at t = k * 0.008 s):
//   id       name of the render
//   secs     length (rounded to whole frames)
//   car      setCar(car) after init (default 'sports')
//   volume   setVolume(volume) after init (default master 1, sfx 0.85, music 0)
//   env      setEnvironment(env, true) after setCar
//   track    music.play(track) then setMusic(true); music.pumpUntil(t + 1)
//            on frames 0, 64, 128, ...
//   state    update(0.008, state) every frame, after changes and events
//   changes  [[t, fields]]: merged into state on the first frame at or after t
//   events   [[t, method, ...args]]: called on the first frame at or after t
//   rivals, sirens, damage, spiked, mood: setRivalEngines(rivals),
//            setSirens(sirens), setDamage(damage), setSpikedTyres(true,
//            spiked), setPursuitMood(mood) every frame
//   analyse  [from, to] seconds that bands.mjs measures
//   seed     Math.random is mulberry32(seed) from before init (default 1)
// Each frame runs: changes, events, update, then the per-frame setters.

const RPMS = [900, 2000, 3500, 5000, 6500, 7700];
const THROTTLES = [0, 1];
const MOTORS = [0.05, 0.25, 0.5, 0.75, 1];

const engineState = (car, rpm, throttle) => ({
  rpm, rpmMax: 7800, throttle, gear: 3, speed: 0, skid: 0, nitro: false, onGround: true, scrape: 0,
  boost: car === 'rally' ? 0.9 * throttle : 0, slip: 0, offroad: 0, scrapeSide: 0,
});
const stopped = (more = {}) => ({ ...engineState('sports', 0, 0), ...more });

export function renderScenarios() {
  const out = [];
  // Engines: steady state, measured after half a second of settling.
  for (const car of ['sports', 'muscle', 'super', 'rally']) {
    for (const rpm of RPMS) {
      for (const thr of THROTTLES) {
        out.push({ id: `engine-${car}-${rpm}-${thr}`, secs: 1.5, car, state: engineState(car, rpm, thr), analyse: [0.5, 1.5] });
      }
    }
  }
  for (const motor of MOTORS) {
    for (const thr of THROTTLES) {
      out.push({
        id: `engine-electric-${motor}-${thr}`, secs: 1.5, car: 'electric', analyse: [0.5, 1.5],
        state: { ...engineState('electric', 7800 * motor, thr), motor, power: thr ? 450 * motor : -60 * motor, regen: thr ? 0 : 0.5 },
      });
    }
  }
  // Engine transitions: lift-off crackle, a gear change, the turbo's blow-off.
  out.push({ id: 'engine-sports-liftoff', secs: 2, state: engineState('sports', 6500, 1), changes: [[0.6, { throttle: 0 }]], analyse: [0, 2] });
  out.push({ id: 'engine-muscle-liftoff', secs: 2, car: 'muscle', state: engineState('muscle', 6000, 1), changes: [[0.6, { throttle: 0 }]], analyse: [0, 2] });
  out.push({ id: 'engine-sports-upshift', secs: 1.5, state: engineState('sports', 7000, 1), events: [[0.6, 'shift', true]], changes: [[0.6, { rpm: 5200 }]], analyse: [0, 1.5] });
  out.push({ id: 'engine-sports-downshift', secs: 1.5, state: engineState('sports', 3500, 0), events: [[0.6, 'shift', false]], changes: [[0.6, { rpm: 5000 }]], analyse: [0, 1.5] });
  out.push({ id: 'engine-rally-blowoff', secs: 2, car: 'rally', state: engineState('rally', 6000, 1), changes: [[0.6, { throttle: 0, boost: 0.8 }]], analyse: [0, 2] });

  // One-shots, fired at 0.05 s with nothing else running.
  const shots = [
    ['impact-0.3', 2, ['impact', 0.3, 0]], ['impact-0.9', 2.5, ['impact', 0.9, -0.5]],
    ['landing-0.3', 1.5, ['landing', 0.3]], ['landing-0.9', 2, ['landing', 0.9]],
    ['beep', 1, ['beep', false]], ['beep-go', 1.5, ['beep', true]],
    ['whoosh-left', 1.5, ['whoosh', -0.8, 0.5]], ['whoosh-right', 1.5, ['whoosh', 0.8, 1]],
    ['nitro-burst', 1.5, ['nitroBurst']],
    ['ui-click', 0.5, ['uiClick', 'click']], ['ui-start', 0.8, ['uiClick', 'start']],
    ['finish-fanfare', 4, ['finishFanfare']], ['siren-horn', 1.5, ['sirenHorn', 0.3]],
    ['busted', 3, ['busted']], ['escaped', 3.5, ['escaped']],
    ['takedown', 2.5, ['takedown', 0.8, 0.3]], ['spike-pop', 2, ['spikePop', -0.4]],
    ['wrecked', 5, ['wrecked']], ['shift-up', 1, ['shift', true]], ['shift-down', 1, ['shift', false]],
    ['radio-burble', 2.5, ['radio', 1.6, 0]],
  ];
  for (const [id, secs, call] of shots) out.push({ id: `shot-${id}`, secs, events: [[0.05, ...call]], analyse: [0, secs] });
  out.push({ id: 'shot-nitro-burst-electric', secs: 1.5, car: 'electric', events: [[0.05, 'nitroBurst']], analyse: [0, 1.5] });

  // Continuous voices, steady state.
  const voice = (id, more) => out.push({ id: `voice-${id}`, secs: 1.5, analyse: [0.5, 1.5], ...more });
  voice('wind-20', { state: stopped({ speed: 20 }) });
  voice('wind-50', { state: stopped({ speed: 50 }) });
  voice('wind-80', { state: stopped({ speed: 80 }) });
  voice('wind-50-air', { state: stopped({ speed: 50, onGround: false }) });
  voice('squeal', { state: stopped({ speed: 25, skid: 0.8, slip: 0.3 }) });
  voice('gravel', { state: stopped({ speed: 25, offroad: 1 }) });
  voice('scrape', { state: stopped({ speed: 30, scrape: 1, scrapeSide: 1 }) });
  voice('nitro', { state: stopped({ speed: 40, nitro: true }) });
  voice('drive-30', { state: { ...engineState('sports', 4000, 1), speed: 30 } });
  voice('reverse', { state: { ...engineState('sports', 2000, 0.5), gear: -1, speed: 5 } });
  voice('tunnel', { env: 'tunnel', state: engineState('sports', 4000, 1) });
  voice('rivals', {
    state: stopped(),
    rivals: [{ dist: 8, pan: 0.5, rpmNorm: 0.6, electric: false }, { dist: 20, pan: -0.4, rpmNorm: 0.9, electric: false }, { dist: 40, pan: 0, rpmNorm: 0.4, electric: true }],
  });
  for (const mode of ['wail', 'yelp', 'hilo']) voice(`siren-${mode}`, { secs: 4, analyse: [0.5, 4], state: stopped(), sirens: [{ id: 1, dist: 40, pan: 0.3, relSpeed: 10, mode }] });
  voice('sirens-3', {
    secs: 4, analyse: [0.5, 4], state: stopped(),
    sirens: [{ id: 1, dist: 30, pan: -0.5, relSpeed: 15, mode: 'yelp' }, { id: 2, dist: 90, pan: 0.2, relSpeed: -5, mode: 'wail' }, { id: 3, dist: 160, pan: 0.7, relSpeed: 0, mode: 'hilo' }],
  });
  voice('damage-0.7', { secs: 2, analyse: [0.5, 2], state: engineState('sports', 3000, 0.5), damage: 0.7 });
  voice('damage-0.95', { secs: 2, analyse: [0.5, 2], state: engineState('sports', 3000, 0.5), damage: 0.95 });
  voice('spiked', { state: stopped({ speed: 20 }), spiked: 20 });

  // Songs: the first thirty seconds through the music bus, SFX silent.
  for (const track of ['midnight-run', 'seabright', 'neon-rush', 'mirage', 'interstate', 'chrome-heart', 'afterburner']) {
    out.push({ id: `song-${track}`, secs: 30, track, volume: { master: 1, sfx: 0, music: 0.7 }, analyse: [0, 30] });
  }
  return out;
}
