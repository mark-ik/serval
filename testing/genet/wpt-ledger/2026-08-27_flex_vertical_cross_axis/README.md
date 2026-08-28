# Vertical flex cross-axis receipt

Source: `560627d0152`; runner: `C:\t\genet-row18-vertical-flex-runner-560627d0152.exe`; runner SHA-256: `cd4e8ab840cc3a5f17a8719fa7f35b46a4745e5e74c3ee40d6db820f15b9c6f9`; manifest SHA-256: `d5ec5be9bf1a75ed00d7e7ab28afe8a694a55e11682ba74305874d70b18dd422`.

The runner was released and frozen with isolated `CARGO_HOME` and `CARGO_TARGET_DIR`, `--locked --offline -j1`; the checked-in JSON files are raw runner outputs. Each case used `genet-wpt reftest css/css-flexbox/<case> --renderer livery --expectation-policy exact --write-expectations <ledger>/<case>.json -v`.

| Cases | Result |
|---|---|
| `flexbox-writing-mode-002/003/005/006` | 4 pass |
| `css-flexbox-row-wrap-reverse`, `css-flexbox-row-reverse-wrap-reverse` | 2 fail, localized 1% diffs |

At source `560627d0152`, locked/offline `-j1` native checks were: matrix 1/1, live fixtures 2/2, direction 1/1, and automatic minimum 1/1. Explicit child `align-self`, mixed baseline alignment, and the auto-width intrinsic residual remain open.
