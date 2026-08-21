# Changelog

## 0.3.3 - 2026-08-09

- Publish the post-0.3.2 workspace state under its own number. The registry
  0.3.2 lacked the additions below, which made the local tree and the
  published crate diverge under one version and failed `cambium-winit`'s
  package verification against the registry.
- Add the shared text-editing primitive and the IME composition surface
  (`CompositionEvent`, `Key::Composition`) that `cambium-winit` 0.3.0
  translates into.
- Move radio-group behavior into Cambium and repair the component-catalog
  receipt gate.
- Canonicalize organization repository URLs.

## Unreleased

- Version `meristem` as 0.2.0 for the public scope cut: remove the unused
  worker, task, environment, and documentation-backend APIs, and keep the
  retained diff and message core.
- Split the Genet/AccessKit accessibility host out of `cambium-winit` into
  the deliberately unpublishable `cambium-winit-a11y` (2026-07-26), leaving
  `cambium-winit` at `cambium` + `winit` only and publishable again. The
  0.3.0 note below ("crates.io publication waits for the standalone
  `genet-layout` package boundary") is superseded by that split.
- Release-record reconciliation (2026-08-09): `cambium` 0.3.3 and
  `cambium-winit` 0.3.0 published the same day, closing the registry gap;
  `meristem` 0.1.1, `sprigging` 0.2.1, and `cambium-nematic` 0.3.1 were
  current at that reconciliation; `cambium-winit` 0.2.0 remains yanked.
  Authority:
  `docs/genet-compatibility.md`.

## 0.3.0 - 2026-07-22

- Add `HoverEvent`, `HoverPhase`, `on_hover`, and runner dispatch seams for
  host-computed Enter, Leave, and Move transitions.
- Expand the component catalog into an executable acceptance surface covering
  controls, editors, action routing, overlays, grid virtualization, semantic
  attributes, keyboard behavior, and Sprigging leaf painting.
- Give data grids explicit grid, row, column-header, and cell semantics, with
  keyboard activation for sortable headers.
- Use the canonical `genet_scripted_dom` Rust crate name throughout Cambium,
  Cambium Nematic, tests, and examples.
- Replace stale Serval-era crate documentation with the current Cambium,
  Meristem, Sprigging, and Genet ownership boundary.
- Add the retained `GraphCanvas` leaf plus Cambium's interactive
  `graph_canvas_swatch`, including focus, data keys, optional expansion, and
  visible node labels.
- Add shared overlay, command-menu, disclosure, summary, detail, sectioned-list,
  split, tab, segmented-control, and reorderable-list surfaces.
- Add `caret_text_field` for DOM-painted carets and forest-DOM projections for
  multiple window roots over one application state.
- Add Cambium Winit scroll policy and the Genet/AccessKit accessibility host.
  The source release is versioned here; crates.io publication waits for the
  standalone `genet-layout` package boundary.

## 0.2.0 - 2026-07-14

- Make `GenetCtx`, `GenetElement`, `GenetAppRunner`, and related `Genet*`
  names canonical. Deprecated `Serval*` aliases remain for migration.
- Make buttons, checkboxes, switches, radio groups, selects, and sliders follow
  standard keyboard and accessibility interaction patterns.
- Add the searchable, keyboard-complete `action_list` component.
- Make normal manifests resolve Genet seams from crates.io so a standalone
  checkout does not require a sibling Genet repository.
- Add CI for formatting, focused Clippy, workspace tests, and package checks.
