# Livery product route R5c imported stylesheet CSSOM receipt

**Date:** 2026-08-09

R5c projects the already-resolved import graph into CSSOM ownership. It does
not add general CSS rule wrappers or make the Pelt Livery pin a live scripted
product route.

## Landed boundary

`genet-document-resources` now records every leading import rule, including
its authored and resolved URL, media condition, optional loaded child-sheet
identity, and the parent import slot of each child. This remains neutral
resource-graph data; the resolver still owns fetching, MIME checks, limits,
and diagnostics.

Livery derives stable string CSSOM keys from direct document owners and import
paths. The runtime exposes loaded imports as `CSSImportRule` entries at the
start of the parent's rule list. An import rule returns its child through
`styleSheet`; the child returns the same wrapper through `ownerRule` and has
no `ownerNode`. Child sheets remain outside `document.styleSheets`.

CSSOM rule indexes include retained import rules. Ordinary rules in an
imported child can use `insertRule` and `deleteRule`; mutation of the retained
`@import` rule itself is deliberately rejected because resource replacement
belongs to the resource graph rather than the selected CSS parser.

## Receipts

- `imported_sheets_precede_their_parent_and_keep_final_identities` proves
  nested parent/child resource identities and import media in the shared
  resolver.
- `imported_stylesheet_cssom_relationships` runs against Boa and Nova, proving
  `CSSImportRule`, `styleSheet`, `ownerRule`, `parentStyleSheet`, null child
  `ownerNode`, and direct-list exclusion.
- `boa_live_cssom_exposes_imported_sheet_owner_graph` proves the same graph
  through a live Livery session, including a mutation of an ordinary rule in
  the imported child.

The verified walls are:

```powershell
cargo test -p genet-document-resources --offline --quiet
cargo test -p script-runtime-api --offline --quiet
cargo test -p genet-scripted --features livery --offline --quiet
cargo test -p genet-livery --lib --offline --quiet
cargo test -p genet-documents --all-features --offline --quiet
cargo test -p pelt-desktop --all-targets --features livery --offline --quiet
```

They passed with 9 resource tests, 116 runtime tests, 59 scripted-Livery
tests, 81 native Livery library tests, 44 document tests, and 28 Pelt desktop
tests.

## Still outside R5c

`CSSRuleList.item()` does not yet project ordinary style, media, keyframes, or
other CSS rule objects. `@import layer(...)` and `@import supports(...)` stay
explicit diagnostics. Cache revalidation, a shared redirect/concurrency
policy, dynamic image/font resource replacement, and a scripted Pelt product
session remain open R5/F4 work.
