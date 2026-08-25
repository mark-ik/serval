# Livery font-feature resolution

**Date:** 2026-08-22

**Status:** Complete on `recovery/k5d-residuals-v2` after `0a521fe414f`.

**Parent:** Buckram/Livery K5 residual closure, the final two accepted red
receipts.

## Ruling

`css/css-fonts/font-feature-resolution-001.html` and `-002.html` reach
Parley's shaping route, but Livery currently rejects
`font-variant-ligatures` and `font-feature-settings`, drops `@font-face`
descriptors, and registers downloaded bytes only under the font's internal
family name. Parley and Fontique already expose the required feature and
family-override inputs.

The first candidate receipt exposed a second defect: Parley 0.10 counts ZWNJ
and ZWJ when scoring font coverage. The WPT face has no ZWNJ `cmap` entry, so
`f<ZWNJ>i` moved to a system fallback and acquired different line metrics. A
narrow vendored Parley patch makes join controls non-covering while retaining
them in the HarfRust shaping buffer.

This slice retains CSS font-face family, URL, and feature descriptors; admits
the two inherited feature properties; registers host-supplied bytes under the
authored family; and resolves the tested precedence order:

`font default < @font-face < font-variant-ligatures < letter-spacing < font-feature-settings`.

The first property lane covers the ligature keywords exercised by the two
receipts. Other `@font-face` descriptors, source selection conditions, and
the remaining `font-variant-*` properties stay explicit follow-ons. Rules
containing unimplemented descriptors remain visible to CSSOM but are not
registered as unconditional faces, which would misapply their bytes.

## Owned files

- `components/livery/properties.toml`
- `components/livery/consumed_longhands.toml`
- `components/livery/build.rs`
- `components/livery/src/values/property.rs`
- `components/livery/src/values/mod.rs`
- `components/livery/src/stylesheet.rs`
- focused Livery tests
- `components/genet-livery/src/style.rs`
- `components/genet-livery/src/document.rs`
- `components/genet-livery/src/text.rs`
- focused Genet-Livery tests
- `Cargo.toml`
- `support/patches/parley`
- this plan

## Required receipts

- Preserve immutable failing runs for both named WPT files.
- Prove property parsing, inheritance, cascade, font-face descriptor parsing,
  authored-family registration, and the five-level feature precedence with
  focused tests.
- Keep all Livery and Genet-Livery targets green.
- Build and copy a fresh release runner, move both named files to pass, and
  compare the full `css/css-fonts` directory against the immutable baseline.

## Stop rules

- Stop before claiming full `@font-face` support. This slice does not select
  by weight, style, stretch, unicode range, `local()`, or `format()`.
- Do not move URL resolution into Livery. The document host continues to own
  resource resolution and bytes.
- Stop on an unexplained pass-to-fail result in the fonts comparison.

## Done condition

Both retained precedence fixtures and both named WPT files are green, the
directory delta is accounted, and the affected crate gates pass.

## Receipts

The immutable release runners and logs are under
`testing/genet/wpt-ledger/2026-08-22_k5d_font_feature_resolution` outside the
repository worktree.

- Baseline runner SHA-256:
  `179DFD4D65BC88245AE12958B94ABCBE109F86F67F4CF64D3A501419474DF0BA`.
- Final runner SHA-256:
  `1A65E49F7BC7E643993E73C13DE78EE0F57B7C6F15155F21B4690AC3BE9EAA63`.
- `font-feature-resolution-001.html`: fail at baseline, pass in the final full
  directory run.
- `font-feature-resolution-002.html`: fail at baseline, pass in the final full
  directory run.
- Full `css/css-fonts` baseline: 187 passed, 143 failed, 209 skipped, 0
  errored, 539 files.
- Full `css/css-fonts` final: 240 passed, 90 failed, 209 skipped, 0 errored,
  539 files.
- Delta: 60 fail-to-pass and 7 pass-to-fail, for a net 53 additional passes.

The seven pass-to-fail files are:

- `font-synthesis-position-001.html`
- `font-synthesis-style-oblique-only.html`
- `font-variant-emoji-003.html`
- `metrics-override-normal-keyword.html`
- `variations/font-slant-2a.html`
- `variations/font-slant-2b.html`
- `variations/font-slant-2c.html`

These are accounted exposure changes. The baseline used system fallback faces
and sometimes passed through the runner's small-difference tolerance. Correct
authored-family loading makes reference pages use their intended faces. The
test pages still depend on unimplemented synthesis position/style, emoji
variant, metrics override, or style/slant range selection. Livery retains such
face rules for CSSOM but does not register their bytes as an unconditional
face.

Affected gates:

- Vendored Parley join-control integration test: 1 passed.
- Focused Genet-Livery font-feature integration tests: 2 passed.
- `cargo test -p livery --all-targets --offline --no-fail-fast`: all targets
  passed, 178 tests passed and 4 ignored.
- `cargo test -p genet-livery --all-targets --offline --no-fail-fast`: all
  targets passed, including 200 library tests and every integration target.
- Adjacent `script-runtime-api` and `genet-scripted` all-target tests: 160
  passed.
- `cargo clippy --no-deps -p livery -p genet-livery --all-targets --offline`:
  passed. Strict `-D warnings` remains red on 146 pre-existing Livery warnings;
  the only warning introduced by this slice was fixed.
- `cargo build -p genet-wpt --release --offline`: passed before the final
  runner was copied.
