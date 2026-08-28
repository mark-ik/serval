# Pelt host reconstruction execution plan

**Date:** 2026-08-22

**Status:** active. P0 completed 2026-08-22. P1 was rebased and reverified on
current `main` 2026-08-24. P2 and P3 completed 2026-08-24. P4 completed on
Windows 2026-08-25 with a registered Scrying producer, native shared-handle
import, repeated fence waits, and visible same-window composition. P5 completed
2026-08-27 with all eight deterministic product receipts. P6's focused-tile
Chrome, structural-inspection, and shell accessibility slices completed
2026-08-27; the narrow-width and high-DPI Chrome receipts completed on Windows
2026-08-27. Pelt's caller-injected durable Chrome appearance store completed
2026-08-28. Reader-in-workspace completed 2026-08-27 with a separate
held-source Fleece/Pelt receipt. IOSurface and DMA-BUF imports remain separate
platform lanes.

## Objective

Restore Pelt as Genet's small reference browser host: embeddable, recursively
tiled, capable of nesting different content surfaces, and able to escalate
content through increasingly expensive engine tiers.

Pelt punches above its weight by composing the stack honestly. Common pages use
the owned Livery/Buckram route. Protocol-native documents remain native. Content
outside the owned engine's present capability can use an explicit surface
engine. All of those routes share one host, one workspace, and one compositor.

Pelt is a forcing consumer for host contracts. It is not another CSS engine,
layout engine, application framework, or product browser.

## Authority map

| Concern | Authority |
|---|---|
| CSS cascade and computed style | Livery |
| Box generation, layout, fragments, hit geometry | Buckram |
| Document and surface engine contracts, routing ids | Inker |
| Recursive split and tab state | `genet-host-api::TileTree` |
| Split and tab view, content-hole geometry | Cambium Frisket |
| Navigation, history, focus, engine selection, composition | Pelt host core |
| winit and platform presentation | `pelt-desktop`, as one adapter |

`EngineProfile` is only a coarse standalone diagnostic override. Product routing
uses Inker engine ids per content item. Windowing mode is separate from engine
selection.

## Live starting point

- Pelt defaults to `EngineProfile::Livery`.
- Scripted Pelt uses Livery/Buckram through `genet-scripted`.
- `genet-document-resources` resolves the retained document resource graph.
- `DocumentSession<Scene>` already exposes frames, nested scrolling, pointer
  selection, links, navigation outcomes, inspection, clipping, and visibility.
- `TileTree::apply` is explicitly the standalone Pelt reference reducer.
- Frisket renders recursive split/tab chrome and marks each active content hole
  with `data-tile`.
- Inker surface contracts already distinguish composited textures, native
  overlays, embedded hosts, and headless surfaces.

`pelt-core` now exposes both a one-session controller and a recursive workspace
of retained document controllers and surface producers. Standalone Pelt
consumes `TileTree` and Frisket, routes complete host effects, selects a visible
engine route per tile through shared long-lived registries, and composites
active document scenes through one desktop surface host. Surface producers are
retained, sized, positioned, polled, and sent input. The desktop adapter does
import D3D12 shared textures into its existing host-owned wgpu device. IOSurface
and DMA-BUF importers remain open for their platform receipts.

## Content tiers

| Tier | Route | Intended use |
|---|---|---|
| T0 | Engine-native document, including smolweb | Small protocol-specific content |
| T1 | `genet.livery` | Script-free HTML through Livery/Buckram |
| T2 | `genet.scripted` | Live HTML with Boa, optionally Nova |
| T3 | Inker surface engine | Content requiring an external embedded engine |
| T4 | Headless/inspect route | Capture, automation, structural inspection |

Routing is per tile. A global `--engine` switch remains useful for diagnostics
and compatibility, but it is not the workspace's routing model.

## Gates

### P0: make the tree tell the truth

**Status:** complete 2026-08-22.

Work:

- Remove unreachable `incumbent` adapters and tests from `genet-documents`.
- Remove `Browser`, `Viewer`, `Static`, `LiveryScripted`, and `Headless` as
  distinct `EngineProfile` states.
- Canonicalize `viewer` and `static` input spellings to `Livery`, and
  `livery-scripted` to `Scripted`.
- Keep the shipped Cargo feature spellings as compatibility aliases.
- Remove the retired `pelt-desktop/viewer` feature gate and make the `smolweb`
  route compile through the current presentation feature.
- Update active README and code comments to name Livery/Buckram rather than the
  deleted Stylo/genet-layout route.
- Mark the 2026-08-07 presentability plan historical and point it here.

Done when:

- `rg` finds zero active `incumbent`, Stylo, or genet-layout claims under Pelt,
  `genet-documents`, `genet-host-api`, and active Cambium source.
- Default Pelt and Pelt with `scripted`, `livery-scripted`, and `smolweb`
  features compile.
- Host API tests prove legacy command spellings canonicalize to the two owned
  HTML profiles and retired profiles are rejected.

Receipts:

- The scoped active-source scan finds none of the retired `incumbent`, Stylo,
  or genet-layout claims.
- `cargo test -p genet-host-api --offline`: 18 passed.
- `cargo check -p pelt --offline` and the individual `scripted`,
  `livery-scripted`, and `smolweb` feature checks pass.
- The combined Livery/scripted/smolweb `genet-documents` matrix builds and runs
  27 tests: 26 pass. The remaining
  `scripted_session_selects_and_clips_the_live_dom` selection-paint assertion
  fails against the concurrent Livery/Buckram working tree and is recorded as
  upstream live-stack work rather than P0 profile-cleanup work.

### P1: restore the browser loop

**Status:** complete 2026-08-24 after integration with the Fleece Reader lane.

Replace the private boolean input results with host effects. The host must
consume `SessionClick::Navigate` and `SessionClick::Submit`, resolve relative
URLs, spawn replacement sessions, and update per-content navigation state.

Add a host-neutral input vocabulary for:

- pointer position, button, capture, and cursor shape;
- keyboard keys, text insertion, composition/IME, and modifiers;
- focus movement, editable focus, form submission, and cancellation;
- reload, stop, back, forward, and address navigation.

Done when one headed Livery document can follow links, edit and submit a local
form, reload, and traverse back/forward without the winit adapter inspecting a
concrete document type.

Landed:

- Inker now carries host-neutral pointer, keyboard, text, IME, focus, cursor,
  form-submission, navigation, and session-effect values.
- Script-free Livery sessions use the existing mutable DOM to retain editable
  input and textarea values, caret and composition state, focus traversal, and
  GET/POST form facts. Each edit travels through retained Livery restyle,
  Buckram layout, and paint.
- `pelt-desktop` owns a retained `BrowserSession` with relative navigation,
  replacement-session safety, reload, back/forward history, address
  navigation, and deterministic GET query encoding.
- The winit adapter translates platform events to Inker input and consumes
  typed host effects. It does not inspect or downcast a concrete document
  session.
- `ports/pelt/examples/browser-loop` is the deterministic local dogfood page
  for links, editing, focus, submission, reload, and history.

Receipts:

- `cargo test -p inker --offline`: 97 passed.
- `cargo test -p genet-documents --no-default-features --features livery
  --offline`: 15 passed, including retained link activation, visible editing,
  and structured GET submission.
- `cargo test -p pelt-desktop --no-default-features --features livery
  --offline`: 6 passed, covering relative links, reload, back/forward, forward
  truncation, GET encoding, and Livery routing.
- The Fleece route remains independent: Reader-only `genet-documents` passes
  7 tests and Reader-only `pelt-desktop` passes 2. The Reader adapter retains
  held source bytes and extraction posture while Livery uses `BrowserSession`.
