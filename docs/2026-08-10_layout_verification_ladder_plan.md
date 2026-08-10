# Layout verification ladder

**Date:** 2026-08-10

**Status:** V0 and V2 landed 2026-08-10 (receipts at the foot of their
sections). V2 found and fixed a plane leak on its first run. V1 is recorded as
a candidate only, against residual 2 of the event-loop follow-ups; it is
deliberately not scheduled here.

**Parent:** [genet-layout: state and roadmap](2026-06-16_genet_layout_roadmap.md)

**Verification lineage:** [Event-loop rigor follow-ups](2026-07-05_event_loop_rigor_followups.md)
(residual 2 owns further TLA+ witnesses), archived parent
[Event-loop rigor plan](archive/2026-06-24_event_loop_rigor_plan.md) (E4 sank
the trace harness), [Lessons from gterzian/formal-web](2026-06-24_formal_web_lessons.md).

**Successor engine:** [Buckram](2026-07-26_buckram_css_layout_engine_plan.md).
`LiveryLayout::atomic_fragments` already carries inline-block and
replaced-element fragments there. V0 is a fix to the **incumbent** engine that
cambium and turnstone ship on today; it is not a Buckram commitment and must
not grow into one.

## Ruling

genet-layout's correctness surface splits by the shape of the claim being
checked, and the tool has to match the shape. Three shapes, in ascending cost:

1. **Spec prose as arithmetic.** CSS 2.2 §10 is equations over box dimensions.
   The tool is reading the section and citing it in the code. No
   infrastructure, and it is the only rung already paying.
2. **A pure function over rich input.** Layout, cascade, the incremental
   splice. The tool is differential comparison against a reference path.
3. **A state machine with interleaving and liveness.** The event loop, host
   frame pacing. The tool is TLA+ trace validation, and the harness for it is
   already built (`docs/tla/`, two witnesses, CI, a negative fixture).

The ladder is ordered because rung 1 removes known-wrong behaviour that rung 2
would otherwise spend its runs rediscovering.

## What prompted it

The 2026-08-09 inline-fragment work (per-inline-box rects, so an inline-level
control has geometry for `absolute_rect`, `genet_probe::resolve`, and AccessKit
bounds). Two findings drove this plan:

- The spec **bounded** the fix. CSS 2.2 §10.6.1 and §10.6.6 have opposite rules
  for vertical padding, so what looked like one gap ("inline layout ignores the
  box model") is one real gap and one behaviour that is already correct.
- The actual defect was **not** spec-shaped. The fragment plane keyed an
  anonymous wrapper by a borrowed DOM id, so one node carried two meanings. No
  standard covers that. It was found with a throwaway probe, not by reading,
  and nothing in the tree would have caught it standing.

Rung 1 handled the first. Rung 2 exists because of the second.

## V0. Atomic inline box model (incumbent engine)

An `inline-block` is an **atomic inline-level box** (CSS 2.2 §9.2.2). genet
measures one from its content plus any definite width/height only:
`InlineBlockBox` (`components/genet-layout/text_measure.rs`) carries `content`,
`css_width`, `css_height`, and `background`, and nothing else of the box model.
So a `width: 240px; padding: 8px 12px; margin-bottom: 8px` button reserves
240x18.4 where the spec requires 264 wide (§10.2, §10.3.9: `width` is the
*content* width, padding and border add on top) and content+16 tall with the
margin counted in the line box (§10.6.6, §10.8: an inline-block contributes its
**margin box** to line box height).

The opposite rule holds one step away and must not be "fixed": for a
non-replaced **inline** box, vertical padding, border, and margin do not affect
line box height at all (§10.6.1). genet is already correct there. Only
horizontal padding and margin apply to those, and that is a separate, smaller
item.

**Done when**

- `InlineBlockBox` carries the resolved padding, border, and margin, and
  `measure_inline_box` reserves the margin box rather than the content box.
- Paint places the inline-block's own content inside its padding, so the
  reserved rect and the painted content agree.
- `crate::inline_fragment` records the **border** box, matching
  `getClientRects`' definition, and `absolute_rect` reports it.
- A `width: 240px; padding: 8px 12px` button reserves 264px of inline advance,
  pinned by a test.
- `smoke.rs`'s buttons render as they did before 2026-08-09, with the
  `display: block` workaround still absent.
- A regression test pins §10.6.1: adding vertical padding to a `display:inline`
  `<a>` does not change its line box height.

**Not in V0:** horizontal padding and margin on non-replaced inline boxes
(separate item, same section); the line-box-versus-content-area height
deviation in `inline_fragment`'s union (pinned in
`an_inline_anchor_reports_its_own_text_box_not_its_paragraph`); anything in
Buckram.

