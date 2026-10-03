# Inventory: rendering and world generation

Generated 2026-10-03 by reading the JS game at commit `7213a89`. It is a map for porting, not a substitute for the source: check each claim against the file it cites before relying on it. Paths are relative to the repository root unless a section says otherwise.

---

All paths in this file are under `src/`. Draw-call and triangle counts are not here: they were measured at runtime and are in SPEC.md section 6.6.

The game is not entirely asset-free. Seaside Raceway loads `levels/seaside/photo.jpg` (708 KB) and base64-deflated survey grids embedded in `levels/seaside/ground.js` (671 KB) and `circuit.js` (120 KB). Everything else is procedural.

## 1. Renderer config (`main.js`)
- **Renderer** (`main.js:55-57`): `WebGLRenderer({antialias:false, powerPreference:'high-performance'})`, ACES Filmic tone mapping, PCFSoft shadows. Output colour space is the r180 default (sRGB). The camera is 62° FOV, near 0.3, far 9000 (`:60`).
- **Anti-aliasing:** the canvas has none. The composer renders into a HalfFloat target with 4× MSAA (`:61`).
- **Post chain** (`:62-66`): RenderPass → UnrealBloomPass(strength 0.38, radius 0.35, threshold 0.92) → OutputPass, which does the tone mapping and sRGB conversion. Bloom is always on.
- **"High quality" toggle** (`applyQuality`, `:70-76`) changes only three things:
  - pixel ratio is `min(devicePixelRatio, 1.5)` when on, 1 when off;
  - the shadow map is on or off;
  - the police PointLight exists only when it is on (`game/PursuitView.js:66`).
  - It defaults off on touch devices (`:33`).
- **Shadows** (`world/Sky.js:207-214`): one DirectionalLight, 2048² map, orthographic box ±70 m, near 1, far 600, bias -0.0004, normalBias 0.6. No cascades. The box re-centres on the focus point every frame (`:271-276`); the comment says it snaps to texels, but no snapping is implemented.
- **Fog:** `FogExp2`, keyed per level (density 0.00024–0.00042) (`Sky.js:219`, `:280`).
- **Global fog patch** (`Sky.js:26-40`): three's shared shader chunks are rewritten so fog picks up the sun's colour when looking toward `directionalLights[0]`.
- **Environment map** (`main.js:100-111`): the live sky dome is copied into a separate scene at scale 50 and turned into a PMREM env map. It is rebuilt whenever route progress moves 2.5% (`:624`); intensity 0.7.
- **Shader warm-up:** `renderer.compileAsync` runs at race start, capped at 3 s (`:389-390`).
- **Frame loop** (`:571-626`): dt comes from the rAF timestamp, clamped to 1/20 s. Order is `race.update` → `world.update(dt, s, focus, camera)` → `refreshEnv` → `composer.render()`.

## 2. Materials and shader patches
**Material counts:** MeshStandardMaterial ×221, MeshBasicMaterial ×35, PointsMaterial ×11, ShaderMaterial ×9, LineBasicMaterial ×6. MeshPhysicalMaterial ×4 is used only in `CarModel.js`; MeshLambertMaterial ×4 (Mountain waterfall and far trees, Beach); SpriteMaterial ×3. Most patches set `customProgramCacheKey`.

**Custom ShaderMaterials (9):**
- **Sky dome** (`Sky.js:42-171`). Uniforms: uZenith, uHorizon, uGround, uSunColor, uSunDir, uMoonDir, uNight, uTime, uCloud, uHaze, tNoise. It draws the gradient, sun glow, twilight arch, horizon haze matched to fog, domain-warped cumulus lit toward the sun, cirrus, sun disc, moon with maria, twinkling stars and a Milky Way. Depth is written at the far plane (`xyww`), back side, renderOrder -10.
- **Particles** (`game/Effects.js:7-32`): soft points with per-vertex size, alpha and colour, plus fog.
- **Skid marks** (`Effects.js:107`): flat dark quads with per-vertex alpha.
- **Police glow billboards** (`vehicles/CarModel.js:2065-2105`): camera-facing quads pulled toward the camera, with a minimum screen size (uMin); uniforms uRed, uBlue.
- **City traffic light streams** (`world/City.js:1334`): points slide along each block in the vertex shader; uniforms uTime, uNight, uFogK, uHalfH.
- **City sky-glow dome** (`City.js:1389`): additive light-pollution haze.
- **Coast surf foam and rings** (`world/Coast.js:26-70`, `:127`): procedural noise foam with swell bands; uniforms uTime, uBright, uSwell.
- **Lighthouse beam cone** (`Coast.js:72-121`, `:988`).
- **Street steam puffs** (`world/streets/props.js:419`), animated entirely in the vertex shader.

