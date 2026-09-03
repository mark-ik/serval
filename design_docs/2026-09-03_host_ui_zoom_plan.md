# Host UI zoom

**Status:** in progress (2026-09-03); Z0 through Z4 landed in genet, Z5 landed in isometry with its design figure open (see Progress); nothing committed. Founded when isometry's host migration exposed a panel laid out for
820 logical pixels on a display that offers 752.

**Related:** `docs/2026-08-09_cambium_desktop_host_g1_receipt.md` (the host
this plan extends); mere's configuration ownership plan
(`mere/design_docs/mere_docs/implementation_strategy/2026-08-06_configuration_ownership_settings_projection_plan.md`,
which already names a `ui_zoom` application setting that turnstone persists
and no host applies);
`isometry/design_docs/2026-09-02_genet_host_migration_plan.md` (the
consumer that forced this).

## 1. Why

A cross-platform stack cannot ship fixed layouts. The shared Cambium desktop
host lays out at `window / device_scale` and has no way for an application or
a user to scale the interface: a design sized for one display is cut off or
tiny on another. Turnstone already persists a `ui_zoom` preference through the
settings contract and can only report it, not apply it. Isometry designed its
chrome for 1100x820 and this laptop is 1280x800 logical.

The engine already has the render half. `LiveryPaintList::scaled_to` composes
a frame laid out at `size / factor` under one root scale; that is how document
page zoom works. What is missing is the host half for chrome.

## 2. The mechanism, stated once

One effective layout scale:

```
layout_scale = device_scale * zoom
```

Layout runs at `window_physical / layout_scale`; rasterization runs at
`layout_scale`; every logical coordinate the host exchanges with the
application (cursor, wheel, leaf boxes, hit-testing, `AppCtx::logical_size`,
the harness) is in post-zoom layout space. The device scale stays the truth
for everything that belongs to the window rather than the document:
host-drawn decorations, the monitor clamp on the initial size, window
geometry persistence, the IME candidate area, the physical capture size.

**One stated exception, ruled by Mark on 2026-09-03: an *application-drawn*
frame's insets are document geometry and carry zoom.**
`HostOptions::app_frame_insets` is documented in CSS pixels and the
application draws that margin out of its own stylesheet, so what lands on the
glass is `inset * layout_scale`; a region reserved at `inset * device_scale`
would name a boundary the application is not drawing to. Everything that
belongs to the window itself — the list above, and a *host*-drawn frame —
stays on the device scale. The rule and its arithmetic live on
`AppFrameInsets::scaled`; see the Findings entry below.

Every `scale_factor()` call site in cambium-rootstock and
cambium-genet-winit-host is audited into exactly one of those two buckets.
That audit is the work; the arithmetic is a line.

## 3. Decisions recorded 2026-09-03 (Mark)

- **Two knobs, one mechanism.** `HostOptions` gains `ui_zoom: f32`
  (default 1.0) and `fit_design: Option<(f32, f32)>`. With `fit_design` set,
  the host recomputes zoom on every resize as
  `min(available_w / design_w, available_h / design_h)`, so a game asks for
  "fit 1100x820" and never handles resize itself. A runtime setter exists for
  the explicit knob, so turnstone applies its persisted preference and the
  keys below step it. A pure `fit_zoom(design, available)` helper is public so
  a consumer can compute the same number itself.
- **The host owns the keyboard convention, the app can veto.** Ctrl+plus,
  Ctrl+minus, Ctrl+0 and Ctrl+wheel step the browser ladder
  (0.5, 0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3). The app's
  `key_intercept` runs first and consuming the chord vetoes the default. With
  `fit_design` set, a keyboard step becomes a user offset multiplied onto the
  fit factor, and Ctrl+0 clears the offset rather than forcing 1.0.
- **Persistence is the application's.** The host reports a zoom change
  through the hook context; an application that wants it durable writes it
  through the settings contract's existing `ui_zoom` axis. The host never
  persists.
- **Pixel-art protection is the consumer's, as a toggle.** Isometry's board
  applies its own scale so tiles land on a multiple of device pixels. Integer
  rounding is a setting, default on for the board, off allowed, because there
  are conditions where the shimmer is no big deal. Nothing in the host knows
  about pixel art.

## 4. Gates

