# Livery flex shorthand plan

**Date:** 2026-08-25

**Status:** In progress. The `flex` and `flex-flow` cascade, bounded
specified/computed CSSOM, distinct `flex-basis` specified/computed modeling,
and physical flex main-axis, alignment, and gap projection are complete. Row
18 remains open for generic declaration reflection, flex-basis content used
values and automatic minimums, the remaining vertical-flex work, and grid.

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

At Phase 3 acceptance, three pass-to-fail results were assigned rather than
hidden:

- `flex-minimum-height-flex-items-029.html` begins exercising the existing
  nested column-flex automatic-minimum-size gap once its valid `flex`
  declarations are no longer ignored.
- `flexbox-writing-mode-slr-row-mix.html` uses `flex-flow` in its reference and
  exposes the existing sideways-writing flex-axis transform gap.
- `gap-001-rtl.html` uses `flex: 1 1 auto` in both files and exposes the
  existing RTL flex-gap versus logical-margin equivalence gap.

Those were baseline coincidences caused by dropping the shorthand. They are
downstream layout residuals, not evidence that the expanded longhands differ
from their authored values. Phase 5 repairs the direction and gap residuals;
the automatic-minimum-size residual remains open under its separate owner.

**Done condition:** every directory identity is accounted for, invalid
unitless bases retain their four baseline passes, and every remaining loss has
a named downstream owner.

## Phase 4: specified and computed CSSOM

Reuse Livery's shorthand cascade parser as the grammar authority for
`element.style` and `CSS.supports()`, while requiring that the synthetic parse
produce exactly the expected longhands. Extra declarations, custom
declarations, embedded `!important`, empty values, and parser diagnostics are
invalid at this seam. Canonical specified `flex` values use
grow/shrink/basis order; canonical `flex-flow` values use direction/wrap order
with initial components elided.

Genet-Livery reconstructs computed `flex` and `flex-flow` from their computed
longhands. A unitless zero flex basis serializes as the computed length `0px`,
while the shorthand's omitted basis remains `0%`.

This phase does not implement shorthand-to-longhand reflection in the generic
JavaScript `CSSStyleDeclaration` store. Variable-bearing shorthand values
continue through the existing pass-through path, so `CSS.supports()` for those
values remains assigned to that generic seam rather than counted here.

**Done condition:** native Livery, Genet-Livery, and Genet-Scripted receipts
prove canonical specified values, invalid-value retention, non-variable
`CSS.supports()` classification, inline-to-computed flow, and computed
shorthand reads. The exact flexbox parsing map attributes every movement to the
six named `flex`/`flex-flow` valid, invalid, and computed files with no
pass-to-fail movement.

## Phase 5: physical flex direction and gaps

Project the container's logical `flex-direction` through Buckram `FlowAxes`
before handing the style to Taffy's physical row/column model. Transpose
`row-gap` and `column-gap` into Taffy's physical width/height slots for vertical
flex containers only; grid retains its separate lowering path. Project
explicit `justify-content: start/end` through the same logical main axis while
preserving `normal` as its distinct computed value and flex-start used value.

**Done condition:** a native matrix covers all five supported writing modes in
both directions, unequal gaps prove the physical component mapping, live RTL
and `sideways-lr` geometry passes, both assigned direction/gap WPT residuals
move fail-to-pass, and the full flexbox reftest map has no unexplained loss.

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
- A Turnstone published-source consumer exposed that Genet's root-local Parley
  patch was not inheritable. Current Genet-Livery uses the fork's
  `last_line_alignment` and `TabSize` APIs, but a downstream root silently
  selected crates.io Parley 0.10 and failed before reaching consumer code.
  `support/patches/parley` is now a workspace package so consumers can patch
  crates.io to the Genet git source explicitly.
- The twelve already-published Knot/Livery support commits had remained on the
  clean `release/knot-editor-host-0.1.0` lane. Their version and publishability
  changes are now merged into main, including Livery 0.0.3, Host API 0.1.1,
  Inker 0.1.1, and Nematic 0.1.1.
