// Engine Sound Lab: the physical exhaust model, run per sample in an
// AudioWorklet. A prototype of docs/vision/sound.md 2.1 and 2.2, for
// listening only: nothing in the game uses it, and it is free to use Math.*.
//
// Signal flow, per sample (one engine, stereo out):
//
//   combustion events ─ pulse ─ header waveguide ┐ (one per cylinder)
//        │                                       ├─ bank collector ─ X-pipe ─┐
//        │                                       ┘                           │
//        │      ┌─────────────────────────────────────────────────────────────┘
//        │      └─ (turbine) ─ saturation ─ pipe waveguide ─ muffler sections
//        │           ─ tailpipe waveguide ─ radiation ─ body ─┐  (one per pipe)
//        ├─ intake pulses + roar ─ intake waveguide ─────────┤
//        ├─ valve-train ticks, gear whine ───────────────────┤
//        └─ overrun / limiter pops (into pipe and tail) ─────┤
//   turbo whistle, hiss, blow-off ────────────────────────────┴─ level ─ DC block
//                                              ─ psychoacoustic bass ─ out
//
// Combustion: each cylinder fires once per 720° at its point in the firing
// order. Every pulse draws its own strength and timing, with cycle-to-cycle
// variation that grows at idle and on overrun and shrinks under load; load
// also makes the pulse bigger and sharper (brighter). A pulse is a gamma-like
// blowdown shape plus turbulent flow noise.
//
// Waveguides: a delay line whose far end reflects (an area change: the
// collector, the muffler inlet, the open tailpipe end) and whose near end
// reflects back, with a one-pole loss in the loop. Lengths are in metres and
// turned into samples with the speed of sound in the hot gas, which rises a
// little with load. Unequal header lengths smear and re-time the pulses
// (the cross-plane burble); equal ones keep them evenly spaced (the scream).
// The muffler is a set of fixed resonant sections, so the firing rate sweeps
// through resonances that stay put: the "deep" part.
//
// Also here: the 'speaker-sim' processor, which simulates the speaker you
// listen on (phone, laptop, ...), so the owner can hear what a phone would.

const TAU = Math.PI * 2;
const clamp = (v, lo, hi) => (v < lo ? lo : v > hi ? hi : v);

class Biquad {
  constructor() { this.b0 = 1; this.b1 = this.b2 = this.a1 = this.a2 = 0; this.z1 = this.z2 = 0; }
  // RBJ cookbook. type: lowpass, highpass, bandpass (0 dB peak), peaking, lowshelf, highshelf.
  set(type, f, q = 0.707, db = 0) {
    f = clamp(f, 5, sampleRate * 0.45);
    const w = (TAU * f) / sampleRate, cs = Math.cos(w), sn = Math.sin(w);
    const al = sn / (2 * q), A = Math.pow(10, db / 40);
    let b0, b1, b2, a0, a1, a2;
    switch (type) {
      case 'lowpass': b0 = (1 - cs) / 2; b1 = 1 - cs; b2 = b0; a0 = 1 + al; a1 = -2 * cs; a2 = 1 - al; break;
      case 'highpass': b0 = (1 + cs) / 2; b1 = -(1 + cs); b2 = b0; a0 = 1 + al; a1 = -2 * cs; a2 = 1 - al; break;
      case 'bandpass': b0 = al; b1 = 0; b2 = -al; a0 = 1 + al; a1 = -2 * cs; a2 = 1 - al; break;
      case 'peaking': b0 = 1 + al * A; b1 = -2 * cs; b2 = 1 - al * A; a0 = 1 + al / A; a1 = -2 * cs; a2 = 1 - al / A; break;
      case 'lowshelf': case 'highshelf': {
        const s = type === 'lowshelf' ? 1 : -1, r = 2 * Math.sqrt(A) * (sn / 2) * Math.SQRT2;
        b0 = A * ((A + 1) - s * (A - 1) * cs + r);
        b1 = 2 * s * A * ((A - 1) - s * (A + 1) * cs);
        b2 = A * ((A + 1) - s * (A - 1) * cs - r);
        a0 = (A + 1) + s * (A - 1) * cs + r;
        a1 = -2 * s * ((A - 1) + s * (A + 1) * cs);
        a2 = (A + 1) + s * (A - 1) * cs - r;
        break;
      }
      default: b0 = 1; b1 = b2 = a1 = a2 = 0; a0 = 1;
    }
    this.b0 = b0 / a0; this.b1 = b1 / a0; this.b2 = b2 / a0; this.a1 = a1 / a0; this.a2 = a2 / a0;
    return this;
  }
  run(x) {
    const y = this.b0 * x + this.z1;
    this.z1 = this.b1 * x - this.a1 * y + this.z2;
    this.z2 = this.b2 * x - this.a2 * y;
    return y;
  }
}

