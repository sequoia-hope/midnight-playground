# The `.mrscene` file, version 1

A scene (SPEC 5.1) as a JSON header followed by little-endian binary
buffers, in the manner of glTF's GLB. The JS exporter writes it
(`tools/parity/scene-export.mjs`, with `tools/parity/lib/scene-page.js` in the
page); `mr_scene::read` and `mr_scene::write` read and write it.

## Layout

| Bytes | What |
|---|---|
| 0–7 | Magic `MRSCENE\0` |
| 8–11 | Format version, u32 LE (1) |
| 12–15 | Header length J in bytes, u32 LE |
| 16 … 16+J | Header: UTF-8 JSON |
| … | Spaces up to the next multiple of 8 |
| B … end | Binary section, `binary_length` bytes |

B is `16 + J` rounded up to a multiple of 8. Every buffer starts at a
multiple of 8 within the binary section, in the order of `accessors`, with
zero bytes between them.

## Header

```json
{
  "format": "mrscene", "version": 1,
  "meta": { "name": "sierra", "level": "sierra", "query": "...", "random_seed": 24301, "three": "180", "base": false },
  "binary_length": 136923936,
  "accessors": [ { "offset": 0, "count": 2925, "item_size": 3, "component": "f32", "normalized": false } ],
  "meshes": [...], "instances": [...], "materials": [...], "textures": [...],
  "nodes": [...], "roots": [0, 523, 524, 525, 526],
  "lights": [...], "night_params": [...], "environment": {...}
}
```

Everything refers to everything else by index into these arrays. Unknown
fields are an error (the reader denies them), so a change to the format
changes the version.

**Numbers.** JSON has no Infinity or NaN; where one can occur (bounds,
matrices, group counts, material values) it is written
`{"num": "Infinity"}`, `{"num": "-Infinity"}` or `{"num": "NaN"}`. Numbers
are JavaScript's shortest round-trip form; the reader parses them exactly
(`serde_json`'s `float_roundtrip`), so an f64 survives the trip bit for bit.

**Constants** (wrap modes, filters, formats, blending, sides, tone mapping)
are three.js r180's numbers; `mr_scene::three` names the common ones.

### accessors → `Scene::buffers`

One per typed array: `count` elements of `item_size` components of type
`component` (`f32`, `f64`, `u8`, `u16`, `u32`, `i8`, `i16`, `i32`), at
`offset` in the binary section. `normalized` is three's flag (integers read
as 0..1 by the GPU). An array three shares between geometries (or a pixel
array shared by two textures with one image) is stored once.

### meshes (geometries)

```json
{ "name": "", "attributes": [ { "name": "position", "accessor": 0 }, { "name": "ph", "accessor": 9, "instanced": true, "mesh_per_attribute": 1 } ],
  "index": 5, "groups": [ { "start": 0, "count": 360, "material_index": 0 } ],
  "draw_range": { "start": 0, "count": null },
  "bounding_box": null, "bounding_sphere": [x, y, z, r] }
```

Attributes keep their JS names and order: `position`, `normal`, `uv`,
`color`, and the custom ones (`aLane`, `aSurf`, `cell`, `fdata`, `ndata`,
`aVar`, `aDepth`, `ph`, `fl`, `gsize`, `aDir`, `aPar`, `aSeed`, `corner`,
`aBlue`, `aSize`, `aColor`, `aAlpha`, `alpha`). An interleaved attribute
(three's sprite quad) is stored de-interleaved. `instanced` marks an
`InstancedBufferAttribute` (one element per instance). `draw_range.count`
null is three's Infinity. The bounds are what the geometry holds (null if
never computed); some builders widen the sphere on purpose so the object
is never culled, so they are data, not something to recompute.

### nodes

```json
{ "name": "road", "type": "Group", "parent": 0, "children": [ ... ],
  "matrix": [16 numbers], "matrix_world": [16 numbers],
  "visible": true, "matrix_auto_update": true, "frustum_culled": true, "render_order": 0,
  "cast_shadow": false, "receive_shadow": true, "layers": 1, "user_data": {},
  "mesh": 3, "materials": [7], "multi_material": false, "instances": 2, "center": [0.5, 0.5], "light": 0 }
```

`type` is one of `Group`, `Object3D`, `Mesh`, `InstancedMesh`, `Points`,
`LineSegments`, `Line`, `LineLoop`, `Sprite`, `DirectionalLight`,
`HemisphereLight`, `SpotLight`, `PointLight`, `AmbientLight`. Matrices are
column-major (three's `elements`): `matrix` is the local transform as three
holds it (the authority when `matrix_auto_update` is false), `matrix_world`
the world transform at export. Both are recorded so a reader may use either
the hierarchy or the flattened transforms. Invisible objects are exported
with `visible: false`. `mesh`, `materials` and `multi_material` are present
on drawables (meshes, points, lines, sprites); with `multi_material` the
materials are indexed by the geometry's groups. `user_data` holds only the
plain values of three's `userData`.

`roots` lists the nodes without a parent. A level's scene has `world.root`
and the sky's dome, sun, sun target and hemisphere light (which three keeps
in the scene, not under the world root).

