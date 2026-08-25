# Livery `pre-wrap` leading-space breaks

**Date:** 2026-08-22

**Status:** Complete on `recovery/k5d-residuals-v2` after `f3bd8a3e80e`.

**Parent:** [Buckram CSS layout engine plan](2026-07-26_buckram_css_layout_engine_plan.md),
K5d residual closure.

## Ruling

`css/css-text/white-space/pre-wrap-leading-spaces-014.html` currently fails
before line breaking. Livery rejects the test's `width: 5ch`, so the box fills
the viewport and Parley never receives the intended 100px line constraint.
After a forced newline, the leading preserved space must remain on its own
line when the following word exactly fills that constraint.

This slice admits `ch` as a deferred font-relative length and resolves it from
the shaped `0` advance in the retained document's actual font collection. It
then verifies the existing `pre-wrap` break behavior under the correct width.
It must preserve the authored text and byte ranges used by retained selection.

## Owned files

- `components/livery/src/values/length.rs`
- `components/livery/src/values/calc.rs`
- `components/livery/tests/values.rs`
- `components/genet-livery/src/document.rs`
- `components/genet-livery/src/style.rs`
- `components/genet-livery/src/text.rs`
- `components/genet-livery/tests/k5d_pre_wrap_leading_spaces.rs`
- this plan

## Required receipts

- A failing-first retained paint fixture proving the post-newline glyph
  distribution, including a span boundary adjacent to preserved spaces.
- All `genet-livery` targets and strict library Clippy green.
- A release runner with an exact WPT comparison for the named file and the
  adjacent `pre-wrap-leading-spaces-013.html` and `-015.html` cases.
- No unexplained pass-to-fail result in the `css/css-text/white-space`
  comparison set.

## Stop rules

- Stop if the change requires inserting shaping characters without an exact
  mapping back to authored byte ranges.
- Stop if the mechanism changes `break-spaces`, collapsed whitespace, or
  `white-space: nowrap` behavior without dedicated tests.
- Stop if the fix belongs in Parley and cannot be expressed through its public
  layout API without carrying a maintained dependency patch.

## Done condition

The retained fixture and `pre-wrap-leading-spaces-014.html` are green, the
adjacent cases have an accounted result, and the full Livery gate stays green.

## Receipts

- Failing first: the retained fixture reported `5ch Ahem width: 320` because
  the declaration was rejected and the box filled its viewport. With the
  shaped metric it reports 100px, and the word baseline is two 20px lines
  below the first glyph. The intervening preserved space owns its line across
  the authored span boundary.
- `cargo test -p livery --all-targets --offline --no-fail-fast` is green. The
  new value receipt proves that `ch` remains deferred without a font metric
  and that `calc(2ch + 1px)` resolves to 25px for a 12px zero advance.
- `CARGO_PROFILE_TEST_DEBUG=0 cargo test -p genet-livery --all-targets
  --offline --no-fail-fast` is green, including 200 library tests and the K5d
  retained fixtures.
- Strict Genet-Livery library Clippy is green. Strict Livery library Clippy is
  blocked by 146 pre-existing Rust 1.97 lint findings outside the owned files;
  the build and all Livery test targets compile the owned code cleanly.
- The release `genet-wpt` runner is
  `1f3c0f5af8f2ec03f89509b65c64322c7e263ac74a3fe0d0249d4a9570357b7a`.
  The immutable baseline runner is
  `37d20f174e852f47679b3f467d773b45118d432cc6ecf33c0ea495bcee65893c`.
- The named `pre-wrap-leading-spaces-014.html` run is 1/1 green. In the full
  452-file `css/css-text/white-space` comparison, the baseline is 191 pass,
  220 fail, and 41 skip; the candidate is 231 pass, 180 fail, and 41 skip.
  The result is stable across a candidate repeat. Cases 013, 014, and 015 all
  move from fail to pass.
- The broad delta contains 56 fail-to-pass and 16 pass-to-fail results. Every
  loss uses `ch`; those tests previously ran without their authored finite
  width or margin. Nine expose the existing trailing-ideographic-space gap,
  three expose balance or line-clamp gaps, and four expose the existing bidi,
  line-edge, float, and nowrap gaps. A dump of
  `trailing-ideographic-space-017.html` confirms the disposition: the old
  unconstrained render matched accidentally, while the correctly constrained
  render reveals red fallback content and incomplete hanging-space behavior.
  These are corrected false greens, not changes to those text algorithms.
- Exact JSON, stdout, immutable runners, and the diagnostic dump are under
  `testing/genet/wpt-ledger/2026-08-22_k5d_pre_wrap_leading_spaces`.
- `git diff --check` is green. Workspace-wide `cargo fmt --all -- --check`
  remains blocked by pre-existing formatting drift across unrelated files; the
  only new owned formatting finding was corrected.
