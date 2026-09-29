// Shared steps for the game-flow tests (menu, race flow, levels).

import assert from 'node:assert/strict';

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// A race is up: a Race object in countdown or racing, and no menu over it.
export const raceStarted = () => !!window.__race && ['countdown', 'racing'].includes(window.__race.state)
  && window.__game?.mode === 'race'
  && document.getElementById('menu').classList.contains('hidden');

export const isShown = (sel) => {
  const el = document.querySelector(sel);
  return !!el && !el.classList.contains('hidden') && getComputedStyle(el).display !== 'none' && el.getClientRects().length > 0;
};

// Read a saved setting (localStorage 'mr.<key>', JSON-encoded).
export const stored = (game, key) => game.eval((k) => {
  const v = localStorage.getItem('mr.' + k);
  return v === null ? null : JSON.parse(v);
}, key);

// Press Race (a tap on a touch device, a click otherwise) and wait for it.
export async function startFromMenu(game, { touch = false, timeout = 20000 } = {}) {
  await (touch ? game.tap('#btn-start') : game.click('#btn-start'));
  await game.waitFor(raceStarted, { timeout, what: 'the race to start' });
}

// Tag the current Race so a test can tell when a new one replaces it.
export const markRace = (game) => game.eval(() => { window.__race.__tagged = true; });
export const newRaceStarted = () => !!window.__race && !window.__race.__tagged
  && ['countdown', 'racing'].includes(window.__race.state) && window.__game?.mode === 'race';

export async function waitRacing(game, timeout = 20000) {
  await game.waitFor(() => window.__race?.state === 'racing', { timeout, what: 'the countdown to end' });
}

export async function expectScreen(game, name, timeout = 8000) {
  await game.waitFor(`(${(n) => {
    const shown = ['loading', 'menu', 'pause', 'results'].filter((id) => !document.getElementById(id).classList.contains('hidden'));
    return (shown.join(',') || 'none') === n;
  }})(${JSON.stringify(name)})`, { timeout, what: `the "${name}" screen` });
  assert.equal(await game.screen(), name);
}
