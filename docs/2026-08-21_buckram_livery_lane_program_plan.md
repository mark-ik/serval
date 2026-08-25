# Buckram and Livery lane program

**Date:** 2026-08-21

**Status:** Active. Wave 1 lanes launched 2026-08-21 from branch
`k5-regression-repair`, which carries the 2026-08-21 K5 repairs, the K5
regression ledger, and this plan. Wave 2 lanes start after the admission
keystone (lane 8) lands, because they build on block dispatch it changes.

**Parent:** [Buckram CSS layout engine plan](2026-07-26_buckram_css_layout_engine_plan.md)
and the [Livery fullweb cutover plan](2026-07-24_livery_fullweb_cutover_and_servo_retirement_plan.md).

## Rulings that shape the program (2026-08-21)

1. Where a correct implementation obviates a repair, do the implementation.
   The 2026-08-21 Taffy-fallback flip and the seven files it regressed are
   resolved by lane 8, not by accepting debt.
2. The anonymous-table residual gets a fresh follow-on plan; K4 stays closed.
3. The three red K5 receipts on `main` are fixed, not deferred.
4. Every other still-red file in the K5 regression ledger is fixed unless the
   correct implementation of a lane obviates it.

## Numbers the lanes are cut from

Final 2026-08-21 ledger
(`testing/genet/wpt-ledger/2026-08-21_anonymous_table_remeasure/post-fix-4-final/`):
10,340 pass / 9,388 fail / 16,583 skip of 36,311 `css` files. Failures by
directory: CSS2 1405, css-writing-modes 850, css-break 781, css-text 781,
css-grid 730, css-flexbox 500, css-backgrounds 342, css-sizing 299, WOFF2 298
(0 pass), css-masking 298, css-multicol 288, css-overflow 245, css-gaps 188,
css-shapes 164, css-transforms 158, css-images 146, css-fonts 143.

## Rules every lane follows

- **Ownership.** A lane edits only the files its row names. A needed change
  elsewhere is written up as a seam request in the lane's plan, not made.
  `genet-livery/src/layout.rs` is shared by region; lanes name their region.
- **Base and branch.** Start from `k5-regression-repair`; work on
  `lane/<name>` in a worktree under `Code/worktrees/genet-lane-<name>`;
  commit there; never touch `main` or push. No `Co-Authored-By` trailers.
- **Build sharing.** `CARGO_TARGET_DIR=C:\Users\mark_\Code\repos\genet\target`
  so dependencies are shared; the release runner binary lands at
  `target/release/genet-wpt.exe` and is clobbered by other lanes, so copy it
  into the lane's ledger directory before measuring.
- **Measuring.** Ledgers go under `testing/genet/wpt-ledger/2026-08-21_<lane>/`.
  Run directory subsets, not the full `css` corpus; the integrator runs the
  corpus ratchet. Two runner processes reading the tree at once can report
  `read-failed`; re-run that file alone. The Livery lib test links only with
  `--config profile.dev.package.genet-livery.debug=1`.
- **Receipts.** A Buckram structural test or Livery fixture for every behavior
  change, an absolute WPT before/after map for the lane's directories, and no
  unexplained loss in them. Three pre-existing red tests are known:
  `positioned_inset_mutation_reuses_a_stable_fragment_subtree` and both in
  `tests/grid_abspos.rs`; lanes 3 and 4 own them.
- **Ladder.** `cargo test -p buckram --offline`;
  `cargo test -p genet-livery --all-targets --offline --no-fail-fast --config profile.dev.package.genet-livery.debug=1`;
  `cargo clippy -p buckram -p genet-livery --offline --no-deps -- -D warnings`;
  `rustfmt --edition 2024 --check <touched files>`; `git diff --check`.
- **Docs.** Each lane writes `docs/2026-08-21_<keyword>_execution_plan.md`
  first (ruling, scope, owned files, receipts, stop rules, done condition,
  no time estimates) and keeps it current. Lanes do not edit the master plan,
  the fullweb register, or this file; the integrator folds their results in.
- **Done means measured.** A lane closes on its done condition with receipts
  in its plan, not on a commit.

## Wave 1

