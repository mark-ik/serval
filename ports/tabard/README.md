# tabard

Name reservation for **tabard**, the theme-authoring port of the Genet
engine.

A tabard is the garment that displays a household's livery. This port is
where a household's livery gets authored: seeds in, liveries out. It composes
what already interlocks: [tinct](https://crates.io/crates/tinct) derives the
full palette from a few seed colours (OKLCH ladders, contrast-gated text
roles); [illume](https://crates.io/crates/illume) emits `(range, kind)` spans
that the derived syntax palette colours; and icons whose format takes a
palette at render time, so a theme change recolours them for free.

Out the other side: **W3C Design Tokens** (the non-negotiable interchange
form), CSS custom-property stylesheets consumed by livery, and theme structs
for hosts. Preview is live, through the engine itself.

The boundaries are the point: not tinct (the derivation math stays a small,
serde-only crate; tabard is its authoring surface), not livery (the CSS
engine consumes stylesheets tabard emits; it does not author them), not
illume (the lexer names spans; tabard decides what they wear), and not pelt
(the viewer previews documents; tabard fits them out).

Lives in the [genet](https://github.com/merely-made/genet) workspace at
`ports/tabard`. No implementation yet.

## License

MIT OR Apache-2.0