**Z0 — The audit.** Every `scale_factor()` read in cambium-rootstock
(`host.rs`, `frame.rs`, `input.rs`, the a11y sync) and
cambium-genet-winit-host (`lib.rs`, `decorations.rs`, `windows_snap.rs`,
`x11_frame.rs`) is classified layout or device in a short table in this
plan's Findings, with the reason. `Host::layout_scale()` is introduced and
the layout-bucket sites call it; the device-bucket sites keep
`scale_factor()`. Zoom is still fixed at 1.0.
**Done when:** the table exists, the sites are switched, every existing host
and cambium test is green, and a new harness test lays out at zoom 1.5 and
asserts `logical_size` is `window / 1.5`, a click at a scaled point hits the
scaled element, and the accessibility root transform carries the layout
scale.

**Z1 — The knobs.** `HostOptions::ui_zoom`, `HostOptions::fit_design`, the
runtime setter (through `AppCtx` or a `WindowCommand`, whichever matches
how sheet swaps are requested today), the `fit_zoom` helper, and a zoom
change reported to the application through the hook context. A zoom change
relayouts at the new logical size in the next frame and invalidates the
raster.
**Done when:** a harness test sets `fit_design (1100, 820)` on a 1100x752
surface and observes zoom 752/820 with `logical_size` 1100x820 (within
rounding); resizing the surface recomputes it; the explicit setter wins over
nothing and composes with the fit factor as §3 says.

**Z2 — Rendering under zoom.** `frame.rs` rasterizes at `layout_scale`; leaf
fragments and the IME area are placed correctly; the physical capture size
is unchanged. Text is laid out and hinted at the layout scale, not scaled as
a bitmap.
**Done when:** a headed run of woodshed at zoom 1.25 shows crisp text at the
larger size (capture, not by eye: glyph edges are not resampled), and a run
at 0.9 fits more content in the same window.

**Z3 — Keys and wheel.** The ladder, the four chords, Ctrl+wheel, the app
veto through `key_intercept`, the fit-offset semantics.
**Done when:** harness tests step the ladder both ways, clamp at its ends,
reset with Ctrl+0, and prove that an app returning `true` from
`key_intercept` for Ctrl+plus leaves zoom unchanged.

**Z4 — Receipt in the reference consumer.** Woodshed sets nothing and is
unchanged at zoom 1.0 (a before/after capture is byte-identical). Turnstone
is not on this host yet and is out of scope; a Findings line records how it
will apply its persisted `ui_zoom` when it moves.
**Done when:** woodshed's existing headed receipt is unchanged and its
tests are green.

**Z5 — Isometry consumes it (isometry repo).** `fit_design: Some((1100.0,
820.0))`; the board takes a scale that lands tiles on an integer multiple of
device pixels when the board's integer-rounding setting is on (default), and
the raw fractional zoom when it is off. Hit-testing stays exact under either.
The whisper self-test capture shows the composer.
**Done when:** the self-test captures on this display show the whole panel
at 1100x752, tiles at an integer device multiple with rounding on, the
setting toggles at runtime, and the existing harness receipts still pass.

## 5. Stop rules

- No consumer other than isometry and the reference check in woodshed is
  touched by this plan. Turnstone's own host is not this host.
- The device bucket never sees zoom. A *host*-drawn frame's geometry that
  changes with zoom is a bug, not a feature. (This rule read "a frame inset"
  until 2026-09-03, when §2's exception was ruled: an app-drawn frame's insets
  are document geometry and must change with zoom, because the application's
  own stylesheet is what draws them.)
- Nothing is persisted by the host.
- A behavior change at zoom 1.0 in any consumer stops the work.

## Findings

### 2026-09-03 — the Z0 audit

Every read of a scale in the three crates, classified. **layout** means the
value belongs to the document and must carry zoom; **device** means it belongs
to the window and must not. Line numbers are post-change.