class OnePole {
  constructor(hz = 1000) { this.y = 0; this.setHz(hz); }
  setHz(hz) { this.a = 1 - Math.exp((-TAU * hz) / sampleRate); return this; }
  run(x) { return (this.y += this.a * (x - this.y)); }
}

// A circular delay line read at a fractional distance (linear interpolation).
class Delay {
  constructor(n = 8192) { this.buf = new Float32Array(n); this.mask = n - 1; this.w = 0; }
  push(x) { this.buf[this.w] = x; this.w = (this.w + 1) & this.mask; }
  // d = 0 is the sample pushed last.
  read(d) {
    const p = this.w - 1 - d, i = Math.floor(p), fr = p - i;
    const a = this.buf[i & this.mask], b = this.buf[(i + 1) & this.mask];
    return a + (b - a) * fr;
  }
}

// A one-dimensional waveguide: wave enters at the near end, arrives at the far
// end `len` samples later; the far end reflects rFar of it (through the loop
// loss), which travels back and reflects rNear at the near end. Returns the
// wave arriving at the far end; (1 + rFar) of it is transmitted onwards.
class Guide {
  constructor() { this.d = new Delay(8192); this.loss = new OnePole(3000); this.len = 40; }
  step(x, rFar, rNear) {
    const L = this.len;
    const arrive = this.d.read(L - 1);
    const back = this.d.read(2 * L - 1);
    this.d.push(x + rNear * this.loss.run(rFar * back));
    return arrive;
  }
}

class Rng {
  constructor(seed = 1) { this.s = seed >>> 0 || 1; }
  next() { // mulberry32
    let t = (this.s = (this.s + 0x6d2b79f5) >>> 0);
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  }
  bi() { return this.next() * 2 - 1; }
  gauss() { return (this.next() + this.next() + this.next() + this.next() - 2) * 1.732; } // ~N(0,1)
}

const MAX_CYL = 16;
const MAX_POPS = 10;

