# Livery HTML presentational hints execution plan

**Date:** 2026-08-08

**Status:** Closed 2026-08-21 on PH0 through PH5, by ruling. PH0 through
PH5 were implemented through 2026-08-14 and the checked-in census assigns
every currently applicable HTML rendering surface. Table and table-part
dimensions use HTML's legacy dimension algorithms, `table[align]` maps to
float or harvested logical margins, and applicable legacy alignment owners
carry HTML's separate `align descendants` policy to Buckram's generic
used-margin solver. The 40-file `table-anonymous-objects-059` through `-098`
family is no longer a condition of this plan: its 10/40 of 2026-08-12 was
lost to a K5 positioning regression at the 2026-08-15 integration merge
(0/40 on `main` from 2026-08-16 until the 2026-08-21 repair restored 10/40),
and the remaining 30 are not hint work. They are recorded as conformance debt
in the fullweb cutover register and in the master plan's K5 regression
ledger; see the correction under PH1 below. Buckram owns the generic
used-value calculation but not the HTML mappings or owner selection.

**Parent:** [Livery fullweb cutover and the servo-* retirement](2026-07-24_livery_fullweb_cutover_and_servo_retirement_plan.md)

**Defect source:** [Buckram K4 CSS tables execution plan](2026-07-28_buckram_k4_css_tables_execution_plan.md#the-largest-remaining-family-is-not-what-its-name-says)

**Entry dependency:** [Livery contextual color computation plan](2026-07-28_livery_contextual_color_computation_plan.md). C1 lands first because
both slices change generated color fields and the cascade representation.

## Ruling

CSS-representable HTML presentational attributes become declarations before
computed style. HTML's explicitly used-value-only algorithms cross a separate
typed metadata seam. Neither becomes an ad hoc late geometry correction.

The HTML adapter derives typed declarations from element attributes and feeds
them into Livery's cascade at the author presentational-hint origin. Livery
remains DOM-language neutral; it knows the origin and priority, not the names
`cellpadding`, `bgcolor`, or `align`. For `align descendants`, the adapter also
selects the deepest applicable HTML owner and carries a line-left, center, or
line-right token beside computed style. Buckram applies that token only while
solving used margins.

The original defect hypothesis came from the 40-file
`table-anonymous-objects-059` through `-098` family. Every HTML comparison table
carries `cellpadding="0" cellspacing="0"`; ignoring both was a real systematic
drift. The post-implementation measurement disproves the stronger claim that
Buckram's anonymous table geometry was already correct: only 10 files pass,
all 20 variants with the CSS-generated table on top fail, and another 10 fail
with that table underneath because its geometry protrudes. Presentational hints
remove the HTML-side drift; the remaining 30 are a K4 construction/sizing wall.

## Standards boundary

CSS Cascade Level 5 gives presentational hints a special-purpose author
presentational-hint origin between normal user and normal author declarations.
It is an independent cascade origin, not an unlayered author rule with a
small specificity. This distinction matters because otherwise cascade layers
can accidentally make hints stronger than authored CSS.

HTML's rendering rules define the mappings, including:

- table `cellspacing` to table `border-spacing`;
- table `cellpadding` to the four paddings on corresponding `td`/`th` cells;
- table, group, row, and cell dimensions to CSS dimensions;
- table-part `align` values to text alignment and descendant alignment; and
- the wider legacy color, border, spacing, and embedded-content families.

Normative anchors:

- [CSS presentational-hint origin](https://drafts.csswg.org/css-cascade-5/#preshint)
- [HTML rendering and presentational hints](https://html.spec.whatwg.org/multipage/rendering.html#the-css-user-agent-style-sheet-and-presentational-hints)
- [HTML table rendering](https://html.spec.whatwg.org/multipage/rendering.html#tables-2)

HTML labels many of these as expected default rendering rather than document
conformance requirements. Genet's standards ledger records that distinction;
it does not use it as a reason to reinterpret the mapping.

## Live defects

1. `Origin` in `components/livery/src/cascade.rs` contains only `UserAgent`,
   `User`, and `Author`.
2. `genet-livery/src/style.rs` matches stylesheet rules and inline style, but
   exposes no document-language declaration provider.
3. table attributes used by the HTML model (`rowspan`, `colspan`, `span`) are
   correctly normalized into Buckram topology, while CSS-representable
   attributes never enter cascade.
4. replaced-image `width` and `height` are applied late in
   `genet-livery/src/layout.rs` when CSS computes to `auto`. This produces a
   useful bounded result but bypasses cascade, computed style, invalidation,
   and CSSOM.
5. `cellpadding` is cross-element: one table attribute contributes
   declarations to its corresponding cells. Treating it as a rule on the
   table cannot implement the HTML mapping.

## Target contracts

The exact names can change in PH0. The ownership may not:

```rust
pub enum Origin {
    UserAgent,
    User,
    AuthorPresentationalHint,
    Author,
}

pub trait PresentationalHintProvider<Id> {
    fn declarations_for(&self, id: Id) -> PresentationalDeclarations;
}

pub struct PresentationalDeclarations {
    pub declarations: Vec<Declaration>,
    pub diagnostics: Vec<PresentationalHintDiagnostic>,
}
```

Ownership: the `Origin` variant lands in `livery`'s cascade; the provider
trait and the HTML mapping implementation land in `genet-livery`; Buckram
sees neither.

Required invariants:

1. Hints cannot be `!important`, cannot define custom properties, and cannot
   enter a cascade layer. The seam asserts this on provider output rather
   than trusting adapters; `Declaration` carries an `important` flag today.
2. Normal author declarations and style attributes override hints.
3. Hints override normal user and UA declarations as CSS Cascade requires.
4. `revert` treats the hint origin as part of author rollback;
   `revert-layer` does not expose it as a layer. If those keywords are not yet
   implemented, fixtures remain red and named.
5. Hints affect computed style but do not appear in a stylesheet's rule list
   or the element's `style` attribute.
6. Attribute mutation invalidates the element and every dependent target,
   such as the cells affected by table `cellpadding`.
7. Invalid legacy values are ignored under the HTML mapping rules and remain
   diagnosable without becoming invalid CSS declarations.

## Execution gates

| Gate | Outcome |
|---|---|
| PH0 | cascade origin and adapter contract |
| PH1 | `cellspacing` and cross-element `cellpadding` |
| PH2 | table dimensions and alignment, complete 2026-08-12 |
| PH3 | table color, border, frame, and rules families |
| PH4 | replaced and embedded element hints; layout fallback deletion |
| PH5 | broader HTML hint census, mutation closure, and ledger update |

### PH0. Cascade contract

Add the author presentational-hint origin to Livery's priority model and a
DOM-neutral provider seam to `genet-livery` style resolution. Use typed
`Declaration` values; do not create synthetic selector strings or a hidden
stylesheet.

Fixtures establish ordering against UA, user, layered and unlayered author
rules, inline style, animation, and `!important`. A hint's source order is
only a deterministic tie-breaker inside the hint origin.

**Receipt:** pure cascade tests prove every origin ordering, and a retained
document reports a hinted computed value without adding a CSSOM rule.

**Stop:** do not spell this as `Origin::Author + Specificity(0)`. CSS Cascade
Level 5 made the hint origin separate specifically to avoid layer ambiguity.

### PH1. Table spacing and cell padding

Implement the defect that exposed the seam:

- parse `table[cellspacing]` as a non-negative integer pixel length for
  `border-spacing`;
- parse `table[cellpadding]` as a non-negative integer pixel length;
- apply that padding to corresponding `td` and `th` cells, respecting nested
  tables and the HTML table association; and
- let any authored cell padding override the hint.

The provider may precompute a table-to-cell dependency index for one style
resolution. It must not ask Buckram's generated box tree which cells belong to
the table; hints precede box generation.

**Receipts:** direct cascade fixtures, nested-table fixtures, and attribute
mutation are green. The full `table-anonymous-objects-059` through `-098`
family is measured at 10/40. That result is not credited as PH1 completion:
the HTML comparison side now receives zero spacing and padding through computed
CSS, while the remaining mismatches are on CSS anonymous-table construction
and sizing and return to K4.

**Correction, 2026-08-21.** The 30 could not return to K4: K4 had been
accepted at `610df0981a8` on 2026-08-10, two days before that sentence was
written. The 10/40 was also never reproducible from a committed tree. No
ledger for the 08-12 or 08-14 run exists under `testing/genet/wpt-ledger`;
the 2026-08-16 full-`css` ledgers, built after the 08-14 checkpoint
`ec27260f08b` and on both sides of the taffy 0.13 move, record 0/40, and a
fresh runner on `main` at `c3b57758a69` measured 0/40 again on 2026-08-21.
Renders of 059 and 060 showed the two tables with identical geometry but the
`position: absolute; top: 0` overlay laid out in normal flow below its
sibling: the K5 integration merge (`27c2c87828f`, 2026-08-15) had made every
block ancestor of a table fall back to Taffy, which was told that absolute
boxes are `position: relative`. Repairing that fallback restored exactly
10/40 (`testing/genet/wpt-ledger/2026-08-21_anonymous_table_remeasure/`,
`post-fix-1-taffy-absolute`). The remaining 30 split by render: 059, 060,
063, and 064 differ only at anti-aliasing level (`maxδ 64`, no red); 093
through 098 fail because the *reference's* HTML `<col style="background">`
does not paint while the test's CSS `table-column` backgrounds do; the other
20 show anonymous first-row cells stacked in column 0 or duplicated rows,
which is the anonymous-table construction defect this paragraph originally
named. That residual is K4-model debt with no open gate, and is recorded in
the fullweb cutover register rather than here.

### PH2. Dimensions and alignment

Implement the HTML table mappings for:

- `table[width]` and `table[height]`;
- `col[width]` (the mapping table names `col` only, not `colgroup`);
- row-group and row `height`;
- cell `width` and `height`; and
- applicable table-part `align` values.

`table[align]` itself maps to `float` (left/right) or centering margins, not
to text alignment. Implement it as those declarations or defer it by name;
do not fold it into the text-align family.

Use HTML's non-negative-integer, dimension, and nonzero-dimension parsing
rules rather than CSS declaration parsing. Preserve percentage dimensions as
percentages until used-value resolution.

**Implemented declaration slices, 2026-08-11:** Livery now activates
`margin-inline-start` and `margin-inline-end` through a generated logical
group projection adapted from Stylo's axis/side mapping. `table[align=left]`
and `table[align=right]` emit `float`; `table[align=center]` emits both logical
auto margins at the presentational-hint origin. Authored physical margins
compete at their original cascade coordinates and logical CSSOM reads project
through the winning writing mode and direction.

The HTML adapter also maps `table[width]`, `table[height]`, `col[width]`,
row-group and row `height`, and cell `width`/`height` to typed `Size` values.
It preserves percentages and the exact per-element zero policy. `colgroup`
does not receive the `col` mapping. Table-part `align` values `center`,
`middle`, `left`, `right`, and `justify` map to `text-align`; `absmiddle` is
covered by the HTML static hint. `caption[align=bottom]` maps to
`caption-side: bottom`. Values are ASCII case-insensitive but are not
whitespace-trimmed.

Fixtures cover parsing edge cases, invalid diagnostics, authored precedence,
nested tables, mutation, percentage preservation, and the real table layout
route. No table-specific late width or height fallback existed to remove; the
remaining direct attribute-size fallback is replaced-element-only and stays
owned by PH4.

**Implemented used-value slice, 2026-08-12:** the adapter now records the
deepest applicable alignment owner for each descendant, suppresses an ancestor
when the element has its own applicable legacy alignment, and treats invalid
enumerated values as non-owners. `center`, `div[align]`, and the applicable
table-part values establish owners; `p[align]`, heading `align`, table
`absmiddle`, and the table/caption special mappings suppress ancestors without
inventing a descendant owner where HTML does not define one.

The metadata remains separate from computed CSS. Buckram's generic
`OverconstrainedInlineAlignment` policy adjusts only a definite-width block
whose two inline margins are non-auto and whose width equation leaves positive
space. It preserves line-left/line-right semantics across direction and writing
mode and leaves width-auto, auto-margin, and overflowing cases on the ordinary
CSS path. Static ownership, scripted mutation, pure used-value, and end-to-end
layout receipts are green; computed margins remain authored values.

**Removal receipt:** delete any table-specific late geometry fallback for a
mapping now represented in computed CSS.

### PH3. Table colors and borders

Cover the table families currently ignored by the lane: `bgcolor`, `border`,
`frame`, `rules`, and their affected table parts. Implement the exact HTML
rendering mappings and selector conditions; do not reduce `frame` or `rules`
to one generic border rectangle.

PH3 begins only after contextual-color C1 so a color hint produces the same
authoritative computed color type as a CSS declaration. K4g remains the owner
of collapsed-border conflict resolution. A hint supplies candidates; it does
not select winners.

**Receipt:** computed-style fixtures distinguish the hinted declaration from
the K4g border result, and focused table border/color WPT has an exact
before/after map.

**Implemented 2026-08-12:** the Genet adapter parses `bgcolor` on tables, row
groups, rows, and cells with a bounded harvest of Stylo's HTML legacy-color
algorithm. `table[bordercolor]` contributes four typed border-color
longhands. Failed colors remain diagnostics and do not become CSS parser
inputs.

`table[border]` now maps valid non-negative integer prefixes to all four
border widths and uses the specified 1px fallback after a parse error. The
nonzero-or-error condition independently controls the table's outset styles
and corresponding cells' 1px inset styles. Every `frame` state maps its exact
physical side pattern. Every recognized `rules` state collapses the table and
projects the specified cell, row, row-group, and column-group widths and
styles. `cols`, `rows`, and `groups` retain logical block/inline semantics
through Livery's generated border logical groups, including vertical writing
modes. Traversal stops at nested tables.

The attribute-sensitive black rule colors and inherited table-part colors are
UA-origin selectors in `CAMBIUM_UA_DEFAULTS`; geometry remains at the author
presentational-hint origin. Ordinary author CSS therefore overrides both at
the correct cascade coordinate. K4g still selects collapsed-border winners.
The end-to-end receipt first asserts the typed table and cell candidates, then
records one collapsed-metrics, assigned, and honored table with no skip.

Focused tests are green: 22 HTML-hint unit tests, logical-border projection,
the K4g receipt, all 112 `genet-livery` library tests, every integration and
example target, and the full `livery` suite. Mutation changes `bgcolor`,
`frame`, and `rules` in one batch without retaining the old side pattern.

The rebuilt release WPT runner reports:

| WPT | before | after |
|---|---:|---:|
| `table-bordercolor-001.html` | pass | pass |
| `table-border-1.html` | fail | pass |
| `table-border-2.html` | pass | pass |
| `table-border-presentational-hints-ascii-case-insensitive.html` | pass | pass |
| `table-attribute.html` | 0/58 | 41/58 |

The broad harness delta is cumulative with the rebuilt PH1/PH2 binary, so it
is not credited wholly to PH3. Its 17 remaining subtests name separate
boundaries: seven `background` URL hints, one computed `border-color`
shorthand serialization gap despite the longhands and rendering being
correct, percentage/used table sizing, default `th` alignment, and harness
DOM/script gaps. The four PH3-focused reftests are all green.

The joint `livery` plus `genet-livery` clippy command remains blocked by 146
pre-existing warnings in untouched Livery selector and color-space code.
`genet-livery` alone is clean under `-D warnings`.

### PH4. Replaced and embedded elements

Migrate the CSS-representable hint families for `img`, `input[type=image]`,
`embed`, `object`, `iframe`, `video`, and `canvas`, including applicable
`width`, `height`, aspect-ratio, `align`, `hspace`, `vspace`, `border`, and
`frameborder` mappings.

Intrinsic replaced sizing remains layout work. The attribute-derived CSS
dimensions do not. Once width/height and aspect-ratio hints reach computed
style, delete `apply_replaced_image_size`'s direct attribute override and
retain only intrinsic sizing against those computed inputs.

**Receipt:** computed CSS, layout geometry, intrinsic aspect ratio, attribute
mutation, and authored override all agree through one style path.

**Implemented 2026-08-14:** the HTML adapter now projects the complete PH4
declaration matrix at the author presentational-hint origin. Dimension
attributes reach `width` and `height` on `img`, `embed`, `iframe`, `object`,
`video`, and image buttons. The applicable image, video, image-button, and
canvas pairs also produce HTML's `auto <width> / <height>` aspect-ratio form.
Livery now preserves that computed form separately from a CSS ratio and uses
its preferred ratio only when the natural ratio is unavailable.

Legacy `align` projects floats or the exact vertical-alignment behavior,
including a separate internal token for HTML's center-on-the-parent-baseline
`middle` and `center` behavior. `hspace` and `vspace` project the four physical
margins. Positive image/object borders project four solid border sides.
`iframe[frameborder]` projects zero widths for zero or a signed-integer parse
error, while the iframe's ordinary 2px inset border remains a UA-origin
default. Authored CSS wins over every one of these declarations.

`apply_replaced_image_size` and its direct `img[width]`/`img[height]` attribute
reader are deleted. Image layout now sees attribute-derived dimensions only
through computed CSS. Decoded image dimensions remain the natural-size and
natural-ratio input; canvas bitmap dimensions remain intrinsic layout data.
The adapter does not expose a natural ratio to the backend when both CSS axes
are definite, because intrinsic ratio must not override two definite sizes.

Static computed-style fixtures cover every element and attribute family.
Retained mutation, authored precedence, decoded-natural-ratio, canvas
intrinsics, shaped inline image, layout, and neutral paint receipts are green.
The frameborder receipt also closed the computed-style surface needed to
serialize absolute zero border widths as `0px` and reconstruct the computed
`border-width`, `border-style`, and `border-color` four-side shorthands.
The full `genet-livery --all-targets` suite and isolated
`genet-livery --no-deps -D warnings` Clippy pass. The full `livery` test suite
also passes; its joint strict-Clippy boundary remains the 146 pre-existing
warnings named under PH3.

The rebuilt 1.97.1 release WPT runner reports:

| WPT | before | after |
|---|---:|---:|
| `embedded-and-images-presentational-hints-ascii-case-insensitive.html` | 0/3 | 3/3 |
| `iframe-frameborder.html` | 0/13 | 13/13 |
| `align.html` | 14/23 | 23/23 |
| `canvas-aspect-ratio.html` | not measured | 13/13 |
| `canvas-dimension-attributes.html` | not measured | pass |
| `img-dim.html` | pass | pass |
| `images/space.html` | fail | pass |
| `dimension-attributes.html` | harness error, 0/0 | harness error, 0/0 |

The broad dimension file still errors in the Boa DOM harness before it emits
a subtest. That result does not contradict the direct computed-style, layout,
and canvas intrinsic receipts above.

The selected-source dimension source for `img` is still unimplemented because
the adapter has no `picture`/`srcset` source-selection state. Actual embedded
content for `embed`, `iframe`, `object`, `video`, and image buttons is likewise
an element or browsing-context capability boundary: their computed hints are
present, but PH4 does not invent replaced-content implementations. The UA
`video { object-fit: contain }` default awaits an active `object-fit` property.
PH5 must record these separately from missing mappings.

### PH5. Census and closure

Build a checked-in census from the HTML rendering section, grouped by:

- implemented property and adapter;
- known Livery property gap;
- unsupported element or browsing-context capability; and
- deliberate UA-origin default rather than author hint.

Cover the remaining low-cost mappings only when their target property already
has a real consumer. Route forms, media, browsing-context, and quirks-only
behavior to their owning plans instead of adding dormant declarations.

Run absolute conformance and Stylo-differential ledgers separately. A family
that matches Stylo but remains wrong against HTML is not a closure receipt.

**Implemented 2026-08-14:** the
[HTML rendering census](2026-08-14_livery_html_rendering_census.md) now keeps
author presentational hints, UA defaults, used-value algorithms, and missing
property/selector/document/element capabilities separate. In particular, it
assigns background URL hints to the absent document/base-URL seam, body link
colors to absent link/visited selector state, `font[face]` to the one-family
grammar and consumer boundary, list hints to marker/counter work, and embedded
contexts to their element hosts. It does not add declarations that cannot yet
be consumed correctly.

The low-cost active-property harvest now includes body margin and color hints,
`pre[wrap]`, `br[clear]`, legacy font color and size, the complete `<hr>` hint
family, table-part vertical alignment, and cell `nowrap`. Livery's `font-size`
value now represents every CSS absolute-size keyword, resolves it through the
same computed metric path as authored CSS, and round-trips it through CSSOM.
The `<hr>` UA defaults are present at UA origin. The current HTML Standard's
`align=bottom` mapping is `vertical-align: bottom`; the older checked-in WPT
expects `baseline`, so that single differential remains deliberately red.

Direct fixtures cover HTML parsing, invalid-first-source behavior, authored
precedence, mutation removal and replacement, computed shorthand CSSOM, and
layout geometry through one retained style plane. No new late attribute reader
or Buckram-specific HTML branch was added.

The rebuilt 1.97.1 release runner reports absolute Livery results separately
from the Stylo differential:

| WPT | Livery before | Livery after | Stylo current |
|---|---:|---:|---:|
| `<hr>` folder, runnable files | 2/4 | 4/4 | 2/4 |
| `body_text_00ffff.xhtml` | fail | pass | fail |
| legacy `font[size]` | 21/28 | 28/28 | 21/28 |
| `table-attribute.html` | 41/58 | 42/58 | 0/58 |
| replaced `align.html` | 23/23 at the PH4 snapshot | 22/23 current-standard mapping | 3/23 |
| list hint ASCII casing | 0/2 | 0/2 assigned gap | 0/2 |
| `pixel-length-attributes.html` | harness error, 0/0 | harness error, 0/0 | harness error, 0/0 |

Three non-runnable files in the seven-file `<hr>` folder remain skipped. The
16 broad table failures are seven background URL cases, one Boa DOM/script subtest error,
percentage/used table sizing, cellpadding/table-column layout, and dynamic
`th` default alignment. The table increase is one subtest; it is not evidence
that K4 sizing is fixed.

The full `livery` suite and `genet-livery --all-targets` suite pass. Isolated
`genet-livery --no-deps -D warnings` Clippy, the release WPT build, explicit
edition-2024 rustfmt check, and `git diff --check` pass. A forced clean-target
joint Clippy run still stops on the same 146 pre-existing Livery selector and
color-space diagnostics; the shared-target cache alone had incorrectly made
that command appear clean.

PH5 is complete as an implementation and assignment gate. On 2026-08-14 the
plan was held open because the done condition also required the 40-file
anonymous-table family to pass for the attributed K4 reason; the rebuilt PH5
runner had remeasured it at 10 passed and 30 failed that day. The 2026-08-21
ruling removed that condition: the family's blocker was a K5 regression, not
a hint, and its residual is not PH work (see the PH1 correction).

## Verification ladder

Every behavior gate runs:

```powershell
cargo test -p livery --offline
cargo test -p genet-livery --all-targets --offline
cargo clippy -p livery -p genet-livery --no-deps --offline -- -D warnings
cargo build -p genet-wpt --release --all-features --offline
rustfmt --edition 2024 --check <touched Rust files>
git diff --check
```

Focused WPT maps are stored outside Git under a gate-specific
`testing/genet/wpt-ledger` directory. Each receipt reports absolute Livery
movement and Stylo differential movement separately.

## Stop rules

- Stop if an HTML attribute is read inside Buckram.
- Stop if a CSS-representable hint is applied after computed style.
- Stop if hints are modeled as ordinary unlayered author rules.
- Stop if `cellpadding` is inherited from the table instead of attributed to
  its corresponding cells.
- Stop if invalid HTML values are fed to the CSS parser and accepted under
  different grammar.
- Stop if presentational hints appear as authored CSSOM rules or inline style.
- Stop if PH3 duplicates K4g conflict resolution.
- Stop if a differential gain is called HTML conformance without the absolute
  receipt.

## Done condition

The plan closes when every HTML presentational hint applicable to Genet's
implemented static/fullweb elements is either represented at the correct
cascade origin or assigned to a named property, element, quirks, or
browsing-context gap, direct layout-side attribute overrides are deleted where
computed CSS now owns them, and mutation/CSSOM receipts prove one authority.
Met 2026-08-21. The table-anonymous family was a condition until that date;
it was removed by ruling because its blocker and its residual are both
outside this plan, and both are recorded where they are owned.
