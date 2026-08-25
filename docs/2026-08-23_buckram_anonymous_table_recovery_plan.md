# Buckram anonymous-table recovery

**Date:** 2026-08-23

**Status:** Implemented and verified on `recovery/anonymous-table-v2` from
published main `c36c34be0ad`.

**Parent:** The anonymous-table follow-on required by the K4 tables and HTML
presentational-hints plans. The archived 2026-08-21 lane is forensic evidence,
not an implementation base.

## Ruling

Recover `css/CSS2/tables/table-anonymous-objects-059.xht` through `-098.xht`
from a clean, independently buildable published base. Implement CSS table-box
fixup and admit generated table grids through the existing Buckram table
pipeline. Keep table sizing contracts, generic block admission, positioning,
text, and harness tolerance outside this slice unless a focused receipt proves
that one of those seams is required.

## Immutable baseline

Ledger:
`testing/genet/wpt-ledger/2026-08-23_anonymous_table_v2` outside the worktree.

Runner SHA-256:
`1A65E49F7BC7E643993E73C13DE78EE0F57B7C6F15155F21B4690AC3BE9EAA63`.

The exact 40-file run is 10 passed and 30 failed. The passing files are 061,
065, 067, 069, 071, 073, 075, 077, 081, and 083. Every other file from 059
through 098 fails.

## Working hypotheses to prove

1. Generated table grids currently receive a table algorithm marker without
   entering the table handoff that computes cell rectangles and wrapper size.
2. Table fixup mishandles whitespace between table parts, consecutive
   non-cell row children, consecutive non-proper table children, and missing
   parents inside generated cells.
3. The 093 through 098 column cases must distinguish a construction failure
   from an HTML `<col>` or `<colgroup>` paint failure.
4. Files 079 and 080 contain ordinary sibling tables. They become a seam
   request if their defect lies outside construction, table handoff, or table
   paint.
5. Files 059, 060, 063, and 064 may be tolerance-only residuals. They remain
   red unless a renderer defect is proved.

## Owned surface

- `components/buckram/src/box_tree.rs`, table fixup and focused tests.
- `components/genet-livery/src/layout.rs`, generated table handoff only.
- `components/genet-livery/src/table_shadow.rs` and `table_block.rs` if the
  handoff must retain generated-grid style.
- `components/genet-livery/src/paint.rs`, table-column phases only if the
  focused paint receipt fails.
- `components/genet-livery/tests/anonymous_tables.rs` for live geometry and
  paint receipts.
- This plan.

## Gates

1. Add failing Buckram model fixtures for every changed fixup rule.
2. Add failing Genet-Livery fixtures for generated-grid admission, wrapper
   sizing, and column paint where required.
3. Run Buckram and Genet-Livery all-target tests with Windows test debug
   disabled if the linker requires it.
4. Run no-dependency Clippy for the affected crates and `git diff --check`.
5. Build a fresh release `genet-wpt`, copy the runner to the ledger, rerun the
   exact 40 files, then compare full `css/CSS2/tables` and `css/css-tables`
   directories against immutable baselines.

## Implemented boundary

- Buckram now applies CSS 2.1 table-fixup whitespace rules, groups consecutive
  improper table and row children, and repairs missing table parents inside
  generated cells.
- Genet-Livery now sends generated table grids through the existing Buckram
  sizing, row, wrapper, and paint handoff while retaining a synthesized table
  style for boxes with no DOM element.
- Table paint keeps structural table roots and decorated subtrees out of the
  positioned-inline coverage shortcut. This preserves overlapping table text
  and HTML column backgrounds.
- Four Buckram model receipts and four live Genet-Livery geometry/paint
  receipts cover the changed contracts.

## Exact family result

Candidate runner SHA-256:
`9DA2CE09CC8B9A9A2353B7A8A16DC96A84DAAF0CE88E49553B93FC6CECAD96E3`.

The candidate is 26 passed and 14 failed. It gains 18 tests against the clean
baseline and turns two prior passes into explained edge-only failures:

- 059 through 078 all pass.
- 093 through 098 all pass, including the six HTML column-background cases.
- 081 and 083 were false-positive baseline passes: the missing generated red
  layer was not painted. With construction live, both layers paint and differ
  from the green-only reference only at glyph edges (`diff=1%`, `max delta=64`).
  Files 082 and 084 have the same edge-only result.

The fourteen remaining files have executable WPT receipts and owners outside
this slice:

| Files | Receipt | Owner |
| --- | --- | --- |
| 079-080 | Three ordinary sibling HTML tables disagree with equivalent block rows (`max delta=255`). | Ordinary table block sizing and inter-table placement, K4c/K4d. |
| 081-084 | Geometry is visually equal; only stacked red/green glyph edges differ (`max delta=64`). | Generic stacking/text antialias composition or reftest tolerance. |
| 085-086 | Inferred cells containing block children disagree, and the WPT itself notes unresolved first-row spec wording. | Generic block formatting inside table cells plus WPT/spec ruling. |
| 087-088 | Split inline text in inferred cells leaves a small wrapped text residual (`diff=0%`, `max delta=255`). | Cross-node inline whitespace/shaping. |
| 089-090 | Nested row and row-group roles inside inferred cells duplicate or displace text. | Nested table-part layout after fixup, K4d. |
| 091-092 | Nested `table` and `inline-table` roles inside inferred cells duplicate or displace text. | Nested table handoff and sizing, K4c/K4d. |

