# Buckram K5h: dirty-root relayout

**Date:** 2026-08-11

**Status:** In progress. K5h now has an explicit damage record,
final-document equivalence harness, a narrow paint-only retained path, and a
conservative Buckram fragment-subtree splice. Livery has one true
selected-root formatter for a closed static block/flex/grid case plus a narrow
table-root case, and falls back to a fresh complete layout outside that
boundary.

**Parent:** [Buckram CSS layout engine plan](2026-07-26_buckram_css_layout_engine_plan.md),
K5h.

## Current boundary

`LiveryDocument` records `LayoutDamage` before it discards a retained layout.
DOM mutations map to the nearest existing Buckram formatting context, and
DOM, stylesheet, device, resource, interaction, and viewport events retain
their distinct event class. Overlapping DOM roots coalesce to a disjoint,
outermost set.

The current general geometry-changing path still recomputes full geometry and
reconciles K5g identities. One admitted exception compares the fresh computed
plane with the retained plane and keeps fragments only when every existing
element differs solely in `background-color`. It updates paint directly and
has a fresh-final-document receipt.

A second, geometric admission accepts exactly one existing absolute or fixed
element whose only computed-style differences are its four insets. Livery
reruns Buckram's K5d equation against the retained containing block, then
translates the retained fragment subtree only when the resulting border-box
size is unchanged. It excludes table internals and fragmented roots. The
fresh-document receipt proves the translated paint output and document extent
match a complete final layout. Any changed used size, second style difference,
box-generation difference, text, inherited metric, resource, custom-property,
diagnostic, or paint-order difference remains a full-layout path.

`retained_mutation_paints_like_a_fresh_final_document` compares the paint
commands and document extent after a retained mutation with a fresh document
constructed from the same final DOM. This is the equivalence harness that a
future dirty-root replacement must continue to satisfy.

`background_color_mutation_repaints_without_a_geometry_pass` proves the first
admission keeps box and fragment identities, does not advance the separate
layout-generation counter, and emits the same commands as a fresh final
document.

`positioned_inset_mutation_reuses_a_stable_fragment_subtree` proves a
fixed-size absolute subtree moves through the K5d equation, preserves its and
an unrelated sibling's generated identities, advances the geometry generation,
and emits the same output as a fresh final document.

`positioned_inset_reuse_updates_nested_scroll_range` moves the same admitted
kind of subtree inside an `overflow: auto` container and verifies the retained
scroll range equals a fresh final layout.

`positioned_leaf_geometry_mutation_resizes_the_retained_fragment` changes the
insets and CSS width/height of a positioned canvas. Buckram supplies the new
used rectangle, the retained leaf changes size in place, and the result is
compared with a fresh final document. A root with descendants is intentionally
rejected because its changed containing size requires reformatting.

`positioned_leaf_resize_updates_nested_scroll_range` places the same admitted
leaf in an `overflow: auto` container. Its changed width, height, and inset
update the retained scroll range, which remains equal to a fresh final layout.

`FragmentTree::replace_subtree` now supplies the bounded Buckram-side splice:
it preserves the selected root's `FragmentId`, gives incoming descendants fresh
identities, restores the old root's structural parent and containing fragment,
rebuilds fragment box indices, replaces the selected static-position records,
and recomputes aggregate overflow from each fragment's own extent. It refuses
an incompatible root box, a partially selected fragmented box, and either
incoming or outgoing static-position dependencies that cross the selected
root. This is not a formatter route yet, and it does not publish Livery's
separate table-paint or node side data on its own.

Livery now admits a nonempty disjoint DOM damage set when every selected root
is an unchanged flex or grid box. It preserves each selected root's fragment
ID, replaces its descendants through `FragmentTree::replace_subtree`, then
installs the fresh reconciled box tree with fresh text, table-paint, and
table-shadow planes. That permits added or retired descendant boxes without
invalidating node-to-box lookup, while unrelated fragment identities remain
retained. The update is all-or-nothing: a rejected root discards the candidate
retained publication. The receipts change one and two flex roots' widths and
insert flex and grid children, prove fresh child fragment identities and
preserved outside identities, and compare paint and document extent with a
fresh final document. This is a publication proof only: formatter work still
recomputes the complete document, while ordinary-block/table roots, changed
root display, and cross-root fragment dependencies fall back to the full
replacement path.

`retained_root_splice_keeps_an_unrelated_table_paint_plane_live` adds the
side-plane receipt: a flex-root splice preserves an unrelated table's generated
identities, every fresh table-paint source resolves to a live retained fragment,
and the final paint remains equal to a fresh document.

