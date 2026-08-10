# fleece

Name reservation for **fleece**, live document extraction for the Genet
engine.

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

Lives in the [genet](https://github.com/merely-made/genet) workspace at
`components/fleece`. No implementation yet.

## License

MIT OR Apache-2.0
