// The recorded police radio voice (audio/radio/, made by tools/radio-voice/):
// which clips exist, their compressed bytes fetched ahead of need, and
// AudioBuffers decoded when a line is said. Decoded clips aren't kept: a
// level's worth would be tens of MB on a phone, and a clip of a few seconds
// decodes in milliseconds.

import { clipId } from './radioLines.js';

const BASE = new URL('../../../audio/radio/', import.meta.url);

export class RadioVoice {
  constructor(base = BASE) {
    this.base = base;
    this.index = null; // clip id → takes, once index.json is in ({} without it)
    this._index = null;
    this._bytes = new Map(); // file → Promise<ArrayBuffer | null>
  }

  load() {
    this._index ??= fetch(new URL('index.json', this.base))
      .then((r) => (r.ok ? r.json() : null))
      .catch(() => null)
      .then((j) => (this.index = j?.clips ?? {}));
    return this._index;
  }

  _file(id, take) { return take > 1 ? `${id}.${take}.mp3` : `${id}.mp3`; }

  _fetch(file) {
    let p = this._bytes.get(file);
    if (!p) {
      p = fetch(new URL(file, this.base)).then((r) => (r.ok ? r.arrayBuffer() : null)).catch(() => null);
      this._bytes.set(file, p);
    }
    return p;
  }

  // Fetch every take of these clips, a few at a time, so a line is ready
  // the moment it's said.
  async prefetch(ids) {
    const index = await this.load();
    const files = ids.flatMap((id) => Array.from({ length: index[id] ?? 0 }, (_, i) => this._file(id, i + 1)));
    let next = 0;
    const worker = async () => { while (next < files.length) await this._fetch(files[next++]); };
    await Promise.all([worker(), worker(), worker()]);
  }

  // A line's parts as AudioBuffers (a random take of each), or null if any
  // part has no recording.
  async buffers(ctx, parts, rng = Math.random) {
    const index = await this.load();
    const files = [];
    for (const p of parts) {
      const id = clipId(p), n = index[id];
      if (!n) return null;
      files.push(this._file(id, 1 + Math.floor(rng() * n)));
    }
    const bytes = await Promise.all(files.map((f) => this._fetch(f)));
    if (bytes.some((b) => !b)) return null;
    // decodeAudioData takes its ArrayBuffer over (detaches it): hand it a copy.
    try { return await Promise.all(bytes.map((b) => ctx.decodeAudioData(b.slice(0)))); } catch { return null; }
  }
}
