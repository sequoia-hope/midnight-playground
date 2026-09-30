// Every police radio clip the game can play, from every level, as JSON on
// stdout for render.py.
//
//   node tools/radio-voice/lines.mjs

import { LEVELS } from '../../src/levels/index.js';
import { levelsRadioClips } from '../../src/game/audio/radioLines.js';

console.log(JSON.stringify(levelsRadioClips(LEVELS), null, 1));
