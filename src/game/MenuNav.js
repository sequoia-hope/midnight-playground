// Moving around the menus with a gamepad (Pads.state.nav: the D-pad or
// left stick, A, B, Start). A highlight goes to the nearest control in the
// direction pushed; A presses it. Sliders and drop-downs: A to adjust,
// ◂ ▸ to change, A or B when done (otherwise ◂ ▸ would be stuck on them).
// B and Start are the screen's (back, start). A screen opens on its
// [data-nav-first] control, or else its primary button. The mouse or a finger
// hides the highlight again.

const DIRS = ['up', 'down', 'left', 'right'];
const KEYS = [...DIRS, 'confirm', 'back', 'start'];
const REPEAT_DELAY = 380, REPEAT_EVERY = 110;
const SLIDER_STEP = 5;

const visible = (el) => el.getClientRects().length > 0 && getComputedStyle(el).visibility !== 'hidden';
// A checkbox, slider or drop-down is shown (and highlighted) as its label.
const boxOf = (el) => (el.matches('input, select') && el.closest('label')) || el;

export class MenuNav {
  // root(): the screen to move around, or null. back(root) / start(root):
  // what B and Start do there.
  constructor({ root, back = () => {}, start = () => {} }) {
    this.root = root;
    this.back = back;
    this.start = start;
    this.prev = {};
    this.next = {};
    this.cur = null;
    this.rootEl = null;
    this.shown = false;
    this.editing = false;
    this.memory = new WeakMap(); // screen → its last highlighted control
    window.addEventListener('pointerdown', () => this.hide(), true);
    window.addEventListener('mousemove', (e) => { if (e.movementX || e.movementY) this.hide(); }, true);
  }

  // Called every frame, menu or not, so a button held from the race (A for
  // nitro, Start to pause) doesn't count as a press on the screen that opens.
  update(nav, now = performance.now()) {
    const fired = {};
    for (const k of KEYS) {
      const on = !!nav[k];
      if (on && !this.prev[k]) { fired[k] = true; this.next[k] = now + REPEAT_DELAY; }
      else if (on && DIRS.includes(k) && now >= this.next[k]) { fired[k] = true; this.next[k] = now + REPEAT_EVERY; }
      this.prev[k] = on;
    }
    const root = this.root();
    if (root !== this.rootEl) {
      if (this.rootEl && this.cur) this.memory.set(this.rootEl, this.cur);
      this.rootEl = root;
      this.editing = false;
      // Already steering with the pad: the new screen opens highlighted.
      const back = root && this.valid(this.memory.get(root)) ? this.memory.get(root) : null;
      this.setCur(back ?? (root && this.shown ? this.defaultFor(root) : null));
    }
    if (!root) return;
    if (this.cur && !this.valid(this.cur)) this.setCur(null);
    if (!KEYS.some((k) => fired[k])) return;
    const cur = this.cur ?? this.defaultFor(root);
    if (!this.shown) {
      this.shown = true;
      document.body.classList.add('pad-nav');
      this.setCur(cur);
      if (DIRS.some((d) => fired[d])) return; // the first push only shows where you are
    }
    if (!cur) return;
    const tag = cur.matches('input[type=range]') ? 'range' : cur.matches('select') ? 'select' : 'other';
    if (fired.back) {
      if (this.editing) this.setEditing(false);
      else this.back(root);
      return;
    }
    if (fired.start) { this.setEditing(false); this.start(root); return; }
    if (fired.confirm) {
      if (tag === 'other') cur.click();
      else this.setEditing(!this.editing);
      return;
    }
    for (const d of DIRS) {
      if (!fired[d]) continue;
      const side = d === 'left' ? -1 : d === 'right' ? 1 : 0;
      if (side && this.editing) { if (tag === 'range') this.stepRange(cur, side); else this.stepSelect(cur, side); }
      else { this.setEditing(false); this.move(root, d); }
    }
  }

  hide() {
    if (!this.shown) return;
    this.shown = false;
    this.setEditing(false);
    document.body.classList.remove('pad-nav');
  }

  valid(el) {
    return !!el && !!this.rootEl?.contains(el) && !el.disabled && visible(el);
  }
  focusables(root) {
    return [...root.querySelectorAll('button, input, select, a[href]')].filter((el) => !el.disabled && visible(el));
  }
  defaultFor(root) {
    const list = this.focusables(root);
    return list.find((el) => el.matches('[data-nav-first]')) ?? list.find((el) => el.matches('.btn.primary')) ?? list[0] ?? null;
  }

  setCur(el) {
    if (this.cur) boxOf(this.cur).classList.remove('pad-focus', 'pad-edit');
    this.cur = el;
    if (!el) return;
    const box = boxOf(el);
    box.classList.add('pad-focus');
    box.classList.toggle('pad-edit', this.editing);
    box.scrollIntoView?.({ block: 'nearest', inline: 'nearest' });
  }
  setEditing(on) {
    this.editing = on;
    if (this.cur) boxOf(this.cur).classList.toggle('pad-edit', on);
  }

  // The nearest control whose middle lies that way, preferring ones in line.
  move(root, dir) {
    const from = boxOf(this.cur).getBoundingClientRect();
    const fx = from.left + from.width / 2, fy = from.top + from.height / 2;
    const vert = dir === 'up' || dir === 'down', sign = dir === 'up' || dir === 'left' ? -1 : 1;
    let best = null, bestScore = Infinity;
    for (const el of this.focusables(root)) {
      if (el === this.cur || boxOf(el) === boxOf(this.cur)) continue;
      const r = boxOf(el).getBoundingClientRect();
      const cx = r.left + r.width / 2, cy = r.top + r.height / 2;
      const along = (vert ? cy - fy : cx - fx) * sign;
      if (along <= 1) continue;
      // How far off to the side it is, 0 where the two overlap.
      const off = vert ? Math.max(0, r.left - from.right, from.left - r.right) : Math.max(0, r.top - from.bottom, from.top - r.bottom);
      const score = along + off * 3;
      if (score < bestScore) { best = el; bestScore = score; }
    }
    if (best) this.setCur(best);
  }

  stepRange(el, side) {
    const step = SLIDER_STEP * (Number(el.step) || 1);
    const v = Math.min(Number(el.max || 100), Math.max(Number(el.min || 0), Number(el.value) + side * step));
    if (v === Number(el.value)) return;
    el.value = v;
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  }
  stepSelect(el, side) {
    const i = el.selectedIndex + side;
    if (i < 0 || i >= el.options.length) return;
    el.selectedIndex = i;
    el.dispatchEvent(new Event('change', { bubbles: true }));
  }
}