| # | Lane | Owns | Done condition |
|---|---|---|---|
| 8 | Block-formatter admission of independent formatting contexts (keystone) | `buckram/src/taffy_adapter.rs` block arm and deferral walk, `buckram/src/block.rs` | A table, flow-root, or non-visible-overflow child no longer defers its block ancestors to Taffy; `BlockAlgorithmCounts.taffy` is 0 on the table fixtures; `with_out_of_flow_children_excluded` is deleted because nothing reaches it; the seven files the flip regressed pass; CSS2 and css-position ratchets show no unexplained loss |
| 1+2+7 | K5 positioning closure, Livery side | `genet-livery/src/layout.rs` regions: `apply_relative_positioning`, `apply_relative_table_part_offsets`, `record_static_position` call sites, `positioned_placements`; `tests/positioned_block_children.rs` | The two ignored tests pass and are un-ignored; the 12 `position-relative-table-*` files, `position-relative-table-caption`, `position-absolute-in-inline-margin-top`, `static-inside-inline-block`, `abspos-containing-block-initial-009e`, `fixed-table-layout-017..020` are green or attributed to lane 8 with a named reason |
| 3 | K5h retained text frame | `genet-livery/src/layout.rs` `reposition_stable_positioned_subtree` and `resize_positioned_leaf`, `genet-livery/src/text.rs` `TextFrame` translation, `document.rs` callers | `positioned_inset_mutation_reuses_a_stable_fragment_subtree` passes; a leaf-resize equivalent fixture exists and passes |
| 4 | K5b grid static rectangle | `buckram/src/taffy_adapter.rs` grid arms and carrier, `genet-livery/src/layout.rs` `positioned_containing_block_rect` | Both `tests/grid_abspos.rs` tests pass; the commit that broke them is named in the plan |
| 5+6 | K5d sizing and vertical-mode insets | `buckram/src/positioning.rs`, `positioned_intrinsic_sizes` admission in `taffy_adapter.rs`, `positioned_replaced_input` in layout.rs | K5d resolves aspect-ratio and replaced sizing itself; `css-sizing/aspect-ratio/abspos-008/-014`, `css-ruby/abs-in-ruby-*`, `css-fonts/font-feature-resolution-001/-002`, `pre-wrap-leading-spaces-014` pass; the references of the 33 `css-shapes` files render their absolute boxes (logical insets in vertical-rl inside an absolute container) |
| 11 | Anonymous-table construction follow-on | new plan `2026-08-21_buckram_anonymous_table_followon_plan.md`; `buckram/src/box_tree.rs` table fixup; `genet-livery/src/paint.rs` table background phases for `<col>` | `table-anonymous-objects-059..098` measures 40/40 or each residual file is named with a reason outside the lane; `<col style="background">` paints in the HTML reference |
| 13 | K6 corpus census (documentation only) | `docs/2026-08-15_buckram_k6_fragmentation_execution_plan.md` inventory sections, new fixture files under `genet-livery/tests/` marked `#[ignore]` | The K6 plan's corpus inventory is regenerated against the final 2026-08-21 ledger; continuation-contract fixtures exist for block, inline, and table roots; no engine code |
| 14 | css-text | `genet-livery/src/text.rs`, `components/livery` text properties, Parley bridging | css-text directory: every failure attributed to a named family; line-breaking, white-space, text-justify, overflow-wrap families measured before/after |
| 15 | Backgrounds, masking, images | `genet-livery/src/paint.rs` non-table paint, `ports/genet-wpt/src/render.rs` resource resolution, `document.rs` image resources | css-backgrounds, css-masking, css-images directories: each failure attributed; the document/base-URL seam the PH census named is either built or written as a seam request |
| 16 | Fonts and WOFF2 | `TextSystem` font loading in `genet-livery/src/text.rs`, font resource plumbing | WOFF2 directory moves off 0/300; css-fonts failures attributed |
| 19+20+21 | Harness and ledger | `ports/genet-wpt/`, new `testing/genet/tools/` | References that depend on unimplemented features are tagged and reported as `pass (reference unverified)`; per-channel tolerance honors WPT `fuzzy` metadata; the concurrent read race is fixed or serialized; a checked-in expectation-diff tool replaces ad hoc JSON diffs |

## Wave 2 (after lane 8)

| # | Lane | Owns |
|---|---|---|
| 9 | Intrinsic sizing contributions | `buckram/src/intrinsic.rs` |
| 10 | Writing modes | `FlowAxes` consumers in Buckram, `to_block_style` |
| 12 | Floats and shapes | `buckram/src/block.rs` float exclusions, shape-outside |
| 17 | Counters, lists, generated content | `livery` cascade `content`/`counter-*`, Livery marker boxes |
| 18 | Flex and grid through Taffy | `taffy_adapter.rs` flex/grid arms, `to_taffy_style` |

## Stop rules

- Stop a lane that starts editing another lane's owned file; write the seam
  request instead.
- Stop a lane whose ratchet shows a loss it cannot attribute.
- Stop if a lane proposes to accept a regression as debt; that is a ruling.

## Done condition

The program closes when every wave 1 lane has closed on its own done
condition, their branches are merged to `main` with the corpus ratchet
showing no unexplained loss, and the master plan's K5 regression ledger has
no file without a green receipt or an accepted deferral.
