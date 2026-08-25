# Livery paint reconciliation

**Date:** 2026-08-24
**Status:** Complete on a clean `origin/main` base.

## Scope

Lane 15 reconciles the exact `css/css-backgrounds`, `css/css-masking`, and
`css/css-images` directories. The implementation is deliberately bounded:

- the single-layer `background` shorthand now retains color, image, position,
  size, repeat, origin, clip, and attachment;
- background position accepts the one through four component forms;
- repeat keeps two axes plus `round` and `space` behavior;
- size, origin, clip, fixed attachment, image tiling, and flat-color text clip
  reach paint;
- a single-candidate `image-set()` containing a supported gradient selects
  that image without pretending to implement URL density selection; and
- CSS2 absolute block sizing fills the remaining space when both block insets
  are definite and the non-replaced block size is automatic.

Multiple background layers, general `image-set()` selection, masks,
`border-image`, and embedded-object replacement fitting remain separate work.

## Frozen inputs

| Input | Identity |
|---|---|
| accepted source base | `29f93579d920319a6185eb363b4915fff9e5ed7a` |
| WPT manifest | `d5ec5be9bf1a75ed00d7e7ab28afe8a694a55e11682ba74305874d70b18dd422` |
| baseline runner | `4c40c07641fd63372ec3393ace96f6369bf7c0ca45c1e632e93d0b7b55611305` |
| final runner | `9216d1193c8c7898caf32d2af2806c993a4fc7547070874a0e0e4e1094d0659d` |
| external ledger | `testing/genet/wpt-ledger/2026-08-24_paint_v2` |

Both runners were copied into the external ledger before use. Every directory
was run with `--expectation-policy exact` against the same manifest.

## Exact WPT result

| Directory | Baseline pass / fail / skip | Candidate pass / fail / skip | Fail to pass | Pass to fail |
|---|---:|---:|---:|---:|
| `css/css-backgrounds` | 247 / 342 / 360 | 322 / 267 / 360 | 75 | 0 |
| `css/css-masking` | 77 / 298 / 160 | 77 / 298 / 160 | 0 | 0 |
| `css/css-images` | 252 / 146 / 99 | 238 / 160 / 99 | 2 | 16 |
| **Total** | **576 / 786 / 619** | **637 / 725 / 619** | **77** | **16** |

The 786 historical failures are fully accounted: 77 are repaired and 709
remain assigned below. The candidate also exposes 16 historical false passes.
Those 16 are explained regressions, not repaired behavior.

The two `css-images` gains are
`object-position-png-001i.html` and `object-position-png-002i.html`. They use
background-painted references and therefore benefit from the corrected
background path even though `object-position` itself remains catalogued as
unimplemented.

## The sixteen exposed false passes

Fifteen are the `001e`, `001o`, and `001p` forms for each of `contain`,
`cover`, `fill`, `none`, and `scale-down`. Those suffixes exercise `embed`,
`object`, and video poster content. The last is
`object-fit-containcontainintrinsicsize-png-001i.tentative.html`.

Before this lane, the tests and their references were both underpainted and
compared equal. Correct background paint makes the references visible while
the test side still lacks embedded replacement content, `object-fit`,
`object-position`, and the tentative intrinsic-size interaction. The catalog
continues to name `object-fit` and `object-position` as unimplemented. Attempts
to widen this lane into embedded-object layout produced broad unrelated losses
and were rejected. The final source contains none of that sidequest.

## Residual ownership

These path-family counts sum to all 725 candidate failures.

### `css-backgrounds`, 267

| Family | Failures | Owner |
|---|---:|---|
| background size and SVG intrinsic sizing | 141 | SVG image decoding and complete CSS image sizing |
| border image | 56 | unimplemented `border-image-*` longhands and shorthand |
| background clip and origin | 28 | remaining inline, descendant, border-area, and multi-layer geometry |
| shadow effects | 20 | inset, scroll, fragmentation, and table shadow composition |
| other core backgrounds | 13 | remaining image, gradient, attachment, canvas, order, and table cases |
| border widths and radii | 9 | subpixel geometry and non-renderable border handling |

### `css-masking`, 298

| Family | Failures | Owner |
|---|---:|---|
| CSS clip path and animation | 90 | unimplemented `clip-path` values, geometry, and interpolation |
| SVG clip-path content | 93 | SVG resource and clip composition |
| legacy `clip` | 17 | CSS2 positioned clipping |
| `clip-rule` | 2 | SVG fill-rule projection |
| CSS mask image stack | 90 | unimplemented `mask-*` longhands and shorthand |
| SVG mask content | 6 | SVG mask resources and luminance/alpha composition |

### `css-images`, 160

| Family | Failures | Owner |
|---|---:|---|
| object fit, position, and view box | 90 | replacement layout and paint for images and embedded elements |
| `image-set()` | 25 | URL/type/resolution candidate selection |
| gradient forms | 26 | conic/radial gradients, missing components, hue, and multiple-position stops |
| image orientation | 10 | decoded image metadata and transforms |
| image rendering | 2 | sampling policy |
| image fallbacks and annotations | 5 | CSS image fallback grammar and selection |
| cross-fade | 1 | image composition |
| SVG script behavior | 1 | SVG image execution policy |

Of these 160 image failures, 16 are the exposed false passes above. The other
144 were already failures in the historical baseline. Together with all 267
background and 298 masking failures, that yields the 709 unrepaired historical
failures.

## Document and base-URL ownership

The required resource seam was already live on accepted main, so this lane did
not duplicate it. `genet-document-resources` retains authored and resolved URLs
for DOM resources and stylesheet-relative resources. Nested imports resolve
relative URLs against the importing sheet's final identity. `genet-wpt` loads
the resolved bytes and inserts them into the Livery session under both authored
and resolved keys.

The full nine-test `genet-document-resources` suite is green, including
`resolves_link_and_css_urls_against_their_own_sources` and
`imported_sheets_precede_their_parent_and_keep_final_identities`. The latter
resolves `images/inner.png` from an imported stylesheet to
`https://cdn.example.test/styles/images/inner.png`.

## Native wall

- `cargo test -p buckram --lib`: 237 passed.
- `cargo test -p livery --all-targets`: all targets passed.
- `cargo test -p genet-livery --all-targets`: all targets passed, including
  69 paint tests.
- `cargo test -p genet-document-resources --all-targets`: 9 passed.
- scoped Buckram strict Clippy: green.
- scoped Livery strict Clippy: green with the accepted-main precision,
  formatting-borrow, question-mark, self-convention, and test-default
  allowances named on the command line.
- scoped Genet-Livery strict Clippy with `--no-deps`: green with two inherited
  test-only allowances named on the command line.
- `git diff --check`: green.

The package-wide formatter check still reports accepted-main drift across
untouched files. A direct check of all eight changed Rust files reports only
the pre-existing replaced-size helper at `buckram/src/positioning.rs:432`.
That unrelated hunk remains byte-identical to accepted main.
