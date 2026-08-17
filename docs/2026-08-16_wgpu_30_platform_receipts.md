# wgpu 30 Platform Receipts

**Date**: 2026-08-16

Closes the receipt list from the
[wgpu 30 unification plan](2026-08-15_wgpu_30_unification_plan.md): Windows
headed composition, CubeCL compute and resident buffers, wasm WebGPU, macOS
IOSurface, Linux DMA-BUF.

**All seven pass.**

| Receipt | Host | Result |
|---|---|---|
| Windows headed composition | this box, RTX 4060 | PASS |
| CubeCL compute | this box, RTX 4060 | PASS |
| Resident buffer | this box, RTX 4060 | PASS |
| wasm WebGPU | this box, Chromium | PASS |
| macOS IOSurface, x86_64 | Q-PC, Intel iMac | PASS |
| macOS IOSurface, aarch64 | Mayola's iMac, Apple M4 | PASS |
| Linux DMA-BUF | ThinkPad, AMD RADV | PASS |

Both macOS architectures have a runtime receipt. The aarch64 half was
outstanding on 2026-08-16 morning because the machine was unreachable; see
**Hosts** for why that was a network fact rather than a hardware one.

Linux DMA-BUF was written up as a hardware failure earlier the same day and
then cleared, because the migration itself supplied the missing piece: wgpu 30
enables the Vulkan extension that AMD's implicit-modifier buffers need, and
wgpu 29 had no such thing. The failing section is kept below rather than
deleted, because the reasoning that made it look permanent is worth keeping.

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

## macOS IOSurface, aarch64 — PASS

`demo-weld-mac` on Mayola's iMac (Apple M4, macOS 26.5.1 / 25F80, arm64),
rebuilt at `02fb1cc` so the run is on the wgpu-30 default row. The README's
prior "verified on M4" line dates from 2026-08-12 and predates the flip, so
this is the first Apple Silicon run on 30.

```
wgpu interop backend: Metal
CEF browser created (1280x800) at https://example.com  →  LoadEnd http_status: 200
imported frame #1 (1280x800 Bgra8Unorm)
probe: 16384/16384 bytes non-zero in the top-left corner; first pixels [238,238,238,255]
VALIDATION PASS: 2 frames imported and the IOSurface carried real pixels
```

Confirm the binary is fresh before believing this. `bundle-demo-weld-mac`
replaces files in place, so the `.app` **directory** mtimes stay at the date of
the first bundling (Aug 12 here) while the executables inside are current.
Read the mtime on `Contents/MacOS/demo-weld-mac`, not on the bundle.

Snapshot at `testing/rendering/2026-08-16_wgpu30_macos_aarch64_iosurface.png`.
It needs `WELD_EXIT_AFTER_FRAMES` set high enough to outlive the asynchronous
PNG callback; at 1 or 2 frames the process exits first and no file appears even
though the probe passed. As on Linux, the PNG is a CEF-side capture and is
context, not evidence. The probe readback is the evidence.

### The `cef` crate pins wgpu 29, so the weld demos carry two rows

`cargo tree -p demo-weld-mac -e normal` resolves **both** wgpu 29.0.3 and
30.0.0. The second row is upstream's: `cef = "148"` declares
`wgpu = "29"` as an optional dependency enabled by its `accelerated_osr`
feature, for its own `osr_texture_import` module.

This does not compromise the receipt. Welding imports through its own
`native_frame` on its own wgpu-30 row and never calls cef's importer; the
seam between them is a raw `IOSurfaceRef`, not a wgpu type, so no version
couples across it. Cef's copy is compiled and unused.

It does mean the "exactly one wgpu row" condition does not hold for the three
`demo-weld-*` packages, and cannot until cef bumps. They are receipt harnesses
in a deliberately multi-row repo rather than active product packages, so this
is noted rather than treated as a regression.

## Linux DMA-BUF — PASS (after the fix below)

Cleared 2026-08-16 on the Fedora ThinkPad, AMD Renoir/RADV, Mesa 26.1.5, the
same host that failed earlier the same day.

```
adapter DMA-BUF import feature: true
imported frame #1 (1280x701 Bgra8UnormSrgb)
imported frame #2 (1366x701 Bgra8UnormSrgb)
VALIDATION PASS: 2 frame(s) imported, 16384/16384 bytes non-zero,
                 first pixels [238,238,238,255]
```

Those numbers match the macOS receipt exactly, which is the point: `#EEEEEE`
uniform across the corner is what example.com's background must be.

