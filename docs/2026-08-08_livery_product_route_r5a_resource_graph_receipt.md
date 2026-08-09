# Livery product route R5a resource-graph receipt

**Date:** 2026-08-08

R5a extends the opt-in Livery route's shared resource graph. It does not make
the static pin an F4 or cutover-ready route.

## Landed boundary

`ResourceFetcher` now has an additive `fetch_response` method. A response
carries final URL, optional content type, and collected bytes. Existing hosts
that implement only `fetch` retain the requested URL as final identity and an
unknown content type.

The shared resolver:

- retains requested and redirect-final stylesheet identities;
- rejects a known non-`text/css` linked or imported response;
- expands leading `@import` sheets before their parent, preserving source
  order and using the final parent URL as the next relative base;
- wraps an import's media condition in `@media`, while retaining the parent
  link media as the document-media gate;
- diagnoses import cycles, unavailable/invalid sheets, out-of-order imports,
  and unsupported `layer(...)` / `supports(...)` conditions; and
- exposes `ResourceLimits` for maximum import depth and stylesheet bytes.

`LiverySessionEngine::with_resource_limits` carries those host-selected bounds
into each static Livery session.

Pelt's `LocalFetcher` forwards HTTP response final URL and content type from
netfetcher. Both static and Livery document loads use the final document URL
as the base for their linked resources.

## Receipts

`genet-document-resources` proves redirected nested imports, sheet-relative
asset resolution, media wrapping, non-CSS rejection, fetch-free diagnostics,
and cycle detection.

`genet-documents` proves that the Livery session applies an imported sheet
before its redirected parent sheet, and that a redirected document resolves a
linked stylesheet from its final document identity.

## Still outside R5a

The static Livery session has immutable DOM and author-sheet inputs. It does
not implement dynamic `<link>` insertion/removal, media-attribute mutation,
CSSOM `ownerRule` / owner-sheet relationships for imports, cache invalidation,
or resource replacement/removal. Those are R5b live-document ownership work.

The direct-sheet portion of that follow-up landed in the R5b receipt. Import
`ownerRule` relationships, cache policy, and the static Pelt product boundary
remain outside R5b.

`@import layer(...)` and `@import supports(...)` are recorded diagnostics,
not partial cascade behavior. Redirect-count policy remains owned by the host
transport, whose netfetcher path currently has its own 20-hop cap; the shared
resolver's synchronous traversal is serial and has no independent concurrency
policy.
