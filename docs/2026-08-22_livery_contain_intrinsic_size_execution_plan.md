# Livery positioned `contain-intrinsic-size`

**Date:** 2026-08-22

**Status:** Complete on `recovery/k5d-residuals-v2` after `1e2df6515b0`.

**Parent:** [Buckram K5d positioned aspect-ratio sizing](2026-08-22_buckram_k5d_aspect_ratio_execution_plan.md),
the explicit `abspos-014` follow-on.

## Ruling

`css/css-sizing/aspect-ratio/abspos-014.html` reaches the accepted positioned
aspect-ratio equation with a zero intrinsic inline contribution. Its
`contain-intrinsic-size: 500px 500px` declaration is currently rejected, so
size containment has no substitute size to transfer through the ratio before
the 100% maximum height clamps it.

This slice admits the bounded `none | <length>{1,2}` physical pair and feeds
the contained inline-axis substitute to Buckram through the existing
`PositionedBoxInput::intrinsic_inline` boundary. It does not claim the full
`auto` grammar or general normal-flow contain-intrinsic sizing.

## Owned files

- `components/livery/properties.toml`
- `components/livery/build.rs`
- `components/livery/src/values/property.rs`
- `components/livery/src/values/mod.rs`
- `components/livery/tests/values.rs`
- `components/genet-livery/src/layout.rs`, positioned intrinsic input only
- `components/genet-livery/tests/k5d_aspect_ratio.rs`
- this plan

## Required receipts

- The retained document fixture must fail first at a zero-sized positioned
  fragment, then produce the required 100px square.
- Livery value tests must cover `none`, one/two-value serialization, relative
  length resolution, and invalid negative or overlong input.
- All Buckram, Livery, and Genet-Livery gates affected by the bridge must stay
  green.
- A source-built release runner must move `abspos-014.html` to pass and compare
  the full `css/css-sizing` directory against the previous immutable runner.

## Stop rules

- Stop before accepting `auto` unless its remembered-size state has a real
  document owner.
- Keep the substitute size as an explicit intrinsic input. The scratch
  formatter does not own positioned constraint transfer.
- Stop on an unexplained pass-to-fail result in the sizing comparison.

## Done condition

The retained fixture and `abspos-014.html` are green, the bounded property is
round-tripped and resolved, and the sizing directory has an accounted delta.

## Receipts

- Failing first: the retained document fixture produced `[0, 0, 0, 0]` for
  the positioned fragment. With the explicit substitute intrinsic input it
  produces `[0, 0, 100, 100]` after the existing ratio/max-height transfer.
- The Livery value receipt covers `none`, compressed one-value and distinct
  two-value serialization, viewport-relative physical-axis resolution, and
  rejection of `auto`, negative lengths, and a third component.
- `cargo test -p buckram --all-targets --offline --no-fail-fast`: 225/225
  library tests green.
- `cargo test -p livery --all-targets --offline --no-fail-fast`: all targets
  green, including 35 value tests and the generated catalog/property-space
  contracts.
- `CARGO_PROFILE_TEST_DEBUG=0 cargo test -p genet-livery --all-targets
  --offline --no-fail-fast`: all targets green, including 200 library tests
  and both retained K5d aspect-ratio fixtures.
- Strict Genet-Livery library Clippy is green. Strict Livery library Clippy is
  still blocked by the same 146 pre-existing Rust 1.97 findings outside the
  owned files.
- The immutable baseline runner is
  `1f3c0f5af8f2ec03f89509b65c64322c7e263ac74a3fe0d0249d4a9570357b7a`.
  The source-built candidate runner is
  `179dfd4d65bc88245ae12958b94abcbe109f86f67f4cf64d3a501419474df0ba`.
  The candidate was copied from Cargo's configured
  `C:/t/graphshell-target/release` directory before measurement; a stale
  adjacent target-path copy was caught by its unchanged hash and replaced.
- `css/css-sizing/aspect-ratio/abspos-014.html` is 1/1 green. Across all 732
  `css/css-sizing` files, the baseline is 212 pass, 300 fail, and 220 skip;
  the candidate is 213 pass, 299 fail, and 220 skip. `abspos-014.html` is the
  only status change, from fail to pass.
- Exact JSON, stdout, and the immutable candidate runner are under
  `testing/genet/wpt-ledger/2026-08-22_k5d_contain_intrinsic_size`.
- `git diff --check` is green. Workspace-wide `cargo fmt --all -- --check`
  remains blocked by unrelated pre-existing formatting drift; all formatting
  findings in the new owned hunks were corrected.