**The receipt is the imported texture itself**, dumped with the new
`WELD_TEXTURE_DUMP` and saved to
`testing/rendering/2026-08-17_linux_dmabuf_imported_texture.png`. It renders
the page with clean text and correct layout, which is the evidence a CEF-side
snapshot could never give: those bytes came out of the wgpu texture backed by
CEF's DMA-BUF.

### What changed

Three things, none of them hardware:

1. `welding` substitutes `DRM_FORMAT_MOD_LINEAR` when CEF reports
   `DRM_FORMAT_MOD_INVALID`, gated on the host device carrying
   `VULKAN_EXTERNAL_MEMORY_DMA_BUF`. wgpu 28 and 29 answer false and keep the
   old refusal, so the multi-row promise holds; all three rows still compile.
2. `demo-weld-linux` requests that feature when the adapter offers it. Without
   the request the device does not advertise it and the gate never opens.
3. The imported texture now declares `COPY_SRC`, and its `VkImage` declares
   `TRANSFER_SRC`. **This was a latent second bug**: the macOS path has always
   declared `COPY_SRC`, Linux never did, so the readback probe would have
   panicked on the first successful import. It was invisible while the
   implicit-modifier refusal fired first, and surfaced the moment the import
   started working.

### The window looked black, and the application is not at fault

Watching the ThinkPad directly, the frame was visible while the window was
expanding and closing but not in between. That observation is real. It is
**not** an application bug.

A readback of the swapchain taken 10 seconds after the last paint, immediately
before present, contains the fully rendered page. Its mean is `0.930155`,
matching the imported texture's `0.930155` at the same 1366x701, saved as
`testing/rendering/2026-08-17_linux_swapchain_present.png`. Import, render pass
and present are therefore all correct in the steady state, and whatever blanks
the window happens after the application hands the frame over, in the
compositor on that Fedora/Xwayland session.

Buffer decay is ruled out as well: probing 19 and 21
seconds after the last paint still returned 16384/16384 non-zero starting with
`#EEEEEE`, so the imported memory is intact long after CEF goes quiet, and the
demo holds the texture and redraws continuously under `ControlFlow::Poll`.

**Do not try to settle this with a screen capture over SSH.** `ffmpeg
-f x11grab -i :0.0` on this Fedora/Xwayland session returns a black frame
whatever is on screen: a white 800x600 window put up as a positive control
captured a mean of `5.58917e-05`, the identical value returned for every
capture of the demo. Wayland surfaces are not in the X root window, so the
grab reads an empty root. `import`/`magick import` fail outright here, and
`grim`, `gnome-screenshot` and `scrot` are not installed. Several captures
were taken and believed before the control exposed them, which is
`[[feedback-prove-the-instrument-before-believing-a-negative]]` exactly.

The instrument that settled it is `WELD_PRESENT_DUMP`, added to
`demo-weld-linux`: it copies the swapchain image out just before presenting.
That reports what the render pass produced and needs nobody's cooperation,
which is exactly what a screen capture cannot claim here. It needs `COPY_SRC`
on the surface, so the demo adds that usage only when the variable is set and
the surface offers it, leaving the ordinary path configured as before.

Three instruments, three different questions, and mixing them up is what cost
the detour: `WELD_TEXTURE_DUMP` shows what CEF handed over, `WELD_PRESENT_DUMP`
shows what this application drew, and a screen capture shows what the
compositor chose to display. Only the last one was ever in doubt, and it is
the only one that cannot be measured from here.

### Two traps in reading the probe

**A `VALIDATION PASS` is not a correct-layout receipt.** The probe counts
non-zero bytes, and a wrongly-tiled buffer is scrambled yet non-zero. Only the
whole-texture dump distinguishes them.

**Probe at least 2 frames.** The first accelerated paint arrives before the
page does. Frame 1 reported 12160/16384 bytes non-zero starting with black,
which is exactly what a tiling bug looks like, and cost a detour before frame 2
reported 16384/16384 starting with `#EEEEEE`. A partial early frame and a
scrambled one are indistinguishable from one sample.

## Appendix: why this looked permanent (superseded)

Kept as written before the fix.

### Linux DMA-BUF — FAIL (hardware, pre-existing)

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

### wgpu 30 lifts the capability barrier; only the modifier remains

Probed on the ThinkPad, 2026-08-16, and this **supersedes the sentence above**
about needing modifier support in wgpu. It is there now.

`Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF` is new in wgpu-types 30 and absent
from 29 entirely. wgpu-hal 30 requests `VK_EXT_image_drm_format_modifier`
whenever the adapter supports it (`vulkan/adapter.rs:1355`) and exposes
`Device::texture_from_dmabuf_fd`. Welding's comment that "wgpu does not enable
it" was true when written against 28/29 and became stale on the move to 30.

A standalone probe on RADV RENOIR (Mesa 26.1.5) reports:

```
VULKAN_EXTERNAL_MEMORY_DMA_BUF : true
VULKAN_EXTERNAL_MEMORY_FD      : true
```

`vulkaninfo` confirms all three underlying extensions on the physical device,
so this is the real hardware answering, not a wgpu default.

What CEF actually supplies, from welding's own trace on the same box:

```
on_accelerated_paint: planes=1, format=CEF_COLOR_TYPE_BGRA_8888,
                      modifier=0xffffffffffffff, coded_size=1280x701
```

Two of the three constraints already fit. `texture_from_dmabuf_fd` documents
single-plane support only, and CEF supplies exactly one plane, in BGRA8888.
**The blocker is now solely `modifier = 0x00ffffffffffffff`**, which is
`DRM_FORMAT_MOD_INVALID`. The new API takes an *explicit* modifier and builds
`ImageDrmFormatModifierExplicitCreateInfoEXT` from it, so the capability
exists and the information does not.

Passing `DRM_FORMAT_MOD_LINEAR` on a guess is not obviously safe. On AMD an
implicit modifier is usually a tiled DCC layout rather than linear, and a wrong
guess reads as garbage pixels rather than a clean error, which is the worst
failure shape for a receipt. Closing this needs either a CEF-side route to the
real modifier, or a readback check that validates the guess before the lane is
declared working. The probe crate used here is at `~/dmabuf-probe` on the
ThinkPad.

**The cef wgpu-29 pin is not implicated.** The obvious suspicion, given that
the weld demos resolve two wgpu rows, is that the Linux import somehow ran on
cef's 29. It did not. `welding::native_frame::vulkan_dmabuf` refuses
`DRM_FORMAT_MOD_INVALID` in an early guard at `vulkan_dmabuf.rs:62`, before it
makes any wgpu or Vulkan call, so the result is byte-identical on 28, 29 and
30. Welding never calls cef's `import_to_wgpu`, and cef's own importer builds
its image with `ImageDrmFormatModifierExplicitCreateInfoEXT`, so it requires an
explicit modifier too and would refuse the same buffer. The constraint is what
CEF hands over on AMD/RADV, not which wgpu receives it.

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
- **ThinkPad** `markik@192.168.4.28`: Fedora 44, AMD Renoir/RADV (Mesa 26.1.5),
  rustc 1.97.1.
- **Mayola's iMac** `markik@192.168.4.57`: Apple M4 (Mac16,3), macOS 26.5.1
  (25F80), arm64, rustc 1.97.1. Host key ED25519
  `SHA256:0sT34z10ELgEyR8+LJFVDvOov3iGNuPz8gOx4QU48vo`, matching the recorded
  fingerprint.

### Reaching the M4, and two wrong turns worth not repeating

This machine read as "down" for most of the day and was not. Both diagnoses
along the way were wrong, in ways a sweep alone could not distinguish.

**It is wired to a business Ethernet line on `192.168.1.0/24`**, which is a
different network from the `Boykin Mesh Network` Wi-Fi the Windows box uses,
with no route between them in either direction. No setting on the Mac could
have fixed that; Remote Login was on the whole time. It was reached by joining
its Wi-Fi to the mesh network while leaving Ethernet plugged in, which macOS
runs concurrently, giving it a second address on the reachable side.

Two traps produced confident wrong answers before that:

1. **The Wi-Fi is a `/22`, not a `/24`.** `192.168.4.0/22` spans
   `192.168.4.0`–`192.168.7.255`. A sweep of `192.168.4.x` covers a quarter of
   it and reads as exhaustive. Check `Get-NetIPAddress` for the prefix length
   before believing a subnet sweep.
2. **`.32` from the old note is now an iOS device**, not the iMac. It answers
   ping, which reads as "the host is up but sshd is off". Port 62078 is
   `lockdownd` and identifies an iPhone or iPad; a Mac would not have it.

The cheap decisive test was neither sweep: **mDNS**. Only Q-PC answered
`_ssh._tcp.local`. Since mDNS is link-local multicast, a machine on the same
segment answers regardless of which services it runs, so silence there is
positive evidence of a different broadcast domain rather than of a closed port.
Reach for it before concluding anything from a port scan.