| # | Site | Bucket | Why |
|---|---|---|---|
| 1 | `cambium-rootstock/src/lib.rs:79` `HostWindow::scale_factor` | device | The seam's definition. A window reports physical over device-logical and knows nothing about zoom. |
| 2 | `host.rs:868` `Host::scale_factor` | device | The device scale itself, now documented as such and as the source of the multiply below. |
| 3 | `host.rs:894` `Host::layout_scale` (new) | — | `scale_factor * ui_zoom`. The one number §2 names. |
| 4 | `host.rs:907` `Host::available_size` (new) | device | `fit_design` is measured against the pre-zoom surface, or the fit would chase its own output. |
| 5 | `host.rs:921` `Host::logical_size` | **layout** | The size laid out at, and the space the cursor, leaf boxes and `AppCtx` all live in. |
| 6 | `host.rs:935` `Host::layout_point` (new) | **layout** | A platform point in physical pixels, divided once, in one place. |
| 7 | `host.rs:1012` `publish_titlebar_area` | device→layout | The platform reserves a *device* strip (macOS traffic lights are a fixed size on screen); the sheet declares CSS pixels. Divided by zoom on the way in. |
| 8 | `frame.rs:324` `sync_ime_area` | layout→device | The caret rect is layout; `set_ime_cursor_area` takes the platform's logical. Multiplied by zoom on the way out. |
| 9 | `frame.rs:421` `redraw` target size | **layout** | Both uses: `(lw, lh) = physical / layout_scale`, and `rasterize_scaled(..., layout_scale)`. |
| 10 | `frame.rs:604` `sync_a11y` | **layout** | The projected boxes are layout; AccessKit wants physical client pixels. |
| 11 | `cambium-winit-a11y/src/lib.rs:120` `scale_tree_to_window` | **layout** | Same transform, now passed in rather than read off the window. |
| 12 | `winit-host/lib.rs:168` `WinitWindow::scale_factor` | device | Forwards winit. The seam. |
| 13 | `winit-host/lib.rs:379` `refresh_snap_layout_rect` | **layout** | The maximize control's rect comes out of the retained layout; Win32 wants physical. Was the device scale, which would have put the Snap Layout hit rectangle where the button is not. |
| 14 | `winit-host/lib.rs:522` `edge_under_cursor` | device | The 8px border grab is the window's affordance and stays 8 device pixels. The layout-space cursor is multiplied back across the zoom rather than the window's size being dragged into layout space. |
| 15 | `winit-host/lib.rs:609` monitor list | device | Monitor rectangles for the restored-geometry reachability check. |
| 16 | `winit-host/lib.rs:631` primary-monitor clamp | device | The initial size clamp is about the display, not the document. §2 names this one. |
| 17 | `winit-host/lib.rs:~860` `CursorMoved` | **layout** | Now `Host::layout_point`. |
| 18 | `winit-host/lib.rs:886` `MouseWheel` `PixelDelta` | **layout** | A flick must travel the same distance under the finger as the pointer does. `LineDelta` is a count of lines and is still unscaled. |
| 19 | `decorations.rs:280` `ShowSystemMenu` | layout→device | The cursor is layout; winit's `Position::Logical` is the platform's. |
| 20 | `decorations.rs:305` `geometry()` | device | Window position and size for persistence. Nothing to do with the document. |
| 21 | `x11_frame.rs:36` `publish_gtk_frame_extents` | **layout** | §2's one exception, ruled 2026-09-03. `app_frame_insets` are CSS pixels the *application's* stylesheet draws, so the reserved region must be the drawn one: `inset * layout_scale`. The scale comes from the caller now, not off the window. |
| 22 | `windows_snap.rs:126` `device_rect` | — | Takes the scale as an argument; site 13 decides which. |
| 23 | `decorations.rs` `WindowCommand::Resize` | device | An application asking for a window size means a window size. |
| 24 | `input.rs` (whole file) | **layout** | No scale is read anywhere in it. Cursor, hit tests, wheel deltas, caret positions and `local_in` are all already in one space, which is why the audit is short. |
| 25 | `capture.rs`, `owned_layout.rs`, `spatial.rs`, `wake.rs` | — | No scale read. Physical sizes and layout coordinates respectively. |

Counts, **as amended 2026-09-03** when site 21 moved buckets: **11 layout**,
**10 device**, 4 sites that read no scale or take one as an argument. (The
table was first published as 10 / 11.) Two sites were *wrong before this work
and are repaired by it* (13, and the `PixelDelta` half of 18 that the
2026-09-02 fix had already moved to the device scale and which now needs the
layout scale); three are new crossings the audit forced into the open (7, 8,
19); one was left ambiguous and is settled below (21).

