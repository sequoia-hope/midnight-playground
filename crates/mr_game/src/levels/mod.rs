//! Client setup for levels `mr_worldgen` builds whole besides Sierra
//! (roadmap M7): each names its scenery modules for the client's world
//! build (`animate`), so the wasm links only the levels the client builds,
//! and holds what the level needs of the client beyond the shared animator
//! path.

pub mod desert;
pub mod streets;
