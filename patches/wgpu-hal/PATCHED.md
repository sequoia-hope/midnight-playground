# wgpu-hal 29.0.4, patched for Midnight Playground

The crates.io release of `wgpu-hal` 29.0.4, unchanged except in its GL
backend (`src/gles/`), which now skips the GL calls that bind what GL
already has (DECISIONS D1190):

- `command.rs` `set_vertex_buffer`: the same buffer at the same offset
  (Bevy's mesh slabs, draw after draw) does not dirty the vertex
  attributes, so they are not pointed again (`bindBuffer`,
  `enableVertexAttribArray`, `vertexAttribPointer`, `vertexAttribDivisor`
  per attribute per draw before).
- `queue.rs` `process`: a `BindCache` on the `Queue` (`mod.rs`) remembers
  the program, and each texture unit's texture (with its mip range) and
  sampler, since the command buffer began; `SetProgram`, `BindTexture` and
  `BindSampler` for what is already bound are no-ops. `reset_state` (the
  start of every command buffer) clears it, and so do the three copies
  that bind a texture on whatever unit is active. `SetIndexBuffer` uses the
  `current_index_buffer` the queue already kept, and the buffer barrier
  that binds `ELEMENT_ARRAY_BUFFER` forgets it.
- `TEXTURE_BASE_LEVEL` and `TEXTURE_MAX_LEVEL` are texture state, not unit
  state: `AdapterShared::tex_levels` remembers the range each texture was
  last set to, and `BindTexture` sets them only when the view's range
  differs (two `texParameteri` per bind before, 3,900 a frame on Sierra).
  Those are the only two places the backend sets them. `destroy_texture`
  forgets the texture, as GL may give its name to a new one.

Why: on Sierra the WebGL2 build made about 24,600 GL calls a frame for
685 draws, where three.js made 4,400 for 780, and every call crosses from
wasm to the browser's GL command buffer. On a 2017 laptop that was the
frame (docs/rust-port/DECISIONS.md D1190; measured with
`tools/parity/gl-calls.mjs`).

Used through `[patch.crates-io]` in the workspace's Cargo.toml. To move to
a new wgpu-hal release: copy the release's crate here (from
`~/.cargo/registry/src/*/wgpu-hal-<version>/`, without its Cargo.lock and
examples), apply the same changes (search for D1190), and update this
note. Worth reporting upstream.

Also `#![allow(warnings)]` at the top of `src/lib.rs`: Cargo caps the
lints of a crates.io dependency but not of a path one.
