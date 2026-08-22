# Buckram K5d positioned aspect-ratio sizing

**Date:** 2026-08-22

**Status:** Complete on `recovery/k5d-residuals-v2`, cut from the accepted K5
recovery plus current committed `main`.

**Parent:** [Buckram CSS layout engine plan](2026-07-26_buckram_css_layout_engine_plan.md),
K5d residual closure.

## Ruling

The archived K5d lane is diagnostic material, not a patch source. This slice
implements the smallest standards-owned mechanism forced by
`css-sizing/aspect-ratio/abspos-008`: a non-replaced positioned box couples
its automatic axes through its preferred aspect ratio, and a block-axis
constraint transfers to the ratio-dependent inline axis.

`abspos-014` also requires `contain-intrinsic-size`, which is not currently a
Livery computed value or a Buckram input. That separate seam is not hidden in
this change.

## Starting receipt

The accepted runner at `2a4792cc208` reports both `abspos-008` and
`abspos-014` as failures. The other six named K5 residuals are also still
red; they belong to ruby positioning, text shaping, and fractional static
position rather than this equation.

## Owned files

- `components/buckram/src/positioning.rs`
- `components/buckram/src/taffy_adapter.rs`, positioned intrinsic admission
- `components/genet-livery/src/layout.rs`, positioned size feedback only if
  the formatter needs Buckram's solved block size
- `components/genet-livery/tests/k5d_aspect_ratio.rs`
- this plan

## Receipts

- Failing-first Buckram unit tests observed `20` instead of `30` for a
  ratio-derived block size and `200` instead of `100` for a transferred
  block-axis maximum. Both are green after the change.
- The failing-first retained Livery fixture observed `[0, 0, 100, 20]`
  instead of `[0, 0, 100, 100]`. It is green after the change.
- `cargo test -p buckram --offline`: `225 passed`.
- `cargo test -p genet-livery --all-targets --offline --no-fail-fast` under
  `CARGO_PROFILE_TEST_DEBUG=0`: all targets green, including 200 library
  tests and the retained K5d fixture.
- `cargo clippy -p buckram -p genet-livery --lib --offline --no-deps -- -D warnings`:
  green.
- `git diff --check`: green. Touched-file `rustfmt --check` still reports two
  pre-existing formatting hunks outside this slice in `positioning.rs:414`
  and `taffy_adapter.rs:2668`; the new fixture and owned hunks are formatted.
- Accepted runner SHA-256:
  `b2818a6329a4e4e5dd69d5a8614f9a5308d0e75cf31574375368093e1f68a570`.
- Candidate runner SHA-256:
  `ea599e827abe0b26049463d343ed245c354cb7c84217e00342a389dc1ecaab8f`.
- Exact WPT comparison:
  - `css/css-sizing`: `208 -> 212` passes. The four gains are
    `abspos-003`, `abspos-008`, `abspos-009`, and `abspos-021`; there are no
    losses.
  - `css/css-position`: `47 -> 47` passes and no status changes.
  - `css/CSS2/abspos`: `14 -> 14` passes and no status changes.
- Candidate receipts and the immutable runner are in
  `testing/genet/wpt-ledger/2026-08-22_k5d_aspect_ratio`.

## Stop rules

- Stop before implementing `contain-intrinsic-size` or general normal-flow
  aspect-ratio sizing.
- Stop on an unexplained pass-to-fail result in an owned ratchet.
- Keep the formatter as a content provider; it does not select the containing
  block, static rectangle, or final positioned geometry.

## Done condition

Met. `abspos-008` passes through the source-built runner, the retained
document fixture and Buckram equations are green, and the owned ratchets have
no losses. `abspos-014` remains red and explicitly assigned to
`contain-intrinsic-size`.
