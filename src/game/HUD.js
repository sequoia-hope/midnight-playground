import { clamp } from '../util/math.js';

const $ = (id) => document.getElementById(id);
const ORD = (n) => (n % 10 === 1 && n % 100 !== 11 ? 'st' : n % 10 === 2 && n % 100 !== 12 ? 'nd' : n % 10 === 3 && n % 100 !== 13 ? 'rd' : 'th');

export function fmtTime(t) {
  if (t == null || !isFinite(t)) return '--:--.--';
  // Round to hundredths first, so 59.996 s reads 1:00.00, not 0:60.00.
  const cs = Math.round(t * 100), m = Math.floor(cs / 6000), s = (cs - m * 6000) / 100;
  return `${m}:${s.toFixed(2).padStart(5, '0')}`;
}

export class HUD {
  constructor(track, level) {
    this.track = track;
    this.level = level;
    this.cruise = level.mode === 'cruise';
    this.root = $('hud');
    this.el = {
      pos: $('hud-pos'), suf: $('hud-pos-suf'), of: $('hud-of'), time: $('hud-time'), zone: $('hud-zone'),
      speed: $('hud-speed'), unit: $('hud-unit'), gear: $('hud-gear'), nitro: $('hud-nitro'), center: $('hud-center'),
      toast: $('hud-toast'), zoneCard: $('zone-card'), dots: $('route-dots'), speedlines: $('speedlines'),
      lap: $('hud-lap'), lapN: $('hud-lap-n'), lapTime: $('hud-lap-time'), lapBest: $('hud-lap-best'),
    };
    // Circuits (a loop raced over laps): lap count and times under the clock.
    this.el.lap.classList.toggle('hidden', !track.laps);
    this.el.nitroBox = this.el.nitro.parentElement;
    this.tach = $('tach').getContext('2d');
    this.mini = $('minimap').getContext('2d');
    this.mph = true;
    this.lastZone = -1;
    this.toastTimer = 0;
    this.centerTimer = 0;
    // Route bar: one segment per zone, widths follow the real zone lengths.
    const bar = $('route-bar');
    bar.querySelectorAll('.route-seg').forEach((e) => e.remove());
    track.zones.forEach((z, i) => {
      const seg = document.createElement('div');
      seg.className = 'route-seg';
      seg.style.flex = `${Math.max(1, z.s1 - z.s0)} 0 0`;
      seg.style.background = z.color || '#888';
      if (i === 0) seg.style.borderRadius = '4px 0 0 4px';
      if (i === track.zones.length - 1) { seg.style.borderRadius = i === 0 ? '4px' : '0 4px 4px 0'; seg.style.borderRight = '0'; }
      seg.innerHTML = `<span>${z.name.toLowerCase().replace(/\b\w/g, (c) => c.toUpperCase())}</span>`;
      bar.insertBefore(seg, this.el.dots);
    });
    // Race vs cruise furniture.
    document.querySelector('.hud-route').classList.toggle('hidden', this.cruise);
    document.querySelector('.hud-tl .pos').classList.toggle('hidden', this.cruise);
    $('hud-cruise').classList.toggle('hidden', !this.cruise);
    this.cruiseEl = { score: $('hud-score'), mult: $('hud-mult'), fill: $('hud-mult-fill'), dist: $('hud-dist'), best: $('hud-best') };
    this.bestScore = 0;
    this.dots = [];
    this.last = {};
    // Hot Pursuit furniture: heat stars, bust/evade bar, damage bar, the
    // penalty-hold card, the served-penalty line and radio chatter.
    this.pz = {
      root: $('hud-pz'), stars: $('pz-stars'), bar: $('pz-bar'), label: $('pz-label'), fill: $('pz-fill'),
      dmg: $('hud-dmg'), dmgFill: $('hud-dmg-fill'), pen: $('hud-pen'),
      hold: $('hud-hold'), holdTitle: $('hold-title'), holdSub: $('hold-sub'), holdFill: $('hold-fill'),
      radio: $('hud-radio'), radioText: $('hud-radio-text'),
    };
    this.pz.root.classList.toggle('cruise', this.cruise);
    this.pz.stars.innerHTML = '';
    this.starFill = [];
    for (let i = 0; i < 5; i++) {
      const star = document.createElement('span'), fill = document.createElement('i');
      star.className = 'pz-star';
      star.appendChild(fill);
      this.pz.stars.appendChild(star);
      this.starFill.push(fill);
    }
    this.clock = 0;
    this.radioTimer = 0;
    this.pursuitOn = null;
    this.setPursuit(false);
  }

