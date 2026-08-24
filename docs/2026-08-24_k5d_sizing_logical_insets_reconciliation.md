# K5d sizing and logical-inset current-main reconciliation

**Date:** 2026-08-24

**Status:** Complete on current main.

**Parent:** [Buckram and Livery lane program](2026-08-21_buckram_livery_lane_program_plan.md),
row 5+6, and [Buckram K5d absolute and fixed used geometry](2026-08-10_buckram_k5d_absolute_fixed_execution_plan.md).

## Ruling

The inherited row combined five already-landed repairs with a stale shapes
count. Current main passes all eight named aspect-ratio, nested-inline,
font-feature, and pre-wrap files. Logical sizing and inset longhands also reach
the retained layout bridge in horizontal and vertical flows.

The shapes bucket is not a K5d residual. The old count of 33 resolves to 36
current affected `shape-outside` files: the original 32 plus four failures
made honest when their references began rendering logical longhands. All 36
remain red because shape exclusion itself is absent. That implementation
belongs to lane 12, while the nine native logical-inset receipts are green.

No source repair belongs in this reconciliation.

## Named inventory

All eight current-main reftests pass:

- `css/css-sizing/aspect-ratio/abspos-008.html`
- `css/css-sizing/aspect-ratio/abspos-014.html`
- `css/css-ruby/abs-in-ruby-base.html`
- `css/css-ruby/abs-in-ruby-base-container.html`
- `css/css-ruby/abs-in-ruby-container.html`
- `css/css-fonts/font-feature-resolution-001.html`
- `css/css-fonts/font-feature-resolution-002.html`
- `css/css-text/white-space/pre-wrap-leading-spaces-014.html`

These receipts preserve their narrow claims. They do not claim general ruby,
font, white-space, aspect-ratio, or writing-mode conformance.

## Current directory ledger

| Subset | Pass | Fail | Skip | Error |
|---|---:|---:|---:|---:|
| `css/css-sizing/aspect-ratio` | 120 | 137 | 31 | 0 |
| `css/css-ruby` | 43 | 40 | 48 | 0 |
| `css/css-fonts` | 240 | 90 | 209 | 0 |
| `css/css-text/white-space` | 226 | 185 | 41 | 0 |
| `css/css-shapes/shape-outside` | 56 | 158 | 77 | 0 |
| `css/css-position` | 56 | 62 | 226 | 0 |
| `css/CSS2/abspos` | 15 | 9 | 7 | 0 |
| `css/css-writing-modes` | 269 | 844 | 255 | 0 |

The exact 36-file shape list is 36 fail, 0 pass, and 0 missing in the current
shape-outside map. Its references use the now-implemented `block-size`,
logical inset, and logical margin properties; the test sides still require
lane 12's shape exclusions.

## Receipts

- The frozen runner SHA-256 is
  `E670FF76C2E392FA5B7C55C11E898427CADD6CA6887976C42E98A57BEBAFE617`.
  It is reused from the K5 positioning closure only after a non-document source
  diff proved current main byte-source-identical to its `e8db57141f1` build.
- The regenerated locked dependency graph SHA-256 is
  `6F91D0C46D3BF171137B16023C04C5A8032B405CD10D321E6D41459CAD4EBE49`.
- The five focused Genet-Livery targets are green: two aspect-ratio, two
  font-feature, one nested-inline, one pre-wrap, and nine logical-inset tests.
- The focused Livery containment and font cascade tests are green.
- `cargo test --locked --offline -p livery -p buckram -p genet-livery
  --all-targets --no-fail-fast` is green, including all 236 Buckram tests.
- Strict `--no-deps` library Clippy is green for Buckram and Genet-Livery.
- Workspace `cargo fmt --all -- --check` remains blocked by accepted formatting
  drift outside this documentation-only lane.
- Exact JSON maps, stdout, named results, shape verdicts, lockfile, and runner
  provenance are under
  `testing/genet/wpt-ledger/2026-08-24_k5d_sizing_vertical_insets_v2`.

## Done condition

Row 5+6 is closed when the eight named files remain green, logical inset
ownership is proved natively, and every red shape reference is assigned to the
open shape-exclusion lane. Those conditions are met.
