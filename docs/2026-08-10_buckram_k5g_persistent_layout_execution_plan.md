# Buckram K5g: persistent box and fragment storage

**Date:** 2026-08-10

**Status:** Planned. Requires the settled K5 positioning records and K5d final
positioned fragment path.

**Parent:** [Buckram CSS layout engine plan](2026-07-26_buckram_css_layout_engine_plan.md),
K5g.

## Scope

The current Livery route regenerates `CssBoxTree` and appends a new dense
`FragmentTree` on each layout. K5g makes identity persistent across an
unfragmented continuous-media relayout. K5h will decide damage, select a root,
and prove incremental equivalence; K5g supplies the storage and invariants
that make that possible.

## Model

- `BoxId` is allocated from retained generation provenance, not from the
  current vector position. Reused element, pseudo, anonymous, and table boxes
  retain identity only when their generation context still matches.
- `FragmentId` is likewise independent of storage position. Fragments outside
  a replacement formatting-context subtree retain identity.
- Retained indices cover parent, containing fragment, fragments by box, boxes
  by node, and formatting-context ownership. Every replacement checks them
  before becoming visible.
- Intrinsic and formatting-context caches state the exact box/subtree and
  style dependencies that invalidate them.

## Work

1. Replace dense-index identity assumptions in Buckram's box and fragment
   stores with stable allocation and checked lookup.
2. Introduce a retained Livery layout state that owns the Buckram stores and
   can rebuild one generation context against prior identities.
3. Implement subtree insert, replacement, and removal with index repair and
   cache invalidation.
4. Preserve table wrapper, grid, caption, row-group, row, and cell identity
   through an unrelated sibling update. A table is a required second consumer
   of the storage model.
5. Add structural receipts for stable outside identities, changed-subtree
   identities, detached-node cleanup, and cache invalidation.

## Acceptance

- An unchanged node's generated box and fragment identifiers survive a layout
  pass.
- Replacing one formatting-context subtree retains all unrelated identities
  and removes no live side-table entry.
- A removed subtree releases its boxes, fragments, positioning records, and
  cache entries together.
- Index invariants are executable in debug/test builds after every mutation.
- The retained state is Buckram/Livery-owned; `genet-layout` is not used as an
  incremental semantic source.

## Stop rules

- Do not claim an incremental relayout until K5h compares it with a fresh
  final-document layout.
- Do not add fragmentation continuations or fragmentainer retention. K6 owns
  those identities.
- Do not preserve an identifier when its generated-box provenance changed.