`layout_retained_formatting_root` is the first actual selected-root formatter.
It regenerates box ownership for the final DOM but builds and computes only one
unchanged, unfragmented block, flex, or grid root, using its retained parent's
content size and translating the accepted local result back to the retained
root origin. The root and its ancestor style inputs must be unchanged. The root and
every descendant must be a static, non-floating element, text, or anonymous
box, with no inline element, table part, or positioned content. The root's used size
must remain unchanged. `TextFrame::replace_subtree_from` replaces prepared
runs, clusters, inline geometry, and text-order entries owned by that DOM
subtree while retaining all outside text-frame data; it preserves global DOM
text order even when the root gains its first text node, and rebuilds used-font
references from surviving prepared runs. Table-paint and
table-shadow planes remain retained. A single DOM root, no container-query
pass, and no active animation are also required.
`retained_root_formatter_adds_its_first_text_source_in_dom_order` and
`retained_root_formatter_reflows_a_text_free_grid_subtree` prove the base
local formatter. The text-bearing flex and grid structural receipts also prove
the same route keeps existing, inserted, and outside find targets live while
matching fresh-final paint and document extent.
`retained_root_formatter_drops_retired_text_sources` proves removed text
cannot retain a shaped run or find target. Tables, inline/atomic content,
positioned descendants, root-size changes, multiple roots, and changed root
style still take the full replacement path.

`retained_root_formatter_updates_fixed_root_overflow` adds the first overflow
receipt: a fixed-height flex root gains a non-shrinking child that extends past
its own box. The local formatter preserves the root and outside identities,
`FragmentTree::replace_subtree` recomputes aggregate overflow through the
retained ancestors, and document extent and paint remain equal to a fresh final
document. A changed used root size still requires parent-flow replacement.

When an unchanged flex or grid root's child-driven used size changes, Livery
promotes the local replacement through each direct DOM parent only while the
formatter proves the selected root's outer size changed. Any other rejected
admission takes the complete-layout path. A root style change retains that
complete-layout publication path. The first stable ancestor must be an admitted
block, flex, or grid formatting context; otherwise the complete-layout fallback
remains authoritative.
`retained_root_formatter_promotes_a_changed_size_to_its_block_parent` proves
that promotion recomputes the changed flex child and following parent sibling,
preserves the parent root identity, retains a sibling outside the parent, and
matches fresh final paint and extent.
`retained_root_formatter_promotes_through_a_changed_parent_to_a_stable_ancestor`
proves a second auto-sized parent can grow before the retained replacement
settles at its stable ancestor.

A static table can now be the local replacement root. Damage to an internal
row, group, or cell maps to the owning table element; the selected fragment
root is its anonymous wrapper, while the element's grid, captions, and table
parts receive fresh identities. The local formatter runs the normal Buckram
column, block, fragment, paint, and verification sequence before publication.
It replaces just the selected table-paint model and per-table shadow-ledger
entry, then rebuilds the existing aggregate ledger from all retained entries.
`retained_root_formatter_replaces_a_fixed_size_table_and_its_paint_plane`
proves a fixed-size table can gain a cell with equal fresh final paint and
extent. `retained_table_root_keeps_an_unrelated_table_paint_plane_live` proves
the two-table case preserves the untouched table's wrapper, fragments, paint
sources, and absolute table-part K5 record. A caption, out-of-flow descendant,
`retained_root_formatter_replaces_a_captioned_fixed_size_table` proves the
same replacement includes a stable caption. An out-of-flow descendant or
nested table inside the selected root still takes the complete path.

`sticky_table_cell_uses_its_nested_scrollport_without_relayout` proves a
sticky table cell uses the retained sticky solver and the table wrapper's
scrollable extent without a relayout. Table row groups and rows, plus absolute
and fixed table parts, remain explicit gaps.

The ordinary block route now keeps absolute and fixed children outside its
normal-flow cursor. Buckram records their static rectangle, formats their
local block subtree, and receives a K5d-resolved inline size for an admitted
second pass; an out-of-flow subtree no longer makes its ordinary block parent
fall back to Taffy. Flex and grid retain the one remaining backend
static-position provider; inline and table-internal absolute/fixed routes
remain separate.
The generic source lowering is deleted, but the scoped flex/grid provider must
be replaced before Taffy's position role can vanish entirely.

## Next replacement seam

1. Compare each incremental result with the fresh-final-document harness.
2. Move flex, grid, inline, and table-internal out-of-flow participation to
   equivalent Buckram-owned routes before deleting the remaining flex/grid
   backend position provider.

## Stop rules

- Do not use `RestyleStats` as evidence of incremental geometry.
- Do not retain stale table paint or static-position source records.
- Do not admit an inset mutation that changes its used border-box size unless
  it is the documented descendant-free leaf resize; every other case requires
  constrained reformatting rather than translation.
- Do not resize a retained root with descendants; its children must receive a
  fresh containing size through the later formatting-root replacement route.
- Do not call K5h complete until a real root is replaced and compared against
  fresh geometry across mutation, style, resource, interaction, and resize
  cases.
