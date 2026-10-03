// A gamepad from end to end, with a fake controller in place of
// navigator.getGamepads: moving around the menus and starting a race with
// it, pause and back, a slider, remapping a button on the Controller
// screen, and rumble.

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { launch, openGame } from './harness.js';
import { raceStarted, waitRacing, expectScreen, stored, sleep } from './flow-helpers.js';

let browser;
before(async () => { browser = await launch(); });
after(async () => { await browser?.close(); });

const PAD_ID = 'Test Pad (STANDARD GAMEPAD Vendor: 045e Product: 028e)';

// Runs in the page before the game: one standard pad on index 0 whose
// rumble effects are recorded in __pad.effects.
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
// Press and let go, a few frames each way.
async function press(game, b) {
  await hold(game, b, true); await sleep(90);
  await hold(game, b, false); await sleep(90);
}
const focused = (game) => game.eval(() => {
  const el = document.querySelector('.pad-focus');
  return el ? { id: el.id || el.querySelector('input, select')?.id || '', text: el.textContent.trim().slice(0, 30), edit: el.classList.contains('pad-edit') } : null;
});
const open = (o = {}) => openGame(browser, { init: fakePad, initArgs: [PAD_ID], ...o });

test('the menus with a gamepad: a highlight, car pick, Start to race, pause and back', async () => {
  const game = await open();
  try {
    await game.waitFor(() => document.body.classList.contains('pad'), { what: 'the pad to be seen' });
    assert.equal(await game.eval(() => getComputedStyle(document.getElementById('btn-pad')).display !== 'none'), true, 'Controller setup shows');
    assert.equal(await focused(game), null, 'no highlight before the pad is used');
    // The first push shows where you are (Race), without moving.
    await press(game, 'down');
    assert.equal((await focused(game))?.id, 'btn-start');
    // Up from Race reaches the car picks; ◂ ▸ moves along them; A picks one.
    await press(game, 'up');
    const first = await focused(game);
    assert.match(first.text, /./);
    const picks = await game.eval(() => [...document.querySelectorAll('#car-pick .pick')].map((b) => b.textContent.trim().slice(0, 30)));
    assert.ok(picks.includes(first.text), `up from Race lands on a car (${first.text})`);
    await press(game, 'right');
    const second = await focused(game);
    assert.notEqual(second.text, first.text, 'right moves to the next car');
    await press(game, 'A');
    const car = await stored(game, 'car');
    const picked = await game.eval(() => document.querySelector('#car-pick .pick.sel').textContent.trim().slice(0, 30));
    assert.equal(picked, second.text, 'A picked it');
    assert.ok(car);

    // Start races.
    await press(game, 'Start');
    await game.waitFor(raceStarted, { timeout: 20000, what: 'the race to start' });
    await waitRacing(game);
    // Start pauses, B resumes; Start pauses, down to Main menu, A.
    await press(game, 'Start');
    await expectScreen(game, 'pause');
    assert.equal((await focused(game))?.id, 'btn-resume', 'Resume is highlighted');
    await press(game, 'B');
    await expectScreen(game, 'none');
    assert.equal(await game.eval(() => window.__game.mode), 'race');
    await press(game, 'Start');
    await expectScreen(game, 'pause');
    await press(game, 'down'); await press(game, 'down'); // Restart, Main menu
    assert.equal((await focused(game))?.id, 'btn-quit');
    await press(game, 'A');
    await expectScreen(game, 'menu');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('A held from the menu doesn\'t fire the nitro when the race starts', async () => {
  const game = await open();
  try {
    await game.waitFor(() => document.body.classList.contains('pad'));
    await hold(game, 'A', true); // A on Race, and keep holding it
    await game.waitFor(raceStarted, { timeout: 20000, what: 'the race to start' });
    await sleep(150);
    assert.equal(await game.eval(() => window.__race.input.state.nitro), false, 'still held: quiet');
    await hold(game, 'A', false); await sleep(100);
    await hold(game, 'A', true); await sleep(100);
    assert.equal(await game.eval(() => window.__race.input.state.nitro), true, 'pressed again: nitro');
  } finally { await game.close(); }
});

test('a slider and a drop-down: A to adjust, ◂ ▸ to change, A when done', async () => {
  const game = await open({ storage: { 'mr.musicVol': 0.5 } });
  try {
    await game.waitFor(() => document.body.classList.contains('pad'));
    await press(game, 'down'); // show the highlight on Race
    await press(game, 'down'); // into the options row, under Race: Track
    assert.match((await focused(game)).text, /^Track/);
    // Not adjusting: ◂ ▸ move along the row.
    await press(game, 'left');
    assert.match((await focused(game)).text, /^SFX/);
    await press(game, 'right');
    await press(game, 'A');
    assert.equal((await focused(game)).edit, true, 'adjusting the track');
    await press(game, 'right');
    assert.equal(await stored(game, 'track'), await game.eval(() => document.getElementById('opt-track').options[1].value));
    await press(game, 'A');
    await press(game, 'left'); await press(game, 'left');
    assert.match((await focused(game)).text, /^Music/);
    await press(game, 'A');
    assert.equal((await focused(game)).edit, true, 'adjusting the volume');
    await press(game, 'right');
    await press(game, 'right');
    assert.equal(await stored(game, 'musicVol'), 0.6);
    await press(game, 'B');
    assert.equal((await focused(game)).edit, false, 'B: done, still on the menu');
    await expectScreen(game, 'menu');
    await press(game, 'right');
    assert.match((await focused(game)).text, /^SFX/);
  } finally { await game.close(); }
});

test('remapping: pick Throttle, press LB, and LB is the throttle (saved per controller)', async () => {
  const game = await open();
  try {
    await game.waitFor(() => document.body.classList.contains('pad'));
    await game.click('#btn-pad');
    await expectScreen(game, 'padsetup');
    const row = (a) => game.eval((act) => document.querySelector(`.pad-bind[data-act=${act}] b`).textContent, a);
    assert.equal(await game.eval(() => document.getElementById('pad-name').textContent), 'Test Pad');
    assert.equal(await row('throttle'), 'RT');
    assert.equal(await row('handbrake'), 'X / RB');
    // Held buttons light their rows.
    await hold(game, 'RT', true); await sleep(80);
    assert.equal(await game.eval(() => document.querySelector('.pad-bind[data-act=throttle]').classList.contains('on')), true);
    await hold(game, 'RT', false);
    // With the pad: highlight Throttle, A, then LB.
    await press(game, 'down');
    for (let i = 0; i < 6 && (await focused(game))?.text !== 'ThrottleRT'; i++) {
      const f = await focused(game);
      await press(game, /Steer/.test(f.text) ? 'down' : 'up');
    }
    assert.equal((await focused(game)).text, 'ThrottleRT');
    await press(game, 'A');
    assert.equal(await game.eval(() => document.querySelector('.pad-bind[data-act=throttle]').classList.contains('listening')), true);
    await press(game, 'LB');
    assert.equal(await row('throttle'), 'LB');
    assert.equal(await row('handbrake'), 'X / RB');
    assert.deepEqual((await stored(game, 'padMaps'))[PAD_ID].throttle, [{ button: 4 }]);
    assert.equal(await game.eval(() => !!document.querySelector('.pad-bind.listening')), false, 'the A after it didn\'t start another');
    // LB taken for the handbrake replaces X and RB there, and frees it from the throttle.
    await game.click('.pad-bind[data-act=handbrake]');
    await sleep(50); // a frame to start listening
    await press(game, 'LB');
    assert.equal(await row('handbrake'), 'LB');
    assert.equal(await row('throttle'), '—');
    // Defaults puts it all back.
    await game.click('#pad-defaults');
    await sleep(100);
    assert.equal(await row('throttle'), 'RT');
    assert.equal((await stored(game, 'padMaps'))?.[PAD_ID], undefined);
    // Remap again, and drive with it.
    await game.click('.pad-bind[data-act=throttle]');
    await sleep(50);
    await press(game, 'LB');
    await press(game, 'B'); // back to the menu
    await expectScreen(game, 'menu');
    await press(game, 'Start');
    await waitRacing(game);
    await hold(game, 'LB', true); await sleep(100);
    assert.equal(await game.eval(() => window.__race.input.state.throttle), 1, 'LB: throttle');
    await hold(game, 'LB', false);
    await hold(game, 'RT', true); await sleep(100);
    assert.equal(await game.eval(() => window.__race.input.state.throttle), 0, 'RT: nothing now');
    assert.deepEqual(game.errors, []);
  } finally { await game.close(); }
});

test('rumble: a wall hit jolts the pad, then stops, and the switch turns it off', async () => {
  const game = await open({ query: 'autostart=sports' });
  try {
    await waitRacing(game);
    const effects = () => game.eval(() => window.__pad.effects.splice(0));
    await effects();
    await game.eval(() => window.__race.phys.events.push({ type: 'impact', strength: 0.8, x: window.__race.player.x, y: 0, z: window.__race.player.z, side: 1 }));
    await sleep(150);
    const hit = (await effects()).filter((e) => e.type === 'dual-rumble');
    assert.ok(hit.length > 0, 'a rumble');
    assert.ok(Math.max(...hit.map((e) => e.strongMagnitude)) > 0.7, 'a hard one');
    await sleep(700);
    assert.ok((await effects()).some((e) => e.type === 'reset'), 'and it stops');
    // Rumble off (the Controller screen's switch, saved).
    await press(game, 'Start');
    await expectScreen(game, 'pause');
    await game.click('#btn-pad-pause');
    await expectScreen(game, 'padsetup');
    await game.click('#opt-rumble');
    assert.equal(await stored(game, 'rumble'), false);
    await press(game, 'Start'); // leaves the Controller screen…
    await expectScreen(game, 'pause');
    await press(game, 'Start'); // …and Start again resumes
    await expectScreen(game, 'none');
    await effects();
    await game.eval(() => window.__race.phys.events.push({ type: 'impact', strength: 0.8, x: 0, y: 0, z: 0, side: 1 }));
    await sleep(150);
    assert.deepEqual((await effects()).filter((e) => e.type === 'dual-rumble'), [], 'off: no rumble');
  } finally { await game.close(); }
});
