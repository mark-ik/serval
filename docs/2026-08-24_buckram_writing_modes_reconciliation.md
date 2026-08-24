# Buckram writing modes: current-main reconciliation

**Date:** 2026-08-24

**Status:** In progress.

**Parent:** [Buckram and Livery lane program](2026-08-21_buckram_livery_lane_program_plan.md),
row 10, and the
[K3 completion execution plan](2026-07-28_buckram_k3_completion_execution_plan.md),
K3r and K3t.

## Ruling

The archived wave-2 row is underspecified. It names `FlowAxes` consumers and
`to_block_style`, but supplies no failing fixture or done condition. Accepted
main already contains substantial K3r/K3t work: five writing modes, both inline
directions, logical box geometry, delayed physical conversion, generated-box
inheritance, retained vertical text, and body/root propagation.

The corrected current-main directory map nevertheless shows a live row-owned
gap. Orthogonal normal-flow boxes with an automatic inline size still consume
the mapped available physical space as a direct inline size instead of using
the Writing Modes fit-content constraint. This lane therefore begins with that
coherent implementation slice in `taffy_adapter.rs` and the existing Livery
live path. It preserves the accepted K3 model rather than adding a parallel
writing-mode API.

## Current contract

- `FlowAxes` owns abstract-to-physical side, size, offset, and rectangle
  conversion for horizontal-tb, vertical-rl, vertical-lr, sideways-rl, and
  sideways-lr with LTR and RTL inline directions.
- Computed `writing-mode` and `direction` are lowered only after cascade has
  selected the winning values. Generated, anonymous, pseudo, marker, and text
  boxes retain those inherited axes.
- Buckram performs normal-flow placement in logical axes. Auto block size is
  finalized before the fragment edge derives physical geometry.
- Orthogonal child layout preserves an indefinite cross-flow size until its
  own formatting result is known. Percentages use the named physical fallback
  only at the cross-flow boundary.
- Fragment consumers retain absolute physical paint rectangles alongside
  flow-relative logical rectangles and logical baselines.

## Explicit adjacent work

- Orthogonal float and clearance continuation stays deferred until Buckram can
  transform exclusion state without copying physical left/right meaning across
  flows.
- `text-orientation` and `text-combine-upright` are not yet represented in
  Livery. The architecture plan assigns their text integration to K7.
- Multi-fragment vertical sizing belongs to K6.
- Positioned, table, flex, and grid writing-mode residuals remain with their
  owning algorithms rather than widening this normal-flow row.

## First implementation slice

Normative basis: [CSS Writing Modes 3, section 7.3](https://www.w3.org/TR/css-writing-modes-3/#orthogonal-flows),
especially [auto-sizing orthogonal flow roots](https://www.w3.org/TR/css-writing-modes-3/#orthogonal-auto).

For an in-flow block whose flow is orthogonal to its containing block and whose
own inline size is automatic:

1. perform sizing in the child's flow while leaving margins and final
   positioning in the containing flow;
2. query the child's min-content and max-content inline contributions;
3. when a perpendicular descendant makes that query block-size-dependent,
   size the descendant first through the admitted Buckram block path and feed
   its used block size into the parent's intrinsic inline contribution;
4. choose the orthogonal inline size with
   `min(max-content, max(min-content, constraint))`;
5. take the constraint from the definite containing-block measurement, or the
   initial containing block when that measurement is indefinite; and
6. carry the selected size into the child's logical layout before physical
   fragment conversion.

The first native fixture is the two-level orthogonal case: a vertical block
containing one horizontal line must keep the parent's full block width while
its own inline size contracts to that line. The complete
`sizing-orthog-{vlr,vrl}-in-htb` and inverse `htb-in-{vlr,vrl}` families are
the focused corpus gate.

Stop if the repair makes physical width or height primary inside Buckram, reads
an intrinsic answer from a completed Taffy block layout, or crosses into the
positioned, table, flex/grid, float-continuation, text-orientation, or
fragmentation owners above. In particular, a measured vertical inline leaf
continues to defer until K7 gives the formatter a logical-axis query; this row
does not relabel its physical-width answer as a vertical inline contribution.

## Acceptance

- Pure `FlowAxes` fixtures cover all modes, directions, side mappings, and
  logical/physical rectangle round trips.
- Adapter fixtures cover vertical stacking, orthogonal auto block sizing,
  horizontal/vertical nesting in both directions, percentage fallback, and
  the retained cross-flow float deferral.
- Live Livery fixtures cover inherited generated-box axes, orthogonal fragment
  geometry and baselines, contained body writing mode, and root/body
  propagation.
- The corrected pre-change map is 186 verified pass, 927 fail, 255 skip, and
  zero error across all 1,368 `css/css-writing-modes` files. The candidate map
  must list every exact movement and contain no unexplained loss.
- Focused native tests, strict scoped Clippy provenance, and `git diff --check`
  are green on source identical to current main.

## Done condition

Row 10 closes when those current-main fixtures and the corrected complete
directory map remain reproducible, the accepted K3r/K3t normal-flow contract
is still live, and every adjacent residual above retains an explicit owner.