## Full-directory comparison

The immutable baseline and candidate were run with `-v`; the ledger holds all
four logs.

| Directory | Baseline | Candidate | Status changes |
| --- | --- | --- | --- |
| `css/CSS2/tables` | 169 pass, 93 fail, 877 skip | 185 pass, 77 fail, 877 skip | 18 gains; the explained 081/083 edge-only losses; no other changes. |
| `css/css-tables` | 60 pass, 70 fail, 198 skip | 65 pass, 65 fail, 198 skip | Five gains and zero losses. |

The CSS Tables gains are `anonymous-table-cell-margin-collapsing.html`,
`percent-height-overflow-auto-in-unrestricted-block-size-cell.tentative.html`,
`percent-height-table-cell-child.html`, and both
`percentages-grandchildren-quirks-mode` cases.

## Sibling-table continuation

**Status:** implemented and verified on `recovery/table-sibling-block-v2`
from accepted main `f23d8eab215`.

The 079-080 seam request proved three coupled defects rather than another
table-fixup rule:

- Livery advertised `white-space: nowrap` but its shorthand expander rejected
  that value, so inherited table-cell text still wrapped.
- Genet-Livery rounded intrinsic cell widths, formatted cell heights, and the
  descendant origin recovered from the algorithm tree. Repeated sibling rows
  accumulated those losses even though Buckram's structural fragments already
  retained the subpixel geometry.
- Once `nowrap` made the spanning-cell intrinsic minimum real,
  `table-colspan-percent-auto.html` exposed a K4c3/K4c4 handoff defect. The
  provisional span increase was applied before a percentage column could
  satisfy it, and percentage tracks used the outer table width rather than the
  assignable grid width after collapsed borders.

The repair accepts `nowrap`, reads unrounded cell measures, places cell
descendants from the committed structural fragment, and records the intrinsic
minimum required by a spanning cell. Used-width selection removes the
provisional span shares, resolves percentages against assignable width, then
reapplies only the unsatisfied span deficit. Focused model and live receipts
cover the percentage/colspan case, three sibling tables in ordinary and
shrink-to-fit block flow, exact table-versus-block glyph positions, and the
subpixel height of a sticky footer group.

### Continuation result

Ledger:
`testing/genet/wpt-ledger/2026-08-23_table_sibling_block_v2` outside the
worktree.

Final runner SHA-256:
`840F44878FA6F1AA09484F571C6CC61C84EA5193A09CA025F4541ACF4AA85B18`.

The exact 059-098 family is now 30 passed and 10 failed. Files 087-090 pass;
the continued slice gained four family tests without losing any accepted
pass. The remaining files and current receipts are:

| Files | Receipt | Owner |
| --- | --- | --- |
| 079-084 | Geometry is visually equal; only glyph-edge pixels differ (`diff=1%`, `max delta=64`). | Generic text antialias composition or a separately ruled harness tolerance. |
| 085-086 | Inferred cells containing block children retain a small text-edge difference (`diff=1%`, `max delta=138`); the WPT notes unresolved first-row wording. | Generic block formatting inside table cells plus WPT/spec ruling. |
| 091-092 | Nested `table` and `inline-table` roles still duplicate or displace text (`diff=2-3%`, `max delta=255`). | Nested table handoff and sizing, K4c/K4d. |

No harness threshold changed. The ordinary sibling-table geometry owner for
079-080 is closed even though the exact reftests remain red under the existing
pixel comparator.

Against the accepted `f23d8eab215` maps:

| Directory | Accepted main | Continuation | Status changes |
| --- | --- | --- | --- |
| `css/CSS2/tables` | 185 pass, 77 fail, 877 skip | 190 pass, 72 fail, 877 skip | `caption-side-applies-to-017` and anonymous-table 087-090 pass; zero losses. |
| `css/css-tables` | 65 pass, 65 fail, 198 skip | 66 pass, 64 fail, 198 skip | `rules-groups.html` passes; zero losses. |

`table-colspan-percent-auto.html` passes under both frozen runners after the
focused correction. It is not a status change and is an explicit regression
control.

### Continuation verification

- `CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p buckram --all-targets
  --no-fail-fast`: 230 passed.
- `CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p livery --all-targets
  --no-fail-fast`: passed.
- `CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p genet-livery
  --all-targets --no-fail-fast`: every unit, integration, and example target
  passed, including all three sibling-table receipts.
- `cargo clippy --offline -p buckram -p livery -p genet-livery --all-targets
  --no-deps`: passed with pre-existing warnings outside the owned diff.
