# Livery product route R4 receipt

**Date:** 2026-08-08

This is the product receipt for R0-R4 of the [Livery product route and document
resources plan](2026-08-08_livery_product_route_and_document_resources_execution_plan.md).
Livery remains an explicit Pelt pin. The static route and fullweb default did
not change.

## Local route

Fixture: `ports/pelt/examples/livery-route/index.html`

| Field | Receipt |
|---|---|
| engine | `genet.livery` through Pelt's `livery` feature and `SessionRegistry` |
| headed viewport | 960x640 physical pixels |
| bounded run | `pelt --engine livery --size 960x640 --frames 2 ...` exited 0 with `window=true redraws=2` |
| authored sheets | two inline sheets, `assets/route.css` with `media=screen`, and `assets/print-only.css` with `media=print` |
| image identities | HTML and sheet-relative references both resolve to `resources/servo_64.png` |
| font identity | sheet-relative `components/genet-layout/Ahem.ttf` |
| diagnostics | none, asserted from `LiveryDocumentSession::resource_set()` |

`static_viewer::livery_route_tests::local_livery_route_keeps_resource_identity_and_interaction_after_resize`
proves the actual Pelt registry construction, source order, screen-media
selection, image paint, linked font attribution, resize, fragment-link hit
testing, and viewport scrolling. It passes under `pelt-desktop`'s `livery`
feature.

The separate-border caption is visible in the headed capture. At the headed
viewport, the collapsed caption wraps into four distinct lines; the last line
clears the grid. The product-scene assertion records that the final caption
baseline precedes the first cell baseline by at least 16px, and
the headed capture shows the same non-overlapping result.

This is not a collapsed-border correctness pass. Border-conflict resolution,
metrics, and final paint semantics remain the active K4g deferral. R4 records
the case so the opt-in route cannot hide that boundary behind a simpler table.

## Merely headed route

The product route loaded `https://merelyllc.com` at the same viewport. The
served document identified its Google Fonts stylesheet and `/site.css`; the
site sheet supplied parchment `#f0ebdd` and oxblood `#6e1712`. The headed
capture visibly carries both colors.

| Field | Receipt |
|---|---|
| engine | `genet.livery` |
| headed viewport | 960x640 physical pixels |
| bounded run | exited 0 with `window=true redraws=2` |
| diagnostics | no process error or stderr; response metadata remains outside the byte-only R4 contract |

## Preserved artifacts

The headed PNGs remain outside Git:

- `C:\t\livery-r4-local-headed.png`
- `C:\t\livery-r4-merely-headed.png`
- `C:\t\pelt-livery-product-route-fresh-k4g-caption-and-table.png`

The screenshot capture tool stores a scaled 482x350 image of each 960x640
headed client window. The final-binary bounded-run logs are kept outside Git:

- `C:\t\livery-r4-final-10d8a9ad-fbbf-4271-a22b-c001eb295314`
- `C:\t\livery-r4-live-final-0e7da325-a0a6-4eff-b646-2ebf93930855`

Each directory contains the Pelt stdout and stderr from the corresponding
two-frame run.

## Boundary after R4

This receipt proves that Pelt can instantiate and present the opt-in route with
host-owned linked resources. It does not claim F4 parity, cutover readiness,
CSS `@import`, response metadata, dynamic resource replacement, or collapsed
border completion. Those remain R5 and their named ownership gates.
