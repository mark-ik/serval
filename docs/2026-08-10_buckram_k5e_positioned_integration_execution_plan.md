# Buckram K5e: positioned-context integration

**Date:** 2026-08-10

**Status:** In progress. This records the first live cross-context receipts;
it does not close K5e or K5.

**Parent:** [Buckram CSS layout engine plan](2026-07-26_buckram_css_layout_engine_plan.md),
K5e.

## Current route

The formatter publishes a K5b static rectangle at the context that formed it.
Livery then passes the retained record to Buckram's absolute/fixed equation
without rerunning the flex, grid, inline, or table-root parent formatter.

The live receipts currently cover:

- flex alignment and grid-area static rectangles surviving the later Buckram
  inset equation;
- a table root using its wrapper's shared K5d route;
- positioned descendants contributing their final fragment geometry to a
  nested scroll container's scrollable overflow; and
- document paint and hit testing consuming the same retained fragment plane;
- numeric positioned stacking levels bracketing the normal paint phase; and
- a flattened positioned stacking item retaining its ancestor overflow clip.

## Evidence

- `absolute_flex_and_grid_children_keep_their_native_static_rectangles`
  verifies flex and grid static rectangles and final inset geometry.
- `absolute_table_root_uses_shared_k5d_wrapper_geometry` verifies the table
  root no longer carries a duplicate table-positioning gap.
- `absolute_table_track_parts_use_zero_track_static_anchors` and
  `fixed_table_track_parts_use_zero_track_static_anchors` verify row groups,
  rows, and cells leave K4b/K4d track topology, retain a zero-track static
  anchor, and resolve through the same K5d geometry path without widening the
  in-flow grid.
- `inline_origin_absolute_position_uses_the_line_fragment_as_its_static_source`,
  `inline_origin_absolute_auto_width_refits_to_the_k5d_inline_size`, and
  `inline_origin_fixed_auto_width_refits_to_the_k5d_inline_size` verify an
  inline-origin root keeps its line fragment for K5b and, for both absolute
  and fixed automatic widths, reforms its text at the K5d-used inline size.
- `positioned_descendant_extends_its_scroll_container_range` verifies an
  absolute descendant produces nested scroll range from its final fragment.
- `positioned_numeric_z_indices_wrap_the_normal_paint_phase` verifies the
  negative, normal, and positive paint phases share the positioned fragment
  plane in that order.
- `positioned_stacking_item_keeps_its_overflow_clip` verifies a positive
  positioned descendant retains the `overflow` clip that would otherwise be
  lost when its stacking context is flattened.
- `positioned_hit_test_respects_stacking_level_and_ancestor_clip` verifies
  the retained hit walk picks the same positive stacking item and excludes it
  outside its ancestor overflow clip.

## Remaining boundary

The supported table parts, including row groups, rows, and cells, no longer
appear in `TableShadowLedger::positioning_gaps`: their explicit out-of-flow
route formats the detached structural subtree after table tracks settle. Full
CSS stacking-context ordering and clipping remain a separate K5e matrix; the
current receipts cover only numeric positioned levels and the supported
`overflow` shorthand and longhands. Generic inline automatic-width roots now
have a distinct formatter root for the admitted horizontal absolute/fixed
subset; unsupported writing modes and unadmitted descendants remain explicit
fallbacks. These are open work, not fallback-free behavior.

## Stop rules

- Do not infer table-internal absolute or sticky behavior by merely
  translating an already in-flow table fragment.
- Do not claim positioned overflow is complete beyond the retained nested
  scroll container receipt.
- Keep fragmentainer integration in K6.
