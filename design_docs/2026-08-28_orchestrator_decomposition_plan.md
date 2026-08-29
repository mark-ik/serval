# Orchestrator decomposition plan

**Status:** in progress (2026-08-29). Phases 1 (Pelt `workspace_viewer.rs`),
2 (`genet-documents/engines.rs`) and 3 (`genet-livery/document.rs`) landed;
Phases 4–7 planned.

genet is healthy but lumpy. The architecture has real seams, yet several active
orchestrators have accumulated receipts, policy and mechanics in one file. This
plan records the measurement, fixes the order the files come apart in, and
carries the done-conditions each phase is judged against.

## Measurement

Measured at pushed `main`, `71c34f5ae7c`, using physical lines.

- 178 Rust files exceed 600 lines: 118 first-party production/tooling, 45
  patched upstream, 15 explicit tests or examples.
- The 118 first-party files hold 62.5% of first-party Rust lines.
- 21 exceed 2,000 lines; 31 are 1,001–2,000; 66 are 601–1,000.

Non-Rust source over 600 lines is modest: `dom/bootstrap.js` (3,034) and
`debugger.js` (609). Everything else large is catalog data, WPT expectations,
licences or rendered receipts.

## Why Pelt goes first

`ports/pelt/desktop/workspace_viewer.rs` is not the largest file —
`genet-livery/layout.rs` is, at 15,301 lines. Pelt goes first on three counts:

1. **It is the fastest-growing.** Across the six commits from `7d74b0c47d6` to
   `71c34f5ae7c` the file grew 8,670 → 10,066 lines: +1,396 in six commits.
2. **Its receipt boundaries are unusually strong.** Nineteen named receipts each
   assert one product claim against a pinned assertion string, so a decomposition
   has a dense, pre-existing behavioural contract to move against.
3. **It is uncontested.** At the time of writing, working-tree edits in
   `repos/genet` touched `genet-livery/{document,layout,text}.rs` and
   `buckram/{taffy_adapter,box_tree}.rs` — items 3, 4 and 7 below.
   `workspace_viewer.rs` was untouched, so Phase 1 could not collide with
   in-flight work.

## Phases

Ordered by payoff against risk. Each phase is judged only by its
done-conditions; none carries a time estimate.

### Phase 1 — Pelt `workspace_viewer.rs` (10,066 lines)

Combined the event loop, rendering, Chrome input, accessibility composition,
native surfaces, nineteen receipt drivers and 2,726 lines of tests.

First cut: `tests.rs`, `accessibility.rs`, and
`receipts/{routing,chrome,a11y,reader}.rs`, keeping `WorkspaceApp` and the event
loop central.

**Done-conditions**

- Identical public behaviour; no change to the `pelt_desktop` public re-exports.
- The `pelt-desktop` and `pelt-core` test set is unchanged, name for name.
- `workspace_viewer::tests` still reports 24 passing under default features.
- `cargo fmt -p pelt-desktop -- --check` clean, touching no unrelated file.
- The moved `cfg`-gated code still compiles under `scripted`, `smolweb`,
  `tabard-preview` and `tabard-reader-preview`, none of which are default.
- Headed receipts `mixed`, `loading-error`, `accessibility-input` and
  `reader-accessibility` still print their pinned assertion strings.

### Phase 2 — `genet-documents/engines.rs` (3,829 lines)

Best architectural payoff of the remaining set. Split the Livery, Scripted and
Smolweb session adapters plus the clipping helpers. Its narrow-feature warnings
are evidence that unrelated engine imports already interfere with one another.

**Done-conditions**: each engine adapter compiles with only its own feature
enabled; the narrow-feature warnings that motivated the split are gone; the
document-engine test set is unchanged name for name.

### Phase 3 — `genet-livery/document.rs` (4,446 lines)

Move 2,292 lines of tests, then separate retained frame/damage, resources,
scrolling/selection and animation clocks. `LiveryDocument` stays put, with impl
blocks in child modules.

**Done-conditions**: `LiveryDocument`'s public surface is byte-identical; the
livery test set is unchanged name for name.

### Phase 4 — `genet-livery/layout.rs` (15,301 lines)