- That release lane's `genet-taffy 0.13.0` routing was stale against current
  Buckram: the published trait had five parameters while current Buckram's
  static-position implementation requires eight. The corrected fork is now
  published as `genet-taffy 0.13.1`, and Buckram plus Genet-Livery route to that
  package identity while the root patch keeps workspace builds on the same
  source.

### 2026-08-26

- “CSSOM shorthand serialization” spans three independent seams: specified
  admission/canonicalization, computed shorthand reads, and generic
  `CSSStyleDeclaration` shorthand-to-longhand reflection. This continuation
  closes the first two only.
- A declaration-block parser is safe to reuse for one CSSOM value only when
  the caller proves the exact expanded property set. Without that guard,
  `1; color: red` and embedded `!important` were accepted as a `flex` value.
- `flex-flow`'s cascade expander admitted an empty component list as the two
  initial longhands. Empty authored and CSSOM values are invalid and now have
  a native regression receipt.
- Taffy's flex direction is physical while CSS `flex-direction` is logical.
  The missing `FlowAxes` projection jointly owned
  `flexbox-writing-mode-slr-row-mix.html` and `gap-001-rtl.html`; vertical gap
  components must be transposed after the same projection.
- Physicalizing the flex main axis also requires projecting explicit
  `justify-content: start/end`. The first candidate exposed two RTL losses.
  Treating every `start` as authored then exposed that Livery had collapsed
  the CSS initial `normal` value to `start`; retaining `normal` and lowering
  its flex used value to `flex-start` repaired that distinction.
- The surviving `flex-minimum-height-flex-items-029.html` residual is separate:
  vendored Taffy's min-content collection creates one item per line and then
  retains only the longest line. That repair requires its own Taffy release
  lane.
- `flex-basis` now has a distinct specified/computed value model. It admits
  `content` and the intrinsic sizing keywords, rejects width-family `none`,
  resolves definite font- and environment-relative values, reapplies its
  non-negative computed range after deferred `ch` and container-unit
  resolution, and retains its own interpolation and CSSOM serialization. The
  temporary Genet-Livery adapter still lowers intrinsic and `content` bases to
  Taffy `auto`; this is an explicit compatibility boundary, not used-value
  support.
- Two focused parsing residuals remain outside that value model:
  `calc(2em + 3ex)` needs shared `ex` font metrics, while
  `calc(0% + 10px)` needs the generic length-percentage representation to
  retain a syntactic zero-percentage term.
- The prior plan records the final shorthand runner name and hash, but the
  executable was not present in the preserved ledger directory on 2026-08-26.
  The continuation therefore rebuilds and freezes both current-main baseline
  and candidate runners instead of treating the old record as a live artifact.

## Phase 1-3 historical native and frozen receipts

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

## Phase 4-5 native and frozen receipts

The continuation baseline is accepted main at `bad78dda19f`. The accepted
source ends at `838167fc179`, followed only by receipt documentation. Both
source trees use Cargo.lock SHA-256
`9618C76EFD48385C11C36933A04287332E78F80453BFB27A5FB306B2D78D03D6`.

- Final frozen runner: `genet-wpt-final-838167fc179-nodebug.exe`.
- Runner SHA-256:
  `45EC1D090656C6CDD229E07AA258864B3A554D0E8C161EF00A8239C56D0090FC`.
- Manifest SHA-256:
  `D5EC5BE9BF1A75ED00D7E7AB28AFE8A694A55E11682BA74305874D70B18DD422`.
- Livery: 189 passed, zero failed, four ignored.
- Genet-Livery: 223 passed, zero failed.
- Genet-Scripted: 25 passed, zero failed.

| Exact receipt | Baseline | Final | Movement |
|---|---:|---:|---:|
| `css/css-flexbox` reftests | 430 pass / 454 fail / 474 skip / 0 error | 449 pass / 435 fail / 474 skip / 0 error | 19 fail-to-pass, 0 pass-to-fail |
| `css/css-flexbox/parsing` testharness | 68 / 183 subtests | 98 / 183 subtests | 30 fail-to-pass, 0 pass-to-fail |
| `flexbox-writing-mode-slr-row-mix.html` | fail | pass | assigned direction residual retired |
| `gap-001-rtl.html` | fail | pass | assigned gap residual retired |
| `flex-minimum-height-flex-items-029.html` | fail | fail | separate automatic-minimum-size residual unchanged |

