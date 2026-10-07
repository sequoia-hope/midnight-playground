// Music Lab: offline renders and numbers, so a change can be checked without
// ears. B renders the lab's worklet on an OfflineAudioContext; A renders the
// game's Music.js on one (it is built to: pumpUntil drives it faster than
// real time). The analysis is the engine lab's band split plus crest factor
// and stereo width.

import { Music } from '../../src/game/audio/Music.js';
import { analyse as bands } from '../engine-lab/measure.js';

export const WORKLET_URL = new URL('worklet.js', import.meta.url);

// The bar where the song is fullest: the first section playing the most parts.
export function fullBar(T) {
  let best = 0, most = -1, bar = 0;
  for (const s of T.sections) {
    const n = Object.keys(s.p || {}).length + (s.drums ? 1 : 0);
    if (n > most) { most = n; best = bar; }
    bar += s.bars;
  }
  return best;
}

export async function renderB(track, { bar = 0, secs = 12, rate = 48000, kit, mix, seed = 5, solo = null } = {}) {
  const oc = new OfflineAudioContext(2, Math.ceil(secs * rate), rate);
  await oc.audioWorklet.addModule(WORKLET_URL);
  const node = new AudioWorkletNode(oc, 'music-lab', {
    numberOfInputs: 0, outputChannelCount: [2],
    processorOptions: { seed, track, kit, play: bar, mix, solo },
  });
  node.connect(oc.destination);
  return oc.startRendering();
}

export async function renderA(id, { bar = 0, secs = 12, rate = 48000, solo = null } = {}) {
  const oc = new OfflineAudioContext(2, Math.ceil(secs * rate), rate);
  const out = oc.createGain();
  // The game's music bus runs into its master limiter at about this level.
  out.gain.value = 0.7;
  out.connect(oc.destination);
  const m = new Music(oc, out);
  m.build();
  m.solo = solo;
  m.play(id, { bar, fade: false });
  m.start();
  m.pumpUntil(secs);
  return oc.startRendering();
}

export function analyse(buf, skip = 0.5) {
  const b = bands(buf, skip);
  const rate = buf.sampleRate, L = buf.getChannelData(0), R = buf.getChannelData(1);
  const i0 = Math.floor(skip * rate);
  let mid = 0, side = 0;
  for (let i = i0; i < L.length; i++) { const m = (L[i] + R[i]) / 2, s = (L[i] - R[i]) / 2; mid += m * m; side += s * s; }
  const peakDb = 20 * Math.log10(b.peak + 1e-9);
  return { ...b, crestDb: +(peakDb - b.rmsDb).toFixed(1), width: +Math.sqrt(side / (mid + 1e-20)).toFixed(2) };
}

// Every song, A and B, one row each.
export async function measureAll(songs, onRow, { secs = 12 } = {}) {
  const rows = [];
  for (const S of songs) {
    const bar = fullBar(S.track);
    const b = analyse(await renderB(S.track, { bar, secs, kit: S.kit }));
    const a = S.gameId ? analyse(await renderA(S.gameId, { bar, secs })) : null;
    const row = { id: S.track.id, title: S.track.title, bar, a, b };
    rows.push(row);
    onRow?.(row);
  }
  return rows;
}
