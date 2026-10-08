// Music Lab: the engine. Plays a song (the game's format, with every part
// carrying a `lab` patch: see instruments.js voiceTrack, and gen.js) on the
// lab's instruments, sample-accurately, and mixes it the way sound.md 3.3
// asks: per-part channels, sends, sidechain pump, a song filter, then a bus
// with glue compression and tape. Also plays single notes and drum hits for
// the page's keyboard and pads. No Web Audio here: worklet.js wraps it, and
// the tests run it directly.
//
//   instruments ─ channel (drive, hp, pan, level) ─┬─ [pump] ─┐
//   drum machine (per-lane level, pan) ────────────┤          ├─ song filter ─┐
//          └ reverb and delay sends ─ ping-pong ───┘──────────┘               │
//   risers, downlifters ──────────────────────────────────────────────────────┤
//   reverb return ────────────────────────────────────────────────────────────┤
//                          energy filter ─ glue ─ tape ─ master ─ limiter ─ out

import { SR, setRate, clamp, rng, OnePole, SVF, PingPong, Reverb, Compressor, Tape, Limiter, tanh } from './dsp.js';
import { Drums, kitVoice, makeInstrument } from './instruments.js';
import { Seq } from './seq.js';

// structuredClone is not in the AudioWorklet scope everywhere.
const clone = (x) => JSON.parse(JSON.stringify(x));

// Reverb sends of the game's default kit (Music.js KIT), per lane.
const LANE_REV = { kick: 0, snare: 0.25, clap: 0.3, hat: 0, ohat: 0.05, ride: 0.05, crash: 0.2, revCrash: 0.2, shaker: 0.05, rim: 0.2, snap: 0.35, tomL: 0.25, tomM: 0.25, tomH: 0.25, boom: 0.1, cowbell: 0.15, congaO: 0.2, congaS: 0.2, tumba: 0.15, bongoH: 0.2, bongoL: 0.2, timbaleH: 0.25, timbaleL: 0.25, cascara: 0.15, guiroL: 0.1, guiroS: 0.1, clave: 0.25 };
// Lab mix: drums against the synths. This, the patch gains and the master
// level were set by soloing every part in A and B and matching their RMS
// (measure.js); B then plays at the loudness of A, so A/B is fair.
const DRUM_LEVEL = 0.263;

class Channel {
  constructor(o) {
    this.o = o; this.buf = [0, 0];
    this.drive = o.drive ?? 0;
    this.dlp = [new OnePole(o.driveLp ?? 5000), new OnePole(o.driveLp ?? 5000)];
    this.hp = o.hp ? [new SVF().set(o.hp, 0.707), new SVF().set(o.hp, 0.707)] : null;
    const pan = o.pan ?? 0;
    this.gl = Math.min(1, 1 - pan); this.gr = Math.min(1, 1 + pan);
    this.level = o.level ?? 1; this.rev = o.rev ?? 0; this.dly = o.dly ?? 0; this.pump = !!o.pump;
    this.dn = this.drive ? Math.tanh(this.drive) : 1;
  }
  // Processes buf in place and clears it for the next sample; returns [l, r].
  run(out) {
    let l = this.buf[0], r = this.buf[1];
    this.buf[0] = 0; this.buf[1] = 0;
    if (this.drive) { l = this.dlp[0].lp(tanh(this.drive * l) / this.dn); r = this.dlp[1].lp(tanh(this.drive * r) / this.dn); }
    if (this.hp) { this.hp[0].run(l); this.hp[1].run(r); l = this.hp[0].hp; r = this.hp[1].hp; }
    out[0] = l * this.gl * this.level; out[1] = r * this.gr * this.level;
  }
}

// A noise sweep for risers and downlifters.
class Sweep {
  constructor(r, f0, f1, g0, g1, len, q) { this.r = r; this.f = new SVF(); this.f0 = f0; this.f1 = f1; this.g0 = g0; this.g1 = g1; this.len = len; this.q = q; this.t = 0; }
  get done() { return this.t >= this.len; }
  run() {
    const u = this.t++ / this.len;
    if ((this.t & 31) === 1) this.f.set(this.f0 * Math.pow(this.f1 / this.f0, u), this.q);
    this.f.run(this.r() * 2 - 1);
    const g = this.g0 * Math.pow(this.g1 / this.g0, u) * (u > 0.98 ? (1 - u) / 0.02 : 1);
    return this.f.bp * g;
  }
}

