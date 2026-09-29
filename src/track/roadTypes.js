// Road cross-section presets shared by every level. hw = half the paved
// width (m); margin = how far past the paved edge the barrier/cliff sits;
// edge = what lines the road: 'terrain' (rock wall or guardrail, decided from
// the ground), 'fence', 'jersey' (concrete barrier), 'rail' (kerb + railing),
// 'curb' (kerb and pavement, built by the level's scenery) or 'none'.
// marks = the painted line scheme (see Road.buildMarkings); bank scales the
// automatic banking in corners (default 1).
//
// New types go on the end: tracks store the index into this list.
export const ROAD_TYPES = {
  mountain: { hw: 5.4, margin: 1.4, lanes: 2, edge: 'terrain', tone: 0, marks: 'double' },
  valley: { hw: 5.2, margin: 1.6, lanes: 2, edge: 'fence', tone: 1, marks: 'dashed' },
  freeway: { hw: 9.4, margin: 0.5, lanes: 4, edge: 'jersey', tone: 2, marks: 'freeway' },
  coastal: { hw: 5.6, margin: 1.4, lanes: 2, edge: 'terrain', tone: 0, marks: 'double' },
  boulevard: { hw: 8.0, margin: 1.1, lanes: 4, edge: 'rail', tone: 1, marks: 'boulevard' },
  // Downtown Streets: two lanes each way between kerbs; the wall is the
  // kerb. Level across (no banking) so the pavements line up.
  street: { hw: 7.0, margin: 0.25, lanes: 4, edge: 'curb', tone: 2, marks: 'avenue', bank: 0 },
  // Desert: a two-lane highway and the open dry-lake course.
  desert: { hw: 5.6, margin: 2.2, lanes: 2, edge: 'none', tone: 0, marks: 'dashed' },
  playa: { hw: 9.0, margin: 3.0, lanes: 4, edge: 'none', tone: 0, marks: 'guide' },
};
export const ROAD_KEYS = Object.keys(ROAD_TYPES);
