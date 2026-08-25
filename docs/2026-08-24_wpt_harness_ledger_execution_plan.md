# WPT harness and ledger: current-main execution plan

**Date:** 2026-08-24

**Status:** Complete at `f9d5174b68d`, based at `b9bc926966c`.

**Parent:** [Buckram and Livery lane program](2026-08-21_buckram_livery_lane_program_plan.md),
rows 19 through 21.

## Ruling

Most of the archived lane brief is already present on accepted main. GPU
readback has waited for the `map_async` callback since `672f766f386`; the
reftest failure ledger has carried an `aa` bucket since `85c5ce40f724`; and
the exact checked-in expectation comparison plus local reftest guard are live
in `ports/genet-wpt/src/main.rs`, `ports/genet-wpt/expectations/reftest/`, and
`support/wpt/check-reftest-baselines.ps1`.

Two live defects remain:

1. `images_match` counts only pixels whose channel delta exceeds
   `maxDifference`. WPT fuzzy metadata has two independent inclusive ranges:
   the largest channel delta over the image and the number of pixels with any
   difference. The current implementation can therefore accept an unlimited
   number of low-delta pixels, discards lower bounds, and ignores exact
   reference keys.
2. Exact result maps record known rendering-coincidence passes as ordinary
   `pass`. The pre-correction K6 census reported 230 such passes in the
   fragmentation directories and 20 in their position, flex, and grid guards.

This lane fixes those two defects and records the already-landed work. It does
not reopen synchronized readback, failure bucketing, or the expectation guard.

## Work

1. Correct fuzzy comparison so the inclusive `maxDifference` and
   `totalPixels` ranges are checked independently, including WPT's zero-value
   exceptions and exact-reference precedence. Clamp authored metadata before
   narrowing it.
2. Add focused tests for exact matching, each fuzzy bound, both bounds
   together, and dimension mismatch.
3. Check in the K6 census's exact Livery reference-verification inventory and
   make the reftest result map emit `pass (reference-unverified)` for a listed
   test only when its pixels otherwise pass.
4. Keep the ordinary pixel-`pass` aggregate for result-format compatibility,
   but print and count the unverified subset separately. Absolute conformance
   reports and deltas must distinguish verified credit from these passes.
5. Diff a frozen current-main CSS result map against the candidate. Every
   pass-to-fail change must be explained by corrected fuzzy semantics; every
   new unverified label must belong to the checked inventory.
6. Refresh the checked reftest baselines affected by the corrected result
   semantics and verify their exact guard.

## Result

The implementation and checked ledgers are accepted at `f9d5174b68d`.

The frozen current-main runner is the final fonts-lane binary, SHA-256
`EAE886FF5CE3EF51B004078700415E26D2D8546F5E36F9CF289A003BF7152725`.
The candidate runner is SHA-256
`3CABCBE7C06892FCD1A4DAAA1B9BF45AA44D69BB633964A823023E3DC9636D81`.
Both ran the same 36,311-file CSS manifest inventory:

| Runner | Pass | Fail | Skip | Error |
|---|---:|---:|---:|---:|
| Current-main baseline | 11,445 | 8,283 | 16,583 | 0 |
| Corrected candidate | 7,814 | 11,914 | 16,583 | 0 |

The exact diff contains 19 gains, 3,650 losses, and 157 reason-only changes.
The gains come from selecting fuzzy metadata for the reference actually used.
Every loss is a prior false pass exposed by checking both independent WPT
fuzzy limits. The old scorer allowed an unlimited number of low-delta pixels;
the corrected scorer requires both maximum channel delta and changed-pixel
count to fit their inclusive ranges. The bounded local GPU jitter floor is a
maximum channel delta of 1 over at most 1% of the raster. Two complete
candidate runs produced identical 36,311-record maps. Renderer source did not
change in this lane.

The K6 refresh now reports 143 unverified direct passes, 60 multicol and 83
css-break, plus 14 unverified guards: one position, eleven flex, and two grid.
The other 93 entries from the old 250-pass census are ordinary failures under
correct fuzzy scoring and receive no pass credit of either kind.

The checked reftest baselines now pin `css/mediaqueries` at 16 pass / 40 fail /
37 skip and `css/css-position` at 45 pass / 73 fail / 226 skip, including the
one `reference-unverified` position pass. Both exact guards report
`unexpected=0` with the frozen candidate runner.

## Acceptance

- Unit tests prove both WPT fuzzy limits independently.
- The checked reference inventory contains the two K6 directory scopes and
  twenty named guard candidates, and rejects duplicate or malformed entries.
- Current K6 results report 143 direct and 14 guard passes as reference
  unverified.
- The full CSS before/after diff has no unexplained status movement: 19
  reference-selection gains and 3,650 corrected-fuzzy false-pass exposures.
- `cargo test -p genet-wpt`, strict no-dependency Clippy, changed-file
  Rustfmt, and `git diff --check` are green.
- The parent row and this plan record the accepted commit and external receipt
  paths before merge.

All executable gates are green: 56 unit tests, strict no-dependency Clippy
with four inherited lint-class allowances, changed-file Rustfmt, Python byte
compilation for the checked diff tool, `git diff --check`, the 49-file
infrastructure reftest with a zero diff, and both checked reftest guards.

Exact runners, result maps, logs, and diffs are under
`testing/genet/wpt-ledger/2026-08-24_harness_ledger`. The refreshed K6
classifier receipt is under
`testing/genet/wpt-ledger/2026-08-24_k6_census_v3`.