- `cargo build --offline -p genet-wpt --release`: passed from the isolated
  target.
- `git diff --check`: passed.

## Nested inline-table continuation and final ruling

**Status:** implemented and verified on `recovery/table-sibling-block-v2`
from frozen WPT baseline `9b99b661a23`. Unrelated current main `5ec8274ed21`
was fast-forwarded into the branch before commit.

The 091-092 defect was in the atomic subtree handoff. Nested inline tables
advertised the viewport-sized atomic fragment to their enclosing table cell,
while anonymous text leaves still estimated intrinsic width as `0.6em` per
character. Their structural fragments were also copied from rounded algorithm
geometry. The repair exports the table grid's intrinsic pair, shapes anonymous
text with the live `TextSystem` during atomic construction, threads intrinsic
query kind through inline measurement, and publishes unrounded atomic
fragments. Table sizing, structural fragments, and paint now consume the same
geometry.

Focused receipts compare glyph coordinates as multisets because atomic subtree
paint order is not DOM vector order. They cover the exact 091 nested-table
fixture and the 085 inferred-cell block-child fixture. Both structural receipts
pass. The existing three sibling-table receipts also pass.

### Standards ruling for 085-086

The current CSS Tables fixup rule requires one anonymous table cell around each
consecutive sequence of non-cell children of a table row. Buckram does that,
and the exact 085 geometry receipt proves the inferred cell and its block child
match the equivalent authored HTML table. The remaining `diff=1%`,
`max delta=138` is a glyph-edge/compositor residual, not an anonymous-box
construction defect. The implementation follows the current
[CSS Tables fixup algorithm](https://drafts.csswg.org/css-tables/#table-fixup).

The final 059-098 family is 32 passed and 8 failed. Files 091-092 are new exact
passes. The eight residuals are 079-084 at `diff=1%`, `max delta=64` and
085-086 at `diff=1%`, `max delta=138`; all have executable geometry receipts
and a compositor/reftest owner. No harness threshold changed.

Against the frozen accepted runner:

| Directory | Accepted runner | Final runner | Status changes |
| --- | --- | --- | --- |
| `css/CSS2/tables` | 190 pass, 72 fail, 877 skip | 192 pass, 70 fail, 877 skip | Only anonymous-table 091-092 changed, both fail to pass. |
| `css/css-tables` | 66 pass, 64 fail, 198 skip | 66 pass, 64 fail, 198 skip | Exact status map unchanged. |

Ledger:
`testing/genet/wpt-ledger/2026-08-23_anonymous_table_remaining_v1` outside the
worktree.

Baseline runner SHA-256:
`840F44878FA6F1AA09484F571C6CC61C84EA5193A09CA025F4541ACF4AA85B18`.

Final runner SHA-256:
`AB9EC0E6A4636F1FC8933FD05E9C655B72C8CDBD2130C92AC5F485D3E1593D46`.

### Final continuation verification

- `CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked --offline -p buckram -p
  livery --all-targets --no-fail-fast`: passed; Buckram has 230 unit tests.
- `CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked --offline -p genet-livery
  --all-targets --no-fail-fast`: every unit, integration, and example target
  passed, including all five sibling-table receipts.
- `cargo clippy --locked --offline -p buckram -p livery -p genet-livery
  --all-targets --no-deps`: passed with pre-existing warnings outside the
  owned diff.
- `cargo build --locked --offline -p genet-wpt --release`: passed from the
  isolated target.
- `git diff --check`: passed.

## Verification

- `CARGO_PROFILE_TEST_DEBUG=0 cargo test -p buckram --all-targets --offline
  --no-fail-fast`: 229 passed.
- `CARGO_PROFILE_TEST_DEBUG=0 cargo test -p genet-livery --all-targets
  --offline --no-fail-fast`: every unit, integration, and example target
  passed, including the four new anonymous-table receipts.
- `cargo clippy -p buckram -p genet-livery --all-targets --offline --no-deps`:
  passed with three pre-existing warnings in untouched files.
- The strict `-D warnings` variant stops on those same pre-existing
  `useless_format` and `field_reassign_with_default` warnings; it reports no
  warning in the owned diff.
- `cargo build -p genet-wpt --release --offline`: passed.
- `git diff --check`: passed.

## Stop rules

- Stop before changing K4c/K4d table sizing arithmetic unless a focused model
  test proves the current contract cannot represent the required result.
- Stop before changing generic block admission, positioning, text, or harness
  tolerance; record a seam request instead.
- Stop on any unexplained pass-to-fail result in either full table directory.
- Do not cherry-pick or copy the archived lane wholesale. Reconstruct each
  accepted change against the current code and current fixtures.

## Done condition

The family reaches 40/40, or every residual has an executable receipt and a
named owner outside this slice. HTML column backgrounds have a live receipt,
both full table directories have no unexplained losses, affected crate gates
pass, and the result is committed on the recovery branch.
