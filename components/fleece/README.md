# fleece

**fleece** is live-document extraction for the Genet engine.

To fleece a document is to shear the readable substance off a rendered page:
article text, metadata, tables, and structure come away clean, and the
document keeps standing. This is the successor name for `genet-extract`'s
lane: render-free content extraction over the profile-neutral LayoutDom.
Analyze, don't paint.

The boundaries are the point: not import (mere's `import` migrates *stored*
browser data; fleece works a *live* document), not crawl (the frontier
decides what to visit; fleece decides what a visited page said), and not
illume (the lexer names spans in source text; fleece harvests rendered
documents).

It exposes the flat `PageExtract` index shape and the structured `Article`
reader shape over any profile-neutral `LayoutDom`. Its runtime dependencies are
`layout_dom_api` and `unicode-segmentation`; parsing, layout, paint, storage,
and network policy stay with callers.

## Text anchors

Fleece 0.2 also exposes `TextAnchor` evidence on every `AnchoredBlock` that
maps to one contiguous source segment. Its sibling `TextPositionSelector` and
`TextQuoteSelector` values refer to the versioned `FleeceDomTextV1` stream:
logical DOM-order, decoded visible text with markup removed and whitespace
collapsed, with one ASCII separator between contributing DOM text nodes and no
other element-boundary characters. Positions are half-open Unicode code-point
offsets. Reader blocks that are synthetic or combine discontinuous source text
have no anchor.

`ExtractionOptions::quote_context` controls the maximum surrounding context in
code points; Fleece preserves extended grapheme boundaries while truncating it.
Fleece names neither source URLs nor Web Annotations. Consumers supply source
identity and serialize the sibling selectors if they need an annotation.

`text_fragment` projects quote evidence into a `:~:text=...` directive
component using the WICG draft revision pinned in its module documentation. It
does not compose a source URL or implement navigation, activation, indication,
or script-visible URL privacy; those remain browser-host responsibilities.

## Structured data

Fleece 0.3 harvests page-carried JSON-LD syntax and HTML Microdata items into
`StructuredData`. `types` preserves every declared `@type` or `itemtype` string
without shortening or expansion, while `id` preserves a raw `@id` or `itemid`.
The original JSON value, including `@context` and unknown members, remains in
`value`; explicit `@graph` objects are also exposed in source order.

Microdata follows the HTML item/property traversal, including `itemref`, nested
items, repeated properties, element-specific values, duplicate suppression,
and cycle protection. URL-valued attributes and identifiers remain raw source
strings because Fleece has no source URL or document-base authority.

This is syntax harvesting, not JSON-LD 1.1 expansion, RDF construction,
vocabulary reasoning, URL resolution, or remote-context loading.

## License

MPL-2.0
