import { ACTIONS, DEFAULT_MAP, bindingLabel } from './Gamepad.js';

// The Controller screen (#padsetup): what each action is bound to on the
// pad last touched, lit while held so you can try it, and picking a new
// button or stick for one (Pads.startCapture). Defaults puts that pad back
// on the standard layout.

// "Xbox Wireless Controller (STANDARD GAMEPAD Vendor: 045e Product: 0b13)"
const padName = (p) => p.id.replace(/\s*\((STANDARD GAMEPAD|Vendor:)[^)]*\)/i, '').trim() || 'Controller';

export class PadSetup {
  constructor(pads, el, { onClose }) {
    this.pads = pads;
    this.el = el;
    this.onClose = onClose;
    this.open = false;
    this.key = '';
    this.rows = new Map();
    const list = el.querySelector('#pad-binds');
    for (const [id, label] of ACTIONS) {
      const b = document.createElement('button');
      b.className = 'pad-bind';
      b.dataset.act = id;
      if (!this.rows.size) b.dataset.navFirst = '';
      b.append(Object.assign(document.createElement('span'), { textContent: label }), document.createElement('b'));
      b.onclick = () => this.pick(id);
      list.appendChild(b);
      this.rows.set(id, b);
    }
    el.querySelector('#pad-defaults').onclick = () => {
      this.pads.cancelCapture();
      this.pads.resetMap(this.pads.active);
      this.hint('Back to the standard layout');
    };
    el.querySelector('#pad-done').onclick = () => this.close();
  }

  show() {
    this.open = true;
    this.key = this.padKey = null;
    this.hint();
    this.update();
  }
  close() {
    if (!this.open) return;
    this.pads.cancelCapture();
    this.open = false;
    this.onClose?.();
  }
  // Esc, P or Start (Start is only heard when not listening): stop
  // listening, or leave.
  escape() {
    if (this.pads.capture) this.pads.cancelCapture(); else this.close();
  }

  hint(text) {
    this.el.querySelector('#pad-hint').textContent = text
      ?? (this.pads.active ? 'Pick an action, then press the button or move the stick you want for it' : 'Press a button on your controller');
  }

  pick(id) {
    const was = this.pads.capture?.action;
    this.pads.cancelCapture();
    if (was === id) return;
    if (!this.pads.active) { this.hint(); return; }
    const label = ACTIONS.find(([a]) => a === id)[1];
    this.rows.get(id).classList.add('listening');
    this.hint(`Press a button or move a stick for ${label} (Esc to cancel)`);
    this.pads.startCapture(id, (b) => {
      this.rows.get(id).classList.remove('listening');
      this.hint(b ? `${label}: ${bindingLabel(b, this.pads.active?.mapping === 'standard')}` : undefined);
    });
  }

  // Every frame while open: the labels when the pad or its map changes, and
  // which actions are held.
  update() {
    const p = this.pads.active;
    const map = p ? this.pads.mapFor(p) : DEFAULT_MAP;
    const padKey = p ? p.index + p.id : '';
    if (padKey !== this.padKey) { this.padKey = padKey; if (!this.pads.capture) this.hint(); }
    const key = padKey + JSON.stringify(map);
    if (key !== this.key) {
      this.key = key;
      const std = !p || p.mapping === 'standard';
      this.el.querySelector('#pad-name').textContent = p
        ? padName(p) + (this.pads.isRemapped(p) ? ' · remapped' : std ? '' : ' · not a standard layout: check the buttons')
        : 'No controller yet';
      for (const [id, row] of this.rows) row.querySelector('b').textContent = (map[id] ?? []).map((b) => bindingLabel(b, std)).join(' / ') || '—';
    }
    const held = this.pads.capture ? {} : this.pads.state.held;
    for (const [id, row] of this.rows) row.classList.toggle('on', !!held[id]);
  }
}
