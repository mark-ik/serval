# Buckram floats and shapes: rectangular reference-box reconciliation

**Date:** 2026-08-25

**Status:** Row 12 in progress. The horizontal, non-curved reference-box
slice is complete on source commit `6bc7986af41`.

**Parent:** [Buckram and Livery lane program](2026-08-21_buckram_livery_lane_program_plan.md),
row 12.

## Ruling

The inherited row named block float exclusions and `shape-outside`, but did
not distinguish the float's placement rectangle from the shape used by inline
line breaking. Existing Buckram already placed floats, cleared them, kept
independent formatting contexts beside them, and contained their margin boxes.
Replacing that rectangle globally would have regressed those accepted rules.

The first slice therefore adds the bounded box values from
[CSS Shapes Level 1](https://www.w3.org/TR/css-shapes/): `none`, `margin-box`,
`border-box`, `padding-box`, and `content-box`. Buckram retains two geometries
for each float:

- the margin box controls float placement, clearance, independent formatting
  context avoidance, collision bands, and containing-block height; and
- the selected reference box controls inline line exclusion and the next
  wider line band.

Both geometries cross ordinary descendant wrappers in the same coordinate
space. `none` and `margin-box` preserve the existing margin-box line area.
Horizontal non-curved border, padding, and content boxes use their actual box
edges. A selected area is clipped to the margin box for negative-margin edge
cases.

Rounded boxes and vertical float-area transforms deliberately retain the
default margin-box line area in this slice. Basic shapes, paths, images,
gradients, `shape-margin`, and `shape-image-threshold` are not represented.

## Implementation boundary

- Livery parses, cascades, inherits as specified, serializes, and retains the
  five bounded `shape-outside` keywords. The Genet consumed-longhand contract
  is now 129 names, with 102 implemented and 27 named residuals.
- Genet-Livery lowers the computed keyword only at the float-style bridge. A
  nonzero border radius or vertical containing flow selects the documented
  margin-box fallback.
- Buckram's `FloatReferenceBox` is a CSS-facing used-value input. The retained
  float state stores distinct margin-box and line-area rectangles; placement
  code never reads the selected line area.
- Line constraints consult the selected area for the full line-height span,
  including retry at the selected area's block end. Descendant export and
  import translate both rectangles together.

## Focused receipts

The exact pre-change runner is the frozen writing-modes runner built before
this source slice. The candidate runner was built from `6bc7986af41`; that
exact source passes the native checks below.

- baseline runner SHA-256:
  `BD221B64AED36C69B6847A8876A9B3D022702E1FF01D0D660815AFD7F882DF8D`
- candidate runner: `candidate-6bc7986af41-genet-wpt.exe`
- candidate runner SHA-256:
  `838EAA55349D5DEF4FC5E770130C0117E477B8DDB52E595AFF1261DC79EA6C5A`
- identical manifest SHA-256:
  `D5EC5BE9BF1A75ED00D7E7AB28AFE8A694A55E11682BA74305874D70B18DD422`

| Exact subset | Baseline | Candidate | Status movement |
|---|---:|---:|---:|
| `css/css-shapes/shape-outside/shape-box` | 2 pass / 40 fail / 0 skip | 4 pass / 38 fail / 0 skip | 2 fail-to-pass, 0 loss |
| `css/CSS2/floats` | 61 pass / 40 fail / 43 skip | 61 pass / 40 fail / 43 skip | byte-status-identical |
| `css/css-shapes` | 18 pass / 205 fail / 148 skip | 21 pass / 202 fail / 148 skip | 3 fail-to-pass, 0 loss |

The two claimed gains are:

- `shape-outside-border-box-001.html`
- `shape-outside-content-box-001.html`

`supported-shapes/path/shape-outside-path-003.html` also changes from fail to
pass, but is excluded from capability credit. Its unsupported `path()` and
the reference's unsupported `circle()` are both dropped, so this is a parser
coincidence rather than path geometry. Both complete-CSS maps already record
this file as passing, so it is not a full-corpus candidate movement.

Focused map SHA-256 values:

| Map | Baseline SHA-256 | Candidate SHA-256 |
|---|---|---|
| shape box | `C7EB99A310F1E1B7CB91F74931BE8AE92CC2A3D9717BEB541DFC4C6BD81C14BD` | `7B22C52AB87B0936001A30FF2E87B9C233C8D46A8FBF307797A36EBE58C71882` |
| CSS2 floats | `23D23C363E0D33C1B469FC3AAEBB26374A7AD1B3B483DF4FD109EA820EC4DC74` | `47DD1FD107358984077FDBCDB1D6FDD1D7AF10209FBA0962621B8DB3DFB3D390` |
| CSS Shapes | `35231233E654FF1AF34B46B073A85E6388A2589AA011DF1CA60B586C717C61D1` | `2AE036FAE924A73C9FB0ACFE9B7E6F9652EE7E107235C59EA94015F43C4AC70E` |

The exact full-CSS baseline is 7,821 pass, 11,907 fail, 16,583 skip,
and 0 error across 36,311 files. Its SHA-256 is
`5A861142E698100D04DA9C8FBE4236654CCC06D377AA352B7FC56292AB5A9C15`.
The candidate is 7,823 pass, 11,905 fail, 16,583 skip, and 0 error. Its
SHA-256 is
`EF4FFFCC967C549B7410305CD6BF991B46D0E607DD49A66A9DD4F8223500A85A`.
The exact comparison has the same two claimed fail-to-pass results, zero
pass-to-nonpass results, zero added tests, and zero missing tests.

All ledger files and frozen runners are under
`testing/genet/wpt-ledger/2026-08-25_float_shape_boxes`.

## Native wall

- Buckram is 243 / 243, including distinct placement and line-area geometry,
  descendant translation, right-float start-edge selection, and the explicit
  vertical fallback.
- Livery's full tests pass. The consumed-set receipt reports 102 / 129
  implemented and 27 named residuals.
- The live Genet-Livery fixture covers `none`, all four box keywords, unchanged
  float placement, the curved fallback, and zero CSS-facing Taffy block runs.
  The sparse-worktree library run is 205 pass with one unrelated fixture
  failure because its referenced in-repository WPT file is outside the sparse
  checkout.
- Buckram strict Clippy, including tests, passes with warnings denied on the
  exact committed source. Livery and Genet-Livery add no warning in touched
  paths; repository-wide strict Clippy remains blocked by the accepted Livery
  warning set under Rust 1.97.
- Changed-file rustfmt and `git diff --check` pass.

## Remaining row 12 work

The 38 red shape-box files are not one mechanism:

- 24 use rounded reference boxes: 16 horizontal and 8 vertical or sideways;
- 3 use `shape-margin` and require a rounded expanded contour;
- 11 plain rectangular cases combine RTL right floats, inline-block packing,
  explicit line breaks, or atomic inline placement with the now-present box
  geometry; and
- the wider Shapes corpus still lacks basic shapes, paths, image and gradient
  alpha maps, and threshold semantics.

The horizontal rounded reference-box primitive is now complete in the
[rounded reference-box reconciliation](2026-08-25_buckram_rounded_shape_boxes_reconciliation.md),
with eight exact gains and zero loss. Its candidate leaves 30 red shape-box
files: eight horizontal rounded cases coupled to inline/block residuals, eight
vertical or sideways rounded cases, three `shape-margin` cases, and the eleven
plain rectangular cases above. The next coherent slice is their shared
inline/block reconciliation. Vertical contour transforms remain with the row
10/12 boundary, and fragmentation remains K6.

## Done condition

This slice is complete when its native geometry receipts pass on the frozen
candidate, the three focused maps have no loss, and the full-CSS guard has no
unexplained pass-to-nonpass movement. Row 12 remains in progress until the
rounded, margin, contour, image, vertical-transform, and plain inline/block
residuals above are implemented or assigned through current-main receipts.
