// Lets plain Node import modules that `import * as THREE from 'three'`
// (Vehicle, Traffic): the browser gets 'three' from index.html's import map,
// so here a resolve hook maps it to the vendored build. Import this, then
// load those modules with a dynamic import().
import { register } from 'node:module';

register('./three-resolve.js', import.meta.url);
