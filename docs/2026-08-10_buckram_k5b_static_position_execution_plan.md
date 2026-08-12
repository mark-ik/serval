# Buckram K5b: static-position rectangles

**Date:** 2026-08-10

**Status:** Partially implemented. K5a supplies the containing-block graph;
K5b records the source fragment and static rectangle for block, inline, and
table paths. Direct flex static rectangles are live. For a direct grid child,
K5b now uses the grid content rectangle by default and switches to the
finalized grid area only when K5a selected that same grid as the actual
containing block. Table, fragmentation, and unadmitted formatting-context
rules remain open.

**Parent:** [Buckram CSS layout engine plan](2026-07-26_buckram_css_layout_engine_plan.md),
K5b. **Prerequisite:** [K5a containing-block graph](2026-08-10_buckram_k5a_containing_blocks_execution_plan.md).

## Question

An out-of-flow box can be placed against a containing block different from its
formatting-context parent, but an `auto` inset still needs the position it
would have occupied in that parent. The current tree has neither a static
rectangle nor the coordinate space that produced it. Taffy's completed
absolute layout cannot supply either: it has already selected a backend parent
and its `Position` lowering has lost the source formatting context.

## Model

Buckram adds a positioning record keyed by `BoxId` with:

- a logical static-position rectangle;
- the `FragmentId` of the formatting-context coordinate space that produced
  it; and
- the selected absolute or fixed containing-block relationship from K5a.

The record is produced while each formatting context places its normal-flow
participants. It is not inferred afterwards from a finished backend layout.
An out-of-flow box receives a placeholder only for this accounting purpose;
the placeholder must not contribute to normal-flow size, float state, or
paint.

## Work

1. Add the Buckram record and checked lookup to the layout result. Its box and
   fragment references remain independent identities, not vector indices.
2. Teach block flow to publish the border-start static rectangle and its
   available inline-size context for each out-of-flow child.
3. Teach the retained inline formatter to publish a line-level rectangle for
   an inline-origin out-of-flow child. A leaf approximation is not accepted.
4. Have flex and grid publish their item-area rectangles from their own
   placement outputs without rerunning a parent algorithm.
5. Consume the K4h wrapper/grid/internal table fragments for table-root and
   table-part static rectangles. Do not restore the deleted positioned-row
   side list.
6. Add one structural fixture per source formatting context, including an
   out-of-flow box whose static source and selected containing block differ.

## Acceptance

- Every supported out-of-flow source has a Buckram static-position record
  before K5d resolves insets.
- The record identifies its source fragment even when the absolute or fixed
  containing block is elsewhere in the tree.
- Inline-source, block-source, flex-source, grid-source, table-source, and
  table-part fixtures cover both the record and its coordinate-space identity.
- No completed backend child location is relabeled as a Buckram static
  rectangle.
- Paint, hit test, and accessibility continue to read ordinary fragments;
  K5d will turn this record into final positioned fragment geometry.

## Current implementation boundary

The first K5b patch establishes `StaticPosition` and
`StaticPositionSource` in Buckram, and records the selected K5a containing
block beside the formatting fragment that emitted an absolute or fixed box.
Block, inline, atomic-inline, and table structural emission paths all use the
same record API.

Buckram's private algorithm adapter carries a formatter-provided pre-inset
location separately from final layout. It remains the temporary direct
flex/grid input while the actual rectangle model is completed. It must not be
credited as a Buckram-owned rectangle: the adapter currently transports a
point and measured child size, while the specifications require a rectangle
and alignment-sensitive resolution.

The 2026-08-12 WPT comparison invalidated an attempted post-layout rewrite of
that point. CSS Flexbox defines cross-axis static-rectangle edges as the flex
container's content edges and defines the main-axis edges using the child as a
sole fixed-size flex item with automatic margins treated as zero. CSS Grid
uses the grid parent's content edges unless that grid also establishes the
actual containing block, in which case the placement area applies. The old
rewrite could not see the K5a containing-block relationship and substituted
one scalar coordinate for those distinct rectangles.

The direct-grid carrier now preserves the exact rectangle Taffy constructed
after grid-line resolution, implicit tracks, direction, borders, scrollbars,
and track alignment. It is stored as `StaticPosition::containing_block_area`
in the grid source fragment's logical coordinates. K5d replaces its ordinary
padding rectangle with that area only when the source grid fragment is also
the K5a-selected containing block. It does not reuse Taffy's final child
location, recompute placement in Buckram, or affect a grid that was merely the
formatting parent.

The direct-grid formatter now receives both candidates before it writes its
pre-inset location: its finalized grid area and its content rectangle. A
backwards-compatible `LayoutGridContainer` callback lets Buckram select the
latter unless the K5a graph selected that grid as the child's containing
block. This preserves Taffy's alignment, direction, automatic-margin, and
track-resolution behavior while making the CSS Positioned Layout condition
explicit. `grid_abspos` covers the asymmetric-padding discriminator on both
sides of that condition.

The retained inline formatter emits an inline-origin positioned child against
its owning line fragment, and K4h table structural fragments emit wrapper and
part records from their own logical rectangles. The remaining K5b work is the
unadmitted source-context matrix, not a second grid-area implementation.

K5b does not calculate final absolute or fixed geometry. K5d now consumes the
wrapper, caption, row-group, row, and cell records through the shared
used-geometry path; row-group, row, and cell records use a zero-track anchor
and a post-track local formatting pass so they cannot contribute to table
sizing.

## Stop rules

- Do not calculate final absolute or fixed used sizes, margins, or insets.
  That is K5d.
- Do not move a fragment, change overflow, or alter scroll behavior. That is
  K5c, K5e, and K5f.
- Do not mutate the table algorithm or create a table-only coordinate model.
- Do not retain or replace a whole layout pass. That is K5g and K5h.