### V0 receipt (2026-08-10)

Six sites, because the reserved box is read in six places and they have to
agree or paint drifts from geometry:

- `InlineBlockBox` carries `padding` / `border` / `margin` as `EdgeSizes`,
  read by `construct::inline_block_box_edges` (definite lengths only;
  percentage and `auto` read as zero, the residual `inline_block_css_size`
  already carried).
- `measure_inline_box` reserves the margin box.
- `paint_emit` fills the border box and offsets the inline-block's own glyphs
  by border + padding.
- `inline_fragment::harvest` strips the margin, so the plane records the
  border box.
- `box_tree::apply_inline_cb_fixups` reduces the placed rect to the **padding**
  box before absolute insets resolve against it (CSS 2.2 §10.1). This one was
  not in the original scope; the parley rect doubles as an abspos containing
  block, so it would have silently gained a margin's worth of offset.
- Tests: `an_atomic_inline_reserves_its_margin_box_and_reports_its_border_box`
  and `vertical_padding_on_an_inline_box_does_not_change_line_box_height`.

Two existing test expectations were **wrong** and were corrected, not
accommodated. `paint_emit::a_sized_input_paints_at_its_css_size` asserted a
`width: 200px` input painted a flat 200x30; the UA sheet gives controls
`padding: 2px 6px; border: 1px solid` (`ua_defaults.rs`), so the border box is
214x36. Three `genet-probe` tests asserted a 20x20 swatch button; they were
already failing on a clean tree for the same reason, since the block path had
always added the ring. The two paths now agree on one declaration, which is the
substance of the fix.

Green: 339 genet-layout, 17 genet-probe, 47 servo-paint e2e, plus genet-render,
cambium host, cambium-winit-a11y, genet-scripted. Headed smoke scenario
`RESULT ok`, 3 non-blank captures with distinct digests. Inline-block and block
button layouts now agree to within a pixel (44.0 vs 44.5 vertical pitch); the
residual is line-box baseline alignment against block stacking, which is a real
difference between the two formatting contexts rather than a leftover gap.

## V1. TLA+ witness for the host frame loop (candidate, not scheduled)