The biggest file, but not the first semantic refactor. Moving its 6,721-line
test module is safe. Then extract the contiguous seams that already exist:
positioned layout, tables, hit testing and Taffy style projection. **Leave the
central layout transaction intact** until those helpers are independently
receipted.

**Done-conditions**: the flexbox and layout receipts hold at their recorded
counts; the central layout transaction is untouched in this phase.

### Phase 5 — `genet-wpt/main.rs` (4,943 lines)

Split argument parsing, expectations, test262, testharness commands and reftest
commands. Tooling decomposition with relatively contained runtime risk.

**Done-conditions**: WPT expectation totals are unchanged before and after.

### Phase 6 — `livery/values/property.rs` (4,168 lines)

A mechanical split by value family: backgrounds, animation, containment,
typography, box/layout, transforms. Re-export the existing types unchanged.

**Done-conditions**: no import path outside the module changes.

### Phase 7 — `buckram/taffy_adapter.rs` (6,996 lines)

Move its 3,582-line tests first. The remaining 3,414-line adapter is fairly
cohesive; separate the public algorithm tree from per-run intrinsic/block
computation later.

**Done-conditions**: the Taffy seam's published surface is unchanged;
`genet-taffy` consumers need no edit.

## Findings

Facts verified during the work, with code references.

### 2026-08-28 — the file's real shape

`workspace_viewer.rs` was not 87 loosely-grouped items but one
`impl WorkspaceApp` block spanning lines 1446–6889 — 5,444 lines, 87 methods —
with `mod tests` at 7340–10066 and free helpers on either side. The receipt
drivers, the accessibility projection and the Chrome input handlers each
occupied contiguous line ranges inside that single block, which is why the cut
could be pure code motion rather than a rewrite.

### 2026-08-28 — privacy runs one way, and it decided the cut

The first attempt also moved the accessibility *types* (`WorkspaceA11yFocus`,
`LiveryA11yChild`, `WorkspaceAccessibility` and eleven more) into
`accessibility.rs`. It failed with 224 errors, **150 of them private-field
accesses**.

The rule is asymmetric: a descendant module can see its ancestors' private
items, but **a parent cannot see a child's**. Relocating the shared a11y
vocabulary therefore forces `pub(...)` onto every type, impl method and struct
field that the parent, the receipt drivers and the tests touch — trading a pure
code motion for a crate-wide visibility rewrite.

The types stayed in the parent. Promoting them is a legitimate follow-up, but it
wants its own visibility pass and its own receipts, not a free ride on a
code-motion commit.

### 2026-08-28 — moved methods need their original scope named

An inherent method with no `pub` is private *to the module holding its impl
block*. Moving 49 methods into child modules silently narrowed them, so sibling
modules could no longer call them (E0624, 150 occurrences).

`pub(in crate::workspace_viewer)` names the original scope exactly — these were
private to `workspace_viewer`, hence callable from it and every descendant. It
widens nothing and narrows nothing. This is the only annotation the moved code
carries.

### 2026-08-28 — `include_*!` paths are resolved against the including file

`include_str!("../examples/workspace/reader/neighbor.html")` sat inside the test
module. Moved one directory deeper it needed `../../`. The two `include_str!`
constants that stayed in the parent were correctly left alone. Any later phase
that moves fixture-loading code must apply the same depth correction.

### 2026-08-28 — default features hide a third of the gates

The moved code carries `cfg` gates for `scripted`, `smolweb`, `tabard-preview`
and `tabard-reader-preview` — **none of them default** — plus five
`not(target_os = "windows")` arms. Six of the file's 30 tests are gated behind
`all(feature = "scripted", feature = "smolweb")`, which is why a default run
reports 24 rather than 30. A green default build proves nothing about any of
them, so Phase 1 was additionally checked under those features.

### 2026-08-28 — the extended-feature test link hits an MSVC PDB limit

`cargo test -p pelt-desktop --features scripted,smolweb,tabard-preview,tabard-reader-preview`
fails at link with `LNK1318: Unexpected PDB error; LIMIT (12)`. This is a debug
-info size limit in `link.exe`, not a code error: `cargo check` with the same
feature set passes. The repo's own recorded receipt invocations already work
around it with `--config 'profile.dev.debug=0'`.