**onBeforeCompile patches on built-in materials:**

| File:line | Purpose |
|---|---|
| `world/TerrainMesh.js:413-546` `patchTriplanar` | Terrain. Triplanar: detail from above, stratified rock from the sides. Four scales from one packed detail texture (9 m, 61 m, 900 m macro, 2.3 m close grain). Ground type is read from vertex colour (grass, sand, dirt) and the `aSurf` attribute (paved slab joints). Desert varnish streaks, a derivative-based bump with no bump map, a roughness tweak, and an optional draped aerial photo (`MR_PHOTO`: tPhoto, tLoose, uPhotoBox). Uniforms: tRock, tDetail. |
| `world/Road.js:111-143` `patchAsphalt` | Lane wheel paths and oil stripe from the `aLane` attribute, repair patches, dusty edges, damp sheen at night (uWet). |
| `Road.js:277-283` | Shoulder: the gravel fades to its average colour toward the outer edge. |
| `Road.js:147-162` `patchMarkings` | Paint wear blotches. |
| `world/Sea.js:115-156` | Sea: depth-driven surf foam, two counter-scrolling normal scales, Fresnel alpha, sun/moon glitter (power 256), foam roughness. Uniforms: uTime, uOff2, tFoam. |
| `vehicles/CarModel.js:124-135` `lightMat` | Car lights: emission multiplied by vertex colour; albedo ignores it. |
| `world/desert/glow.js:14-83` | Flicker Points and InstancedMesh ground pools; per-point phase attribute, shared `glowTime`. |
| `world/Desert.js:1786` | Floodlight beam cones fade where the surface turns edge-on. |
| `world/Mountain.js:63-90` | Instancing-aware triplanar rock. |
| `Mountain.js:886` | Reflector emission takes the instance colour. |
| `world/coast/kit.js:89` | Triplanar rock (same pattern as Mountain). |
| `world/desert/parts.js:52` | Sandstone triplanar strata. |
| `world/Valley.js:89` `surfaceDetail` | World-space lap siding, board and shingle patterns. |
| `world/Beach.js:66` | Triplanar stucco grain. |
| `City.js:1518` `glowPointsMaterial` | Halo points with per-point size, a minimum pixel size and gentler fog. |
| `world/city/cityTextures.js:160` `patchAtlasMaterial` | Atlas cell picked by the `cell` attribute; `textureGrad` keeps mips correct. |
| `cityTextures.js:493` `patchCityMaterial` | Per-pixel lit windows, shopfronts, glass reflection, street-light spill, window-detail LOD fallback. |
| `world/harbor/textures.js:221` | Container atlas row from the `aVar` attribute, tinted by colour. |
| `world/streets/facades.js:395` | Façade atlas with a hashed lit-window function and street bounce light. |
| `world/streets/textures.js:349` `patchStreetAtlas` | Street atlas, same technique as the city atlas. |
| `streets/props.js:15` `ambientPatch` | Small ambient emissive proportional to albedo. |
| `streets/props.js:27` `neonFlicker` | Four neon flicker modes from the `ndata` attribute. |

## 3. Textures
- **How they're made:** almost all are canvas 2D, through `CanvasTexture` (`world/textures.js:19-27`). Defaults are repeat wrap, mipmaps, `LinearMipmapLinear`, anisotropy 8; car carbon fibre uses anisotropy 4 (`CarModel.js:50`). Pixel-level work uses `ImageData`; shapes use paths, ellipses, gradients, `shadowBlur`, `destination-out` compositing and `fillText`. Randomness comes from the seeded `mulberry32`.
- **DataTextures (2):**
  - the packed 256² RGBA terrain detail texture (`textures.js:111-152`): R soft noise, G grain, B cellular cells, A broad noise. It is shared by terrain, road, sky clouds and sea foam;
  - the seaside "loose ground" R8 mask (`TerrainMesh.js:561`).
