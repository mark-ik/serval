# genet Documentation Index

The canonical index for `genet/design_docs/`, per [`DOC_POLICY.md`](DOC_POLICY.md)
§6. If any other index disagrees with this file, this file wins.

Founded 2026-08-24, when the canonical policy core was distributed across the
workspace and the component documents for inker, nematic and verso-tile were
repatriated here from mere.

> **Read the policy's "Two doc homes" section first.** This repository also has
> a flat `docs/` directory of ~163 older engine documents that this index does
> **not** cover and that no index covers. That split is deliberate and
> temporary; migrating it is open, unscheduled work. New documents go here, not
> there.

## Required reading order

1. The root [`README.md`](../README.md) — what genet is.
2. [`DOC_POLICY.md`](DOC_POLICY.md) — the shared core plus this repo's addendum,
   including the `docs/` boundary and the smolweb split.
3. The area root you are working in, below.

## fleece — reader extraction

- [fleece_standards_adoption_plan](2026-08-24_fleece_standards_adoption_plan.md)
  (**complete 2026-08-25**: Fleece 0.2 shipped canonical DOM-text coordinates,
  W3C Text Quote and Text Position selectors, and a Text Fragment projection;
  0.3 hardened JSON-LD syntax harvesting and HTML Microdata; 0.4 added ordered
  Open Graph grouping, DOM document links, and semantic HTML table grids and
  header associations. The releases preserve Fleece's render-free, raw-URL,
  source-identity, and caller-owned annotation boundaries.)
- [fleece_followthrough_plan](2026-08-26_fleece_followthrough_plan.md)
  (**complete 2026-08-26**: the
  `genet-extract` shim is retired; retained static/scripted hosts activate
  Fleece-generated Text Directives with element fallback, indication, scrolling,
  one-fetch behavior, and script-visible URL privacy; Mere crawl and Gazette now
  consume supplied documents while eidetic-search drops its misplaced edge.
  Focused automated gates and the headed activation/indication receipt are
  green.)

## layout and styling

- [livery_flex_shorthand_plan](2026-08-25_livery_flex_shorthand_plan.md)
  (**complete flex-shorthand slice; Row 18 remains in progress**: Livery now
  expands `flex` and `flex-flow` into the longhand style fields already lowered
  to Taffy. The exact 1,358-file flexbox map records 115 gains, three assigned
  downstream false-pass losses, and the numeric-basis parser repair forced by
  the first candidate. The current eight-input Taffy seam is published and
  consumed as `genet-taffy 0.14.0`, published and tagged
  `genet-taffy-v0.14.0` at the Row 18 closure; 0.13.1 was the eight-input
  seam before it.)

## workspace composition

- [workbench_component_plan](2026-08-31_workbench_component_plan.md)
  (**W1–W3 implemented, W4 receipts captured, 2026-09-01**; the temporary
  `genet-host-api::tile` shim is removed. Establishes Workbench as the reusable Genet
  split/tab/tearout component, moves the existing `TileTree` reducer out of
  `genet-host-api`, makes Pelt and Cambium direct consumers, and defines the
  coordinated Mere/Graphshell Projection Editor adoption without transferring
  graph, browser-session, or window authority into the component.)

## inker_docs/ — the engine controller

- [engine_picker_and_pluggability_plan](inker_docs/implementation_strategy/2026-06-15_engine_picker_and_pluggability_plan.md)
  (**Phases 0–3 shipped + verified** — route → activate → manage → pick. The
  user-facing engine **picker** as an inker affordance, distinct from verso's
  flip; the three-level pluggability model (in-build / registered-but-off /
  active via `is_available = contains && enabled`), a global default with
  per-session override, and the two content tiers as the two registries —
  glass/black-box, wasm/native. No-handler fallback and local-file ingestion
  are in scope but not yet shipped (Phase 4).
  **Remaining**: Phase 4 (no-handler and local files), Phase 2b (per-host and
  per-session), Phase 5 (the verso flip). **Page capture P1 landed 2026-08-30**:
  Inker defines correlated viewport-only capture requests/results, but no
  engine yet claims support. Its progress log names meerkat, which was deleted
  2026-07-18; read those as the Turnstone/mere hosts.)

## nematic_docs/ — the smolweb engine and knot composition

