# Buckram K5g: persistent box and fragment storage

**Date:** 2026-08-10

**Status:** In progress. K5a through K5d expose the records needed for
retained identity; K5g has replaced dense box and fragment identity with a
reconciled allocation layer. Dirty-root replacement remains K5h.

**Parent:** [Buckram CSS layout engine plan](2026-07-26_buckram_css_layout_engine_plan.md),
K5g.

## Scope

The current Livery route regenerates `CssBoxTree` and appends a new dense
`FragmentTree` on each layout. K5g makes identity persistent across an
unfragmented continuous-media relayout. K5h will decide damage, select a root,
and prove incremental equivalence; K5g supplies the storage and invariants
that make that possible.

## Current implementation boundary

`BoxId` and `FragmentId` now name allocation slots independently from their
fresh construction order. A rebuilt `LayoutResult` reconciles compatible
generation contexts against its immediately preceding result, then repairs
parent, containing-fragment, by-box, by-node, principal-box, and
static-position source indices before that result is exposed. New contexts
allocate above the previous high-water mark, so a detached identity cannot be
resurrected by a later insertion.

`LiveryDocument` keeps an invalidated layout only as an internal identity
source. Its next frame still recomputes complete geometry, reconciles the new
Buckram result, and drops the prior generation. A sibling-insertion receipt
proves ordinary content and the table wrapper/grid retain their box and
fragment identities despite changed dense construction order.

This is persistent storage, not K5h incremental layout. It does not select a
dirty formatting-context root, reuse a prior geometric result, or yet name
intrinsic and formatting-cache invalidation dependencies.

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

1. Complete: replace dense-index identity assumptions in Buckram's box and
   fragment stores with stable allocation, checked lookup, and executable
   index invariants.
2. Complete for full-generation reconciliation: introduce retained Livery
   identity state that rebuilds a fresh generation against prior identities.
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
