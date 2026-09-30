// Hot Pursuit's radio voice: every line dispatch can say, on every level,
// has its recordings in audio/radio/ (so none falls back to the burble),
// the clip list agrees with what RADIO says, callsigns stay inside the set
// that was recorded, and RadioVoice picks, fetches and decodes the takes.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { LEVELS } from '../../src/levels/index.js';
import { RADIO, DIRS, TAKES, clipId, placeName, radioClips, levelsRadioClips } from '../../src/game/audio/radioLines.js';
import { RadioVoice } from '../../src/game/audio/RadioVoice.js';
import { CALLSIGNS, callsign } from '../../src/game/Pursuit.js';

const RADIO_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../audio/radio');
const all = levelsRadioClips(LEVELS);
const ids = new Set(all.map((c) => c.id));
const POLICE = LEVELS.filter((l) => l.police); // the levels Hot Pursuit runs on
const zones = POLICE.flatMap((l) => l.zones.map((z) => placeName(z.name)));
const names = [...new Set(POLICE.flatMap((l) => (l.rivals ?? []).map((r) => r.name)))];

// Everything PursuitView can say, with every value filled in.
function everyLine() {
  return [
    ...zones.flatMap((z) => DIRS.map((d) => RADIO.pursuit(d, z))),
    ...zones.flatMap((z) => CALLSIGNS.map((u) => RADIO.intercept(u, z))),
    ...CALLSIGNS.flatMap((u) => [RADIO.joining(u), RADIO.unitDown(u)]),
    ...[2, 3, 4, 5].map(RADIO.heat),
    ...names.map(RADIO.rivalBusted),
    RADIO.spotted(), RADIO.lost(), RADIO.escaped(), RADIO.roadblock(true), RADIO.roadblock(false),
    RADIO.spikes(), RADIO.spiked(), RADIO.busted(), RADIO.wrecked(),
  ];
}

test('names: clip ids are the words, place names are title case', () => {
  assert.equal(clipId('Unit 23, speeder on Nob Hill!'), 'unit-23-speeder-on-nob-hill');
  assert.equal(clipId("Bring in everything we've got."), 'bring-in-everything-we-ve-got');
  assert.equal(placeName('INTERSTATE 9'), 'Interstate 9');
  assert.equal(placeName('OLD MILL VALLEY'), 'Old Mill Valley');
});

test('the clip list covers every part of every line, once, and nothing else', () => {
  const said = new Set(everyLine().flatMap((l) => l.parts.map(clipId)));
  assert.deepEqual([...said].sort(), [...ids].sort());
  assert.equal(ids.size, all.length, 'no clip listed twice');
  for (const l of everyLine()) assert.ok(l.parts.length && l.text, 'a line has text and something to say');
});

test('the intercept call is the callsign, then the message', () => {
  const l = RADIO.intercept(17, 'Nob Hill');
  assert.equal(l.text, 'Unit 17, speeder on Nob Hill, moving to intercept.');
  assert.deepEqual(l.parts, ['Unit 17.', 'Speeder on Nob Hill, moving to intercept.']);
});

test('lines with nothing filled in get extra takes', () => {
  assert.equal(all.find((c) => c.id === clipId(RADIO.spikes().text)).takes, TAKES);
  assert.equal(all.find((c) => c.id === clipId(RADIO.joining(12).text)).takes, 1);
});

test('every callsign a unit can get was recorded', () => {
  for (let i = 0; i < 7; i++) {
    for (const r of [0, 0.5, 0.9999]) assert.ok(CALLSIGNS.includes(callsign(i, () => r)), `unit ${i}`);
  }
  assert.deepEqual([CALLSIGNS[0], CALLSIGNS.at(-1)], [10, 30]);
});

test('a level\'s preload list is a subset of the recordings', () => {
  assert.ok(POLICE.length >= 4 && !POLICE.some((l) => l.id === 'cruise'));
  for (const l of POLICE) {
    const mine = radioClips({ zones: l.zones.map((z) => z.name), units: [11, 16], names: (l.rivals ?? []).map((r) => r.name) });
    for (const c of mine) assert.ok(ids.has(c.id), c.id);
  }
});