Recorded against **residual 2** of the event-loop follow-ups, which owns
further protocol witnesses and already names three candidates (Navigation,
MessagePort ordering, the transitions plan's atomic rendering tick). This adds
a fourth and does not jump the queue. The gate there is unchanged: do one
protocol at a time, when one becomes scary enough to pay for.

**The candidate.** `cambium-genet-winit-host`'s frame loop:
`IdlePolicy::{Wait, Animate, A11yWake}` (`src/lib.rs:405`), `request_redraw`
called from five sites across three files, capture arming through a
thread-local, and the a11y wake path.

**Why it is worth recording.** `A11yWake` encodes a **liveness** claim: a
screen reader acted while the app was idle, so the queued accessibility action
is eventually drained. Safety properties a test can approximate by checking the
states it reaches. Liveness it cannot, because the failure is an infinite path
where nothing bad happens and nothing good happens either. A stuck screen
reader is exactly that bug, and TLA+ states the property in one line.

**Honest cost correction.** The follow-ups' "the harness cost is already sunk"
holds for `components/script-runtime-api`, whose
`Runtime::scheduler_trace_ndjson()` feeds `scheduler_trace_to_tla.py`. A
cambium host witness needs a *new* tracer in a different crate, so this
candidate is more expensive than the three already listed. It should not be
picked on cheapness. **Trigger:** the first real dropped-frame or stuck-reader
report, or the transitions tick landing and making the host side the remaining
unmodelled half.

## V2. Differential splice harness

`IncrementalLayout::try_splice_at` is a refinement claim: the spliced plane
equals the plane a full relayout would produce. Failure is silent, a stale rect
rather than a crash, and it is currently held up by hand-reasoned bail
conditions. The 2026-08-09 work added a second table to that claim
(`FragmentPlane::inline_boxes`) and settled its staleness question by
inspection, which is precisely the kind of reasoning this rung replaces.

Two things make it harder than "run both paths":

- **Generator yield.** The splice bails to full relayout on five conditions
  (`no-prior-fragment`, `no-scoped-fragment`, `margin-collapse`, `outer-size`,
  `graft-bail`). A naive mutation generator produces cases that mostly bail, at
  which point both arms run the same code and nothing is tested. Yield is
  directly measurable: every bail already emits `reason` on the
  `genet_layout::splice` tracing target.
- **Equality.** Float tolerance, and both `rects` and `inline_boxes`.

**Done when**

- A corpus of (DOM, mutation) pairs where every splice-eligible case yields
  plane equality within tolerance, across both tables.
- The corpus reports its own splice-path yield, so a run that silently stopped
  exercising the splice is visible rather than green.
- At least one deliberately broken splice is **rejected**, following E4's
  bad-trace fixture discipline: a checker that has never failed is not known to
  work.

**Blocked on V0.** A harness built while a known box-model gap is live spends
its runs rediscovering that gap.

### V2 receipt (2026-08-10)

`components/genet-layout/tests/splice_differential.rs`. A hand-built corpus of
seven shapes, each run down both paths and compared over the whole plane, both
tables. No new dependency, per the stop rule. Yield on landing: 4/7 reach the
splice, 1 takes the attribute-restyle path, 2 bail to a full relayout.

**It found a defect on its first run.** A spliced *removal* left the removed
node's entry in the plane. The splice writes results by walking the attached
DOM, so a detached node is never visited; the box tree's own graft already
purges the departed subtree from `node_map`, `inline_sources`, and the text
cache, making the plane the one side table that drifts. Unbounded growth across
a session's churn, plus a `fragment_count` that over-reports. Fixed with
`FragmentPlane::retain_live`, swept from `apply_structural` only when the batch
actually detached something.

The predicate matters and the first attempt was wrong: `LayoutDom::is_live`
means "still in the arena", and a removed node stays live so it can be
re-inserted, so liveness would have kept precisely the entries that must go.
The sweep tests **attachment** (reachability from `document()`) instead.

Three anti-vacuity disciplines, since a differential harness is easy to make
green and worthless:

- Yield is printed and floored at 3. A bailed case runs the same code down both
  arms, so a corpus that stopped reaching the splice would pass while testing
  nothing.
- `the_comparator_detects_a_stale_plane` feeds the comparator a knowingly stale
  plane and requires a complaint, naming the specific missing node. Borrowed
  from E4's deliberate bad-trace fixture: a checker that has never failed is not
  known to work.
- Both tables are compared, key sets as well as values. The 2026-08-09
  inline-box table's splice staleness had been settled by inspection, which is
  the blind spot this rung exists to close.

**Residual.** Two inline cases (an inline-block relabel, an inline-anchor text
change) currently bail to a full relayout rather than splicing, so the inline
lane is exercised through one splicing case ("append a second inline-block
button") rather than three. Worth revisiting when the bail reasons are
attributed; they are on the `genet_layout::splice` tracing target.

## Stop rules

- V0 touches the incumbent `genet-layout` only. Buckram already models atomic
  fragments; do not mirror this work there.
- V0 does not extend the box model to non-replaced inline boxes beyond the
  horizontal axis. §10.6.1 says the vertical axis is already right.
- V1 is recorded, not scheduled. It does not consume a slot ahead of the
  follow-ups' three named candidates.
- V2 adds no new dependency (proptest, quickcheck) without asking first; a
  hand-built corpus is the default.

## Done condition

V0 landed with its receipts, V1 recorded on the follow-ups residual, and V2
either landed or explicitly deferred with its blocking reason named.

**Met 2026-08-10.** V0 and V2 landed; V1 recorded, unscheduled, with its cost
corrected. The ladder's claim held up in practice: rung 1 (reading §10.6.1 next
to §10.6.6) both fixed a real gap and stopped a wrong one from being "fixed",
and rung 2 caught a defect in its first run that no amount of spec reading would
have found, because no standard says how to key a fragment plane.
