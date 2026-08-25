# K5b grid static-rectangle reconciliation

**Date:** 2026-08-24

**Status:** Complete on accepted `origin/main`
`7eaaaf724a57b5f4fd3f18b21037777a40a5510d`.

**Parent:** [K5b static-position rectangles](2026-08-10_buckram_k5b_static_position_execution_plan.md)
and row 4 of the
[Buckram and Livery lane program](2026-08-21_buckram_livery_lane_program_plan.md).

## Ruling

Row 4 asked for the two live `tests/grid_abspos.rs` receipts and the current
code path responsible for them. Both are present and green on accepted main.
No archived lane source was applied and no current source repair was required.

The authority chain is:

1. Livery's `enable_flex_grid_static_position_provider` admits only a direct
   absolute or fixed child of a flex or grid scratch parent.
2. The Livery box graph supplies the K5a relationship. It selects the grid area
   only when that same grid is the child's containing block.
3. Buckram's `AlgorithmTree::enable_flex_grid_static_position_provider` owns
   the private renderer-role switch, and
   `use_grid_area_for_static_position` records the explicit selection.
4. Buckram's `LayoutGridContainer::grid_child_static_position_area` chooses
   the grid content box by default, or the finalized grid area with padding
   edges for automatic lines when K5a selected that grid.

This is a narrow provider boundary, not a claim that Buckram has replaced the
private flex/grid renderer algorithm.

## Provenance

- Worktree: `worktrees/genet-k5b-grid-static-rectangle-v2`.
- Isolated target: `C:\t\genet-k5b-grid-static-rectangle-v2-target`.
- External ledger:
  `testing/genet/wpt-ledger/2026-08-24_k5b_grid_static_rectangle_v2`.
- Ignored offline `Cargo.lock` SHA-256:
  `6F91D0C46D3BF171137B16023C04C5A8032B405CD10D321E6D41459CAD4EBE49`.
- The provider, content-edge alignment, subject-writing-mode mapping, and K5a
  grid-area selection commits `0c33e5e0defe`, `c55df1f6cf17`,
  `ae8112f0e93e`, and `6bbbf29694c5` are all ancestors of current main.

## Current-main receipts

- `cargo test --locked --offline -p genet-livery --test grid_abspos`: 2/2.
  The receipts distinguish a grid that is only the formatting parent from a
  grid that is also the K5a containing block.
- Buckram `grid_static_layout` filter: 3/3. It covers content-box selection,
  explicit grid-area selection, and automatic-line padding edges.
- Genet-Livery `grid_static` library filter: 4/4. It covers both K5a branches,
  vertical placement, and subject-writing-mode self alignment.
- `cargo test --locked --offline -p buckram -p genet-livery --all-targets`
  passed, including 236/236 Buckram unit tests and every Genet-Livery target.
- Strict production-library Clippy for Buckram and Genet-Livery passed with
  `--no-deps -D warnings`.
- `git diff --check` passed.

No new WPT comparison is claimed. The native discriminator directly inspects
the two geometry branches named by row 4; the broader K5b and K5e corpus
boundaries remain in their active plans.

## Done condition

Both current `grid_abspos` receipts pass, their selected code path is named,
and its lower and live integration tests pass. Row 4 is closed. The broader
K5b source-context matrix and eventual private-provider replacement remain
open.