- Default `pelt-desktop` passes 6 tests with Livery, Reader, and netfetch joined.
- Strict all-targets Clippy passes for Inker, `genet-documents`, and
  `pelt-desktop` with dependency linting excluded. The Fleece static outcome's
  article payload is boxed so the public three-way result stays pointer-sized.
- Bounded headed Livery and Reader commands each created an 800x600 window,
  presented one frame from `ports/pelt/examples/browser-loop/index.html`, and
  exited with `redraws=1`.
- Headed Pelt on the checked-in local fixture changed title from `Pelt browser
  loop` to `Pelt relative navigation`, returned and advanced through history,
  reloaded the retained route, visibly edited the textarea to `cedarx`, moved
  focus with Tab, and submitted to `Pelt local form result`.

POST submissions are exposed as typed form facts but remain an explicit
unsupported transport result in this synchronous local controller. Stop is in
the host vocabulary and P2 consumes it synchronously; cancellable host
transport remains a separate open seam.

### P2: extract the embeddable host core

**Status:** complete 2026-08-24.

Create a public Pelt controller that owns registries, sessions, navigation, host
effects, and frame production without creating an event loop or window. Inject
the host's resource policy, engine registries, target size, clock, and settings.

Implementation ruling: resource and settings policy enter through the
caller-constructed session engines and initial `SessionSpawnRequest`. The
controller is generic over the engine frame, so a raw wgpu device or queue is
not part of its contract. The embedding host owns those resources and the
presentation target. This keeps the reusable core independent of winit, wgpu,
Netrender, and any concrete paint backend.

`pelt-desktop` becomes a thin consumer that translates winit events and presents
the returned frame. A second focused test host drives the same controller with a
caller-owned target.

Done when standalone Pelt and the test embedder use the same controller, and the
controller creates neither a winit event loop nor a wgpu device.

Landed:

- `pelt-core` owns the document and surface registries, retained session,
  complete spawn-request history, navigation, host effects, target size, clock,
  pumping, and generic frame production. Pelt and `pelt-desktop` re-export its
  public contract.
- The initial held body and content type survive reload and history traversal.
  Fresh navigation starts with a fresh address-only request, so a prior response
  cannot leak into a new route.
- Host-owned address resolution moved to `genet-host-api`; `genet-documents`
  keeps a compatibility re-export.
- Default Livery and Reader viewers use the controller. The desktop layer now
  translates winit events, pointer capture, redraw, and presentation only.
- A recording embedder drives the same controller with `String` frames and a
  caller-owned target, without window or GPU dependencies.

Receipts:

- `cargo test -p pelt-core --offline`: 1 focused embedder test passed, covering
  clock pumping, frame production, relative navigation, reload, back/forward,
  GET encoding, resize, surface-registry ownership, and held-body history.
- `cargo test -p genet-host-api --offline`: 20 passed. The moved local, remote,
  root-relative, scheme-relative, absolute, and Windows-path resolver remains
  covered.
- Livery+Reader `genet-documents`: 16 passed. Reader-only `pelt-desktop`: 2
  passed. Default `pelt-desktop`: 3 passed. Default Pelt builds and tests.
- Strict all-targets Clippy passes for `pelt-core`, `genet-host-api`, and
  `pelt-desktop`, with dependency linting excluded.
- `cargo tree -p pelt-core --offline` contains neither winit, wgpu, Netrender,
  `genet-winit-host`, nor `genet-render-host`.
- Bounded standalone Pelt created an 800x600 native window, presented one
  Livery frame from `ports/pelt/examples/browser-loop/index.html`, and exited
  with `redraws=1`.

Deferred by gate rather than lost: recursive multi-session state remains P3;
scripted, smolweb, Reader link refetch/rerouting, and surface composition remain
P4. POST transport and cancellable Stop remain explicit transport seams.

### P3: restore recursive tiling and nesting

**Status:** complete 2026-08-24.

Build the Pelt workspace from `TileTree` and Frisket. Keep one navigation entry
and one live content handle per tile. Use Frisket's `data-tile` content holes to
place each active scene or surface. Recursive split nodes provide nesting;
recursive Pelt instances are not required.

Route pointer, wheel, keyboard, focus, resize, and visibility through the active
tile. Inactive tabs call `set_hidden(true)` and retain their session state.

Done when a headed receipt can:

- open two independent documents;
- split horizontally and vertically;
- tab, activate, resize, drag, and close tiles;
- navigate each tile independently;
- preserve scroll and history across tab activation.

Landed:

- `pelt-core::PeltWorkspace` owns the authoritative `TileTree`, one retained
  `PeltController` per document tile, focused input and pointer capture, active
  content-hole geometry, visibility, independent navigation, and generic
  per-tile frame production. Non-document holes remain present for P4.
- Inactive tabs receive `set_hidden(true)` while their controller, current
  session, scroll, and navigation history remain live. Activation, divider
  resize, drag, and close all apply through `TileTree::apply`; identity follows
  `TileId`, not tree position.
- Cambium Frisket now exposes close and content semantic targets alongside its
  existing tab, divider, stack, and drop helpers. Its structural CSS uses
  explicit flex longhands, which the owned Livery/Buckram lane sizes without a
  shorthand compatibility dependency.
- `pelt-desktop` renders the actual Frisket DOM through Livery/Buckram and reads
  `data-tile` fragment rectangles from that retained layout. One `SurfaceHost`
  rasterizes the pane frame and every active document scene, then composites
  them into one target. The adapter routes pointer, wheel, keyboard, IME,
  focus, resize, cursor, divider drag, tab drag/drop, activation, and close.
- `pelt --tiles <url>...` opens the recursive workspace. The checked-in
  `ports/pelt/examples/workspace` fixture supplies four tall, independently
  navigable documents. `--tile-receipt` drives the bounded interaction sequence
  through live Frisket coordinates.

Receipts:

- `cargo test -p pelt-core --offline`: 2 integration receipts passed. The
  recursive receipt covers a tab stack beside a nested row/column split,
  Frisket-sized frames, local pointer translation, independent navigation and
  history, hidden visibility, scroll retention, divider resize, edge drag, and
  close without respawning retained controllers.
- Focused Cambium Frisket tests pass 8 semantic/view receipts, including close
  and active content-hole target resolution.
- `cargo test -p pelt-desktop --no-default-features --features livery
  --offline`: 7 passed, including recursive Livery content-hole geometry,
  semantic hits, the four-URL tree shape, and edge-drop geometry.
- Default Pelt compiles and its package tests pass with Livery, Reader, and
  netfetch joined.
- Strict all-target Clippy passes for `pelt-core`, the Livery-only
  `pelt-desktop` slice, and default Pelt with dependency linting excluded.
  Cambium's focused Frisket tests are the component gate; full-crate Cambium
  Clippy still reports pre-existing warnings outside this lane.
- The bounded native command below created one 1000x700 window, presented nine
  frames through the shared compositor, drove independent navigation, tab
  hide/reactivation, root-divider resize, tab-to-edge split, and close, then
  exited with `tiles=3 interaction_receipt=true`:

  `cargo run -p pelt --offline -- --tiles --tile-receipt --size 1000x700
  ports/pelt/examples/workspace/a/index.html
  ports/pelt/examples/workspace/b/index.html
  ports/pelt/examples/workspace/c/index.html
  ports/pelt/examples/workspace/d/index.html`

Deferred by gate: P3 constructs Livery document controllers. Per-tile engine
selection, shared long-lived registries, smolweb/scripted/Reader routing, and
external surface composition remain P4.

### P4: route by capability and compose surface engines