  show(on) { this.root.classList.toggle('hidden', !on); }

  // Shows or hides all the Hot Pursuit furniture. update() also calls it
  // from st.pursuit, so a race without the mode never shows any of it.
  setPursuit(on) {
    on = !!on;
    if (on === this.pursuitOn) return;
    this.pursuitOn = on;
    const p = this.pz;
    p.root.classList.toggle('hidden', !on);
    p.dmg.classList.toggle('hidden', !on);
    this.el.nitroBox.parentElement.classList.toggle('pz-on', on);
    if (!on) {
      p.pen.classList.add('hidden');
      p.hold.classList.add('hidden');
      p.radio.classList.remove('show');
      this.radioTimer = 0;
    }
  }

  // A line of police radio chatter, bottom centre. Rate limiting is the
  // caller's job; a new line replaces the current one.
  radio(text, dur = 3) {
    const r = this.pz.radio;
    this.pz.radioText.textContent = text;
    r.classList.remove('show');
    void r.offsetWidth; // restart the slide-in
    r.classList.add('show');
    this.radioTimer = dur;
  }

  setRacers(racers) {
    this.el.dots.innerHTML = '';
    this.dots = racers.map((r) => {
      const d = document.createElement('div');
      d.className = 'rdot' + (r.player ? ' me' : '');
      d.style.background = '#' + r.color.toString(16).padStart(6, '0');
      this.el.dots.appendChild(d);
      return d;
    });
    this.el.of.textContent = racers.length;
  }

  center(text, cls = 'pop', dur = 1) {
    const c = this.el.center;
    c.className = '';
    c.textContent = text;
    void c.offsetWidth; // restart animation
    c.className = cls;
    this.centerTimer = dur;
  }

  toast(text, dur = 1.6) {
    this.el.toast.textContent = text;
    this.el.toast.classList.add('show');
    this.toastTimer = dur;
  }

  zoneCard(z) {
    const c = this.el.zoneCard;
    c.querySelector('.zc-name').textContent = z.name;
    c.querySelector('.zc-sub').textContent = z.sub;
    c.classList.remove('show');
    void c.offsetWidth;
    c.classList.add('show');
  }

  set(key, el, val) {
    if (this.last[key] !== val) { this.last[key] = val; el.textContent = val; }
  }

