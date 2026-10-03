// Install the parity kernel in a Node process before anything else runs:
//   NODE_OPTIONS=--import=./tools/parity/kernel/register.mjs node --test ...
// (NODE_OPTIONS, not --import, so the test runner's child processes get it.)
import fs from 'node:fs';
import { KERNEL_WASM, installKernel } from '../../../src/parity/kernel.js';

installKernel(fs.readFileSync(KERNEL_WASM));
