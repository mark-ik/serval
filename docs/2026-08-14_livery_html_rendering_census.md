# Livery HTML rendering and presentational-hint census

**Date:** 2026-08-14

**Scope:** the current HTML rendering section as it applies to Genet's
implemented static/fullweb DOM and Livery's active property consumers.

**Execution plan:** [Livery HTML presentational hints](2026-08-08_livery_html_presentational_hints_execution_plan.md)

**Normative source:** [HTML rendering](https://html.spec.whatwg.org/multipage/rendering.html#rendering), including the separate [CSS user-agent sheet and presentational-hint](https://html.spec.whatwg.org/multipage/rendering.html#the-css-user-agent-style-sheet-and-presentational-hints) algorithms.

## Boundary

This census keeps four kinds of behavior separate:

1. author presentational hints, which enter Livery's typed cascade at
   `AuthorPresentationalHint`;
2. UA-origin defaults, which belong in `CAMBIUM_UA_DEFAULTS` or a future
   dynamic UA rule provider;
3. used-value or element algorithms, which cannot be represented by a CSS
   declaration alone; and
4. missing property, selector, document-resource, or element capabilities.

Only the first category is implemented in
`genet-livery/src/presentational_hints.rs`. A declaration is not counted when
its property has no working downstream consumer.

## Implemented author presentational hints

| HTML surface | Projection | Evidence |
|---|---|---|
| `body[marginheight]`, then `body[topmargin]` | physical top and bottom pixel margins, using the first existing source | parser, invalid-first-source, precedence, mutation, CSSOM, layout |
| `body[marginwidth]`, then `body[leftmargin]` | physical left and right pixel margins, using the first existing source | parser, precedence, mutation, CSSOM, layout |
| `body[bgcolor]`, `body[text]` | typed background and foreground colors | legacy-color parser, precedence, mutation |
| `pre[wrap]` | `white-space-collapse: preserve; text-wrap-mode: wrap` | computed style, precedence, removal mutation |
| `br[clear]` | `clear: left`, `right`, or `both` | computed style, precedence, mutation |
| `font[color]`, `font[size]` | typed color and clamped CSS absolute-size keyword | legacy parser, CSS keyword round trip, precedence, mutation, CSSOM, layout |
| `hr[align]`, `[color]`, `[noshade]`, `[size]`, `[width]` | margins, border style/width, height, width, and color per the HTML algorithm | computed style, shorthand CSSOM, precedence, mutation, layout |
| heading and `p[align]`; `div[align]`; `center` | text alignment plus the separate descendant used-margin owner where HTML defines it | computed style, ownership, mutation, used layout |
| `table[cellspacing]`, `table[cellpadding]` | border spacing and table-owned cell padding | nested-table ownership, precedence, mutation, layout |
| table and table-part dimensions | `table` width/height, `col` width, row-group/row height, cell width/height | HTML dimension parsers, percentages, precedence, mutation |
| table and table-part alignment | table float/logical centering; caption side; group/row/cell text and vertical alignment | computed style, logical projection, precedence, mutation |
| `td[nowrap]`, `th[nowrap]` | `text-wrap-mode: nowrap` when the legacy condition applies | computed style and removal mutation |
| table color, border, frame, and rules families | typed color, physical/logical border, collapse, and rule declarations | computed style, K4g candidate receipt, mutation, focused WPT |
| image and embedded-content dimensions | width/height on the applicable `img`, image button, `embed`, `object`, `iframe`, `video`, and `canvas` surfaces | computed style, precedence, mutation, replaced layout |
| replaced-content ratio, alignment, spacing, border, frame border | aspect ratio, float/vertical alignment, physical margins and borders | computed style, CSSOM, mutation, intrinsic layout, paint, focused WPT |

The `<br>` declaration is implemented and consumed by Buckram's block
clearance path. Full `<br>` line-breaking and inline-clear behavior still
depends on HTML element box generation, so this row is not a complete element
conformance claim.

For replaced `align`, the current HTML Standard maps `bottom` to
`vertical-align: bottom`. The checked-in WPT currently expects `baseline` for
that one case; Livery follows the current standard and records the differential
rather than copying the stale expectation.

## Assigned capability gaps

| HTML surface | Missing authority | Reason it is not a dormant declaration |
|---|---|---|
| `body[background]` and table-part `background` | document URL/base-URL input at the hint adapter | HTML requires legacy URL parsing and serialization relative to the document. `LayoutDom` exposes neither document URL nor base URL. Emitting the raw attribute would be observably wrong. |
| `body[link]`, `[vlink]`, `[alink]` | link/visited selector state and visited-color policy | Livery has active/hover/focus states but no link or visited pseudo-class. |
| `font[face]` | font-family list grammar and fallback consumer | Livery's `FontFamily` intentionally stores one seed family and rejects comma-separated lists. |
| `ul[type]`, `ol[type]`, `li[type]` | complete `list-style-type` vocabulary and marker paint | Livery has only `none`, `disc`, and `decimal`; Buckram creates an empty marker box and does not consume the computed marker style. |
| `ol[start]`, `ol[reversed]`, `li[value]` | counter properties, reversed-list start calculation, and marker text | counter reset/set semantics and marker generation are absent. |
| container `frame` margin and scrolling fallbacks for `body`/`iframe` | browsing-context/container state | the required values come from the containing frame, not the element's local attributes alone. |
| selected-source dimensions for `picture`/`img[srcset]` | image source-selection state | the static adapter has no selected-source result to project. |
| actual `embed`, `object`, `iframe`, `video`, and image-button content | element/browsing-context implementations | their CSS dimensions are present; the replaced content or nested context is not. |
| `marquee` mappings and behavior | marquee element/widget | adding background, size, and spacing declarations would not create the required scrolling box behavior. |
| `frame` and `frameset` rendering | frame-tree and nested browsing contexts | HTML defines element algorithms rather than ordinary CSS-only mappings. |
| image maps | hit testing against parsed map areas | this belongs to element interaction, not cascade. |

## UA-origin defaults

`CAMBIUM_UA_DEFAULTS` currently owns the active static defaults for structural
block and table display, list-item display, body 8px fallback margins, heading
and paragraph spacing, basic list padding/style, preformatted whitespace,
`<hr>` defaults, iframe inset borders, and the attribute-sensitive table rule
colors required by PH3.

The following current HTML rules remain UA-origin gaps rather than missing
author hints:

- the broader element display/hidden-default sheet beyond the bounded Cambium
  structural set;
- `th`'s context-sensitive default alignment;
- nested-list marker variants and real marker generation;
- `legend` alignment through `justify-self`; and
- `video { object-fit: contain }` until `object-fit` has an active property and
  replaced-media consumer.

## Used-value, quirks, and host routing

HTML's `align descendants` rule already crosses a typed metadata seam into
Buckram's generic used-margin solver. It does not appear in computed CSS.
Table collapsed-border conflict resolution remains K4g work after PH3 supplies
the candidates.

Quirks-only width, table, image, and legacy alignment differences remain on the
document-mode/quirks lane. Form-control appearance and metrics belong to the
form widget lane. Media playback, nested navigation, and frame scrolling belong
to their element or browsing-context hosts.

## Open evidence wall

The author-hint census is assigned end to end, but the parent execution plan is
not closed. The named `table-anonymous-objects-059` through `-098` family is
still 10/40 because Buckram's anonymous-table construction and sizing remain
wrong after both comparison sides receive the same HTML hints. The broad pixel
dimension harness also fails before assertions in the current Boa DOM/script
runner. The 10/40 result was remeasured with the rebuilt PH5 runner on
2026-08-14. These are separate from missing hint mappings.