test('audio/radio has every take of every clip, and index.json lists them', () => {
  const index = JSON.parse(fs.readFileSync(path.join(RADIO_DIR, 'index.json'), 'utf8'));
  const missing = [];
  for (const c of all) {
    if (index.clips[c.id] !== c.takes) missing.push(`${c.id} (${index.clips[c.id] ?? 0}/${c.takes} in index.json)`);
    for (let t = 1; t <= c.takes; t++) {
      const f = path.join(RADIO_DIR, t > 1 ? `${c.id}.${t}.mp3` : `${c.id}.mp3`);
      if (!fs.existsSync(f) || fs.statSync(f).size < 1000) missing.push(path.basename(f));
    }
  }
  assert.deepEqual(missing, [], 'record them with tools/radio-voice/render.py');
  const extra = fs.readdirSync(RADIO_DIR).filter((f) => f.endsWith('.mp3') && !ids.has(f.replace(/(\.\d)?\.mp3$/, '')));
  assert.deepEqual(extra, [], 'clips no line says any more');
});

// A RadioVoice over a fake fetch and a fake decoder.
function fakeVoice(clips) {
  const got = [];
  const saved = globalThis.fetch;
  globalThis.fetch = async (url) => {
    const name = String(url).split('/').pop();
    got.push(name);
    if (name === 'index.json') return { ok: true, json: async () => ({ voice: 'test', clips }) };
    return { ok: true, arrayBuffer: async () => new TextEncoder().encode(name).buffer };
  };
  const ctx = { decodeAudioData: async (b) => ({ name: new TextDecoder().decode(b), duration: 1 }) };
  return { voice: new RadioVoice(new URL('https://game.test/audio/radio/')), got, ctx, restore: () => { globalThis.fetch = saved; } };
}

test('RadioVoice: a line decodes one take of each part, in order', async () => {
  const f = fakeVoice({ 'unit-17': 1, 'suspect-in-custody': 2 });
  try {
    const bufs = await f.voice.buffers(f.ctx, ['Unit 17.', 'Suspect in custody.'], () => 0.99);
    assert.deepEqual(bufs.map((b) => b.name), ['unit-17.mp3', 'suspect-in-custody.2.mp3']);
    const first = await f.voice.buffers(f.ctx, ['Suspect in custody.'], () => 0);
    assert.deepEqual(first.map((b) => b.name), ['suspect-in-custody.mp3']);
    assert.equal(await f.voice.buffers(f.ctx, ['Unit 17.', 'Never recorded.']), null, 'a missing part: no voice');
  } finally { f.restore(); }
});

test('RadioVoice: prefetch fetches every take once, and a line then needs no new fetches', async () => {
  const f = fakeVoice({ 'spike-strip-deployed': 2, 'unit-12': 1 });
  try {
    await f.voice.prefetch(['spike-strip-deployed', 'unit-12', 'not-recorded']);
    assert.deepEqual(f.got.sort(), ['index.json', 'spike-strip-deployed.2.mp3', 'spike-strip-deployed.mp3', 'unit-12.mp3']);
    const n = f.got.length;
    await f.voice.buffers(f.ctx, ['Spike strip deployed.']);
    await f.voice.buffers(f.ctx, ['Spike strip deployed.']);
    assert.equal(f.got.length, n);
  } finally { f.restore(); }
});

test('RadioVoice: without index.json there is no voice, and nothing throws', async () => {
  const saved = globalThis.fetch;
  globalThis.fetch = async () => ({ ok: false });
  try {
    const v = new RadioVoice(new URL('https://game.test/audio/radio/'));
    assert.equal(await v.buffers({}, ['Suspect in custody.']), null);
    await v.prefetch(['suspect-in-custody']);
  } finally { globalThis.fetch = saved; }
});
