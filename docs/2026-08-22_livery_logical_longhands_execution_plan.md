# Livery logical longhands and the inset shorthand

**Date:** 2026-08-22

**Status:** Implemented and measured. Cut from `recovery/k5-red-receipts` at `62c07e2c134`.

**Parent:** [Buckram CSS layout engine plan](2026-07-26_buckram_css_layout_engine_plan.md),
K5d residual closure.

**Supersedes as a program:** the
[lane program plan](2026-08-21_buckram_livery_lane_program_plan.md) is
historical evidence of a 2026-08-21 over-expansion, not a plan to resume. This
document covers one mechanism only.

## Ruling

Where a correct implementation obviates a repair, do the implementation. The
32 `css-shapes/shape-outside` files that turned red on 2026-08-21 are false
passes exposed, not a regression to revert.

## Diagnosis, measured on the recovery base

The 2026-08-21 ratchet recorded 40 losses. Recomputed from the ledger JSONs
rather than from prose, that is **32 shapes + 8 non-shapes**; the earlier
"33 shapes / seven non-shapes" wording was wrong in both halves.

Measured on `62c07e2c134` with the recovery runner (`runner_sha256
f0a50c7e8de8`), all 8 non-shape residuals and all 32 shape references are
still red, so no archived diagnosis was invalidated by the recovery.

The 32 shape *references* place their marker boxes with logical properties:

| property | uses in the references | catalog state before this change |
|---|---:|---|
| `block-size` | 15 | `[[unimplemented]]` |
| `inset-block-start` | 13 | `[[unimplemented]]` |
| `inset-inline-start` | 13 | `[[unimplemented]]` |
| `margin-block-end` | 1 | `[[unimplemented]]` |
| `inline-size` | 8 | implemented |
| `margin-inline-start` | 2 | implemented |

Every box therefore resolved `auto` insets, stacked at its static position,
and painted as a single solid block — which happened to match an equally
wrong test side, producing the false pass.

## What landed

Catalog data plus one shorthand expansion arm. The generator in
`components/livery/build.rs` already emits the full logical-to-physical
projection from catalog data; 53 logical properties rode it before this
change, so no cascade, computed-value, or layout code was touched.

- **`inset` group created.** `top`, `right`, `bottom`, and `left` gained
  `logical_group = "inset"` and their `physical_side`. The group did not
  exist, which is why no `inset-*` longhand could resolve: the generator
  asserts a logical side has all four physical targets in its group.
- **Seven longhands promoted** from `[[unimplemented]]` to `[[property]]`:
  `inset-block-start`, `inset-block-end`, `inset-inline-start`,
  `inset-inline-end`, `block-size`, `margin-block-start`, `margin-block-end`.
- **The `inset` shorthand implemented**, after measurement showed it was the
  real cause of the `css-sizing` auto-margin family. This one is not catalog
  data alone: shorthand expansion is hand-written per `ShorthandId` in
  `components/livery/src/cascade.rs`, so it needed a match arm beside the
  existing `margin`/`padding` ones as well as the catalog entry.

Whole groups were completed rather than only the four names the references
need. A half-populated logical group is a latent trap — `margin-block-end`
working while `margin-block-start` does not would be worse than neither — and
the border groups already establish the eight-member shape.

`consumed_longhands.toml` is deliberately untouched: it is the F0 audit
contract, it says so in its own header, and logical longhands resolve onto
physical properties that are already in the consumed set.

## Receipts

- `cargo test -p livery`: green, including `catalog_contract`'s
  `generated_logical_groups_project_axes_and_sides` and the
  `source_url` assertion (the new entries use `css_logical_properties_1`).
- `components/genet-livery/tests/logical_insets.rs`, new: 9 green tests, none
  ignored.
  They assert the projection **end to end** — cascade, computed style, used
  geometry — not merely `PropertyId::to_physical`, because a catalog entry
  alone would not catch a cascade that never applied it:
  - `inset-block-start` equals `top` in `horizontal-tb` and actually moves the box;
  - `inset-inline-start` follows `direction` (ltr left edge, rtl right edge);
  - `inset-block-start` follows `writing-mode` (vertical-rl right edge, vertical-lr left);
  - `block-size` is height in `horizontal-tb` and width in `vertical-rl`;
  - `margin-block-end` pushes the following sibling;
  - a later physical declaration beats an earlier logical one and vice versa;
  - the shape-reference shape places three boxes distinctly instead of stacking them;
  - `inset: 0` with `margin: auto` centres for `height`/`block-size` in both
    definite and `max-content` form;
  - the `inset` shorthand expands one, two, and four values, and drops an
    invalid declaration whole rather than applying it partially.
- `cargo test -p buckram`: 223 green. Full `genet-livery --all-targets`: green,
  including `grid_abspos` and the K5h retained-inset receipt, both of which the
  recovery had already closed.
- Clippy adds no warning (the 146 `livery` diagnostics are the pre-existing
  selector and color-space set); rustfmt and `git diff --check` clean.

### WPT