  update(dt, st) {
    const e = this.el;
    this.set('pos', e.pos, st.position);
    this.set('suf', e.suf, ORD(st.position));
    this.set('time', e.time, fmtTime(st.time));
    const spd = st.speed * (this.mph ? 2.23694 : 3.6);
    this.set('speed', e.speed, Math.round(Math.abs(spd)));
    this.set('unit', e.unit, this.mph ? 'MPH' : 'KM/H');
    this.set('gear', e.gear, st.gear === -1 ? 'R' : st.gear === 0 ? 'N' : st.electric ? 'D' : String(st.gear));
    e.nitro.style.width = (st.nitro * 100).toFixed(1) + '%';
    e.nitroBox.classList.toggle('active', !!st.nitroActive);
    e.speedlines.style.opacity = clamp((st.speed - 45) / 35, 0, 1) * (st.nitroActive ? 1 : 0.6);

    const z = this.track.zone[this.track.idx(st.s)];
    if (z !== this.lastZone) {
      this.lastZone = z;
      const zone = this.track.zones[z];
      this.set('zone', e.zone, zone.name);
      // (On a circuit, only the first time round.)
      if (st.started && !(st.laps && (st.laps.lap > 1 || st.laps.time == null))) this.zoneCard(zone);
    }

    if (st.laps) {
      const l = st.laps;
      this.set('lapN', e.lapN, `LAP ${l.lap}/${l.of}`);
      this.set('lapTime', e.lapTime, l.time == null ? '' : fmtTime(l.time));
      this.set('lapBest', e.lapBest, l.best == null ? '' : `BEST LAP ${fmtTime(l.best)}`);
    }

    // Route dots: along the route, or round the lap on a circuit.
    const L = this.track.laps ? this.track.n : this.track.finishS;
    st.racers.forEach((r, i) => {
      const d = this.dots[i];
      if (d) d.style.left = (clamp(r.s / L, 0, 1) * 100).toFixed(2) + '%';
    });

    if (st.cruise) {
      const c = this.cruiseEl, cr = st.cruise;
      this.set('score', c.score, Math.floor(cr.score).toLocaleString());
      this.set('mult', c.mult, `×${cr.mult}`);
      c.fill.style.width = (cr.mult > 1 ? clamp(cr.multTimer / 6, 0, 1) * 100 : 0).toFixed(1) + '%';
      const km = cr.dist / 1000;
      this.set('dist', c.dist, this.mph ? `${(km / 1.60934).toFixed(1)} mi` : `${km.toFixed(1)} km`);
      this.set('best', c.best, Math.floor(Math.max(this.bestScore, cr.score)).toLocaleString());
    }
    if (this.toastTimer > 0) { this.toastTimer -= dt; if (this.toastTimer <= 0) e.toast.classList.remove('show'); }
    if (this.radioTimer > 0) { this.radioTimer -= dt; if (this.radioTimer <= 0) this.pz.radio.classList.remove('show'); }
    this.clock += dt;
    this.setPursuit(!!st.pursuit);
    if (st.pursuit) this.updatePursuit(st.pursuit);
    if (st.electric) this.drawPower(st.power ?? 0);
    else this.drawTach(st.rpm, st.speed);
    this.drawMinimap(st);
  }

  updatePursuit(pu) {
    const p = this.pz;
    const flash = pu.flash !== false;
    p.root.classList.toggle('flash', flash);
    p.root.classList.toggle('patrol', pu.state === 'patrol');
    // Stars: whole ones up to the heat, the next one filling with the meter.
    const heat = clamp(Math.floor(pu.heat || 1), 1, 5);
    this.starFill.forEach((f, i) => {
      const v = i < heat ? 1 : i === heat ? clamp(pu.heatMeter || 0, 0, 1) : 0;
      f.style.width = (v * 100).toFixed(1) + '%';
    });
    p.stars.classList.toggle('max', heat === 5);
    // Bar: BUST while it's filling, EVADE in cooldown, nothing in patrol.
    const bust = pu.bust > 0, evade = !bust && pu.state === 'cooldown';
    p.bar.classList.toggle('hidden', !bust && !evade);
    p.bar.classList.toggle('bust', bust);
    p.bar.classList.toggle('evade', evade);
    if (bust || evade) {
      this.set('pzLabel', p.label, bust ? 'BUST' : 'EVADE');
      p.fill.style.width = (clamp(bust ? pu.bust : pu.evade || 0, 0, 1) * 100).toFixed(1) + '%';
    }
    // Damage: green → amber → red, pulsing past 75 %.
    const d = clamp(pu.damage || 0, 0, 1);
    p.dmgFill.style.width = (d * 100).toFixed(1) + '%';
    p.dmgFill.style.background = `hsl(${Math.round(125 * (1 - d))}, 100%, 55%)`;
    p.dmg.classList.toggle('crit', d > 0.75);
    p.dmg.classList.toggle('flash', flash);
    // Penalty served, under the race clock.
    const pen = pu.penalties || 0;
    p.pen.classList.toggle('hidden', !(pen > 0));
    if (pen > 0) this.set('pen', p.pen, `+${pen.toFixed(1)} s`);
    // The hold card while the car is held for a penalty.
    const held = pu.hold > 0;
    p.hold.classList.toggle('hidden', !held);
    if (held) {
      const wrecked = pu.holdReason === 'wrecked';
      p.hold.classList.toggle('wrecked', wrecked);
      this.set('holdTitle', p.holdTitle, wrecked ? 'WRECKED' : 'BUSTED');
      this.set('holdSub', p.holdSub, `+${pu.hold.toFixed(1)} s PENALTY`);
      p.holdFill.style.width = (clamp(pu.hold / (pu.holdTotal || pu.hold), 0, 1) * 100).toFixed(1) + '%';
    }
  }

