//! World generation: turns a level into an engine-neutral `mr_scene::Scene`
//! plus animators and the data the simulation needs (SPEC 5).

#![forbid(unsafe_code)]

pub mod builder;
pub mod color;
pub mod color_builder;
pub mod colorizer;
pub mod flora;
pub mod geom;
pub mod material;
pub mod object;
pub mod road;
pub mod sea;
pub mod sky;
pub mod stages;
pub mod terrain;
pub mod terrain_mesh;
pub mod textures;
pub mod three_geom;
pub mod world;