class EngineLab extends AudioWorkletProcessor {
  constructor(opts) {
    super();
    const o = opts?.processorOptions || {};
    this.rng = new Rng(o.seed || 12345);
    this.sr = sampleRate;
    // Controls: targets from the page, smoothed per sample.
    this.rpmT = 800; this.thrT = 0; this.boostT = 0; this.speedT = 0;
    this.rpm = 800; this.thr = 0; this.boost = 0; this.speed = 0;
    this.overrun = 0; this.loadSlow = 0; this.lastThrT = 0;
    this.psycho = 0; this.running = true; this.gain = 0; this.gainT = 1;
    this.cyclePos = 0;
    this.hdr = [];
    for (let i = 0; i < MAX_CYL; i++) this.hdr.push({ g: new Guide(), t: 0, tau: 1, amp: 0, sharp: 1, on: false, bank: 0, lenM: 0.6 });
    this.pipes = [0, 1].map(() => ({
      dcIn: new Biquad(), turb: new OnePole(6000), guide: new Guide(), tail: new Guide(),
      mlp: new Biquad(), mlp2: new Biquad(), secs: [0, 1, 2, 3, 4, 5].map(() => new Biquad()), nSec: 0,
      radLp: new OnePole(80), body: new Biquad(), popBp: new Biquad(),
    }));
    this.intakeShape = new Biquad(); this.turbLp = new OnePole(1600); this.intakeGuide = new Guide(); this.intakeLp = new OnePole(2000);
    this.intakeNoiseBp = new Biquad();
    this.tick1 = new Biquad(); this.tick2 = new Biquad();
    this.whinePh = 0;
    this.turboPh = [0, 0]; this.hissBp = new Biquad(); this.bovBp = new Biquad(); this.bovT = -1; this.bovAmp = 0;
    this.pops = []; for (let i = 0; i < MAX_POPS; i++) this.pops.push({ t: 0, len: 1, amp: 0, pipe: 0, on: false });
    this.idleWob = new OnePole(1.5); this.idleWob2 = new OnePole(0.4);
    this.dcL = new Biquad().set('highpass', 18, 0.6); this.dcR = new Biquad().set('highpass', 18, 0.6);
    // Psychoacoustic bass: isolate the sub band, normalise it by its own
    // envelope, distort it into harmonics, keep the 2nd..5th, restore level.
    this.pbLp = new Biquad().set('lowpass', 140, 0.7); this.pbLp2 = new Biquad().set('lowpass', 140, 0.7);
    this.pbHp = new Biquad().set('highpass', 30, 0.7);
    this.pbOutHp = new Biquad().set('highpass', 150, 0.7); this.pbOutHp2 = new Biquad().set('highpass', 150, 0.7);
    this.pbOutLp = new Biquad().set('lowpass', 700, 0.7);
    this.pbEnv = 0;
    this.limitCut = false;
    this.setParams(o.params || {});
    if (o.state) this.setState(o.state, true);
    if (o.psycho !== undefined) this.psycho = o.psycho;
    this.port.onmessage = (e) => {
      const m = e.data;
      if (m.type === 'params') this.setParams(m.params);
      else if (m.type === 'state') this.setState(m);
      else if (m.type === 'psycho') this.psycho = m.amount;
      else if (m.type === 'run') this.gainT = m.on ? 1 : 0;
    };
  }

  setState(s, jump = false) {
    if (s.rpm !== undefined) this.rpmT = s.rpm;
    if (s.throttle !== undefined) this.thrT = clamp(s.throttle, 0, 1);
    if (s.boost !== undefined) this.boostT = clamp(s.boost, 0, 1);
    if (s.speed !== undefined) this.speedT = s.speed;
    if (jump) {
      this.rpm = this.rpmT; this.thr = this.thrT; this.boost = this.boostT; this.speed = this.speedT;
    this.lastThrT = this.thrT; this.loadSlow = this.thrT;
      this.overrun = this.thrT < 0.08 && this.rpm > (this.P?.idle || 800) * 1.6 ? 1 : 0;
    }
  }