**Status:** complete on Windows 2026-08-25. Capability routing, registered
external production, host-owned D3D12 import, fence synchronization, cached
epoch reuse, and visible same-window composition all have receipts. IOSurface
and DMA-BUF follow as independent platform lanes.

Install long-lived document and surface registries in the host core. Select a
route from scheme, content type, user settings, and declared capability. Keep
the selection visible and user-overridable per tile.

Compose `CompositedTexture` surfaces into the shared wgpu frame. Treat
`NativeOverlay` and `EmbeddedHost` as explicit platform contracts. Avoid
per-tile devices, readback, and hidden child windows.

Done when one window simultaneously hosts a smolweb document, static Livery
HTML, scripted Livery HTML, and an external surface fallback.

Landed:

- `PeltRegistries` owns one shared document registry, surface registry, route
  policy, surface profile, and document fallback. Every routed controller uses
  that same pair rather than reconstructing registries per tile.
- Each tile retains its selected route, decision source, active engine, and
  explicit fallback reason. User overrides survive unavailable engines and are
  visible in the title. A failed live replacement restores the prior request,
  controller or surface, and route atomically. Successful replacement keeps
  the tile's current navigated address.
- Registered `CompositedTexture` engines become retained surface producers.
  Pelt forwards physical-pixel size, offset, focus, pointer, wheel, keyboard,
  and web navigation; visible producers keep the redraw loop polling for late
  frames. `NativeOverlay` and `EmbeddedHost` remain explicit unattached
  contracts and visibly use the document fallback.
- The desktop registry constructs one host fetcher and registers Livery,
  Reader, Boa scripted, optional Nova scripted, and smolweb-native session
  engines over it. Global `--engine` selection now applies to every workspace
  tile, while repeatable `--tile-engine N=engine-id` choices win per tile.
- `--capability-receipt` uses checked-in Gemtext, static HTML, scripted HTML,
  and fallback fixtures. It verifies the Gemtext link, static title and
  heading, Boa's retained DOM mutation, shared registry identities, and the
  selected external fallback route before accepting the headed run.
- The Windows importer opens D3D12 shared textures on the existing host device,
  validates their dimensions, format, two-dimensional mip/sample shape, and
  simultaneous-access flag, and wraps them as wgpu textures for Netrender's
  external-texture composition path.
- Imported resources are retained by tile and resource epoch. The first Scrying
  frame transfers its one-shot texture handle; later zero-handle frames reuse
  the cached resource. Each surface retains its opened fence COM reference,
  stages the declared queue wait before sampling, and returns the resource to
  COMMON before the next producer capture.
- Windows registers a lazy `scrying.web` producer over the real Scrying/WebView2
  implementation. It receives Pelt's existing HWND and host-created shared
  fence after winit resumes while preserving the full Inker `SurfaceProducer`
  and `WebSurface` control plane.
- The animated native fixture changes every 80 ms. Its high-contrast
  magenta/cyan content makes stale or missing composition immediately visible
  inside Frisket's fourth content hole.

Receipts:

- `cargo test -p pelt-core --offline`: 3 integration tests passed after
  rebasing on current `origin/main`. The routing receipt covers shared
  registries, document and surface routes, unavailable and unattached
  fallbacks, high-DPI surface geometry, late-frame polling, current-address
  rerouting, override precedence, and failed-spawn rollback.
- `cargo check -p pelt --features scripted,smolweb --offline` passes. Strict
  all-target Clippy passes for `pelt-core` and for Pelt with that feature pair,
  with dependency linting excluded. Nematic and `genet-scripted` still emit
  their pre-existing unused-code warnings outside this lane.
- The bounded native `--capability-receipt --size 1000x700` command created one
  window, presented two frames, and exited with `tiles=4
  capability_receipt=true` and routes `nematic.gemtext:document`,
  `genet.livery:document`, `genet.scripted:document`, and
  `scrying.web:fallback:genet.livery`.
- The repaired bounded command presented 600 host frames and exited with
  `capability_receipt=true`, route four as
  `scrying.web:surface:CompositedTexture`, and native counters `frames=4
  imports=1 waits=4 compositions=600`. More producer frames than imports proves
  same-epoch cache reuse; repeated waits and compositions prove the live
  synchronization and compositor paths. A headed screenshot showed the
  magenta/cyan WebView2 fixture beside the Gemtext, static Livery, and scripted
  Livery tiles in the same window.
- The P3 headed regression still presents nine frames, drives the full split,
  tab, navigation, resize, drag, and close sequence, and exits with `tiles=3
  interaction_receipt=true`.
- `cargo test -p scrying-engine
  dx12_epoch_handle_is_transferred_and_reuse_may_omit_it --offline` passes its
  focused transferred-handle and zero-handle reuse test.
- `cargo test -p pelt-desktop --no-default-features --features livery
  dx12_surface::tests --offline` passes its exact format and cached-epoch handle
  rules. Strict all-target Clippy passes for `scrying-engine`, `pelt-core`,
  `pelt-desktop` with Livery, and full Pelt with scripted and smolweb. The
  `present`-only dependency canary does not pull Scrying.

Deferred platform and hardening lanes:

- Scrying currently creates its D3D11 capture device on the default hardware
  adapter. A multi-GPU host may select a different D3D12 adapter, in which case
  shared texture or fence opening fails visibly. Adapter-LUID selection and a
  multi-GPU receipt remain Windows hardening work.
- IOSurface and DMA-BUF require their own import, ownership, synchronization,
  and device-compatibility receipts.
- Native overlay and embedded-host attachment remain separate platform seams
  rather than aliases for texture composition.

### P5: make Pelt the product receipt for Livery/Buckram

**Status:** complete 2026-08-27. All eight deterministic fixtures have named
commands, bounded captures, semantic assertions, and GPU-free checks.

The ordinary-article receipt is:

```sh
cargo run -p pelt --no-default-features --features livery -j 1 -- \
  --product-receipt article --artifact target/pelt-receipts/article.png
```

The named receipt owns `examples/livery-route`, a 960x640 viewport, and three
presented frames unless the caller explicitly overrides size or frame count. It
drives the retained jump link through press, pointer capture, release, and
fragment scroll before accepting the artifact. Capture composes into an owned
RGBA8 target on Pelt's host device, reads that target to PNG, then presents the
same target. The 2026-08-25 Windows receipt recorded all three frames, the
interaction assertion, a nonblank 960x640 PNG, and digest
`973595d7fbd90151`. The checked-in GPU-free assertion drives the same receipt
through `PeltController`.

The nested-scroll and controls receipt is:

```sh
cargo run -p pelt --no-default-features --features livery -j 1 -- \
  --product-receipt controls --artifact target/pelt-receipts/controls.png
```

The named receipt owns `examples/p5-controls`, the same 960x640 physical
viewport, and three presented frames. Its fixture fits the resulting 480x320
logical viewport on a 2x display. The semantic driver resolves retained text
inside the overflow panel, routes a wheel delta at that point, and proves
`Home` cannot move the untouched document viewport. It then uses ordinary Tab
focus and text input to change the first editable control from `cedar` to
`cedar and ash`, accepting the capture only when the structural report carries
that edited textbox value. The 2026-08-26 Windows receipt recorded all three
frames, the interaction assertion, a nonblank 960x640 PNG, and digest
`ceb4d98e10c231b8`. The checked-in GPU-free test drives the same receipt through
`PeltController`.

The responsive grid and table receipt is:

```sh
cargo run -p pelt --no-default-features --features livery -j 1 -- \
  --product-receipt responsive \
  --artifact target/pelt-receipts/responsive.png
```