### 2026-09-03 — site 21 ruled: frame insets are CSS pixels and carry zoom

**Mark's call on the audit's one ambiguous site.** `HostOptions::app_frame_insets`
are CSS pixels, so `_GTK_FRAME_EXTENTS` is published at the **layout** scale.
The reason is that the field is documented in CSS pixels and the *application*
draws that frame from its own stylesheet: what it puts on the glass is
`inset * layout_scale`, and the region reserved has to be the region drawn. At
device scale 2 and zoom 1.25, an 8 CSS px margin is 20 device pixels;
publishing `inset * device_scale` claims 16 and tells Mutter the frame's edge
is four pixels inside where the application is painting. §2 gains the
exception, §5's stop rule is narrowed to a *host*-drawn frame, and the table
row above moves to layout.

**Nothing observable changes today.** The path needs an app-drawn frame, on
X11, at a zoom other than 1 — three conditions, and no shipping consumer meets
the third. At zoom 1 `layout_scale()` is bit-for-bit `scale_factor()`, so the
published numbers are what they were. This is correctness of the record, and
of the code the first time it is reached, rather than a fix to observed
behaviour.

**Windows and macOS do not read the field for geometry, and need no change.**
Checked every reader of `app_frame_insets` in the workspace: `x11_frame.rs`,
`HostOptions::effective_app_frame_insets` / `app_frame_is_transparent` (a
boolean — whether the surface needs alpha, which no scale touches), and
`examples/smoke.rs`. Windows' only frame geometry is the Snap Layout rect
(site 13, already the layout scale) and the 8-pixel border grab (site 14,
correctly the device scale); its app-frame path passes `decorations(false)`
and draws the caption in the page, reserving no inset. macOS never sees the
field either: `decorations::titlebar_insets` measures the traffic-light rect
off `NSWindow` in device-logical points, and `publish_titlebar_area` (site 7)
already divides it by zoom on the way into the sheet. So the exception is
X11's alone, which is why §2 can state it in one sentence.

**Where the arithmetic lives, and how it is receipted.** `x11_frame` is
`#[cfg(target_os = "linux")]`, so nothing in it compiles or runs on the
Windows development machine — its existing inset test never executed here.
The rule was therefore moved to `AppFrameInsets::scaled(layout_scale)` in
`cambium-rootstock/src/host.rs`, beside the doc comment that declares the
insets to be CSS pixels; `x11_frame::device_extents` is now only the
property's word order. `app_frame_insets_are_css_pixels_and_carry_zoom` runs
on every platform. The X11 module was additionally type-checked and its two
tests run on Windows by temporarily un-gating the module and adding `x11rb`
to the Windows target (both reverted): 18 passed, 0 failed. The atom write
itself is not testable without an X server; exercising it wants a headed run
on the Fedora or Mint box with `CAMBIUM_HOST_FRAME_TRACE=1`, an app-frame
consumer and `CAMBIUM_UI_ZOOM=1.25`, reading the property back with
`xprop -id <xid> _GTK_FRAME_EXTENTS`.

**Recorded, not decided: when the property is republished.** It is written at
window creation and on `ScaleFactorChanged`, which was sufficient while the
value depended only on the device scale. Now it also depends on zoom, and
neither a keyboard step nor a `fit_design` recompute on resize republishes it,
so the property can go stale in exactly the situation the ruling is about.
Three defensible answers — republish beside `refresh_fit_zoom()` in the
`Resized` and `ScaleFactorChanged` arms (covers the fit, misses Ctrl+plus);
republish from `redraw()` behind a last-published-value guard (covers
everything, costs a comparison per frame); or leave it, since the whole path
is unreachable. **Mark's call**, and out of scope for the ruling itself.

### 2026-09-03 — the render half needed no `scaled_to`

`LiveryPaintList::scaled_to` is not used and should not be. `rasterize_scaled`
already ends in netrender appending the master scene under one root
`Affine::scale` (`netrender/src/vello_tile_rasterizer/mod.rs`, the
`scaled_master` branch), and vello then rasterizes the scaled **vectors** at
the physical target size. Passing `layout_scale` there instead of the device
scale is the whole render change; pushing a second scale into the paint list
would double it.

