//! Scene data: meshes, instances, materials, textures, nodes and lights as
//! plain data, and the `.mrscene` reader and writer (SPEC 5.1). Both the world
//! generator and the client depend on it; it depends on nothing of theirs.

#![forbid(unsafe_code)]
