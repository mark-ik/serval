# Buckram floats and shapes: relative float-state reconciliation

**Date:** 2026-08-25

**Status:** Row 12 in progress. The relative normal-flow and forced-break
slice is complete on source commit `ca57c615a45`.

**Parent:** [Buckram and Livery lane program](2026-08-21_buckram_livery_lane_program_plan.md),
row 12. This continues the [rounded reference-box reconciliation](2026-08-25_buckram_rounded_shape_boxes_reconciliation.md).

## Ruling

A relatively positioned block remains in its parent's block formatting
context. Buckram therefore carries inherited and descendant float state through
that block before Livery applies the retained visual offset to its fragment
subtree. Absolute and fixed boxes remain out of flow. Sticky positioning is not
admitted by this slice.

The new admission exposed two existing float-boundary defects, both repaired at
their owning seams:

- a zero-height float excludes only a line that strictly straddles its block
  position; a line beginning at the same position is not narrowed;
- an auto-width float or atomic inline root includes fixed-size leaf descendants
  in its intrinsic inline contribution. Positioned intrinsic queries retain
  their separate empty-leaf contract.

These boundaries let direct text after forced breaks receive the selected
`shape-outside` line band inside a relative wrapper without changing relative
offset ownership or widening positioned intrinsic sizing.

## Exact WPT receipts

The baseline maps and frozen runner come from rounded source commit
`8de690e3427`. The candidate runner was built locked and offline from
`ca57c615a45`, then copied before this receipt document changed the tree.

- candidate runner: `candidate-ca57c615a45-genet-wpt.exe`
- candidate runner SHA-256:
  `F3BA3DA7590EBD61617EAE2766496DE08B53C05D52786A4FF9590326AA7ADEE7`
- identical manifest SHA-256:
  `D5EC5BE9BF1A75ED00D7E7AB28AFE8A694A55E11682BA74305874D70B18DD422`

| Exact subset | Baseline | Candidate | Status movement |
|---|---:|---:|---:|
| `css/css-shapes/shape-outside/shape-box` | 12 pass / 30 fail / 0 skip | 19 pass / 23 fail / 0 skip | 7 fail-to-pass, 0 loss |
| `css/css-shapes` | 29 pass / 194 fail / 148 skip | 36 pass / 187 fail / 148 skip | 7 fail-to-pass, 0 loss |
| `css/CSS2/floats` | 61 pass / 40 fail / 43 skip | 62 pass / 39 fail / 43 skip | 1 fail-to-pass, 0 loss |
| `css/CSS2/floats-clear` | 96 pass / 115 fail / 38 skip | 98 pass / 113 fail / 38 skip | 2 fail-to-pass, 0 loss |
| `css/css-position` | 45 pass / 73 fail / 226 skip | 45 pass / 73 fail / 226 skip | status-identical |
| full `css` | 7,831 pass / 11,897 fail / 16,583 skip | 7,852 pass / 11,876 fail / 16,583 skip | 21 fail-to-pass, 0 loss |

The seven shape-box gains are:

- `shape-outside-box-002.html`
- `shape-outside-box-003.html`
- `shape-outside-box-004.html`
- `shape-outside-box-006.html`
- `shape-outside-box-007.html`
- `shape-outside-box-008.html`
- `shape-outside-box-009.html`

The other fourteen full-CSS gains are:

- `clear-on-child-with-margins.html`
- `clear-with-top-margin-after-cleared-empty-block.html`
- `floats-placement-003.html`
- `baseline-block-with-overflow-001.html`
- `padding-applies-to-009.xht`
- `padding-applies-to-012.xht`
- `padding-applies-to-015.xht`
- `block-non-replaced-width-001.xht`
- `height-applies-to-012.xht`
- `inline-block-non-replaced-height-001.xht`
- `tall-float-pushed-to-next-fragmentainer-000.html`
- `flexbox-min-height-auto-001.html`
- `flexbox-min-height-auto-003.html`
- `below-float2.html`

The complete-CSS comparison covers 36,311 identities. Exactly those 21
changed; there are zero pass-to-nonpass results, zero added or missing tests,
and zero other status or pinned-reason changes. The fragmentation gain remains
marked `reference-unverified`, as in the exact map.

| Map | Baseline SHA-256 | Candidate SHA-256 |
|---|---|---|
| shape box | `F5B285E75DFFF835C90D5CBC42565DECC625F2E8FF6186E23F577D4D6E171E49` | `091A78C78F83F152A9C50AA6BA1760FC509D3A472D06837EDD23223A9F905FD3` |
| CSS Shapes | `CDCAEDB9697B4DF8FA75D04C0CBD6C5874A7D37CA3D7FB09C9E802A5938252B9` | `B4F386A9BBCD950BD2F070623E8247563E32976295B01194DE31E0A932ADA6EC` |
| CSS2 floats | `475CA34D58B6EFB1E641426E70D66594ED7BB4FB4DDDAA8B35DAEA444D48FA97` | `56E576151B5444E5AF0A8D085AB4A6F6B271D0F73748D031E155EA74D91E3824` |
| css-position | `74DE7C0761B79152E13010538BB8B87EA32C853846A24CE3AA9031DE233B83E4` | `C623F839FA950950DBED446CD06AD38103B03634C2F6907CEEB42C6B527DC32B` |
| full CSS | `32C1EC5C53ACF53C9CAF237864E60731688CE9B1BA5725114D42371D80265779` | `530226B01D3435A6F347FD3E4E4704BC46167BE5BDA34FCD94EFB8AE8F5694C5` |

All maps and frozen runners are under
`testing/genet/wpt-ledger/2026-08-25_relative_float_state`.

## Native wall

- Buckram is 250 / 250. The added receipt covers the same-start and strictly
  straddling zero-height float boundaries.
- Buckram strict Clippy passes for library and tests with warnings denied.
- Genet-Livery is 212 / 212.
- Live Genet-Livery receipts cover direct text after two forced breaks inside a
  relative wrapper and an auto-width floated descendant inside a zero-height
  relative wrapper.
- Changed-file rustfmt and `git diff --check` pass.

## Remaining row 12 work

This receipt's 23 red files were a historical count, not one atomic line-height
family. The following [horizontal direction reconciliation](2026-08-25_buckram_horizontal_float_direction_reconciliation.md)
proved that eight were LTR/RTL float-state boundaries and repaired them.

The current exact shape-box map has 15 red files: two horizontal padding-box
paint/overlap cases, two rounded split-inline float cases, eight vertical or
sideways rounded cases, and three `shape-margin` cases. Those families retain
separate owners and done conditions.

## Done condition

This slice is complete because relative normal-flow blocks retain float state,
the exposed zero-height and fixed-leaf boundaries have focused native receipts,
all five focused maps have no loss, and the complete-CSS map has 21 exact gains
with zero loss. Row 12 remains in progress; its current 15 shape-box residuals
and wider basic-shape and image families are assigned in the
[horizontal direction reconciliation](2026-08-25_buckram_horizontal_float_direction_reconciliation.md).
