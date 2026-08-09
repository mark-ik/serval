# Livery product route R5d fetch policy, live assets, and rule objects receipt

**Date:** 2026-08-09

R5d closes the named resource-graph work left after R5c. It does not make the
Pelt Livery pin a scripted product route or change the default engine.

## Landed boundary

`ResourceFetchPolicy` is host configuration for redirects, concurrent remote
fetches, decoded response bytes, and timeout. `LocalFetcher` uses a shared
default remote client, so a document and its linked CSS, images, and fonts use
one cache and one policy. `LocalFetcher::with_resource_policy` creates an
isolated shared client for a host session or persona. The client uses
netfetcher's in-memory RFC 9111 cache, so an immediately stale ETag response
revalidates with `If-None-Match` and a 304 returns the retained 200 body.

The resource graph now has a typed delta. A live Livery CSSOM installation can
deliver the full next ledger plus added, updated, and removed resources to its
host-owned sink. `LiveryDocument` replaces the complete image/font ledgers;
image removal invalidates layout and paint, while font replacement rebuilds
the document's font context from the surviving source bytes.

The CSSOM projection covers all Livery-supported parsed rule types:

- `CSSStyleRule`, including `selectorText`, read-only `style`, and `cssText`;
- `CSSImportRule`, including the R5c child-sheet owner graph;
- `CSSMediaRule` and `CSSContainerRule`, including nested `cssRules`;
- `CSSKeyframesRule` and `CSSKeyframeRule`, including name/key/declaration
  reads and parent-rule identity.

Top-level `CSSStyleSheet.insertRule` and `deleteRule` remain the mutation
entrypoint. Group-rule editing is not represented as successful mutation until
it can update Livery's parser state and the resource graph together.

## Focused receipts

- `configured_fetcher_revalidates_a_shared_cached_response` proves one policy
  client sends an ETag conditional request and serves the stored body after a
  304.
- `configured_fetcher_enforces_redirect_and_body_limits` proves a zero redirect
  cap rejects a 302 and a decoded body cap rejects an oversized response.
- `live_cssom_replaces_image_and_font_resources_after_a_dom_reconciliation`
  proves a live resource sink receives image and font replacements.
- `boa_cssom_projects_every_livery_rule_object` proves the full supported rule
  object set, nesting, parent identity, and rule-specific reads through Boa.

## Still outside this receipt

`@import layer(...)` and `@import supports(...)` remain explicit diagnostics.
Pelt has no scripted Livery product-route session yet. Those are separate F4
integration work, not hidden success claims for this resource boundary.
