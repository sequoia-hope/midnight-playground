// Font gallery (roadmap WP 3.2, SPEC 5.3): the game's own sign and texture
// strings drawn by mp_canvas with each candidate substitute for the system
// fonts the JS names, so the owner can pick the bundled set.
//
//   node tools/parity/fonts-gallery.mjs
//
// The bundled defaults are in assets/fonts/ (fonts.json; the owner chose
// the Roboto family for the Arials, DECISIONS D370). The alternatives
// are fetched from the google/fonts repository into target/font-candidates/
// (once; build output, not committed). The page and its PNGs go to
// parity/report/fonts/, viewed through the registered server at
// /parity/report/fonts/.

import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { ROOT } from './lib/jstree.mjs';

const GF = 'https://raw.githubusercontent.com/google/fonts/main/';
const CACHE = path.join(ROOT, 'target/font-candidates');
const OUT = path.join(ROOT, 'parity/report/fonts');

// A candidate: its faces as [file, [weight range], style]. A file under
// assets/fonts/ is bundled; anything else is a google/fonts path.
const B = (file, w, style = 'normal') => ({ file: 'assets/fonts/' + file, weight: w, style });
const G = (file, w, style = 'normal') => ({ file: 'gf:' + file, weight: w, style });
const LICENCE = { 'gf:ofl': 'OFL', 'gf:apache': 'Apache 2.0' };

