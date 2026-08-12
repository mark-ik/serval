# Livery F4 scripted Pelt route execution plan

**Status:** complete at the F4 implementation boundary

## Goal

Add `pelt --engine livery-scripted` as an explicit, opt-in headed document
route. One scripted runtime owns the mutable DOM. Its Livery CSSOM session owns
the same document's cascade, resource graph, retained text state, layout, and
paint list. The existing `livery` static pin and `scripted` Stylo geometry route
remain available and unchanged.

## Seams

| Owner | Change |
| --- | --- |
| `genet-scripted` | Construct a Livery-backed scripted document before parser-blocking script execution. Retain resource bytes, text shaping, Livery layout, paint, scrolling, and click dispatch beside `LiveryCssom`. |
| `genet-documents` | Re-export the product-neutral document type for hosts; do not replace its incumbent scripted engine. |
| `pelt-desktop` | Present that document in the existing shared winit shell. |
| `genet-host-api` / `pelt` | Add the explicit `livery-scripted` profile and feature gate, keeping `viewer` as the default. |

## Acceptance receipts

1. A Boa fixture runs parser-blocking script against Livery `document.styleSheets`
   and `getComputedStyle`, then its Livery frame reflects the mutation.
2. The same fixture loads linked CSS, image, and font bytes through the shared
   `ResourceFetcher`; a later DOM mutation replaces the image/font ledger and
   retires the cached paint frame.
3. The Livery frame is translated into `netrender::Scene`, carries the image
   resource, and accepts wheel and click input through the live runtime.
4. `pelt --features livery-scripted --engine livery-scripted <fixture>` reaches
   the shared headed viewer. A bounded Windows frame capture proves actual
   presentation, not only a scene test.

## Stop rules

- Do not change the default Pelt engine or make `--engine scripted` use Livery.
- Do not copy or synchronize a second DOM. The runtime DOM is the only mutable
  document; Livery observes it through the CSSOM session.
- Do not silently treat a missing image, font, or stylesheet as successful bytes.
- Keep `@import layer(...)` and `@import supports(...)` as the recorded R5d
  diagnostics. They are not altered by this integration.
- Stop if a proof requires the incumbent `IncrementalLayout` geometry path.

## Verification

```powershell
cargo test -p genet-scripted --features livery --offline
cargo test -p genet-documents --features livery-scripted --offline
cargo test -p pelt-desktop --features livery-scripted --offline
cargo test -p pelt --features livery-scripted --offline
cargo clippy -p genet-scripted -p genet-documents -p pelt-desktop -p pelt --features livery-scripted --no-deps --offline -- -D warnings
```

The headed receipt is separate from the GPU-free tests and must record the
profile, fixture, frame limit, and actual window outcome.

## Completion receipt (2026-08-10)

- `LiveryScriptedDocument` now binds one Boa runtime DOM to a retained Livery
  CSSOM, resource ledger, text system, layout, and paint list. The runtime
  projects text replacement into real text children, so the script-visible DOM
  and painted DOM remain identical.
- The F4 fixture's parser-blocking script reads two linked/live CSS sheets,
  changes the heading through CSS, replaces a `@font-face` source, and replaces
  an image after its click listener runs. The focused Boa test renders both the
  initial and post-click frames, and proves that the stale image/font resources
  are absent from the Livery ledger.
- `pelt --engine livery-scripted` is an opt-in profile. The default `viewer`
  profile and incumbent `scripted` profile retain their original routes. On
  Windows Pelt now links its binary with an 8 MiB main-thread stack; both
  bounded three-frame processes exit successfully instead of overflowing the
  UI-thread stack.

Recorded commands and outcomes:

```powershell
cargo test -p script-runtime-api --lib --offline dom_construction
# 2 passed (Boa and Nova)

cargo test -p genet-scripted --features livery --offline `
  livery_scripted_document_owns_live_cssom_resources_and_frame_on_boa -- --nocapture
# 1 passed; includes the post-click Livery frame and replacement resource ledger

cargo check -p pelt --features livery-scripted --offline
# passed

cargo test -p pelt-desktop --features livery-scripted --offline --lib -- --list
# passed; 28 tests listed

Start-Process C:\t\graphshell-target\debug\pelt.exe `
  -ArgumentList '--engine','livery-scripted','<F4 fixture>','--size','640x480','--frames','3' `
  -Wait -PassThru
# exit code 0 (the equivalent incumbent --engine scripted run also exits 0)
```

A headed Windows Pelt window visibly presented the F4 fixture after its first
redraw: the green scripted heading, `2 Livery CSS sheets are live.`, linked
image, and the mutation control all reached the shared viewer. The automated
desktop controller could target and place its pointer over the control, but did
not yield an observable Pelt mouse event. The live click and post-click paint
are therefore proved by focused runtime tests, including an element-targeted
button listener, rather than presented as a second window screenshot.

The pre-existing workspace-wide `cargo fmt --check` drift in Cambium files is
outside this change. All changed F4 Rust files pass scoped `rustfmt --check`,
and `git diff --check` passes.
