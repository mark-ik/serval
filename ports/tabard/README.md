# tabard

Tabard is the theme-authoring port of the Genet engine.

A tabard is the garment that displays a household's livery. This port is where
a household's livery gets authored: seeds in, liveries out. It composes what
already interlocks: [tinct](https://crates.io/crates/tinct) derives the full
palette from a few seed colours, [illume](https://crates.io/crates/illume)
emits syntax spans that the derived syntax palette colours, and palette-aware
icons can recolour at render time.

Tabard authors portable theme artifacts. Livery consumes stylesheets Tabard
emits. Pelt has an optional named preview receipt for those artifacts, but does
not own their model.
Tinct remains the small, serde-only derivation crate. Illume continues to name
syntax spans rather than decide their appearance.

## Current artifact

The first implementation is deliberately portable and library-only:

- Theme owns a name and tinct::Seeds, then derives Tinct's normal-contrast
  base palette.
- Theme::design_tokens emits a typed DTCG 2025.10 color document. Every token
  has an explicit color type and a structured sRGB value, while the name,
  seeds, and derivation choice live under org.merely.tabard in $extensions.
- Theme::css_custom_properties emits a deterministic :root stylesheet with the
  same palette as --tabard-color-* custom properties. Livery can consume it
  as an ordinary author sheet.
- Pelt's optional `tabard-preview` receipt maps those generic properties onto
  Pelt-owned Chrome roles. It proves the shell recolors while the focused
  document, session history, tabs, and content aperture remain held. It is not
  a persistent appearance setting and does not recolor document content.
- Pelt's optional `tabard-reader-preview` receipt maps the same portable
  palette onto Reader's existing host palette. It proves a Fleece article
  keeps its held source and route-restoration behavior while Pelt supplies the
  Reader colors. Fleece and `genet-documents` do not depend on Tabard.

Recorded 2026-08-28: both named headed Windows consumer receipts passed at
960x640. `tabard-preview` completed after three redraws with compositor digest
`d0affd3746b03554`; `tabard-reader-preview` completed after nine redraws with
digest `ea505825544747b9`. The latter held a `genet.reader` Fleece article
beside a `genet.livery` neighbor and retained the Reader inspector's lineage.
These receipts validate consumer seams, not persistence or a platform theme
policy.

This slice deliberately does not add host theme structs, syntax-color policy,
icon policy, persistence, imports, or a DTCG resolver.
Those become consumer work after the shared artifact has a stable shape.

## License

MPL-2.0 (see the repository `LICENSE`)
