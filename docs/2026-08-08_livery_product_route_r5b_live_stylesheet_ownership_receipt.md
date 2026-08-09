# Livery product route R5b live stylesheet ownership receipt

**Date:** 2026-08-08

R5b proves live direct stylesheet reconciliation in the scripted Livery
consumer. It does not make the static Pelt Livery pin a cutover-ready
live-document route.

## Landed boundary

`LiveryCssom::install_live` owns a host-selected `ResourceFetcher`, document
URL, and `ResourceLimits`. On a pending scripted-DOM mutation, the bridge
re-resolves `ResolvedDocumentResources`, reconciles the Livery author sheets,
and rebuilds the cascade before a computed-style or stylesheet-list request.

Each direct `<style>` or `<link>` sheet carries its opaque source element
identity. The browser-facing stylesheet list uses that identity as a stable
key, not as its current document index. Direct sheets preserve their parsed
Livery stylesheet while owner, URL, type, media, and authored text agree;
therefore a CSSOM `insertRule` survives unrelated document-sheet changes.
The stylesheet-list cache keeps the matching `CSSStyleSheet` wrapper and its
`ownerNode` while a linked sheet's URL changes. A failed replacement makes the
entry disappear and makes a held wrapper's `ownerNode` null.

The live list includes only direct document sheets. Imported sheets remain
part of the cascade through the shared resolver but are intentionally absent
from `document.styleSheets`.

## Receipt

`boa_live_cssom_reconciles_inserted_removed_and_media_gated_sheets` proves,
in one mutable `ScriptedDom` and Livery CSSOM session:

- a direct base sheet reports its `ownerNode` and retains an inserted CSSOM
  rule;
- an inserted inline sheet and an inserted linked sheet join the cascade;
- changing link `media` removes and restores its cascade effect;
- removing the inline sheet leaves the base CSSOM mutation intact;
- replacing the linked `href` updates its rule while preserving the sheet
  wrapper and owning element; and
- replacing that `href` with an unavailable sheet removes the direct entry,
  clears the held wrapper's owner node, and restores the retained base rule.

The executed verification ladder is:

```powershell
cargo test -p genet-document-resources --offline --quiet
cargo test -p genet-livery --all-targets --offline --quiet
cargo test -p script-runtime-api stylesheet_cssom_routes_to_handler --offline --quiet
cargo test -p genet-scripted --features livery --offline --quiet
cargo test -p genet-documents --all-features --offline --quiet
```

All listed tests passed. The last two product-facing suites completed with 58
and 44 tests respectively.

## Still outside R5b

R5c subsequently added `CSSImportRule`, imported child stylesheet wrappers,
and the CSSOM `ownerRule` relationship. `@import layer(...)` and `@import
supports(...)` remain diagnostics. Cache revalidation, a shared
redirect/concurrency policy, and dynamic image/font resource replacement are
also outside this slice. The Pelt Livery route remains the deliberately static
opt-in product pin.
