# Buckram block-formatter admission execution plan

**Date:** 2026-08-23

**Status:** complete on source commit `ac73b07badb`, rebased onto
`55ec948b0f4`.

**Receipt:** Buckram 234/234; Genet-Livery 200/200 unit tests and every
integration target; live `block_admission` 4/4; strict scoped Clippy, Rust 2024
format check, and `git diff --check` clean. The release `genet-wpt` runner is
SHA-256 `B888A987A2091762A64A436FAABB3464EEBDBB4FEC60C14B7DE0442F4AAD9CB7`.
Against the frozen current-main runner, `css/CSS2/tables` remains
192/70/877 pass/fail/skip and `css/css-position` remains 47/71/226; both result
logs are byte-identical to baseline.

**Parent:** [Buckram and Livery lane program](2026-08-21_buckram_livery_lane_program_plan.md), lane 8.

## Ruling

An independent formatting context is opaque to the block formatting context
that contains it. Its own formatter computes its border box; the containing
Buckram block formatter owns only normal-flow placement, margins, and float
avoidance at that boundary. A table, flow-root, or scroll container must not
make every block ancestor fall back to Taffy merely because its descendants use
a different formatter.

Flex and grid keep their current explicit boundary until their Wave 2 lane.
Float avoidance stays named and conservative when an active float needs a
capability the child has not admitted.

## Provenance and scope

This lane continues the clean recovery branch `1f494b6c632`, not any archived
`lane/*` branch or the invalid `7499aff278b` overlay. Its owned source is:

- `components/buckram/src/block.rs`
- `components/buckram/src/taffy_adapter.rs`
- `components/genet-livery/src/layout.rs`
- `components/genet-livery/tests/block_admission.rs`

The implementation may add a narrow table-wrapper width handoff because CSS
Tables makes the wrapper border box an algorithm result. It does not enter
table track sizing, flex/grid ownership, general intrinsic sizing, or K6.

## Receipts

1. Buckram unit tests prove that a table wrapper and a flow-root remain opaque
   children of a Buckram parent, while an unadmitted active-float case retains
   its named fallback.
2. `block_admission` proves live Livery construction for nested tables, table
   auto margins, caption wrapper width, and a locally deferred flow-root. Its
   admission measure is zero CSS-facing Taffy block runs; backend scratch
   sizing is counted separately.
3. Full Buckram and Genet-Livery test walls pass on an isolated target.
4. CSS2 tables and css-position WPT directories show no unexplained
   pass-to-fail result against the clean pre-lane baseline. The seven historical
   flip regressions are checked explicitly if they remain red in that baseline.
5. Strict Clippy passes for the touched libraries and `git diff --check` is
   clean.

## The out-of-flow fallback shim

The parent program proposed deleting
`with_out_of_flow_children_excluded` because lane 8 would make it unreachable.
That is a hypothesis, not an editing instruction. Current Buckram still uses
the shim for named CSS-facing fallbacks and for backend sizing below an already
recorded fallback. Delete it only if call-site and regression receipts prove it
unreachable or unnecessary. Otherwise retain it and correct the master plan;
preserving K5 out-of-flow semantics is the stronger condition.

## Stop rules

- Stop on an unexplained WPT regression.
- Stop before widening flex/grid dispatch, float avoidance, or table sizing.
- Stop if a backend scratch run is misreported as a CSS-facing ancestor
  fallback.
- Rebase onto current `origin/main` only after the lane is independently green;
  then rerun every receipt on the rebased result.

## Done condition

The lane closes when the four live fixtures and focused Buckram fixtures pass,
the CSS-facing fallback count is zero for admitted table/flow-root cases, the
full native walls and strict Clippy pass, the WPT ratchets have no unexplained
loss, and the rebased commit is reproducible from current `origin/main`.
