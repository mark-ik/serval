# Cambium desktop host — G1 completion receipt

**Date:** 2026-08-09
**Package:** `components/cambium/cambium-genet-winit-host` (`publish = false`)
**Plan:** retinue `design_docs/2026-08-09_signalman_cambium_desktop_scope.md`, lane G1

The host landed on 2026-08-09 as an extraction from the woodshed-genet donor
(genet `246f0f1e7`). That commit closed the assembly but left four routing gaps
and every receipt. This records closing both.

## What changed since the extraction

### Pointer Down / Move / Up, with captured-element local coordinates

The extraction dispatched only `dispatch_click`, with `local: (0.0, 0.0)` — a
placeholder, not a coordinate. `on_pointer` was therefore unreachable: sliders,
knobs, and drag handles could be built in Cambium and never receive an event.

The press path now resolves the hit node, dispatches the click with the real
hit point in the target's space, then asks `pointer_target` which ancestor
*would* capture and measures against **that** element's box before
`dispatch_pointer_down`. Moves and the release route to `pointer_capture()`,
so a drag keeps working after the cursor leaves the element — which is what
capture means, and what a slider needs.

The coordinates come from a new engine query:

- `genet_layout::genet_lane::painted_origin` — the single-target form of
  `accumulate_painted_origins`, so one element's painted origin costs a walk
  rather than a whole-tree map per pointer move.
- `IncrementalLayout::painted_rect` — `absolute_rect` carried through every
  ancestor scroll container and the document scroll, expressed in the same
  scene coordinates `hit_test` takes its point in. `absolute_rect` could not
  serve: inside a scrolled list it names where the box would be *unscrolled*,
  so a drag there would have read a stale offset. `position: fixed` subtrees
  keep their pinned origin, mirroring the hit walk's own fixed branch.

### Wheel handlers before the layout's scrolling

The extraction sent the wheel straight to `layout.scroll_at_target`, so
`on_wheel` never fired. The wheel now resolves `wheel_target` from the hit
node, dispatches a `WheelEvent` with cursor-local coordinates (so a handler can
anchor a zoom under the pointer), and runs the scrolling default **only** when
no handler called `prevent_default`. A canvas that pans on the wheel no longer
scrolls the page behind it.

### Tab through `dispatch_key`, and `default_prevented` for host defaults

The extraction intercepted Tab and called `focus_traverse` directly, so a view
could never handle it. Cambium's runner has done Tab traversal and Enter/Space
activation as *cancellable defaults* since G2.3; the host was reaching around
that. Tab is now an ordinary routed key.

The order is the browser's throughout: application `key_intercept` first, then
`dispatch_key`, then the host's own defaults gated on `default_prevented()`.

The caret default needed care. Cambium's text field applies a *logical* arrow
move in its own key handler, and the host applies a *visual* one that consults
the laid-out lines (so wrapped and bidi text move the way they look). Routing
first would have double-moved the caret. The host now snapshots the selection
before dispatch and recomputes the visual move from that snapshot, overwriting
the logical result — the layout-aware answer still wins, the field still works
unchanged in a host with no layout, and a handler that prevents the default
suppresses the host's move entirely.

### Typed AccessKit actions

`A11yHost::sync` collapsed `Click` and `Focus` into one `Vec<NodeId>` and the
host clicked every entry. A screen reader's virtual cursor issues `Focus` as it
moves *across* controls, so that arrangement pressed every button a reader
passed over. `sync` now returns `Vec<A11yRequest>` carrying the action, and the
host routes `Click` through `dispatch_click` and `Focus` through `set_focus`.

### Suspension

`ApplicationHandler::suspended` is implemented: it drops the `SurfaceHost` and
nothing else. Window handle, runner state, retained layout, accessibility tree,
and leaf registry all survive, and `resumed` re-boots a surface against the
same window and repaints. `HostOptions::netrender` became a factory closure
because of this — `NetrenderOptions` is not `Clone`, and the second surface
must have the first one's renderer configuration rather than silently falling
back to defaults. The lifecycle claim in the scope document is now true rather
than narrowed.

## Receipts

### Deterministic retained-DOM / input test

`tests/input_routing.rs`, 10 cases, all passing. They run against a real
retained DOM and a real retained layout with no window and no GPU, through the
new `Harness` — the same `Host`, constructed with `window: None`, driven
through the same `click` / `key` / `wheel` / `relayout` methods the winit event
loop calls. A test that passes there exercised production routing rather than a
parallel copy of it.

Covered: click-to-focus and dispatch; drag Down/Move/Up local coordinates
including outside the box; no capture without an `on_pointer` ancestor; wheel
to the handler with local coordinates; wheel `prevent_default` suppressing the
scrolling default (proven against a real overflow container that does scroll
without it); Tab routed then traversing; `prevent_default` on Tab holding
focus; Enter activating a focused button; clicking by role and label; and the
caret defaults through the `focused_text` seam.