  drawTach(rpm, speed) {
    const g = this.tach, W = 260, cx = 130, cy = 130, R = 112;
    g.clearRect(0, 0, W, W);
    const a0 = Math.PI * 0.75, a1 = Math.PI * 2.25;
    const maxR = 8000;
    // Backplate.
    g.beginPath(); g.arc(cx, cy, R + 10, 0, Math.PI * 2);
    g.fillStyle = 'rgba(8,10,18,0.5)'; g.fill();
    // Track.
    g.lineWidth = 10; g.lineCap = 'butt';
    g.beginPath(); g.arc(cx, cy, R, a0, a1); g.strokeStyle = 'rgba(255,255,255,0.12)'; g.stroke();
    // Redline zone.
    g.beginPath(); g.arc(cx, cy, R, a0 + (a1 - a0) * (7000 / maxR), a1); g.strokeStyle = 'rgba(255,56,96,0.55)'; g.stroke();
    // Fill.
    const f = clamp(rpm / maxR, 0, 1);
    const grad = g.createLinearGradient(0, W, W, 0);
    grad.addColorStop(0, '#3ad7ff'); grad.addColorStop(0.7, '#b36bff'); grad.addColorStop(1, '#ff3860');
    g.beginPath(); g.arc(cx, cy, R, a0, a0 + (a1 - a0) * f); g.strokeStyle = grad; g.lineWidth = 10; g.stroke();
    // Ticks.
    g.fillStyle = 'rgba(255,255,255,0.7)'; g.font = '600 13px Rajdhani, Arial Narrow, sans-serif'; g.textAlign = 'center'; g.textBaseline = 'middle';
    for (let k = 0; k <= 8; k++) {
      const a = a0 + (a1 - a0) * (k / 8);
      g.fillText(String(k), cx + Math.cos(a) * (R - 22), cy + Math.sin(a) * (R - 22));
      g.beginPath();
      g.moveTo(cx + Math.cos(a) * (R - 9), cy + Math.sin(a) * (R - 9));
      g.lineTo(cx + Math.cos(a) * (R - 14), cy + Math.sin(a) * (R - 14));
      g.strokeStyle = 'rgba(255,255,255,0.5)'; g.lineWidth = 2; g.stroke();
    }
    // Needle.
    const na = a0 + (a1 - a0) * f;
    g.beginPath(); g.moveTo(cx + Math.cos(na) * 30, cy + Math.sin(na) * 30); g.lineTo(cx + Math.cos(na) * (R - 4), cy + Math.sin(na) * (R - 4));
    g.strokeStyle = '#ff3860'; g.lineWidth = 3; g.stroke();
  }

