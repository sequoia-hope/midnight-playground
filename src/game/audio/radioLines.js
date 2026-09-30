// Police radio chatter in Hot Pursuit: what dispatch says for each event,
// as the text on the HUD and the recorded clips that speak it.
//
// A line is { text, parts }: parts are what's spoken, one clip each, in
// order (usually just the text; a callsign can be its own clip so it
// doesn't multiply every sentence it starts). A clip is named after its
// words (clipId), so the recordings in audio/radio/ can't drift from the
// text: change a line and its old clip simply stops matching, and the
// radio falls back to the synthesised burble until render.py records it.
//
// Three-free: tools/radio-voice/ lists every line from here to record them.

import { CALLSIGNS } from '../Pursuit.js';

export const DIRS = ['east', 'southeast', 'south', 'southwest', 'west', 'northwest', 'north', 'northeast'];

// A zone's name as dispatch says it: SIERRA PASS → Sierra Pass.
export const placeName = (name) => name.toLowerCase().replace(/\b\w/g, (c) => c.toUpperCase());

export const clipId = (words) => words.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');

const line = (text) => ({ text, parts: [text] });

export const RADIO = {
  pursuit: (heading, zone) => line(`All units, suspect heading ${heading} on ${zone}. Pursuit is on.`),
  spotted: () => line('Visual on the suspect again, closing in.'),
  lost: () => line('Lost visual. All units, search the area.'),
  escaped: () => line('We lost the suspect. All units, resume patrol.'),
  heat: (heat) => line(heat >= 4 ? `Heat level ${heat}. Bring in everything we've got.` : `Heat level ${heat}. Requesting more units.`),
  intercept: (unit, zone) => ({
    text: `Unit ${unit}, speeder on ${zone}, moving to intercept.`,
    parts: [`Unit ${unit}.`, `Speeder on ${zone}, moving to intercept.`],
  }),
  joining: (unit) => line(`Unit ${unit} joining the pursuit.`),
  unitDown: (unit) => line(`Unit ${unit} is down! Unit down!`),
  roadblock: (heavy) => line(heavy ? 'Heavy roadblock in position. Nobody gets through.' : 'Roadblock set up ahead. Suspect is heading right for it.'),
  spikes: () => line('Spike strip deployed.'),
  spiked: () => line('Suspect hit the spikes!'),
  busted: () => line('Suspect in custody.'),
  rivalBusted: (name) => line(`${name} is in custody.`),
  wrecked: () => line('Suspect vehicle is totalled. Tow it back onto the road.'),
};

// Lines with nothing filled in come up in every pursuit, so they get a
// second take to keep them from sounding canned.
export const TAKES = 2;

// Every clip the radio can play, as [{ id, text, takes }], for the given
// zones (display names), callsigns and rival names: all of them for the
// recording tool, one level's worth to preload in a race.
export function radioClips({ zones, units = CALLSIGNS, names = [] }) {
  const places = zones.map(placeName);
  const lines = [
    ...places.flatMap((z) => DIRS.map((d) => RADIO.pursuit(d, z))),
    ...places.flatMap((z) => units.map((u) => RADIO.intercept(u, z))),
    ...units.flatMap((u) => [RADIO.joining(u), RADIO.unitDown(u)]),
    ...[2, 3, 4, 5].map(RADIO.heat),
    ...names.map(RADIO.rivalBusted),
  ];
  const fixed = [RADIO.spotted(), RADIO.lost(), RADIO.escaped(), RADIO.roadblock(true), RADIO.roadblock(false),
    RADIO.spikes(), RADIO.spiked(), RADIO.busted(), RADIO.wrecked()];
  const clips = new Map();
  const add = (words, takes) => {
    const id = clipId(words);
    if (!clips.has(id)) clips.set(id, { id, text: words, takes });
  };
  for (const l of lines) for (const p of l.parts) add(p, 1);
  for (const l of fixed) for (const p of l.parts) add(p, TAKES);
  return [...clips.values()];
}

// Every clip for the levels that have police: all of them, for the
// recording tool.
export function levelsRadioClips(levels) {
  const police = levels.filter((l) => l.police);
  return radioClips({
    zones: police.flatMap((l) => l.zones.map((z) => z.name)),
    names: [...new Set(police.flatMap((l) => (l.rivals ?? []).map((r) => r.name)))],
  });
}