export const ROLES = [
  {
    family: 'Arial Narrow', why: 'Road and harbour signs, Mountain and Coast lettering (bold, italic 900)',
    candidates: [
      { name: 'Roboto Condensed', faces: [B('robotocondensed/RobotoCondensed[wght].ttf', [100, 900]), B('robotocondensed/RobotoCondensed-Italic[wght].ttf', [100, 900], 'italic')] },
      { name: 'Archivo Narrow', faces: [G('ofl/archivonarrow/ArchivoNarrow[wght].ttf', [400, 700]), G('ofl/archivonarrow/ArchivoNarrow-Italic[wght].ttf', [400, 700], 'italic')] },
      { name: 'Barlow Condensed', faces: [G('ofl/barlowcondensed/BarlowCondensed-Bold.ttf', [700, 700]), G('ofl/barlowcondensed/BarlowCondensed-BoldItalic.ttf', [700, 700], 'italic'), G('ofl/barlowcondensed/BarlowCondensed-Black.ttf', [900, 900]), G('ofl/barlowcondensed/BarlowCondensed-BlackItalic.ttf', [900, 900], 'italic')] },
    ],
    samples: [
      { lines: [['EXIT 8', 'bold 64px "Arial Narrow", Arial, sans-serif'], ['Grand Ave', 'bold 64px "Arial Narrow", Arial, sans-serif']], bg: '#0b6b3a', fg: '#fff', border: '#fff', w: 512, h: 200 },
      { lines: [['PORT MERIDIAN', 'bold 54px "Arial Narrow", Arial, sans-serif'], ['TERMINAL GATE', 'bold 54px "Arial Narrow", Arial, sans-serif']], bg: '#123a52', fg: '#fff', border: '#fff', w: 512, h: 160 },
      { lines: [['HOLLOW PT. COFFEE', 'italic 900 54px "Arial Narrow", "Helvetica Neue", Arial, sans-serif'], ['SURF · COFFEE · BAIT', 'bold 24px "Arial Narrow", "Helvetica Neue", Arial, sans-serif']], bg: '#14343a', fg: '#ffb347', w: 512, h: 128 },
      { lines: [['ROAD CLOSED', 'bold 84px "Arial Narrow", Arial']], bg: '#f2f2f2', fg: '#111', border: '#111', w: 512, h: 192 },
    ],
  },
  {
    family: 'Arial', why: 'Sub-lines, price boards, banners, harbour signs (bold)',
    candidates: [
      { name: 'Roboto', faces: [B('roboto/Roboto[wdth,wght].ttf', [100, 900])] },
      { name: 'Arimo', faces: [G('ofl/arimo/Arimo[wght].ttf', [400, 700]), G('ofl/arimo/Arimo-Italic[wght].ttf', [400, 700], 'italic')] },
    ],
    samples: [
      { lines: [['MERIDIAN STAR', 'bold 64px Arial, sans-serif']], bg: '#1f3550', fg: '#fff', w: 512, h: 96 },
      { lines: [['WELCOME TO', 'bold 42px Arial'], ['pop. 4,210  ·  est. 1912', 'bold 40px Arial']], bg: '#1f7f8c', fg: '#fff', w: 640, h: 140 },
      { lines: [['FUEL', 'bold 44px Arial'], ['NEXT SERVICES 98 MILES', 'bold 56px Arial']], bg: '#c83a34', fg: '#fff', w: 768, h: 150 },
    ],
  },
  {
    family: 'Arial Black', why: 'Shop fronts, billboards, neon (bold)',
    candidates: [
      { name: 'Roboto Black', faces: [B('roboto/Roboto[wdth,wght].ttf', [900, 900])] },
      { name: 'Archivo Black', faces: [G('ofl/archivoblack/ArchivoBlack-Regular.ttf', [100, 900])] },
      { name: 'Rubik Black', faces: [G('ofl/rubik/Rubik[wght].ttf', [900, 900])] },
    ],
    samples: [
      { lines: [['TACOS', 'bold 96px "Arial Black", Arial']], bg: '#f2c233', fg: '#c83a34', w: 384, h: 128 },
      { lines: [['SURF SHOP', 'bold 84px "Arial Black", Arial']], bg: '#1f7f8c', fg: '#fff4d8', w: 640, h: 128 },
      { lines: [['LAST GAS', 'bold 130px "Arial Black", Arial']], bg: '#f2e8d0', fg: '#b8322a', w: 760, h: 170 },
      { lines: [['ARCADE', 'bold 104px "Arial Black", Arial']], bg: '#120a1e', fg: '#ffd84d', glow: true, w: 512, h: 150 },
    ],
  },
  {
    family: 'Georgia', why: 'Wood signs, cafes, motels, billboards (bold, italic bold)',
    candidates: [
      { name: 'Gelasio', faces: [B('gelasio/Gelasio[wght].ttf', [400, 700]), B('gelasio/Gelasio-Italic[wght].ttf', [400, 700], 'italic')] },
      { name: 'Lora', faces: [G('ofl/lora/Lora[wght].ttf', [400, 700]), G('ofl/lora/Lora-Italic[wght].ttf', [400, 700], 'italic')] },
      { name: 'Noto Serif', faces: [G('ofl/notoserif/NotoSerif[wdth,wght].ttf', [100, 900]), G('ofl/notoserif/NotoSerif-Italic[wdth,wght].ttf', [100, 900], 'italic')] },
    ],
    samples: [
      { lines: [['GENERAL STORE', 'bold 74px Georgia, serif'], ['MILL VALLEY  ·  FEED · SEED · GAS', 'bold 30px Georgia, serif']], bg: '#7a2e22', fg: '#f1e2c0', w: 768, h: 160 },
      { lines: [['BOARDWALK CAFE', 'bold 70px Georgia, serif']], bg: '#3b2a20', fg: '#f3dfb8', w: 640, h: 128 },
      { lines: [['HOME MADE PIE', 'italic bold 110px Georgia, serif']], bg: '#f4ece0', fg: '#2e5a8a', w: 1016, h: 160 },
    ],
  },
  {
    family: 'Brush Script MT', why: 'Ice cream, welcome boards, neon script (bold)',
    candidates: [
      { name: 'Yellowtail', faces: [B('yellowtail/Yellowtail-Regular.ttf', [100, 900])] },
      { name: 'Caveat', faces: [B('caveat/Caveat[wght].ttf', [400, 700])] },
      { name: 'Kaushan Script', faces: [G('ofl/kaushanscript/KaushanScript-Regular.ttf', [100, 900])] },
      { name: 'Pacifico', faces: [G('ofl/pacifico/Pacifico-Regular.ttf', [100, 900])] },
    ],
    samples: [
      { lines: [['ICE CREAM', 'bold 80px "Brush Script MT", cursive']], bg: '#f7d6e3', fg: '#c2346c', w: 512, h: 128 },
      { lines: [['Seabright', 'bold 120px "Brush Script MT", "Segoe Script", cursive']], bg: '#1f7f8c', fg: '#fff', w: 640, h: 170 },
      { lines: [['Starlite Diner', 'bold 86px "Brush Script MT", "Segoe Script", cursive']], bg: '#120a1e', fg: '#ff5fd2', glow: true, w: 640, h: 150 },
    ],
  },
  {
    family: 'Courier New', why: 'Fuel price boards (bold)',
    candidates: [
      { name: 'Courier Prime', faces: [B('courierprime/CourierPrime-Regular.ttf', [400, 400]), B('courierprime/CourierPrime-Bold.ttf', [700, 700])] },
      { name: 'Cousine', faces: [G('ofl/cousine/Cousine-Regular.ttf', [400, 400]), G('ofl/cousine/Cousine-Bold.ttf', [700, 700])] },
    ],
    samples: [
      { lines: [['REG 4.39', 'bold 40px "Courier New", monospace'], ['PLS 4.59', 'bold 40px "Courier New", monospace'], ['DSL 4.79', 'bold 40px "Courier New", monospace']], bg: '#fff', fg: '#111', w: 256, h: 190 },
    ],
  },
  {
    family: 'Rajdhani', why: 'The HUD (CSS and its canvas dials: 600 and 700)',
    candidates: [
      { name: 'Rajdhani', faces: [B('rajdhani/Rajdhani-SemiBold.ttf', [600, 600]), B('rajdhani/Rajdhani-Bold.ttf', [700, 700])] },
      { name: 'Barlow Condensed', faces: [G('ofl/barlowcondensed/BarlowCondensed-SemiBold.ttf', [600, 600]), G('ofl/barlowcondensed/BarlowCondensed-Bold.ttf', [700, 700])] },
    ],
    samples: [
      { lines: [['188', '700 72px Rajdhani, "Arial Narrow", sans-serif'], ['KM/H  ·  LAP 2/3  ·  0:42.18', '600 26px Rajdhani, "Arial Narrow", sans-serif']], bg: '#0d1117', fg: '#e8f4ff', w: 480, h: 150 },
      { lines: [['N  E  S  W', '600 13px Rajdhani, Arial Narrow, sans-serif'], ['POS 3/8', '700 10px Rajdhani, Arial Narrow, sans-serif']], bg: '#0d1117', fg: 'rgba(255,255,255,0.7)', w: 160, h: 48 },
    ],
  },
];