export class Engine {
  constructor({ seed = 1, rate = 48000 } = {}) {
    setRate(rate);
    this.seed = seed;
    this.r = rng(seed);
    this.drums = new Drums(seed + 3);
    this.kit = 'tr909';
    this.kitTweak = {};
    this.parts = {};
    this.seq = null; this.T = null;
    this.playing = false;
    this.nextStep = 0; // samples until the next 16th
    this.pending = []; // [samplesLeft, event]
    this.autos = [];
    this.fx = [];
    this.audition = null; this.auditionCh = new Channel({ rev: 0.2 });
    this.pumpG = 1; this.pumpT = 0; this.pumpOn = false;
    this.songLp = [new SVF(), new SVF()]; this.lpFrom = 20000; this.lpTo = 20000; this.lpLen = 0; this.lpT = 0;
    this.energy = 1; this.enLp = [new SVF(), new SVF()]; this.enHz = 20000;
    this.delay = new PingPong();
    this.reverb = new Reverb(seed + 11);
    this.glue = new Compressor(); this.tape = [new Tape(), new Tape()]; this.lim = new Limiter(0.95);
    this.mix = { glue: 0.5, tape: 0.35, reverb: 1, revSize: 2.4, revTone: 0.5, master: 1.07 };
    this.setMix({});
    this.onSection = null; this.onEnd = null;
    this.t = 0; this.grSum = 0; this.grN = 0;
    this._s = [0, 0]; this._o = [0, 0]; this._rv = [0, 0]; this._dl = [0, 0];
  }

  setMix(m) {
    Object.assign(this.mix, m);
    const x = this.mix;
    this.glue.set(-10 - x.glue * 10, 1 + x.glue * 2.5, 0.012, 0.18);
    for (const t of this.tape) t.set(x.tape);
    this.reverb.set(x.revSize, x.revTone);
  }

  setKit(name, tweak = this.kitTweak) {
    this.kit = name; this.kitTweak = tweak;
    this.laneK = {};
  }
  lane(name) {
    if (this.laneK?.[name]) return this.laneK[name];
    const ko = this.T?.kit?.[name] || {};
    const base = kitVoice(this.kit, name, ko.s);
    if (!base) return null;
    const k = { ...base, ...(this.kitTweak[name] || {}) };
    k.lvl = (k.lvl ?? 1) * (ko.g ?? 1);
    k.rev = ko.rev ?? LANE_REV[name] ?? 0;
    (this.laneK ||= {})[name] = k;
    return k;
  }

  // A song: the game's format with `lab` patches on every part.
  setTrack(T, { kit } = {}) {
    this.T = T;
    if (kit) this.kit = kit; else if (T.kitName) this.kit = T.kitName;
    this.laneK = {};
    this.seq = new Seq(T);
    this.parts = {};
    let i = 0;
    for (const [name, p] of Object.entries(T.parts)) {
      const lab = clone(p.lab);
      this.parts[name] = { lab, inst: makeInstrument(lab, this.seed * 31 + i++), ch: new Channel(p.ch || {}) };
    }
    this.delay.time = (T.delay ?? 0.75) * (60 / T.bpm);
    this.delay.fb = T.delayFb ?? 0.38;
    this.pending = []; this.autos = []; this.fx = [];
    this.lpFrom = this.lpTo = 20000; this.lpLen = 0;
    this.nextStep = 0;
  }
  setPatch(part, lab) {
    const P = this.parts[part];
    if (!P) return;
    Object.assign(P.lab, lab);
    if (P.inst.setPatch) P.inst.setPatch(P.lab);
  }

  play(bar = 0, delay = 0.02) { if (!this.seq) return; this.seq.seekBar(bar); this.playing = true; this.nextStep = Math.max(1, Math.round(delay * SR)); this.pending = []; }
  stop() { this.playing = false; this.pending = []; }
  setEnergy(e) { this.energy = clamp(e, 0, 1); if (this.seq) this.seq.energy = this.energy; }