### 2026-08-28 — receipt digests are not a portable contract; the assertion is

Comparing a refactor against the digests recorded in
`docs/2026-08-22_pelt_host_reconstruction_execution_plan.md` is unsound unless
two conditions hold. Both were found the hard way.

**The worktree path is rendered into the frame.** Pelt's Chrome draws the
address bar, and a receipt's fixture URL is an absolute path. Running
`loading-error` from `C:	\genet-pelt-decomp` and from
`C:	\genet-pelt-pristine` produces different digests and a consistent
1,270-byte PNG size difference, purely because the two path strings rasterise
differently. Run from the *same* directory, refactored and unmodified code gave
the identical digest `41f5cce26391e757`. A digest is only comparable against
another run from the same absolute path.

**Some receipts are not digest-stable at all.** Unmodified `reader-accessibility`
produced three different digests in three consecutive runs from one binary and
one path (`25d6713d1dabf7d1`, `ce693dfdabca01a0`, `2ce6d12c0398dd55`). A
pixel diff of two differing `loading-error` frames isolated the mechanism: **one
pixel, one channel, delta of 1** — GPU rasterisation tie-breaking on an
antialiased edge. Receipts with more antialiased text hit more ties.

`loading-error` is stable per path; `accessibility-input` reproduced its
recorded digest `f52b6536f31f06ad` exactly; `reader-accessibility` is not
stable. So the pinned assertion string, the redraw count and the verified sizes
are the contract a decomposition should be judged against. Treat a digest as
corroboration when it matches and as a prompt to investigate when it does not —
never as a failure on its own.


### 2026-08-28 — headed receipt verification

All four named receipts print their pinned assertion strings on the refactored
tree. Because digests are path-dependent (above), each comparison below was run
from the *same* absolute path for both code versions.

| Receipt | Result | Evidence |
|---|---|---|
| `loading-error` | pass | refactored and unmodified both give `41f5cce26391e757` from the same path — bit-identical |
| `accessibility-input` | pass | digest `f52b6536f31f06ad`, reproducing the value recorded 2026-08-28 exactly |
| `reader-accessibility` | pass | assertion correct; digest not stable on *unmodified* code (three values in three runs), refactored value is one of them |
| `mixed` | pass | assertion exact; 9 of 10 runs green |

**`mixed` is flaky, and it was flaky before this change.** Its first run here
failed with `mixed receipt Gemtext gesture did not navigate tile 1`; the next
nine passed. The receipt drives a Gemtext gesture against a live Scrying
producer and is timing-sensitive by construction — the execution plan itself
records 160, 219 and 224 redraws across three runs, and this session measured
288–298 (refactored, n=10) against 294–296 (unmodified, n=5). The two
distributions overlap completely, so the failure is not attributable to the
decomposition, which moves no logic. It is recorded here rather than dropped:
a receipt that fails one run in ten is worth a bound or a retry, and a later
phase should not rediscover it as a surprise.

## Progress

### 2026-08-28 — Phase 1 implemented

Branch `pelt-workspace-viewer-decomp`, based on `71c34f5ae7c`.

`workspace_viewer.rs` **10,066 → 3,217 lines (−68%)**. 49 of 87 methods moved;
`WorkspaceApp`, the event loop, `render`, pointer input and the shared a11y and
Chrome type vocabulary stay central.

| Module | Lines | Holds |
|---|---:|---|
| `workspace_viewer.rs` | 3,217 | `WorkspaceApp`, `ApplicationHandler`, `render`, pointer input, shared types |
| `workspace_viewer/tests.rs` | 2,696 | the whole test module |
| `workspace_viewer/receipts/a11y.rs` | 1,148 | the seven typed AccessKit action receipts |
| `workspace_viewer/receipts/chrome.rs` | 1,007 | Chrome, LoadingError, NarrowChrome, ChromeDpi, Appearance, TabardPreview |
| `workspace_viewer/receipts/routing.rs` | 831 | Mixed, Fallback, the step machine, synthetic click helpers |
| `workspace_viewer/accessibility.rs` | 821 | the composite accessibility projection and action routing |
| `workspace_viewer/receipts/reader.rs` | 405 | Reader, ReaderAccessibility, TabardReaderPreview |
| `workspace_viewer/receipts.rs` | 9 | submodule declarations |