  setParams(p) {
    const P = (this.P = {
      idle: 800, redline: 7000, events: [[0], [1], [0], [1]], angles: null, bankGain: [1, 1],
      tauDeg: 30, sharp: 1.5, variation: 0.1, jitterDeg: 2, misfire: 0, turbNoise: 0.3,
      headers: [0.6], headerScale: 1, headerSpread: 1, headerR: -0.5, headerRv: 0.85, headerLossHz: 3500,
      pipes: 2, xpipe: 0, pipeLen: 2.5, pipeR: -0.5, pipeRc: 0.3, pipeLossHz: 2500,
      mufflerLp: 2000, sections: [], mufflerTune: 1, mufflerGain: 1,
      tailLen: 0.5, tailR: -0.45, radHz: 80, radLow: 0.4, body: 3,
      drive: 1.5, intake: 1, intakeLen: 0.35, mech: 0.5, pops: 1, popHz: 900,
      imbalance: 0.1, turbo: 0, bov: 0, antiLag: 0, level: 1, width: 0.5, compThresh: -30, compRatio: 3, makeup: 9, ...p,
    });
    // Firing events: [bank, header index?]; angles default to even firing.
    const n = Math.min(P.events.length, MAX_CYL);
    this.n = n;
    const prev = this.ev || [];
    this.ev = [];
    const base = Math.floor(this.cyclePos);
    // Cylinder imbalance: no two cylinders breathe and burn quite alike, a
    // fixed difference in strength and firing angle per cylinder. It is what
    // puts energy at the half orders (below the firing rate) of an even-firing
    // engine: its lope.
    const ir = new Rng(4242);
    for (let k = 0; k < n; k++) {
      const [bank, hi = k] = P.events[k];
      const ang = (P.angles ? P.angles[k] : (k * 720) / n) + P.imbalance * 12 * ir.gauss();
      this.hdr[hi % MAX_CYL].imb = Math.max(0.3, 1 + P.imbalance * ir.gauss());
      const nominal = prev[k] && prev.length === n ? prev[k].nominal : base + ang / 720 + (ang / 720 < this.cyclePos - base ? 1 : 0);
      this.ev.push({ bank: bank & 1, hdr: hi % MAX_CYL, nominal, next: prev[k] && prev.length === n ? prev[k].next : nominal });
      this.hdr[hi % MAX_CYL].bank = bank & 1;
    }
    // Header lengths: around their mean, with the spread scaled.
    const hl = P.headers;
    let mean = 0;
    for (let i = 0; i < n; i++) mean += hl[i % hl.length];
    mean /= n;
    for (let i = 0; i < MAX_CYL; i++) {
      const raw = hl[i % hl.length];
      this.hdr[i].lenM = Math.max(0.08, (mean + (raw - mean) * P.headerSpread) * P.headerScale);
      this.hdr[i].g.loss.setHz(P.headerLossHz);
    }
    for (const pp of this.pipes) {
      pp.dcIn.set('highpass', 12, 0.6);
      pp.turb.setHz(P.turbo ? 3800 : 20000);
      pp.guide.loss.setHz(P.pipeLossHz);
      pp.tail.loss.setHz(1400);
      pp.mlp.set('lowpass', P.mufflerLp, 0.6);
      pp.mlp2.set('lowpass', P.mufflerLp * 1.6, 0.5);
      const secs = P.sections.slice(0, 6);
      pp.nSec = secs.length;
      secs.forEach(([f, q, db], i) => pp.secs[i].set('peaking', f * P.mufflerTune, q, db * P.mufflerGain));
      pp.radLp.setHz(P.radHz);
      pp.body.set('lowshelf', 120, 0.7, P.body);
      pp.popBp.set('bandpass', P.popHz, 0.9);
    }
    this.intakeGuide.loss.setHz(3000);
    this.tick1.set('bandpass', 4300, 9); this.tick2.set('bandpass', 7400, 12);
    this.retune();
  }

  // Waveguide lengths in samples from metres and the gas temperature.
  retune() {
    const P = this.P, sr = this.sr;
    const c = 460 + 120 * this.loadSlow; // m/s in the hot exhaust
    for (let i = 0; i < MAX_CYL; i++) this.hdr[i].g.len = Math.max(1.5, (this.hdr[i].lenM / c) * sr);
    for (const pp of this.pipes) {
      pp.guide.len = Math.max(2, (P.pipeLen / (c * 0.92)) * sr); // cooler downstream
      pp.tail.len = Math.max(2, (P.tailLen / (c * 0.85)) * sr);
    }
    this.intakeGuide.len = Math.max(1.5, (P.intakeLen / 343) * sr);
  }

  spawnPop(amp, pipe) {
    for (const p of this.pops) {
      if (p.on) continue;
      const r = this.rng;
      p.on = true; p.t = 0; p.amp = amp; p.pipe = pipe;
      p.len = this.sr * (0.004 + r.next() * 0.018);
      return;
    }
  }

