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

## Boundary

K5d owns absolute and fixed used geometry. K5f owns sticky constraints and
scroll-time movement. Positioned-table absolute, fixed, and sticky entries
remain explicit K5d/K5f gaps; this gate does not reinterpret them as relative
offsets.
