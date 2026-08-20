# Window decorations: generalizing woodshed's CSD across the stack

**Date:** 2026-08-10
**Status:** W0, W1 and W2 landed, plus window-geometry validation from §6. W1
has same-stylesheet headed receipts on Windows, Intel macOS and Apple Silicon
macOS. W3's Windows maximized-overflow receipt also landed; its Linux halves
and W4 remain staged with reasons. See §8. Originally a research brief, kept as
the design record.
**Scope:** the window frame itself — title bar, caption buttons, resize
borders, shadow — for every Cambium desktop app, plus the browser/PWA lane.
Not in scope: in-app chrome (shellbar, panes, toolbars), which is product UI.

**Related:**

- `components/cambium/cambium-genet-winit-host` — the extracted host, which
  already owns half of this.
- Woodshed's [genet host cross-platform plan](https://github.com/merely-made/woodshed)
  (`design_docs/2026-07-04_genet_host_cross_platform_plan.md`), whose 2026-07-05
  slice 8 and 2026-07-08 polish tail built the CSD this brief generalizes.
- Retinue's `2026-08-09_signalman_cambium_desktop_scope.md` — the consumer
  that will want a frame next.
- The W3C standards review (mere, 2026-07-05) — "WPT subsets as done-condition
  currency" applies directly; see §5.

---

## 1. The term

"Window chrome" is fine colloquially, but the precise term for what woodshed
does is **client-side decorations (CSD)**: the application paints its own
non-client area instead of the window manager painting one for it. The
opposite is **server-side decorations (SSD)**, which is what produces the
candy-apple-red Windows title bar that started this.

The pieces have their own names, and they matter because the platforms treat
them differently:

- **caption buttons** (Win32) / **traffic lights** (macOS) — the
  minimize/maximize/close trio.
- **non-client area** (Win32 `WM_NC*`) — everything outside the client
  rectangle: title bar, borders, resize grips.
- **decorations** (winit, Wayland `xdg-decoration`) — the whole frame.

## 2. What is already general, and what is not

The host extraction on 2026-08-09 quietly generalized the *harder* half.
`cambium-genet-winit-host` already owns, for every consumer:

- `HostOptions::decorations`, so an app opts into CSD by construction;
- eight-direction edge resize with an 8px grab margin (`resize_edge`);
- the resize cursors an undecorated window gets from nobody
  (`edge_cursor` + `update_resize_cursor`, deduped on transition);
- monitor-clamped initial sizing and position.

What did **not** generalize, and is still woodshed-local:

- **the title bar view itself** (`desktop_chrome`: title text, drag surface,
  three buttons, `.chrome-*` CSS);
- **the drag/minimize/maximize/close plumbing**, which is four `bool` flags on
  `UiState` that the host drains after every dispatch;
- **the theming** that makes it match the app rather than the OS.

The flags are the part worth replacing rather than lifting. They work, but
they mean every app that wants a frame must add four fields to its state and
a drain block to its `after_dispatch`, and the host must know a product's
state shape. §5 proposes the seam that removes both.

## 3. The cross-platform matrix (where CSD has teeth)

CSD is not one feature; it is three different negotiations with three window
systems. The honest per-platform picture:

| Concern | Windows 10/11 | macOS | Linux/Wayland | Linux/X11 |
|---|---|---|---|---|
| Who draws the frame | app, once undecorated | **OS, always** (traffic lights are not ours to draw) | app on GNOME/Mutter (SSD unsupported); compositor on KDE if asked | app, or WM |
| Protocol | `WM_NCCALCSIZE` / `WM_NCHITTEST` | `NSWindow` style masks | `xdg-decoration` (optional; GNOME refuses SSD) | `_MOTIF_WM_HINTS` |
| Shadow when undecorated | DWM keeps it if the frame is extended, loses it if simply removed | OS | compositor | **lost**; needs `_GTK_FRAME_EXTENTS` |
| Snap/tiling affordance | Snap Layouts on hover-maximize, **requires returning `HTMAXBUTTON`** | green button menu (OS) | compositor gesture | WM |
| Fallback library | — | — | `libdecor` | — |

Three findings shape any design here:

**macOS is not a CSD platform in the way the other two are.** The correct
macOS pattern is *not* to draw three buttons; it is `fullSizeContentView` +
`titlebarAppearsTransparent`, letting content extend under the real traffic
lights, which keep their hover glyphs and the green button's window-arrangement
menu. Apps that draw their own controls on macOS read as wrong to Mac users,
and Tauri's own issue tracker is full of the resulting traffic-light
repositioning bugs. **So the portable abstraction cannot be "our three
buttons everywhere."** It has to be "reserve a region; the platform fills it
where it has controls, we fill it where it does not."

