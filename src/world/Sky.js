import * as THREE from 'three';
import { clamp, lerp, smoothstep } from '../util/math.js';
import { terrainDetailTexture } from './textures.js';

// Sky dome, sun/moon light, fog and exposure — all driven by how far along
// the route the player is, using the level's time-of-day keys (Level 1 runs
// from afternoon sun to midnight, Level 2 from blue hour to morning).

// Time-of-day keys come from the level (see src/levels/*.js).
const COLOR_KEYS = ['zen', 'hor', 'sun', 'hemiS', 'hemiG', 'fog'];
function prepKeys(keys) {
  return keys.map((k) => {
    const o = { ...k };
    for (const c of COLOR_KEYS) o[c + 'C'] = new THREE.Color(k[c]);
    return o;
  });
}

// Aerial perspective for every fogged material: looking toward the sun (or
// the moon, once the key light has crossed over) the fog picks up the light's
// colour, so distant hills glow at sunset the way the sky behind them does.
// Patched into the shared chunks once, rather than per material, so scenery
// built by any module gets it for free. The sun lookup only exists in lit
// shaders (lights_pars_begin declares directionalLights); everything else
// keeps plain fog.
if (!THREE.ShaderChunk.fog_fragment.includes('MR_SUN_FOG')) {
  const C = THREE.ShaderChunk;
  C.fog_pars_vertex += '\n#ifdef USE_FOG\n\tvarying vec3 vFogView;\n#endif';
  C.fog_vertex += '\n#ifdef USE_FOG\n\tvFogView = mvPosition.xyz;\n#endif';
  C.fog_pars_fragment += '\n#ifdef USE_FOG\n\tvarying vec3 vFogView;\n#endif';
  C.lights_pars_begin += '\n#if NUM_DIR_LIGHTS > 0\n\t#define MR_SUN_FOG\n#endif';
  C.fog_fragment = C.fog_fragment.replace('gl_FragColor.rgb = mix( gl_FragColor.rgb, fogColor, fogFactor );', `vec3 fogTint = fogColor;
	#ifdef MR_SUN_FOG
		float fogSun = clamp( dot( vFogView, directionalLights[ 0 ].direction ) / max( length( vFogView ), 1e-3 ), 0.0, 1.0 );
		float fogSun4 = fogSun * fogSun * fogSun * fogSun;
		float fogSun24 = fogSun4 * fogSun4 * fogSun4; fogSun24 *= fogSun24;
		fogTint += min( directionalLights[ 0 ].color, vec3( 4.0 ) ) * ( fogSun4 * 0.035 + fogSun24 * 0.07 );
	#endif
	gl_FragColor.rgb = mix( gl_FragColor.rgb, fogTint, fogFactor );`);
}

const skyVert = /* glsl */`
varying vec3 vDir;
void main() {
  vDir = normalize(position);
  vec4 p = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
  gl_Position = p.xyww;
}`;

