# Livery flex shorthand plan

**Date:** 2026-08-25

**Status:** Complete for the `flex` and `flex-flow` cascade and live-layout
slice. Row 18 remains in progress for the residual CSSOM, flex sizing, and grid
work named below.

**Parent:** [Buckram and Livery lane program](../docs/2026-08-21_buckram_livery_lane_program_plan.md),
row 18.

## Target

Make Livery expand the two core flex shorthands into the longhand values that
Genet-Livery already lowers into Taffy. This slice does not introduce another
flex model or widen Buckram's block-formatting ownership. The existing
`ComputedValues` fields remain authoritative and Taffy remains the flex layout
algorithm.

## Phase 1: cascade surface

Implement `flex` and `flex-flow` in Livery's generated shorthand catalog and
remove them from the known-unimplemented list.

`flex` expands `none`, factor-only, factor-pair, basis-only, and combined
factor/basis forms. The basis may occur before, between, or after the factor
group when its token is unambiguous. Three entirely numeric tokens retain the
conventional grow/shrink/basis interpretation: a trailing unitless zero is a
valid zero basis, while a non-zero unitless basis is invalid. Omitted values
use the Flexbox shorthand defaults, including `0%` for an omitted basis.

`flex-flow` accepts direction and wrapping in either order and supplies the
longhand initial value for an omitted component. Both shorthands expand CSS-wide
keywords across every longhand and preserve `!important`.

**Done condition:** catalog generation succeeds, native cascade tests cover
keywords, arities, basis order, invalid numeric bases, CSS-wide values, and
importance, and the property-space ledger removes exactly two shorthand gaps.

## Phase 2: live style bridge

Add one Genet-Livery test that resolves authored shorthand declarations, checks
`flex-flow` against an equivalent pair of longhands, and observes Taffy
geometry. A 100px wrapping host produces two 50px items followed by one 100px
item; the equivalent longhand host matches it; and `nowrap` keeps all three
items on one line.

**Done condition:** the focused receipt and the full Genet-Livery library wall
pass without adding a new layout path or a Buckram fallback claim.

## Phase 3: exact WPT ledger

The accepted directory is `css/css-flexbox`, enumerated from the pinned WPT
manifest. The baseline runner is the final horizontal-float runner from source
commit `0af65021c56`. The candidate was rebased across the Fleece 0.4 and Pelt
receipt commits that landed during its release build. A preserved pre-rebase
runner and the post-Fleece pre-fix runner produced status-identical maps across
all 1,358 identities, so those intervening commits contribute zero flexbox
status movement.

| Exact receipt | Baseline | Candidate | Movement |
|---|---:|---:|---:|
| `css/css-flexbox` reftests | 318 pass / 566 fail / 474 skip | 430 pass / 454 fail / 474 skip | 115 fail-to-pass, 3 assigned false-pass losses |
| `css/css-flexbox/parsing` testharness | 68 / 183 subtests | 68 / 183 subtests | status-identical |
| `flexbox-align-items-center-nested-001.html` | fail | pass | one verified gain |
| `flexbox-flex-wrap-horiz-001.html` | fail | pass | one verified gain |

The 115 directory gains divide into 54 `flexbox_flex-*` value-matrix files,
19 named `flex-flow`/wrapping files, and 42 other live flex geometry files.
There is no skip movement. Four initially exposed unitless-basis losses forced
the numeric-token repair before acceptance and pass in the final map.

The three surviving pass-to-fail results are assigned rather than hidden:

- `flex-minimum-height-flex-items-029.html` begins exercising the existing
  nested column-flex automatic-minimum-size gap once its valid `flex`
  declarations are no longer ignored.
- `flexbox-writing-mode-slr-row-mix.html` uses `flex-flow` in its reference and
  exposes the existing sideways-writing flex-axis transform gap.
- `gap-001-rtl.html` uses `flex: 1 1 auto` in both files and exposes the
  existing RTL flex-gap versus logical-margin equivalence gap.

Those were baseline coincidences caused by dropping the shorthand. They are
downstream layout residuals, not evidence that the expanded longhands differ
from their authored values.

**Done condition:** every directory identity is accounted for, invalid
unitless bases retain their four baseline passes, and every remaining loss has
a named downstream owner.

## Findings

### 2026-08-25

- CSS Flexbox's `||` basis ordering cannot be implemented by unconstrained
  token permutation. Ambiguous all-numeric triples use factor/factor/basis
  order; otherwise an interior zero can incorrectly validate `flex: 0 1 4`.
- Livery's declaration cascade and the test runner's CSSOM shorthand
  serialization are separate seams. Layout and reftest gains are live while
  the 27-file parsing-testharness map remains unchanged.
- Correct shorthand admission exposes real downstream flex gaps that an
  ignored declaration could accidentally hide. Those gaps must retain their
  own owners rather than weakening shorthand parsing.
- Current main changed twice during the release build. The first rebase changed
  compiled Fleece inputs but produced a status-identical flexbox map; the
  second reconciliation added only `design_docs`, with no non-document tree
  difference between the built candidate and the final rebased candidate.

## Native and frozen receipts

- Livery: all package tests pass, including 33 cascade tests.
- Genet-Livery: 214 / 214 library tests pass.
- Final source commits: `4d22d7e7d07`, `0902dfd9ccb`, and `7da01dd6293`.
- Frozen runner: `candidate-7da01dd6293-genet-wpt.exe`.
- Runner SHA-256:
  `F3FA180E095DACD37AB043AF69AD339F962970B06DA7B701569802BDB607EE40`.
- Manifest SHA-256:
  `D5EC5BE9BF1A75ED00D7E7AB28AFE8A694A55E11682BA74305874D70B18DD422`.
- Final flexbox map SHA-256:
  `524A0BB1609DF3FA4C75BBC53EB95D4CD04299FE2CE44C313C2B8C2E71FE436E`.
- Final parsing map SHA-256:
  `F0D1B4815A135BBCC0A810C56A697BD21354535B46AC7563F91CD122B42D3D9E`.

The ignored workspace lock was regenerated offline after Fleece's 0.4 version
bump. The accepted release runner then built with `--locked --offline -j 1`.
All frozen artifacts are under
`testing/genet/wpt-ledger/2026-08-25_flex_shorthands`.

## Remaining Row 18 work

- Model `flex-basis: content` distinctly instead of accepting it through the
  generic `Size` type.
- Implement CSSOM shorthand serialization and computed-value receipts for
  `flex` and `flex-flow`.
- Repair the named automatic-minimum-size, sideways-axis, and RTL-gap layout
  residuals.
- Inventory and implement the remaining grid surface, including auto tracks
  and template areas, under its own current-main receipt.

## Progress

### 2026-08-25

- Added and generated the two shorthand catalog entries.
- Implemented bounded, order-aware expansion and CSS-wide reset semantics.
- Added native cascade and live Taffy geometry receipts.
- Repaired the invalid numeric-basis ambiguity exposed by the first exact map.
- Completed the 1,358-file flexbox comparison with 115 gains and three assigned
  false-pass losses.
