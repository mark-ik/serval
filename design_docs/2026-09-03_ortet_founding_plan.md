# Ortet founding plan

**Status:** plan, 2026-09-03. O0 and O1 in progress the same day.

Ortet is the raw genet host: the one headed port that proves the engine runs
without Mere. The platform boundary plan
(`mere/design_docs/mere_docs/implementation_strategy/2026-09-02_platform_boundary_and_repository_topology_plan.md`,
§2 and §9.3 ruling 4) leaves Genet "WPT, conformance harnesses, and one small
host that proves Genet works without Mere". Pelt cannot be that host: its
desktop port enables the reader and netfetch lanes by default, and `pelt-core`
depends on `workbench` and `inker`, both Mere authority. Under the "consumers
first" order Mark ruled on 2026-09-03, Pelt moves to mere before Cambium does,
and from that commit genet's only host is `genet-wpt`, which is headless. Ortet
takes the role a smaller host was always going to take.

The name is the botanical one. A genet is a whole clonal colony; an ortet is the
original individual it descends from; a ramet is any member. The raw host is
the reference individual of the engine. Mark ruled "ortet is fine" on
2026-09-03; the name is free on crates.io and is claimed with a real publish
when the crate has one worth publishing (naming ledger rule).

## What ortet is, and is not

- A `ports/ortet` binary over engine crates only: `genet-winit-host` (window,
  wheel translation, AccessKit bridge) and through it `genet-render-host`
  (wgpu + netrender boot, rasterize, acquire, compose), `genet-documents` with
  its `livery` feature (`LiverySessionEngine`), `document-session-api`
  (`SessionEngine`, `DocumentSession`, `SessionSpawnRequest`), `netrender`
  (`Scene`), `genet-host-api` (`ResourceFetcher`, `ResourceResponse`) and
  `netfetcher` with its default transport for http(s).
- It drives the session itself. Pelt routes through inker's `SessionRegistry`
  and `PeltController`; ortet holds one `LiverySessionEngine`, spawns one
  `DocumentSession<Scene>` for the address it was given, and maps winit events
  onto the session's semantic input directly. Following a link is spawning a
  new session for the new address.
- It has no chrome. One window, one document, no tabs, no tiles, no reader
  lane, no smolweb, no settings, no persistence. Anything of that kind is Mere.
- **Its dependency cone contains no Mere crate**, and the cone witness says
  so on every CI run: none of `inker`, `workbench`, `cambium*`, `mere-*`,
  `nematic`, `errand`, `document-canvas`, `fleece`, `pelt*`, `tabard`,
  `knot-editor-host`.
- Its receipts are self-driven. `ortet --url <file or http(s)> --frames N
  --artifact out.png` presents N frames, reads the last one back through
  `genet-render-host`, writes the PNG, prints the frame digest and exits
  non-zero on a blank frame. That is what CI and a plan can cite; a person
  looking at the window is confirmation, not the gate.

## Phases

### O0. Found the crate

- `ports/ortet/Cargo.toml` (workspace-inherited version, license, edition;
  `publish` as the workspace sets it; a description that says what it is),
  `ports/ortet/src/main.rs`, `ports/ortet/README.md`, workspace member entry
  beside `ports/pelt`; every new source carries the house header (copyright
  line, Exhibit A, SPDX).
- `support/ci/check_dependency_cones.py` gains `assert_ortet_cone`: resolve
  ortet's cone from `cargo metadata` and fail on any crate in the forbidden
  set above. Positive control in the same function: run the same check over
  `pelt-desktop`'s cone and require that it *does* report `inker`, so the
  check is proven able to see what it forbids. `assert_ports_depend_inward`
  keeps its Pelt assertion until Pelt moves and gains the ortet manifest
  beside it.

**Done when:** `cargo check -p ortet` is green, the witness passes with the
positive control, and `python support/ci/check_dependency_cones.py` is what
CI already runs.

