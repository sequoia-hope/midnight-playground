// Music Lab: the AudioWorklet around the engine (engine.js). Messages from
// the page:
//   { type: 'track', track, kit }      load a song (lab patches on every part)
//   { type: 'play', bar } / 'stop'     transport
//   { type: 'patch', part, lab }       live patch edits
//   { type: 'kit', name, tweak }       drum machine and per-lane tweaks
//   { type: 'audition', lab }          the keyboard's instrument
//   { type: 'on', midi, vel } / 'off'  keyboard notes
//   { type: 'hit', lane, vel }         drum pads
//   { type: 'energy', value }          0..1, the game's intensity
//   { type: 'mix', ...fields }         glue, tape, reverb, revSize, revTone, master
//   { type: 'mute', names } / 'solo'   per part or drum lane ('drums' = all)
// and it posts { type: 'pos', ... } about 15 times a second, with the share
// of the audio thread's time budget it used (load).

import { Engine } from './engine.js';

// performance.now is missing in some worklet scopes; Date.now is coarse but
// unbiased once averaged over many blocks.
const now = globalThis.performance?.now ? () => globalThis.performance.now() : () => Date.now();

class MusicLab extends AudioWorkletProcessor {
  constructor(opts) {
    super();
    const o = opts?.processorOptions || {};
    this.e = new Engine({ seed: o.seed ?? 1, rate: sampleRate });
    this.e.onSection = (sec) => this.port.postMessage({ type: 'section', sec });
    this.e.onEnd = () => this.port.postMessage({ type: 'end' });
    this.n = 0; this.busy = 0; this.blocks = 0;
    this.port.onmessage = (m) => this.msg(m.data);
    if (o.track) this.e.setTrack(o.track, { kit: o.kit });
    if (o.play != null) this.e.play(o.play);
    if (o.mix) this.e.setMix(o.mix);
    if (o.solo && this.e.seq) this.e.seq.solo = o.solo;
  }
  msg(d) {
    const e = this.e;
    switch (d.type) {
      case 'track': e.setTrack(d.track, { kit: d.kit }); break;
      case 'play': e.play(d.bar ?? 0, d.delay); break;
      case 'stop': e.stop(); break;
      case 'patch': e.setPatch(d.part, d.lab); break;
      case 'kit': e.setKit(d.name, d.tweak ?? e.kitTweak); break;
      case 'audition': e.setAudition(d.lab); break;
      case 'on': e.noteOn(d.midi, d.vel ?? 0.9, d.dur ? Math.round(d.dur * sampleRate) : undefined, { accent: d.accent, glideFrom: d.glideFrom }); break;
      case 'off': e.noteOff(d.midi); break;
      case 'hit': e.hit(d.lane, d.vel ?? 1); break;
      case 'energy': e.setEnergy(d.value); break;
      case 'mix': e.setMix(d); break;
      case 'mute': if (e.seq) e.seq.mute = new Set(d.names); break;
      case 'solo': if (e.seq) e.seq.solo = d.name || null; break;
      default: break;
    }
  }
  process(_in, outputs) {
    const out = outputs[0];
    const L = out[0], R = out[1] || out[0];
    const t0 = now();
    this.e.process(L, R, L.length);
    this.busy += now() - t0; this.blocks++;
    if (++this.n >= 25) {
      this.n = 0;
      const budget = (this.blocks * L.length * 1000) / sampleRate;
      this.port.postMessage({ type: 'pos', pos: this.e.position, gr: this.e.takeGr(), load: budget ? this.busy / budget : null });
      this.busy = 0; this.blocks = 0;
    }
    return true;
  }
}

registerProcessor('music-lab', MusicLab);
