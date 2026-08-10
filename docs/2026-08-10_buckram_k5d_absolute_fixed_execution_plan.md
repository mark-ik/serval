# Buckram K5d: absolute and fixed used geometry

**Date:** 2026-08-10

**Status:** In progress. K5a and K5b are committed. The first live route owns
ordinary horizontal non-table placement; the remaining sizing and table work
stays explicit below.

**Parent:** [Buckram CSS layout engine plan](2026-07-26_buckram_css_layout_engine_plan.md),
K5d.

## Scope

K5d turns a K5b static rectangle plus K5a containing-block relationship into
final absolute or fixed fragment geometry. It owns physical-to-logical
conversion, inset resolution, auto margins, min/max constraints,
shrink-to-fit contributions, percentage bases, and the CSS over-constraint
rules for the implemented non-replaced and replaced/aspect-ratio subset.

It replaces Livery's current lowering of both `absolute` and `fixed` to
Taffy's `Position::Absolute`.

## Data boundary

`components/buckram/src/positioning.rs` will accept CSS-facing `BlockStyle`
inputs, a resolved K5a containing rectangle, a K5b static rectangle, and
Buckram intrinsic/replaced contributions. It will return logical used geometry
and overflow inputs for the fragment owner. It must not accept a Taffy style,
node, layout result, or parent index.

The initial absolute and initial fixed containing rectangles remain separate
inputs even when continuous media makes their initial geometry equal. A later
fixed-position trigger can therefore change only the fixed chain without
silently changing an absolute result.

## Current implementation boundary

`buckram::solve_positioned_box` now resolves the implemented non-replaced
logical inset equation from a selected containing rectangle, K5b static
rectangle, measured border-box fallback, auto margins, and min/max bounds.
Livery gives the scratch formatter auto insets for absolute and fixed boxes,
then translates the emitted ordinary horizontal fragment subtree from this
Buckram result and rewires its containing-fragment link to K5a's selection.
The fixed receipt uses a transform-established fixed containing block.

The formatter still excludes the out-of-flow box and supplies its measured
fallback size. K5d remains open until Buckram owns shrink-to-fit, replaced and
aspect-ratio sizing, vertical writing modes, and the table wrapper/internal
part route. Those gaps are not treated as final K5 behavior.

## Work

1. Add logical inset and automatic-margin resolution to `BlockStyle`'s
   positioning inputs, preserving percentages until the selected containing
   rectangle is known.
2. Implement the non-replaced block equation, including the static rectangle
   for `auto` inset sides, the shrink-to-fit width branch, and deterministic
   over-constraint direction.
3. Implement only the replaced/aspect-ratio cases necessary for absolute and
   fixed geometry. General normal-flow replaced sizing remains K7.
4. Make positioned descendants leave normal-flow sizing while their final
   fragments attach to the K5a containing fragment.
5. Route table roots and internal table parts through the same input and
   output path, using K5b wrapper/internal static records.
6. Delete the Taffy absolute/fixed position lowering and add a source audit.

## Acceptance

- Absolute and fixed fixtures differ when their selected containing blocks
  differ, including a transform or containment fixed trigger.
- `left/right/width`, `top/bottom/height`, percentage, auto-margin,
  shrink-to-fit, min/max, replaced, and aspect-ratio receipts are explicit.
- Direction and every supported writing mode keep the same logical equation.
- Positioned tables and table parts use the wrapper/internal K5b records and
  produce ordinary Buckram fragments.
- No Taffy `Position` value selects browser absolute or fixed behavior.

## Stop rules

- Do not turn sticky into fixed or relative. K5f owns sticky constraints.
- Do not add paged-media replication or positioned fragmentation. K6 owns it.
- Do not implement general normal-flow aspect-ratio/replaced sizing. K7 owns
  that wider capability.
- Do not make a missing trigger fall back to the nearest positioned ancestor.
