// test/e2e/gamepad.test.js against the Rust build (roadmap WP 6.4): a
// gamepad from end to end, with a fake controller in place of
// navigator.getGamepads (the JS suite's, installed once the page is up: the
// client reads getGamepads every frame, as Gamepad.js does).
//
// `__race.input.state` is `__mr.race.input`, `__pads` is `__mr.pads`, the
// highlight is `__mr.focus` with `__mr.padNav`, the Controller screen's DOM
// is `__mr.padsetup`, and a control is named by its id rather than its
// text; the JS suite's injected wall impact is a real one here (full
// throttle and full right lock into the barrier).
//
//   cargo xtask web --release && node --test tools/parity/e2e/gamepad.test.mjs

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame, stored, waitRacing, expectScreen, raceStarted, sleep } from './harness.mjs';

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

test('A held into the race doesn\'t fire the nitro until it is pressed again', async () => {
  const game = await open();
  try {
    await hold(game, 'A', true); // held from the menu, and kept held
    await game.click('#btn-start');
    await game.waitFor(() => !!window.__mr.race && window.__mr.mode === 'race' && window.__mr.screen === 'none', { timeout: 30000, what: 'the race to start' });
    await waitRacing(game);
    await sleep(150);
    assert.equal((await input(game)).nitro, false, 'still held: quiet');
    await hold(game, 'A', false); await sleep(100);
    await hold(game, 'A', true); await sleep(100);
    assert.equal((await input(game)).nitro, true, 'pressed again: nitro');
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

test('the menus with a gamepad: a highlight, car pick, Start to race, pause and back', async () => {
  const game = await open();
  try {
    await game.waitFor(() => window.__mr.ui('btn-pad')?.visible === true, { what: 'Controller setup to show' });
    assert.equal(await focused(game), null, 'no highlight before the pad is used');
    // The first push shows where you are (Race), without moving.
    await press(game, 'down');
    assert.equal((await focused(game))?.id, 'btn-start');
    // Up from Race reaches the car picks; ◂ ▸ moves along them; A picks one.
    await press(game, 'up');
    const first = (await focused(game)).id;
    assert.match(first, /^pick-/, `up from Race lands on a car (${first})`);
    await press(game, 'right');
    const second = (await focused(game)).id;
    assert.match(second, /^pick-/);
    assert.notEqual(second, first, 'right moves to the next car');
    await press(game, 'A');
    assert.equal(await stored(game, 'car'), second.slice(5), 'A picked it');

    // Start races.
    await press(game, 'Start');
    await game.waitFor(raceStarted, { timeout: 30000, what: 'the race to start' });
    await waitRacing(game);
    // Start pauses, B resumes; Start pauses, down to Main menu, A.
    await press(game, 'Start');
    await expectScreen(game, 'pause');
    await game.waitFor(() => window.__mr.focus === 'btn-resume', { timeout: 3000, what: 'Resume highlighted' });
    await press(game, 'B');
    await expectScreen(game, 'none');
    assert.equal(await game.eval(() => window.__mr.mode), 'race');
    await press(game, 'Start');
    await expectScreen(game, 'pause');
    await game.waitFor(() => window.__mr.focus === 'btn-resume', { timeout: 3000, what: 'Resume highlighted' });
    await press(game, 'down'); await press(game, 'down'); // Restart, Main menu
    assert.equal((await focused(game))?.id, 'btn-quit');
    await press(game, 'A');
    await expectScreen(game, 'menu');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('a slider and a drop-down: A to adjust, ◂ ▸ to change, A when done', async () => {
  const game = await open({ storage: { 'mr.musicVol': 0.5 } });
  try {
    await press(game, 'down'); // show the highlight on Race
    await press(game, 'down'); // into the options row, under Race: Track
    assert.equal((await focused(game)).id, 'opt-track');
    // Not adjusting: ◂ ▸ move along the row.
    await press(game, 'left');
    assert.equal((await focused(game)).id, 'vol-sfx');
    await press(game, 'right');
    await press(game, 'A');
    assert.equal((await focused(game)).edit, true, 'adjusting the track');
    await press(game, 'right');
    const track = await stored(game, 'track');
    assert.ok(track && track !== 'auto', `the next track (${track})`);
    await press(game, 'A');
    await press(game, 'left'); await press(game, 'left');
    assert.equal((await focused(game)).id, 'vol-music');
    await press(game, 'A');
    assert.equal((await focused(game)).edit, true, 'adjusting the volume');
    await press(game, 'right');
    await press(game, 'right');
    assert.equal(await stored(game, 'musicVol'), 0.6);
    await press(game, 'B');
    assert.equal((await focused(game)).edit, false, 'B: done, still on the menu');
    await expectScreen(game, 'menu');
    await press(game, 'right');
    assert.equal((await focused(game)).id, 'vol-sfx');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('remapping: pick Throttle, press LB, and LB is the throttle (saved per controller)', async () => {
  const game = await open();
  try {
    await game.waitFor(() => window.__mr.ui('btn-pad')?.visible === true, { what: 'Controller setup to show' });
    await game.click('#btn-pad');
    await expectScreen(game, 'padsetup');
    await game.waitFor(() => !!window.__mr.padsetup);
    const row = async (a) => (await padsetup(game)).binds[a];
    assert.equal((await padsetup(game)).name, 'Test Pad');
    assert.equal(await row('throttle'), 'RT');
    assert.equal(await row('handbrake'), 'X / RB');
    // Held buttons light their rows.
    await hold(game, 'RT', true); await sleep(80);
    assert.deepEqual((await padsetup(game)).on, ['throttle']);
    await hold(game, 'RT', false);
    // With the pad: highlight Throttle, A, then LB.
    await press(game, 'down');
    for (let i = 0; i < 6 && (await focused(game))?.id !== 'pad-bind-throttle'; i++) {
      const f = await focused(game);
      await press(game, /left|right/.test(f.id) ? 'down' : 'up');
    }
    assert.equal((await focused(game)).id, 'pad-bind-throttle');
    await press(game, 'A');
    assert.equal((await padsetup(game)).listening, 'throttle');
    await press(game, 'LB');
    assert.equal(await row('throttle'), 'LB');
    assert.equal(await row('handbrake'), 'X / RB');
    assert.equal((await padsetup(game)).hint, 'Throttle: LB');
    assert.deepEqual((await stored(game, 'padMaps'))[PAD_ID].throttle, [{ button: 4 }]);
    assert.equal((await padsetup(game)).listening, null, 'the A after it didn\'t start another');
    // LB taken for the handbrake replaces X and RB there, and frees it from the throttle.
    await game.click('#pad-bind-handbrake');
    await sleep(50); // a frame to start listening
    await press(game, 'LB');
    assert.equal(await row('handbrake'), 'LB');
    assert.equal(await row('throttle'), '—');
    assert.equal((await padsetup(game)).name, 'Test Pad · remapped');
    // Defaults puts it all back.
    await game.click('#pad-defaults');
    await sleep(100);
    assert.equal(await row('throttle'), 'RT');
    assert.equal((await stored(game, 'padMaps'))?.[PAD_ID], undefined);
    // Remap again, and drive with it.
    await game.click('#pad-bind-throttle');
    await sleep(50);
    await press(game, 'LB');
    await press(game, 'B'); // back to the menu
    await expectScreen(game, 'menu');
    await press(game, 'Start');
    await game.waitFor(raceStarted, { timeout: 30000, what: 'the race to start' });
    await waitRacing(game);
    await hold(game, 'LB', true); await sleep(100);
    assert.equal((await input(game)).throttle, 1, 'LB: throttle');
    await hold(game, 'LB', false);
    await hold(game, 'RT', true); await sleep(100);
    assert.equal((await input(game)).throttle, 0, 'RT: nothing now');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
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