- **Image file (1):** `photo.jpg`, loaded with `TextureLoader`, flipY false, anisotropy 8 (`TerrainMesh.js:550-565`).
- **Shared set** (`textures.js`, cached in a module Map): detail 256, rock 256, asphalt ×3 tones at 512, gravel 256, concrete 256, chevron 128, checker 256×64, glow 128, smoke 64, sign text textures (512×256), façade map plus emissive pairs 256×512. Sea wave normals are 256 (`Sea.js:11`).
- **Atlases:**
  - City façades 2560×512, three layers: map, emissive, mask (`cityTextures.js:354`);
  - Streets façades 1024² ×2 (`facades.js:97`), neon 1024×512 ×2, street atlas;
  - SignAtlas 2048 painted plus 1024 neon (`world/beach/atlas.js:7`), used by Beach and Desert;
  - separate SignAtlas copies in `Mountain.js:191` and `coast/kit.js:216`;
  - harbour container atlas 1024×512; raceway crowd 512×256 and banner atlas.
- **Per level:** roughly 20–40 distinct textures. This is an estimate; the true number is `renderer.info.memory.textures` (shown as `__stats.tex`).

## 4. Geometry construction
**Builders:**
- `Builder` (`world/valley/Builder.js:21`): `setFrame`, `pushFrame`/`popFrame`, `add(key, geo, local)`, `put`, `box` (base-pivoted), `cbox`, `beam(a, b, t)`, `build(materials, {castShadow})` → one merged mesh per bucket, `mergeAll`.
- `PaintBuilder` (`Builder.js:137`) and `ColorBuilder` (`world/beach/ColorBuilder.js:21`): flat paints become vertex colours in one "solid" bucket, split into near and far channels (near casts shadows).
- `GeoBuilder` (`world/city/geom.js:9`): `quad`, `tri`, `triUV`, `prism(footprint, y0, y1, {tileW, tileH, cell, roof…})`, `box` (rotated by yaw), `build`. It is flat-shaded and non-indexed, with optional `color` and `cell` attributes. Helpers: `staticMesh`, `instanced`, `trs`, `yawOf`.
- Others:
  - `extrude(track, ranges, profile)` (`Road.js:14`): cross-section profiles swept along the track;
  - `sweep` and `chunked` in `city/freeway.js:70,111`;
  - `Batch` (700 m chunks), `beam`, `frustum`, `ribbon` in `world/harbor/build.js`;
  - `ChunkedGeo` (`City.js:1617`) and `Chunks(560)` (`Streets.js:229`);
  - `SurfaceSampler` in `coast/kit.js:14` and `Mountain.js:21`; `makeGround` in `valley/ground.js`.
- **three primitives used:** Box 119, Cylinder 114, Plane 45, Icosahedron 26, Sphere 24, Cone 20, Torus 15, Extrude 11, Circle 9, Lathe 3, Capsule 3, Tube 1, plus `ShapeUtils.triangulateShape` and `CatmullRomCurve3`. Some code depends on BoxGeometry's face order for UV regions (`Streets.js` train; harbour containers).

**Static merging, instancing and LOD:**
- **Static merging:** most static content is merged per material and per spatial chunk. Road meshes are 560 m chunks (`Road.js:56`). Chunk sizes elsewhere: Mountain 480, Desert 500, City 600 (2000 on the cruise loop), Valley 600, Coast 720, freeway 700 (1800 loop), Raceway 420.
- **Instancing:** InstancedMesh appears in 16 files: rocks, trees, posts, piers, chevrons, boats, train cars, crowds, bulbs. `setColorAt` is used and sometimes animated; custom per-instance attributes exist in `desert/glow.js:61`.
- **Distance culling:**
  - City loop: chunks hide beyond 2000 m (`City.js:1418-1456`);
  - Mountain flag and waterfall skip their updates beyond 600 / 800 m.
  - There is no streaming: everything is built at load and frustum culling does the rest.
