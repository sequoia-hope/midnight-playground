// The scene-wide shading inputs (lights, fog, shadow, environment, sky),
// one row of RGBA32F texels written every frame by `render::globals`. Every
// material built on three_std binds the texture at the same place in its own
// bind group, so the per-frame state lives in one small texture and no
// material is re-prepared when the light moves. The layout is
// `render::globals::G_*`; keep the two in step.

#define_import_path mr::three_globals

const G_SUN_DIR: i32 = 0;       // xyz world direction to the light; w 1 if there is one
const G_SUN_COLOR: i32 = 1;     // rgb colour × intensity; w 1 if it casts a shadow
const G_HEMI_SKY: i32 = 2;      // rgb sky colour × intensity; w 1 if there is one
const G_HEMI_GROUND: i32 = 3;   // rgb ground colour × intensity
const G_HEMI_DIR: i32 = 4;      // xyz world direction; w environment intensity (0: none)
const G_FOG: i32 = 5;           // rgb colour; w FogExp2 density (0: no fog)
const G_SHADOW_M: i32 = 6;      // 6..9: three's shadowMatrix, columns
const G_SHADOW: i32 = 10;       // bias, normal bias, map size, shadow intensity
const G_AMBIENT: i32 = 11;      // rgb ambient light; w exposure
const G_SKY_ZENITH: i32 = 12;
const G_SKY_HORIZON: i32 = 13;
const G_SKY_GROUND: i32 = 14;
const G_SKY_SUN: i32 = 15;
const G_SKY_SUN_DIR: i32 = 16;
const G_SKY_MOON_DIR: i32 = 17;
const G_SKY_PARAMS: i32 = 18;   // uNight, uTime, uCloud, uHaze
const G_SPOT_POS: i32 = 19;     // xyz; w distance
const G_SPOT_DIR: i32 = 20;     // xyz world direction the cone points; w decay
const G_SPOT_COLOR: i32 = 21;   // rgb colour × intensity; w 1 if there is one
const G_SPOT_CONE: i32 = 22;    // coneCos, penumbraCos
const G_POINT_POS: i32 = 23;    // xyz; w distance
const G_POINT_COLOR: i32 = 24;  // rgb colour × intensity; w decay (0 colour: none)
const G_ANIM: i32 = 25;         // road uWet, sea uTime, sea uOff2 (xy)
const G_ANIM2: i32 = 26;        // sea normal-map scroll (xy), glowTime, pixel ratio
