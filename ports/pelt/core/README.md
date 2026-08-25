# pelt-core

`pelt-core` is the embeddable controller of the Pelt reference browser. A
`PeltController` owns one retained document session, its engine registries,
navigation history, host-neutral input effects, target size, and frame
production. `PeltWorkspace` arranges one controller per document tile through
the shared `TileTree`, retaining inactive tabs and routing Frisket content-hole
geometry without adding a window or paint dependency.

Concrete engines receive resource policy when the caller registers them.
The controller receives a caller-owned clock and returns the engine's generic
frame type. Window creation, wgpu device and queue ownership, rasterization,
and presentation remain with the embedding host. This lets standalone Pelt,
Tabard previews, and focused test hosts drive the same controller without
depending on winit or a paint backend.
