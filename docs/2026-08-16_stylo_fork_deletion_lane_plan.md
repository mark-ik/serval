# The stylo fork deletion lane

**Date:** 2026-08-16

**Status:** L0-L6 complete (2026-08-18). L7 is blocked on the named
dogfooding gate.

**Parent:** [Livery fullweb cutover and the servo-* retirement](2026-07-24_livery_fullweb_cutover_and_servo_retirement_plan.md),
whose 2026-08-16 revision rules the goal and authorises F5 feature-gating.

## The ruling this serves

One owned styling-and-layout system serving every platform lane, sized by a
dogfooding gate rather than the W3C surface. Not Stylo replacement. If a
Stylo behaviour is ever needed it comes from upstream servo/stylo, never
from our fork.

## Fork deletion is not a fork-side task

Nothing done to Code/crates/stylo advances this. The fork enters the build
from **crates.io**, as the published genet-stylo rename family pinned in
[workspace.dependencies] (root Cargo.toml, lines 311-326). There is no
[patch] redirect and no path edge; the local checkout is only the source of
future releases. So the checkout can be archived at any time,
independently, and doing so changes nothing about the build.

The dependency dies when its last consumer dies. The lane is therefore the
genet-layout retirement, with fork deletion as its final consequence.

## Corrected maps (measured 2026-08-16 by audit)

Three inherited claims are wrong and should not be planned against.

**"genet-layout has 12 in-repo consumers" is wrong; it is 9** workspace
members plus one out-of-workspace example. Seven are unconditional;
genet-scripted and pelt-desktop are nominally optional but reached by their
own default features. **Zero are gated off in a default build.** The 12
likely came from grepping, which catches six crates that mention
genet-layout only in comments.

**"genet-documents is the first edge to cut" is wrong.** It depends
unconditionally on genet-render, which is the deepest consumer in the repo,
so cutting its direct edge is cosmetic: genet-layout stays in its graph
regardless. The graph is a funnel, not a fan. **genet-render is the actual
first edge**, with roughly 20 entry points spanning cascade, layout, paint
emission, caret and selection, scrollbars and a11y.

**"genet-layout is the sole stylo consumer" is wrong.** It is the largest at
304 references across 27 files, but **11 other in-repo crates** carry a
manifest dependency on the fork. The old audit scoped only genet-era crates
and ignored the inherited servo-* ring.

## Two findings that reorder the work

**There is no engine-selection seam, and the existing one is shaped
backwards.** No feature named stylo, incumbent, or layout exists. The livery
features in genet-documents, genet-scripted and pelt are **additive**:
enabling Livery adds a second engine path while genet-layout stays compiled
in. The in-repo comments confirm this is deliberate. A real gate must be
created, and it must invert that shape into either/or. This is why the gate
is L0 and not a later tidy-up.

**The Livery lane is not as stylo-free as its own test suggests.** The chain
is real, both edges unconditional and non-dev:

    engine-observables-api -> servo-malloc-size-of -> genet-stylo

genet-livery reaches it only through a **dev-dependency** on
genet-scripted-dom, so Livery shipped library code is clean and only its
test builds pull the fork. But engine-observables-api is publish = true and
its description names Hekate, mere-host and Apparatus as consumers, so
**those inherit the fork from a crate that has no business carrying it**.
servo-malloc-size-of is 53 references in a single lib.rs, all of them
MallocSizeOf impls over fork types. It is the highest-leverage cut in the
repo and it is cheap.

## Stages

Each stage lands with a receipt. No time estimates.

**L0 - the exclusive seam.** Create the engine-selection gate and invert the
additive livery features into either/or. Done when a default build with the
incumbent off compiles and cargo tree shows no genet-layout.

**L1 - decontaminate the shared crates.** Cut servo-malloc-size-of out of
engine-observables-api, then cut the fork out of servo-malloc-size-of. Done
when cargo tree -p engine-observables-api -i genet-stylo is empty and the
published crate no longer exports fork types to Hekate, mere-host or
Apparatus. Independent of every other stage; do it first regardless of what
else slips.

**L2 - the free leaves.** servo-paint (dev-dep, test-file only), genet-probe
(one call site), cambium-winit-a11y (four references), and style_tests.

**L3 - the trivial fork edges.** Five crates at one to eight references
each: servo-canvas-traits, servo-paint, servo-paint-api,
servo-embedder-traits (all CSSPixel / AbsoluteColor / PrefersColorScheme /
OpaqueNode), and servo-config (eight set_pref calls in one contiguous
block). Done when their manifests carry no fork dependency.