- **LOD schemes:**
  - terrain tiles (section 5);
  - Mountain forest in three tiers: verge, mid, and far Lambert quads out to 2300 m (`Mountain.js` `buildForest`);
  - Streets `farBlock` boxes, and parked cars as full or low models (`streets/props.js:308`);
  - car far LOD (section 8).

## 5. Terrain
- **Height function:** analytic, evaluated per query on the CPU by `Terrain.heightAt` (`world/Terrain.js:524`). Inputs:
  1. a far field: 32 m grid over the track bounds plus a 2600 m margin, holding distance to road, road height, nearest s and side (`:154-191`);
  2. sparse near-field tiles: 256 m, 4 m nodes, holding road-flatten weight K and target height (`:193-253`);
  3. per-zone landform functions blended across zone edges by x;
  4. flattens and carves registered by scenery `plan()` (`:98-112`).
- **Landforms:** mountain, valley, city, coast, beach, harbor, streets, canyon, desert, playa, raceway (`:28-45`). They are built from noise, fbm and ridged noise; canyon and desert terraces come from `strata()`. Streets and raceway use the level's own ground function (survey grids for raceway).
- **Road sculpting:** ground is lerped toward the road surface by K, with per-landform shoulder r0, band and pow. Raceway uses a corridor rule out to the walls. Elevated sections are skipped so they stand on piers.
- **Meshing** (`Terrain.js:597-620`, `TerrainMesh.js:295-395`):
  - 256 m tiles at a step of 4 m (near the road, or under 60 m from it), 16 m (under 700 m), or 32 m beyond;
  - edges shared with a coarser neighbour are interpolated to match, plus double-sided skirts;
  - each quad's diagonal follows the closest heights;
  - vertex colours come from `TerrainColorizer` (per landform, with crease darkening and mottling);
  - tiles are merged into 2×2, 3×3 or 5×5 groups depending on step, all with one material.
- **Physics** never queries the terrain. Cars ride `track.surfaceY(s, lat)` (`vehicles/CarPhysics.js:281,289`; `Kinematic.js:83`). Only the camera (`game/CameraRig.js:69`) and scenery placement use terrain heights.
- **Placement on rendered triangles:** `makeGround` and `SurfaceSampler` place objects on the triangles as drawn, not on the exact function. They always use the parity diagonal and ignore the height-based diagonal and edge stitching, so they can disagree slightly with `TerrainMesh`.

## 6. Lights
At most five real lights:
- Sun/moon DirectionalLight, the only shadow caster (`Sky.js:207`).
- HemisphereLight (`Sky.js:216`).
- Player headlight SpotLight: range 140, angle 0.55, no shadow, intensity `140 × lightsOn` (`game/Race.js:68`, `:348`).
- One shared police PointLight, HQ and flash setting only, placed at the nearest unit (`PursuitView.js:67`, `:285-290`).
- Desert train SpotLight (`Desert.js:962`).

Everything else is faked:
- emissive materials with colour above 1 so bloom catches them;
- additive glow Points and Sprites;
- additive ground "pool" quads under lamps and ahead of every car's headlights (`Effects.js:160-249`);
- police billboards;
- the `nightMaterials` list, which scales emissive intensity with nightfall (`World.js:95-103`).

## 7. Sky and time of day
- **Keying:** each level has a key array `{s, sunEl, zen, hor, sun, sunI, hemiS, hemiG, hemiI, fog, fogD, exp, night}` (example: `levels/sierra.js:84-93`). `s` is the fraction of track length.
- **Interpolation:** `Sky.sample` smoothsteps between keys (`Sky.js:225-239`). Loop levels (seaside, cruise) are pinned at p = 0.5. `?t=` overrides it.
- **What animates:** dome uniforms, sun direction (elevation plus a fixed azimuth), the light crossfading from sun to moon direction, sun colour and intensity, hemisphere colours and intensity, fog colour and density, `toneMappingExposure`, cloud cover and haze (`Sky.js:241-285`).
- **Night factor** `n` drives road wetness and every scenery updater.

