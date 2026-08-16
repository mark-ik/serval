# wgpu 30 Platform Receipts

**Date**: 2026-08-16

Closes the receipt list from the
[wgpu 30 unification plan](2026-08-15_wgpu_30_unification_plan.md): Windows
headed composition, CubeCL compute and resident buffers, wasm WebGPU, macOS
IOSurface, Linux DMA-BUF.

**Five of six pass. Linux DMA-BUF fails on the only Linux hardware available,
for a documented pre-existing reason unrelated to wgpu 30.**

| Receipt | Host | Result |
|---|---|---|
| Windows headed composition | this box, RTX 4060 | PASS |
| CubeCL compute | this box, RTX 4060 | PASS |
| Resident buffer | this box, RTX 4060 | PASS |
| wasm WebGPU | this box, Chromium | PASS |
| macOS IOSurface | Q-PC, Intel iMac | PASS |
| Linux DMA-BUF | ThinkPad, AMD RADV | FAIL — hardware |

---

## Windows headed composition — PASS

`pelt --windows-present-smoke` and `--windows-present-surfaces-smoke`, feature
`windows-present`. Path: winit window → HWND → `netrender::boot()` →
`paint::WindowsDxgiBackend` (DCOMP visual tree + composition swapchain) →
`ServoCompositor` → per-redraw netrender render + present + DCOMP commit.