The receipt owns `examples/p5-responsive`, the same 960x640 physical viewport,
and three presented frames. At a pinned 480x320 logical probe, its semantic
driver proves two retained grid labels share one row and that both table body
rows preserve two ordered columns. It then reframes the retained document at
320x320, proves the cards stack, both table axes remain intact, and the table's
column separation contracts with the viewport. These pinned CSS-pixel probes
keep the assertion independent of host DPI; the headed host restores its live
viewport before capture. The 2026-08-26 Windows run presented all three 960x640
frames, recorded the viewport-reflow assertion, produced a nonblank PNG, and
recorded digest `a5d1a743599c5493`. The checked-in GPU-free driver passed in the
full 13/13 `pelt-desktop` Livery test wall.

The scripted mutation, timer, and navigation receipt is:

```sh
cargo run --locked --offline -p pelt --no-default-features \
  --features scripted -j 1 -- \
  --engine scripted --product-receipt scripted \
  --artifact target/pelt-receipts/scripted.png
```

The receipt owns `examples/p5-scripted/index.html` and its replacement
`next.html`, the same 960x640 physical viewport, and three presented frames.
The first document mutates one visible status during parser execution and a
second from a zero-delay timer. Its ordinary external anchor returns a typed
navigation default only after script listeners have had the opportunity to
cancel it. The semantic driver presses and releases through retained text
geometry, lets `PeltController` resolve and replace the document, proves the
replacement title, heading, and parser mutation, then traverses
controller-owned Back. The restored document is a new scripted session, so the
driver also proves that its timer mutation runs again before accepting the
capture.

The cancellation listener is delegated from `document`, so it remains rooted
across the frame-cadence GC turn. Element wrappers are still weakly cached while
their native nodes remain attached; element-local listener and expando lifetime
across that collection boundary is open scripted-runtime hardening. This receipt
does not claim that broader cross-heap identity guarantee.

The 2026-08-26 locked Windows receipt presented all three frames and recorded
`parser and timer mutated the live DOM; cancelled default stayed, release
navigated, and Back replayed the timer`, with digest `9fdce1c66a85e37a` and
SHA-256
`7d7f6bb1a11fc801823dd5a93aaec5f6f1540747fa83d159516ccd4f52449179`.
The GPU-free driver compiled inside the `pelt-desktop` scripted wall; executing
that wall is blocked on this Windows toolchain by `LNK1140` while writing its
oversized test PDB. The same driver passed in the bounded headed binary, and the
full 18/18 `genet-documents` scripted wall passes. That wall recovered the
P0-deferred scripted selection test by painting selection overlays on cached
frames and deriving clipped links from selected text-node ancestry instead of
overlapping element rectangles.

The redirected resource-graph receipt is:

```sh
cargo run --locked --offline -p pelt --no-default-features --features livery -j 1 -- \
  --product-receipt resources \
  --artifact target/pelt-receipts/resources.png
```

The receipt owns `examples/p5-resources/start/index.html` and its redirected
`final/index.html` response. The receipt fetcher returns the final identity and
delegates every other request to `LocalFetcher`, so the run is deterministic and
does not need a network server. The final document links `styles/root.css`,
whose leading `@import` loads `styles/palette.css`; the linked sheet also names
the reused `Ahem.ttf` font and `servo_64.png` image through relative URLs.

The GPU-free assertion inspects the retained resource ledger for the redirected
document identity, imported-before-parent order, import parent/child IDs,
source/request URL identities, sheet-relative image and font entries, and an
empty diagnostic set. The headed driver remains host-neutral: its source clip
proves the final response identity, the scene proves the image, named Ahem font
bytes, and imported accent color, and a shared fetch trace proves the complete
request sequence.

The 2026-08-27 locked, offline Windows receipt presented three 960x640 frames
after each flattened font source was resolved against its final stylesheet
identity. It recorded `redirected identity, imported cascade, linked image/font,
and fetch trace held`, with digest `b2736b2a574240f7`. Two successive captures
were byte-identical at 88,850 bytes with SHA-256
`77b75a5b7da67b354c63513f6f339b317a3690b4eddc13a8b57feb4a702b2f65`.
The focused retained-resource assertion passes, followed by the full 15/15
`pelt-desktop` Livery test wall and its doc-test wall.

The protocol-native Gemtext receipt is:

```sh
cargo run --locked --offline -p pelt --no-default-features --features smolweb -j 1 -- \
  --product-receipt gemtext \
  --artifact target/pelt-receipts/gemtext.png
```

The receipt owns `examples/p5-gemtext/index.gmi` and its linked `next.gmi`, the
same 960x640 physical viewport, and three presented frames. Pelt supplies the
initial `text/gemini` body through `SessionSpawnRequest`; the registered
`SmolwebSessionEngine` lowers it through Nematic without consulting transport.
A receipt-local fetcher serves only the two fixed Gemtext URLs so the retained
native link can open the second document without network access.

The GPU-free driver proves the controller's engine ID is
`nematic.gemtext`, the held body and content type survive the initial spawn,
the structural report and retained hit table expose the resolved native link,
and both documents paint nonempty Gemtext glyph runs. Smolweb currently uses
Inker's compatibility click-on-press floor: the primary press captures and
navigates exactly once, pointer-up releases capture, and `PeltController`
replaces the initial request with a fresh bodyless request for the fixed
destination. The destination title, return link, and painted scene must all
pass before capture is accepted.

The 2026-08-27 locked, offline Windows receipt presented three 960x640 frames
and captured the navigated document with its retained return link. It recorded
`held Gemtext body lowered through Nematic; retained native link navigated
through PeltController`, digest `5c8cf8e4e09843d3`, and a 69,169-byte PNG with
SHA-256
`8465cb47b4667bb5ca49d283c1bd53804ad98f8727be79aedefa86bf395615c8`.
The focused routed controller test passes 1/1, followed by the full 17/17
`pelt-desktop` Smolweb test wall.

The mixed tiled-workspace receipt is:

```sh
cargo run --locked --offline -p pelt --no-default-features \
  --features livery,scripted,smolweb -j 1 -- \
  --workspace-receipt mixed \
  --workspace-size-matrix 960x640,1024x768,1280x800,1440x900 \
  --artifact target/pelt-receipts/workspace-mixed.png
```

The receipt reuses the four P4 fixtures and starts at 960x640. The optional
physical-size matrix resizes the same live window through XGA, WXGA, and
1440x900. Each stage accepts the actual `WindowEvent::Resized`, rebuilds
Frisket content holes, and requires tile 4 to publish a newly imported texture
whose physical extent matches its current content hole. Fenced waits and
compositions must also advance before the next resize. The final 1440x900
state owns the PNG and three postcondition frames.

Readiness uses a configurable monotonic deadline, 20 seconds by default, for
each stage. The acquire and present cycle runs before expiry is checked, so
Scrying's own stalled-capture restart can publish a recovery frame. This
replaces the coupled 600-redraw host bound, which could exit on the same poll
that restarted capture. After the matrix is verified, the receipt clicks the
retained Gemtext `static.html` link through tile 1's real content hole.
Smolweb navigates on primary press and releases capture on pointer-up.

The driver then removes only tile 1's receipt pin and lets shared routing select
Livery for the new HTML address. It proves the same content hole remains tile
1, the independent static tile retains its Livery title and heading, Boa's
mutation remains in tile 3, and tile 4 retains its Scrying surface or visible
platform fallback. On Windows with a registered producer, the three
postcondition frames begin only after the native content-readiness checks pass.