  fire(ev) {
    const P = this.P, r = this.rng, rpm = this.rpm;
    const h = this.hdr[ev.hdr];
    const idleness = clamp(1 - (rpm - P.idle) / 1300, 0, 1);
    const load = Math.max(this.thr, 0.3 * idleness) + 0.25 * this.boost * this.thr;
    const over = this.overrun;
    const rn = clamp((rpm - P.idle) / (P.redline - P.idle), 0, 1);
    // Rev limiter: ignition cut on about half the events; unburnt charge
    // sometimes lights in the pipe.
    if (this.limitCut && r.next() < 0.55) {
      h.on = true; h.t = 0; h.amp = 0.12; h.tau = Math.max(this.sr * 0.0003, ((P.tauDeg * 1.4) / (6 * rpm)) * this.sr); h.sharp = 0.9;
      if (r.next() < 0.22 * P.pops) this.spawnPop(0.5 + 0.5 * r.next(), ev.bank);
      return;
    }
    // Cycle-to-cycle variation: largest at idle and on overrun, smallest under load.
    const v = P.variation * (0.45 + 1.4 * idleness + 1.8 * over) * (1 - 0.6 * Math.min(1, load));
    let amp = (0.2 + 0.8 * Math.pow(Math.min(1.3, load), 0.8)) * (1 + v * r.gauss());
    if (r.next() < P.misfire * (idleness + over) * 0.03) amp *= 0.15;
    amp = Math.max(0.02, amp) * P.bankGain[ev.bank] * h.imb;
    const tauDeg = P.tauDeg * (1.15 - 0.3 * Math.min(1, load)) * (1 + 0.3 * over);
    h.on = true; h.t = 0; h.amp = amp;
    h.tau = Math.max(this.sr * 0.00025, (tauDeg / (6 * rpm)) * this.sr); // deg → s at this rpm
    h.sharp = P.sharp * (0.7 + 0.55 * Math.min(1, load)) * (over ? 0.8 : 1);
    // Intake: a suction pulse into the intake shaper, scaled by the throttle opening.
    this.intakeKick -= (0.25 + 0.75 * this.thr) * (0.6 + 0.4 * r.next());
    // Valve train: a faint tick per event.
    this.tickKick += P.mech * (0.3 + 0.7 * r.next()) * (0.4 + 0.6 * rn);
    // Overrun crackle: unburnt fuel lighting in the hot pipe, off-throttle at high rpm.
    // Pops per second: a few on a long overrun, a burst just after a lift,
    // more with anti-lag; spread over the firing events.
    const perSec = P.pops * (over * clamp((rn - 0.3) / 0.4, 0, 1) * (4 + 22 * this.liftBurst) + P.antiLag * (this.thr < 0.15 ? 1 : 0) * rn * 6);
    if (r.next() < perSec / ((rpm / 120) * this.n)) this.spawnPop((0.4 + 0.6 * r.next()) * (0.5 + 0.5 * rn), ev.bank);
  }

