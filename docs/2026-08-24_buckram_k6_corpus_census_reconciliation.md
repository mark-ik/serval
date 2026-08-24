# Buckram K6 corpus census: current-main reconciliation

**Date:** 2026-08-24

**Status:** Complete on current main.

**Parent:** [Buckram and Livery lane program](2026-08-21_buckram_livery_lane_program_plan.md),
row 13, and the [K6 fragmentation execution plan](2026-08-15_buckram_k6_fragmentation_execution_plan.md).

## Ruling

The archived 2026-08-21 census was diagnostically sound but was compiled
under the lane program's uncommitted read-only overlay. Its result maps also
predated the accepted K5 recovery. This reconciliation reruns the census from
accepted current main and carries forward none of that build provenance.

The current result is sharper than the historical one: all 230 passes in
`css/css-multicol` and `css/css-break` are unverified. Neither side consumed
a fragmentation input. Another 20 unverified passes sit in the position,
flex, and grid guard directories. These results receive no K6 capability
credit.

## Method

The frozen current-main runner wrote exact result maps for:

- `css/css-multicol`
- `css/css-break`
- `css/css-position`
- `css/css-tables`
- `css/css-flexbox`
- `css/css-grid`
- `css/css-page`, as an explicit all-skipped print-hosting receipt

The classifier pairs each result with the checked-in manifest and examines
the passing test, its direct references, and linked local stylesheets for
unconsumed fragmentation declarations: column definition and rule
properties, `break-*`, `page-break-*`, `orphans`, `widows`,
`box-decoration-break`, and `@page`. A pass is unverified when the test,
reference, or both use one of those declarations. `column-gap` alone does not
flag a result because it is independently consumed by flex and grid.

## Current absolute inventory

Runnable is pass plus fail. The current runner reports no crash results in
these maps; non-reftest and script-dependent items remain skipped.

| Family | Total | Runnable | Pass | Fail | Skip | Unverified | Verified |
|---|---:|---:|---:|---:|---:|---:|---:|
| `css/css-multicol` | 708 | 397 | 105 | 292 | 311 | 105 | 0 |
| `css/css-break` | 1,170 | 915 | 125 | 790 | 255 | 125 | 0 |
| `css/css-break/table` | 164 | 120 | 15 | 105 | 44 | 15 | 0 |
| `css/css-break/flexbox` | 329 | 290 | 33 | 257 | 39 | 33 | 0 |
| `css/css-break/grid` | 100 | 94 | 6 | 88 | 6 | 6 | 0 |
| `css/css-page` | 278 | 0 | 0 | 0 | 278 | 0 | 0 |
| `css/css-tables` | 328 | 130 | 66 | 64 | 198 | 0 | 66 |
| `css/css-position` | 344 | 118 | 56 | 62 | 226 | 5 | 51 |
| `css/css-flexbox` | 1,358 | 884 | 395 | 489 | 474 | 11 | 384 |
| `css/css-grid` | 1,891 | 1,143 | 438 | 705 | 748 | 4 | 434 |

The subdirectory rows are included in the `css/css-break` total and are not
added again. The direct fragmentation total is therefore 230 unverified
passes, not 230 plus its table, flex, and grid breakdowns.

The 20 unverified guard passes are:

- five in `css/css-position`: four vertical-rl multicol static-position
  cases and `position-absolute-multicol-001`;
- eleven in `css/css-flexbox`: the eight `flexbox-break-request-*` cases,
  `flexbox-with-multi-column-property`, and the two
  `flexbox_columns-flexitems*` cases;
- four in `css/css-grid`: `grid-lanes-gap-002`,
  `column-property-should-not-apply-on-grid-container-001`,
  `grid-inline-multicol-001`, and `grid-multicol-001`.

## Named ratchets

- `multicol-fill-auto-*` is 2 pass / 6 fail. The two passes remain
  unverified; `multicol-fill-auto-001.xht` remains the first red K6c target.
- `multicol-basic-001` through `-004` fail; `-005` through `-008` pass
  unverified.
- `multicol-break-000` and `-001` pass unverified.
- `basic-pagination-001-print.html` skips as `non-reftest`, as do all 278
  `css/css-page` files in the current renderer path.

## Current K5 handoff

The blocker list inherited by the archived census is closed or reassigned on
accepted main:

- K5 positioning closure is recorded at `e8db57141f1` with 26/26 named
  files green and ten directory gains without loss.
- the retained text-frame seam and grid static-rectangle seam are reconciled
  at `7eaaaf724a5` and `67c041d0cda`;
- K5d's eight named sizing/text files and nine logical-inset fixtures are
  green at `ed288ef1c3e`; the 36 red shapes cases belong to lane 12's absent
  `shape-outside` exclusions.

K6a can therefore begin from this frozen current-main baseline. Its input
gate still receives no layout credit, and any source change before K6a starts
requires the six guard maps to be refreshed.

## Continuation contracts

`components/genet-livery/tests/k6_fragmentation_contracts.rs` contains six
ignored tests: structural and retained-session contracts for block, inline,
and table roots in a two-column sequential-fill container. They require one
CSS box across continued fragments, distinct containing column fragments,
one non-initial fragmentation context, a continuation on every fragment but
the last, and correct session geometry, text, and hit testing.

The ignore reasons name the still-absent continuation kernel or fragmented
session consumer. They no longer claim K5h is blocking K6.

## Receipts

- Manifest SHA-256:
  `D5EC5BE9BF1A75ED00D7E7AB28AFE8A694A55E11682BA74305874D70B18DD422`.
- Frozen runner SHA-256:
  `E670FF76C2E392FA5B7C55C11E898427CADD6CA6887976C42E98A57BEBAFE617`.
  A non-document source diff proves current main source-identical to that
  runner's K5 positioning build.
- `cargo test --locked --offline -p genet-livery --test
  k6_fragmentation_contracts -- --list` lists six tests and zero benchmarks.
- Strict no-dependency Clippy for that target is green.
- File-local Rustfmt and `git diff --check` are green.
- Exact maps, classifier, per-file classifications, stdout, lockfile, and
  provenance are under
  `testing/genet/wpt-ledger/2026-08-24_k6_census_v2`.

## Done condition

Row 13 closes when current-main manifest and result inventories exist, every
fragmentation-directory pass is classified, six ignored continuation
contracts compile, and the obsolete K5 blocker list is reconciled. Those
conditions are met without engine code.
