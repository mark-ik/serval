# Genet's Parley patch

This is a vendored copy of `parley 0.10.0`, wired through the workspace
`[patch.crates-io]` table.

## Join controls do not select a fallback face

Parley's font-coverage pass counts U+200C ZERO WIDTH NON-JOINER and U+200D ZERO
WIDTH JOINER because both are inherited-format characters. Many fonts omit
these default-ignorable controls from `cmap`. A cluster such as `f<ZWNJ>i`
therefore selects a system fallback even when the requested face covers both
visible letters. The fallback face then supplies the run's line metrics.

The patch excludes the two join controls from coverage scoring. It does not
remove them from the segment: `shape/mod.rs` still puts every source character
into the HarfRust buffer, so the controls continue to affect joining and
ligature formation. It also excludes their hidden zero-advance clusters from
letter-spacing, keeping `a<ZWNJ>b` the same width as unligated `ab`.

Retire this fork when an upstream Parley release makes default-ignorable join
controls non-covering during fallback selection while retaining them in the
shaping buffer.

## Receipts

- `support/patches/parley/tests/join_controls.rs` proves ZWNJ keeps the selected
  face and receives no letter spacing.
- `components/genet-livery/tests/k5d_font_feature_resolution.rs` proves that
  `fi`, `f<ZWNJ>i`, and the U+FB01 presentation ligature keep one authored
  face's line metrics, and proves join controls do not add letter spacing.