The 2026-08-27 locked, offline Windows receipt used the registered Scrying
producer and captured all four lanes in one compositor target. It exited after
160, 219, and 224 host redraws across three runs, within the 600-redraw bound,
with three postcondition frames and routes `genet.livery:document` for tiles 1 and 2,
`genet.scripted:document` for tile 3, and
`scrying.web:surface:CompositedTexture` for tile 4. It recorded `Gemtext
navigation rerouted only tile 1; Livery, scripted, and external neighbors
held`, digest `aaa7ca8720c3aad4`. The two visually inspected 100,839-byte PNGs
were byte-identical with SHA-256
`49ea93762c04f06cc85af2489d0519cb548d1153e37e3e8587538eb5a07797ce`.
The GPU-free mixed driver passes without a Scrying producer by requiring the
same explicit Livery fallback for tile 4 while still proving locality across
all four tiles.

The 2026-08-27 live-resize receipt verified `960x640`, `1024x768`,
`1280x800`, and `1440x900` in one window. It accepted seven native frames with
four imports and seven fence waits, recorded 267 native compositions, and
captured the final 1440x900 state with compositor digest
`cecca1bbdde70a0d`. The 147,416-byte PNG has SHA-256
`8af66f6d5b16de45c3cf502612d9296396128b357ab08011e2315cee848438e9`.

The explicit external-engine fallback receipt is:

```sh
cargo run --locked --offline -p pelt --no-default-features \
  --features livery -j 1 -- \
  --workspace-receipt fallback \
  --artifact target/pelt-receipts/workspace-fallback.png
```

The receipt owns `examples/workspace/p5-fallback/index.html` and its retained
`next.html` destination, again at 960x640 with three postcondition frames. It
starts as an ordinary Livery document, applies the explicit `scrying.web`
selection through the workspace route-policy seam with no Scrying producer
registered, and requires the exact visible fallback reason and route title. On
the following frame it clicks the real `next.html` link through the Frisket
content hole and proves the replacement document remains interactive under the
same fallback route. This receipt tests policy selection and visible document
fallback. A user-facing route-control surface remains P6 work.

The 2026-08-27 locked, offline Windows receipt exited after five host redraws,
including three postcondition frames, with route
`scrying.web:fallback:genet.livery`. Two successive runs visibly captured the
navigated destination and route title and recorded `explicit Scrying pin fell
back visibly to Livery; retained link navigation stayed interactive`. Its
digest is `b12523eac8a17028`; both 105,832-byte PNGs have SHA-256
`556d49064fce0cc18f1f81839920a072b6dc2e8962cd6f1dcf60de574307b8e3`.
The GPU-free test drives the same two policy-and-navigation steps.

`WorkspaceReceiptOutcome` retains the receipt id, assertion, artifact path,
compositor digest, and ordered verified physical sizes through the host
outcome. The CLI prints that evidence alongside routes and frame bounds. Named
workspace receipts reject the older P3 and P4 receipt drivers so two semantic
sequences cannot silently share one capture. The full locked, offline `pelt-desktop` wall with
`livery,scripted,smolweb` passes 23/23, including both new GPU-free drivers;
the focused Pelt CLI parser receipt passes 1/1, and the three-test `pelt-core`
integration wall passes. The final Windows wall used
`--config 'profile.test.package."pelt-desktop".debug=0'`: the default-debug
relink reached MSVC's `LNK1318` PDB limit, while the package-only debug override
left dependency code unchanged and passed all 23 tests.

Fleece's retained Text Fragment follow-through has an additional named receipt:

```sh
cargo run -p pelt --no-default-features --features livery -j 1 -- \
  --product-receipt text-fragment \
  --artifact target/pelt-receipts/text-fragment.png
```

The receipt owns `examples/text-fragment`, addresses its below-fold heading
through a Text Directive, and accepts the capture only when the retained clip
contains the exact target, its geometry is visible, and the primary-document
fetch ledger remains at one. The 2026-08-26 Windows run presented three
960x640 frames, visibly painted the blue selection indication, and recorded
digest `bc3106237d566a57`. Its GPU-free semantic driver passed 1/1.

Check in deterministic fixtures and bounded headed capture commands for:

1. an ordinary article with fonts and images;
2. nested scrolling plus editable form controls;
3. responsive grid and table layout;
4. scripted mutation, timer, and navigation;
5. redirects, imports, linked CSS, images, and fonts;
6. a protocol-native document;
7. a mixed tiled workspace;
8. an explicit external-engine fallback.

Layout defects found here return to Livery/Buckram. Host defects remain in Pelt.
WPT is supporting engine evidence rather than a gate on the reference host.

Done when every fixture has a named command, bounded frame count, captured
artifact, and interaction assertion. At least one optional live-site smoke may
supplement those deterministic receipts.

### P6: presentability and inspection

**Status:** complete for the retained Pelt shell scope. The bounded focused-tile
Chrome, engine-choice-menu, structural-inspection, loading/error-document,
Pelt-owned appearance with optional caller-injected durable storage, shell
AccessKit, narrow-width Chrome, and actual high-DPI Chrome receipts completed
on Windows 2026-08-28.

Add the browser surface that makes the host usable: address field, title and
status, back/forward/reload, tile controls, route indicator, per-tile engine
override, loading/error pages, settings, structural inspector, theme, and
accessibility state.

Done when the chrome is itself a Cambium/Livery composition, remains usable at
small window sizes and high DPI, and exposes failures without consulting the
terminal.

The first bounded Chrome receipt is:

```sh
cargo run --offline -p pelt --no-default-features \
  --features livery,scripted,smolweb -j 1 -- \
  --workspace-receipt chrome \
  --artifact target/pelt-receipts/workspace-chrome.png
```

It reuses P4's four mixed-engine fixtures, but leaves tile 2 as an ordinary
automatic HTML-to-Livery route. The retained Pelt chrome owns a focused-tile
address field, Back, Forward, Reload, title, full route indicator, loading or
failure status, and an Engine control. That control opens a retained one-level
menu with explicit Automatic, Livery, and Scripted-when-compiled rows. Each row
is a `menuitemradio` with checked state, and selection remains a
`PeltWorkspace::set_route_override` operation rather than a Frisket action.
The menu is bound to the tile focused when it opens, then closes on a focus
change, content click, navigation, or explicit selection; it cannot redirect a
later choice onto a neighboring tile. The address driver enters `surface.html`
through the same key path used by the window adapter, submits it, traverses Back
and Forward, reloads the same focused history entry, and then proves the menu
dismissal and explicit engine choices while all other lanes hold.

`Inspect` is a retained, read-only Chrome drawer that follows focused-tile
selection. `PeltWorkspace::inspection` asks the provider actually active for
that tile: document controllers supply their declared capability and optional
structural report, while a live surface reports only the capability declared by
its registered surface engine. An opaque provider suppresses even an accidental
report and discloses exactly `Contents not inspectable on this surface.` The
drawer overlays rather than resizes Frisket content holes, leaves pointer input
with the workspace, and is replayed from its exact Frisket pixel crop after
native texture composition so it remains visible above an external surface.

After Chrome's semantic assertion, its named receipt freezes the producer,
retains the already-imported texture and its required fence wait, captures the
first fully composed postcondition frame, then exits. That boundary avoids a
second producer acquire or an unnecessary return-to-COMMON wait. P5's mixed and
fallback receipts keep their existing three-postcondition-frame cadence.

The chrome is a Pelt-specific retained wrapper around Cambium Frisket, rendered
by Livery and laid out by Buckram. Frisket still owns the pane tree, tab and
divider semantics, and active content holes. To keep a tab title from running
into its close control, the Pelt wrapper supplies a compact visible title while
the chrome carries the full focused title and route; Frisket gives each tab a
fixed 28px close gutter. Ordinary `pelt --tiles` uses chrome. The older P3,
P4, and P5 captures deliberately retain their existing frameless surfaces, so
their named evidence does not silently change.