The same reading answers "does the layout scale reach text shaping": **there is
no scale to reach it.** Livery shapes and breaks lines in CSS pixels and has no
device-scale input anywhere. Zoom therefore works exactly as browser page zoom
does — the CSS pixel is unchanged, the viewport shrinks in CSS pixels, the text
re-wraps, and hinting and antialiasing happen in the rasterizer at the final
device size. Receipted two ways: `text_is_relaid_out_at_the_new_logical_width`
(the same prose wraps to more lines in a narrower CSS viewport, which no bitmap
scale can do), and the headed capture below.

### 2026-09-03 — headed receipt, and why it is not woodshed

Artifacts and full numbers: `Code/testing/genet/ui_zoom/`. Collected with the
host's own `examples/smoke.rs` — the same host, window, GPU and in-process
readback — at zoom 1.0 (twice), 1.25 and 0.9, on a 200% display.

- **The physical frame never moves**: 1800x1280 and 1120x800 at every zoom.
- **The interface scales inside it**: the 240 CSS px `.rail` measures 480, 600
  and 431 device pixels at layout scales 2.0, 2.5 and 1.8.
- **Glyphs are re-rasterized, not resampled**: the maximum per-pixel luminance
  step along a text row is 211 — the whole swing between the text and its
  ground — at *every* zoom. Interpolating a one-pixel edge across 1.25 pixels
  caps that step near 169, so the 1.25 frame cannot be an upscale of the 1.0
  frame.

**Woodshed could not be built on 2026-09-03**, for reasons outside this work:
`woodshed/Cargo.toml` is uncommitted-modified by another session and no longer
declares the `workbench` workspace dependency `crates/woodshed-views` inherits,
so `cargo metadata` fails before resolution; and `woodshed/.cargo/config.toml`
redirects the whole genet graph to `.../worktrees/genet-workbench`, a checkout
that does not exist on this machine. Repairing either means editing woodshed,
which this work is not allowed to do.

### 2026-09-03 — two things the headed pass turned up, neither zoom's

- **The smoke scenario's selector coordinates come from `genet-probe`'s own
  re-layout**, not from the shipping layout, so which control a resolved point
  lands on drifts. `assert snap level >= 45` fails at zoom 1.0 and 1.25 and
  *passes* at 0.9, where the later `Reset` assertions fail instead. At zoom 1.0
  the probe's inputs are byte-for-byte what they were before this work, so the
  drift is upstream of it — the tree carries another session's uncommitted
  `genet-livery` box-tree, `build_block`, `build_inline` and `style` changes.
  `Harness::resolve` already avoids this hazard deliberately and its receipts
  are unaffected; `examples/smoke.rs` does not.

- **A fractional layout scale leaves one unpainted row or column.** Measured:
  the `resized` capture at zoom 0.9 has exactly 1120 fully transparent pixels,
  one row of a 1120-wide frame. `frame.rs` emits the scene at
  `DeviceIntSize::new(lw as i32, lh as i32)` — a truncation — and netrender
  renders `round(viewport * scale)`, so 800 physical over a layout scale of 1.8
  becomes `trunc(444.44) = 444` and then `round(444 * 1.8) = 799` into an
  800-tall texture. Every capture whose physical size divides exactly by its
  layout scale reports zero transparent pixels. **Pre-existing, not zoom's**:
  the same arithmetic misses today with no zoom at all on any 150% display
  (1000 physical over 1.5 is 666.67). Zoom only makes fractional layout scales
  ordinary rather than rare. The repair belongs in netrender, whose render size
  should be the target's rather than the viewport's times the scale — a
  sibling-repo change, so it is recorded rather than made.

### 2026-09-03 — how turnstone applies its persisted `ui_zoom`

Turnstone is not on this host and was not touched. When it moves, its
persisted preference is one assignment and one hook:

- At launch, `HostOptions { ui_zoom: settings.ui_zoom, ..Default::default() }`.
  Nothing else: the host seeds its runtime zoom from the option in `Host::new`,
  so the first frame is already at the persisted size.
- At runtime, read `AppCtx::ui_zoom` in `after_dispatch` (or `after_frame`) and
  write it back through the settings contract when `AppCtx::zoom_changed` is
  true. That flag is the **edge**, raised in exactly one hook per change, so
  the settings write happens once per Ctrl+plus rather than once per frame.
