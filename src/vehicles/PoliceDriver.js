import { clamp, damp, smoothstep } from '../util/math.js';
import { KinematicCar } from './Kinematic.js';
import { findBlock, passLat } from './AIDriver.js';

// A police unit (Hot Pursuit). Lives in track coordinates like the rivals
// and traffic, so walls, spin and collision response come for free. The
// Pursuit decides who it chases and which tactic it uses (`behaviour`,
// `slot`); this class only drives it there.
//
// mode:
//   parked     waiting on the shoulder, lights off (patrol)
//   chase      after `target` using `behaviour`:
//                chase  follow a few lengths back
//                bump   drive into the target's rear bumper
//                pit    line up on a rear quarter and push it sideways
//                roll   get in front and brake gently (rolling block)
//                box    hold a slot round the target (`slot`: ahead,
//                       left, right, behind) at its speed minus a little
//   oncoming   spawned ahead in the oncoming lane; U-turns once it passes
//              its target
//   search     lost the suspect: cruise to its last known s and weave
//   hold       stopped beside a busted car, lights flashing
//   standdown  off the chase: roll to a stop on the shoulder
//   block      a roadblock or spike-strip car: parked at an angle, heavy
//   disabled   taken down: rolls to a stop, smoking

export const UNIT_TYPES = {
  patrol: { mass: 1700, top: 0.88, power: 520, aggression: 0.4 },
  interceptor: { mass: 1550, top: 1.0, power: 640, aggression: 0.7 },
  suv: { mass: 2400, top: 0.93, power: 560, aggression: 1.0 },
};

const BLOCK_MASS = 6000; // roadblock cars barely move when you hit them

export class PoliceDriver extends KinematicCar {
  constructor(vehicle, track, type = 'patrol') {
    super(vehicle, track);
    this.police = true;
    this.type = type;
    this.spec = UNIT_TYPES[type];
    this.aggression = this.spec.aggression;
    this.active = false;
    this.mode = 'parked';
    this.behaviour = 'chase';
    this.behT = 0;          // time in the current behaviour
    this.slot = null;       // box slot
    this.pitSide = 1;       // which side of the target a PIT lines up on
    this.pitCooldown = 0;
    this.target = null;     // a racer body (s, lat, speedAlong, halfW, halfL)
    this.lastSeenS = 0;
    this.health = 1;
    this.disabledT = 0;
    this.modeT = 0;
    this.parkLat = 0;
    this.blockYaw = 0;
    this.gapLat = null;     // roadblock cars: where the gap is (rivals aim for it)
    this.laneLat = 0;
    this.siren = 'off';     // 'off' | 'flash' | 'disabled'
    this.cap = 60;          // top speed (m/s), set by the Pursuit from heat
    this.weave = Math.random() * 10;
  }

  get mass() { return this.mode === 'block' ? BLOCK_MASS : this.spec.mass; }
  get disabled() { return this.mode === 'disabled'; }
  // Units that are actually on the chase (count toward the heat's limit).
  get chasing() { return this.active && (this.mode === 'chase' || this.mode === 'oncoming' || this.mode === 'search'); }

  setMode(mode) {
    if (this.mode === mode) return;
    this.mode = mode;
    this.modeT = 0;
    if (mode === 'chase') { this.behaviour = 'chase'; this.behT = 0; this.slot = null; }
  }

