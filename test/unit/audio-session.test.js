// game/Audio askForPlayback: on an iPhone (navigator.audioSession, Safari
// 17+) the game asks to play like a media app, so Silent mode doesn't mute
// it; anywhere without the API, or where it refuses, nothing happens.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { askForPlayback } from '../../src/game/Audio.js';

test('asks an audio session for playback', () => {
  const nav = { audioSession: { type: 'auto' } };
  askForPlayback(nav);
  assert.equal(nav.audioSession.type, 'playback');
});

test('leaves a session already playing back alone', () => {
  let sets = 0, type = 'playback';
  const nav = { audioSession: { get type() { return type; }, set type(v) { sets++; type = v; } } };
  askForPlayback(nav);
  assert.equal(sets, 0);
});

test('no audio session API, or one that refuses: no error', () => {
  askForPlayback({});
  askForPlayback(undefined);
  askForPlayback({ audioSession: { get type() { return 'auto'; }, set type(v) { throw new TypeError('read-only'); } } });
});
