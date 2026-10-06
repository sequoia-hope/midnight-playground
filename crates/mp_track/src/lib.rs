//! Level definition types, the `Track` sampled every metre, and road types
//! (port of `src/track/Track.js` and `roadTypes.js`).

#![forbid(unsafe_code)]
// Index loops stay index loops: they mirror the JS line for line (DECISIONS D52).
#![allow(clippy::needless_range_loop)]

pub mod level;
pub mod road_types;
pub mod track;

pub use level::*;
pub use road_types::{ROAD_TYPES, RoadType, road_index, road_type};
pub use track::{
    Bounds, FenceGap, Frame, Projection, RUNOFF_FLAT, RoadDistance, Tag, Track, TrackZone,
    trapezoid,
};