The GPU-free `pelt-desktop` test drives the full sequence without a registered
Scrying producer and therefore requires tile 4's visible Livery fallback. It
also proves opaque data cannot leak from an accidental report, the retained
drawer preserves content-hole geometry and tab input, and its physical crop
replays above a native layer. The full Livery, Scripted, and Smolweb profile
passed 28 tests. The headed Windows run created one 960x640 window, retained
all four lanes, captured after 252 redraws, and recorded 14 native frames, 11
imports, 14 fence waits, and 252 compositions. Its assertion was
`focused-tile chrome navigated history, bound an explicit engine choice menu,
applied a per-tile override, and exposed truthful structural inspection while
the mixed workspace held`; the captured compositor digest was
`cc5edea62df69340`. The PNG visibly shows the live Scrying tile below the
topmost `Opaque surface` drawer and its honest disclosure.

The bounded P6 loading/error receipt is deliberately smaller and does not
depend on a native surface:

```sh
cargo run --offline -p pelt --no-default-features --features livery -j 1 -- \
  --workspace-receipt loading-error \
  --artifact target/pelt-receipts/workspace-loading-error.png
```

`PeltController` exposes `PeltDocumentState::{Ready, Loading, Error}` for its
active session. `Loading` is not a claim about asynchronous transport: the
current registry spawn is synchronous, so it means a successful replacement
session awaits one host-composed frame. The host marks it ready only after that
frame presents. A failed Address, Reload, Back, or Forward replacement records
the attempted address and error while retaining the prior session and history.

Chrome projects those states as a Pelt-owned retained document in the focused
Frisket content hole. The overlay is not synthetic Livery HTML and does not
change engine ownership. It names the attempted address, explains that the
previous document and history remain available, and leaves underlying content
input routed to its original tile. Its exact Frisket crop is replayed after
document and native tile composition, so the diagnostic remains visible above
the content it describes.

The receipt starts at `examples/workspace/p6-load-error/index.html`, opens the
checked-in `next.html` through the same address-entry path as the window, then
submits the intentionally absent `missing.html`. It proves a composed loading
document, a composed error document in the same content hole, the failed
address and engine error, and the retained `next.html` controller with Back
history. The GPU-free test additionally replaces that error with a successful
address, proves one loading document, and then proves `Ready`. This keeps
missing-resource behavior local and deterministic rather than manufacturing an
engine-level error page or requiring Scrying.

Recorded 2026-08-27: the focused core rollback test passed, including a failed
Back that preserves the active session and history cursor; the Livery-only
`pelt-desktop` suite passed 25 tests; and Pelt's viewer parser/fixture suite
passed 4 tests. The headed Livery run passed at 960x640 after 6 redraws. Its
assertion was `host-owned loading and error documents preserved the focused
tile's prior session and history`; the captured compositor digest was
`6d566041b3febf18`. The PNG shows live Chrome above the retained error document
in the content hole, with the complete recovery instruction visible.

The bounded P6 appearance receipt is:

```sh
cargo run --config 'profile.dev.debug=0' --offline -p pelt \
  --no-default-features --features livery -j 1 -- \
  --workspace-receipt appearance \
  --artifact target/pelt-receipts/workspace-appearance.png
```

It owns `examples/workspace/p6-appearance/index.html` at 960x640. The driver
opens Pelt's retained Appearance drawer through its live Frisket hit path,
selects Light, and captures the composed Chrome frame. Dark and Light are
explicit Pelt-owned choices. The palette reaches Pelt's Chrome, Frisket tabs,
drawer, inspector, and diagnostic documents, while the document engine keeps
its own theme authority. The recorded 2026-08-27 capture used the default
in-memory store; the durable follow-up is recorded below.

The GPU-free checks verify semantic radio controls, the Light action, the exact
physical crop calculation used for post-native replay, unchanged tile-1
address/history posture, unchanged content-hole geometry, and normal content
hit routing after drawer dismissal. Recorded 2026-08-27: the Livery-only
`pelt-desktop` suite passed 28 tests; Pelt's viewer parser/fixture suite passed
4 tests; and the headed Livery run passed at 960x640 after 4 redraws. Its assertion was
`session-only appearance changed the live Pelt chrome theme while the focused
document held`; the captured compositor digest was `b1f8e383dea75045`. The PNG
shows the Light Chrome shell, checked Light choice, retained document hole, and
the session/document ownership notes together.

#### P6 follow-up: caller-owned durable Chrome appearance

Recorded 2026-08-28: Pelt now owns its application-local Chrome appearance
value without taking ownership of a platform configuration directory. The
single `AppearanceTheme` type serves the retained Chrome model and the
`AppearanceSettingsProvider`. That provider describes
`pelt/appearance` / `chrome.theme` as an ordinary, live, application-scoped,
local-only `Text` choice (`dark` or `light`). A rejected reference, setting,
type, or value does not change the active palette.

`WorkspaceViewerConfig::with_appearance_store` receives the caller's store;
without one, Pelt uses an in-memory Dark store and the drawer says that the
choice is session-only. `FileAppearanceStore::load` defaults missing or
malformed values to Dark but returns access and sharing errors to the caller.
Its write path syncs a sibling file and replaces the prior value atomically
(`ReplaceFileW` on Windows), so the live palette changes only after the durable
write succeeds. The reference Pelt CLI exposes this explicitly as
`--appearance-store <path>`, which implies `--tiles`; it invents neither an
app-data path nor a Servo-global preference.

With a caller-supplied file, the same drawer says `Saved for this Pelt
application.` The setting changes only Pelt Chrome, Frisket tabs, drawers, and
diagnostic documents. It does not recolor the Livery document, turn Tabard into
a persistence owner, or move document-engine settings into Pelt.

The GPU-free restart test selects Light through the real Chrome action,
reconstructs the workspace with a fresh store for the same file, and proves the
Light setting, address, selected route, Back/Forward posture, and Frisket
content hole all hold. Missing/malformed fallback, nonrecoverable load errors,
typed settings rejection, and a Light-to-Dark file round trip are covered in
the focused store/provider tests.

The Windows headed receipt used an explicit isolated store and captured the
persisted view:

```sh
cargo run --config 'profile.dev.debug=0' --offline -p pelt \
  --no-default-features --features livery -j 1 -- \
  --workspace-receipt appearance \
  --appearance-store C:\t\genet-pelt-appearance\workspace-appearance.theme \
  --artifact C:\t\genet-pelt-appearance\workspace-appearance-persistent.png
```

It completed after four redraws at 960x640 with assertion
`Pelt-owned appearance changed the live chrome theme while the focused document
held`, digest `f1022ff378c5862e`, and a 134,840-byte PNG. The final store value
was `light`; the capture visibly shows the checked Light option and the saved
application scope beside engine-owned document content. Focused store and
appearance tests passed 9/9, the full Livery `pelt-desktop` suite passed 43/43,
Pelt's viewer suite passed 4/4, and `genet-host-api` settings tests passed 4/4.

The bounded P6 accessibility receipt is:

```sh
cargo run --config 'profile.dev.debug=0' --offline -p pelt \
  --no-default-features --features livery -j 1 -- \
  --workspace-receipt accessibility \
  --artifact target/pelt-receipts/workspace-accessibility.png
```

It owns `examples/workspace/p6-accessibility/index.html` at 960x640. Pelt
projects the completed Frisket Livery/Buckram layout through AccessKit before
revealing its Windows window. The root carries the physical DPI transform, and
the adapter preserves typed `Focus` separately from `Click`: Focus changes
virtual focus only; Click routes through the same Chrome, tab, and close paths
as pointer activation. Because Frisket rebuilds its retained document when
Chrome state changes, virtual focus is held as a semantic target and resolved
again in the fresh tree instead of retaining a foreign DOM id.

