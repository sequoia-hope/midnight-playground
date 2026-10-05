// A gamepad against the Rust build, beyond test/e2e/gamepad.test.js (which
// runs against it with `npm run test:e2e:rust`, WP 6.7): driving with the
// triggers and the stick (its dead zone and curve), a map saved for the
// pad's id, rumble from a real wall hit and the countdown, and the
// Controller screen from pause. The fake controller is the JS suite's,
// installed once the page is up (the client reads getGamepads every frame).
// `__race.input.state` is `__mr.race.input`, `__pads` is `__mr.pads`, the
// highlight is `__mr.focus` with `__mr.padNav`, the Controller screen's DOM
// is `__mr.padsetup`.
//
//   cargo xtask web --release && node --test tools/parity/e2e/gamepad.test.mjs

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame, stored, waitRacing, expectScreen, sleep } from './harness.mjs';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

const PAD_ID = 'Test Pad (STANDARD GAMEPAD Vendor: 045e Product: 028e)';

// One standard pad on index 0 whose rumble effects are recorded in
// __pad.effects (test/e2e/gamepad.test.js's fakePad).
function fakePad(id) {
  const pad = {
    id, index: 0, connected: true, mapping: 'standard', timestamp: 0,
    axes: [0, 0, 0, 0],
    buttons: Array.from({ length: 17 }, () => ({ pressed: false, touched: false, value: 0 })),
    effects: [],
    vibrationActuator: {
      playEffect(type, p) { pad.effects.push({ type, ...p }); return Promise.resolve('complete'); },
      reset() { pad.effects.push({ type: 'reset' }); return Promise.resolve('complete'); },
    },
  };
  window.__pad = pad;
  Object.defineProperty(navigator, 'getGamepads', { configurable: true, value: () => [pad, null, null, null] });
}

const BTN = { A: 0, B: 1, X: 2, Y: 3, LB: 4, RB: 5, LT: 6, RT: 7, Back: 8, Start: 9, up: 12, down: 13, left: 14, right: 15 };
const hold = (game, b, on) => game.eval((i, v) => { const x = window.__pad.buttons[i]; x.pressed = v; x.value = v ? 1 : 0; }, BTN[b], on);
const axis = (game, i, v) => game.eval((j, x) => { window.__pad.axes[j] = x; }, i, v);
// Press and let go, a few frames each way.
async function press(game, b) {
  await hold(game, b, true); await sleep(90);
  await hold(game, b, false); await sleep(90);
}
const input = (game) => game.eval(() => window.__mr.race.input);
// The highlight (`.pad-focus`): the control's id, and whether it is being
// adjusted (`.pad-edit`); null while hidden.
const focused = (game) => game.eval(() => (window.__mr.padNav?.shown && window.__mr.focus
  ? { id: window.__mr.focus, edit: !!window.__mr.padNav.editing } : null));
const padsetup = (game) => game.eval(() => window.__mr.padsetup);
async function open(o = {}) {
  const game = await openGame(browser, o);
  await game.eval(fakePad, PAD_ID);
  await game.waitFor(() => window.__mr.pads?.connected === true, { what: 'the pad to be seen' });
  return game;
}

