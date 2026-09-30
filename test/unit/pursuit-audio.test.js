// game/Audio: the Hot Pursuit sounds' pure parts. The siren patterns span
// the right pitches, doppler raises the pitch of a closing unit and lowers a
// receding one, the siren level falls with distance to silence at 350 m, and
// every pursuit method is a no-op before the audio has started. (The sound
// itself is measured in the browser: tools/audio-test.html renders it.)

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { GameAudio, SIREN_PATTERNS, sirenDoppler, sirenLevel } from '../../src/game/Audio.js';

test('siren patterns: wail and yelp sweep 650-1450 Hz, hi-lo alternates ~960/770 Hz', () => {
  const { wail, yelp, hilo } = SIREN_PATTERNS;
  for (const p of [wail, yelp]) assert.deepEqual([p.lo, p.hi, p.shape], [650, 1450, 'tri']);
  assert.equal(wail.period / 2, 1.7, 'wail takes 1.7 s each way');
  assert.ok(yelp.period < wail.period / 4, 'yelp is much faster than wail');
  assert.deepEqual([hilo.lo, hilo.hi, hilo.shape, hilo.period / 2], [770, 960, 'square', 0.5]);
});

test('doppler: closing raises the pitch, receding lowers it, standing still leaves it', () => {
  assert.equal(sirenDoppler(0), 1);
  assert.ok(Math.abs(sirenDoppler(30) - 343 / 313) < 1e-9);
  assert.ok(Math.abs(sirenDoppler(-30) - 343 / 373) < 1e-9);
  assert.ok(sirenDoppler(1e6) < 2 && sirenDoppler(-1e6) > 0.5, 'clamped for silly speeds');
  assert.equal(sirenDoppler(undefined), 1);
});

test('siren level falls with distance and is silent from 350 m', () => {
  const d = [0, 5, 20, 50, 100, 200, 300, 349];
  for (let i = 1; i < d.length; i++) assert.ok(sirenLevel(d[i]) < sirenLevel(d[i - 1]), `${d[i]} m quieter than ${d[i - 1]} m`);
  assert.ok(sirenLevel(0) > 0.2 && sirenLevel(0) < 0.5, 'loud right behind, not painful');
  assert.ok(sirenLevel(100) > 0.02, 'still clearly audible at 100 m');
  assert.equal(sirenLevel(350), 0);
  assert.equal(sirenLevel(1000), 0);
});

test('pursuit methods are no-ops before init', () => {
  const a = new GameAudio();
  assert.equal(a.ready, false);
  a.setSirens([{ dist: 5, pan: 0, relSpeed: 10, mode: 'wail' }]);
  a.setSirens([]);
  a.sirenHorn(0.5); a.radio(2, -0.5); a.radioLine(['Suspect in custody.']); a.busted(); a.escaped(); a.takedown(1); a.spikePop(); a.wrecked();
  a.setSpikedTyres(true, 30); a.setPursuitMood('cooldown'); a.setDamage(0.9);
  assert.equal(a.ctx, null);
});