  update(dt, ctx) {
    const t = this.track;
    const F = this.frame();
    this.modeT += dt;
    this.behT += dt;
    this.pitCooldown = Math.max(0, this.pitCooldown - dt);
    const myW = this.v.halfW;
    const lim = Math.min(F.wallR, F.wallL) - myW - 0.35;
    let vT = 0, latT = this.lat, acc = Math.min(10, this.spec.power / Math.max(this.speed, 5)), brake = 14;

    switch (this.mode) {
      case 'block': {
        // Parked across the road at an angle; hits shove it a little.
        this.speed = damp(this.speed, 0, 5, dt);
        this.latVel = damp(this.latVel, 0, 5, dt);
        this.s += this.speed * this.dir * dt;
        this.lat += this.latVel * dt;
        this.spinRate = damp(this.spinRate, 0, 4, dt);
        this.blockYaw += this.spinRate * dt * 0.3;
        this.spin = this.blockYaw;
        this.writePos();
        return;
      }
      case 'parked':
        vT = 0; latT = this.parkLat; brake = 20;
        break;
      case 'hold':
        vT = 0; latT = this.lat; brake = 9;
        break;
      case 'standdown': {
        // Off the chase: pull onto the nearer shoulder and stop.
        const side = this.lat >= 0 ? 1 : -1;
        vT = Math.max(0, this.speed - 6 * dt);
        latT = side * Math.max(0, (side > 0 ? F.wallR : F.wallL) - myW - 0.5);
        brake = 6;
        break;
      }
      case 'disabled':
        vT = 0; latT = this.lat; brake = 7;
        this.disabledT += dt;
        break;
      case 'oncoming': {
        // Head toward the suspect in the oncoming lane; once past it, flip
        // round in a cloud of tyre smoke and give chase.
        vT = Math.min(t.speedProfile[t.idx(this.s)] * 0.8, 26);
        latT = this.laneLat;
        // Slow right down for anything coming at us in our lane.
        for (const o of ctx.cars) {
          if (o === this) continue;
          const ahead = t.ds(o.s, this.s); // > 0: o is in front of us (we drive toward lower s)
          if (ahead > 0 && ahead < 70 && Math.abs(o.lat - this.lat) < this.halfW + o.halfW + 0.6) vT = Math.min(vT, 4);
        }
        const T = this.target;
        if (T && t.ds(T.s, this.s) < -4) {
          this.dir = 1;
          this.speed = 3;
          this.spin = Math.PI; // the heading flips with dir; the spin unwinds it into a U-turn
          this.spinRate = 0;
          this.uturned = true;
          this.setMode('chase');
        }
        break;
      }
      case 'search': {
        // Cruise toward where the suspect was last seen, weaving across
        // the lanes to look down the side roads.
        const d = t.ds(this.s, this.lastSeenS);
        vT = d > -60 ? Math.min(t.speedProfile[t.idx(this.s + 20)], 26) : 12;
        this.weave += dt;
        latT = Math.sin(this.weave * 0.5) * lim * 0.7;
        break;
      }
      case 'chase': {
        const T = this.target;
        if (!T) { vT = 20; break; }
        const r = this.driveChase(T, F, lim, dt);
        vT = r.vT; latT = r.latT;
        break;
      }
    }

    // Stay out of other cars' way (except whoever we're trying to hit).
    // Police avoid civilians, but an aggressive unit won't give up the
    // chase for one: it ploughs through.
    if (this.mode === 'chase' || this.mode === 'search') {
      const T = this.target;
      const block = findBlock(this, ctx.cars, latT, (o) => o === T || (o.police && this.behaviour === 'box'));
      if (block) {
        const choice = passLat(this, block, F);
        if (choice !== null) { this.avoid = choice; this.avoidTimer = 0.7; }
        else if (block.dir === 1 && !(block.kinematicOnly && this.aggression > 0.6)) vT = Math.min(vT, block.speedAlong - 0.5);
      }
      if (this.avoidTimer > 0) { this.avoidTimer -= dt; latT = this.avoid; }
    }
    if (this.stunned > 0) vT *= 0.6;
    latT = clamp(latT, -lim, lim);

    // Lateral controller: a little sharper than the rivals'. A PIT push
    // adds lateral speed on top.
    const latA = clamp((latT - this.lat) * 4 - this.latVel * 2.8, -9, 9);
    this.latVel += latA * dt;
    if (this.mode === 'chase' && this.behaviour === 'pit' && this.pitPush) this.latVel += this.pitPush * dt;
    this.latVel = clamp(this.latVel, -8, 8);

    const dv = vT - this.speed;
    this.speed += dv > 0 ? Math.min(dv, acc * dt) : Math.max(dv, -brake * dt);
    this.speed = Math.max(0, this.speed);
    this.v.brakeLight = dv < -1.5 ? 1 : 0;
    this.v.accelLong = dv > 0 ? acc * 0.6 : -Math.min(8, brake);
    this.v.accelLat = this.speed * this.speed * F.kappa;
    this.advance(dt);
  }

  // Speed and lane for a unit on the chase.
  driveChase(T, F, lim, dt) {
    const t = this.track;
    const gap = t.ds(this.s, T.s);             // > 0: the target is ahead
    const contact = this.halfL + T.halfL;
    const look = clamp(this.speed * 0.5, 6, 40);
    const profile = Math.min(t.speedProfile[t.idx(this.s)], t.speedProfile[t.idx(this.s + look)]);
    // Far behind: flat out (a little over the corner speeds the rivals
    // use, and +8 m/s over the heat's top speed), so the chase stays tense.
    let vT = Math.min(profile * 1.04, this.cap);
    if (gap > 60) vT = Math.min(profile * 1.12, this.cap) + 8 * smoothstep(60, 140, gap);
    let latT = T.lat;
    if (gap > 60 || gap < -60) return { vT: gap < -60 ? Math.min(vT, T.speedAlong * 0.8) : vT, latT };

    const side = (want) => {
      // A lane beside the target on the `want` side, or the other if that
      // one doesn't fit between the walls.
      const off = T.halfW + this.halfW + 0.5;
      const a = T.lat + want * off, b = T.lat - want * off;
      return Math.abs(a) < lim ? a : Math.abs(b) < lim ? b : null;
    };
    let want; // the gap to hold (negative = in front)
    this.pitPush = 0;
    switch (this.behaviour) {
      case 'bump':
        want = contact - 1.2;
        break;
      case 'pit': {
        want = contact * 0.35;
        const l = side(this.pitSide);
        if (l === null) { want = contact + 5; break; }
        latT = T.lat + (l - T.lat) * 0.55; // overlapping its rear quarter
        // Alongside the rear quarter: steer into it.
        if (gap < contact * 0.9 && gap > -1) this.pitPush = -Math.sign(l - T.lat) * 14 * this.aggression;
        break;
      }
      case 'roll':
      case 'box': {
        const slot = this.behaviour === 'roll' ? 'ahead' : this.slot || 'behind';
        if (slot === 'ahead') {
          want = -(contact + 3);
          // Still behind it: go round first.
          if (gap > -contact) latT = side(T.lat > this.lat ? -1 : 1) ?? T.lat;
        } else if (slot === 'left' || slot === 'right') {
          const l = side(slot === 'left' ? -1 : 1);
          want = l === null ? contact + 1.5 : 0;
          latT = l ?? T.lat;
        } else want = contact + 1.2;
        break;
      }
      default:
        want = contact + 7;
    }
    // Hold the gap: target speed plus a correction, harder when aggressive.
    const push = this.behaviour === 'roll' || (this.behaviour === 'box' && this.slot === 'ahead') ? -4 : 0;
    const corr = clamp((gap - want) * 0.9, -12, 6 + this.aggression * 5);
    vT = Math.min(this.cap + 8, Math.max(0, T.speedAlong + corr + push));
    // Don't take corners faster than the road allows while following.
    vT = Math.min(vT, profile * 1.12 + 4);
    return { vT, latT };
  }
}