// Everything is procedural: a gradient with a sun-side glow and horizon haze
// that matches the fog, two cloud layers (puffy cumulus shaded toward the
// sun, and thin cirrus streaks), sun disc, moon with maria, stars and a faint
// Milky Way. Hashes avoid sin() so it stays cheap on phone GPUs.
const skyFrag = /* glsl */`
uniform vec3 uZenith, uHorizon, uGround, uSunColor;
uniform vec3 uSunDir, uMoonDir;
uniform float uNight, uTime, uCloud, uHaze;
varying vec3 vDir;

float hash(vec3 p) { p = fract(p * 0.3183099 + 0.1); p *= 17.0; return fract(p.x * p.y * p.z * (p.x + p.y + p.z)); }
float hash2(vec2 p) { vec3 p3 = fract(vec3(p.xyx) * 0.1031); p3 += dot(p3, p3.yzx + 33.33); return fract((p3.x + p3.y) * p3.z); }
float vnoise(vec2 p) {
  vec2 i = floor(p), f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash2(i), hash2(i + vec2(1, 0)), u.x), mix(hash2(i + vec2(0, 1)), hash2(i + vec2(1, 1)), u.x), u.y);
}
// Cloud noise from the shared tileable noise texture (broad noise in A,
// multi-octave detail in R): two fetches instead of dozens of hashes.
uniform sampler2D tNoise;
float cloudN(vec2 q) {
  float a = texture2D(tNoise, q * 0.05).a;
  float b = (texture2D(tNoise, q * 0.23 + 0.31).r - 0.59) * 2.4;
  return a * 0.62 + b * 0.38;
}

void main() {
  vec3 d = normalize(vDir);
  float h = d.y;
  float sd = dot(d, uSunDir);
  float sunAmt = max(sd, 0.0);
  float t = pow(clamp(h, 0.0, 1.0), 0.45);
  vec3 col = mix(uHorizon, uZenith, t);
  // Sun-side glow: a broad warm wash along the horizon plus a tighter halo.
  float sun2 = sunAmt * sunAmt;
  col += uSunColor * ((sun2 * sun2 * sun2) * 0.35 * (1.0 - t) + sun2 * 0.08 * (1.0 - t) * (1.0 - t));
  // Twilight arch: while the sun is just below the horizon, a warm band
  // hugs the horizon on its side of the sky (and a faint pink one opposite,
  // the Belt of Venus), so blue hour isn't a flat gradient.
  float twi = smoothstep(-0.22, -0.02, uSunDir.y) * (1.0 - smoothstep(0.03, 0.12, uSunDir.y));
  if (twi > 0.0) {
    float az = dot(normalize(d.xz + vec2(1e-4)), normalize(uSunDir.xz + vec2(1e-4)));
    float low = 1.0 - smoothstep(0.0, 0.28, max(h, 0.0));
    col += vec3(1.0, 0.5, 0.3) * twi * low * low * pow(max(az, 0.0), 3.0) * 0.16;
    col += vec3(0.5, 0.3, 0.42) * twi * (1.0 - smoothstep(0.02, 0.2, abs(h - 0.08))) * max(-az, 0.0) * 0.05;
  }
  // Horizon haze: the band just above the horizon fades into the fog colour
  // (plus the same sun tint the fog chunk adds), so fogged far terrain meets
  // the sky without a seam.
  vec3 hazeCol = uGround + uSunColor * (sun2 * sun2 * 0.06 + pow(sunAmt, 24.0) * 0.1);
  float haze = exp(-max(h, 0.0) * 16.0) * uHaze;
  col = mix(col, hazeCol, haze);
  // Below the horizon fade to the fog/ground tone.
  col = mix(col, hazeCol, smoothstep(0.0, -0.06, h));

  // Clouds.
  float cloudCover = 0.0;
  if (h > 0.0 && uCloud > 0.01) {
    vec2 cp = d.xz / (h + 0.09);
    vec2 wind = vec2(uTime * 0.005, uTime * 0.0018);
    float fade = smoothstep(0.0, 0.14, h);
    // Cumulus: domain-warped fbm, shaded by comparing density one step
    // toward the sun (thinner toward the sun = lit side).
    vec2 q = cp * 1.35 + wind;
    q += (texture2D(tNoise, q * 0.021 + 0.7).a - 0.5) * 1.2;
    float n = cloudN(q);
    vec2 toSun = normalize(uSunDir.xz + vec2(1e-4)) * 0.3;
    float n2 = cloudN(q + toSun);
    float cov = mix(0.6, 0.42, uCloud);
    float dens = smoothstep(cov, cov + 0.22, n);
    float lit = clamp(0.55 + (n - n2) * 5.0, 0.0, 1.0);
    // Base (shadowed) and lit tones: sky-tinted greys by day, sun-coloured
    // at dawn/dusk, deep blue at night with moonlit edges.
    vec3 shadowC = mix(uZenith, uHorizon, 0.55) * 0.72 + 0.03;
    vec3 litC = uSunColor * 0.95 + uHorizon * 0.35 + 0.04;
    float day = 1.0 - uNight;
    vec3 cc = mix(shadowC, litC, lit * mix(0.35, 1.0, day));
    // Silver lining: thin edges near the sun glow.
    cc += uSunColor * pow(sunAmt, 10.0) * (1.0 - dens) * 1.6;
    cc = mix(cc, uHorizon * 0.35 + vec3(0.02, 0.025, 0.05) + vec3(0.1, 0.11, 0.14) * lit * pow(max(dot(d, uMoonDir), 0.0), 6.0), uNight * 0.85);
    // Distant clouds sink into the haze.
    cc = mix(cc, hazeCol, (1.0 - smoothstep(0.02, 0.35, h)) * 0.6);
    // Cirrus: high thin streaks, stretched along the wind.
    vec2 cq = d.xz / (h + 0.3);
    vec2 cs = vec2(cq.x * 1.2 + cq.y * 0.4, cq.y * 5.0 - cq.x * 1.5) + wind * 2.5;
    float ci = texture2D(tNoise, cs * 0.06 + 0.13).a * 0.7 + (texture2D(tNoise, cs * 0.25).r - 0.59) * 0.8;
    ci = smoothstep(0.55, 0.85, ci) * 0.4 * (1.0 - dens);
    vec3 ciC = mix(uHorizon * 1.05 + 0.05, uSunColor * 0.8 + uHorizon * 0.4, pow(sunAmt, 2.0));
    ciC = mix(ciC, uHorizon * 0.5, uNight * 0.8);
    col = mix(col, ciC, ci * uCloud * fade);
    cloudCover = clamp((dens * min(uCloud * 1.25, 1.0) + ci * uCloud) * fade, 0.0, 1.0);
    col = mix(col, cc, dens * min(uCloud * 1.25, 1.0) * fade);
  }

  // Sun disc and glow.
  col += uSunColor * (smoothstep(0.9993, 0.9997, sd) * 6.0 + pow(sunAmt, 180.0) * 0.8 + pow(sunAmt, 32.0) * 0.18) * step(-0.02, h) * (1.0 - cloudCover * 0.8);
  // Moon: disc with dusky maria, plus a soft halo.
  float md = dot(d, uMoonDir);
  float disc = smoothstep(0.99955, 0.99975, md);
  float maria = 0.0;
  if (md > 0.9995) maria = vnoise(d.xz * 1400.0 + d.y * 900.0) * 0.35 + vnoise(d.xz * 3100.0) * 0.15;
  col += vec3(0.85, 0.9, 1.0) * uNight * (1.0 - cloudCover * 0.7) * (disc * (2.4 - maria * 1.6) + pow(max(md, 0.0), 400.0) * 0.25 + pow(max(md, 0.0), 30.0) * 0.05);
  // Stars (twinkling, slightly tinted) and the Milky Way band.
  if (uNight > 0.5 && h > 0.0) {
    float nightSky = smoothstep(0.5, 0.95, uNight) * smoothstep(0.0, 0.25, h);
    vec3 sp = d * 420.0;
    vec3 cell = floor(sp);
    float r = hash(cell);
    float star = step(0.9965, r) * smoothstep(0.55, 0.0, length(fract(sp) - 0.5));
    float tw = 0.7 + 0.3 * sin(uTime * 3.0 + r * 60.0);
    vec3 tint = mix(vec3(1.0, 0.85, 0.7), vec3(0.75, 0.85, 1.0), fract(r * 91.0));
    // A band across the sky, tilted: faint glow plus a denser star field.
    float bq = dot(d, normalize(vec3(0.35, 0.45, -0.82))) * 5.0;
    float band = exp(-bq * bq);
    float mw = band * (0.5 + texture2D(tNoise, d.xz / (h + 0.4) * 0.4 + d.y).r) * 0.5;
    float faint = step(0.985 - band * 0.01, hash(floor(d * 900.0))) * 0.35;
    col += (tint * star * tw * 1.4 + vec3(0.8, 0.85, 1.0) * faint * band + vec3(0.07, 0.075, 0.1) * mw) * nightSky * (1.0 - cloudCover);
  }
  gl_FragColor = vec4(col, 1.0);
  #include <tonemapping_fragment>
  #include <colorspace_fragment>
}`;