## 8. Car models (`vehicles/CarModel.js`, 2377 lines)
- **Kinds:** `SPECS` has 13: 5 racers (sports, muscle, super, rally, electric), 6 traffic (sedan, hatch, van, pickup, boxtruck, tractor), and police plus policeSuv. Muscle and sports can also take a police livery.
- **Bodies:** lofts (`loft`, `:287`) of rounded cross-sections between the roof line and sill line, with arches cut in. Each face is assigned to a material bucket.
- **Detail:** decals projected from top, side, front or rear 2D views onto the surface (`Surf`, `decal`, `ribbon`, `:422-572`). Plus sweeps, X-axis lathes, airfoils, ExtrudeGeometry, boxes and cylinders, collected per bucket by `Parts` and merged (`:723`).
- **Detail levels:** high (vertex colours, finer stations) and low (`detail()`, `:1079`). Geometry is cached by kind|lod|variant (`:2018`).
- **Per-instance materials:**
  - paint: MeshPhysical with clearcoat, plus sheen on the supercar (`:2139-2143`); low LOD uses cached MeshStandard;
  - head, tail and reverse light materials and the electric car's accent;
  - siren red and blue lenses.
  - Everything else is shared (`shared()`, `:76`; glass is transparent MeshPhysical at high LOD).
- **Hierarchy:** root → body (roll and pitch springs, `vehicles/Vehicle.js:78-92`) → four wheels (front ones on steer pivots), each a single multi-material mesh of tyre, rim and brake. Calipers are added on high-LOD racers. There is a headlight anchor and, on police cars, a siren anchor (`:2180-2296`).
- **Light API:** `setBrake`, `setHeadlights`, `setReverse`, `setBoost`, `setSiren(mode, t)` drive emissive intensity.
- **Far LOD** (`:2309-2365`): everything except paint and lights is baked into one shared vertex-coloured mesh. It switches past 95 m and back inside 85 m (`vehicles/Traffic.js:18-24`). The e2e test asserts at most 4 calls versus about 15 for the full model.
- **Parked cars:** Beach, Streets, Desert, City and Coast bake them into static geometry.

## 9. Effects (`game/Effects.js`)
- Smoke: Points, 700 ring buffer, normal blend. Sparks: Points, 500, additive. Both are simulated on the CPU (drag, gravity, growth) and re-uploaded every frame.
- Skid marks: ring buffer of 2400 quads in one dynamic mesh.
- Nitro flames: two additive open cones per exhaust, scaled randomly each frame.
- Headlight ground pool: one additive plane per car. All cars share one material, so the per-car opacity gets overwritten (`:249` vs `:273`). `tailPoolMat` is created but never used.
- Camera (`CameraRig.js`): chase, far and bumper modes. FOV widens with speed (+16°) and nitro (+7°). Portrait screens get a wider vertical FOV. Shake is sine-based and comes from impacts, high-speed rumble and nitro.
- Speed lines are a CSS overlay (`game/HUD.js:161`), not rendered in 3D.

## 10. Animated scenery
All are closures in `world.updaters`, called as `(dt, night, camera, s)`:

| Item | How it animates |
|---|---|
| Desert freight train | Instance matrices along a rail path, triggered by player s (`Desert.js` `updateTrain`). |
| Tumbleweeds | CPU bounce physics on instances (`updateWeeds`). |
| Streets elevated train | Single mesh; z position scripted to meet the player (`Streets.js:1636`). |
| Mountain waterfall | Scrolling canvas textures on ribbons, rotating foam, pulsing sprites (`Mountain.js:1295`). |
| Mountain flag | CPU vertex sine wave (`Mountain.js:1136`). |
| Valley wheels | Windpump wheel instances, waterwheel, mill sails, scrolling creek normal map (`Valley.js:393`). |
| Beach | Ferris wheel and gondolas, coaster on a CatmullRom curve, foam texture offset and opacity, traffic-light emissive cycle (`Beach.js:1356`). |
| Coast | Foam uniforms, rotating lighthouse beam, bobbing boats with running-light Points (`Coast.js:395,1006,1472`). |
| Harbor | Boats, blinking crane and port lights via instance colour (`Harbor.js`). |
| Freeway | Chaser bulbs via `setColorAt` (`city/freeway.js:924`). |
| City | Rotor, aircraft lights, traffic streams in the shader. |
| Streets | Signal cycle, neon flicker in the shader, steam in the shader, lantern points. |
| Sea | Normal-map scroll (`Sea.js:100`). |
| Crowds | Static: a texture for Raceway, instanced figures plus twinkling Points for Streets. |

