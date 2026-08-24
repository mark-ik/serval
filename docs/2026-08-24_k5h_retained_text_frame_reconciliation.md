# K5h retained text-frame reconciliation

**Date:** 2026-08-24

**Status:** Complete on accepted `origin/main`
`e8db57141f1572d620011ffd06768ed1bd57b196`.

**Parent:** [K5h dirty-root relayout](2026-08-11_buckram_k5h_dirty_root_execution_plan.md)
and row 3 of the
[Buckram and Livery lane program](2026-08-21_buckram_livery_lane_program_plan.md).

## Ruling

The archived `lane/k5h-text` worktree is not an integration source. Its useful
diagnosis was rechecked against accepted current code.

Current main already contains the repair. `TextFrame::translate_subtree`
moves prepared text commands, inline fragments, line keys, test baselines, and
selection clusters for a DOM subtree. Both retained positioned routes pair
their Buckram fragment translation with that text-frame translation. A leaf
whose used size changes and owns prepared text is rejected before mutation so
ordinary formatting reshapes it.

The accepted implementation is attributable to commits `55af2b28916c` and
`810fd04809b2`, both ancestors of current main. No archived source overlay was
applied and no current source repair was required.

## Provenance

- Worktree: `worktrees/genet-k5h-retained-text-frame-v2`.
- Isolated target: `C:\t\genet-k5h-retained-text-frame-v2-target`.
- External ledger:
  `testing/genet/wpt-ledger/2026-08-24_k5h_retained_text_frame_v2`.
- Ignored offline `Cargo.lock` SHA-256:
  `6F91D0C46D3BF171137B16023C04C5A8032B405CD10D321E6D41459CAD4EBE49`.

## Current-main receipts

The focused `positioned_` library filter passed 23/23. Its load-bearing cases
are:

- `positioned_inset_mutation_reuses_a_stable_fragment_subtree`, which retains
  fragment identities and compares translated text paint with a fresh final
  document;
- `positioned_inset_reuse_updates_nested_scroll_range`, which proves retained
  overflow equals fresh layout after translation;
- `positioned_leaf_geometry_mutation_resizes_the_retained_fragment`, which
  proves geometry-only leaf resize and paint equivalence;
- `positioned_leaf_resize_updates_nested_scroll_range`, which proves the leaf
  export reaches nested scrolling;
- `positioned_text_leaf_resize_reformats_instead_of_reusing_shaped_text`, which
  proves a text-bearing size change reshapes to fresh-final equivalence.

The native wall also passed:

- `cargo test --locked --offline -p buckram -p genet-livery --all-targets`,
  including 236/236 Buckram unit tests and every Genet-Livery target;
- strict production-library Clippy for Buckram and Genet-Livery with
  `--no-deps -D warnings`;
- `git diff --check`.

No WPT comparison is claimed. These routes alter retained mutation frames;
`genet-wpt reftest` constructs fresh documents and cannot exercise them.

## Done condition

Accepted current code moves shaped text with a retained positioned fragment
subtree, refuses geometry-only reuse when text needs reshaping, proves
geometry-only leaf resize and scroll exports, and matches fresh-final paint.
Row 3 is closed. The broader K5h damage-class matrix remains open in its own
plan.
