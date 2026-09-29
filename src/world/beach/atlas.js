import * as THREE from 'three';

// Signs for the whole town are drawn into two canvas atlases (painted signs
// and neon), so every storefront, pole sign and billboard in Seabright
// shares two materials instead of one each.

export class SignAtlas {
  constructor(size = 2048, bg = null) {
    this.S = size;
    this.c = document.createElement('canvas');
    this.c.width = this.c.height = size;
    this.g = this.c.getContext('2d');
    if (bg) { this.g.fillStyle = bg; this.g.fillRect(0, 0, size, size); }
    this.x = 0; this.y = 0; this.rowH = 0;
    this.pad = 4;
  }

  // Reserve a w×h pixel cell, draw into it, return its UV rectangle.
  add(w, h, draw) {
    const S = this.S, p = this.pad;
    if (this.x + w + p > S) { this.x = 0; this.y += this.rowH + p; this.rowH = 0; }
    if (this.y + h > S) throw new Error('sign atlas full');
    const x = this.x, y = this.y;
    this.x += w + p;
    this.rowH = Math.max(this.rowH, h);
    const g = this.g;
    g.save();
    g.translate(x, y);
    g.beginPath(); g.rect(0, 0, w, h); g.clip();
    draw(g, w, h);
    g.restore();
    // Inset half a texel so mipmaps don't bleed neighbours in.
    const e = 1.5;
    return { u0: (x + e) / S, u1: (x + w - e) / S, v0: 1 - (y + h - e) / S, v1: 1 - (y + e) / S, aspect: w / h };
  }

  texture() {
    const t = new THREE.CanvasTexture(this.c);
    t.colorSpace = THREE.SRGBColorSpace;
    t.anisotropy = 8;
    return t;
  }
}

// A plane w×h (metres) textured with an atlas cell; front faces +Z.
export function signGeometry(rect, w, h) {
  const g = new THREE.PlaneGeometry(w, h);
  const uv = g.getAttribute('uv');
  for (let i = 0; i < uv.count; i++) {
    uv.setXY(i, rect.u0 + uv.getX(i) * (rect.u1 - rect.u0), rect.v0 + uv.getY(i) * (rect.v1 - rect.v0));
  }
  return g;
}

// ── Drawing helpers ───────────────────────────────────────────────
export function paintedSign(text, { bg = '#f4efe2', fg = '#1f5f8b', border = null, font = 'bold 80px "Arial Black", Arial, sans-serif', sub = null, subFont = 'bold 34px Arial, sans-serif', stripe = null } = {}) {
  return (g, w, h) => {
    g.fillStyle = bg; g.fillRect(0, 0, w, h);
    if (stripe) { g.fillStyle = stripe; g.fillRect(0, h - h * 0.14, w, h * 0.14); g.fillRect(0, 0, w, h * 0.08); }
    if (border) { g.strokeStyle = border; g.lineWidth = Math.max(6, h * 0.06); g.strokeRect(g.lineWidth / 2, g.lineWidth / 2, w - g.lineWidth, h - g.lineWidth); }
    g.fillStyle = fg; g.textAlign = 'center'; g.textBaseline = 'middle';
    g.font = font;
    fitText(g, text, w * 0.9);
    g.fillText(text, w / 2, sub ? h * 0.42 : h / 2);
    if (sub) { g.font = subFont; fitText(g, sub, w * 0.9); g.fillText(sub, w / 2, h * 0.76); }
  };
}

export function neonSign(text, { color = '#ff4fa3', glow = null, font = 'bold 86px "Brush Script MT", "Segoe Script", cursive', sub = null, subColor = '#62f0ff', subFont = 'bold 40px Arial, sans-serif', frame = true } = {}) {
  return (g, w, h) => {
    g.fillStyle = '#0b0a10'; g.fillRect(0, 0, w, h);
    if (frame) {
      g.strokeStyle = subColor; g.lineWidth = 5; g.shadowColor = subColor; g.shadowBlur = 12;
      roundRect(g, 10, 10, w - 20, h - 20, 18); g.stroke();
    }
    g.textAlign = 'center'; g.textBaseline = 'middle';
    g.font = font; fitText(g, text, w * 0.86);
    const y = sub ? h * 0.42 : h / 2;
    g.shadowColor = glow || color; g.shadowBlur = 22;
    g.strokeStyle = color; g.lineWidth = 7; g.strokeText(text, w / 2, y);
    g.shadowBlur = 0; g.lineWidth = 2.5; g.strokeStyle = '#fff4fb'; g.strokeText(text, w / 2, y);
    if (sub) {
      g.font = subFont; fitText(g, sub, w * 0.86);
      g.shadowColor = subColor; g.shadowBlur = 14; g.fillStyle = subColor; g.fillText(sub, w / 2, h * 0.78);
      g.shadowBlur = 0;
    }
  };
}

function fitText(g, text, maxW) {
  const m = g.measureText(text);
  if (m.width > maxW) {
    const px = parseFloat(g.font.match(/(\d+(\.\d+)?)px/)[1]);
    g.font = g.font.replace(/(\d+(\.\d+)?)px/, `${Math.floor(px * maxW / m.width)}px`);
  }
}

export function roundRect(g, x, y, w, h, r) {
  g.beginPath();
  g.moveTo(x + r, y); g.lineTo(x + w - r, y); g.quadraticCurveTo(x + w, y, x + w, y + r);
  g.lineTo(x + w, y + h - r); g.quadraticCurveTo(x + w, y + h, x + w - r, y + h);
  g.lineTo(x + r, y + h); g.quadraticCurveTo(x, y + h, x, y + h - r);
  g.lineTo(x, y + r); g.quadraticCurveTo(x, y, x + r, y); g.closePath();
}
