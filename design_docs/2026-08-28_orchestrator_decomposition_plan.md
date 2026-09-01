# Orchestrator decomposition plan

**Status:** complete (2026-08-29). All seven phases landed. Follow-ups are
named at the foot of the Progress log rather than left implicit.

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

### 2026-08-29 — Phase 4 landed

`layout.rs` **15,382 → 6,165 lines**, taking only the four seams the phase
named and leaving the central transaction alone: `layout/tests.rs` (6,726),
`layout/taffy_style.rs` (993: the CSS-to-Taffy projection, its flex and grid
axis mappings, the value converters, and the calc() scratch),
`layout/positioned.rs` (861: static positions, relative offsets, absolute and
fixed containing blocks), `layout/tables.rs` (446: the retained table paint
model, plane, and structure commit), and `layout/hit_testing.rs` (223:
stacking-aware candidate collection).

`layout_impl`, `layout_atomic_subtrees`, `layout_inline_groups`,
`layout_retained_formatting_root` and `layout_with_text_system` all stay in the
parent, untouched, as the phase requires. The parent's only additions are the
module ladder, the re-exports, and two call sites.

**Both done-conditions met.** The livery test set is unchanged name for name at
234 entries, and the flexbox receipt holds exactly: `genet-wpt run
css/css-flexbox` reports **1321 passed, 0 failed, 0 errored, 37 skipped of
1358 files** on this branch and, run again in the same environment, the
identical figures on `origin/main`. That 1,358 is the same identity count the
flex-shorthand plan records. Buckram 257, both Pelt suites, and
genet-documents hold at their baselines.

The WPT subset totals cited in this plan come from `genet-wpt run`, which is a
crash-smoke pass: a file counts as passed when it lays out without panicking.
That is the right instrument for a behaviour-preserving split, where the claim
is "nothing changed", and identical totals plus the per-crate unit suites are
what each phase was held to. It is not a conformance measure. Pixel
conformance is `genet-wpt reftest <subset>`, which renders and compares against
the reference; its baselines at `9df392f42d8` are css-sizing 164/348,
css-flexbox 485/399 and css-grid 290/853 (passed/failed), captured per-test on
2026-09-01 so later behaviour changes can be diffed by name rather than by count.

Three findings this file added to the ledger:

- **A seam module must not shadow a function it carries.** Naming the module
  `hit_test` broke `lib.rs`'s `layout::hit_test` re-export, because the module
  won the path. It is `hit_testing`.
- **A glob re-import does not re-export.** Four converters
  (`line_height_px`, `length_percentage_px`, `signed_length_percentage_px`,
  `border_width_px`) plus `TablePaintModel`, the two stacking helpers and both
  hit tests are called from `text`, `paint`, `table_sizing`, `document` and
  `lib` as `layout::<name>`; each needs an explicit `pub(crate) use` or
  `pub use` beside the glob, or every external call site breaks.
- **Trait-impl methods take no visibility qualifier.** Scoping inherent
  methods is right; doing it inside `impl Trait for T` is `E0449`.

### 2026-08-29 — Phase 5 landed

`genet-wpt/src/main.rs` **4,943 → 1,013 lines**: `reftest.rs` (873: reference
resolution, fuzzy policy, image comparison, the table ledger),
`expectations.rs` (780: recorded and actual results, subtest normalisation,
the compare/check/write cycle), `net.rs` (686) and `tests.rs` (595) lifted out
of their existing inline `mod` blocks, `test262_cmd.rs` (514: the command lane
over the runner core), `args.rs` (262) and `testharness.rs` (245). The parent
keeps `main`, `real_main`, the command dispatch, test classification and
collection.

Done-condition met twice over: `genet-wpt run css/css-flexbox` gives **1321
passed, 0 failed, 0 errored, 37 skipped of 1358** and `run css/CSS2/floats`
gives **144 of 144**, each identical to the same run on `origin/main` in this
environment. The crate's own suite is 56 passed, 3 ignored, 0 failed.

**A near-miss worth the ledger: check for an existing file before naming a
module.** The crate already had `test262.rs` (the runner core: frontmatter
parsing and script assembly) alongside `harness.rs`, `manifest.rs`,
`render.rs` and `conformance.rs`. Extracting the test262 *command* lane to
that name silently overwrote it; `git status` showed it as modified rather
than added, which is how it was caught. The command lane is `test262_cmd.rs`.
Phase 4's shadowing rule generalises: a new seam module must collide with
neither an existing item name nor an existing file.

### 2026-08-29 — Phase 6 landed

`livery/values/property.rs` **4,168 → 23 lines**, a pure family split with no
test module involved: `layout.rs` (975: sizing, flex and grid vocabulary,
aspect ratio, radii, gaps, ordering, padding, decoration lines, border width,
stacking level), `animation.rs` (794: durations, delays, names, the transition
property list, timing functions), `typography.rs` (788), `backgrounds.rs`
(641), `transforms.rs` (599: opacity, rotate, scale, the transform list and
its functions, shadows), and `containment.rs` (391).

The families interleave in the original file, so three of them are gathered
from several ranges rather than one. A coverage assertion in the split script
proved **every non-blank line is claimed exactly once**, which is the property
that makes a scattered gather safe.

The done-condition holds strictly: the only paths that changed are
`property.rs` itself and its new directory. `git status` shows no other file
touched, and genet-livery, buckram and genet-documents compile with zero
errors and no edits, because the parent re-exports every family with
`pub use`. Livery's own suites are green and buckram holds at 257.

One wrinkle worth naming: **a relative path moves with the code.** The moved
code carried `super::calc`, `super::LengthUnit`, `super::Color` and
`pub(super)` — all of which meant `values` from `values/property.rs` but mean
`values::property` one level deeper. They are now spelled `crate::values::…`
and `pub(in crate::values)`, which say what they meant and no longer depend on
nesting depth.