**Windows 11 Snap Layouts need a native answer we do not have.** Hovering the
maximize button should raise the layout picker; that requires returning
`HTMAXBUTTON` from `WM_NCHITTEST`, which winit has not exposed
([winit#3884](https://github.com/rust-windowing/winit/issues/3884), opened
2024-08-21 and still open with no linked PR when re-checked 2026-08-17). The known-good workaround, from `tauri-plugin-frame`, is a small native
child `HWND` over the custom maximize button that answers `HTMAXBUTTON`. Until
that exists, an undecorated Windows app silently loses a feature users have.

**The undecorated-maximize overflow is the classic Windows CSD bug.** A
frameless window, maximized, extends past the work area by the resize-border
width, so its edges spill onto adjacent monitors and it can cover the taskbar.
The fix is handling `WM_NCCALCSIZE` and insetting when maximized. Worth a test
in the matrix rather than a later bug report.

**GNOME/Wayland is the easy case** and the reason to bother: Mutter supports
only CSD, so an app that draws its own frame is *more* native there, not less.

## 4. Prior art worth reading

| Source | What to take |
|---|---|
| [Window Controls Overlay](https://wicg.github.io/window-controls-overlay/) (WICG) | The model itself, and the CSS vocabulary — see §5 |
| [`tauri-plugin-frame`](https://crates.io/crates/tauri-plugin-frame) | The child-HWND trick for Snap Layouts; the only shipping Rust answer |
| [libdecor](https://xeechou.net/posts/libdecor/) | The Wayland fallback when a compositor wants SSD and we have no frame |
| [NSWindowStyles](https://github.com/lukakerr/NSWindowStyles) | The catalogue of what `NSWindow` masks actually produce |
| [Microsoft's Snap Layout guidance](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/ui/apply-snap-layout-menu) | The `HTMAXBUTTON` contract, from the platform owner |
| Woodshed's own polish tail | The bugs already paid for: content-box overflow clipping the ×, the off-screen × stealing clicks from another window, the 8px grab margin, the taskbar-clearing default height |

## 5. The recommendation: express it in CSS, not in host flags

The stack already has a doctrine for this — standards-correct over host hacks
— and the standard exists. The Window Controls Overlay spec standardizes
Chromium's proprietary `-webkit-app-region` as the **`app-region`** CSS
property (`drag` / `no-drag`), and defines four CSS environment variables
(`titlebar-area-x/y/width/height`) describing the region left over beside the
platform's own controls.

That is exactly the abstraction §3 demanded, and it is already a spec.

**The proposal in one line:** teach livery `app-region`, teach the host to
read drag regions out of the laid-out DOM instead of a `chrome_drag` bool, and
publish the reserved title-bar rect to app CSS as the standard env variables.

What that buys:

- **The four `UiState` flags disappear.** A drag surface becomes
  `app-region: drag` in the app's stylesheet; the host hit-tests it like any
  other region. No product state, no drain block, no host knowledge of an
  app's shape.
- **macOS becomes correct for free.** On macOS the host reserves the traffic
  lights' rect and reports the remainder through `titlebar-area-*`; the app's
  own CSS lays its title and controls out in whatever is left. The same
  stylesheet that draws three buttons on Windows draws none on macOS, because
  the env variables told it the region was smaller.
- **The browser/PWA lane gets it at no extra cost.** A woodshed or turnstone
  PWA installed with `display_override: ["window-controls-overlay"]` gets the
  same layout from the same CSS. Genet is a browser engine; this is a
  capability it should have anyway.
- **The done-condition currency already exists in-tree.** Genet's vendored WPT
  suite *already carries the WCO tests*, unrun:
  `tests/wpt/tests/appmanifest/display-override-member/` holds both
  `...app-region-window-controls-overlay.webmanifest` and
  `...css-environment-variables-window-controls-overlay-manual.tentative.html`.
  The conformance target is sitting in the repository waiting for an
  implementation.

The host keeps only what genuinely cannot be CSS: the native calls
(`drag_window`, `drag_resize_window`, `set_minimized`, `set_maximized`), the
platform quirks in §3, and the reserved-rect computation it feeds back in.

## 6. Features worth having beyond parity

Ordered by value per unit of work, and each one is a thing woodshed's current
frame does not do:

1. **Accessible window controls.** The host already syncs an AccessKit tree,
   but hand-drawn caption buttons carry no roles or names unless the app
   supplies them. A shared frame can guarantee `button` + "Minimize" /
   "Maximize" / "Close" everywhere, plus keyboard reachability. This is the
   one that turns a nicety into a correctness fix.
2. **Double-click to maximize, and the system menu.** Double-clicking a title
   bar toggles maximize on every platform; right-click (or Alt+Space) opens
   the system menu on Windows. Both are muscle memory, both are missing.
3. **Snap Layouts** (§3), once the child-HWND route is worth its cost.
4. **Window state persistence.** Position, size, and maximized-ness restored
   across launches, monitor-validated so a window on a since-unplugged
   display does not open off-screen. The host already clamps to the primary
   monitor at boot; this is the memory half.
5. **Chrome that follows the theme, including the OS accent.** Tinct already
   derives palettes from a seed; reading the system accent colour as an
   optional seed would make the frame feel native without being native. Also
   the honest place to respect high-contrast and reduced-motion.
6. **Drag-to-edge and multi-monitor DPI**, mostly free via `drag_window`, but
   worth an explicit receipt because DPI-change-mid-drag is a classic
   crasher.
7. **A titlebar that can host content.** Once the region is expressed as CSS,
   an app can put a search field or tab strip up there (the reason WCO exists).
   Turnstone wants this; woodshed does not.

Deliberately *not* recommended: a shared "titlebar component" with a fixed
look. The frame is per-product identity, and the stack's rule is that a
component earns promotion from a second real consumer, not from anticipation.
What generalizes is the *seam* (`app-region`, the env variables, the native
calls, the platform quirks), not the pixels.

## 7. Suggested phasing

- **W0 — the seam.** `app-region` in livery + the host reading drag regions
  from layout; woodshed's four flags deleted. Done when woodshed drags,
  maximizes, and closes with no chrome fields in `UiState`.
- **W1 — the reserved region.** `titlebar-area-*` env variables published by
  the host, and the macOS `fullSizeContentView` path that makes them nonzero
  there. Done when one stylesheet lays out correctly on Windows and macOS.
- **W2 — accessibility and the muscle memory.** Roles/names on the controls,
  double-click maximize, the system menu. Done when the frame is keyboard- and
  screen-reader-complete on the Windows route.
- **W3 — the platform quirks.** Maximized overflow, X11 `_GTK_FRAME_EXTENTS`
  shadow, the Wayland SSD-preferring compositor fallback. Done when the four
  desktop targets each have a receipt.
- **W4 — Snap Layouts**, and the WPT WCO subset as the browser-lane receipt.

W0 and W1 are the ones that pay for themselves immediately; W4 is optional
until a Windows-first product ships.

## 8. Progress

**2026-08-10 — W0 landed** (genet `dc7122fc5`, woodshed `45358e0`).

`genet-layout::computed_custom_property` reads a custom property off the
retained cascade, and a cascade test pins the three behaviours the frame
depends on: a declaration on a container reaches a nested descendant, a
descendant overrides it, and unrelated content sees nothing. That is the
containment `app-region` needs, obtained from inheritance rather than an
ancestor walk. `app_region_of` prefers the real longhand and falls back to
the custom property, so the livery cutover is a no-op for stylesheets.

The host gained `decorations.rs`: `AppRegion`, the cloneable
`WindowCommands` handle, `WindowGeometry` with monitor-reachability
validation, and `ClickCadence` (winit reports presses, not clicks). A press
on a drag surface moves the window, a double-click maximizes, a right-click
raises the system menu, and `prevent_default` in a handler vetoes all three.
The DOM always sees the press first, so a drag surface can still take focus
and a `no-drag` control keeps its click.

Woodshed is migrated and is the receipt: `--app-region` in its sheet, the
four `chrome_*` bools deleted from `UiState` (which matters because that
state is the browser host's too), `sync::window_chrome` deleted, and the
drag element demoted to a plain spacer. Its `Logic` became a boxed closure
so the caption handlers can capture the handle instead of setting flags.
377 woodshed tests green; 39 host tests green including eight new frame
tests driven through the real press path.

**W2 landed with it.** Double-click-to-maximize and the right-click system
menu are host-side and free once regions are known. Accessible caption
buttons turned out to be already done in woodshed (role + `aria-label`, with
the spacer `aria-hidden`), and the smoke now demonstrates the same, so the
pattern is documented in two places rather than enforced by a component.

**Two testability gaps closed on the way**, both found by writing the tests:
`Harness::press_at` was calling bare `click` and so bypassed the frame path
entirely (a receipt proving something the shipping build does not do), and
`Harness::with_commands` now exists because a test could otherwise build an
orphaned `WindowCommands` and prove the exact opposite of the truth. The
host also records performed verbs, which is how a windowless harness can
assert the whole matrix.

**The remote-receipt lane exists** (`scripts/remote-receipt.ps1`), so the
staged items below are one command each once a machine is reachable. It runs
an app's existing `.scn` scenario on the remote machine's *own* screen and
brings the receipt, the captures, and a provenance manifest back into
`testing/<repo>/<host>-<stamp>/`.

SSH drives; it does not render. `ssh -X` would draw against the *local*
window manager, which for decoration work would confidently report the wrong
answer, and Wayland has no equivalent worth trusting. So the script attaches
the run to the graphical session already logged in there — `systemd-run
--user` on Linux, `launchctl asuser` on macOS — and refuses to start if there
is no such session, because a headed run with no display draws nothing and
still exits 0. The manifest records the remote OS, the session type, the
remote commit and whether the checkout was dirty, the exact environment, the
exit code, and a SHA-256 per artifact; a capture without that provenance is
not evidence six months later. Authentication is ordinary SSH, so
`personae-agent` serves the vault's SSH slots over the OpenSSH pipe and no key
sits on disk in the clear.

**2026-08-19: W1 landed on Windows and both macOS architectures** (genet
`02a71acbda5`). `HostWindow::titlebar_insets` is the seam: the platform reports
what it reserved along the top edge and the neutral layer turns that into the
four Window-Controls-Overlay values, because computing them needs a window
width the platform layer does not have. They are published as a `:root` rule
carrying `--titlebar-area-*` custom properties, not an inline style on the
root, whose `style` attribute belongs to the application. This stands in for
`env()` exactly as `--app-region` stands in for the `app-region` longhand.
macOS keeps its frame decorated and makes the title bar transparent over a
full-size content view; passing `decorations = false` there is borderless and
takes the traffic lights with it. The insets are measured from the buttons'
own frames rather than assumed at a size Apple has changed before.

The smoke's one stylesheet uses those four values on both platforms and now
writes exact frame readbacks. Windows returned `RESULT ok` with three nonblank,
distinct frames at 840x640 and 1120x800 in
`testing/genet/w1-current-main-windows-2026-08-19_142326/`. The unlocked Intel
iMac ran the same scenario from a clean `02a71acbda5` checkout in its Aqua
session and returned `RESULT ok` with the same sizes in
`testing/genet/192.168.4.105-2026-08-19_152925/`. The macOS frames reserve 138
pixels at the left of the overlay for AppKit's controls, start the app title in
the remaining region, and keep ordinary content below the 32-pixel bar through
resize. Native traffic lights are compositor chrome and therefore absent from
the in-process pixels; Aqua preflight plus the AppKit-measured reservation is
the headed platform half of the proof. The macOS `opened.bmp` SHA-256 is
`0c9d31cbb54c5f6b73a86b3bdd7e2e653d0796b81447e93cf780835f553bff1c`.

Mayola's Apple M4 iMac then ran the same scenario from a clean `b0d5120fe74`
checkout in its Aqua session, returning `RESULT ok` with the same frame counts
and sizes in `testing/genet/192.168.4.57-2026-08-19_153505/`. Its measured
traffic-light reservation is 154 pixels rather than the Intel iMac's 138. The
same stylesheet adapts to both measurements and preserves the content boundary
through resize, which is stronger evidence for the platform seam than two
machines returning the same assumed constant. The M4 `opened.bmp` SHA-256 is
`976a0972e91bba6e7059083faf79c8ef39868eede55fd69e20cafb7259a779c5`.

The iMac compile initially exposed dependency drift: its ignored lockfile still
resolved Netrender before `apply_limit_buckets` landed. Resolving Netrender at
`f6d449843` fixed that compatibility defect; both the compile and headed receipt
then passed on current `main`.

**2026-08-19: W3a Windows maximized overflow verified** (genet
`82caf0d71e6`). The pinned winit already handles `WM_NCCALCSIZE`; this was a
receipt task, not new Win32 frame code. `scripts/windows-maximized-receipt.ps1`
runs the real Cambium CSD window, waits until the semantic scenario has
maximized it, and measures that same HWND before releasing the scenario to
restore.

The clean run in
`testing/genet/w3a-windows-maximized-2026-08-19_171837/` returned `RESULT ok`
from both sides. Win32 reported an outer DWM rect of
`[-7,-7,1286,758]`, but the drawable client and monitor work rect were both
exactly `[0,0,1280,752]`: zero overflow on all four edges. The app captured
three nonblank frames at 840x640, 2560x1504 and 840x640; the restored frames
have the same digest, so maximize/restore returned to the original pixels. The
`maximized.bmp` SHA-256 is
`34c23ad84d430bb51c9b1596207cfeea765c8eb5cf25356f9c1d1be06e03d0e4`.

**2026-08-19: W3b Wayland frame policy verified** (genet
`01a682b3d06`; policy introduced in `41cbfa552c3`, effective-frame trace in
`f07e2f23f9f`). The earlier three-choice model was false precision. Pinned
winit exposes two application-visible providers: the app draws the frame, or
the host does. On Wayland the host choice prefers compositor decoration when
the protocol exists and uses winit's SCTK client frame otherwise. Those are
the same result to the app, so `System` and `Prefer system` cannot be separate
settings without new lower-level winit work and a consumer that needs it.

`WindowFrame::{Host, App}` now names that boundary. The same value creates the
native window and decides whether the smoke view includes its title row, so
the two providers cannot overlap. `Host` is the default. The old
`decorations: false` input still maps to `App` for source compatibility with
Woodshed's moving lane; it is explicit migration debt rather than a second
policy.

The paired Fedora 44 / GNOME 50.4 Wayland run in
`testing/genet/w3b-wayland-frame-01a682b3-2026-08-19/` came from a clean
`01a682b3d06` checkout. Host mode reported
`backend=wayland policy=Host decorated=true`, while its app-authored opening
frame omitted the app title row and measured 420x285. App mode reported
`backend=wayland policy=App decorated=false`; its opening frame contained the
single Cambium title row and measured 420x320. Both scenarios returned
`RESULT ok` with three nonblank frames, three distinct digests and two sizes.
The opening-frame SHA-256 values are
`cc4a13bd5d4bf7da49aee6ce0f36544cb1aa1483b3dfa40ff4098e84a476d67a`
for Host and
`58a512302d20df068a0834dc6eeea473f260dee5a841e134666a2ccde20145ee`
for App. `scripts/wayland-frame-receipt.ps1` reproduces the paired run and
requires the complementary post-configure decoration results.

**Staged, with reasons rather than intentions:**

- **W3c X11 shadow.** `_GTK_FRAME_EXTENTS` is still absent and wants its own
  headed X11 shadow receipt. It is independent of Wayland frame negotiation.

  **The instrument exists; it is not a login session** (checked 2026-08-17 on
  the Fedora ThinkPad). There is no X11 session to log into any more —
  `/usr/share/xsessions/` is empty, because Fedora 44 ships GNOME 50 and GNOME
  removed its X11 session. Installing another desktop to get one is a large
  change for one receipt. It is also unnecessary: XWayland is already running
  rootless under mutter on `:0`, the X11 client libraries are present, and
  mutter advertises the property under test —

  ```
  xprop -root _NET_SUPPORTED | tr ',' '
' | grep _GTK_FRAME_EXTENTS
  ```

  returns it. So the receipt runs the app as an ordinary X11 client
  (`WINIT_UNIX_BACKEND=x11`) against a real reparenting window manager that
  implements the extension, with no session change and nothing installed. Note
  `XAUTHORITY` over ssh: mutter's cookie is
  `/run/user/1000/.mutter-Xwaylandauth.*`, not `~/.Xauthority`.

  **Label it XWayland, not bare X11.** The protocol exchange is genuine, but
  the compositor underneath is still mutter-on-Wayland, so anything specific to
  a different WM (xfwm4, openbox) or to a non-compositing X server is *not*
  covered by it. Claiming otherwise would be the receipt lying about its own
  scope.

  **Not WSL.** WSLg is a Weston-based compositor running XWayland in RAIL mode,
  where each window is composited into the Windows desktop and no ordinary
  reparenting WM negotiates frame extents. A receipt taken there would be
  measuring WSLg's bridge rather than X11 — the exact class of receipt this
  lane exists to prevent.
- **W4 Snap Layouts.** Gate re-checked 2026-08-17 rather than assumed:
  [winit#3884](https://github.com/rust-windowing/winit/issues/3884) is **still
  open**, labelled `DS - win32` / `S - enhancement`, unassigned, with no linked
  PR and no in-API workaround — unchanged since it was opened 2024-08-21. Nor
  does an upgrade help: 0.30.13 (2026-03-02) is the newest stable winit and is
  what genet already pins; 0.31 has not left beta. So the only route remains
  `tauri-plugin-frame`'s native child `HWND` over the custom maximize button,
  and W4 stays worth its cost only for a Windows-first product. Re-check the
  issue before starting, not the brief.
- **Window-state persistence** has its host half (`AppCtx::geometry` plus
  `WindowGeometry::is_reachable_on`, unit-tested against the
  unplugged-monitor and dragged-off-the-bottom cases). The storing half is
  an application's, and woodshed's next session can wire it to muniment in
  a few lines.