export class Sky {
  constructor(scene, renderer, track, keys, sunAzimuth = 0.2) {
    this.keys = prepKeys(keys);
    this.scene = scene;
    this.renderer = renderer;
    this.track = track;
    this.time = 0;
    this.night = 0;
    this.override = null; // set to a fraction to pin time of day

    this.uniforms = {
      uZenith: { value: new THREE.Color() },
      uHorizon: { value: new THREE.Color() },
      uGround: { value: new THREE.Color() },
      uSunColor: { value: new THREE.Color() },
      uSunDir: { value: new THREE.Vector3(0, 1, 0) },
      uMoonDir: { value: new THREE.Vector3(...(track.level.moonDir || [-0.3, 0.55, 0.7])).normalize() },
      uNight: { value: 0 },
      uTime: { value: 0 },
      uCloud: { value: 0.7 },
      uHaze: { value: 0.8 },
      tNoise: { value: terrainDetailTexture() },
    };
    const geo = new THREE.SphereGeometry(1, 48, 24);
    const mat = new THREE.ShaderMaterial({
      vertexShader: skyVert, fragmentShader: skyFrag, uniforms: this.uniforms,
      side: THREE.BackSide, depthWrite: false, depthTest: true, fog: false,
    });
    mat.userData.kind = 'SkyDome'; // MaterialKind for the scene export
    this.dome = new THREE.Mesh(geo, mat);
    this.dome.frustumCulled = false;
    this.dome.renderOrder = -10;
    this.dome.scale.setScalar(5000);
    scene.add(this.dome);

    this.sun = new THREE.DirectionalLight(0xffffff, 2.5);
    this.sun.castShadow = true;
    const sc = this.sun.shadow.camera;
    sc.left = -70; sc.right = 70; sc.top = 70; sc.bottom = -70; sc.near = 1; sc.far = 600;
    this.sun.shadow.mapSize.set(2048, 2048);
    this.sun.shadow.bias = -0.0004;
    this.sun.shadow.normalBias = 0.6;
    scene.add(this.sun, this.sun.target);

    this.hemi = new THREE.HemisphereLight(0xffffff, 0x444444, 1);
    scene.add(this.hemi);

    scene.fog = new THREE.FogExp2(0xffffff, 0.0003);
    this.sunAzimuth = sunAzimuth; // radians around Y; 0 = ahead along +X
    this._k = {};
  }

