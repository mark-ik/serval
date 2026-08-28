# Vertical flex cross-axis receipt

Source: `63950e568b2`; runner: `C:\t\genet-row18-vertical-flex-runner-63950e568b2.exe`; runner SHA-256: `73a476e96efc44d16b16f59cd8175f10698438be3eea42826634e7056408918e`; manifest SHA-256: `d5ec5be9bf1a75ed00d7e7ab28afe8a694a55e11682ba74305874d70b18dd422`.

The runner was released and frozen with isolated `CARGO_HOME` and `CARGO_TARGET_DIR`, `--locked --offline -j1`; the checked-in JSON files are raw runner outputs. The ignored `Cargo.lock` SHA-256 is `BDBE23A467DCFCA4260E74F42587E8929B775B1EC39DE1E98DBA79529ACFB661`; it was regenerated offline after current-main Pelt manifest changes before the successful locked build. Each case used `genet-wpt reftest css/css-flexbox/<case> --renderer livery --expectation-policy exact --write-expectations <ledger>/<case>.json -v`.

| Cases | Result |
|---|---|
| `flexbox-writing-mode-002/003/005/006` | 4 pass |
| `css-flexbox-row-wrap-reverse`, `css-flexbox-row-reverse-wrap-reverse` | 2 fail, localized 1% diffs |

At source `560627d0152`, locked/offline `-j1` native checks were: matrix 1/1, live fixtures 2/2, direction 1/1, and automatic minimum 1/1.

## Element-owned flex item self-alignment

Implementation source `0f49c1ab4fc` adds the parent-context projection for
element-owned flex items. It distinguishes logical `start`/`end` from
flex-relative edges, resolves `self-start`/`self-end` against the subject's
writing mode, inherits the container's effective `align-items` for
`align-self: auto`, and applies the content-keyword fallback only when that
effective alignment is `normal` or `stretch`. Anonymous, text, and pseudo box
provenance is not treated as the owner's computed style.

The ignored root lock SHA-256 was
`20D4CF1F8F6D0A3B29437516DD7E39F8990908EFFB02CD76DB2B4E7553769196`.
All counted commands used `--locked --offline -j1` and
`--config profile.test.debug=0` with the isolated target
`C:\t\genet-row18-flex-align-self-debug0-target`.

| Native gate | Result |
|---|---|
| Genet-Livery library, known canvas baseline filtered | 229 pass |
| Vertical cross-axis live fixtures | 5 / 5 |
| Automatic minimum | 1 / 1 |
| Content basis | 2 / 2 |
| Content-basis repro | 6 / 6 |
| Buckram library | 255 / 255 |

The isolated canvas baseline still fails with `200 x 200` observed against
`100 x 100` expected. No upstream WPT movement is claimed for this sub-slice:
the available `flexbox-align-self-vert-*` family also exercises baseline,
intrinsic dimension, and reference-layout behavior. Mixed-writing-mode
baseline alignment, the two auto-width intrinsic row-wrap cases, and
generated/pseudo inherited self-edge projection remain separate residuals.

## Auto-width row-wrap closure

The two auto-width row-wrap residuals were repaired at implementation source
`c5cb56902fe` and pass twice under the exact release receipt at
`testing/genet/wpt-ledger/2026-08-28_flex_vertical_auto_block`. The failed maps
above remain the historical pre-repair evidence. Mixed-writing-mode baseline
alignment and generated/pseudo inherited self-edge projection remain open.
