// Every level, ready to build: levels made from survey data (Seaside
// Raceway) load it in prepare(), which the game's world builder awaits
// before it lays out the track. Tests that build tracks import LEVELS from
// here instead of src/levels/index.js.

import { LEVELS, levelById } from '../../../src/levels/index.js';

await Promise.all(LEVELS.map((l) => l.prepare?.()));

export { LEVELS, levelById };
