# Livery fonts and WOFF2 reconciliation

**Date:** 2026-08-24
**Status:** Complete on a clean `origin/main` base.

## Scope

Lane 16 reconciles the exact `css/WOFF2` and `css/css-fonts` directories.
The implementation closes two live seams:

- Livery retains an ordered `font-family` list as exact CSS source and passes
  that list to Parley's existing ordered family selector; and
- Genet-Livery validates and converts WOFF2 resources through Fontsan before
  registering the resulting SFNT bytes with Fontique.

The host still owns URL resolution and resource bytes. Livery still does not
claim full `@font-face` descriptor matching, WOFF1, font synthesis, metric
overrides, palette selection, or the broader `font-variant-*` surface.

## Frozen inputs

| Input | Identity |
|---|---|
| accepted source base | `a057853571855e93fc833b07f6dfe417de4d4ce3` |
| WPT manifest | `d5ec5be9bf1a75ed00d7e7ab28afe8a694a55e11682ba74305874d70b18dd422` |
| baseline runner | `9216d1193c8c7898caf32d2af2806c993a4fc7547070874a0e0e4e1094d0659d` |
| final runner | `eae886ff5ce3ef51b004078700415e26d2d8546f5e36f9cf289a003bf7152725` |
| external ledger | `testing/genet/wpt-ledger/2026-08-24_fonts_woff2` |

Both runners were copied into the external ledger before use. Both directories
were run against the same manifest and exact expectation maps.

## Exact WPT result

| Directory | Baseline pass / fail / skip | Final pass / fail / skip | Fail to pass | Pass to fail |
|---|---:|---:|---:|---:|
| `css/WOFF2` | 0 / 298 / 2 | 292 / 6 / 2 | 292 | 0 |
| `css/css-fonts` | 240 / 90 / 209 | 255 / 75 / 209 | 15 | 0 |
| **Total** | **240 / 388 / 211** | **547 / 81 / 211** | **307** | **0** |

The WOFF2 zero-pass baseline was not primarily a decoder failure. All 298
reftests use an ordered primary-plus-fallback family list, and Livery rejected
every such declaration before shaping. Retaining that list exposes the actual
format boundary. Fontsan then rejects malformed WOFF2 inputs and converts
accepted inputs to the SFNT bytes Fontique already consumes.

The 15 `css-fonts` gains are the `font-family-name` cases 001-006 and 009-012,
plus `font-palette-31.html`, `font-size-adjust-014.html`,
`quoted-generic-ignored.html`, `system-ui-ar.html`, and `system-ui-ur.html`.
They are direct ordered-family-selection repairs. There are no directory
losses.

## WOFF2 residuals

The six remaining failures are all localized font-output differences:

- `directory-knowntags-001.xht` and `tabledata-glyf-bbox-001.xht` are valid
  inputs whose sanitized reconstruction differs from the reference pixels;
- `header-totalsfntsize-001.xht` and the three
  `tabledata-glyf-origlength-00{1,2,3}.xht` cases are malformed inputs which
  the current Fontsan/OTS boundary still accepts.

These are upstream sanitizer/reconstruction limits, not unresolved resource
discovery or family-selection failures. The other 292 files cover valid CFF
and TTF inputs plus malformed headers, directories, Brotli streams, table
transforms, metadata, and private data.

## css-fonts attribution

The 75 final failures divide at the runner boundary:

- 26 `mismatch-eq` results are identical mismatch test/reference renders and
  belong to the open harness/ledger row;
- 10 local results require fuller `@font-face` descriptor and family matching;
- 10 require alternates, shaping features, kerning, or additional
  `font-variant-*` values;
- 13 require font metrics, size adjustment, or math-script behavior;
- 14 require synthesis, weight/style selection, or variation-axis matching;
  and
- 2 require the standard generic-family mapping surface.

This accounts for every current `css-fonts` failure without treating the 26
identical mismatch comparisons as rendering support.

## Native wall

- ordered family parsing and exact serialization: focused Livery receipt
  passed;
- malformed WOFF2 rejection and unchanged SFNT pass-through: focused
  Genet-Livery receipts passed;
- `cargo check -p genet-livery`: passed;
- `cargo test -p livery --all-targets --offline --no-fail-fast`: 182 passed
  and 4 ignored;
- `cargo test -p genet-livery --all-targets --offline --no-fail-fast`: 409
  passed and 6 ignored;
- scoped all-target strict Clippy passed with the accepted-main
  formatting-borrow, precision, question-mark, self-convention, test-default,
  and test-format allowances named on the command line;
- `cargo build -p genet-wpt --release --offline`: passed from the isolated
  target before the final runner was copied; and
- direct formatting checks for all three changed Rust files plus
  `git diff --check` passed. The package-wide formatter check still reports
  accepted-main drift in untouched files.