  // The keyboard: one instrument outside the song.
  setAudition(lab) { this.audition = makeInstrument(clone(lab), 999); this.auditionLab = lab; }
  noteOn(midi, vel = 0.9, durS = SR * 30, opt = {}) { this.audition?.trigger([midi], vel, durS, opt); }
  noteOff(midi) {
    const a = this.audition;
    if (!a) return;
    if (a.voices) for (const v of a.voices) { if (v.midi === midi && v.left > 1) v.left = 1; }
    else if (a.left > 1) a.left = 1;
  }
  hit(lane, vel = 1) { this._drum(lane, vel, 0); }

  _drum(lane, vel, len) {
    const k = this.lane(lane);
    if (!k) return;
    this.drums.hit(lane, k, vel, 1, len ? Math.round(len * SR) : 0);
    if (lane === 'kick' && this.T?.pump && this.playing) { this.pumpT = 0; this.pumpOn = true; }
  }

  _fire(e) {
    switch (e.k) {
      case 'drum': this._drum(e.lane, e.vel, e.len); break;
      case 'note': {
        const P = this.parts[e.part];
        if (P) P.inst.trigger(e.midis, e.vel, Math.round(e.dur * SR), { glideFrom: e.glideFrom, accent: e.accent });
        break;
      }
      case 'lp': this.lpFrom = e.from; this.lpTo = e.to; this.lpLen = Math.round(e.dur * SR); this.lpT = 0; break;
      case 'auto': {
        const [part, param] = e.target.split('.');
        if (this.parts[part]) this.autos.push({ lab: this.parts[part].lab, param, from: e.from, to: e.to, len: Math.round(e.dur * SR), t: 0 });
        break;
      }
      case 'riser': this.fx.push(new Sweep(this.r, 350, 7500, 0.002, 0.16, Math.round(e.dur * SR), 1.4)); break;
      case 'down': this.fx.push(new Sweep(this.r, 6000, 250, 0.12, 0.0005, Math.round(e.dur * SR), 1.2)); break;
      case 'section': if (this.onSection) this.onSection(e.sec); break;
      case 'end': if (this.onEnd) this.onEnd(); break;
      default: break;
    }
  }

  _clock() {
    if (!this.playing) return;
    if (--this.nextStep > 0) {
      for (let i = this.pending.length - 1; i >= 0; i--) if (--this.pending[i][0] <= 0) { this._fire(this.pending[i][1]); this.pending.splice(i, 1); }
      return;
    }
    this.nextStep += Math.round(this.seq.stepDur * SR);
    const evs = this.seq.step();
    for (const e of evs) {
      const d = Math.round((e.dt || 0) * SR);
      if (d > 0) this.pending.push([d, e]); else this._fire(e);
    }
  }

