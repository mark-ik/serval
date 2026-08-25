# Buckram floats and shapes: rounded reference-box reconciliation

**Date:** 2026-08-25

**Status:** Row 12 in progress. The horizontal rounded reference-box slice is
complete on source commit `8de690e3427`.

**Parent:** [Buckram and Livery lane program](2026-08-21_buckram_livery_lane_program_plan.md),
row 12. This continues the [rectangular reference-box reconciliation](2026-08-25_buckram_float_shape_boxes_reconciliation.md).

## Ruling

A box-valued `shape-outside` follows the selected box's rounded contour. The
float's margin box still controls placement, clearance, independent formatting
context avoidance, collision bands, and containing-block height. Only inline
line exclusion follows the contour.

For the box values admitted by the first slice, Buckram now resolves circular
corner radii against the used border box and derives border-, padding-,
content-, or margin-box radii. The positive-margin rule follows
[CSS Shapes Level 1](https://www.w3.org/TR/css-shapes/): the margin-box corner
radius is the border radius plus the margin when the border radius is positive,
with the cubic interpolation used when the border radius is zero. The resulting
curve is clipped to the margin box, after the curve itself is constructed, so
negative margins cannot turn a curved reference box into a false rectangle.

Line constraints use the conservative union of the contour across the full
line-height span. When an unbreakable line does not fit beside one active
rounded contour, Buckram searches for the first lower contour band that admits
the required advance, capped to the containing width. Rectangular-only retries
retain exact boundary behavior.

## Implementation boundary

- Livery continues to own the five bounded `shape-outside` keywords. Its
  current radius syntax represents circular radii; CSS slash-separated
  elliptical pairs remain deferred.
- Genet-Livery lowers the four physical circular corner radii with the selected
  reference box. A nonlinear math expression in a consumed radius keeps
  Buckram authoritative but uses the default margin-box line area.
- Buckram retains a rectangular or rounded line area separately from the
  placement margin box. It resolves percentages against the used border-box
  dimensions, normalizes overlapping radii, transforms radii for the selected
  reference box, and samples the two inline edges independently.
- Descendant export and import translate the curve and its clip together.
- Vertical and sideways flows retain the default margin-box line area.
- A single active rounded contour receives the continuous retry search. Several
  simultaneous rounded contours retry at complete contour ends until a combined
  moving-envelope solver exists.
- Basic shapes, paths, images and gradients, `shape-margin`, and
  `shape-image-threshold` remain outside this slice.

## Exact WPT receipts

The baseline is the frozen runner and maps from rectangular source commit
`6bc7986af41`. The candidate runner was built locked and offline from
`8de690e3427`, then copied before any receipt documentation changed the tree.

- candidate runner: `candidate-8de690e3427-genet-wpt.exe`
- candidate runner SHA-256:
  `DE364B68CE02C4BF41E1ABC0E24CC2DF594D01BBD5F315EE277C059771838614`
- identical manifest SHA-256:
  `D5EC5BE9BF1A75ED00D7E7AB28AFE8A694A55E11682BA74305874D70B18DD422`

| Exact subset | Baseline | Candidate | Status movement |
|---|---:|---:|---:|
| `css/css-shapes/shape-outside/shape-box` | 4 pass / 38 fail / 0 skip | 12 pass / 30 fail / 0 skip | 8 fail-to-pass, 0 loss |
| `css/css-shapes` | 21 pass / 202 fail / 148 skip | 29 pass / 194 fail / 148 skip | 8 fail-to-pass, 0 loss |
| `css/CSS2/floats` | 61 pass / 40 fail / 43 skip | 61 pass / 40 fail / 43 skip | status-identical |
| full `css` | 7,823 pass / 11,905 fail / 16,583 skip | 7,831 pass / 11,897 fail / 16,583 skip | 8 fail-to-pass, 0 loss |

The eight gains are:

- `shape-outside-border-box-border-radius-001.html`
- `shape-outside-border-box-border-radius-002.html`
- `shape-outside-content-box-border-radius-001.html`
- `shape-outside-margin-box-border-radius-001.html`
- `shape-outside-margin-box-border-radius-002.html`
- `shape-outside-margin-box-border-radius-005.html`
- `shape-outside-margin-box-border-radius-006.html`
- `shape-outside-padding-box-border-radius-001.html`

The complete-CSS comparison covers 36,311 identities. Exactly those eight
changed; there are zero pass-to-nonpass results, zero added or missing tests,
and zero other status changes.

| Map | Baseline SHA-256 | Candidate SHA-256 |
|---|---|---|
| shape box | `7B22C52AB87B0936001A30FF2E87B9C233C8D46A8FBF307797A36EBE58C71882` | `F5B285E75DFFF835C90D5CBC42565DECC625F2E8FF6186E23F577D4D6E171E49` |
| CSS Shapes | `2AE036FAE924A73C9FB0ACFE9B7E6F9652EE7E107235C59EA94015F43C4AC70E` | `CDCAEDB9697B4DF8FA75D04C0CBD6C5874A7D37CA3D7FB09C9E802A5938252B9` |
| CSS2 floats | `47DD1FD107358984077FDBCDB1D6FDD1D7AF10209FBA0962621B8DB3DFB3D390` | `475CA34D58B6EFB1E641426E70D66594ED7BB4FB4DDDAA8B35DAEA444D48FA97` |
| full CSS | `EF4FFFCC967C549B7410305CD6BF991B46D0E607DD49A66A9DD4F8223500A85A` | `32C1EC5C53ACF53C9CAF237864E60731688CE9B1BA5725114D42371D80265779` |

All maps and frozen runners are under
`testing/genet/wpt-ledger/2026-08-25_rounded_shape_boxes`.

## Native wall

- Buckram is 249 / 249. The receipts cover left and right contour edges,
  asymmetric corners, percentage normalization, margin-box radius expansion,
  negative-margin clipping, descendant translation, contour retry, and the
  multi-contour fallback.
- Buckram strict Clippy passes for library and tests with warnings denied.
- Livery's full tests pass. Genet-Livery's library run has 209 passes and one
  unrelated failure whose referenced in-repository WPT fixture is absent from
  the sparse worktree.
- Live Genet-Livery receipts cover both float sides, nonlinear-radius fallback
  with zero CSS-facing Taffy block runs, and an unbreakable line retry inside a
  rounded bottom contour.
- Changed-file rustfmt and `git diff --check` pass. Repository-wide strict
  Genet-Livery Clippy remains obscured by the accepted Livery warning baseline
  under Rust 1.97; the touched Buckram surface has the clean strict gate.

## Remaining row 12 work

The subsequent [relative float-state reconciliation](2026-08-25_buckram_relative_float_state_reconciliation.md)
repairs the seven explicit-break rectangular cases with zero loss. The exact
shape-box map now has 23 red files:

- 4 rectangular cases use `line-height: 0` with atomic inline children;
- 8 horizontal rounded cases share that provisional atomic-line height seam;
- 8 vertical or sideways rounded cases need a vertical contour transform; and
- 3 use `shape-margin` and need an expanded contour.

The next coherent slice is the atomic inline line-height reconciliation. It can
address the four rectangular and eight horizontal rounded cases without
widening the vertical-flow or shape-expansion claims.

## Done condition

This slice is complete because the exact native source passes its focused wall,
all three focused maps have no loss, and the complete-CSS map changes only the
eight claimed tests. Row 12 remains in progress until the residual families
above, plus wider basic-shape and image semantics, are implemented or assigned
through current-main receipts.
