import * as THREE from 'three';
import { mulberry32 } from '../../util/math.js';

// Canvas textures for the port: corrugated steel for sheds and containers.

function canvas(w, h) {
  const c = document.createElement('canvas');
  c.width = w; c.height = h;
  return [c, c.getContext('2d')];
}

function tex(c, repeat = true) {
  const t = new THREE.CanvasTexture(c);
  if (repeat) t.wrapS = t.wrapT = THREE.RepeatWrapping;
  t.colorSpace = THREE.SRGBColorSpace;
  t.anisotropy = 8;
  return t;
}

let corrugated = null;
// Vertical ribs, light-grey so vertex/instance colours tint it.
export function corrugatedTexture() {
  if (corrugated) return corrugated;
  const W = 128, H = 128;
  const [c, g] = canvas(W, H);
  for (let x = 0; x < W; x++) {
    const ph = (x % 16) / 16;
    const v = 200 + Math.sin(ph * Math.PI * 2) * 32 + (ph > 0.9 ? -40 : 0);
    g.fillStyle = `rgb(${v},${v},${v})`;
    g.fillRect(x, 0, 1, H);
  }
  const rng = mulberry32(12);
  // Streaks of grime.
  for (let k = 0; k < 40; k++) {
    g.fillStyle = `rgba(60,50,40,${0.03 + rng() * 0.06})`;
    g.fillRect(rng() * W, rng() * H * 0.4 + H * 0.6, 1 + rng() * 3, rng() * H * 0.5);
  }
  corrugated = tex(c);
  return corrugated;
}

let containerTex = null;
// A 40 ft container side: ribs, a frame, door bars and a stencilled code.
export function containerTexture() {
  if (containerTex) return containerTex;
  const W = 256, H = 64;
  const [c, g] = canvas(W, H);
  for (let x = 0; x < W; x++) {
    const ph = (x % 6) / 6;
    const v = 212 + Math.sin(ph * Math.PI * 2) * 26;
    g.fillStyle = `rgb(${v},${v},${v})`;
    g.fillRect(x, 0, 1, H);
  }
  g.fillStyle = 'rgba(40,40,40,0.55)';
  g.fillRect(0, 0, W, 3); g.fillRect(0, H - 4, W, 4);
  g.fillRect(0, 0, 4, H); g.fillRect(W - 4, 0, 4, H);
  // Door bars at one end.
  g.fillStyle = 'rgba(30,30,30,0.5)';
  for (const x of [W - 30, W - 20, W - 12]) g.fillRect(x, 4, 2, H - 8);
  g.fillStyle = 'rgba(255,255,255,0.75)';
  g.font = 'bold 10px Arial, sans-serif';
  g.fillText('MRDU 402117 4', 12, 16);
  const rng = mulberry32(8);
  for (let k = 0; k < 60; k++) {
    g.fillStyle = `rgba(90,60,40,${0.05 + rng() * 0.12})`;
    g.fillRect(rng() * W, rng() * H, 1 + rng() * 5, 1 + rng() * 6);
  }
  containerTex = tex(c);
  return containerTex;
}

// ── Yard paving ──────────────────────────────────────────────────────
// 16 m of concrete slabs per tile: 4 m panels with sawn joints, per-panel
// tone, patched panels, oil drips and tyre scuffs. Mapped with uS = vS = 16.
let paving = null;
export function pavingTexture() {
  if (paving) return paving;
  const S = 512, P = S / 4;
  const [c, g] = canvas(S, S);
  const rng = mulberry32(404);
  for (let i = 0; i < 4; i++) for (let j = 0; j < 4; j++) {
    const v = 168 + rng() * 26, w = rng() < 0.12 ? -22 : 0; // an occasional patched (darker) panel
    g.fillStyle = `rgb(${v + w},${v + w - 3},${v + w - 8})`;
    g.fillRect(i * P, j * P, P, P);
  }
  // Aggregate speckle.
  for (let k = 0; k < 9000; k++) {
    const v = rng() < 0.5 ? 255 : 0;
    g.fillStyle = `rgba(${v},${v},${v},${0.03 + rng() * 0.05})`;
    g.fillRect(rng() * S, rng() * S, 1 + rng() * 2, 1 + rng() * 2);
  }
  // Oil drips and scuffs, heavier along the lanes (vertical = along the road).
  for (let k = 0; k < 70; k++) {
    g.fillStyle = `rgba(40,34,30,${0.05 + rng() * 0.12})`;
    g.beginPath(); g.ellipse(rng() * S, rng() * S, 3 + rng() * 14, 2 + rng() * 9, rng() * 3, 0, 7); g.fill();
  }
  for (let k = 0; k < 26; k++) {
    const x = rng() * S;
    g.fillStyle = `rgba(30,28,26,${0.04 + rng() * 0.06})`;
    g.fillRect(x, 0, 4 + rng() * 5, S);
  }
  // Hairline cracks.
  g.strokeStyle = 'rgba(50,45,40,0.35)'; g.lineWidth = 1;
  for (let k = 0; k < 14; k++) {
    let x = rng() * S, y = rng() * S;
    g.beginPath(); g.moveTo(x, y);
    for (let q = 0; q < 6; q++) { x += (rng() - 0.5) * 30; y += (rng() - 0.5) * 30; g.lineTo(x, y); }
    g.stroke();
  }
  // Sawn joints between panels.
  g.fillStyle = 'rgba(45,40,36,0.75)';
  for (let k = 0; k <= 4; k++) { g.fillRect(k * P - 1, 0, 2, S); g.fillRect(0, k * P - 1, S, 2); }
  paving = tex(c);
  return paving;
}

