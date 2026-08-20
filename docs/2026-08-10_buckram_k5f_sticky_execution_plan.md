# Buckram K5f: retained sticky constraints

**Date:** 2026-08-10

**Status:** In progress. K5f starts from K5g's stable fragment identities and
the document scroll state already retained by Livery.

**Parent:** [Buckram CSS layout engine plan](2026-07-26_buckram_css_layout_engine_plan.md),
K5f.

## Scope

K5f keeps a sticky box in normal flow, then derives a scroll-dependent physical
translation from its normal fragment, used physical insets, containing block,
and nearest scrollport. The base `LiveryLayout` remains unchanged between
scroll frames. A cloned active layout receives the constraint before paint and
hit testing, so scrolling does not relayout ordinary content.

## Data boundary

`buckram::StickyAxisInput` owns one physical-axis constraint. It accepts
normal-flow, scrollport, containing-block, used-inset, and scroll-offset
inputs, and returns only the physical translation. It has no DOM, paint list,
or formatter dependency.

Livery resolves the nearest scrollport from retained DOM/style state, passes
its static rectangle and nested or viewport scroll offset to Buckram, then
translates the sticky fragment subtree. The scratch formatter receives auto
sticky insets, so it cannot choose a browser sticky location.

## Current implementation boundary

The initial route covers ordinary sticky boxes with physical start and end
insets, viewport and nested scrollports, containing-block clamping, retained
paint, and retained hit testing. The scroll repaint clones the static layout;
it does not alter fragment identities or normal-flow geometry.

Table-internal sticky parts, scroll-container clipping semantics, and the
complete CSS positioned-layout constraint matrix remain explicit follow-up
work. This is not a K5 closure claim.

## Acceptance

- A `top: 0` box reaches the viewport edge after its normal position scrolls
  past that edge.
- The static layout fragment and its identifiers remain unchanged after a
  sticky scroll repaint.
- Start, end, and containing-block constraints have isolated Buckram tests.
- A nested scrollport uses its own retained offset rather than the viewport
  offset.
- The formatter has no sticky inset lowering.

## Stop rules

- Do not lower sticky to relative or fixed positioning.
- Do not rebuild layout merely to apply a scroll offset.
- Do not add fragmentation or page replication. K6 owns those cases.