  // Electric: a power meter in place of the rev counter — regen in green
  // below zero, drive power up to 1000 kW.
  drawPower(kw) {
    const g = this.tach, W = 260, cx = 130, cy = 130, R = 112;
    g.clearRect(0, 0, W, W);
    const a0 = Math.PI * 0.75, a1 = Math.PI * 2.25;
    const lo = -250, hi = 1000;
    const at = (v) => a0 + (a1 - a0) * ((clamp(v, lo, hi) - lo) / (hi - lo));
    g.beginPath(); g.arc(cx, cy, R + 10, 0, Math.PI * 2);
    g.fillStyle = 'rgba(8,10,18,0.5)'; g.fill();
    g.lineWidth = 10; g.lineCap = 'butt';
    g.beginPath(); g.arc(cx, cy, R, a0, a1); g.strokeStyle = 'rgba(255,255,255,0.12)'; g.stroke();
    g.beginPath(); g.arc(cx, cy, R, a0, at(0)); g.strokeStyle = 'rgba(77,255,138,0.28)'; g.stroke();
    if (kw >= 0) {
      const grad = g.createLinearGradient(0, W, W, 0);
      grad.addColorStop(0, '#3ad7ff'); grad.addColorStop(0.75, '#9ff3ff'); grad.addColorStop(1, '#ffffff');
      g.beginPath(); g.arc(cx, cy, R, at(0), at(kw)); g.strokeStyle = grad; g.stroke();
    } else {
      g.beginPath(); g.arc(cx, cy, R, at(kw), at(0)); g.strokeStyle = '#4dff8a'; g.stroke();
    }
    g.fillStyle = 'rgba(255,255,255,0.7)'; g.font = '600 13px Rajdhani, Arial Narrow, sans-serif'; g.textAlign = 'center'; g.textBaseline = 'middle';
    for (let v = 0; v <= hi; v += 200) {
      const a = at(v);
      g.fillText(String(v / 100), cx + Math.cos(a) * (R - 22), cy + Math.sin(a) * (R - 22));
      g.beginPath();
      g.moveTo(cx + Math.cos(a) * (R - 9), cy + Math.sin(a) * (R - 9));
      g.lineTo(cx + Math.cos(a) * (R - 14), cy + Math.sin(a) * (R - 14));
      g.strokeStyle = 'rgba(255,255,255,0.5)'; g.lineWidth = 2; g.stroke();
    }
    g.font = '700 10px Rajdhani, Arial Narrow, sans-serif';
    g.fillStyle = 'rgba(77,255,138,0.85)';
    g.fillText('REGEN', cx + Math.cos(a0) * (R - 26) + 10, cy + Math.sin(a0) * (R - 26) + 4);
    g.fillStyle = 'rgba(255,255,255,0.55)';
    g.fillText('kW ×100', cx + Math.cos(a1) * (R - 30) - 10, cy + Math.sin(a1) * (R - 30) + 4);
    const na = at(kw);
    g.beginPath(); g.moveTo(cx + Math.cos(na) * 30, cy + Math.sin(na) * 30); g.lineTo(cx + Math.cos(na) * (R - 4), cy + Math.sin(na) * (R - 4));
    g.strokeStyle = kw < 0 ? '#4dff8a' : '#3ad7ff'; g.lineWidth = 3; g.stroke();
  }