The shared Genet projection now carries the Frisket roles and state it already
declares: menu and radio groups, tabs and tab panels, list and combo controls,
separators, toolbars, trees, selected/expanded/checked/pressed state,
orientation, and popup state. `LiveryDocument::retained_layout` is a borrow of
completed geometry, never a layout request. P6 uses it only for Frisket's
fixed, unscrolled shell; wheel and scroll keys remain routed to the active tile
engine. A later scrollable shell needs visual scroll-adjusted bounds before it
can reuse this tree.

Each active Frisket content hole becomes a labelled AccessKit region whose
a11y description states the active engine's declared `A11yCapability`. The
region deliberately stops there: document and native-surface trees still have
opaque ids, and Pelt has not yet defined per-tile namespaces plus action
routing for a composite tree. Thus `Opaque`, `Partial`, and `Full` are each
declared truthfully without claiming that Pelt can merge their child semantics.
If an ordinary platform install fails, Pelt reveals its normal window and
retries installation on a later redraw; the named receipt instead fails until
the bridge reports `Installed`.

The GPU-free Pelt desktop test lays out the shell at 125% DPI, names the
content aperture, verifies its partial child-tree boundary, focuses Theme,
clicks Theme, focuses Light, clicks Light, and proves Light stays checked and
focused after the retained Chrome document rebuilds while the document route
holds. The focused shared projection suite passed 11 tests, the retained-layout
borrow test passed, the Livery-only Pelt desktop suite passed 30 tests, and the
Pelt viewer parser/fixture suite passed 4 tests. Recorded 2026-08-27: two
headed Windows runs both installed 26 shell nodes before show, completed in 6
redraws at 960x640, and produced compositor digest `82555e330e8c7dfd`; their
PNG SHA-256 was also identical,
`930938AFB838DCD8C79C14DCE457CE809E49CD39F28CDE4EE3081D3F35C8D4DA`.

#### P6 follow-up: accessible Chrome address replacement

The host accessibility bridge now retains optional `ActionData` on each queued
`A11yActionRequest`, including AccessKit's string payload for `SetValue`. Pelt
accepts that payload only when it targets the current Frisket Chrome Address
node. It updates Pelt's address buffer and takes its existing address-submit
path. Generic Genet DOM editing remains outside this slice.

The GPU-free Pelt desktop check verifies the Address node's projected value and
`SetValue` action, accepts a checked-in `next.html` destination, observes
`Loading` and settled `Ready`, then submits a checked-in missing destination.
It proves that the focused tile reaches `Error` while retaining the successful
controller address and Back history, that the diagnostic occupies the same
content hole, and that the sibling tile holds. Wrong targets, stale nodes,
missing data, and numeric data are inert. The host bridge unit test separately
proves that string action data survives queue capture.

The named headed receipt is:

```sh
cargo run --config 'profile.dev.debug=0' --offline -p pelt \
  --no-default-features --features livery -j 1 -- \
  --workspace-receipt accessibility-address \
  --artifact target/pelt-receipts/workspace-accessibility-address.png
```

It uses `examples/workspace/p6-load-error/index.html` in two tiles and requires
the platform bridge to report `Installed` before the window becomes visible.
Its bounded driver supplies a typed request through Pelt's current action map,
then captures `Loading`, `Ready`, and retained `Error`. That is evidence for
Pelt's installed-bridge routing and payload handling, not a claim that an
operating-system screen reader emitted `SetValue`. OS-specific injection is a
separate platform receipt.

Recorded 2026-08-28: `genet-winit-host` passed 3/3 tests, the Livery-only Pelt
desktop suite passed 43/43, and Pelt's viewer suite passed 4/4. The headed
Windows run installed 36 retained workspace nodes, completed after five redraws
at 960x640, and reported `installed AccessKit address SetValue routing
navigated only the focused Pelt tile through loading and retained error content
while preserving the successful address and history`; its compositor digest was
`e2ce0126d99cf2c7`. The capture visibly shows the left tile's retained error
document, live Chrome with readable tab labels and close controls, and the
independent right-hand Ready document.

The compact Chrome receipt is:

```sh
cargo run --config 'profile.dev.debug=0' --offline -p pelt \
  --no-default-features --features livery -j 1 -- \
  --workspace-receipt narrow-chrome \
  --artifact target/pelt-receipts/workspace-narrow-chrome.png
```

It fixes the viewport at 360x480 logical pixels through Winit's `LogicalSize`.
The compact retained toolbar uses two rows: navigation, engine, inspection, and
theme controls remain on the first row while the address retains a full second
row. The semantic driver requires that every control stay in bounds, the
address stay at least 300 logical pixels wide, the focused tab retain at least
48 logical pixels of visible label space, and Frisket's independent 28px close
gutter still hit `Close` rather than the tab. It then opens and dismisses the
engine choices, passes through the composed loading state, and captures the
missing-file error document in the unchanged content hole.

On the 2.00x Windows display recorded 2026-08-27, three headed runs completed
after eight redraws at 720x960 physical pixels. All three reported the same
semantic assertion: `compact two-row Chrome kept controls, tab text, and close
targets usable while loading and error documents held their content hole`. The
decoded narrow PNGs differed only by one or two raster pixels in the Chrome
status text, so this receipt deliberately treats its bounded artifact and
GPU-free geometry assertion as its stability boundary rather than claiming a
byte-identical PNG. The focused Livery-only desktop suite passed 33 tests and
the Pelt viewer parser/fixture suite passed 4 tests, covering this path without
GPU or monitor dependence.

Recorded 2026-08-28: Pelt's own Chrome sheet now makes the active tab label
bold and renders its existing `Close` target as a high-contrast, bordered
28px control in both dark and light appearances. The narrow GPU-free test
checks the label centre still selects the tab, the close centre still dispatches
`Close`, its 28px geometry remains fixed, and both appearances resolve the
expected close surface and border. The full Livery-only Pelt desktop suite
passed 43 tests. A headed 2.00x Windows run again completed after eight redraws
at 720x960 physical pixels with the same semantic assertion and compositor
digest `7ce57a057fc3f581`; its checked artifact visibly shows the readable
`Pelt load-state destination` label and distinct close button while the error
document holds the content hole. This is a Pelt-local visual change: Frisket's
shared tab semantics, hit routing, and document-engine ownership are unchanged.

The actual high-DPI Chrome receipt is:

```sh
cargo run --config 'profile.dev.debug=0' --offline -p pelt \
  --no-default-features --features livery -j 1 -- \
  --workspace-receipt chrome-dpi \
  --artifact target/pelt-receipts/workspace-chrome-dpi.png
```

It requests a 960x640 logical viewport and refuses to run below the actual
Winit monitor scale factor of 1.25. Its driver routes the centres of Theme and
Light through the same physical-to-logical conversion as `CursorMoved`, then
proves the Light drawer's physical crop stays in bounds while the active
document address, history, and content aperture do not move. This makes a
simulated unit conversion insufficient as headed evidence.

The current Windows monitor reported 2.00x, so the headed run produced a
1920x1280 physical capture after four redraws. Two runs recorded the same
assertion and compositor digest `3ad1cf889bdfdf4d`; their PNG SHA-256 was
identical, `9B46E903CF51A7D46A4ADCC662210F3455C218C5A7AC9664D2321B8AB3102F47`.
The GPU-free desktop test separately fixes a 2.00x conversion to guard the
coordinate path on machines where the headed environmental receipt correctly
declines to run.

