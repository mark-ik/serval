# Buckram K5h: dirty-root relayout

**Date:** 2026-08-11

**Status:** In progress. K5h now has an explicit damage record and
final-document equivalence harness. It has not replaced a formatting-context
subtree yet.

**Parent:** [Buckram CSS layout engine plan](2026-07-26_buckram_css_layout_engine_plan.md),
K5h.

## Current boundary

`LiveryDocument` records `LayoutDamage` before it discards a retained layout.
DOM mutations map to the nearest existing Buckram formatting context, and
DOM, stylesheet, device, resource, interaction, and viewport events retain
their distinct event class. Overlapping DOM roots coalesce to a disjoint,
outermost set.

The current correctness path still recomputes full geometry, reconciles K5g
identities, and records the damage for inspection. `LayoutDamage` is therefore
not an incremental-layout claim.

`retained_mutation_paints_like_a_fresh_final_document` compares the paint
commands and document extent after a retained mutation with a fresh document
constructed from the same final DOM. This is the equivalence harness that a
future dirty-root replacement must continue to satisfy.

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
- Do not call K5h complete until a real root is replaced and compared against
  fresh geometry across mutation, style, resource, interaction, and resize
  cases.
