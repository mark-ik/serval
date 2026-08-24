# K5 positioning closure execution plan

**Date:** 2026-08-24

**Status:** Complete from accepted `origin/main`
`b860e34ca84474df6acc883564efcb7498efcab4`, then rebased without source
overlap and revalidated on accepted `origin/main`
`d650ff8b44d3dc306255d2313be635000b9962ea`.

**Parent:** [Buckram and Livery lane program](2026-08-21_buckram_livery_lane_program_plan.md),
row 1+2+7.

## Purpose

Re-measure the historical K5 positioning residuals on current main, then close
each current failure through the existing Buckram positioning model or name its
actual owner. Archived lane receipts and dirty overlays are evidence only.

## Provenance

- Worktree: `worktrees/genet-k5-positioning-closure-v2`.
- Isolated target: `C:\t\genet-k5-positioning-closure-v2-target`.
- External ledger:
  `testing/genet/wpt-ledger/2026-08-24_k5_positioning_closure_v2`.
- `Cargo.lock` is intentionally ignored by this repository. The clean-tree
  offline resolution used for the locked baseline has SHA-256
  `358CF8F93C6D888D016F96B4FF45AFF92F759F3B4B2789A56F924AD9FEC82EDC`.
- The accepted integration tip added a workspace member. Its regenerated
  offline lockfile has SHA-256
  `6F91D0C46D3BF171137B16023C04C5A8032B405CD10D321E6D41459CAD4EBE49`.
- Every accepted runner is copied to the ledger and identified by SHA-256
  before measurement.

## Named inventory

- Every test file matching `css/css-position/position-relative-table-*`, with
  references excluded.
- `css/css-position/position-relative-table-caption.html`.
- `css/css-position/position-absolute-in-inline-margin-top.html`.
- `css/CSS2/abspos/static-inside-inline-block.html`.
- `css/CSS2/abspos/abspos-containing-block-initial-009e.xht`.
- `css/CSS2/tables/fixed-table-layout-017.xht` through `-020.xht`.
- The current ignored or active receipts in
  `components/genet-livery/tests/positioned_block_children.rs`.

## Ownership

The current-main failures proved three lower seams through focused native
receipts: positioned block-size resolution, block clearance with negative
margins, and Taffy scratch measurement of renderer-owned out-of-flow children.
The completed ownership therefore includes the causal regions of
`components/buckram/src/positioning.rs`, `components/buckram/src/block.rs`, and
`components/buckram/src/taffy_adapter.rs`, together with the K5 positioning
regions of `components/genet-livery/src/layout.rs`, focused Genet-Livery
positioning tests, this execution plan, and the parent lane ledger.

## Work

1. Freeze and hash the current-main release runner.
2. Run every named file and the current native positioning receipts.
3. For each failure, compare the test and reference geometry and identify the
   live static source, containing fragment, used inset, and translated text or
   descendant subtree.
4. Repair only causal K5 positioning defects. Keep specification or reference
   disagreements explicit.
5. Build a candidate runner and compare full `css/css-position`,
   `css/CSS2/abspos`, and `css/CSS2/tables` status maps against the frozen
   baseline.
6. Run affected-crate all-target tests, scoped Clippy, formatting, and
   `git diff --check`.

## Results

- Frozen baseline runner SHA-256:
  `C8C80BF4EF0B08217B661EF51AEB756A530AE8B8A722026B3260CF395281310C`.
- Candidate runner SHA-256:
  `E670FF76C2E392FA5B7C55C11E898427CADD6CA6887976C42E98A57BEBAFE617`.
- The named inventory moved from 16 pass / 10 fail to 26 pass / 0 fail.
- Full `css/css-position` comparison: 9 gains, 0 losses.
- Full `css/CSS2/abspos` comparison: 1 gain, 0 losses.
- Full `css/CSS2/tables` comparison: byte-identical status map.
- After rebasing, all 26 named files remained green and all 623 runnable
  per-file directory statuses were identical to the pre-rebase candidate.
- The gains cover out-of-flow table-part sizing and baselines, relative
  table-part border-grid geometry, the inline static-position margin case,
  and initial-containing-block percentage block sizing.
- `cargo test --locked --offline -p buckram -p genet-livery --all-targets`
  passed, including 236 Buckram unit tests and all Genet-Livery targets.
- Strict production-library Clippy and the changed positioned-block integration
  target passed. The broad all-target command remains blocked by existing
  `derivable_impls` in `genet-host-api`, two `useless_format` cases already in
  `positioned_block_children.rs`, and `field_reassign_with_default` in
  `table_sizing.rs`.
- `git diff --check` passed.

## Stop rules

- Stop on any unexplained pass-to-fail result.
- Do not restore Taffy as the source of browser static-position semantics.
- Do not relabel a completed backend rectangle as a Buckram static rectangle.
- Do not change reftest tolerance to close a geometry defect.
- Keep flex/grid provider replacement, general K5d sizing, sticky, and
  fragmentation in their owning rows unless a focused receipt proves this
  lane must cross that boundary.

## Done condition

Every named current-main file is green or has an executable receipt and a
named owner outside this lane; positioned text and descendant subtrees move
with their fragments; the three full directory comparisons have no
unexplained losses; affected native gates pass; and the result is integrated
into accepted main from the isolated worktree.
