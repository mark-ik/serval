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
reader shape over any profile-neutral `LayoutDom`. The implementation has one
runtime dependency, `layout_dom_api`; parsing, layout, paint, storage, and
network policy stay with callers.

## Text anchors

Fleece 0.2 also exposes `TextAnchor` evidence on every `AnchoredBlock` that
maps to one contiguous source segment. Its sibling `TextPositionSelector` and
`TextQuoteSelector` values refer to the versioned `FleeceDomTextV1` stream:
logical DOM-order, decoded visible text with markup removed and whitespace
collapsed. Positions are half-open Unicode code-point offsets. Reader blocks
that are synthetic or combine discontinuous source text have no anchor.

`ExtractionOptions::quote_context` controls the maximum surrounding context in
code points; Fleece preserves extended grapheme boundaries while truncating it.
Fleece names neither source URLs nor Web Annotations. Consumers supply source
identity and serialize the sibling selectors if they need an annotation.

## License

MPL-2.0
