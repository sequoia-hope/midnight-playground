// The JS half of the kernel bit check (`cargo xtask kernel`): evaluate every
// kernel function through the patched Math on the generator's inputs and
// print one hash per function as JSON. xtask compares them with the same
// hashes computed natively in Rust.
//
//   node tools/parity/kernel/check.mjs [wasm-path] [n]
import fs from 'node:fs';
import { KERNEL_WASM, installKernel } from '../../../src/parity/kernel.js';

const wasm = process.argv[2] || KERNEL_WASM;
const n = Number(process.argv[3] || 1_000_000);
installKernel(fs.readFileSync(wasm));

// Same order as FUNCTIONS in tools/parity/kernel/src/lib.rs.
const FUNCTIONS = [
  ['sin', 1, Math.sin], ['cos', 1, Math.cos], ['tan', 1, Math.tan],
  ['asin', 1, Math.asin], ['acos', 1, Math.acos], ['atan', 1, Math.atan],
  ['atan2', 2, Math.atan2], ['exp', 1, Math.exp], ['log', 1, Math.log],
  ['log2', 1, Math.log2], ['log10', 1, Math.log10], ['pow', 2, Math.pow],
  ['tanh', 1, Math.tanh], ['hypot', 2, Math.hypot], ['hypot3', 3, Math.hypot],
];

const input = Math.__kernelInput;
const view = new DataView(new ArrayBuffer(8));
const step = (h, y) => {
  let lo = 0, hi = 0x7ff80000;
  if (y === y) { view.setFloat64(0, y, true); lo = view.getUint32(0, true); hi = view.getUint32(4, true); }
  h = Math.imul(h ^ lo, 0x01000193);
  return Math.imul(h ^ hi, 0x01000193);
};

const out = {};
FUNCTIONS.forEach(([name, arity, fn], f) => {
  let h = 0x811c9dc5 | 0;
  for (let i = 0; i < n; i++) {
    const y = arity === 1 ? fn(input(f, i, 0))
      : arity === 2 ? fn(input(f, i, 0), input(f, i, 1))
      : fn(input(f, i, 0), input(f, i, 1), input(f, i, 2));
    h = step(h, y);
  }
  out[name] = h >>> 0;
});
console.log(JSON.stringify(out));
