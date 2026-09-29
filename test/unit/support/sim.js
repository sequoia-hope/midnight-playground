// Shared bits for the driving tests: a test track, car bodies without a
// three.js model, and a seeded Math.random.
import { Track } from '../../../src/track/Track.js';
import { mulberry32 } from '../../../src/util/math.js';
import './three.js';

const { Vehicle } = await import('../../../src/vehicles/Vehicle.js');
export { Vehicle };

// Body sizes of the five player cars (from CarModel.js).
export const DIMS = {
  sports: { length: 4.47, width: 1.9, wheelBase: 2.6 },
  muscle: { length: 4.86, width: 1.95, wheelBase: 2.8 },
  super: { length: 4.57, width: 2.05, wheelBase: 2.7 },
  electric: { length: 4.74, width: 1.98, wheelBase: 2.9 },
  rally: { length: 4.12, width: 1.9, wheelBase: 2.55 },
  rival: { length: 4.6, width: 1.95, wheelBase: 2.7 },
};

// A car with no model: just the dimensions physics and AI read.
export function makeVehicle(kind = 'sports', mass = 1400) {
  const dims = DIMS[kind] || DIMS.rival;
  return new Vehicle({ dims: { ...dims, height: 1.3, wheelRadius: 0.34 }, root: { visible: false } }, { kind, mass });
}

// A dead-straight, flat road (x grows along it; +z is to the right).
export function straightTrack(length = 3000, road = 'freeway') {
  return new Track({
    id: 'test-straight', mode: 'race', startHeight: 0, startHeading: 0, finishRunoff: 180,
    segments: [[length, 0, 0, { zone: 0, road }]],
    zones: [{ key: 'test', name: 'TEST', landform: 'valley' }],
  });
}

// Run fn with Math.random replaced by a seeded generator.
export async function withSeededRandom(seed, fn) {
  const orig = Math.random;
  Math.random = mulberry32(seed);
  try { return await fn(); } finally { Math.random = orig; }
}
