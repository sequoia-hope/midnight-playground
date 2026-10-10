# wgpu 29.0.4, patched for Midnight Playground

The crates.io release of `wgpu` 29.0.4, unchanged except for
`src/backend/webgpu.rs` (and one line in `src/lib.rs`, below): `WebDevice` reads the device's limits and
features from the browser once and keeps them (`limits` and `features`,
two `OnceCell`s), instead of mapping every field of `GPUSupportedLimits`
on each call. A WebGPU device's limits and features never change after it
is made, so the answers are the same.

Why: Bevy 0.19's `SetMeshBindGroup` asks `RenderDevice::limits()` on every
draw (`skins_use_uniform_buffers`), which cost about 7 % of a phone's frame
on Coast and Sierra (docs/rust-port/DECISIONS.md, D1182; first found in
D866 item 3).

Used through `[patch.crates-io]` in the workspace's Cargo.toml. To move to a
new wgpu release: copy the release's crate here (from
`~/.cargo/registry/src/*/wgpu-<version>/`, without its Cargo.lock),
apply the same change (search for D1182), and update this note. Worth
reporting upstream; remove the patch once a release caches the limits.

Also `#![allow(warnings)]` at the top of `src/lib.rs`: Cargo caps the
lints of a crates.io dependency but not of a path one, and this release
has a few unused-import warnings in our feature set.