### O1. The desktop viewer

- Boot `SurfaceHost` on a winit window sized from `--size` (default 960x640);
  `NetrenderOptions::for_untrusted_content()`, since a raw browsing host is
  exactly the case that option exists for.
- Fetcher: `LocalFetcher` for `file:` and bare paths, falling back to a
  netfetcher-backed `ResourceFetcher` for http(s) over the default transport,
  on a background tokio runtime the host owns. Nothing else: no trust store,
  no scheme sniffing, no smolweb.
- Per frame: `session.frame(w, h)` gives the `Scene`; rasterize, acquire,
  compose, present, as the host crates document. `pump` and `settled` drive
  redraw scheduling.
- Input: pointer down/move/up, wheel via the host's translation into
  `scroll_at`, keyboard through `key_input` and `scroll_for_key`, text and IME
  input, focus in and out, resize. A `SessionClick` whose effect navigates
  spawns a new session for the new address; the window title follows.
- The receipt path above, with the PNG written through the `png` crate the
  workspace already carries and the digest from `RgbaFrame::digest`.
- A fixture under `ports/ortet/examples/`: one script-free article with a
  linked stylesheet, an image and an in-page link, so one receipt exercises
  fetch, layout, paint and navigation. Reuse Pelt's `p5-resources` fixture
  only if it needs no Pelt code.

**Done when:** the article receipt produces a non-blank frame with a digest
that is identical across two runs on the same machine; a headed run opens
the fixture, scrolls, and follows its in-page link and one external link to a
second document; `cargo test -p ortet` (unit tests for the argument parser and
the fetcher's scheme split) is green; and the O0 witness still passes.

### O2. Accessibility

- Wire `AccessKitBridge` the way Pelt's workspace viewer does, over the
  session's accessibility projection from `document-session-api`, with
  `A11yActionRequest` routed back into the session.

**Done when:** the bridge reports a tree whose root carries the article's
heading, and an action request (focus, scroll into view) reaches the session.

### O3. The web target

Deferred until P2 of the boundary plan lands, because the working canvas host
today is `cambium-genet-web-host`, which is Cambium and moves to mere. Ortet's
web target is the same `RenderCore` over an `HtmlCanvasElement`, driven by DOM
events, with no Cambium. Its receipt must come from real Chromium: the
in-app browser pane never composites canvases, so a screenshot there proves
nothing (`Code/testing` harness notes).

**Done when:** `wasm32-unknown-unknown` builds, the article renders in Chromium
with a non-blank canvas readback, and the witness holds for the web target's
cone too.

### O4. Take over Pelt's place

When Pelt moves to mere (boundary plan P3): ortet becomes the workspace
`default-members` entry, the witness drops the Pelt manifest assertion and
keeps ortet's, the boundary plan's ruling 4 records the smaller host as the
answer, and the naming ledger's claim is made with a real publish if the crate
is worth one by then.

**Done when:** genet's root `cargo build` builds ortet, the witness has no Pelt
reference, and the boundary plan and naming ledger both say so.

## Findings

- 2026-09-03: `genet-winit-host` and `genet-render-host` already split the
  window-specific from the target-neutral present mechanics, and both
  document the per-frame shape a host follows; ortet adds no rendering code.
- 2026-09-03: `LiverySessionEngine<Fetch>` needs only a `ResourceFetcher`;
  `genet-documents` no longer depends on inker, netfetcher, errand, nematic or
  document-canvas after the boundary plan's P1, so the engine half is the
  whole of what ortet needs from the documents crate.
- 2026-09-03: Pelt's `static_viewer.rs` (2,622 lines) is the nearest prior
  art, but 140 references to the engine and host crates are wrapped in inker
  routing, Pelt profiles and seven product receipts. Ortet is written fresh
  against the two host crates and the session traits, not extracted from it.

## Progress

- 2026-09-03: plan written; O0 and O1 dispatched.
