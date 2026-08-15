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

- flex alignment and grid-content static rectangles surviving the later Buckram
  inset equation, while a placed grid area remains the containing block for
  positioned insets;
- same-flow `self-end` grid self-alignment reaching the formatter instead of
  being discarded as an invalid value;
- a table root using its wrapper's shared K5d route;
- positioned descendants contributing their final fragment geometry to a
  nested scroll container's scrollable overflow; and
- document paint and hit testing consuming the same retained fragment plane;
- numeric positioned and direct static flex/grid item stacking levels bracketing
  the normal paint phase; and
- direct flex/grid item paint and hit traversal using their order-modified
  document order; and
- a flattened positioned stacking item retaining its ancestor overflow clip.

## Evidence

- `absolute_flex_and_grid_children_keep_their_native_static_rectangles`
  verifies flex and grid static rectangles and final inset geometry.
- `absolute_grid_self_end_uses_the_grid_content_end` verifies that a same-flow
  `align-self: self-end` absolute grid child reaches the grid content end
  instead of falling back to the static start edge.
- `absolute_grid_static_position_uses_content_edges_not_placed_area` keeps the
  static rectangle separate from the placed grid-area containing block, as
  [CSS Grid 9.1](https://drafts.csswg.org/css-grid-1/#abspos) and
  [9.2](https://drafts.csswg.org/css-grid-1/#static-position) require.
- `vertical_grid_static_alignment_uses_the_content_block_end` is the first
  current-spec same-writing-mode vertical receipt: `align-self: end` uses the
  grid content's physical left edge in vertical-rl and physical right edge in
  vertical-lr.
- The legacy WPT
  `grid-abspos-staticpos-align-self-vertWM-001.html` reference instead places
  the static rectangle in the assigned grid area. The current-source runner
  fails that reference. It remains useful historical interoperability
  diagnostics, but conflicts with the current Grid 9.2 content-edge rule and
  is not a current-spec acceptance receipt.
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
- `static_grid_item_z_indices_wrap_the_normal_paint_phase` and
  `static_flex_item_z_indices_wrap_the_normal_paint_phase` verify direct
  static items with numeric levels establish the same negative, normal, and
  positive phases. `static_block_z_index_keeps_normal_hit_order` keeps that
  admission from leaking to ordinary static blocks.
- `grid_items_paint_in_order_modified_document_order`,
  `flex_items_paint_in_order_modified_document_order`, and
  `grid_item_order_changes_the_topmost_hit_target` verify direct same-level
  flex/grid items use order-modified document order in both paint and hit
  traversal.
- `positioned_stacking_item_keeps_its_overflow_clip` verifies a positive
  positioned descendant retains the `overflow` clip that would otherwise be
  lost when its stacking context is flattened.
- `positioned_hit_test_respects_stacking_level_and_ancestor_clip` verifies
  the retained hit walk picks the same positive stacking item and excludes it
  outside its ancestor overflow clip.
- `grid_static_self_alignment_uses_the_subject_writing_mode` verifies direct
  positioned grid children resolve `self-start`/`self-end` from the subject's
  writing mode on both grid axes, including orthogonal and RTL vertical flows.
- `positioned_grid_area_transforms_from_flow_relative_tracks_to_physical_insets`
  verifies direct positioned grid children project their flow-relative
  placement area before physical `top`/`right`/`bottom`/`left` insets resolve
  against definite used dimensions, across vertical rl/lr and ltr/rtl flows.
- `absolute_nonleaf_reformats_at_buckrams_resolved_inline_size` verifies an
  admitted horizontal absolute block root and its child reformat at Buckram's
  final used width before their final offset is published.

## Remaining boundary

The supported table parts, including row groups, rows, and cells, no longer
appear in `TableShadowLedger::positioning_gaps`: their explicit out-of-flow
route formats the detached structural subtree after table tracks settle. Full
CSS stacking-context ordering and clipping remain a separate K5e matrix; the
current receipts cover only numeric positioned and direct static flex/grid item
levels, direct flex/grid DOM-item order, and the supported `overflow` shorthand
and longhands. Flattened or generated flex/grid item ordering remains part of
that matrix. Generic inline automatic-width roots now
have a distinct formatter root for the admitted horizontal absolute/fixed
subset; direct grid static alignment admits `self-start`/`self-end` across
writing modes, using the positioned subject's corresponding physical edge.
Positioned subtrees without an admitted intrinsic input remain explicit
fallbacks.
The renderer still supplies the narrow flex/grid static-position
provider, so its private position role cannot disappear until Buckram has an
equivalent flex/grid static-position algorithm. These are open work, not
fallback-free behavior.

## Stop rules

- Do not infer table-internal absolute or sticky behavior by merely
  translating an already in-flow table fragment.
- Do not claim positioned overflow is complete beyond the retained nested
  scroll container receipt.
- Keep fragmentainer integration in K6.