  process(_inputs, outputs) {
    const out = outputs[0];
    const L = out[0], R = out[1] || out[0];
    const N = L.length, P = this.P, sr = this.sr, r = this.rng;
    // Per-block control work.
    this.loadSlow += (this.thr - this.loadSlow) * (N / sr) * 2;
    this.retune();
    const kc = 1 - Math.exp(-1 / (0.008 * sr)), kt = 1 - Math.exp(-1 / (0.012 * sr)), kb = 1 - Math.exp(-1 / (0.05 * sr));
    if (P.turbo && this.lastThrT > 0.5 && this.thrT < 0.2 && this.boost > 0.35 && P.bov > 0) {
      this.bovT = 0; this.bovAmp = this.boost;
    }
    if (this.lastThrT > 0.5 && this.thrT < 0.15) this.liftBurst = 1;
    this.liftBurst *= Math.exp(-(N / sr) / 0.35);
    this.lastThrT = this.thrT;
    const overT = this.thrT < 0.08 && this.rpmT > P.idle * 1.6 ? 1 : 0;
    this.overrun += (overT - this.overrun) * Math.min(1, (N / sr) * 12);
    this.limitCut = this.rpm >= P.redline * 0.985 && this.thr > 0.5;
    this.intakeShape.set('lowpass', clamp((this.rpm * this.n) / 120 * 1.5, 30, 4000), 0.55);
    this.intakeLp.setHz(300 + 2200 * this.thr);
    this.intakeNoiseBp.set('bandpass', 200 + 900 * (this.rpm / P.redline), 0.8);
    this.hissBp.set('bandpass', 1400 + 2600 * this.boost, 1.5);
    const sat = P.drive, satN = 1 / sat;
    const hr = P.headerR, hrv = P.headerRv, ht = 1 + hr;
    const width = P.width;
    const turboAmt = P.turbo ? 0.55 + 0.45 * this.boost : 0;
    const rn = clamp((this.rpm - P.idle) / (P.redline - P.idle), 0, 1);

    for (let i = 0; i < N; i++) {
      this.rpm += (this.rpmT - this.rpm) * kc;
      this.thr += (this.thrT - this.thr) * kt;
      this.boost += (this.boostT - this.boost) * kb;
      this.speed += (this.speedT - this.speed) * kb;
      this.gain += (this.gainT * this.running - this.gain) * 0.0005;
      // Idle hunting: a slow wander in speed, only near idle.
      const idleness = clamp(1 - (this.rpm - P.idle) / 1300, 0, 1);
      const wob = this.idleWob2.run(this.idleWob.run(r.bi() * 30));
      const rpmNow = Math.max(200, this.rpm * (1 + 0.02 * idleness * wob));
      this.cyclePos += rpmNow / 120 / sr;
      for (let k = 0; k < this.n; k++) {
        const ev = this.ev[k];
        let guard = 0;
        while (this.cyclePos >= ev.next && guard++ < 2) {
          this.fire(ev);
          ev.nominal += 1;
          const jit = (P.jitterDeg / 720) * (0.5 + 1.5 * idleness + this.overrun) * r.gauss();
          ev.next = ev.nominal + clamp(jit, -0.4 / this.n, 0.4 / this.n);
        }
      }

      // Combustion pulses into the headers; headers into the bank collectors.
      // Turbulent flow noise rides on each pulse; its spectrum falls off.
      const turb = P.turbNoise * (0.4 + 0.6 * this.thr) * this.turbLp.run(r.bi()) * 2.5;
      let b0 = 0, b1 = 0;
      for (let k = 0; k < this.n; k++) {
        const h = this.hdr[this.ev[k].hdr];
        let p = 0;
        if (h.on) {
          const u = h.t / h.tau;
          if (u > 12) h.on = false;
          else {
            const g = u * Math.exp(1 - u);
            p = h.amp * Math.pow(g, h.sharp) * (1 + turb);
            h.t++;
          }
        }
        const y = h.g.step(p, hr, hrv) * ht;
        if (h.bank) b1 += y; else b0 += y;
      }
      if (P.pipes < 2) { b0 += b1; b1 = 0; }
      else if (P.xpipe > 0) { const x = P.xpipe, m0 = b0; b0 = (1 - x) * b0 + x * b1; b1 = (1 - x) * b1 + x * m0; }

      // Pops: a noise burst in the pipe and a crack at the tail.
      let pop0 = 0, pop1 = 0;
      for (const pp of this.pops) {
        if (!pp.on) continue;
        const env = Math.min(1, pp.t / (sr * 0.0004)) * Math.exp(-pp.t / pp.len);
        const s = pp.amp * env * r.bi() * 0.45;
        if (pp.pipe && P.pipes >= 2) pop1 += s; else pop0 += s;
        if (++pp.t > pp.len * 7) pp.on = false;
      }

      let oL = 0, oR = 0;
      const np = P.pipes >= 2 ? 2 : 1;
      for (let j = 0; j < np; j++) {
        const pp = this.pipes[j];
        let s = j ? b1 : b0;
        s = pp.dcIn.run(s);
        if (turboAmt) s = pp.turb.run(s) * (1 - 0.3 * turboAmt);
        // High-level pressure waves steepen and compress: asymmetric soft clip.
        const d = s * sat;
        s = (d >= 0 ? Math.tanh(d) : Math.tanh(0.75 * d) / 0.75) * satN;
        const pop = j ? pop1 : pop0;
        s += pop * 0.6;
        let m = pp.guide.step(s, P.pipeR, P.pipeRc) * (1 + P.pipeR);
        m = pp.mlp2.run(pp.mlp.run(m));
        for (let q = 0; q < pp.nSec; q++) m = pp.secs[q].run(m);
        m += pp.popBp.run(pop) * 1.4;
        const tl = pp.tail.step(m, P.tailR, 0.25);
        // Open-end radiation: the low end radiates less (high-pass-ish shelf).
        let rad = tl - (1 - P.radLow) * pp.radLp.run(tl);
        rad = pp.body.run(rad);
        if (np === 1) { oL += rad; oR += rad; }
        else if (j === 0) { oL += rad * (0.5 + 0.5 * width); oR += rad * (0.5 - 0.5 * width); }
        else { oL += rad * (0.5 - 0.5 * width); oR += rad * (0.5 + 0.5 * width); }
      }

      // Intake: suction pulses shaped by a low-pass, plus roar noise; the
      // throttle is the opening (low-pass and level).
      const ik = this.intakeKick; this.intakeKick = 0;
      let isrc = this.intakeShape.run(ik * 6) + this.intakeNoiseBp.run(r.bi()) * 0.05 * this.thr * (0.3 + rn);
      let iv = this.intakeGuide.step(isrc, -0.75, 0.6);
      iv = this.intakeLp.run(iv) * P.intake * (0.15 + 0.85 * this.thr) * 0.5;

      // Mechanical: valve-train ticks and a faint gear whine with road speed.
      const tk = this.tickKick; this.tickKick = 0;
      let mech = (this.tick1.run(tk) + 0.6 * this.tick2.run(tk)) * 0.012;
      if (this.speed > 0.5) {
        this.whinePh += (this.speed * 26) / sr; this.whinePh -= Math.floor(this.whinePh);
        mech += Math.sin(this.whinePh * TAU) * 0.0025 * P.mech * Math.min(1, this.speed / 30) * (0.3 + 0.7 * this.thr);
      }

      // Turbo: shaft whistle (blade pass), intake hiss, blow-off valve.
      let tb = 0;
      if (P.turbo) {
        const b = this.boost;
        for (let t = 0; t < Math.min(2, P.turbo); t++) {
          const f = (1900 + b * 5200 + rn * 700) * (t ? 1.031 : 1);
          this.turboPh[t] += f / sr; this.turboPh[t] -= Math.floor(this.turboPh[t]);
          const ph = this.turboPh[t] * TAU;
          tb += (Math.sin(ph) + 0.3 * Math.sin(ph * 1.5)) * 0.008 * b * b * (0.4 + 0.6 * this.thr);
        }
        tb += this.hissBp.run(r.bi()) * 0.025 * b * this.thr;
        if (this.bovT >= 0) {
          const t = this.bovT / sr;
          if ((this.bovT & 31) === 0) this.bovBp.set('bandpass', 1300 + 2300 * Math.exp(-t / 0.15), 1.3);
          const env = Math.min(1, t / 0.006) * Math.exp(-t / 0.13);
          tb += this.bovBp.run(r.bi()) * env * 0.25 * P.bov * this.bovAmp;
          if (++this.bovT > sr * 0.8) this.bovT = -1;
        }
      }

      const c = (iv + mech + tb);
      let l = (oL + c) * P.level * this.gain;
      let rr = (oR + c) * P.level * this.gain;
      l = this.dcL.run(l); rr = this.dcR.run(rr);

      // Psychoacoustic bass: harmonics of the sub band so a small speaker
      // still implies the fundamental.
      if (this.psycho > 0) {
        const sub = this.pbHp.run(this.pbLp2.run(this.pbLp.run((l + rr) * 0.5)));
        const a = Math.abs(sub);
        this.pbEnv += (a - this.pbEnv) * (a > this.pbEnv ? 0.004 : 0.0004);
        const nrm = sub / (this.pbEnv + 1e-4);
        const hgen = 0.5 * Math.abs(nrm) + 0.5 * Math.tanh(2.5 * nrm);
        const hb = this.pbOutLp.run(this.pbOutHp2.run(this.pbOutHp.run(hgen))) * this.pbEnv * 7 * this.psycho;
        l += hb; rr += hb;
      }
      // Bus compressor (like the game's SFX bus): RMS-ish detector, soft ratio.
      const lev = (l * l + rr * rr) * 0.5;
      this.compEnv += (lev - this.compEnv) * (lev > this.compEnv ? 0.0042 : 0.00014);
      const envDb = 10 * Math.log10(this.compEnv + 1e-12), over = envDb - P.compThresh;
      const cg = over > 0 ? Math.pow(10, (-over * (1 - 1 / P.compRatio)) / 20) : 1;
      l *= cg * P.makeup; rr *= cg * P.makeup;
      // Peak limiter (instant attack, ~60 ms release), as the game's master has.
      const pk = Math.max(Math.abs(l), Math.abs(rr));
      this.limEnv = pk > this.limEnv ? pk : this.limEnv + (pk - this.limEnv) * 0.00035;
      if (this.limEnv > 0.84) { const lg = 0.84 / this.limEnv; l *= lg; rr *= lg; }
      // Safety: soft knee above 0.9 so a bad tweak can't blast.
      L[i] = Math.abs(l) > 0.9 ? Math.sign(l) * (0.9 + 0.1 * Math.tanh((Math.abs(l) - 0.9) * 10)) : l;
      R[i] = Math.abs(rr) > 0.9 ? Math.sign(rr) * (0.9 + 0.1 * Math.tanh((Math.abs(rr) - 0.9) * 10)) : rr;
    }
    return true;
  }
}
EngineLab.prototype.intakeKick = 0;
EngineLab.prototype.tickKick = 0;
EngineLab.prototype.liftBurst = 0;
EngineLab.prototype.compEnv = 0;
EngineLab.prototype.limEnv = 0;
registerProcessor('engine-lab', EngineLab);