async function fetchFace(f) {
  if (!f.file.startsWith('gf:')) return path.join(ROOT, f.file);
  const rel = f.file.slice(3);
  const dst = path.join(CACHE, rel);
  if (!fs.existsSync(dst)) {
    fs.mkdirSync(path.dirname(dst), { recursive: true });
    const url = GF + rel.split('/').map(encodeURIComponent).join('/');
    const r = await fetch(url);
    if (!r.ok) throw new Error(`fetch ${url}: HTTP ${r.status}`);
    fs.writeFileSync(dst, Buffer.from(await r.arrayBuffer()));
    // Its licence beside it.
    const dir = rel.split('/').slice(0, 2).join('/');
    for (const lic of ['OFL.txt', 'LICENSE.txt']) {
      const lr = await fetch(GF + dir + '/' + lic);
      if (lr.ok) { fs.writeFileSync(path.join(CACHE, dir, lic), Buffer.from(await lr.arrayBuffer())); break; }
    }
    console.log('  fetched', rel);
  }
  return dst;
}

const spec = { out: OUT, roles: [] };
for (const role of ROLES) {
  const cands = [];
  for (const c of role.candidates) {
    const faces = [];
    for (const f of c.faces) faces.push({ path: await fetchFace(f), weight: f.weight, style: f.style });
    const src = c.faces[0].file;
    cands.push({ name: c.name, bundled: !src.startsWith('gf:'), licence: src.startsWith('gf:') ? LICENCE[src.slice(0, 6)] ?? 'see licence' : 'bundled', source: src.startsWith('gf:') ? 'https://github.com/google/fonts/tree/main/' + src.slice(3).split('/').slice(0, 2).join('/') : src, faces });
  }
  spec.roles.push({ family: role.family, why: role.why, candidates: cands, samples: role.samples });
}
fs.mkdirSync(OUT, { recursive: true });
const specFile = path.join(CACHE, 'gallery.json');
fs.mkdirSync(CACHE, { recursive: true });
fs.writeFileSync(specFile, JSON.stringify(spec, null, 1));
execFileSync('cargo', ['run', '--release', '-q', '-p', 'mp_canvas', '--example', 'font-gallery', '--', specFile], { cwd: ROOT, stdio: 'inherit' });
console.log(`font gallery: ${path.relative(ROOT, OUT)}/index.html (served at /parity/report/fonts/)`);