### P7: Livery child accessibility composition

**Status:** complete 2026-08-28 for the retained Livery document lane. This
supersedes P6's deliberately empty Livery content aperture only. Reader,
Scripted, Smolweb, and native surface trees remain declared apertures until
their own engine contracts and action routes exist.

The current Windows AccessKit bridge receives one root update and does not
retain a child `target_tree` identity when an action returns. Pelt therefore
flattens a completed Livery retained tree beneath its Frisket content aperture
with host-owned IDs. Each ID is scoped by tile, installed session generation,
and Livery's local node ID; retired IDs are never reused. A replacement session
gets a fresh namespace, and an old platform action cannot become a new session
or a newly rebuilt Frisket action.

The child root carries the content-hole origin, applied page zoom, and root
document scroll transform. Livery adjusts its child bounds for active nested
element scrolls before Pelt composes them. Pelt withholds Click for a child
whose transformed centre is outside its own content hole, and Livery withholds
Click below an active nested scroller. That leaves inner-scroll
ScrollIntoView, nested-scroller action routing, text/value editing, and native
accessibility grafts as explicit later work rather than publishing a wrong
pointer target.

Focus stays virtual and does not activate or replace the document. A clickable
Livery node re-enters Pelt through the ordinary content pointer path, checks
its tile, session generation, content rect, and point before press, then
checks the generation and rect again before release. The host keeps its Chrome
and the other tiles throughout.

Native HTML anchors with an `href` project as AccessKit links in Livery; anchors
without an `href` remain generic containers. P7's fixture intentionally uses
ordinary anchors, so its existing focus, click, and stale-action proof covers
that engine semantic rather than an ARIA workaround.

The named focused receipt is:

```sh
cargo run --config 'profile.dev.debug=0' --offline -p pelt \
  --no-default-features --features livery -j 1 -- \
  --workspace-receipt accessibility-children \
  --artifact target/pelt-receipts/workspace-accessibility-children.png
```

It owns `examples/workspace/p7-accessibility-children/index.html` and
`next.html`. The driver requires the platform bridge, one composed Livery child
root under the labelled partial-accessibility aperture, a virtual Focus on
`Open child destination`, a Click through Pelt that replaces only tile 1, and a
fresh destination subtree with the old focus absent. Its GPU-free checks add
two identical Livery tiles, root scroll, 125% page zoom, sibling namespace
separation, and an inert stale action after navigation.

Recorded 2026-08-28: the focused core routing suite passed 3 tests, the full
Genet-render suite passed 19 tests including native-link and nested-scroll
projection, the Livery-only Pelt desktop suite passed 37 tests, and Pelt's
viewer parser suite passed 4 tests. The headed Windows run installed the
platform bridge, completed after 4 redraws at 960x640, and reported `Pelt
composed the focused Livery child tree through its retained content hole; Focus
stayed virtual and Click navigated only that session`; its compositor digest was
`01c8f2b912a72ebe`. The final capture shows the destination document inside
live Chrome. The focused Livery zoom check is part of this receipt's maintenance
gate.

### Reader in workspace

**Status:** complete 2026-08-27 as a distinct Fleece/Pelt integration receipt,
not a ninth P5 fixture.

Reader consumes a host-held source response; it does not fetch. When an
installed document exposes the standard `SourceResponse` clip artifact, Pelt
copies its canonical identity, media type, and text body into the tile's
existing `SessionSpawnRequest` while that request has no caller-owned body. The
`SourceResponse` artifact is byte-bearing at the clip boundary; the existing
request seam uses `String::from_utf8_lossy` for the held text body. Pelt
preserves a requested fragment when the canonical response URI has none.

This is intentionally a Pelt-core handoff rather than a Fleece special case.
An ordinary Livery route acquires the source once, Pelt retains its identity,
media type, and text body in the tile request, and an explicit `genet.reader`
selection can reuse it. Moving back to Automatic reconstructs Livery from that
same held request. Fleece and `ReaderSessionEngine` remain fetch-free. An
initially explicit Reader pin is
still host policy: the desktop host acquires the source before constructing that
Reader request, without giving fetching authority to Fleece.

The named receipt is:

```sh
cargo run --config 'profile.dev.debug=0' --offline -j 1 -p pelt \
  --no-default-features --features livery,reader -- \
  --workspace-receipt reader \
  --artifact target/pelt-receipts/workspace-reader.png
```

It owns `examples/workspace/reader/index.html` and its ordinary Livery neighbor.
The driver starts with both tiles on automatic Livery, opens the live Chrome
engine menu, chooses Reader for the focused article, inspects Fleece lineage,
returns to Automatic Livery, and chooses Reader again. Its GPU-free counterpart
uses a counting host fetcher: the article and neighbor each acquire one response,
and the Livery-to-Reader-to-Livery-to-Reader cycle causes no second document
fetch. It also checks the held body, Reader's resolved link, Livery's original
relative link spelling, extracted heading/title, Fleece lineage, and the
unchanged neighbor route.

Recorded 2026-08-27: `pelt-core` routing passed 3 tests, including
`source_artifact_is_held_across_a_live_route_switch`; the Livery/Reader
`pelt-desktop` suite passed 35 tests; the Pelt viewer parser suite passed 4;
and the Reader-only `genet-documents` suite passed 5. Two headed Windows runs
completed after 9 redraws at 960x640 with routes
`1=genet.reader:document,2=genet.livery:document`. They produced the same
semantic assertion and two 177,896-byte PNGs, but not byte-identical rasters:
their compositor digests were `c0636e846a7651d2` and `28fca32b4c7b3797`, with
SHA-256 `D3B775C92B814202D6BC3B967EFE38B216A1F29962D99BDCD6861D5346C1B798`
and `A9D518956D7E41FAE2A21CD9BDB6E4687C740AC3F1193B0AAA6CA342E0039618`.
The decoded captures differ by one blue-channel value in one Chrome-status text
pixel, so this receipt treats its semantic driver and bounded artifact as its
stability boundary rather than claiming a byte-identical PNG. The capture shows
the Reader tab and close control, selected Reader engine, extracted article, and
Fleece lineage in the retained inspector.

## Cross-gate rules

- One host-owned wgpu device and compositor serve the full workspace.
- Livery and Buckram retain CSS and layout authority; Pelt consumes their
  session contract.
- Frisket owns the split/tab view; `TileTree` owns standalone Pelt arrangement
  state.
- Transport and protocol choices remain host policy.
- Tier escalation is visible, configurable, and testable.
- Compatibility spellings may remain at parsing and Cargo boundaries. They do
  not survive as distinct runtime states.
- Each gate lands with a focused deterministic receipt before the next gate
  widens the host.

## Immediate next lane

P6's retained-shell receipts, P7's Livery child-tree receipt, and the separate
Reader-in-workspace receipt are complete. The AccessKit tree now composes the
retained Livery child tree for its focused supported lane; Reader, Scripted,
Smolweb, and native child trees still need their own namespace and action
contracts. Livery still needs nested-scroll ScrollIntoView/action routing and
text/value semantics before Pelt can widen that child lane. Pelt now owns an
optional caller-injected local appearance store; system-theme integration,
multi-window synchronization, and a canonical configuration-directory policy
remain distinct work. The held-source handoff is now a reusable Pelt boundary,
not a fetch API for Fleece.

The completed Windows P4 route remains the external-engine comparison lane.
IOSurface, DMA-BUF, multi-GPU adapter selection, native-overlay attachment, and
embedded-host attachment stay independently receipted platform work. An
optional live-site smoke may supplement the deterministic P5 set, but does not
replace or reopen it.