Making this possible required splitting the frame: `Host::relayout` is the
half with no GPU and no window, and `redraw` is layout-then-paint over it.

### Accessibility regression

`tests/accessibility.rs`, 5 cases, all passing. `cambium-winit-a11y` grew
`project_tree` — the pure half of `sync`, no window and no adapter — and
`A11yHost::map_request`, the seam between "the OS asked" and "the app does".

Covered: the projection carries role, accessible name, and a box, and **the
announced box is the same box the pointer hits** (asserted against
`painted_rect`, so a projection that drifts from layout fails); a `Click`
request activates; a `Focus` request focuses **without** activating and the
next projection reports that focus; and an unresolvable raw request maps to
nothing rather than to a default node.

### Idle accessibility wake

`about_to_wait`'s decision moved into `Host::idle_policy`, returning
`IdlePolicy::{Wait, Animate, A11yWake}`. `Host::a11y_waker()` builds the exact
closure handed to the AccessKit adapter. The test fires that closure and
asserts the policy goes `Wait` → `A11yWake` → `Wait`: the wake becomes a
repaint, and it is consumed once rather than re-firing every idle turn.

The OS adapter itself is the one link no test can supply, and it is the one
link not asserted. Everything on both sides of it is.

### Bounded headed smoke record

`examples/smoke.rs` + `examples/smoke.scn`. Run:

```bash
HOST_SMOKE_SCENARIO=components/cambium/cambium-genet-winit-host/examples/smoke.scn HOST_SMOKE_RECEIPT=smoke.receipt HOST_SMOKE_WIDTH=420 HOST_SMOKE_HEIGHT=320 cargo run -p cambium-genet-winit-host --example smoke
```

Recorded run (`Code/testing/genet/host_smoke.receipt`, 2026-08-09, Windows):

```text
RESULT ok
host smoke: the first frame installed accessibility and revealed the window
captured opened
captured busy
captured resized
host smoke: done
capture opened  840x640  digest=35068b598c9874f0
capture busy    840x640  digest=b3bc17d940630141
capture resized 1120x800 digest=d24f94053217d801
frames: 3 captured, 0 blank, 3 distinct digests, 2 distinct sizes
```

Captures are in-process readbacks of the frame that was presented (the new
`host::read_frame`, extracted so woodshed stops carrying its own copy), so they
need no compositor, no foreground window, and cannot photograph the wrong
window. The example fails the receipt itself — beyond the scenario's own
assertions — if any frame is blank, if the digests never differ across a state
change, or if the size never changes across the resize. The `resize 560 400`
step drives the window through the host, and the recorded 840x640 → 1120x800
(2× DPI) is the redraw-after-resize evidence.

**DPI change is not claimed.** A scale-factor change cannot be forced
programmatically from the application, so the receipt does not assert one. The
host handles `ScaleFactorChanged` by re-requesting a redraw, and the resize
path proves the same relayout-and-repaint route; a real DPI receipt needs a
display change during a manual pass.

### `genet-probe` semantic interaction receipt

The same run is the semantic receipt: every interaction in `smoke.scn` is a
selector (`click role:button Reset`, `click role:slider Level`), resolved
against the retained DOM, and every resulting pointer event goes back through
the host's own routing via the new `HostPointer` queue on `AppCtx`. Nothing in
the scenario is a coordinate.

`HostPointer` is the general mechanism: an application queues `Moved`, `Press`,
or `Release` in logical coordinates and the host delivers them once the hook
returns, through the same path a real mouse takes. The host owns hit testing,
capture, and dispatch order, so a self-driving application must not re-roll
that routing — and a receipt collected through the production path is worth
having. The `Probe` borrow-struct in `smoke.rs` is the reference for how an
application wires `Automatable`/`Driveable` over `AppCtx`.

## One engine gap found, not fixed here

Genet's UA default makes `button`, `input`, `select`, and `textarea`
`display: inline-block`, and **inline-level boxes share their line's fragment
rather than getting one each**. So an inline `<button>` has no rect of its own:
`painted_rect` returns the line box for the first one and `None` for the
second, `genet-probe` cannot resolve either, and `cambium-winit-a11y` cannot
give a screen reader accurate bounds for them.

The workaround every consumer already uses without knowing it is styling
controls block-level, which is why nobody hit this before. `smoke.scn` only
passes because `smoke.rs`'s sheet sets `display: block` on `.button`, and that
line carries a comment saying so.

This is outside G1 and is filed separately. Applications built on this host —
`signalman-desktop` included — must give any control they intend to reach
semantically a block-level display until it is fixed.

## Two things the headed pass turned up afterwards

### Injected text was dropped entirely

