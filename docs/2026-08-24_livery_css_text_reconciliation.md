# Livery css-text reconciliation

**Date:** 2026-08-24

**Status:** Complete on the current accepted-main lineage.

**Program row:** [Buckram and Livery lane program, row 14](2026-08-21_buckram_livery_lane_program_plan.md).

## Ruling

The archived css-text lane at `c92d3a8a519` is diagnosis only. Its parent and
source tree included unrelated dirty-base K5h repairs already recovered by
accepted main. This lane started from `7e7b7f776ee`, used its own worktree and
target directory, and ported only the property catalog and parser ideas that
could be re-proved against the current renderer.

The historical “781 failures” count was not a current baseline. The frozen
current runner enumerated 1,964 files: 663 pass, 723 fail, 578 skip, and zero
error. The final runner reports 979 pass, 407 fail, 578 skip, and zero error.
That is 316 exact fail-to-pass changes and zero pass-to-fail changes.

## Landed behavior

- Livery now parses and cascades `text-align-last`, `text-justify`,
  `text-indent`, `text-transform`, `overflow-wrap` and its `word-wrap` alias,
  `word-break`, `line-break`, `hyphens`, `tab-size`, and the first-edge slice
  of `hanging-punctuation`.
- The text bridge applies Unicode case, capitalize, full-width, and
  full-size-kana transforms before shaping; resolves percentage spacing from
  the element font size; suppresses soft hyphens for `hyphens:none`; and maps
  wrapping, alignment, indentation, and tab policy into Parley.
- Preserved tabs advance to dynamic tab stops derived from the shaped space
  advance plus letter and word spacing. Atomic inline decorations reuse the
  retained positioned fragment, avoiding a second formatter at the line
  origin.
- Parley retains per-style tab stops, separate last-line alignment,
  reversible justification, emergency-wrap distinctions for intrinsic
  sizing, nested-nowrap atomic boundaries, and line-edge hanging for U+3000.
- `hanging-punctuation:first` offsets the first eligible opening punctuation
  by its shaped advance and respects a preceding non-zero inline edge.

## Exact WPT receipt

The ledger is external to the checkout at
`C:\Users\mark_\Code\testing\genet\wpt-ledger\2026-08-24_css_text_v2`.
Both maps use manifest SHA-256
`d5ec5be9bf1a75ed00d7e7ab28afe8a694a55e11682ba74305874d70b18dd422`.

| Runner | SHA-256 | Pass | Fail | Skip | Error |
|---|---|---:|---:|---:|---:|
| Frozen baseline | `e670ff76c2e392fa5b7c55c11e898427cadd6ca6887976c42e98a57bebafe617` | 663 | 723 | 578 | 0 |
| Final candidate | `4c40c07641fd63372ec3393ace96f6369bf7c0ca45c1e632e93d0b7b55611305` | 979 | 407 | 578 | 0 |

The 316 gains are: i18n 158, text-transform 57, text-align 24,
overflow-wrap 16, white-space 16, hyphens 11, word-break 10, line-break 6,
text-justify 5, text-indent 4, text-autospace 2, word-spacing 2,
writing-system 2, and one each in hanging-punctuation, letter-spacing, and
word-space-transform.

## Remaining 407 failures

Every red file has a family owner. Counts below sum to 407.

| Family | Count | Next implementation seam |
|---|---:|---|
| white-space | 169 | Phase-II trimming and hanging variants, control characters, balance, float interaction, and intrinsic sizing |
| line-break | 42 | Locale-specific loose, normal, and strict tailoring beyond the current anywhere mapping |
| line-breaking | 37 | Atomic and replaced boundaries plus segment-break transforms |
| hyphens | 31 | Language dictionaries, automatic hyphenation, punctuation, character, and limit controls |
| text-align | 21 | Remaining writing-mode, last-line, and justification edges |
| text-autospace | 12 | Property and script-boundary spacing model |
| text-transform | 12 | Language tailoring, math-auto, and remaining capitalize/full-width edges |
| letter-spacing | 11 | Bidi, controls, nesting, and shaping edges |
| overflow-wrap | 11 | Cluster, shaping, and intrinsic-size remainder |
| word-space-transform | 10 | Property model |
| word-break | 9 | Language, cluster, and intrinsic remainder |
| text-fit | 9 | Property and per-line fitting model |
| hanging-punctuation | 8 | End-edge modes and boundary ownership; first-edge behavior is landed |
| shaping | 8 | Font and script boundary shaping |
| text-indent | 5 | Floats, intrinsic sizing, and out-of-flow children |
| bidi | 3 | Bidi line construction |
| text-encoding | 3 | Arabic joining and shaping |
| text-justify | 2 | Inter-word separator handling |
| boundary-shaping | 1 | Shaper boundary contract |
| text-spacing-trim | 1 | Property model |
| text-stroke-width-subpixel | 1 | Paint precision |
| word-spacing | 1 | Remaining shaping edge |

## Native receipts

`components/genet-livery/tests/css_text_lane.rs` contains 16 focused receipts:
forced and preserved breaks; ordinary and spaced tab stops; retained inline
paint placement; first-edge hanging punctuation; ideographic-space hanging;
Unicode transforms; percentage spacing; overflow and word-break distinctions;
keep-all; indentation; direction-aware alignment; last-line alignment;
justify-all and `text-justify:none`; soft-hyphen suppression; and nested
nowrap. The affected Livery, Parley, and Genet-Livery test and strict-Clippy
gates are recorded in the integrating commit.

## Done condition

Row 14 is complete when the current directory is fully attributed, the
focused native receipts pass, the exact candidate has no baseline loss, and
the frozen runners and maps remain in the external ledger. Those conditions
are met. The 407 classified failures are future css-text slices, not hidden
expectations or accepted regressions.
