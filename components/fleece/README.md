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

## License

MPL-2.0