  drawMinimap(st) {
    const g = this.mini, S = 220, c = S / 2;
    const t = this.track;
    g.clearRect(0, 0, S, S);
    g.save();
    g.beginPath(); g.arc(c, c, c - 2, 0, Math.PI * 2); g.clip();
    const scale = 0.28; // px per metre
    const p = st.player;
    g.translate(c, c + 30);
    g.rotate(-p.yaw - Math.PI / 2);
    g.scale(scale, scale);
    g.translate(-p.x, -p.z);
    // Road near the player.
    const s0 = t.loop ? Math.floor(st.s - 500) : Math.max(0, Math.floor(st.s - 500));
    const s1 = t.loop ? Math.ceil(st.s + 900) : Math.min(t.n - 1, Math.ceil(st.s + 900));
    g.lineCap = 'round'; g.lineJoin = 'round';
    for (const [w, col] of [[26, 'rgba(0,0,0,0.5)'], [14, 'rgba(230,236,255,0.85)']]) {
      g.beginPath();
      for (let i = s0; i <= s1; i += 6) { const k = t.idx(i); if (i === s0) g.moveTo(t.px[k], t.pz[k]); else g.lineTo(t.px[k], t.pz[k]); }
      // The straight runout past the last sample.
      if (!t.loop && t.runout > 0 && st.s + 900 > t.n - 1) {
        const a = t.frame(Math.max(s0, t.n - 1)), e = t.frame(Math.min(t.roadEnd, st.s + 900));
        if (s0 > s1) g.moveTo(a.x, a.z);
        g.lineTo(e.x, e.z);
      }
      g.lineWidth = w; g.strokeStyle = col; g.stroke();
    }
    // Finish marker.
    if (!t.loop) {
      const fi = t.idx(t.finishS);
      g.fillStyle = '#4dff8a';
      g.beginPath(); g.arc(t.px[fi], t.pz[fi], 22, 0, Math.PI * 2); g.fill();
    }
    // Traffic.
    g.fillStyle = 'rgba(180,190,210,0.8)';
    for (const o of st.traffic) { g.beginPath(); g.arc(o.x, o.z, 10, 0, Math.PI * 2); g.fill(); }
    // Rivals.
    for (const r of st.racersFull) {
      if (r.player) continue;
      g.fillStyle = '#' + r.color.toString(16).padStart(6, '0');
      g.beginPath(); g.arc(r.v.x, r.v.z, 16, 0, Math.PI * 2); g.fill();
      g.lineWidth = 4; g.strokeStyle = '#000'; g.stroke();
    }
    if (st.pursuit) this.drawPolice(g, st.pursuit);
    g.restore();
    // Player arrow (screen space, always pointing up).
    g.save();
    g.translate(c, c + 30);
    g.fillStyle = '#fff'; g.strokeStyle = '#000'; g.lineWidth = 2;
    g.beginPath(); g.moveTo(0, -9); g.lineTo(6.5, 7); g.lineTo(0, 3.5); g.lineTo(-6.5, 7); g.closePath(); g.fill(); g.stroke();
    g.restore();
  }

  // Police on the minimap (world space, called inside drawMinimap's
  // transform): roadblocks as red bars across the road, spikes as thin
  // amber ones, units as red/blue dots that blink unless flashing is off.
  drawPolice(g, pu) {
    g.lineCap = 'butt';
    // Drawn wider and thicker than life so they read on a 190 px map.
    for (const [list, w, col] of [[pu.roadblocks, 18, '#ff3040'], [pu.spikes, 9, '#ffb43c']]) {
      if (!list) continue;
      for (const b of list) {
        const half = Math.max(b.width, 36) / 2, ax = -Math.sin(b.yaw) * half, az = Math.cos(b.yaw) * half;
        g.beginPath(); g.moveTo(b.x - ax, b.z - az); g.lineTo(b.x + ax, b.z + az);
        g.lineWidth = w + 6; g.strokeStyle = '#000'; g.stroke();
        g.lineWidth = w; g.strokeStyle = col; g.stroke();
      }
    }
    const phase = pu.flash !== false ? Math.floor(this.clock * 4) % 2 : 0;
    (pu.units || []).forEach((u, i) => {
      g.beginPath(); g.arc(u.x, u.z, 16, 0, Math.PI * 2);
      if (u.disabled) { g.fillStyle = 'rgba(120,126,140,0.6)'; g.fill(); return; }
      const red = (i + phase) % 2 === 0;
      g.fillStyle = red ? '#ff3040' : '#2f6bff'; g.fill();
      g.lineWidth = 5; g.strokeStyle = pu.flash !== false ? '#000' : red ? '#2f6bff' : '#ff3040'; g.stroke();
    });
  }
}
