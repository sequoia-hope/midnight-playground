import sierra from './sierra.js';
import coast from './coast.js';
import streets from './streets.js';
import desert from './desert.js';
import seaside from './seaside.js';
import cruise from './cruise.js';

// Everything the menu offers, in order.
export const LEVELS = [sierra, coast, streets, desert, seaside, cruise];
export const levelById = (id) => LEVELS.find((l) => l.id === id) || LEVELS[0];