The "live typing not confirmed" note above was resolved by tracing a headed run
rather than guessing. The trace named it exactly:

```text
[cambium-host] key Named(Tab) ... focus=None
[cambium-host]   dispatching Named(Tab)
[cambium-host] key Unidentified(Windows(0x00E7)) ... focus=Some(NodeId(…))
[cambium-host]   dropped: no Cambium key for it
```

`0x00E7` is `VK_PACKET` — what Windows delivers when text is injected through
`SendInput` with `KEYEVENTF_UNICODE`. winit cannot name such a key, so the host
dropped it.

That is **not** only a test-automation artifact. On-screen keyboards, keyboard
remappers, and several assistive input tools all type that way, so a person
using one of them could not enter text into any Cambium application at all —
an accessibility defect, found while collecting an accessibility receipt.

`KeyPress` now carries winit's `text` beside the logical key, and an unnamed key
that reports text is typed as that character. A Ctrl or Super chord is still
dropped, because a shortcut must not become typed text. Two regression tests
cover both halves.

The `CAMBIUM_HOST_KEY_TRACE` diagnostic that found it stays. "Typing does not
work" has three causes that are indistinguishable from outside — the window
never got the event, winit could not name the key, or the tree dropped it — and
one line per press tells them apart. It is env-gated and reads the environment
once.

**Live receipt** (woodshed, Windows, 2026-08-09), before and after:

```text
before: key Unidentified(Windows(0x00E7))                 -> dropped
after:  key Unidentified(Windows(0x00E7)) text=Some("m")  -> Character("m")
```

and in `signalman-desktop`, "4.2" typed into the board-revision field by
keyboard alone, no pointer involved.

### Spatial focus navigation: hold Tab, steer with the arrows

Tab traversal walks document order, which is the wrong shape for anything laid
out in two dimensions. Woodshed's fretboard puts sixty focusable notes between
you and the search field — reaching it by Tab is not something a person would
do. A 5×5 grid takes 24 presses to cross diagonally.

So **holding Tab turns the arrow keys into focus steering**, using the laid-out
geometry: nearest control in that direction, preferring one that overlaps the
current element's band, so "down" means down the column you are looking at.

This belongs to the host and nowhere else: it needs the focusable set, which
only the runner knows, and the geometry, which only the layout knows. No
application has both, and the view layer has no layout at all. `cambium` gained
`GenetAppRunner::focusables()` for the half it owns.

Design notes:

- "Held" means physically down, not held *long enough*. Pressing Tab and
  arrowing immediately works, rather than waiting out the OS repeat delay.
- The Tab press still traverses. A tap is byte-for-byte what it always was, and
  the traversal is simply where the steering starts from.
- Repeats while held are swallowed, so holding Tab does not walk document order
  sixty times underneath the steering.
- An edge holds rather than wrapping: spatial movement is not a ring.
- `HostOptions::spatial_focus` (default on) turns it off for an application
  whose arrow keys are already spoken for.

`tests/spatial_focus.rs`, 5 cases over a 5×5 grid, plus 3 unit tests on the
scoring. **Live receipt** (woodshed, Windows):

```text
[cambium-host]   Tab down: spatial focus armed
[cambium-host]   dispatching Named(Tab)
[cambium-host]   spatial Right moved=true   (×3)
[cambium-host]   spatial Down  moved=true   (×2)
```

## API notes for consumers

Breaking against `246f0f1e7`:

- `HostOptions::netrender` is `Box<dyn Fn() -> NetrenderOptions>`, not
  `Option<NetrenderOptions>`. `..Default::default()` is unaffected.
- `AppCtx::window` is `Option<&Window>`, and `AppCtx::logical_size` is a field.
  An application that requests window chrome must tolerate no window, because
  under `Harness` there is none.
- `AppCtx` gained `pointer: &mut Vec<HostPointer>`.
- `HostHooks::key_intercept` takes `&KeyPress`, not `&winit::event::KeyEvent`.
  `winit::event::KeyEvent` cannot be constructed outside winit, so a host whose
  keyboard path took one could never be driven from a test — and a
  keyboard-order receipt that cannot run in `cargo test` is one nobody collects.
- `cambium_winit_a11y::A11yHost::sync` returns `Vec<A11yRequest>`.
- `Harness::hold_tab` takes a `forward: bool` and no longer releases; `tab`
  is the tap (press *and* release). A test that holds Tab must release it, or
  the arrows keep steering.

New: `Harness`, `inert_hooks`, `KeyPress`, `HostPointer`, `IdlePolicy`,
`Direction`, `HostOptions::spatial_focus`, `Frame`, `read_frame`,
`cambium_winit_a11y::{A11yAction, A11yRequest, project_tree}`,
`IncrementalLayout::painted_rect`, `genet_lane::painted_origin`,
`GenetAppRunner::focusables`.