### instances

```json
{ "node": 41, "count": 812, "capacity": 812, "matrices": 17, "colors": 18, "bounding_sphere": [x, y, z, r] }
```

An `InstancedMesh`'s data: `matrices` is a buffer of `capacity` 4×4
column-major f32 matrices, of which the first `count` are drawn; `colors`
the linear RGB `instanceColor`, if any. Per-instance custom attributes live
in the mesh (`instanced: true`).

### materials

```json
{ "kind": "Terrain", "kind_opts": { "packed": true, "photo": false },
  "type": "MeshStandardMaterial", "name": "", "program_key": "triplanar:true:false",
  "params": { "color": { "color": [1, 1, 1] }, "roughness": 0.96, "map": { "texture": 0 }, ... },
  "uniforms": { "tRock": { "texture": 1 }, "tDetail": { "texture": 2 } },
  "shader": null }
```

- `kind`: the `MaterialKind`, one per distinct shader of the JS game (SPEC
  6.2). A patched or custom material carries it as a tag in the JS
  (`material.userData.kind`); a built-in material without a patch gets
  `Standard`, `Physical`, `Lambert`, `Basic`, `Line`, `Sprite` or `Points`
  from its type. The exporter refuses an untagged custom material.
- `kind_opts`: what a JS patch takes from its closure rather than the
  material (`Siding` mode; `FlickerPoints` rate, depth and blink;
  `AmbientProp` rgb; `Terrain` packed and photo).
- `program_key`: the material's `customProgramCacheKey()`, where set.
- `params`: the material's own properties in JS order (private `_x` fields
  under their public name `x`). A colour is `{"color": [r, g, b]}` (linear),
  a vector or quaternion `{"vec": [...]}`, a matrix `{"mat": [...]}`
  (column-major), an Euler `{"euler": [x, y, z, "XYZ"]}`, a texture
  `{"texture": index}`; other values as plain JSON.
- `uniforms`: for a `ShaderMaterial`, all its uniforms; for a built-in
  material with an `onBeforeCompile` patch, the uniforms the patch adds
  (read from the compiled program, minus the ones three's own shader has);
  null otherwise. Values as in `params`.
- `shader`: a `ShaderMaterial`'s `{ "vertex", "fragment" }` GLSL.

### textures

```json
{ "name": "", "source": "canvas", "url": null, "width": 256, "height": 256, "channels": 4, "pixels": 6,
  "format": 1023, "type": 1009, "flip_y": true, "color_space": "srgb", "premultiply_alpha": false,
  "unpack_alignment": 4, "generate_mipmaps": true, "wrap_s": 1000, "wrap_t": 1000,
  "mag_filter": 1006, "min_filter": 1008, "anisotropy": 8,
  "offset": [0, 0], "repeat": [1, 1], "rotation": 0, "center": [0, 0], "matrix_auto_update": true, "channel": 0 }
```

`pixels` is a u8 buffer of `width × height` elements of `channels`
components, top row first, exactly as the canvas (`getImageData`) or the
`DataTexture` array holds them. `source` is `canvas`, `data` or `image`
(Seaside's aerial photo, decoded by the browser; `url` names the file).
`flip_y` says whether three flips the rows on upload (canvas textures do,
data textures and the photo do not); the UVs and shader maths assume that
flip (SPEC 5.2). `color_space` is three's (`srgb`, `srgb-linear` or `""`).

### lights

```json
{ "node": 523, "type": "DirectionalLight", "color": [1, 0.87, 0.67], "intensity": 3.4, "cast_shadow": true,
  "target": [0, 120, 0],
  "shadow": { "map_size": [2048, 2048], "bias": -0.0004, "normal_bias": 0.6, "radius": 1, "blur_samples": 8,
              "camera": { "type": "OrthographicCamera", "near": 1, "far": 600, "left": -70, "right": 70, "top": 70, "bottom": -70, "fov": null } } }
```

Plus `ground_color` (hemisphere), `distance` and `decay` (spot, point),
`angle` and `penumbra` (spot). Colours linear; the light's position is its
node's.

### night_params

`{ "material": 5, "prop": "emissiveIntensity", "day": 0, "night": 0.06 }`:
`world.nightMaterials`, the material properties that follow nightfall
(`value = day + (night - day) × n`), for the materials the scene draws.

### environment

Fog (`FogExp2` colour and density), tone mapping and exposure, environment
intensity, the sky's night factor and the camera, as they were at export.

## The digest

`mr_scene::digest::digest` computes, and the exporter writes beside each
file as `<name>.digest.json`, a summary that two scenes can be compared by:
per mesh the vertex and index counts, local bounds, SHA-256 of each
attribute's bytes and of the index, and (for meshes drawn as triangles) the
surface area and area-weighted centroid; per texture the size and a SHA-256
of the pixels; per material its kind, type and textures; per drawable node
its mesh, kinds, textures, instance count and hashes, and world-space bounds
over every vertex of every instance. Both sides use the same f64 arithmetic
in the same order, so a file read back gives an identical digest
(`cargo xtask parity scene-check`).