// What the target speaker does to the sound: its low-frequency cut (steep,
// with a little resonant bump), its presence peak, and on a phone, mono and a
// touch of small-driver distortion.
const PROFILES = {
  headphones: { chain: [] },
  big: { chain: [['highpass', 30, 0.7]] },
  laptop: { chain: [['highpass', 170, 0.54], ['highpass', 170, 1.31], ['peaking', 600, 1.5, -2], ['peaking', 2800, 1, 3], ['lowpass', 15000, 0.7]] },
  phone: { mono: true, drive: 1.3, chain: [['highpass', 330, 0.54], ['highpass', 330, 1.31], ['peaking', 1000, 1.2, 4], ['peaking', 3500, 2, 3], ['lowpass', 9000, 0.7]] },
};

class SpeakerSim extends AudioWorkletProcessor {
  constructor(opts) {
    super();
    this.set(opts?.processorOptions?.profile || 'headphones');
    this.port.onmessage = (e) => { if (e.data.type === 'profile') this.set(e.data.name); };
  }
  set(name) {
    const p = PROFILES[name] || PROFILES.headphones;
    this.p = p;
    this.f = [0, 1].map(() => p.chain.map(([t, f, q, db]) => new Biquad().set(t, f, q, db || 0)));
  }
  process(inputs, outputs) {
    const inp = inputs[0], out = outputs[0];
    const N = out[0].length;
    if (!inp || !inp.length) { for (const ch of out) ch.fill(0); return true; }
    const iL = inp[0], iR = inp[1] || inp[0];
    const p = this.p;
    for (let i = 0; i < N; i++) {
      let l = iL[i], r = iR[i];
      if (p.mono) l = r = (l + r) * 0.5;
      for (const f of this.f[0]) l = f.run(l);
      for (const f of this.f[1]) r = f.run(r);
      if (p.drive) { l = Math.tanh(l * p.drive) / p.drive; r = Math.tanh(r * p.drive) / p.drive; }
      out[0][i] = l;
      if (out[1]) out[1][i] = r;
    }
    return true;
  }
}
registerProcessor('speaker-sim', SpeakerSim);
