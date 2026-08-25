# Buckram intrinsic sizing: current-main reconciliation

**Date:** 2026-08-24

**Status:** Complete on current main.

**Parent:** [Buckram and Livery lane program](2026-08-21_buckram_livery_lane_program_plan.md),
row 9, and the
[K3 completion execution plan](2026-07-28_buckram_k3_completion_execution_plan.md),
K3m and K3q.

## Ruling

The archived wave-2 row is partly stale and underspecified. It names only
`components/buckram/src/intrinsic.rs` and gives no done condition. Accepted
main already contains the actual intrinsic-contribution implementation from
K3m at `f72d35ff164` and the definite-basis block-axis query from K3q at
`48236d48db0`.

The live implementation necessarily crosses the query contract in
`intrinsic.rs`, subtree contribution and flex/grid query dispatch in
`taffy_adapter.rs`, the shrink-to-fit equation in `block.rs`, and Livery's IFC
measurement callback. Reopening only `intrinsic.rs` would create an unused
parallel API.

This lane therefore verifies the accepted contribution implementation on
current main and closes that inventory row. It does not manufacture source
churn around a live feature.

One adjacent gap remains explicit. Normal-flow used sizing for
`min-content`, `max-content`, and `fit-content()` still reaches
`BlockDeferral::IntrinsicSize` and Taffy's physical block fallback, whose
style bridge represents those keywords as `auto`. That is a used-size and
dispatch-policy gap, not missing contribution infrastructure. It needs its own
post-cutover slice across `block.rs`, `taffy_adapter.rs`, and Livery's live
formatter instead of being hidden inside this row's nominal `intrinsic.rs`
ownership.

## Current contract

- `IntrinsicSizeQuery` distinguishes min-content from max-content by `BoxId`
  and logical axis; one validated pair is cached for both queries.
- Re-entrant queries report an explicit cycle. Indefinite containing sizes and
  fragmentation-dependent block queries remain explicit non-answers.
- An admitted block subtree contributes the maximum outer min-content and
  max-content widths of its in-flow children. Padding, border, and margin are
  included exactly once.
- Inline formatting contexts are measured in min-content and max-content
  modes. Admitted flex and grid subtrees retain their algorithm roles and are
  queried in intrinsic mode rather than read back from final layout.
- Floats and atomic inline blocks use the same CSS shrink-to-fit clamp while
  retaining distinct placement paths.

## Acceptance

- The box-keyed cache, pair validation, cycle behavior, and definite-basis
  block query tests pass on current main.
- Pure and adapter fixtures prove distinct min/max contributions for
  multi-child and block-content subtrees, and widths below, between, and above
  the intrinsic bounds.
- The live Livery fixture proves multi-child floats and atomic inline blocks
  with zero CSS-facing Taffy block fallback.
- The corrected current-main `css/css-sizing` result map is frozen with no
  runner errors. Its remaining failures are not claimed as row-9 regressions
  without a pre-row result movement.
- Buckram and the focused live Livery fixture pass, scoped strict Clippy is
  green, and formatting plus `git diff --check` are clean.

## Current-main receipt

The corrected harness at runner SHA-256
`3CABCBE7C06892FCD1A4DAAA1B9BF45AA44D69BB633964A823023E3DC9636D81`
ran all 732 `css/css-sizing` files: 163 verified pass, 349 fail, 220
skip, and zero error. The exact map and log are outside Git under
`Code/testing/genet/wpt-ledger/2026-08-24_intrinsic_sizing_reconciliation`;
the JSON SHA-256 is
`D87F0F91B0BE81E56B100FE86770C2D91DAF2B649439DE28A2B21D508BACC50B`.

The archived K3m scorer reported 202 pass, 310 fail, and 220 skip for the same
cardinality. Its 39 additional passes cannot be treated as current regressions:
the harness reconciliation subsequently corrected fuzzy scoring, chosen
references, readback synchronization, and false-pass accounting. Row 9 makes
no source change, so the corrected current-main map is the relevant frozen
receipt.

Native verification on the same source:

- the focused intrinsic filter passed 21 tests;
- the complete Buckram suite passed 237 tests and its doc tests;
- `live_multi_child_float_and_atomic_inline_use_intrinsic_subtrees` passed on
  the source-identical Genet-Livery test binary from the fonts lane;
- Genet-Livery has no source delta from that binary's `b9bc926966c` commit to
  this lane's `5080f784faf` base;
- Buckram has no source delta from the paint lane's scoped strict-Clippy source
  at `a0578535718` to this lane's base; and
- `git diff --check` passed for the owned documentation.

## Done condition

Row 9 closes when the accepted K3m/K3q implementation is shown to remain live
on current main through the native fixtures above and one corrected exact
css-sizing map. Used sizing for content keywords, replaced content, orthogonal
flow, fragmentation, and unadmitted flex/grid roles remain separately named
layout-policy work rather than being relabeled as missing intrinsic
contribution infrastructure.