Verified:

- `cargo test -p pelt-desktop -p pelt-core` — **63 tests, identical set name for
  name** against the pre-refactor baseline; `workspace_viewer::tests` 24/24.
- `cargo check -p pelt-desktop --tests` — clean.
- `cargo check -p pelt-desktop --tests --features scripted,smolweb,tabard-preview,tabard-reader-preview`
  — clean, on both the pre-refactor and post-refactor trees.
- `cargo fmt -p pelt-desktop -- --check` — clean, and rustfmt touched no file
  outside the split.
- `cargo test -p pelt` — the viewer parser/fixture suite, **4/4**.
- All four headed receipts green with their pinned assertions (table above).

Two environmental notes for whoever runs this next. Linking `pelt` or its test
binary with debug info fails on this machine with `LNK1318: Unexpected PDB
error; LIMIT (12)`; every headed run above used
`--config 'profile.dev.debug=0'`, which the repo's own recipes already do.
And the receipt harness is a separate path from `cargo test`: the tests call
the driver methods directly and never open a window, so they cannot catch a
regression in the winit/wgpu present path.

The split was performed by a brace-accurate extractor rather than by hand; a
line-multiset comparison across all eight files showed the only content
difference to be `mod tests {` becoming `mod tests;`, with every other new line
a module doc, an `impl WorkspaceApp {` wrapper, a `use`, or a `mod` declaration.

### 2026-08-29 — Phase 2 landed

`engines.rs` **3,829 → 73 lines**: the parent keeps the shared capability
helper and the module ladder; `engines/livery.rs` (1,583 with its editable
types), `engines/clip.rs` (the ClipRange/report/semantic-clip family Livery
and Scripted share), `engines/scripted.rs` (245, taking `scripted_scroll_key`
home), `engines/smolweb.rs` (166), and `engines/tests.rs` (1,640). Whole-module
feature gates replace thirty-odd per-item gates, and each child carries its own
imports.

Verified: every lane compiles with only its own feature, zero errors; the
narrow-feature warnings that motivated the split went **3/11/15 → 0/0/0**
(one pre-existing stray remains in `net_fetch.rs` under smolweb-only, outside
this file); the test set is unchanged name for name at 46 passing plus the two
livery native-editing failures that already fail on `origin/main` (owned by
the in-flight genet-livery work); pelt-desktop and pelt-core suites hold at
their Phase 1 counts.

The Pelt findings repeated on schedule: tests moving from child to sibling of
the code they inspect need the touched internals scoped (`pub(crate)` on five
session fields, four methods, and the editable types), and a helper only its
own module uses (`dom_path`) went back to private instead of riding the
re-export.

### 2026-08-29 — Phase 3 landed

`document.rs` **4,451 → 502 lines**, along the plan's four areas plus tests:
`document/frame.rs` (513: layout-damage tracking and the paint pipeline,
including the sticky pass), `document/scrolling.rs` (456: scrollport and
nested-scroller geometry, extents, clamping, scroll-into-view),
`document/animation.rs` (349: transitions, keyframes, and the `pump`/`settled`
clock), `document/selection.rs` (348: text selection, caret geometry, links,
pointer activation), `document/resources.rs` (95: host image and font bytes),
and `document/tests.rs` (2,280). `LiveryDocument`, its fields, and the shared
damage/animation types stay in the parent, so 71 of 96 methods moved out of a
1,843-line impl block.

**The public surface is byte-identical**: 54 `pub fn` before, 54 after, none
of them touched. The only annotation the moved code carries is
`pub(in crate::document)` on the 37 methods that were private — naming the
scope they already had, since a private inherent method is private to the
module holding its impl block. `find_id` needed a `#[cfg(test)]` re-export
because only the test module still calls it.

Verified: `cargo check -p genet-livery` clean on the first attempt (the
visibility shape was known from Phases 1 and 2); the livery test set unchanged
name for name at 234 entries — 232 passing plus
`replaced_html_dimensions_use_computed_css_and_canvas_intrinsics`, which
already fails on `origin/main`; genet-documents and both Pelt suites hold at
their own baselines; `cargo fmt` clean.
