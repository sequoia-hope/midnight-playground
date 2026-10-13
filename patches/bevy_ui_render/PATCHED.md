# bevy_ui_render 0.19.1, patched for Midnight Playground

The crates.io release of `bevy_ui_render` 0.19.1, unchanged except that
the five UI draw commands (`render_pass.rs` `DrawUiNode`,
`ui_material_pipeline.rs`, `ui_texture_slice_pipeline.rs`,
`box_shadow.rs`, `gradient.rs`) return before drawing a batch whose range
is empty (DECISIONS D1200).

Why: `prepare_uinodes` starts a new batch at an empty index range, and the
batch stays empty when its nodes add no quads (a clipped node, an empty
text); it was drawn anyway. Chrome's WebGPU then warns once as the menu
first draws, `Calling [RenderPassEncoder "ui"].Draw with an index count of
0 is unusual.`, which the `audio` e2e suite's no-warnings test counts
(D907). An empty draw does nothing, so skipping it changes no picture.

To move to a newer Bevy: check whether upstream skips empty batches; if
not, copy the new release here and add the same five guards (search for
`D1200`).
