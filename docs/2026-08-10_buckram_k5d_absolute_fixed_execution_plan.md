# Buckram K5d: absolute and fixed used geometry

**Date:** 2026-08-10

**Status:** In progress. K5a and K5b are committed. The first live route owns
ordinary non-table placement across the implemented writing modes; the
remaining sizing and table work stays explicit below.

**Parent:** [Buckram CSS layout engine plan](2026-07-26_buckram_css_layout_engine_plan.md),
K5d.

## Scope

K5d turns a K5b static rectangle plus K5a containing-block relationship into
final absolute or fixed fragment geometry. It owns physical-to-logical
conversion, inset resolution, auto margins, min/max constraints,
shrink-to-fit contributions, percentage bases, and the CSS over-constraint
rules for the implemented non-replaced and replaced/aspect-ratio subset.

Its closure replaces Livery's current temporary lowering of both `absolute`
and `fixed` to Taffy's `Position::Absolute`.

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
rectangle, admitted intrinsic inline contributions, measured border-box
fallback, auto margins, and min/max bounds. An admitted horizontal block
formatting root gets its min-content and max-content pair through Buckram's
formatter query, never by reading a completed normal-flow rectangle. For that
subset, an automatic inline size shrink-wraps when an inset is automatic and
fills the remaining inline space when both insets are definite.

When this query changes an admitted root's used inline size, Livery feeds that
CSS width back to the formatter, clears its scratch cache, and collects a
second content pass before final positioning. Wrapping and block-size output
therefore follow Buckram's used inline size instead of retaining the first
auto-width fragment.

For a positioned replaced image or canvas leaf, Livery separately passes HTML
width/height hints and decoded intrinsic dimensions to Buckram. The solver
keeps a replaced automatic width at its intrinsic size even when both inline
insets are definite, and derives the automatic opposite axis from the usable
intrinsic or CSS ratio. Its final border-box size replaces the retained leaf
fragment's size before the K5a translation. This is intentionally only the
leaf subset: it does not make normal-flow replaced sizing or replaced
subtrees a Buckram formatting context.

Livery gives the scratch formatter auto insets for absolute and fixed boxes,
then translates the emitted fragment subtree from this Buckram result and
rewires its containing-fragment link to K5a's selection. The bridge converts
the static source rectangle through physical space into the selected containing
flow, so vertical writing modes keep the same logical used-inset equation.
The same path now handles a table root at its K4h wrapper; its grid no longer
carries a duplicate root-only absolute/fixed gap. The fixed receipt uses a
transform-established fixed containing block.

Before that conversion, a selected non-inline containing fragment now changes
from its emitted border rectangle to its CSS positioning padding rectangle.
The bridge subtracts only the resolved physical border widths: percentages and
auto insets therefore use the padding-box size, while the padding itself stays
inside the positioning rectangle. `positioned_child_uses_the_positioned_ancestor_padding_box`
pins asymmetric border edges and a 100% absolute child. This is the ordinary
containing-block rule only. An inline establishing ancestor still needs its
multi-fragment content-edge rectangle, and a grid establishing the selected
containing block still needs to replace the padding rectangle with the
specified grid area before K5d receives it.

Taffy still excludes the out-of-flow box, and remains the measured fallback
for unadmitted descendants, the non-replaced block axis, flex/grid roots, and
replaced non-leaves or missing contributions. The retained inline formatter
now omits each outermost absolute or fixed descendant from its enclosing line,
then gives that descendant a separate local block-formatting root. Its
structural fragment parent remains the enclosing inline line, so K5b keeps the
line-level static source while K5d queries intrinsic sizes, returns the used
inline size, and triggers the local reformat pass. The absolute and fixed
automatic-width receipts both prove wrapping follows that returned width.
Table wrappers, captions, row groups, rows, and cells now supply their K5b
records to the shared K5d path; detached row-group, row, and cell subtrees are
formatted only after K4d track work. K5d remains open until the remaining
routes and out-of-flow participation itself are Buckram-owned. Those gaps are
not treated as final K5 behavior.

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
- A non-inline positioned ancestor with asymmetric borders supplies a
  padding-box percentage basis and padding-edge origin.
- Positioned tables and table parts use the wrapper/internal K5b records and
  produce ordinary Buckram fragments.
- No Taffy `Position` value selects browser absolute or fixed behavior.

## Stop rules

- Do not turn sticky into fixed or relative. K5f owns sticky constraints.
- Do not add paged-media replication or positioned fragmentation. K6 owns it.
- Do not implement general normal-flow aspect-ratio/replaced sizing. K7 owns
  that wider capability.
- Do not make a missing trigger fall back to the nearest positioned ancestor.