  // Renders n stereo samples into L and R (they are overwritten).
  process(L, R, n = L.length) {
    const s = this._s, o = this._o, rv = this._rv, dl = this._dl;
    const T = this.T;
    const pumpDepth = T?.pump?.depth ?? 0.6, pumpRel = (T?.pump?.release ?? 0.16) / 3;
    const kPump = 1 - Math.exp(-1 / (pumpRel * SR));
    const enHz = 250 * Math.pow(2, this.energy * 6.3);
    for (let i = 0; i < n; i++) {
      this._clock();
      // Automation (once every 64 samples is plenty).
      if ((this.t & 63) === 0) {
        for (let a = this.autos.length - 1; a >= 0; a--) {
          const A = this.autos[a];
          A.t += 64;
          const u = Math.min(1, A.t / A.len);
          A.lab[A.param] = A.from + (A.to - A.from) * u;
          if (u >= 1) this.autos.splice(a, 1);
        }
        this.enHz += (enHz - this.enHz) * 0.05;
      }
      this.t++;
      // Pump: down in 6 ms, held to 30 ms, then back with the release.
      if (this.pumpOn) {
        const ms = (this.pumpT++ / SR) * 1000;
        if (ms < 6) this.pumpG = 1 - pumpDepth * (ms / 6);
        else if (ms > 30) { this.pumpG += (1 - this.pumpG) * kPump; if (this.pumpG > 0.999) this.pumpOn = false; }
      }
      let pl = 0, pr = 0, ul = 0, ur = 0;
      rv[0] = rv[1] = 0; dl[0] = dl[1] = 0;
      for (const name in this.parts) {
        const P = this.parts[name];
        P.inst.run(P.ch.buf);
        P.ch.run(o);
        if (P.ch.pump) { pl += o[0]; pr += o[1]; } else { ul += o[0]; ur += o[1]; }
        if (P.ch.rev) { rv[0] += o[0] * P.ch.rev; rv[1] += o[1] * P.ch.rev; }
        if (P.ch.dly) { dl[0] += (o[0] + o[1]) * 0.5 * P.ch.dly; }
      }
      let dL = 0, dR = 0;
      this.drums.run((h, y) => {
        const g = y * DRUM_LEVEL;
        const p = h.pan ?? 0;
        const l = g * Math.min(1, 1 - p), r = g * Math.min(1, 1 + p);
        dL += l; dR += r;
        if (h.k.rev) { rv[0] += l * h.k.rev; rv[1] += r * h.k.rev; }
      });
      // The keyboard's instrument goes straight in, with a little reverb.
      if (this.audition) {
        this.audition.run(this.auditionCh.buf);
        this.auditionCh.run(o);
        ul += o[0]; ur += o[1]; rv[0] += o[0] * 0.25; rv[1] += o[1] * 0.25;
      }
      // Song filter over the instruments, drums and the delay return.
      s[0] = 0; s[1] = 0;
      this.delay.run(dl[0], s);
      let ml = pl * this.pumpG + ul + dL + s[0], mr = pr * this.pumpG + ur + dR + s[1];
      if (this.lpFrom < 19999 || this.lpTo < 19999) {
        if ((this.lpT & 31) === 0) {
          const u = this.lpLen ? Math.min(1, this.lpT / this.lpLen) : 1;
          const hz = this.lpFrom * Math.pow(this.lpTo / this.lpFrom, u);
          this.songLp[0].set(hz, 0.9); this.songLp[1].set(hz, 0.9);
        }
        this.lpT++;
        ml = this.songLp[0].run(ml); mr = this.songLp[1].run(mr);
      }
      for (let f = this.fx.length - 1; f >= 0; f--) {
        const F = this.fx[f];
        const y = F.run();
        ml += y; mr += y; rv[0] += y * 0.5; rv[1] += y * 0.5;
        if (F.done) this.fx.splice(f, 1);
      }
      s[0] = 0; s[1] = 0;
      this.reverb.run(rv[0] * this.mix.reverb, rv[1] * this.mix.reverb, s);
      ml += s[0] * 0.42; mr += s[1] * 0.42;
      // Energy: the whole mix closes down as the energy falls.
      if (this.enHz < 18000) {
        this.enLp[0].set(this.enHz, 0.8); this.enLp[1].set(this.enHz, 0.8);
        ml = this.enLp[0].run(ml); mr = this.enLp[1].run(mr);
      }
      const gain = T?.gain ?? 1;
      ml *= gain; mr *= gain;
      // Bus: glue, tape, master, limiter.
      const gr = this.mix.glue > 0 ? this.glue.run(ml, mr) : 1;
      this.grSum += gr; this.grN++;
      ml = this.tape[0].run(ml * gr) * this.mix.master;
      mr = this.tape[1].run(mr * gr) * this.mix.master;
      this.lim.run(ml, mr, o);
      L[i] = o[0]; R[i] = o[1];
    }
  }

  // Where the song is, for the page.
  get position() {
    if (!this.seq) return null;
    const p = this.seq.pos;
    return { sec: p.sec, bar: p.bar, step: p.step, barIndex: this.seq.barIndex, bars: this.seq.bars, playing: this.playing };
  }
  takeGr() { const g = this.grN ? this.grSum / this.grN : 1; this.grSum = 0; this.grN = 0; return g; }
}