Base variant printed `800x600 frames=1 created_window=true
declared_subsurface=false` and exited 0. The surfaces variant was confirmed
visually: a red field (netrender's master) with a **green top-left rectangle**,
which is the declared compositor surface arriving as its own per-`SurfaceKey`
DCOMP child visual. That is the composition path, not just a present.

## CubeCL compute — PASS

`crates/probes/cubecl-repulsion` (gitignored, standalone). Raw CubeCL kernel
over buffers Burn allocated, on the vendored wgpu-30 `cubecl-wgpu`.

```
runtime: wgpu<wgsl>   adapter: NVIDIA GeForce RTX 4060 Laptop GPU (Vulkan)
carriage: CubeCL-JIT raw kernel, Burn-owned buffers
client receipt: Burn tensors and raw kernel share one CubeCL client
numeric receipt: n=512, mean relative error 1.26e-7, worst 1.30e-6, tolerance 1.0e-3
timing receipt: n=50000, 20 synchronized frames, avg 11.46 ms (best 9.24, worst 12.68)
bridge verdict: disappears; the probe contains no wgpu resource extraction or copy
```

The numeric receipt is the one that matters: the kernel computes the right
answer on wgpu 30, three orders of magnitude inside tolerance.

## Resident buffer — PASS

`crates/probes/resident-graph`. Positions and velocities resident on the GPU,
WGSL kernels advancing them, a tenant drawing from the same buffer through
netrender's tenancy seam, and a four-byte settle flag as the only readback.

```
50000 nodes, 149990 edges       adapter: NVIDIA GeForce RTX 4060 Laptop GPU
gpu resident: 300 frames, avg 15.82 ms (best 13.03, worst 17.94)
cpu barnes-hut: one force pass over 50000 nodes in 266.18 ms
receipt: 3 distinct colours -> testing/mere/p2_resident_graph.png (lit 26.5%)
```

Both probes needed a `[patch.crates-io]` added locally: they are standalone
workspaces, so they inherit neither mere's vello fork redirect nor its vendored
`cubecl-wgpu`. Without those they resolve vello 0.10 (wgpu 29) and
cubecl-wgpu 0.10 (wgpu 29) beside their own wgpu 30.

## wasm WebGPU — PASS

`examples/genet_web_smoke`, built for `wasm32-unknown-unknown`, wasm-bindgen
0.2.126, served locally and opened in the Chromium pane. Chain: cambium view
tree → ScriptedDom → genet-layout → PaintList → paint_list_render → netrender
→ `Renderer::render_vello` → WebGPU canvas present.

Console logged `genet web smoke: PASS` and the page title flipped to
`SMOKE PASS`, which is the example's own automation hook for success. One
source fix was needed to build at all: `SurfaceConfiguration::color_space`,
missed in the earlier sweep because that example only compiles under wasm32.

## macOS IOSurface — PASS

`demo-weld-mac` on Q-PC (Intel iMac, macOS 15.7.7, x86_64), welding on its new
wgpu-30 default row. CEF must run from a real `.app`, so the receipt runs
`bundle-demo-weld-mac` and then the bundle's inner executable directly, which
is how the env vars get through.

```
wgpu interop backend: Metal
CEF browser created (1280x800) at https://example.com  →  LoadEnd http_status: 200
imported frame #1 (1280x800 Bgra8Unorm)
probe: 16384/16384 bytes non-zero in the top-left corner; first pixels [238,238,238,255]
VALIDATION PASS: 1 frames imported and the IOSurface carried real pixels
```

Snapshot at `testing/rendering/2026-08-16_wgpu30_macos_iosurface.png` shows the
rendered example.com page, so Chromium's output really did cross
IOSurface → MTLTexture → wgpu Metal.

**One frame, not the ten requested, is correct.** Accelerated OSR is
change-driven: a static page paints once and then produces no further damage.
The demo gates on elapsed time as well as frame count for exactly this reason,
and it is the same trap as
`[[feedback-paint-counts-are-not-clocks]]`.

## Linux DMA-BUF — FAIL (hardware, pre-existing)

`demo-weld-linux` on the Fedora ThinkPad (Fedora 44, **AMD Radeon Renoir,
radeonsi/RADV/ACO**), over Xwayland (`DISPLAY=:0` plus the mutter auth cookie,
`WAYLAND_DISPLAY` unset).

**Zero frames imported.** The crate's own diagnostic states the cause:

> Vulkan external memory import failed: CEF supplied `DRM_FORMAT_MOD_INVALID`
> (implicit modifier). Importing it needs `VK_EXT_image_drm_format_modifier` on
> the wgpu device, which wgpu does not enable; linear tiling is rejected for
> `DMA_BUF` by the format query. **Seen on AMD/RADV; Intel/Mesa supplies an
> explicit modifier and works.**

That message predates this migration and names the exact hardware split, so
this is not a wgpu 30 regression — it is the lane meeting hardware it was never
verified on. `demo-weld-linux`'s own header says "Validated against Intel/Mesa
+ Vulkan + X11". Clearing this receipt needs an Intel/Mesa Linux box, or
`VK_EXT_image_drm_format_modifier` support in wgpu.

Do not be fooled by the snapshot at
`testing/rendering/2026-08-16_wgpu30_linux_dmabuf.png`: it shows a rendered
page, but with zero imported frames it cannot be evidence of the interop path.
`cpu-paint-fallback` is not enabled for this demo either, so the snapshot is a
CEF-side capture rather than anything that crossed into wgpu.

### A second, separable finding: the predicted wgpu 30 fence bug

The same run produced repeated Vulkan validation errors:

```
VUID-vkAcquireNextImageKHR-fence-10066
vkAcquireNextImageKHR(): (VkFence ...) is already in use by another submission.
```

This is the bug flagged at the very start of the migration: wgpu's `v30` branch
carries an **unreleased** backport, "use `vk::Fence::null` on non-Windows
swapchain" ([#9918](https://github.com/gfx-rs/wgpu/pull/9918)), which lands on
exactly this non-Windows swapchain path. The prediction is now observed rather
than inferred.

It could **not** be tested in place. A `[patch.crates-io] wgpu = { branch =
"v30" }` in wgpu-weld replaces the `wgpu` package for *all three* of welding's
rows, because 28/29/30 are package aliases of the same crate name, and the v30
branch cannot satisfy `^28.0.0`. Resolution fails before building. Testing the
fix needs a single-row consumer, or waiting for 30.0.1. The probe patch was
reverted; the ThinkPad's only dirty path is its untracked CEF download.

## Hosts

- **This box**: Windows 11, RTX 4060 Laptop, rustc 1.97.1.
- **Q-PC** `markik@192.168.4.105`: Intel iMac, macOS 15.7.7, x86_64, rustc 1.97.1.
- **ThinkPad** `markik@192.168.4.28`: Fedora 44, AMD Renoir/RADV, rustc 1.96.0.
- **Mayola's iMac** `.32` (M4, the preferred mac target) was **not reachable** —
  ssh/22 refused. The macOS receipt therefore ran on Intel, not Apple Silicon,
  so `aarch64-apple-darwin` IOSurface is still unproven at runtime. It does
  cross-compile clean.

Note the recorded addresses in the SSH access memory are `192.168.1.x`; the
network is now `192.168.4.x` with the same last octets.
