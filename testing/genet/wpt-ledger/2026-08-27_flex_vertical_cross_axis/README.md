# Vertical flex cross-axis receipt

Source: `63950e568b2`; runner: `C:\t\genet-row18-vertical-flex-runner-63950e568b2.exe`; runner SHA-256: `73a476e96efc44d16b16f59cd8175f10698438be3eea42826634e7056408918e`; manifest SHA-256: `d5ec5be9bf1a75ed00d7e7ab28afe8a694a55e11682ba74305874d70b18dd422`.

The runner was released and frozen with isolated `CARGO_HOME` and `CARGO_TARGET_DIR`, `--locked --offline -j1`; the checked-in JSON files are raw runner outputs. The ignored `Cargo.lock` SHA-256 is `BDBE23A467DCFCA4260E74F42587E8929B775B1EC39DE1E98DBA79529ACFB661`; it was regenerated offline after current-main Pelt manifest changes before the successful locked build. Each case used `genet-wpt reftest css/css-flexbox/<case> --renderer livery --expectation-policy exact --write-expectations <ledger>/<case>.json -v`.

| Cases | Result |
|---|---|
| `flexbox-writing-mode-002/003/005/006` | 4 pass |
| `css-flexbox-row-wrap-reverse`, `css-flexbox-row-reverse-wrap-reverse` | 2 fail, localized 1% diffs |

At source `560627d0152`, locked/offline `-j1` native checks were: matrix 1/1, live fixtures 2/2, direction 1/1, and automatic minimum 1/1. Explicit child `align-self`, mixed baseline alignment, and the auto-width intrinsic residual remain open.
