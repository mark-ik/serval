# Buckram K5a: containing-block graph and inputs

**Date:** 2026-08-10

**Status:** Complete. Landed 2026-08-10.

**Parent:** [Buckram CSS layout engine plan](2026-07-26_buckram_css_layout_engine_plan.md),
K5a.

## Scope

K5a resolves the generated box tree's containing-block relationships before
any geometry is placed. It replaces the old `ContainingBlock::Pending` marker
with a graph that records three distinct facts for every generated box:

1. its normal-flow containing box;
2. the nearest implemented absolute-position containing box; and
3. the nearest implemented fixed-position containing box, or the initial
   containing block when none exists.

It carries the currently implemented containing-block triggers from Livery:
non-static `position` for the absolute chain, and non-`none` `transform` plus
`contain: layout` / `contain: paint` for both chains. Unsupported properties
remain outside this graph until Livery has computed values for them.

K5a does not place an out-of-flow box, synthesize a static rectangle, move
fragments, choose a sticky scrollport, or retain a layout pass. Those are K5b
through K5h.

## Ownership

- `components/buckram/src/box_tree.rs` owns the graph, anonymous-box transfer,
  and the public `ContainingBlock` relationship.
- `components/genet-livery/src/box_tree.rs` lowers computed CSS triggers into
  that Buckram input. It does not search ancestors or choose a containing box.
- The table wrapper receives the table root's positioning context before its
  grid and internal parts materialize. This consumes the K4h wrapper seam and
  does not create a table-specific positioning path.

## Work

1. Replace the pending rule enum with resolved `Initial | Box(BoxId)`
   relationships during materialization.
2. Thread separate normal, absolute, and fixed chains through the generation
   walk. The current box updates the chains only for its descendants.
3. Move the table root's establishment triggers with its existing position and
   float transfer onto the anonymous wrapper.
4. Add structural Buckram tests for ordinary positioned ancestors,
   transform-like fixed capture, and positioned table internals.
5. Add a Livery integration test proving that computed transform and containment
   reach the correct graph flags and relations.

## Acceptance

- No `ContainingBlock::Pending` or independently resolved backend
  containing-block map remains.
- Static, relative, sticky, absolute, and fixed boxes all expose a resolved
  relationship.
- A fixed descendant ignores a merely positioned ancestor but follows an
  implemented fixed-position trigger.
- An absolute table part resolves through the table wrapper when the table
  root establishes the containing block.
- `cargo test -p buckram --lib --offline -j1` and the focused
  `genet-livery` wall pass, followed by formatting and a source audit.

## Stop rules

- Do not use Taffy's `Position` or parent links to answer a containing-block
  question.
- Do not lower an unimplemented CSS trigger to an ordinary positioned
  ancestor.
- Do not place or size an out-of-flow box in this gate. K5b defines static
  rectangles and K5d defines absolute/fixed used geometry.
- Do not restore a table bridge or add a table-only containing-block side list.

## Next gate

K5b receives its own execution plan before code begins. It will record static
position rectangles from block, inline, flex, grid, and table formatting
contexts against this graph.

## Receipt

- `ContainingBlock::Pending` and `ContainingBlockRule` are gone. Every box
  now carries a resolved `Initial | Box(BoxId)` relationship selected while
  the generated tree materializes.
- Buckram carries independent normal-flow, absolute, and fixed chains. The
  table wrapper receives the table element's establishment trigger before its
  grid and internal parts are resolved.
- Livery lowers the currently computed triggers only: non-static `position`
  for absolute descendants, and `transform`, `contain: layout`, and
  `contain: paint` for both absolute and fixed descendants.
- `cargo test -p buckram --lib --offline -j1`: 188 passed.
- `cargo test -p genet-livery --lib --offline -j1`: 87 passed.
- Rustfmt (toolchain 1.97.1) and `git diff --check` passed.