  // Interpolated key at fraction p.
  sample(p) {
    const K = this.keys;
    let a = K[0], b = K[K.length - 1];
    for (let i = 0; i < K.length - 1; i++) {
      if (p >= K[i].s && p <= K[i + 1].s) { a = K[i]; b = K[i + 1]; break; }
    }
    const t = smoothstep(a.s, b.s, p);
    const k = this._k;
    for (const key of ['sunEl', 'sunI', 'hemiI', 'fogD', 'exp', 'night']) k[key] = lerp(a[key], b[key], t);
    for (const c of COLOR_KEYS) {
      if (!k[c]) k[c] = new THREE.Color();
      k[c].copy(a[c + 'C']).lerp(b[c + 'C'], t);
    }
    return k;
  }

  update(dt, s, focus) {
    this.time += dt;
    const p = this.override ?? (this.track.loop ? 0.5 : clamp(s / this.track.length, 0, 1));
    const k = this.sample(p);
    this.night = k.night;
    const u = this.uniforms;
    u.uZenith.value.copy(k.zen);
    u.uHorizon.value.copy(k.hor);
    u.uGround.value.copy(k.fog);
    u.uSunColor.value.copy(k.sun).multiplyScalar(clamp(k.sunI / 2.5, 0.25, 1.2) * (1 - k.night * 0.8));
    u.uNight.value = k.night;
    u.uTime.value = this.time;
    u.uCloud.value = lerp(0.75, 0.35, k.night);
    // Thicker fog, deeper horizon haze.
    u.uHaze.value = clamp(k.fogD / 0.00032, 0.45, 1.0) * 0.85;

    const el = (k.sunEl * Math.PI) / 180;
    const az = this.sunAzimuth;
    const sunDir = new THREE.Vector3(Math.cos(el) * Math.cos(az), Math.sin(el), Math.cos(el) * Math.sin(az));
    u.uSunDir.value.copy(sunDir);

    // Directional light: sun by day, crossfading to moonlight once the sun
    // is gone. Keep it above the horizon so shadows stay sane.
    const moonDir = u.uMoonDir.value;
    const toMoon = smoothstep(0.3, 0.75, k.night);
    const lightDir = sunDir.clone();
    lightDir.y = Math.max(lightDir.y, 0.12);
    lightDir.lerp(moonDir, toMoon).normalize();
    this.sun.color.copy(k.sun);
    this.sun.intensity = k.sunI;
    if (focus) {
      this.sun.position.set(focus.x + lightDir.x * 300, focus.y + lightDir.y * 300, focus.z + lightDir.z * 300);
      this.sun.target.position.copy(focus);
      // Snap the shadow camera to texels to stop shimmering.
      this.sun.target.updateMatrixWorld();
    }
    this.hemi.color.copy(k.hemiS);
    this.hemi.groundColor.copy(k.hemiG);
    this.hemi.intensity = k.hemiI;
    this.scene.fog.color.copy(k.fog);
    this.scene.fog.density = k.fogD;
    this.renderer.toneMappingExposure = k.exp;
    this.dome.position.copy(focus || this.dome.position);
    return k;
  }
}
