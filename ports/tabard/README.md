# tabard

Tabard is the theme-authoring port of the Genet engine.

A tabard is the garment that displays a household's livery. This port is where
a household's livery gets authored: seeds in, liveries out. It composes what
already interlocks: [tinct](https://crates.io/crates/tinct) derives the full
palette from a few seed colours, [illume](https://crates.io/crates/illume)
emits syntax spans that the derived syntax palette colours, and palette-aware
icons can recolour at render time.

Tabard authors portable theme artifacts. Livery consumes stylesheets Tabard
emits. Pelt may later preview those artifacts, but does not own their model.
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

This slice deliberately does not add a Pelt preview, host theme structs,
syntax-color policy, icon policy, persistence, imports, or a DTCG resolver.
Those become consumer work after the shared artifact has a stable shape.

## License

MIT OR Apache-2.0