## 11. Browser-only render dependencies
- **Canvas 2D everywhere:** `fillText`/`strokeText` in 12 files.
- **System fonts:** "Arial Black", "Arial Narrow", Georgia, "Brush Script MT", "Segoe Script", "Courier New".
- **2D effects:** `shadowBlur` for neon, `roundRect`, `measureText` to shrink-fit text (`world/beach/atlas.js:90`), `createImageData`.
- **Seaside loading:** `atob`, `Blob`, `DecompressionStream('deflate')`, `TextureLoader` for photo.jpg (`levels/seaside/load.js:14-23`).
- **Scenery loading:** dynamic `import()` per scenery module (`World.js:82`).
- **Other:** `renderer.domElement.height` in the City streams shader; HUD minimap canvas.

## 12. Level load flow
- **Build order:** `World.build` (`World.js:29-74`), with `setTimeout(0)` yields and progress callbacks:
  - `level.prepare()` (seaside async data) → `Track`;
  - `Terrain` → scenery `plan()` (may add flattens or carves) → `buildFields` → `resolveFlattens`;
  - terrain meshes (progress 0.1 to 0.65, a yield every 24 tiles);
  - Road → Sky → Sea;
  - each scenery's `async build` (0.72 to 0.98). Most scenery builds yield internally.
- Levels build behind the menu when one is selected (`main.js:250-259`). A broken scenery module logs an error and is skipped.
- **Disposal:** `World.dispose` (`World.js:108-131`) frees every geometry, material and texture under the world root, then the sky dome and lights.
  - Module-level caches survive and are re-uploaded on next use: the texture cache, atlases, car parts/wheel/far caches, shared car materials.
  - `Race.dispose` only removes objects from the scene. The vehicle `handle.dispose()` is never called.

## Hardest to port
1. About 25 onBeforeCompile patches plus the global fog-chunk rewrite. They depend on three's PBR shader internals: `diffuseColor`, `vColor`, `totalEmissiveRadiance`, `roughnessFactor`, `normal`, `directionalLights[0]`, `outgoingLight`.
2. The terrain shader: triplanar, four-scale packed detail, derivative bump, `textureGrad` atlas work, photo drape.
3. Canvas 2D generation with text, fonts, blur and compositing, and getting deterministic output (seeded RNG call order).
4. The CarModel loft and decal system, which leans on three helpers: Lathe, Extrude with bevel, triangulateShape, mergeGeometries, primitive vertex and UV order.
5. Exact parity of the noise and terrain functions, since placement is tied to the rendered triangles.
6. Volume: about 20.5k lines of scenery code.
7. Variable-size points with a minimum pixel size (`gl_PointSize`, `gl_PointCoord`), LineSegments wires, additive sprites.
8. Reliance on `renderOrder` and transparency sorting, plus `polygonOffset` decals.
9. PMREM env map rebuilt from the sky; UnrealBloom's exact look.
10. Per-frame material and instance-colour animation through the night-factor closures.

## Line counts
| Area | Lines |
|---|---|
| Render infrastructure (main 633, World 132, Terrain 623, TerrainMesh 609, Road 679, Sky 286, Sea 157, textures 416) | 3,535 |
| Track, math, roadTypes (feed geometry) | 619 |
| Scenery top-level files: Mountain 1309, Valley 1460, City 1630, Coast 1474, Beach 1396, Harbor 1617, Streets 1688, Desert 1878, Raceway 788 | 13,240 |
| Scenery helpers: valley 998, beach 628, city 1986, desert 1256, coast 277, harbor 408, streets 1527, raceway 175 | 7,255 |
| Vehicles render: CarModel 2377, Vehicle 95 | 2,472 |
| Effects 275, CameraRig 94, PursuitView 445 (partly render) | 814 |
| Level data `.js` files, excluding the embedded seaside blobs | ~1,169 |