test('driving: the triggers, the stick (analogue, with its dead zone), A, X, Start to pause and resume', async () => {
  const game = await open({ query: 'autostart=sports' });
  try {
    await waitRacing(game);
    await hold(game, 'RT', true); await sleep(100);
    assert.equal((await input(game)).throttle, 1, 'RT: throttle');
    await hold(game, 'RT', false);
    await game.eval(() => { window.__pad.buttons[6].value = 0.4; });
    await sleep(100);
    assert.ok(Math.abs((await input(game)).brake - 0.4) < 1e-6, 'LT, partly: brake 0.4');
    await game.eval(() => { window.__pad.buttons[6].value = 0; });
    await axis(game, 0, 0.1); await sleep(100);
    assert.equal((await input(game)).steer, 0, 'inside the dead zone');
    await axis(game, 0, 0.56); await sleep(100);
    let s = await input(game);
    assert.ok(Math.abs(s.steer - Math.pow((0.56 - 0.12) / 0.88, 1.4)) < 1e-9, `the stick's curve (${s.steer})`);
    assert.equal(s.analog, true, 'the stick is analogue');
    await axis(game, 0, -1); await sleep(100);
    assert.equal((await input(game)).steer, -1);
    await axis(game, 0, 0);
    await hold(game, 'A', true); await hold(game, 'X', true); await sleep(100);
    s = await input(game);
    assert.equal(s.nitro, true, 'A: nitro');
    assert.equal(s.handbrake, true, 'X: handbrake');
    await hold(game, 'A', false); await hold(game, 'X', false);
    // The D-pad is the menus', not the steering, on the standard layout.
    await hold(game, 'right', true); await sleep(100);
    assert.equal((await input(game)).steer, 0, 'the D-pad doesn\'t steer');
    await hold(game, 'right', false);
    await press(game, 'Start');
    await expectScreen(game, 'pause');
    await press(game, 'Start');
    await expectScreen(game, 'none');
    assert.equal(await game.eval(() => window.__mr.mode), 'race');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('a remapped pad (mr.padMaps, keyed by its id) drives with its own map', async () => {
  const map = {
    left: [{ axis: 0, dir: -1, rest: 0 }], right: [{ axis: 0, dir: 1, rest: 0 }],
    throttle: [{ button: 4 }], brake: [{ button: 6 }], nitro: [{ button: 0 }], handbrake: [{ button: 2 }, { button: 5 }],
    lookBack: [{ button: 1 }], camera: [{ button: 3 }], reset: [{ button: 8 }], pause: [{ button: 9 }],
  };
  const game = await open({ query: 'autostart=sports', storage: { 'mr.padMaps': { [PAD_ID]: map } } });
  try {
    await waitRacing(game);
    await hold(game, 'LB', true); await sleep(100);
    assert.equal((await input(game)).throttle, 1, 'LB: throttle');
    await hold(game, 'LB', false);
    await hold(game, 'RT', true); await sleep(100);
    assert.equal((await input(game)).throttle, 0, 'RT: nothing now');
    await hold(game, 'RT', false);
    assert.equal(await game.eval(() => window.__mr.pads.resetLabel), 'Back');
    assert.deepEqual((await stored(game, 'padMaps'))[PAD_ID].throttle, [{ button: 4 }], 'the map is left as it was');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('rumble: the countdown and a wall hit jolt the pad, then it stops; off, nothing', async () => {
  const game = await open({ query: 'autostart=sports' });
  try {
    const effects = () => game.eval(() => window.__pad.effects.splice(0));
    await waitRacing(game);
    await sleep(400); // GO's kick runs 200 ms
    const start = await effects();
    assert.ok(start.some((e) => e.type === 'dual-rumble' && e.duration === 140), 'the countdown and GO buzz');
    assert.ok(start.some((e) => e.type === 'reset'), 'and stop');
    // Full throttle, full right lock: into the barrier.
    await hold(game, 'RT', true);
    await axis(game, 0, 1);
    let hit = [];
    for (let i = 0; i < 80 && !hit.some((e) => e.strongMagnitude > 0.5); i++) {
      await sleep(100);
      hit = hit.concat((await effects()).filter((e) => e.type === 'dual-rumble'));
    }
    await hold(game, 'RT', false);
    await axis(game, 0, 0);
    assert.ok(hit.length > 0, 'a rumble');
    assert.ok(Math.max(...hit.map((e) => e.strongMagnitude)) > 0.5, `a hard one (${Math.max(...hit.map((e) => e.strongMagnitude))})`);
    assert.ok(hit.every((e) => e.strongMagnitude <= 1 && e.weakMagnitude <= 1));
    // Stopped, the scrape's buzz lapses and the pad is reset.
    await game.waitFor(() => window.__mr.race.speed < 0.5, { timeout: 15000, what: 'the car to stop' });
    await sleep(700);
    assert.ok((await effects()).some((e) => e.type === 'reset'), 'and it stops');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }

  // The Rumble switch off (saved): nothing at all.
  const off = await open({ query: 'autostart=sports', storage: { 'mr.rumble': false } });
  try {
    await waitRacing(off);
    await off.eval(() => window.__pad.effects.splice(0));
    await hold(off, 'RT', true); await axis(off, 0, 1);
    await sleep(4000);
    assert.deepEqual(await off.eval(() => window.__pad.effects), [], 'off: no rumble');
    assert.equal(await off.eval(() => window.__mr.pads.rumbleOn), false);
  } finally { await off.close(); }
});

test('the Controller screen from pause: Esc stops listening, the Rumble switch, Start leaves and Start resumes', async () => {
  const game = await open({ query: 'autostart=sports' });
  try {
    await waitRacing(game);
    await press(game, 'Start');
    await expectScreen(game, 'pause');
    await game.click('#btn-pad-pause');
    await expectScreen(game, 'padsetup');
    await game.click('#pad-bind-camera');
    await sleep(50);
    assert.equal((await padsetup(game)).listening, 'camera');
    await game.key('Escape');
    assert.equal((await padsetup(game)).listening, null, 'Esc: not listening');
    await expectScreen(game, 'padsetup');
    await game.click('#opt-rumble');
    assert.equal(await stored(game, 'rumble'), false);
    await press(game, 'Start'); // leaves the Controller screen…
    await expectScreen(game, 'pause');
    await press(game, 'Start'); // …and Start again resumes
    await expectScreen(game, 'none');
    await game.eval(() => window.__pad.effects.splice(0));
    await hold(game, 'RT', true); await axis(game, 0, 1);
    await sleep(4000);
    assert.deepEqual((await game.eval(() => window.__pad.effects.splice(0))).filter((e) => e.type === 'dual-rumble'), [], 'off: no rumble');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});
