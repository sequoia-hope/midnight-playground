// Captures of the scenery replace Math.random with a seeded mulberry32
// before the page's own scripts run (the harness's `init` option), because
// world generation draws from it in two places (docs/rust-port/DECISIONS.md
// D20). The game itself is untouched.
//
//   openGame(browser, { query, init: seedRandom, initArgs: [RANDOM_SEED] })

export const RANDOM_SEED = 0x5eed;

// Runs in the page (serialised by evaluateOnNewDocument): self-contained.
export function seedRandom(seed) {
  let a = seed >>> 0;
  Math.random = function random() {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
