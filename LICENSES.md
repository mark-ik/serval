# Licenses in this repository

**This repository: MPL-2.0.** genet is a derivative of Servo, and every file
Mark wrote carries Exhibit A and the SPDX tag `MPL-2.0`, per the
[license posture brief](../mere/design_docs/2026-08-22_license_posture_brief.md)
of 2026-08-22 (mere `design_docs/2026-08-22_license_posture_brief.md`). The
full text is in [`LICENSE`](LICENSE).

This file is the provenance ledger. It is the authority for what the relicense
tool (mere `scripts/relicense_headers.py`) skips: the backtick-quoted paths in
the **Retained licenses** table are never touched. Provenance comes before
license: a file gets Exhibit A only if Mark wrote it.

## Servo heritage

The Servo-derived components (`components/allocator`, `components/config`,
`components/default-resources`, `components/deny_public_fields`,
`components/geometry`, `components/media`, `components/paint`,
`components/pixels`, `components/profile`, `components/servo_tracing`,
`components/shared`, `components/url`, `components/wakelock`,
`components/webgl-essl`, `components/webgl-wgpu`, `components/webgpu`,
`components/webxr`, `components/xpath`, and the Servo-lineage files inside
`components/genet-scripted-dom` and its neighbours) are MPL-2.0 already and
carry Servo's bare Exhibit A: no Merely copyright line is added to code Mark
did not write. The tool leaves any file whose leading lines already carry
Exhibit A untouched, so these need no ledger row. Their copyright remains with
The Servo Project Developers; see `LICENSE_WHATWG_SPECS` for the WHATWG
specification text Servo ships alongside its DOM.

## Retained licenses

Third-party code keeps its own license and its own notices. Nothing here is
relicensed, and nothing here receives a Merely copyright line.

| Path | License | Upstream | Notice files |
|---|---|---|---|
| `support/patches/taffy` | MIT | [DioxusLabs/taffy](https://github.com/DioxusLabs/taffy) | `LICENSE.md` in-tree |
| `support/patches/parley` | Apache-2.0 OR MIT | [linebender/parley](https://github.com/linebender/parley) | in-tree |
| `support/patches/ipc-channel` | MIT OR Apache-2.0 | [servo/ipc-channel](https://github.com/servo/ipc-channel) | in-tree |
| `support/patches/gpu-allocator` | MIT OR Apache-2.0 | [Traverse-Research/gpu-allocator](https://github.com/Traverse-Research/gpu-allocator) | in-tree |
| `support/patches/sonic-rs-0.5.8` | Apache-2.0 | [cloudwego/sonic-rs](https://github.com/cloudwego/sonic-rs) | in-tree |
| `support/name-claims/genet-taffy` | MIT | a taffy fork's name claim; taffy's terms | its manifest |
| `components/hyper_serde` | MIT OR Apache-2.0 | [servo/hyper_serde](https://github.com/servo/hyper_serde) | `LICENSE-MIT`, `LICENSE-APACHE` in-tree |
| `components/malloc_size_of` | MIT OR Apache-2.0 | [servo/servo](https://github.com/servo/servo) (`components/malloc_size_of`) | `LICENSE-MIT`, `LICENSE-APACHE` in-tree |
| `components/default-resources/resources` | MPL-2.0 | [servo/servo](https://github.com/servo/servo) (`resources/`) | Servo's |
| `resources` | MPL-2.0 | [servo/servo](https://github.com/servo/servo) (`resources/`, including the about:license page that quotes the MPL text) | Servo's |
| `third_party/mozdebug` | MPL-2.0 | Mozilla (`mozdebug`) | in-tree headers |
| `tests/wpt` | BSD-3-Clause | [web-platform-tests/wpt](https://github.com/web-platform-tests/wpt) | `tests/wpt/tests/LICENSE.md` |
| `tests/blink_perf_tests` | BSD-3-Clause | Chromium (Blink performance tests) | `tests/blink_perf_tests/LICENSE_FOR_ABOUT_CREDITS` |
| `tests/html` | MPL-2.0 | [servo/servo](https://github.com/servo/servo) (`tests/html`) | Servo's |
| `tests/dromaeo` | MIT | Dromaeo (John Resig) | in-tree |
| `tests/jquery` | MIT | jQuery | in-tree |
| `tests/power` | MPL-2.0 | [servo/servo](https://github.com/servo/servo) | in-tree |
| `LICENSE_WHATWG_SPECS` | BSD-3-Clause | WHATWG | the file itself |

## Derivatives carrying MPL-2.0 with an upstream notice retained

These are **not** skipped. Each file receives Exhibit A and Mark's copyright
line, and every upstream copyright line above it is kept verbatim. Apply with
`--retain-notice`.

| Path | Upstream | Notices kept |
|---|---|---|
| _(none in genet as of 2026-09-03)_ | — | — |

`meristem` was the one entry in this table. It left for mere with the rest of
the Cambium family on 2026-09-03 under the platform boundary plan, and its
Xilem notice obligation — the `Copyright 2022 the Xilem Authors` lines and the
Apache-2.0 text kept beside them as the upstream notice file — travelled with
the crate. The ruling below is retained because it is the precedent the sweep
was decided on, not because genet still carries the files.

Ruled 2026-08-27 in mere's ledger, on the brief's substantial-derivative
precedent: cambium and meristem go MPL-2.0 with the Apache notice retained.
`cambium` itself is Mark's Serval-derived work and already carried Exhibit A.
`sprigging` is Mark's (it arrived with the cambium adoption on 2026-07-23 and
its July `MIT OR Apache-2.0` line was his own choice, not a third party's), so
it takes the default with no notice to retain. Published versions keep the
grant they shipped with (`sprigging` 0.2.1, `illume` 0.0.2, `errand` 0.3.4,
`tinct` 0.1.2, `inker` 0.1.1 and the rest); MPL-2.0 ships at each crate's
next functional bump, per the sweep plan's invariant 8.

**This section is deliberately not the skip list.** The tool reads only the
`## Retained licenses` table above.

## Exceptions under the fork/vendor criterion

**None.** `illume`, `buckram`, `errand` and `tinct` were each proposed and
declined on 2026-08-22.

## How to add a file from elsewhere

1. Do not delete or rewrite the upstream copyright or license notice, ever.
2. Add its path to **Retained licenses** above with its license, upstream URL,
   and where its notice text lives. The tool then skips it automatically.
3. If it is a substantial derivative rather than a verbatim import, the brief's
   rule is MPL-2.0 on the derivative *with the upstream notice retained*;
   record it in that section so the distinction is not lost.
4. Never add `license-file` to an owned manifest.
5. Re-run `python ../mere/scripts/relicense_headers.py --repo . --audit` and
   confirm the owned source count moved by exactly what you expected.