**L4 - genet-render.** The project. Port its ~20 entry points onto the
Livery lane behind the L0 seam. Two public re-exports make this a breaking
change rather than an internal swap: genet-render re-exports VisualAffinity,
VisualCaret, VisualMovement and VisualSelection; genet-scripted re-exports
Applied and IncrementalLayout. Done when genet-render builds with the
incumbent gated off and the paint receipts hold.

**L5 - the remaining consumers.** genet-documents (one constructor, ~29
session call sites, one ImageLoader impl; its engines.rs is already
livery-gated but document.rs is not), cambium-genet-winit-host (type
plumbing only), genet-scripted, pelt-desktop, and the standalone
genet_web_smoke example. Unblocked by L4.

**L6 - genet-wpt is a policy decision, not a refactor.** Gating it off costs
the incumbent its conformance coverage, which is the oracle. See below.

**L7 - delete.** genet-layout, its stylo_taffy adapter, then the fork
dependency, then the servo-* cone. Each is the previous one losing its last
dependent. The support/patches/taffy vendor is unrelated and survives.

## Execution record

### L0 - complete (2026-08-18)

Engine selection is now exclusive at the default host boundary. Pelt defaults
to `livery`; the frozen Stylo + genet-layout route is named `incumbent` and is
reachable through the compatibility `viewer` feature. The winit/wgpu shell is
an engine-neutral `present` feature, so selecting Livery no longer selects the
incumbent tile surface as a side effect.

`genet-documents` now carries the same either/or seam. Its local, data, file,
and optional network fetcher moved out of the incumbent `document` module, and
its Livery session produces inspection and clip reports without routing through
`genet-render`. The incumbent document and render dependencies are optional and
selected only by `incumbent` or the still-hybrid scripted compatibility lane.

The native-window keyboard vocabulary also moved above layout. The present
shell emits `ViewerScrollKey`; each engine adapter translates that into its own
scroll command. A Livery-only Pelt graph therefore contains neither
`genet-layout` nor fork types.

Receipts:

- `cargo check` (the default Pelt member and default Livery graph): passed.
- `cargo check -p pelt-desktop --no-default-features --features livery,netfetch`:
  passed.
- `cargo tree -p pelt --prefix none`: zero `genet-layout` nodes and zero
  `genet-stylo` / `style` / `style_traits` nodes in the default graph.
- `cargo check -p genet-documents --no-default-features --features livery`:
  passed.

### L1 - complete (2026-08-18)

`engine-observables-api` no longer derives or exposes Servo heap-accounting
traits. `servo-malloc-size-of` no longer depends on `genet-stylo` or
`genet-stylo-dom`, and its 53 fork-type bridge references are gone. Both the
direct observables edge and the indirect edge through `genet-paint-types` are
therefore fork-free.

The audit missed one Rust ownership consequence: those bridge impls were used
by Stylo-owned fields in `servo-fonts-traits`, `servo-fonts`, and two
`servo-canvas-traits` color fields. Because the
accounting trait belongs to `servo-malloc-size-of`, the impls cannot move to an
incumbent adapter crate. The font fields are explicitly
excluded from Servo's memory report instead. L3 moved the canvas payload to a
neutral owner, so those two exclusions then disappeared.

Receipts:

- `cargo tree -p engine-observables-api --prefix none`: zero
  `genet-stylo` nodes.
- `cargo tree -p servo-malloc-size-of --prefix none`: zero `genet-stylo`
  nodes.
- `cargo test -p engine-observables-api -p servo-malloc-size-of`: 5 passed.
- `cargo check -p servo-fonts-traits -p genet-layout -p genet-livery`: passed.
- The wider non-test workspace wall crossed the edited crates and stopped on
  pre-existing `servo-webgpu-traits` drift against wgpu 30. The all-targets
  wall also exposes the already-stale Stylo oracle tests. Neither failure is
  in the L1 dependency cone.

### L2 - complete (2026-08-18)

`genet-probe` now resolves selector geometry with a stateless Livery + Buckram
pass. Its fixtures explicitly size controls instead of inheriting engine UA
padding, so their window-point receipts describe the authored test surface.

`cambium-winit-a11y` now owns only AccessKit adapter lifecycle over a
caller-projected tree. The frozen genet-layout projection moved beside its
remaining consumer in `cambium-genet-winit-host`; this cuts the leaf crate's
layout edge without pretending the incumbent host is already ported.

The `servo-paint` Stylo-to-pixels integration test and its genet-layout-only
development dependencies are deleted. The orphaned `style_tests` workspace
member is also deleted. Both remain recoverable from Git history.

The audit was wrong about one named leaf: `stylo_taffy` has many live calls in
`genet-layout/box_tree.rs`. Its source is retained until genet-layout dies at
L7. L5 later removes the out-of-workspace web smoke's patch entry when that
consumer moves to Livery.

Receipts:

- `cargo tree -p genet-probe --prefix none`: zero `genet-layout` and zero
  Stylo-family nodes.
- `cargo tree -p cambium-winit-a11y --prefix none`: zero `genet-layout` and
  zero Stylo-family nodes.
- `cargo tree -p servo-paint --prefix none`: zero `genet-layout` nodes; its
  direct fork vocabulary remains L3 work.
- `cargo metadata --no-deps --format-version 1`: zero `style_tests` packages.
- `cargo check -p genet-probe -p cambium-winit-a11y -p servo-paint`: passed.
- `cargo check -p cambium-genet-winit-host`: passed.
- `cargo test -p genet-probe`: 17 passed.
- `cargo test -p cambium-genet-winit-host --test accessibility`: 5 passed.

### L3 - complete (2026-08-18)

The five boundary crates no longer name a fork package in their manifests.
`genet-paint-types` now owns the CSS-pixel marker and the lossless absolute
color payload used by canvas messages. Paint and embedder geometry use that
neutral pixel vocabulary; the redundant engine-owned geometry conversions are
gone.

The paint API's unused `Overflow -> ScrollType` convenience impl and the
embedder's unused `Theme -> PrefersColorScheme` and `OpaqueNode ->
UntrustedNodeAddress` impls were fork adapters living in the wrong layer. They
are deleted. `servo-config` still owns and observes its public preferences but
no longer mirrors eight of them into the frozen Stylo global table.

The wider incumbent compile exposed two more L1 accounting fields in
`servo-fonts`; those are now marked outside Servo heap accounting. The next
layer, `servo-layout-api`, exposes the same orphan-rule fallout and remains L5
work rather than evidence against the completed L3 manifests.

Receipts:

- Cargo metadata reports zero direct fork manifest edges for
  `servo-canvas-traits`, `servo-paint`, `servo-paint-api`,
  `servo-embedder-traits`, and `servo-config`.
- `cargo check -p servo-canvas-traits@0.2.0 -p servo-paint-api -p servo-paint
  -p servo-embedder-traits -p servo-config`: passed.
- `cargo test -p servo-config -p servo-canvas-traits@0.2.0
  -p servo-paint-api -p servo-embedder-traits --lib`: 7 passed.
- `cargo check -p genet-layout`: passed.

### L4 - complete (2026-08-18)

`genet-render` now defaults to the owned Livery + Buckram route. Its one-shot
and retained-session entries produce the same neutral paint-list vocabulary,
then cross the existing `paint_list_render` bridge into NetRender. The default
crate graph contains neither `genet-layout` nor a Stylo-family package.

The render driver now owns the host caret vocabulary that it previously
re-exported from `genet-layout`. Livery supplies retained fragment, caret,
selection, element-scroll, and hit-test facts; the driver owns cursor and focus
overlays plus the AccessKit projection. `LiveryPaintList::push_overlay_rect` is
the explicit boundary for those host overlays, so they do not become CSS paint
responsibilities.

The old implementation remains frozen behind the exclusive `incumbent`
feature. This makes the source and API break visible: Livery session entries
take `LiveryDocument`, fallible layout returns `LayoutError`, and the old
`IncrementalLayout` signatures exist only in the oracle build.

Receipts:

- `cargo tree -p genet-render --prefix none`: `genet-livery` and `buckram`
  present; zero `genet-layout` and zero Stylo-family nodes.
- `cargo check -p genet-render`: passed.
- `cargo test -p genet-render`: 6 passed, including concurrent one-shot render,
  hit-test, live mutation, and retained-session overlay translation.
- `cargo check -p genet-render --no-default-features --features incumbent`:
  passed.

### L5 - complete (2026-08-18)

Every owned consumer route now selects Livery + Buckram without compiling
`genet-layout`. The old implementation remains reachable only through named
incumbent features in `genet-render`, `genet-scripted`, `genet-documents`, and
Pelt. `genet-wpt` is the only unconditional in-workspace consumer left, which
is exactly the L6 oracle boundary.

`genet-scripted` now defaults to its existing live Livery CSSOM owner. Its
runtime-owned DOM no longer needs the incumbent `render` feature merely to
produce a frame. Keyboard scrolling uses a host-neutral enum, and the live
Livery bridge now exposes retained link, text-target, selection, and DOM-read
facts for document-session consumers.

`genet-documents` makes that owned scripted document its ordinary `scripted`
lane. Selection clipping continues to emit a typed DOM-range receipt. Pelt's
`scripted` and compatibility `livery-scripted` profiles now build on the
engine-neutral present shell rather than selecting `viewer` and its incumbent
tile surface.

The standalone wasm smoke now drives Livery cascade, Buckram layout, retained
text, and Livery paint emission directly over Cambium's `ScriptedDom`. Its
bundled Roboto bytes enter through `TextSystem`; the authored sheet requests
that supplied family explicitly. The obsolete `genet-layout` dependency and
`stylo_taffy` patch entry are gone.

The Cambium host was not "type plumbing only" as the audit claimed. It owned
custom-leaf paint splicing, nested and viewport scroll, caret and selection
geometry, interaction restyles, spatial queries, and AccessKit projection. A
host-local Livery session now owns those responsibilities over Cambium's
external `ScriptedDom`. Sprigging commands cross an explicit host-command seam
on `LiveryPaintList`; retained renderer fragments keep their existing marker
path. The host's entire normal dependency graph is now fork-free.

Receipts:

- `cargo test -p genet-scripted --no-default-features --features livery`: 19
  passed.
- `cargo tree -p genet-scripted --prefix none`: Livery and Buckram present;
  zero `genet-layout` and zero Stylo-family nodes.
- `cargo check -p genet-documents --no-default-features --features scripted`:
  passed; its tree contains Livery and Buckram and no incumbent nodes.
- The focused `genet-documents` selection-and-clip test compiled through the
  test crate but MSVC could not link its combined Boa + wgpu image:
  `LNK1318: Unexpected PDB error; LIMIT (12)`. This is not recorded as a test
  pass.
- `cargo check -p pelt --no-default-features --features scripted,netfetch` and
  both Pelt desktop scripted feature spellings: passed. Their trees contain no
  `genet-layout` or Stylo-family nodes.
- Both frozen Pelt incumbent checks passed after explicit
  `genet-render/incumbent` forwarding.
- `cargo test -p cambium-genet-winit-host`: 42 passed across library,
  accessibility, decorations, input routing, lifecycle, and spatial focus.
  The tightened nearest-overflow wheel default was rerun separately and passed.
- `cargo tree -p cambium-genet-winit-host --prefix none`: Livery and Buckram
  present; zero `genet-layout` and zero Stylo-family nodes.
- `cargo build --manifest-path examples/genet_web_smoke/Cargo.toml --target
  wasm32-unknown-unknown`: passed. Its standalone tree contains Livery and
  Buckram and no `genet-layout`, `genet-stylo`, or `stylo_taffy`.
- The rebuilt wasm-bindgen bundle ran in the in-app Chromium WebGPU host. The
  page title reached `SMOKE PASS`, the console logged `genet web smoke: PASS`,
  and visual inspection showed the full text, navigation, sidebar, board, and
  note dots.

### L6 - complete (2026-08-18)

The oracle stays live until the dogfooding gate and receives no further
rebases. `genet-wpt` is now the sole unconditional in-workspace
`genet-layout` consumer. Its differential deliberately compiles both the
frozen Stylo + genet-layout route and the Livery + Buckram route. The remaining
incumbent features in `genet-render`, `genet-scripted`, `genet-documents`, and
Pelt are named compatibility paths rather than default ownership.

No refactor was required here. Gating the oracle before the gate would spend
the evidence while it is still useful; updating its Stylo side would move the
baseline. The L6 action is the ruled policy boundary and a live compile
receipt.

Receipts:

- `cargo tree -p genet-wpt --prefix none`: both Livery + Buckram and the frozen
  Stylo + genet-layout oracle are present.
- `cargo check -p genet-wpt`: passed.

## The oracle conflict, and its resolution

Deleting the fork destroys the Livery/Stylo differential, which is our only
instrument for detecting Livery regressions against a mature engine. That
instrument earns its keep through L4 and L5 and becomes dead weight after.

**Ruling: the oracle expires with the dogfooding gate.** genet-wpt keeps its
incumbent path until that gate fires; past it we are measuring against an
engine we have already declined to be. Do not bump the fork to keep the
oracle current: an oracle wants stability, not currency, or the measured
delta moves for reasons unrelated to Livery.

## Open, and needed before L7

**The dogfooding gate is undefined.** It is a named set of real pages and
flows that must work, chosen because we use them. Every stage above can
proceed without it; deletion cannot, because nothing else says when we are
allowed to stop. This one is Mark to name.

## Adjacent tool boundary - resolved (2026-08-18)

`components/livery/tools/import-stylo-db` already accepted an explicit Stylo
property-directory path, but its generated provenance was hard-coded to
`mark-ik/stylo` and `genet-rename`. It now defaults to an upstream
`servo/stylo` source label and accepts `--source-label` for an exact receipt.
It can therefore regenerate `properties.toml` and `PROPERTY_SPACE.md` from a
fresh upstream checkout after the frozen oracle is archived. Existing
generated files retain their historical fork provenance until the next real
regeneration.

Receipt: `cargo check --manifest-path
components/livery/tools/import-stylo-db/Cargo.toml`: passed.
