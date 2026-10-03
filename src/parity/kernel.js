// Parity hook (Rust port, roadmap WP 0.3; SPEC 4.2): replace every inexact
// Math function with mr_math's kernel, compiled to wasm, so the JS reference
// run computes them with exactly the bits the Rust port does.
//
// Off unless something installs it: the game only does so for ?kernel=1
// (src/parity/hooks.js), Node tools through tools/parity/kernel/register.mjs.
// The exact functions (sqrt, floor, round, abs, min, max, sign, imul,
// fround, ...) are left alone. Any other inexact one throws, so a call the
// kernel does not cover is found instead of silently differing.

// Built by `cargo xtask kernel` from tools/parity/kernel (Rust).
export const KERNEL_WASM = new URL('../../tools/parity/kernel/mr_kernel.wasm', import.meta.url);

const UNARY = ['sin', 'cos', 'tan', 'asin', 'acos', 'atan', 'exp', 'log', 'log2', 'log10', 'tanh'];
const BINARY = ['atan2', 'pow'];
const UNCOVERED = ['sinh', 'cosh', 'asinh', 'acosh', 'atanh', 'cbrt', 'expm1', 'log1p'];

export function kernelInstalled() {
  return Math.__kernel === true;
}

// bytes: the wasm file's contents. Synchronous, so it can run before any
// game module evaluates.
export function installKernel(bytes) {
  if (kernelInstalled()) return;
  const k = new WebAssembly.Instance(new WebAssembly.Module(bytes), {}).exports;
  for (const name of [...UNARY, ...BINARY]) Math[name] = k['k_' + name];
  const hypot2 = k.k_hypot;
  // Math.hypot for any number of arguments, as the kernel defines it: abs for
  // one, libm's hypot for two, the square root of the left-to-right sum of
  // squares for more (mr_math::kernel::hypot_n).
  Math.hypot = function hypot(a, b) {
    const n = arguments.length;
    if (n === 2) return hypot2(a, b);
    if (n === 1) return Math.abs(a);
    if (n === 0) return 0;
    let inf = false, nan = false;
    for (let i = 0; i < n; i++) {
      const x = +arguments[i];
      if (x === Infinity || x === -Infinity) inf = true;
      else if (x !== x) nan = true;
    }
    if (inf) return Infinity;
    if (nan) return NaN;
    let sum = 0;
    for (let i = 0; i < n; i++) { const x = +arguments[i]; sum += x * x; }
    return Math.sqrt(sum);
  };
  for (const name of UNCOVERED) {
    Math[name] = () => { throw new Error(`Math.${name} is not in the parity kernel (src/parity/kernel.js)`); };
  }
  Object.defineProperty(Math, '__kernel', { value: true });
  Object.defineProperty(Math, '__kernelInput', { value: k.k_input });
}