Measured with a runner built from this tree, against the recovery base.

Directory subsets, recovery runner (`f0a50c7e8de8`) versus a runner built
from this tree. Per-file diffs, not just totals.

| directory | recovery base | after | gains | losses |
|---|---:|---:|---:|---:|
| `css/css-position` | 43 / 75 | **47 / 71** | +4 | 0 |
| `css/css-writing-modes` | 262 / 851 | **264 / 849** | +2 | 0 |
| `css/css-sizing` | 213 / 299 | 208 / 304 | +2 | 7 |
| `css/css-shapes/shape-outside` | 60 / 154 | 56 / 158 | 0 | 4 |
| `css/CSS2/tables` | 169 / 93 | 169 / 93 | 0 | 0 |
| `css/CSS2/abspos` | 14 / 10 | 14 / 10 | 0 | 0 |

**+8 gains, 11 false passes converted to honest failures, 0 genuine
regressions.** The gains are all logical-axis positioning: four
`css-position/vrl-*-in-multicol`, two
`css-writing-modes/abs-pos-border-offset-00{1,2}`, and two
`css-sizing/*-block-size-small-or-larger-than-container-*`.

Every remaining loss is a file whose two sides previously agreed *because* a
declaration was dropped, and now disagree because one side is correct. Each
has a named owner outside this change:

- **4 in `css-shapes`** (`circle-054`, `circle-055`, `inset-020`, `inset-021`)
  have references built from the same longhand set as the 32 already-red
  files. That family is now 36 files failing honestly, owned by the absent
  `shape-outside`.
- **6 `css-sizing/div-fit-content-*`** use the bare `fit-content` keyword.
  Livery's `Size` grammar accepts only the `fit-content(<length-percentage>)`
  function form, so the declaration is dropped. Bare `fit-content` is
  `fit-content(stretch)` in CSS Sizing 3 and needs a new value variant in both
  crates plus a CSSOM contract (it must not serialize as `fit-content(100%)`).
  That is CSS Sizing feature work, deliberately not bundled here.
- **1 `css-sizing/intrinsic-percent-replaced-029`** pairs `block-size: 100%`
  with `inline-size: min-content` on a replaced element, so it needs
  percentage block-size resolution against replaced intrinsics.

The `css-sizing` auto-margin family — 8 of the original 15 — is closed by the
`inset` shorthand described next.

That first diagnosis was wrong, and the correction is the substance of this
change. `solve_positioned_box` centres identically for `Length` and
`MaxContent` once the insets are definite -- a direct Buckram probe showed
`inline_start: 150` and margins `150/150` for both. The two cases diverged
only in their *static rectangle* (`inline_start` 150 versus 0), which the box
falls back to when it has no definite insets.

It had none, because the **`inset` shorthand was unimplemented**: `inset: 0`
never reached `top`/`right`/`bottom`/`left`, so every one of these boxes sat
at a static position that legitimately differs between the definite and
intrinsic block-size paths. Implementing the shorthand resolves all four
variants to the same centred result.

- `components/livery/properties.toml`: added `[shorthands.inset]` over the
  four physical longhands and removed the now-contradictory
  `[[unimplemented_shorthand]]` entry, which the generator itself flagged.
- `components/livery/src/cascade.rs`: added the `ShorthandId::Inset` arm to
  `expand_box_shorthand`, reusing the shared one-to-four `box_sides` pattern.

The remaining logical shorthands (`inset-block`, `inset-inline`,
`margin-block`, `margin-inline`) take one or two values rather than one to
four, so they need a different expansion and are deliberately left for a
follow-up rather than bundled in here.

## What this does not fix

The 32 shape files stayed red, and four more joined them, which is the
correct outcome of this change rather than a shortfall. Their *test* sides
need `shape-outside`
— **absent from the catalog entirely**, not merely unimplemented — and
`clip-path`, which is `[[unimplemented]]`. Fixing the references converts the
family from a false pass, through the 2026-08-21 false failure, into an honest
failure with a named cause. Floats and shapes own the remainder.

The other eight residuals are four separate mechanisms and are not touched
here: aspect-ratio transfer (`css-sizing/aspect-ratio/abspos-008`, `-014`),
ruby positioning (three `css-ruby/abs-in-ruby-*`), font-feature resolution
(two files, likely shaping rather than geometry), and fractional static
position (`css-text/white-space/pre-wrap-leading-spaces-014`).

## Stop rules

- Stop if a fix requires editing `consumed_longhands.toml`; that file changes
  only from a re-run of the F0 audit.
- Stop if a logical group would be left half-populated.
- Stop if closing a file requires implementing `shape-outside`; that is a
  separate program, not this one.
- Do not describe a family as closed on a differential; the absolute receipt
  decides.

## Done condition

The four logical longhands the shape references depend on resolve to used
geometry through cascade and layout, with end-to-end receipts; whole groups
are populated; the affected directories show no loss; and the residual
K5d matrix is restated with the shape family attributed to `shape-outside`
rather than to positioning.
