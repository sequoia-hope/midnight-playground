// The e2e suites against the Rust build (SPEC 8.5, roadmap WP 6.7): what
// harness.js needs for `target: 'rust'`.
//
// The suites drive the JS game through its globals (`__race`, `__game`,
// `__audio`, `__world`, `__camera`, `__stats`, `__pads`) and its DOM
// (`document.getElementById('menu').classList.contains('hidden')`, a
// select's `value` and `change` event, a pad's bounding box). The Rust
// build draws everything into one canvas and reports through
// `window.__mr` (`screen`, `mode`, `ui(id)`, `race`, `audio`, `hud`,
// `settings`, `stage(cmd)`, ...). `installBridge` runs in the page before
// its scripts and puts the JS game's globals over `__mr`, so the suites run
// unchanged:
//
// - `__race` and friends are views built from the last frame's `__mr`
//   snapshots, once per `game.eval`. A write to a view (`r.player.vx = 3`,
//   `r.phys.reset(s, 0)`, `a.writePos()`, `el.value = v` and its `change`)
//   becomes a `__mr.stage` command, sent in order when the eval ends; the
//   harness then waits for the frame that applied them (`__mr.staged`)
//   before the next read, as the JS game applied them at once.
// - While a `game.eval` runs (and only then: the page's own scripts never
//   see them), `document.getElementById`, `querySelector(All)` and
//   `getComputedStyle` answer the selectors the suites use with stand-ins
//   backed by `__mr.uiNodes` (the canvas UI's controls, by their DOM ids).
//
// Hot Pursuit (M8): `__pursuit` and `__race.pv` over `__mr.pursuit`, their
// writes and calls as staging commands (`unit`, `hurt`, `say`, `set
// pursuit.*`, `set pv.*`). What has no Rust counterpart reads as absent:
// `__world.renderer` and the three.js objects. The list, with what each
// JS read maps to, is in DECISIONS (WP 6.7, WP 8.3).

// The level tabs and car picks in the menu's order (src/levels/index.js,
// CarPhysics' CAR_SPECS): `:nth-child(n)` selectors name them by position.
export const LEVEL_IDS = ['sierra', 'coast', 'streets', 'desert', 'seaside', 'cruise'];
export const CAR_IDS = ['sports', 'muscle', 'super', 'rally', 'electric'];