The first candidate produced 15 reftest gains but regressed the two RTL
`justify-content: start/end` files. The alignment projection repaired those;
a second focused candidate then exposed the collapsed `normal` initial value.
Both rejected runners and their focused maps remain frozen beside the accepted
artifacts. The final full map has 19 exact gains and no loss. Its SHA-256 is
`E1C7DECAAF59AC0152AB8394B23AFD7A89910ACDD0548DD57CE3758DE4C2C285`;
the final parsing map SHA-256 is
`F197F29880341D72B89A87E1906B2186D1A9902803B0775F38FB31AFBA7E5EEF`.

The reproducible commands, all runner hashes, exact transition identities,
and focused maps are recorded under
`testing/genet/wpt-ledger/2026-08-26_flex_axis_cssom`.

## Remaining Row 18 work

- Carry `auto` and `content` distinctly through Taffy's flex-basis API, skip
  preferred-main-size fallback for `content`, publish the fork, and retire the
  eight focused `flexbox-flex-basis-content-*` layout residuals.
- Add shared `ex` font metrics and generic zero-percentage provenance rather
  than encoding either exception inside `FlexBasis`.
- Implement generic `CSSStyleDeclaration` shorthand-to-longhand reflection for
  `flex` and `flex-flow`, including the variable-bearing `CSS.supports()` seam.
- Repair the automatic-minimum-size residual in vendored Taffy and publish the
  corresponding fork release.
- Validate and repair the remaining vertical-flex cross-axis alignment and
  wrap-reversal surface beyond the main-axis projection.
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
- Published the Parley fork as a git-addressable Genet workspace package and
  proved Genet-Livery from the workspace root before retrying the Turnstone
  consumer.
- Merged the twelve-commit published release history into current main,
  retained its valid package-version changes, and rejected only the stale
  published-Taffy routing after a compiler proof. `cargo check -p genet-livery
  -j 1` passes again on the current fork.

### 2026-08-26

- Published `genet-taffy 0.13.1` from commit `506d84a6c659` and tagged that
  source as `genet-taffy-v0.13.1`.
- Regenerated the complete fork delta against upstream Taffy 0.13.0 and proved
  that it reproduces all 50 vendored source files byte for byte.
- Ran all 131 library tests from the packaged all-features source and all 254
  Buckram tests against the exact release candidate. A clean standalone
  Buckram check then resolved and compiled the published registry crate.
- Split the CSSOM continuation into specified admission, computed reads, and
  still-open generic declaration reflection rather than treating them as one
  surface.
- Added exact-value guards around shorthand canonicalization, rejected empty
  `flex-flow`, and added native specified, computed, inline, and
  `CSS.supports()` receipts.
- Projected flex main directions through `FlowAxes`, transposed vertical flex
  gaps, and added a ten-case axis matrix plus unequal-gap live geometry.
- Projected explicit flex `justify-content: start/end`, restored the distinct
  `normal` initial and computed value, and retained `normal`'s flex-start used
  behavior through physical axis lowering.
- Froze current-main baseline and accepted candidate runners. The exact
  parsing map gains 30 subtests, and the full 1,358-file flexbox map gains 19
  reftests with zero pass-to-fail movement.
- Added a distinct `FlexBasis` type through generated property dispatch,
  cascade, computed-value resolution, animation interpolation, CSSOM, and an
  explicit temporary Taffy adapter. Native Livery, Genet-Livery, and scripted
  Boa receipts are green.
- Reapplied the non-negative computed range at each deferred relative-length
  boundary. A focused native regression retains `calc(10cqw - 1em)` until its
  container is known, then clamps the resolved negative length to zero.
- Rebuilt matched baseline and candidate runners with one ignored root lockfile
  copied byte-for-byte between worktrees. The focused flex-basis surface moves
  from 14 / 27 to 25 / 27; the complete flex parsing map moves from 98 / 183 to
  113 / 183 with 15 exact gains and zero losses.
- Characterized all eight `flexbox-flex-basis-content-001a` through `004b`
  reftests. They remain local failures in both maps, so this slice makes no
  content used-value or layout claim.
