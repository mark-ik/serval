# Pelt host reconstruction execution plan

**Date:** 2026-08-22

**Status:** active. P0 completed 2026-08-22. P1 was rebased and reverified on
current `main` 2026-08-24. P2 and P3 completed 2026-08-24. P4 completed on
Windows 2026-08-25 with a registered Scrying producer, native shared-handle
import, repeated fence waits, and visible same-window composition. P5 is the
current lane. IOSurface and DMA-BUF imports remain separate platform lanes.

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

**Status:** in progress 2026-08-26. Fixtures 1-5 are complete; fixtures 6-8
remain open.

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

Add the browser surface that makes the host usable: address field, title and
status, back/forward/reload, tile controls, route indicator, per-tile engine
override, loading/error pages, settings, structural inspector, theme, and
accessibility state.

Done when the chrome is itself a Cambium/Livery composition, remains usable at
small window sizes and high DPI, and exposes failures without consulting the
terminal.

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

Continue P5 from the completed ordinary-article, controls, responsive-layout,
scripted-navigation, and redirected-resource-graph receipts. Add named receipts
for protocol-native content, the mixed workspace, and the explicit fallback.
The Reader tile must carry held source bytes rather than teaching Fleece to
fetch. Each remaining fixture needs a bounded command, captured artifact, and
interaction assertion. The completed Windows P4 route remains the external
engine comparison lane; IOSurface, DMA-BUF, and multi-GPU hardening stay
independently receipted work.
