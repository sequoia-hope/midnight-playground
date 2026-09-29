// game/HUD.js fmtTime: the race clock, lap and results times, and the best
// times on the menu card.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fmtTime } from '../../src/game/HUD.js';

test('fmtTime shows m:ss.hh', () => {
  assert.equal(fmtTime(0), '0:00.00');
  assert.equal(fmtTime(5.2), '0:05.20');
  assert.equal(fmtTime(59.5), '0:59.50');
  assert.equal(fmtTime(60), '1:00.00');
  assert.equal(fmtTime(65.432), '1:05.43');
  assert.equal(fmtTime(185.123), '3:05.12');
  assert.equal(fmtTime(3600), '60:00.00');
});

test('fmtTime shows dashes for no time', () => {
  for (const t of [null, undefined, NaN, Infinity, -Infinity]) assert.equal(fmtTime(t), '--:--.--', String(t));
});

// The last few milliseconds of a minute round up to the next minute, not
// to ":60.00".
test('fmtTime rounds across the minute', () => {
  assert.equal(fmtTime(59.996), '1:00.00');
  assert.equal(fmtTime(119.999), '2:00.00');
  assert.equal(fmtTime(0.004), '0:00.00');
  assert.equal(fmtTime(9.996), '0:10.00');
});
