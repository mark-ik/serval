# Buckram K5b: static-position rectangles

**Date:** 2026-08-10

**Status:** Accepted. K5a supplies the containing-block graph; K5b supplies
the distinct static rectangle and source fragment that K5d will consume.

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
- No source cache or post-Taffy query is used to reconstruct static positions.
- Paint, hit test, and accessibility continue to read ordinary fragments;
  K5d will turn this record into final positioned fragment geometry.

## Current implementation boundary

The first K5b patch establishes `StaticPosition` and
`StaticPositionSource` in Buckram, and records the selected K5a containing
block beside the formatting fragment that emitted an absolute or fixed box.
Block, inline, atomic-inline, and table structural emission paths all use the
same record API.

Buckram's private algorithm adapter now carries a formatter-provided
pre-inset location separately from final layout. Block, flex, and grid routes
publish that location before an explicit inset changes final placement. The
flex receipt uses center/end alignment plus explicit insets, proving the
record preserves the aligned static coordinate rather than copying the final
inset coordinate. The retained inline formatter emits an inline-origin
positioned child against its owning line fragment, and K4h table structural
fragments emit wrapper and part records from their own logical rectangles.

K5b does not calculate final absolute or fixed geometry. In particular, the
existing positioned-table geometry gap remains named until K5d consumes these
records through the shared used-geometry path.

## Stop rules

- Do not calculate final absolute or fixed used sizes, margins, or insets.
  That is K5d.
- Do not move a fragment, change overflow, or alter scroll behavior. That is
  K5c, K5e, and K5f.
- Do not mutate the table algorithm or create a table-only coordinate model.
- Do not retain or replace a whole layout pass. That is K5g and K5h.
