# genet Documentation Index

The canonical index for `genet/design_docs/`, per [`DOC_POLICY.md`](DOC_POLICY.md)
§6. If any other index disagrees with this file, this file wins.

Founded 2026-08-24, when the canonical policy core was distributed across the
workspace and the component documents for inker, nematic and verso-tile were
repatriated here from mere.

> **Boundary correction landed, 2026-09-03:** Cambium and Genet's upper
> application components moved to Mere under
> `mere/design_docs/mere_docs/implementation_strategy/2026-09-02_platform_boundary_and_repository_topology_plan.md`.
> Their documents moved with the code and are indexed in Mere. Genet retains
> web-platform implementation, observable behavior, raw host contracts, WPT,
> and a minimal engine host.

> **Read the policy's "Two doc homes" section first.** This repository also has
> a flat `docs/` directory of ~163 older engine documents that this index does
> **not** cover and that no index covers. That split is deliberate and
> temporary; migrating it is open, unscheduled work. New documents go here, not
> there.

## Required reading order

1. The root [`README.md`](../README.md) — what genet is.
2. [`DOC_POLICY.md`](DOC_POLICY.md) — the shared core plus this repo's addendum,
   including the `docs/` boundary and the smolweb split.
3. The section you are working in, below. genet has no topic area root left:
   the last three moved to mere with their code on 2026-09-03, and active
   plans sit flat in `design_docs/`.

## Scripted host capabilities

- [scripted_host_capabilities_plan](2026-09-06_scripted_host_capabilities_plan.md)
  (**in progress 2026-09-06**): per-document deferred Fetch and WebGL services
  installed before authored scripts, ordinary-host receipts, and a bounded
  Row 17 generated-text slice. K6 continues separately.

## ortet — the raw host

- [ortet_founding_plan](2026-09-03_ortet_founding_plan.md) (**O0, O1 and O4
  landed 2026-09-03; O2 accessibility and O3 web target open**: the one headed port that proves the
  engine runs without Mere, over `genet-winit-host`, `genet-render-host`,
  `genet-documents`' Livery lane and `document-session-api`, with a cone
  witness that forbids every Mere crate and a self-driven frame receipt.
  Takes Pelt's place when Pelt moves to mere under the boundary plan.)

## fleece — reader extraction

- [fleece_followthrough_plan](2026-08-26_fleece_followthrough_plan.md)
  (**complete 2026-08-26**: the
  `genet-extract` shim is retired; retained static/scripted hosts activate
  Fleece-generated Text Directives with element fallback, indication, scrolling,
  one-fetch behavior, and script-visible URL privacy; Mere crawl and Gazette now
  consume supplied documents while eidetic-search drops its misplaced edge.
  Focused automated gates and the headed activation/indication receipt are
  green.)

## layout and styling

- [common_script_font_fallback_plan](2026-09-04_common_script_font_fallback_plan.md)
  (**scope, 2026-09-04**: on Windows and macOS the stack never successfully
  consults font fallback for any Common-script codepoint — arrows, geometric
  shapes, box drawing, dingbats, punctuation above Latin-1 — because fontique
  keys fallback on a per-script *sample string* and has no sample for Common,
  so `fallback()` returns `None` by construction. Verified link by link in
  parley and fontique source. T0 a failing instrument, T1 a stack-side repair,
  T2 the codepoint-aware upstream fix both platform APIs already support,
  T3 whether Linux shares it.)

- [livery_flex_shorthand_plan](2026-08-25_livery_flex_shorthand_plan.md)
  (**complete flex-shorthand slice; Row 18 remains in progress**: Livery now
  expands `flex` and `flex-flow` into the longhand style fields already lowered
  to Taffy. The exact 1,358-file flexbox map records 115 gains, three assigned
  downstream false-pass losses, and the numeric-basis parser repair forced by
  the first candidate. The current eight-input Taffy seam is published and
  consumed as `genet-taffy 0.14.0`, published and tagged
  `genet-taffy-v0.14.0` at the Row 18 closure; 0.13.1 was the eight-input
  seam before it.)

## cambium — the desktop host

- Cambium, Workbench and `mere-surface-api` left genet for mere on 2026-09-03
  under the platform boundary plan; the `host_ui_zoom_plan` and the
  `workbench_component_plan` travelled with them and are now in mere's
  `design_docs/`.

## inker_docs/, nematic_docs/, verso_docs/ — moved to mere

- The engine-management layer left genet for mere on 2026-09-03 under the
  platform boundary plan: `inker`, `document-canvas`, the scrying/graft/weld
  engine adapters, `verso-tile`, `nematic`, `illume`, `errand` and `tinct`.
  These three area roots travelled with their code and are now in mere's
  `design_docs/`.

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

## archive_docs/ — completed plans

Per policy §4 and §8: a plan moves here once complete and once its open
points have a home elsewhere. Links into a moved plan are repaired in the
same session; links out of it are rewritten for its new depth.

- [2026-09-02/fleece_standards_adoption_plan](archive_docs/2026-09-02/2026-08-24_fleece_standards_adoption_plan.md)
  (**complete 2026-08-25, archived 2026-09-02**: Fleece 0.2 shipped canonical
  DOM-text coordinates, W3C Text Quote and Text Position selectors, and a Text
  Fragment projection; 0.3 hardened JSON-LD syntax harvesting and HTML
  Microdata; 0.4 added ordered Open Graph grouping, DOM document links, and
  semantic HTML table grids and header associations. No open points carried.)
- [2026-09-02/knot_evaluation_export_plan](archive_docs/2026-09-02/2026-06-12_knot_evaluation_export_plan.md)
  (**reconciled and complete for the first production capability set
  2026-07-27, archived 2026-09-02**; `include` closed, TOFU location rehomed to
  the fidelity plan, badge default carried by the block resolver plan. `include` transclusion fences over errand's smolweb transports,
  `lua eval` / `rhai eval` script fences via the `BlockEvaluator` slice,
  `to_gemtext` and gophermap exporters, the Knot production effect bridge,
  Turnstone consent, and the sealed attributable resolve cache. The production
  Knot adapter supplies anonymous HTTP(S) plus read-only Gemini, Gopher,
  Finger, Spartan, Nex and Guppy; Titan stays excluded.)

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
covers four flat plans plus seven documents in three area roots, with two
completed plans in `archive_docs/`. The engine corpus in `docs/` is not indexed here
and is not governed by the policy yet.