- To *set* it from turnstone's own preferences UI, assign
  `*ctx.set_ui_zoom = Some(zoom)` inside any hook; the host applies it when the
  hook's borrows end, exactly as `set_sheet` is applied.

The host never persists, per §3. `ui_zoom` composes with `fit_design` when
both are set, so a turnstone that later wants a design fit keeps its stored
preference as the user's offset on it.

## Progress

- **2026-09-03.** Plan founded; decisions in §3 taken with Mark.
- **2026-09-03. Z0 landed.** The audit table above; `Host::layout_scale`,
  `Host::available_size` and `Host::layout_point` introduced and the ten
  layout-bucket sites switched to them. `Accessibility::sync` takes the layout
  scale (breaking; `cambium-genet-web-host` updated with it). All 58
  pre-existing host, a11y and cambium tests green unchanged, plus
  `tests/ui_zoom.rs`: `logical_size` is `window / 1.5`, a click at the scaled
  point hits the scaled element and arrives in layout coordinates, and the
  accessibility root transform is `Affine::scale(1.5)`.
- **2026-09-03. Z1 landed.** `HostOptions::{ui_zoom, fit_design}`, the public
  `fit_zoom` and `ladder_step` helpers and `ZOOM_LADDER`, the runtime setter as
  `AppCtx::set_ui_zoom` (**through `AppCtx`, not a `WindowCommand`** — zoom is
  document-side, it must work in a windowless harness, and a window verb is
  drained only by an event source that has a window), and the change reported
  as `AppCtx::{ui_zoom, zoom_changed}`. A zoom change zeroes `layout_size`,
  which forces the next frame down the rebuild path — the branch that carries
  both scroll planes across, so Ctrl+plus does not snap a scrolled interface
  back to the top. **The plan's Z1 done-when has an arithmetic slip**: with
  `min` of the two ratios, a design of 1100x820 on a 1100x752 surface gives
  zoom 752/820 and a logical size of **1199.5x820**, not 1100x820. Only the
  binding axis lands on the design figure; the other keeps its slack, which is
  what `min` is for. The test asserts the correct pair.
- **2026-09-03. Z2 landed.** `frame.rs` rasterizes at `layout_scale`; the IME
  area and the published titlebar strip cross the zoom explicitly; leaf boxes
  and capture size are unchanged. Headed receipt in
  `Code/testing/genet/ui_zoom/`, and the two non-zoom defects it surfaced are
  in the Findings above.
- **2026-09-03. Z3 landed.** The ladder, Ctrl+plus/equals, Ctrl+minus, Ctrl+0
  and Ctrl+wheel. The chords run **after** `key_intercept` (so returning `true`
  vetoes them) and **before** `dispatch_key` (so a focused text field cannot
  swallow Ctrl+plus as typed text); Ctrl+wheel is a host default gated on the
  wheel handler's `prevent_default`, like the scrolling default beside it.
  Shift is ignored rather than rejected, because `Ctrl+Shift+=` is how a plus
  sign is typed on a US layout.
- **2026-09-03. Z4 landed, with a named substitution.** Woodshed could not be
  built (see Findings); the zoom-1.0 claim is carried instead by the arithmetic
  — `ui_zoom()` is exactly `1.0f32`, every new crossing is a multiply or divide
  by it, and `layout_scale()` is bit-for-bit `scale_factor()` — by the 58
  pre-existing tests passing unchanged, by `zoom_one_is_the_identity` in
  `tests/ui_zoom.rs`, and by two headed zoom-1.0 runs whose `busy` and
  `resized` digests are byte-identical to each other and differ at every other
  zoom. The `opened` capture is **not** byte-reproducible run to run even with
  no change at all — it races the accessibility reveal — so a byte-identical
  before/after comparison is only meaningful on the other two.
