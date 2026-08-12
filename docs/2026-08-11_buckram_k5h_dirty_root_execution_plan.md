# Buckram K5h: dirty-root relayout

**Date:** 2026-08-11

**Status:** In progress. K5h now has an explicit damage record,
final-document equivalence harness, and one narrow paint-only retained path.
It has not replaced a formatting-context subtree yet.

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

## Next replacement seam

1. Make Buckram replace the selected formatting-context fragment subtree,
   including parent, containing-fragment, by-box, by-node, static-position,
   table-paint, and overflow indices.
2. Recompute a selected root against its retained containing inputs.
3. Propagate changed used size or overflow to the first dependent ancestor.
   Escalate to a wider root only when that dependency actually changes.
4. Compare each incremental result with the fresh-final-document harness.

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