// ── Containers ───────────────────────────────────────────────────────
// A mask atlas, not a colour map: R = corrugation shading, G = white paint
// (logos, codes, stripes), B = rust and grime. The container material tints
// R by the instance colour so one texture serves every box. Four variant
// rows (aVar = 0..3), each laid out as side | door end | roof across u.
export const CONTAINER_BRANDS = ['MERIDIAN', 'OCEANIX', 'NORDSTAR', ''];
let containerAtlasTex = null;
export function containerAtlas() {
  if (containerAtlasTex) return containerAtlasTex;
  const W = 1024, RH = 128, H = RH * 4;
  const SW = 768, EW = 192; // side 0..768, end 768..960, roof 960..1024
  const [c, g] = canvas(W, H);
  const rng = mulberry32(4040);
  const letters = 'ABCDEFGHKLMNPRSTU';
  for (let row = 0; row < 4; row++) {
    const y0 = row * RH;
    // Side: trapezoidal corrugation, bright crests and dark valleys.
    for (let x = 0; x < SW; x++) {
      const ph = (x % 10) / 10;
      const v = ph < 0.35 ? 200 : ph < 0.5 ? 150 : ph < 0.85 ? 175 : 215;
      g.fillStyle = `rgb(${v},0,0)`; g.fillRect(x, y0, 1, RH);
    }
    // Door end: vertical ribs, two door leaves, locking bars.
    for (let x = SW; x < SW + EW; x++) {
      const ph = ((x - SW) % 12) / 12;
      const v = ph < 0.5 ? 196 : 170;
      g.fillStyle = `rgb(${v},0,0)`; g.fillRect(x, y0, 1, RH);
    }
    // Roof: plain, slightly dished.
    g.fillStyle = 'rgb(185,0,0)'; g.fillRect(SW + EW, y0, W - SW - EW, RH);
    // Frame rails and corner posts (dark shading).
    g.fillStyle = 'rgb(95,0,0)';
    g.fillRect(0, y0, SW, 7); g.fillRect(0, y0 + RH - 9, SW, 9);
    g.fillRect(0, y0, 8, RH); g.fillRect(SW - 8, y0, 8, RH);
    g.fillRect(SW, y0, EW, 8); g.fillRect(SW, y0 + RH - 9, EW, 9);
    g.fillRect(SW, y0, 9, RH); g.fillRect(SW + EW - 9, y0, 9, RH);
    g.fillRect(SW + EW / 2 - 1, y0 + 8, 2, RH - 17);
    // Locking bars and handles on the doors.
    g.fillStyle = 'rgb(80,0,0)';
    for (const f of [0.18, 0.36, 0.64, 0.82]) g.fillRect(SW + EW * f - 2, y0 + 10, 4, RH - 20);
    g.fillStyle = 'rgb(120,0,0)';
    for (const f of [0.18, 0.36, 0.64, 0.82]) g.fillRect(SW + EW * f - 5, y0 + RH * 0.55, 10, 4);
    // White paint: brand on the side, codes on side and doors.
    g.fillStyle = 'rgb(0,255,0)';
    g.globalCompositeOperation = 'lighter';
    const brand = CONTAINER_BRANDS[row];
    if (brand) {
      g.font = `bold ${row === 1 ? 52 : 58}px Arial, sans-serif`;
      g.textAlign = 'center'; g.textBaseline = 'middle';
      g.fillText(brand, SW * 0.47, y0 + RH * 0.52);
      if (row === 0) { g.fillRect(SW * 0.12, y0 + RH * 0.78, SW * 0.7, 4); }
      if (row === 2) { g.beginPath(); g.arc(SW * 0.13, y0 + RH * 0.52, 18, 0, 7); g.fill(); }
      g.font = `bold 30px Arial, sans-serif`;
      g.fillText(brand, SW + EW / 2, y0 + RH * 0.22);
    }
    g.textAlign = 'left'; g.textBaseline = 'alphabetic';
    g.font = 'bold 13px Arial, sans-serif';
    const code = `${letters[Math.floor(rng() * letters.length)]}${letters[Math.floor(rng() * letters.length)]}${letters[Math.floor(rng() * letters.length)]}U ${String(100000 + Math.floor(rng() * 899999))} ${Math.floor(rng() * 10)}`;
    g.fillText(code, SW - 150, y0 + 26);
    g.fillText(code, SW + 16, y0 + 30);
    g.fillText('45G1', SW + 16, y0 + 46);
    g.font = 'bold 9px Arial, sans-serif';
    g.fillText('MAX GROSS 32500 KG', SW + 16, y0 + RH - 22);
    g.globalCompositeOperation = 'source-over';
    // Rust and grime: drips below the top rail, scrapes, dirty bottom rail.
    g.globalCompositeOperation = 'lighter';
    for (let k = 0; k < 90; k++) {
      const x = rng() * W, y = y0 + 6 + rng() * 10;
      g.fillStyle = `rgba(0,0,255,${0.15 + rng() * 0.35})`;
      g.fillRect(x, y, 1 + rng() * 2, 4 + rng() * 40);
    }
    for (let k = 0; k < 50; k++) {
      g.fillStyle = `rgba(0,0,255,${0.1 + rng() * 0.3})`;
      g.fillRect(rng() * W, y0 + rng() * RH, 2 + rng() * 14, 1 + rng() * 3);
    }
    const grd = g.createLinearGradient(0, y0 + RH - 30, 0, y0 + RH);
    grd.addColorStop(0, 'rgba(0,0,255,0)'); grd.addColorStop(1, 'rgba(0,0,255,0.45)');
    g.fillStyle = grd; g.fillRect(0, y0 + RH - 30, W, 30);
    g.globalCompositeOperation = 'source-over';
  }
  containerAtlasTex = tex(c, false);
  containerAtlasTex.colorSpace = THREE.NoColorSpace;
  return containerAtlasTex;
}