### 2026-08-29 — Phase 7 landed, and the plan is complete

`buckram/taffy_adapter.rs` **7,014 → 1,223 lines**: `taffy_adapter/tests.rs`
(3,582) and `taffy_adapter/run.rs` (2,219). The phase took its deferred half
as well as the test move, because the run seam proved contiguous:
`AlgorithmRun`, its per-run input structs, its 1,740-line impl, the intrinsic
inline-size helpers, and all eight Taffy trait impls targeting it occupy one
unbroken range. What stays is the published surface — `AlgorithmTree`, the
algorithm vocabulary, and the size and available-space conversions.

Both done-conditions hold: **8 `pub` items before and after**, and `git
status` shows only the adapter and its new directory touched, with
genet-livery and genet-documents compiling unedited. Buckram is 257 passed, 0
failed on the first attempt; both Pelt suites green.

## Outcome

Seven files totalling 49,853 lines are now 12,216 in place, with the rest
moved into modules along seams that already existed:

| File | Before | After |
|---|---:|---:|
| `genet-livery/layout.rs` | 15,382 | 6,165 |
| `pelt/workspace_viewer.rs` | 10,066 | 3,217 |
| `buckram/taffy_adapter.rs` | 7,014 | 1,223 |
| `genet-wpt/main.rs` | 4,943 | 1,013 |
| `genet-livery/document.rs` | 4,451 | 502 |
| `livery/values/property.rs` | 4,168 | 23 |
| `genet-documents/engines.rs` | 3,829 | 73 |

No behaviour changed. Every phase verified its test set unchanged name for
name, and the receipt-bearing phases verified their receipts: sixteen headed
Pelt receipts, the flexbox map at 1321/1358 and floats at 144/144, and
narrow-feature warnings driven from 3/11/15 to zero.

**Follow-ups this work names rather than leaves implicit:**

- The `recovery/genet-primary-dirty-pre-reconcile-2026-08-28` branch edits
  `document.rs` (151+/295−), `layout.rs` (31+/15−) and `taffy_adapter.rs`
  (3+/1−) as they stood before Phases 3, 4 and 7. Its hunks now target code
  that lives in child modules, and weave resolves entities within a file
  rather than following one to another file, so that lane needs a manual
  reconciliation pass.
- Promoting Pelt's accessibility *types* into
  `workspace_viewer/accessibility.rs` wants its own visibility pass; Phase 1
  measured 150 private-field errors for it. **Landed 2026-09-01.** The pass
  was cheaper than the first attempt suggested: 434 lines moved as one
  contiguous band (eleven types, three impls and their one free helper),
  `pub(super)` on every moved type, field and externally called method, and
  the compiler then reported exactly one error, the projection's private
  constructor. `pub(super)` is the ceiling because nothing outside
  `workspace_viewer` names any of it; the receipt drivers and tests reach it
  through the parent's glob as before. Parent 4,388 to 3,970 lines;
  `accessibility.rs` 821 to 1,266. Verified by the 53-test suite name for
  name against main and the four accessibility receipts driven headed.
- Separating `genet-livery/layout.rs`'s central transaction stays
  deliberately undone. It needed the extracted helpers independently
  receipted first, which Phase 4 has now made possible.
- Margins, paddings, insets and flex-basis still flatten `calc()` at a zero
  percentage basis. The width repair in `78f6bd3eafd` is the shape that fix
  takes. **Landed 2026-09-01 in `9df392f42d8`**: `flex_basis`,
  `length_percentage` and `length_percentage_auto` take the tagged path.
- A block-level replaced element under `box-sizing: border-box` still
  stretches to its container. `3cd0f5abb7b` leaves border-box on Taffy's
  leaf measure on purpose, because a forced size bypasses the ratio-preserving
  min/max clamp of CSS 2.1 10.4 (`box-sizing-replaced-001..003`); the unit
  test pins the stretch so widening the rule is a deliberate change with
  those reftests watching.
- **Landed 2026-09-01.** The inline replaced sizing bug is fixed, and it was
  not about `<img>` versus `<canvas>` at all. `img` carries
  `display: inline-block` from the UA sheet (`lib.rs` CAMBIUM_UA_DEFAULTS)
  and `canvas` does not, so only `img` satisfied the shrink-to-fit predicate.
  An atomic inline root that qualifies was wrapped in a viewport-sized
  containing block, which the very next statement then formatted under
  MaxContent; Buckram bailed on the indefinite inline size and Taffy
  stretched the leaf to the wrapper, with the natural ratio turning 320 into
  480. The wrapper now skips replaced roots: CSS 2.1 10.3.2 gives an inline
  replaced element with `width: auto` its intrinsic width outright, so there
  is no shrink-to-fit step needing a containing block to run in.
  Confirmed by prediction *before* the fix, not after: an inline `<canvas>`
  forced to `display: inline-block` reproduced the failure at 320x320, while
  the plain-inline control and a `vertical-align: bottom` variant stayed
  correct -- three probes of one predicate.

  Two pre-existing bugs this un-masked. Both previously *passed* only because
  their reference half rendered as wrongly as their test half:
  - `css-sizing/grid-item-image-percentage-min-height-computes-as-0`: a
    percentage `min-height` on a replaced grid item resolves to a stretch
    rather than to zero, which is precisely what that test asserts. Proven by
    probe: with the fix the reference half moves 320x320 -> 60x60 while the
    test half stays 320x320.
  - `CSS2/tables/table-anonymous-objects-211`: an `<img>` given
    `display: table-cell` against one inside a cell `<div>`. Mechanism not
    isolated. CSS2/tables is net-neutral across the change, 151 failures
    either side, trading this test against `table-cell-001`.
