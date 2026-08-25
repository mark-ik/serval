# Livery nested-inline positioned containing blocks

**Date:** 2026-08-22

**Status:** Complete on `recovery/k5d-residuals-v2` after `2aa59741b9e`.

**Parent:** [Buckram CSS layout engine plan](2026-07-26_buckram_css_layout_engine_plan.md),
K5d residual closure.

## Ruling

The three `css/css-ruby/abs-in-ruby-*` failures currently exercise ordinary
nested inline boxes: Livery does not parse the ruby display roles used by the
test support sheet. Their visible mismatch is nevertheless a K5 positioning
defect. Wrapping an empty positioned inline ancestor in additional ordinary
inlines moves its absolute child to a later line instead of preserving the
same line-fragment containing block.

This slice fixes that general nested-inline identity. It does not claim ruby
formatting support and does not add partial ruby display keywords.

## Owned files

- `components/genet-livery/src/text.rs`, inline fragment ownership if forced
- `components/genet-livery/src/layout.rs`, positioned inline containing-block
  selection
- `components/genet-livery/tests/k5d_nested_inline_positioning.rs`
- this plan

## Receipts

- The failing-first retained fixture compared the flat inline reference shape
  with the same positioned ancestor under two ordinary inline wrappers. The
  nested absolute box was `[0, 150, 800, 150]` while the flat shape was
  `[0, -50, 33, 150]`; the two shapes are equal after the change.
- `cargo test -p genet-livery --all-targets --offline --no-fail-fast` under
  `CARGO_PROFILE_TEST_DEBUG=0`: all targets green, including 200 library
  tests and both K5d retained fixtures.
- `cargo clippy -p genet-livery --lib --offline --no-deps -- -D warnings`:
  green.
- `git diff --check`: green. Touched-file `rustfmt --check` still reports
  pre-existing formatting hunks in `text.rs:1398-1985`; the owned hunk and
  new fixture are formatted.
- Baseline runner SHA-256:
  `ea599e827abe0b26049463d343ed245c354cb7c84217e00342a389dc1ecaab8f`.
- Candidate runner SHA-256:
  `37d20f174e852f47679b3f467d773b45118d432cc6ecf33c0ea495bcee65893c`.
- Exact WPT comparison:
  - `css/css-ruby`: `40 -> 43` passes. The only changes are
    `abs-in-ruby-base-container`, `abs-in-ruby-base`, and
    `abs-in-ruby-container`, all fail-to-pass.
  - `css/css-position`: `47 -> 47` passes and no status changes.
  - `css/CSS2/abspos`: `14 -> 14` passes and no status changes.
- Candidate receipts and the immutable runner are in
  `testing/genet/wpt-ledger/2026-08-22_k5d_nested_inline_positioning`.

## Stop rules

- Stop if the fix needs ruby layout roles or annotation placement; that is a
  separate feature program.
- Stop if an ordinary inline without its own retained fragment would need a
  synthetic rectangle not grounded in its containing line.
- Stop on any unexplained pass-to-fail result in the owned ratchets.

## Done condition

Met. The nested-inline fixture and all three `abs-in-ruby-*` files are green,
with no loss in `css/css-position` or `css/CSS2/abspos`. The rest of the ruby
directory remains outside this claim: Livery still does not parse or format
ruby display roles.