// Unit container (1 × 1 × 1, bottom at y = 0, length along local X) whose
// faces map onto the atlas regions above.
export function containerGeometry() {
  const g = new THREE.BoxGeometry(1, 1, 1);
  g.translate(0, 0.5, 0);
  const uv = g.getAttribute('uv');
  // BoxGeometry face order: +x, -x, +y, -y, +z, -z (4 vertices each).
  const region = [[0.75, 0.9375], [0.75, 0.9375], [0.9375, 1], [0.9375, 1], [0, 0.75], [0, 0.75]];
  for (let f = 0; f < 6; f++) {
    const [u0, u1] = region[f];
    for (let k = 0; k < 4; k++) {
      const i = f * 4 + k;
      uv.setX(i, u0 + uv.getX(i) * (u1 - u0));
      uv.setY(i, 0.02 + uv.getY(i) * 0.96);
    }
  }
  return g;
}

export function containerMaterial() {
  const m = new THREE.MeshStandardMaterial({ map: containerAtlas(), roughness: 0.7, metalness: 0.25 });
  m.onBeforeCompile = (sh) => {
    sh.vertexShader = sh.vertexShader
      .replace('#include <common>', '#include <common>\nattribute float aVar;')
      .replace('#include <uv_vertex>', '#include <uv_vertex>\n#ifdef USE_MAP\n  vMapUv.y = (vMapUv.y + aVar) * 0.25;\n#endif');
    sh.fragmentShader = sh.fragmentShader
      .replace('#include <map_fragment>', 'vec4 cT_ = texture2D(map, vMapUv);')
      .replace('#include <color_fragment>', `
        vec3 body_ = vec3(1.0);
        #ifdef USE_COLOR
          body_ = vColor.rgb;
        #endif
        float sh_ = pow(cT_.r, 2.2) * 1.9;
        diffuseColor.rgb *= mix(body_ * sh_, vec3(0.8), cT_.g * 0.9) * mix(vec3(1.0), vec3(0.42, 0.26, 0.16), cT_.b * 0.8);`);
  };
  m.customProgramCacheKey = () => 'harbor-container';
  return m;
}

// Roller door: horizontal slats, light grey for tinting.
let roller = null;
export function rollerDoorTexture() {
  if (roller) return roller;
  const [c, g] = canvas(64, 128);
  for (let y = 0; y < 128; y++) {
    const ph = (y % 8) / 8;
    const v = ph < 0.15 ? 150 : 205 + Math.sin(ph * Math.PI) * 20;
    g.fillStyle = `rgb(${v},${v},${v})`; g.fillRect(0, y, 64, 1);
  }
  g.fillStyle = 'rgba(60,60,60,0.8)'; g.fillRect(0, 0, 3, 128); g.fillRect(61, 0, 3, 128); g.fillRect(0, 120, 64, 8);
  roller = tex(c);
  return roller;
}