- **2026-09-03, Z5 landed in isometry (uncommitted).** `fit_design
  (1100, 820)`; on this laptop the clamped 1100x752 window gives zoom 0.9171
  and `logical_size` 1199.5x820. The board rounds its 8 px elevation unit
  (the 4:2:1 projection's finest step) to whole device pixels behind a
  default-on `integer_pixel_rounding` toggle, rewriting its own geometry so
  emission and hit-testing share one projection; no CSS transform. At zoom
  1.0 the emitted DOM is byte-identical. Seven new receipts; 313 tests.
  **The design figure was wrong:** the side panel measures 1038 logical
  pixels on the demo board, not 820, so at 820 the composer is still below
  the fold; at a trial 1040 (zoom 0.7231) it is inside the frame. Which
  figure isometry ships is Mark's call, recorded in the isometry plan.
  Seam gap recorded: `AppCtx` exposes no laid-out geometry, so a self-test
  cannot print an element's painted edge; the harness can.
- **2026-09-03, Z4's zoom-1.0 claim receipted against a clean HEAD.** The
  host smoke scenario was run in a throwaway worktree at `8c1e324` (HEAD,
  none of today's uncommitted genet work) and in the working tree at zoom
  1.0, with captures saved. The `busy` (1800x1280) and `resized`
  (1120x800) frames are pixel-identical between the two: 0 differing
  pixels, max channel delta 0. The scenario's `assert snap level >= 45`
  fails identically on clean HEAD (`got 0`), so that drift predates the
  secondary-press, clip-scope, stacker, wheel and zoom work. The worktree
  was removed afterwards.
- **2026-09-03, the zoom-1.0 receipt corrected, and the instrument with it.**
  The Z4 claim rested on two headed frames agreeing byte for byte, once
  between the two zoom-1.0 runs and once against a clean HEAD worktree. That
  comparison is not decisive: **the capture is not deterministic.** Three
  runs of one unchanged binary return the `busy` frame under two digests
  (`c0d00467bf8493b3`, `27c01a961612a3ba`) differing by a single channel of a
  single pixel by one, and a netrender pass reproduced the flip six times and
  pinned it by building a control with the old arithmetic, which returned the
  other digest. The frames that agreed agreed by luck. A single-run digest
  comparison cannot settle a claim at one-pixel precision, and no future
  receipt in this plan should use one.
  What carries Z4 instead is mechanism, which held all along: at zoom 1.0
  `ui_zoom()` is exactly `1.0f32`, `layout_scale()` is then bit-for-bit
  `scale_factor()`, the harness test `zoom_one_is_the_identity` asserts it,
  and a temporary diagnostic logging every rasterize call whose derived size
  differed from its target emitted nothing across a full zoom-1.0 scenario.
  A note carrying this correction is attached to `86019ea`, whose message
  makes the overstated claim and which was already pushed.
- **2026-09-03, "integer scales are already exact" was wrong, and netrender
  had the truncation.** Exactness needs the scale to *divide the target*, not
  merely to be an integer: an 801-pixel surface at device scale 2, or 64 at
  3, lost a row with no zoom involved. `render_scaled_with` rebuilt the render
  size as `round(scene.viewport * scale)` from a viewport the host had already
  truncated when it divided physical by scale, so 800 over 1.8 laid out at 444
  and rendered 799 rows of an 800-row texture. It now reads the size off the
  target view's own texture and leaves `scale` to the root affine. The zoom
  0.9 `resized` frame goes from **1120 transparent pixels to 0**. One site
  only; the compositor path is scale-free and cannot honour a layout scale at
  all, which is a separate gap recorded in netrender's verification record
  §11.37.
- **2026-09-03, the Z0 audit's one ambiguous site ruled and closed.** Frame
  insets on an *app-drawn* frame are CSS pixels and carry zoom (Mark). §2
  gains the exception, §5's stop rule narrows to a host-drawn frame, row 21 of
  the audit table moves to the layout bucket and the counts become 11 / 10. In
  code: `AppFrameInsets::scaled(layout_scale)` in `cambium-rootstock` now owns
  the arithmetic and the rule's doc comment, `x11_frame::device_extents` is
  reduced to the property's word order, and `publish_gtk_frame_extents` takes
  the scale from its caller — `self.layout_scale()` — instead of reading
  `window.scale_factor()` itself. Windows and macOS were checked and need
  nothing. Nothing observable moves: the path needs an app-drawn frame, on
  X11, at a zoom other than 1. 23 rootstock and 75 host tests green, including
  the new platform-independent
  `app_frame_insets_are_css_pixels_and_carry_zoom`. The republish-cadence
  question the ruling exposes is left open for Mark in the Findings entry
  above.