- [polyglot_knot_design](nematic_docs/implementation_strategy/2026-05-08_polyglot_knot_design.md)
  (**implemented 2026-05-08/09**, retained as design rationale and format spec:
  extends the `nematic.knot` note format from frontmatter-plus-markdown to a
  polyglot composition where every other `nematic.*` protocol's blocks embed
  fenced-code-block-style and round-trip back to the source protocol's syntax.)
- [knot_evaluation_export_plan](nematic_docs/implementation_strategy/2026-06-12_knot_evaluation_export_plan.md)
  (**reconciled and complete for the first production capability set,
  2026-07-27**: `include` transclusion fences over errand's smolweb transports,
  `lua eval` / `rhai eval` script fences via the `BlockEvaluator` slice,
  `to_gemtext` and gophermap exporters, the Knot production effect bridge,
  Turnstone consent, and the sealed attributable resolve cache. The production
  Knot adapter supplies anonymous HTTP(S) plus read-only Gemini, Gopher,
  Finger, Spartan, Nex and Guppy; Titan stays excluded.)
- [polyglot_block_resolver_plan](nematic_docs/implementation_strategy/2026-06-13_polyglot_block_resolver_plan.md)
  (**planned**: collapse the three separate passes — `expand_fenced_blocks`,
  `resolve_transclusions`, `evaluate_blocks`, each with its own dispatch and
  trust handling — into one registry that resolves *any* fenced block by its
  tag, and the new resolver kinds that makes pluggable: graph/eidetic query
  blocks, diagram DSLs, sandboxed wasm blocks.)
- [native_smolweb_rendering_plan](nematic_docs/implementation_strategy/2026-06-27_native_smolweb_rendering_plan.md)
  (**planning (with Mark)**: render every smolweb format natively and
  idiomatically rather than flattening it into one model. The **two-family
  model** — document family (djot/markdown/reader-HTML, native `Block`) versus
  smolweb family (gemtext/gopher/feed/scroll/misfin, a per-format AST, views
  shared with the host because they avoid `Block`). Its §5 crate-home diagram
  is superseded by the smolweb home decision; crate homes read through that.)
- [smolweb_fidelity_plan](nematic_docs/implementation_strategy/2026-07-01_smolweb_fidelity_plan.md)
  (**planning (with Mark)**: recovers the spec-faithfulness the flavour-neutral
  pipeline collapses. Key code-verified finding — **the losses are at the parse
  ASTs, not the box rendering**, so the fix is richer ASTs rather than a
  different render regime. Three workstreams: enrich the parse ASTs; produce
  trust at the transport and carry it through the native lane, which currently
  drops `DocumentTrustState`; and bespoke rendering only where the line model
  is not box-shaped, gopher's fixed-width typed column being the clear case.
  **WS1's enrichment lands wherever the grammar lives at the time** — if a
  grammar has moved to a smolweb crate, the enrichment goes there.)

## verso_docs/ — rendering surfaces and the engine flip

- [compatibility_view_charter](verso_docs/technical_architecture/2026-06-10_compatibility_view_charter.md)
  (**charter decision (Mark, 2026-06-10)**, pre-implementation: verso reborn as
  the engine-flip / compatibility-view seam — portable view-state carriers and
  the one-hop invariant, minted at the first genet→scrying flip.)
- [genet_scrying_flipcarrier_plan](verso_docs/implementation_strategy/2026-06-23_genet_scrying_flipcarrier_plan.md)
  (**design resolved; `verso-api` plus the genet donor primitives shipped
  2026-06-23**: the first flip. The plan's crate layering — `verso-api` plus
  per-engine `verso-genet`/`verso-scry`/`verso-weld`/`verso-graft` adapters
  plus a `verso` orchestrator — was **consolidated into the single
  `components/verso-tile` crate on 2026-07-09** (modules `api`, `flip`,
  `scry`, and the `genet-donor` feature); read the plan's crate names as that
  crate's modules. Host-wired and feature-gated with no engine stacking;
  `FlipDonor`/`FlipBack`/`FlipReceiver` encode no-chain in the types. Gated on
  the inker picker's Phase 4.)

## codebase structure

- [orchestrator_decomposition_plan](2026-08-28_orchestrator_decomposition_plan.md)
  (**complete 2026-08-29, all seven phases landed**: the measured inventory of
  first-party files over 600 lines, and the order the large orchestrators came
  apart in. Pelt's `workspace_viewer.rs` went 10,066 → 3,217 lines (3,960 by
  2026-09-01, grown by the accessibility work since), `genet-livery/layout.rs`
  15,382 → 6,165, `buckram/taffy_adapter.rs` 7,014 → 1,223, all as pure code
  motion. Records two constraints any later phase inherits: module privacy runs
  parent-to-child only, so relocating shared types forces a visibility rewrite;
  and moved inherent methods must have their original scope named explicitly.)

## Working principles

- **New docs go in `design_docs/`, never `docs/`.** See the policy's two-homes
  section for why both exist and what it would cost to merge them.
- **The smolweb boundary is spec versus use.** What a protocol *is* belongs to
  the smolweb workspace; what a browser *does with it* belongs here. Cite
  across the boundary by path — relative links do not survive it.
- **Prefer runtime verification to extended static tracing.** If runtime
  diagnostics are blocked, surface that blocker early rather than continuing to
  read code.
- **State the exact standards layer implemented.** Selector values are not the
  Web Annotation Protocol; JSON-LD syntax harvesting is not JSON-LD processing;
  raw URL attributes are not resolved links. Keep these boundaries visible in
  public types and receipts.
- **Parallel work needs commit fences as well as file fences.** Pin one base,
  give each worker a disposable detached worktree and disjoint write paths,
  inspect staged paths before committing, and remove the worktree immediately
  after integration.

## Status

Founded 2026-08-24; audited against the tree 2026-09-02. The active index
covers five flat plans plus eight documents in three area roots. The engine corpus in `docs/` is not indexed here
and is not governed by the policy yet.
