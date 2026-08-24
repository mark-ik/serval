# Buckram and Livery lane program

**Date:** 2026-08-21
**Status:** Reconstituted 2026-08-24 from accepted main. Lane 8 and the
anonymous-table continuation are complete. Wave 2 is unblocked. Every other
row remains an inventory item until its current-main receipt is named below.

**Parent:** [Buckram CSS layout engine plan](2026-07-26_buckram_css_layout_engine_plan.md)
and the [Livery fullweb cutover plan](2026-07-24_livery_fullweb_cutover_and_servo_retirement_plan.md).

## Provenance ruling

The original parallel launch from `k5-regression-repair` was retired because
some lane results depended on uncommitted source and a shared build directory.
Archived `lane/*` branches and `7499aff278b` are forensic evidence only. New
work starts from accepted `origin/main`, in an isolated worktree and target
directory. A lane may reuse a historical diagnosis, but not its unverified
receipt or dirty overlay.

The accepted recovery chain and later focused continuations are the source of
truth. Lane 8 closed on `ac73b07badb` and its receipt commit `0e2a6bebed3`.
The anonymous-table construction and sibling-table continuation are recorded
in [their recovery plan](2026-08-23_buckram_anonymous_table_recovery_plan.md).

## Rules every lane follows

- **Ownership.** Edit only the files and regions named by the lane. Record a
  required cross-lane change as a seam request.
- **Base.** Fetch and start from accepted `origin/main`. Inspect status before
  staging and stage only owned paths.
- **Build isolation.** Give each worktree its own `CARGO_TARGET_DIR`. On
  Windows use `CARGO_PROFILE_TEST_DEBUG=0` when PDB pressure blocks runnable
  tests, and serialize jobs when linker or disk pressure requires it.
- **Runner identity.** Build the release `genet-wpt` runner from the candidate,
  copy it into the external ledger, and record its commit and SHA-256. A shared
  `target/release/genet-wpt.exe` is not a frozen receipt.
- **Measuring.** Keep ledgers under `testing/genet/wpt-ledger/<dated-lane>/`.
  Run the lane directories before and after from frozen runners. Any
  unexplained pass-to-fail result stops the lane.
- **Native wall.** Run focused fixtures, full affected-crate tests, scoped
  strict Clippy, formatting, and `git diff --check`.
- **Done means measured.** A commit closes a row only when its plan names the
  native and WPT receipts that prove the done condition.

## Current lane ledger

| # | Lane | State on 2026-08-24 | Done condition or next proof |
|---|---|---|---|
| 8 | Block-formatter admission | **Complete** | Independent tables and flow roots stay opaque to the containing block formatter; CSS-facing Taffy block runs are zero, backend scratch sizing is counted separately, and CSS2 tables plus css-position are byte-identical to baseline. See the [lane plan](2026-08-23_buckram_block_formatter_admission_execution_plan.md). |
| 1+2+7 | K5 positioning closure, Livery side | Inventory required | Re-run the named relative-table, static-position, inline-block, and fixed-table files on current main; close each residual with a live fixture or a named owner. |
| 3 | K5h retained text frame | Inventory required | Prove retained positioned-subtree translation and leaf resize with current fixtures. Do not rely on the archived dirty `translate_subtree` overlay. |
| 4 | K5b grid static rectangle | Inventory required | Both `tests/grid_abspos.rs` receipts pass and the responsible current code path is named. |
| 5+6 | K5d sizing and vertical-mode insets | Inventory required | Re-run aspect-ratio, ruby, font-feature, pre-wrap, and the 33 shapes references from current main; separate solved font/contain work from remaining logical-inset work. |
| 11 | Anonymous-table construction and sibling tables | **Complete** | The 059-098 family is 32 pass / 8 explained compositor residuals; column backgrounds, nested tables, block children, and sibling geometry have live receipts, with zero directory losses. |
| 13 | K6 corpus census | Open | Regenerate the fragmentation inventory from the current ledger and add ignored continuation-contract fixtures for block, inline, and table roots. |
| 14 | css-text | Open | Attribute every current css-text failure by family and land focused line-breaking, white-space, justification, and overflow-wrap receipts. |
| 15 | Backgrounds, masking, images | Open | Attribute css-backgrounds, css-masking, and css-images; prove document and base-URL resource ownership. |
| 16 | Fonts and WOFF2 | Open | Move WOFF2 off its zero-pass historical baseline and attribute css-fonts against the current text/font stack. |
| 19+20+21 | Harness and ledger | Open | Report unverified references honestly, honor WPT fuzzy metadata, remove the concurrent read race, and retain a checked-in expectation diff. |

## Wave 2, now unblocked

Lane 8 admitted independent block roots without widening flex or grid. These
lanes can now start independently from current main, subject to their own
plans and receipts.

| # | Lane | Owned surface | State |
|---|---|---|---|
| 9 | Intrinsic sizing contributions | `buckram/src/intrinsic.rs` | Open |
| 10 | Writing modes | `FlowAxes` consumers in Buckram and `to_block_style` | Open |
| 12 | Floats and shapes | block float exclusions and `shape-outside` | Open |
| 17 | Counters, lists, generated content | Livery `content`/`counter-*` cascade and marker boxes | Open |
| 18 | Flex and grid | Taffy adapter flex/grid arms and style bridge | Open |

Lane 8 retained `with_out_of_flow_children_excluded`. It still has named
fallback and backend-sizing call sites, so deletion is not a condition for
Wave 2. Its stronger receipt is zero CSS-facing fallback for admitted
table/flow-root cases.

## Stop rules

- Stop on an unexplained WPT loss.
- Stop before using an archived lane as an integration base.
- Stop if an implementation crosses another lane's owned boundary without a
  written seam and a focused receipt.
- Stop if a backend scratch run is reported as a CSS-facing fallback.

## Done condition

The program closes when every row in both waves has a current-main plan and
receipt, the current K5 ledger has no unattributed red file, the corpus
ratchet has no unexplained loss, and each result is integrated into accepted
main from isolated, reproducible inputs.