// The Rust control id for a JS selector, or null when it is not a control.
// Self-contained: it is also injected into the page.
export function selectorId(sel) {
  const L = ['sierra', 'coast', 'streets', 'desert', 'seaside', 'cruise'];
  const C = ['sports', 'muscle', 'super', 'rally', 'electric'];
  const s = String(sel).trim();
  let m;
  if ((m = s.match(/^(?:#level-pick )?\.lvl-tab:nth-child\((\d+)\)$/))) return 'lvl-tab-' + L[Number(m[1]) - 1];
  if (/^(?:#level-pick )?\.lvl-tab:first-child$/.test(s)) return 'lvl-tab-' + L[0];
  if (/^(?:#level-pick )?\.lvl-tab:last-child$/.test(s)) return 'lvl-tab-' + L[L.length - 1];
  if ((m = s.match(/^(?:#car-pick )?\.pick:nth-child\((\d+)\)$/))) return 'pick-' + C[Number(m[1]) - 1];
  if ((m = s.match(/^#touch \[data-(?:act|tap)="(\w+)"\](?:, #touch \[data-(?:act|tap)="\w+"\])?$/))) return 'touch-' + m[1];
  const touchParts = { '.t-stick': 'stick', '.t-slider': 'slider', '.t-drift-strip': 'drift', '.t-pedal': 'pedal', '.t-wheel': 'wheel' };
  if ((m = s.match(/^#touch (\.t-[a-z-]+)$/)) && touchParts[m[1]]) return 'touch-' + touchParts[m[1]];
  if ((m = s.match(/^#mode-pick \[data-mode="?(\w+)"?\]$/))) return 'mode-' + m[1];
  if ((m = s.match(/^\.pad-bind\[data-act="?(\w+)"?\]$/))) return 'pad-bind-' + m[1];
  if (s === '#pause .title') return 'pause-title';
  if ((m = s.match(/^(?:#(?:menu|pause) )?\.(vol-music|vol-sfx)$/))) return m[1];
  if (s === '#menu .controls') return 'menu-controls';
  if (s === '#menu .touch-help') return 'touch-help';
  if ((m = s.match(/^#([\w-]+)$/))) return m[1];
  return null;
}

// Runs in the page (puppeteer's evaluateOnNewDocument) before its scripts.
// `selectorSrc` is selectorId's source.
export function installBridge(selectorSrc) {
  // Only on the Rust build's page (a link may lead to a JS page: music.html).
  if (!/\/dist\/next\//.test(location.pathname)) return;
  // eslint-disable-next-line no-new-func
  const selectorId = new Function('return (' + selectorSrc + ')')();
  const LEVELS = ['sierra', 'coast', 'streets', 'desert', 'seaside', 'cruise'];
  const CARS = ['sports', 'muscle', 'super', 'rally', 'electric'];
  // Gamepad.js ACTIONS: the Controller screen's row labels.
  const ACTION_LABELS = {
    left: 'Steer left', right: 'Steer right', throttle: 'Throttle', brake: 'Brake / reverse', nitro: 'Nitro',
    handbrake: 'Handbrake', lookBack: 'Look back', camera: 'Camera', reset: 'Reset car', pause: 'Pause',
  };
  // The label a highlighted row starts with (`.pad-focus` is the row).
  const ROW_LABELS = { 'opt-track': 'Track', 'vol-music': 'Music', 'vol-sfx': 'SFX', 'opt-steer': 'Steering', 'opt-pedals': 'Pedals', 'opt-tilt-sens': 'Tilt' };
  // A setting each option shows (`__mr.settings`), for when it is not on screen.
  const SETTING = {
    'opt-mph': 'mph', 'opt-hq': 'hq', 'opt-flash': 'flash', 'opt-autogas': 'autogas', 'opt-fullscreen': 'fullscreen', 'opt-rumble': 'rumble',
    'opt-track': 'track', 'opt-steer': 'steering', 'opt-pedals': 'pedals',
  };
  const SLIDERS = { 'vol-music': 'music', 'vol-sfx': 'sfx', 'opt-tilt-sens': 'tiltSens' };
  // TouchControls' SLIDER bands, for the panel's `--gas` and `--brk`.
  const GAS_BOTTOM = 0.36, BRAKE_TOP = 0.3;

  const M = () => window.__mr || {};
  const U = (id) => (M().uiNodes || {})[id] || null;
  const shownU = (u) => !!u && !!u.visible && u.w > 0 && u.h > 0;
  const pct = (f) => (f * 100).toFixed(2) + '%';
  // A length as the CSSOM serialises it back (`62.0px` reads `62px`).
  const cssNum = (v) => String(+(+v).toFixed(6));

  const S = {
    depth: 0,
    sent: 0,
    queue: [],
    view: undefined,
    pview: undefined,
    tag: null,
    fps: null,
    // A stage command, sent when the eval ends (or at once outside one).
    send(cmd) {
      S.queue.push(cmd);
      if (S.depth === 0) queueMicrotask(S.flush);
    },
    flush() {
      const q = S.queue.splice(0);
      for (const c of q) {
        if (!M().stage) throw new Error('the Rust build has no __mr.stage');
        M().stage(c);
        S.sent++;
      }
    },
    run(thunk) {
      if (S.depth++ === 0) { S.view = undefined; S.pview = undefined; swap(true); syncBody(); }
      let r;
      try {
        r = thunk();
      } finally {
        if (--S.depth === 0) { swap(false); S.view = undefined; S.pview = undefined; S.flush(); }
      }
      const out = (v) => ({ v, wait: S.sent > (M().staged ?? 0) ? S.sent : 0 });
      return r && typeof r.then === 'function' ? Promise.resolve(r).then(out) : out(r);
    },
  };
  window.__mrShim = S;

  // `body.touch` and `body.pad`, which the suites read off the real body.
  function syncBody() {
    const b = document.body;
    if (!b) return;
    b.classList.toggle('touch', !!M().touchUi);
    b.classList.toggle('pad', !!M().pads?.connected);
  }

  // ── The race ───────────────────────────────────────────────────────

  // An object whose fields read from `vals` and whose writes send `set`.
  function fields(vals, prefix) {
    const raw = { ...vals };
    const o = {};
    for (const k of Object.keys(raw)) {
      Object.defineProperty(o, k, {
        enumerable: true, configurable: true,
        get: () => raw[k],
        set: (x) => { raw[k] = x; S.send({ cmd: 'set', path: prefix + k, value: x }); },
      });
    }
    Object.defineProperty(o, '_raw', { value: raw });
    return o;
  }

  function trackView(t) {
    return {
      startS: t.startS, finishS: t.finishS, n: t.n, length: t.length, loop: t.loop, roadEnd: t.roadEnd,
      frame(s) {
        const a = M().trackFrame(s);
        return { x: a[0], y: a[1], z: a[2], fx: a[3], fz: a[4], rx: a[5], rz: a[6], hw: a[7], s };
      },
      wrap: (s) => M().trackWrap(s),
    };
  }

  function touchView(t) {
    const v = fields({ autoGas: t.autoGas }, 'touch.');
    Object.assign(v, {
      stickR: t.stickR, stick: t.stick ? { ...t.stick } : null, steering: t.steering, mode: t.mode,
      held: { ...t.held }, tilt: t.tilt ? { ...t.tilt } : { state: 'off' },
    });
    return v;
  }

  function raceView() {
    // Back on the menu the JS keeps its last Race in `window.__race`.
    if (M().race) S.last = M().race;
    const o = M().race || S.last;
    if (!o) return undefined;
    const track = trackView(o.track || {});
    const player = fields({
      x: o.x, y: o.y, z: o.z, vx: o.vx, vz: o.vz, yaw: o.yaw, s: o.s, lat: o.lat,
      speed: o.along, steerAngle: o.steerAngle, prog: o.prog,
    }, 'player.');
    const phys = fields({
      locked: o.locked, gear: o.gear, nitro: o.nitro, nitroActive: o.nitroActive, skid: o.skid, damage: o.damage,
    }, 'phys.');
    phys.reset = (s, lat = 0) => {
      S.send({ cmd: 'reset', s, lat });
      // As CarPhysics.reset leaves the car: on the road there, at rest,
      // pointing along it.
      const f = track.frame(s);
      Object.assign(player._raw, {
        s, lat, x: f.x + f.rx * lat, z: f.z + f.rz * lat, vx: 0, vz: 0, yaw: Math.atan2(f.fz, f.fx), speed: 0,
      });
    };
    phys.events = { push: (...es) => { for (const e of es) S.send({ cmd: 'event', ...e }); return es.length; } };
    const ais = (o.ais || []).map((a, i) => {
      const ai = fields({ s: a.s, lat: a.lat, speed: a.speed, prog: a.prog, finished: a.finished, finishTime: a.finishTime }, `ais.${i}.`);
      ai.v = { x: a.x, y: a.y, z: a.z, s: a.s, lat: a.lat, vx: a.vx, vz: a.vz, yaw: a.yaw };
      ai.writePos = () => S.send({ cmd: 'aiWritePos', i });
      // A rival is a body a police unit can chase (`u.target = ai`).
      Object.defineProperty(ai, '__body', { value: 'rival:' + i });
      return ai;
    });
    const r = fields({
      lastS: o.lastS, odo: o.odo, score: o.score, nearMisses: o.nearMisses,
    }, '');
    Object.assign(r, {
      state: o.state, time: o.time, countdown: o.countdown, cruise: o.cruise, lap: o.lap, lapTimes: o.lapTimes,
      playerFinished: o.playerFinished, playerTime: o.playerTime, dist: o.dist, pursuitOn: o.pursuitOn,
      // Hot Pursuit's PursuitView (null in a race without it).
      pv: pvView(),
      playerBody: PLAYER_BODY,
      traffic: o.traffic ? {} : null,
      track, player, phys, ais,
      cam: fields({ mode: o.camMode, snap: false }, 'cam.'),
      input: { state: { ...o.input }, touch: M().touchUi && o.touch ? touchView(o.touch) : null },
      progS: {
        set(obj, s) {
          const i = obj === player ? 0 : ais.indexOf(obj) + 1;
          if (i >= 0) S.send({ cmd: 'set', path: 'progS.' + i, value: s });
        },
      },
      standings() {
        const n = o.racers || 1;
        return Array.from({ length: n }, (_, i) => ({ player: i === (o.place || 1) - 1 }));
      },
    });
    // flow-helpers' markRace: a race started since is a new Race.
    Object.defineProperty(r, '__tagged', {
      configurable: true,
      get: () => S.tag !== null && (M().races ?? 0) <= S.tag,
      set: (on) => { S.tag = on ? (M().races ?? 0) : null; },
    });
    return r;
  }

  function view() {
    if (S.depth === 0) return raceView();
    if (S.view === undefined) S.view = raceView();
    return S.view;
  }

  // ── Hot Pursuit ────────────────────────────────────────────────────

  // `race.playerBody`: the player's body, as a unit's target.
  const PLAYER_BODY = Object.freeze({ __body: 'player' });

  // `race.pv`: the PursuitView's counts and its `say` and `hurt`.
  function pvView() {
    const p = M().pursuit;
    if (!p || !p.available) return null;
    const pv = fields({ penalty: p.pv.penalty, damage: p.pv.damage }, 'pv.');
    Object.assign(pv, {
      wrecks: p.pv.wrecks,
      flash: p.flash,
      held: (p.player?.hold ?? 0) > 0,
      // `say(line, force)`: a RADIO line ({text, parts}) through the
      // client's radio, rate limit and all.
      say(line, force = false) {
        S.send({ cmd: 'say', text: String(line.text), parts: (line.parts || [line.text]).map(String), force: !!force });
      },
      hurt(d) { S.send({ cmd: 'hurt', d: Number(d) }); },
    });
    return pv;
  }

  // `window.__pursuit`: the race's Pursuit (undefined without one).
  function pursuitView() {
    const p = M().pursuit;
    if (!p || !p.available) return undefined;
    const units = (p.units || []).map((u, i) => {
      const o = fields({ speed: u.speed, s: u.s, lat: u.lat }, `pursuit.units.${i}.`);
      Object.assign(o, {
        i, active: u.active, mode: u.mode, siren: u.siren, callsign: u.callsign, type: u.type, health: u.health,
        v: { x: u.x, z: u.z, model: { root: { visible: !!u.visible } } },
      });
      let target = u.target;
      Object.defineProperty(o, 'target', {
        enumerable: true,
        get: () => target,
        set: (b) => { target = b; S.send({ cmd: 'set', path: `pursuit.units.${i}.target`, value: b?.__body ?? null }); },
      });
      return o;
    });
    const v = fields({ state: p.state }, 'pursuit.');
    Object.assign(v, {
      heat: p.heat, maxHeat: p.maxHeat, heatMeter: p.heatMeter, bust: p.bust, evade: p.evade,
      busts: p.busts, takedowns: p.takedowns, flash: p.flash, maxUnits: p.maxUnits,
      units, player: p.player ? { ...p.player } : null,
      // `activate(u, s, lat, speed, mode, dir)`.
      activate(u, s, lat, speed, mode, dir = 1) {
        S.send({ cmd: 'unit', i: u.i, s, lat, speed, mode, dir });
        Object.assign(u._raw, { s, lat, speed });
        u.active = true;
        u.mode = mode;
      },
      placeRoadblock(s) { S.send({ cmd: 'roadblock', s }); },
      placeSpikes(s) { S.send({ cmd: 'spikes', s }); },
    });
    return v;
  }

  function pview() {
    if (S.depth === 0) return pursuitView();
    if (S.pview === undefined) S.pview = pursuitView();
    return S.pview;
  }

  function audioView() {
    const a = M().audio;
    const ctx = a && a.context && a.context !== 'none' ? {
      state: a.context,
      suspend: () => { S.send({ cmd: 'audio', op: 'suspend' }); return Promise.resolve(); },
      resume: () => { S.send({ cmd: 'audio', op: 'resume' }); return Promise.resolve(); },
    } : undefined;
    return {
      ctx,
      ready: !!a?.ready,
      _musicOn: !!a?.musicOn,
      musicGate: { gain: { value: a?.musicGate ?? 0 } },
      _vol: { master: a?.vol?.master ?? 1, music: a?.vol?.music ?? 0.7, sfx: a?.vol?.sfx ?? 0.85 },
      trackInfo: a?.playing ? { id: a.playing } : null,
      // The radio transmission on the air (or the last one): `{ srcs }`,
      // the browser's own source nodes (play/audio.rs, WP 8.4).
      _radioCur: a?.radioCur ?? null,
    };
  }

  const define = (name, get) => Object.defineProperty(window, name, { configurable: true, get });
  define('__race', view);
  define('__game', () => ({ get mode() { return M().mode; } }));
  define('__audio', audioView);
  define('__ready', () => M().ready === true && M().screen !== 'loading' && !!M().uiNodes);
  define('__pads', () => M().pads);
  define('__camera', () => {
    const r = M().race;
    const right = r?.camRight || { x: 1, y: 0, z: 0 };
    const p = r?.camPos || { x: 0, y: 0, z: 0 };
    return { matrixWorld: { elements: [right.x, right.y ?? 0, right.z, 0] }, position: { ...p } };
  });
  define('__world', () => {
    const lamps = M().lamps || { n: 0, lit: 0 };
    return {
      level: { id: M().level },
      scenery: [{ lampMats: Array.from({ length: lamps.n }, (_, i) => ({ emissiveIntensity: i < lamps.lit ? 6 : 0 })) }],
    };
  });
  // Frames per second from the client's frame count, between two reads.
  define('__stats', () => {
    const now = performance.now(), f = M().frames ?? 0;
    const prev = S.fps;
    if (!prev || now - prev.t > 400) S.fps = { t: now, f, fps: prev && now > prev.t ? +(1000 * (f - prev.f) / (now - prev.t)).toFixed(1) : prev?.fps };
    return { fps: S.fps.fps };
  });
  define('__pursuit', pview);

  // ── The DOM stand-ins ──────────────────────────────────────────────

  const NONE = { left: 0, top: 0, right: 0, bottom: 0, width: 0, height: 0, x: 0, y: 0 };
  const styleOf = (props = {}) => ({ ...props, getPropertyValue: (k) => props[k] ?? '' });

  class El {
    constructor(spec) { this.spec = spec; this.pending = undefined; }
    get u() { return this.spec.uid ? U(this.spec.uid) : null; }
    get id() { return this.spec.domId ?? this.spec.uid ?? ''; }
    visible() { return this.spec.visible ? !!this.spec.visible() : shownU(this.u); }
    get classList() {
      const has = (c) => {
        if (this.spec.cls) { const v = this.spec.cls(c); if (v !== undefined) return v; }
        if (c === 'hidden') return !this.visible();
        if (c === 'sel') return !!this.u?.sel;
        return false;
      };
      const list = () => (this.spec.classes ? this.spec.classes() : []);
      return { contains: has, [Symbol.iterator]: () => list()[Symbol.iterator]() };
    }
    getClientRects() { return this.visible() ? [this.getBoundingClientRect()] : []; }
    getBoundingClientRect() {
      const u = this.u;
      if (!u || !this.visible()) return { ...NONE };
      return { left: u.x, top: u.y, width: u.w, height: u.h, right: u.x + u.w, bottom: u.y + u.h, x: u.x, y: u.y };
    }
    get textContent() {
      if (this.spec.text) return this.spec.text() ?? '';
      const v = this.u?.value;
      return typeof v === 'string' ? v : '';
    }
    get checked() {
      const v = this.u?.value;
      if (typeof v === 'boolean') return v;
      const k = SETTING[this.spec.uid];
      return !!(k && M().settings?.[k]);
    }
    get value() {
      if (this.pending !== undefined) return this.pending;
      const id = this.spec.uid;
      const v = this.u?.value;
      if (SLIDERS[id]) {
        if (typeof v === 'number') return String(v);
        const x = M().settings?.[SLIDERS[id]];
        return x === undefined ? '' : String(Math.round(x * 100));
      }
      if (typeof v === 'string') return v;
      const k = SETTING[id];
      return k && M().settings?.[k] !== undefined ? String(M().settings[k]) : '';
    }
    set value(v) { this.pending = String(v); }
    get options() { return (M().selects?.[this.spec.uid] || []).map((value) => ({ value })); }
    get dataset() { return this.spec.dataset ? this.spec.dataset() : {}; }
    get style() { return styleOf(this.spec.style ? this.spec.style() : {}); }
    get rowIndex() { return this.spec.rowIndex ?? -1; }
    get cells() { return this.spec.cells ? this.spec.cells() : []; }
    closest() { return this; }
    contains(o) { return o === this; }
    querySelector(sel) { return this.spec.child ? this.spec.child(sel) : null; }
    focus() { S.send({ cmd: 'focus', id: this.spec.uid }); }
    click() { S.send({ cmd: 'act', id: this.spec.uid }); }
    dispatchEvent(e) {
      const id = this.spec.uid, type = e && e.type;
      if (type === 'change' && this.pending !== undefined && !SLIDERS[id]) S.send({ cmd: 'choose', id, value: this.pending });
      if ((type === 'input' || type === 'change') && this.pending !== undefined && SLIDERS[id]) S.send({ cmd: 'slide', id, value: Number(this.pending) });
      return true;
    }
  }

  const hud = () => M().hud || {};
  const touch = () => M().race?.touch || {};
  const pad = () => M().padsetup || {};
  const tiles = () => Object.entries(M().uiNodes || {}).filter(([k]) => /^res-stat-\d+$/.test(k))
    .sort((a, b) => Number(a[0].slice(9)) - Number(b[0].slice(9))).map(([, u]) => String(u.value || '').split('|'));
  const rows = () => {
    const n = Number(U('res-table')?.value) || 0;
    return Array.from({ length: n }, (_, i) => {
      const u = U('res-row-' + i);
      const [place = '', name = '', value = ''] = String(u?.value || '').split('|');
      return new El({
        uid: 'res-row-' + i, domId: '', rowIndex: i, visible: () => shownU(U('res-table')),
        cls: (c) => (c === 'me' ? !!u?.sel : undefined),
        cells: () => [place, name, value].map((textContent) => ({ textContent })),
        text: () => place + name + value,
      });
    });
  };
  const padRow = (act) => new El({
    uid: 'pad-bind-' + act, domId: '',
    cls: (c) => (c === 'on' ? (pad().on || []).includes(act) : c === 'listening' ? pad().listening === act : undefined),
    text: () => (ACTION_LABELS[act] || act) + (pad().binds?.[act] ?? ''),
    child: (sel) => (sel === 'b' ? new El({ text: () => pad().binds?.[act] ?? '' }) : null),
  });
  // The highlighted control (`.pad-focus`), as the JS's row reads.
  const focusEl = (id) => {
    const bind = id.match(/^pad-bind-(\w+)$/);
    if (bind) return padFocus(padRow(bind[1]), id);
    return padFocus(new El({
      uid: id,
      text: () => {
        if (ROW_LABELS[id]) return ROW_LABELS[id] + ' ' + new El({ uid: id }).value;
        const v = U(id)?.value;
        return typeof v === 'string' ? v : '';
      },
    }), id);
  };
  const padFocus = (el, id) => {
    const cls = el.spec.cls;
    el.spec.cls = (c) => (c === 'pad-edit' ? !!M().padNav?.editing : c === 'pad-focus' ? true : cls ? cls(c) : undefined);
    el.spec.domId = id;
    return el;
  };

  // By the JS game's element id.
  function byId(id) {
    switch (id) {
      case 'loading': case 'menu': case 'pause': case 'results': case 'padsetup':
        return new El({ domId: id, visible: () => M().screen === id });
      case 'hud':
        return new El({ domId: id, visible: () => !!hud().shown });
      case 'touch':
        return new El({ domId: id, visible: () => !!(M().touchUi && touch().visible && M().screen === 'none') });
      case 'tilt-sens-row':
        return new El({ domId: id, visible: () => shownU(U('opt-tilt-sens')) });
      case 'hud-lap':
        return new El({ domId: id, visible: () => !!(hud().shown && hud().laps) });
      case 'hud-lap-n': return new El({ domId: id, text: () => hud().texts?.lapN ?? '' });
      case 'hud-lap-best': return new El({ domId: id, text: () => hud().texts?.lapBest ?? '' });
      // Hot Pursuit's furniture (`__mr.hud`: each shows as its `.hidden`
      // says, inside a shown HUD).
      case 'hud-pen': return new El({ domId: id, visible: () => !!(hud().shown && hud().pen), text: () => hud().texts?.pen ?? '' });
      case 'hud-pz': case 'pz-stars': return new El({ domId: id, visible: () => !!(hud().shown && hud().pz) });
      case 'pz-bar': return new El({ domId: id, visible: () => !!(hud().shown && hud().pzBar) });
      case 'pz-label': return new El({ domId: id, visible: () => !!(hud().shown && hud().pzBar), text: () => hud().pzLabel ?? '' });
      case 'hud-dmg': return new El({ domId: id, visible: () => !!(hud().shown && hud().dmg) });
      case 'hud-hold': return new El({ domId: id, visible: () => !!(hud().shown && hud().hold) });
      case 'hud-radio': return new El({ domId: id, visible: () => !!hud().shown, cls: (c) => (c === 'show' ? !!hud().radio : undefined) });
      case 'hud-radio-text': return new El({ domId: id, visible: () => !!hud().shown, text: () => hud().radioText ?? '' });
      case 'res-extra':
        return new El({ domId: id, visible: () => tiles().length > 0, text: () => tiles().map(([v, l]) => v + ' ' + l).join(' ') });
      case 'pad-name': return new El({ uid: id, text: () => pad().name ?? (U(id)?.value || '') });
      case 'pad-hint': return new El({ uid: id, text: () => pad().hint ?? (U(id)?.value || '') });
      case 'tilt-note': return new El({ uid: id });
      default:
        return new El({ uid: id });
    }
  }

  function query(sel) {
    const s = String(sel).trim();
    let m;
    if (s === '.pad-focus') {
      const f = M().focus;
      return M().padNav?.shown && f ? focusEl(f) : null;
    }
    if (s === '#car-pick .pick.sel') {
      const k = CARS.find((c) => U('pick-' + c)?.sel);
      return k ? new El({ uid: 'pick-' + k }) : null;
    }
    if (s === '#mode-pick .sel') {
      const k = ['race', 'pursuit'].find((c) => U('mode-' + c)?.sel);
      return k ? new El({ uid: 'mode-' + k, dataset: () => ({ mode: k }) }) : null;
    }
    if (s === '#res-table tr.me') return rows().find((r) => r.classList.contains('me')) || null;
    if (s === '.pad-bind.listening') return pad().listening ? padRow(pad().listening) : null;
    if ((m = s.match(/^\.pad-bind\[data-act="?(\w+)"?\](?: b)?$/))) {
      const row = padRow(m[1]);
      return s.endsWith(' b') ? row.querySelector('b') : row;
    }
    if (s === '#touch .t-stick') {
      return new El({
        uid: 'touch-stick',
        cls: (c) => (c === 'active' ? !!touch().stick : c === 'lock' ? !!touch().lock : undefined),
        style: () => ({ left: touch().stick ? cssNum(touch().stick.x0) + 'px' : '', top: touch().stick ? cssNum(touch().stick.y0) + 'px' : '' }),
      });
    }
    if (s === '#touch .t-stick-knob') {
      return new El({ uid: 'touch-stick', style: () => { const d = touch().knob || 0; return { transform: d ? `translateX(${cssNum(d.toFixed(1))}px)` : '' }; } });
    }
    if (s === '#touch .t-pedal') {
      return new El({
        uid: 'touch-pedal',
        cls: (c) => (c === 'hidden' ? undefined : ['active', 'gas', 'brake', 'nitro', 'drift'].includes(c) ? (touch().panel || []).includes(c) : undefined),
        classes: () => ['t-pedal', ...(touch().panel || [])],
        style: () => {
          const u = touch().u;
          return {
            '--u': u == null ? '0%' : pct(u),
            '--gas': u != null && u > GAS_BOTTOM ? pct(u - GAS_BOTTOM) : '0%',
            '--brk': u != null && u < BRAKE_TOP ? pct(BRAKE_TOP - u) : '0%',
          };
        },
      });
    }
    if (s === '#touch .t-wheel') {
      return new El({ uid: 'touch-wheel', style: () => ({ transform: `rotate(${cssNum((touch().wheel || 0).toFixed(1))}deg)` }) });
    }
    const id = selectorId(s);
    if (id) return id.startsWith('touch-') || id.startsWith('lvl-tab-') || id.startsWith('pick-') || id.startsWith('mode-')
      || id === 'pause-title' || id === 'menu-controls' || id === 'touch-help' || id === 'vol-music' || id === 'vol-sfx'
      ? new El({ uid: id }) : byId(id);
    return undefined;
  }

  function queryAll(sel) {
    const s = String(sel).trim();
    if (/^(?:#level-pick )?\.lvl-tab$/.test(s)) return LEVELS.map((l) => new El({ uid: 'lvl-tab-' + l }));
    if (/^(?:#car-pick )?\.pick$/.test(s)) return CARS.map((c) => new El({ uid: 'pick-' + c }));
    if (s === '#res-table tr') return rows();
    if (s === '#res-extra .res-stat small') return tiles().map(([, label]) => new El({ text: () => label }));
    if (s === '.vol-music' || s === '.vol-sfx') {
      const id = s.slice(1);
      return [new El({ uid: id }), new El({ uid: id })];
    }
    if (s.startsWith('#pz-stars')) return [];
    return undefined;
  }

  const native = {
    byId: Document.prototype.getElementById,
    q: Document.prototype.querySelector,
    qa: Document.prototype.querySelectorAll,
    cs: window.getComputedStyle,
  };
  function swap(on) {
    if (on) {
      document.getElementById = (id) => byId(String(id));
      document.querySelector = (sel) => { const r = query(sel); return r === undefined ? native.q.call(document, sel) : r; };
      document.querySelectorAll = (sel) => { const r = queryAll(sel); return r === undefined ? native.qa.call(document, sel) : r; };
      window.getComputedStyle = (el, ...a) => (el instanceof El ? { display: el.visible() ? 'block' : 'none', getPropertyValue: () => '' } : native.cs.call(window, el, ...a));
    } else {
      delete document.getElementById;
      delete document.querySelector;
      delete document.querySelectorAll;
      window.getComputedStyle = native.cs;
    }
  }
}

// Chrome for the Rust build: WebGPU on Vulkan (SPEC 8.5) beside the
// harness's GPU flags.
export const RUST_ARGS = [
  '--enable-unsafe-webgpu', '--enable-features=Vulkan', '--use-angle=vulkan', '--ignore-gpu-blocklist', '--mute-audio',
  '--autoplay-policy=document-user-activation-required',
];

export const RUST_TYPES = { '.wasm': 'application/wasm', '.ttf': 'font/ttf' };

// The page the Rust build is served at.
export const RUST_PAGE = 'dist/next/index.html';
