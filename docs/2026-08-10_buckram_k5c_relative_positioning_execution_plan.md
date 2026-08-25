# Buckram K5c: relative positioning and positioned overflow

**Date:** 2026-08-10

**Status:** Accepted. This gate replaces Taffy's relative inset movement for
ordinary generated fragments. K4h's table-part fragment traversal remains its
own proven structural consumer.

**Parent:** [Buckram CSS layout engine plan](2026-07-26_buckram_css_layout_engine_plan.md),
K5c. **Prerequisite:** [K5a containing-block graph](2026-08-10_buckram_k5a_containing_blocks_execution_plan.md).

## Model

Normal-flow layout first creates the unshifted fragments. Buckram then resolves
the retained logical relative inset, maps it through the containing flow, and
translates the selected fragment and all structural descendants in place. The
fragment identifiers, parent links, containing-fragment links, paint source,
and hit-test source stay unchanged.

## Receipt

- `BlockStyle` retains physical inset inputs and resolves the start-wins
  relative offset in logical coordinates.
- `FragmentTree::translate_subtree` owns the physical and per-fragment logical
  translation, while preserving identifiers and propagating the added overflow
  extent through ancestors.
- The Livery adapter gives Taffy auto insets for `position: relative`; Taffy
  supplies only normal-flow geometry.
- The live fixture proves a relative box and child move together while the
  following normal-flow sibling stays in its original position.
- Table row, row-group, and cell offsets continue through K4h's
  `TableFragments::apply_relative_offsets`, which already moves the structural
  part and cell-content geometry as one operation.
- **2026-08-21:** block-axis percentage insets resolve against the containing
  block's specified block size and behave as `auto` when it is indefinite
  (CSS 2.1 §9.3.2), on both routes; the table-part route takes the row's
  specified height for a cell and the table's for a row or group. A
  stretched flex item's cross size and a grid item's area are definite
  (Flexbox §9.8, Grid §6.6), and a percentage block size inherits the
  definiteness of its own basis (CSS 2.1 §10.5). Until then both axes used
  the inline basis, which `position-relative-006` through `-013` falsified,
  and the first correction treated stretched items as indefinite, which
  `css-flexbox/position-relative-percentage-top-002/-003` falsified. Receipts: `relative_block_percentages_need_a_definite_containing_block_size`
  (Buckram), `relative_block_percentage_insets_resolve_only_against_a_specified_height`
  (Livery), and those eight files.
- **Open, 2026-08-21:** a relatively positioned `caption` does not move. It is
  excluded from the generic route with the other internal table roles, and the
  table-part route does not translate it. Pinned by
  `relative_caption_moves_by_its_inset`; `position-relative-table-caption`
  stays red until it is routed.

## Boundary

K5d owns absolute and fixed used geometry. K5f owns sticky constraints and
scroll-time movement. Positioned-table absolute, fixed, and sticky entries
remain explicit K5d/K5f gaps; this gate does not reinterpret them as relative
offsets.
