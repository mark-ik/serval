# Vertical flex automatic block-size receipt

Implementation source: `c5cb56902fef30f6ebc50ac653dbc2777fa09036`.
Integrated runner source: `91a87cbe340`.

- Runner: `C:\t\genet-row18-vertical-auto-block-runner-91a87cbe340.exe`
- Runner SHA-256: `E833A632E9F2CD590DE5B4D983A7EAC923E155814749BDBE662BE8C76A7F3D6D`
- WPT manifest: `tests/wpt/meta/MANIFEST.json`
- WPT manifest SHA-256: `D5EC5BE9BF1A75ED00D7E7AB28AFE8A694A55E11682BA74305874D70B18DD422`
- Ignored `Cargo.lock` SHA-256: `31B4D71424162871768B03660C91AAF035A2D2F93329A5A3BEF61D6C2F4B81AF`

The release runner was built from the integrated source with an isolated
`CARGO_TARGET_DIR` at
`C:\t\genet-row18-vertical-auto-block-release-target`,
`CARGO_PROFILE_RELEASE_DEBUG=0`, and
`cargo build -p genet-wpt --release --locked --offline -j 1`. Each checked-in
map is the raw output of a separate `genet-wpt reftest` invocation using the
Livery renderer and exact expectation policy.

## Exact WPT receipt

| Cases | Result |
|---|---|
| `css-flexbox-row-wrap-reverse.html` | pass twice, reference verified |
| `css-flexbox-row-reverse-wrap-reverse.html` | pass twice, reference verified |
| `flexbox-writing-mode-002/003/005/006.html` | 4 / 4 pass, reference verified |

The two maps for each target case are byte-identical. Their SHA-256 pairs are
`C41DD7DC1FF8A6BD78300FC8CD325B2ED6F3F3C7C9623B541B30C787A58D34FA`
for row and
`7AB0A297298BDA8BC96B175668140BB014DACF8C519D7D4886363486FE4AB41E`
for row-reverse.

## Native receipt

All counted native commands used the reconciled lock above,
`--locked --offline -j 1 --config profile.test.debug=0`, and the isolated
target `C:\t\genet-row18-vertical-auto-block-debug0-target`.

| Gate | Result |
|---|---|
| `genet-livery --test flex_vertical_cross_axis` | 6 / 6 pass |
| `buckram` package wall | 257 / 257 pass, 0 doc tests |

Buckram now keeps a horizontal block formatting context when its child is the
bounded vertical flex row shape: exact containing-flow provenance, definite
logical inline size, automatic logical block size, and Taffy physical
`Column` or `ColumnReverse`. The admission excludes out-of-flow, float,
clear, shrink-to-fit, replaced, aspect-ratio, containment, nonlinear,
min/max, percentage, intrinsic-keyword, physical-row, grid, and mismatched
containing-flow shapes. Descendant `float` state is ignored only inside this
admitted flex shape because floats on flex items do not participate in float
layout.

Mixed-writing-mode baseline alignment, generated or pseudo inherited
self-edge projection, inline flex, and general orthogonal intrinsic sizing
remain outside this receipt.
