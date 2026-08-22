# Pelt host reconstruction execution plan

**Date:** 2026-08-22

**Status:** active. P0 completed 2026-08-22. P1 is the next implementation
lane.

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

The present Pelt executable still owns one private winit event loop and one
document session. Its click adapter reduces `SessionClick` to a boolean and
therefore discards host-owned navigation and submission outcomes. No Pelt code
currently consumes `TileTree`, Frisket, or a `SurfaceEngineRegistry`.

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

### P2: extract the embeddable host core

Create a public Pelt controller that owns registries, sessions, navigation, host
effects, and frame production without creating an event loop or window. Inject
the host's resource policy, engine registries, wgpu device/queue, target size,
clock, and settings.

`pelt-desktop` becomes a thin consumer that translates winit events and presents
the returned frame. A second focused test host drives the same controller with a
caller-owned target.

Done when standalone Pelt and the test embedder use the same controller, and the
controller creates neither a winit event loop nor a wgpu device.

### P3: restore recursive tiling and nesting

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

### P4: route by capability and compose surface engines

Install long-lived document and surface registries in the host core. Select a
route from scheme, content type, user settings, and declared capability. Keep
the selection visible and user-overridable per tile.

Compose `CompositedTexture` surfaces into the shared wgpu frame. Treat
`NativeOverlay` and `EmbeddedHost` as explicit platform contracts. Avoid
per-tile devices, readback, and hidden child windows.

Done when one window simultaneously hosts a smolweb document, static Livery
HTML, scripted Livery HTML, and an external surface fallback.

### P5: make Pelt the product receipt for Livery/Buckram

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

P1 should begin at `LiveryViewerContent::click_at` and `DocumentSession`. Define
the smallest host-effect and input contracts that make link navigation, form
editing, and history work in the present single-document adapter. That receipt
then becomes the forcing consumer for extracting the P2 host core.
