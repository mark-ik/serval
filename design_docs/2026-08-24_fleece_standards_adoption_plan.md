# Plan: fleece standards-shaped extraction

**Date:** 2026-08-24  
**Status:** planned 2026-08-24; parallel execution map added 2026-08-24. Phase
A is the next executable slice. The completed 0.1 reader/extraction work remains recorded in the
[original scope and receipt](../docs/2026-08-22_fleece_reader_extraction_scope.md).

## Purpose

Fleece 0.1 turns a live `LayoutDom` into a flat page extract and a proprietary
reader `Article`. This plan adds interoperable anchors and then hardens the
other standards-shaped data already harvested by Fleece. It implements the
Genet findings from
`mere/design_docs/2026-08-24_standards_survey_brief.md`, with one correction:
`TextPositionSelector` and `TextQuoteSelector` are parallel descriptions of the
same segment. They are not a `refinedBy` chain. In the W3C model, `refinedBy`
applies a narrower selector to the result of a broader selector.

The first release target is deliberately narrow: Fleece 0.2 adds stable text
coordinates, quote and position selectors, and an optional Text Fragment
projection. Microdata, metadata/link, and table work are later independent
phases and do not hold that release hostage.

## Standards rulings

| Standard | Ruling for Fleece |
|---|---|
| [Web Annotation Data Model](https://www.w3.org/TR/annotation-model/) | Adopt `TextQuoteSelector` and `TextPositionSelector` as source-relative selector values. Fleece does not mint a complete Annotation. |
| [URL Fragment Text Directives](https://wicg.github.io/scroll-to-text-fragment/) | Adopt as a derived, pasteable projection of a quote selector. Pin the draft revision used by the implementation. It is not durable authority. |
| [JSON-LD 1.1](https://www.w3.org/TR/json-ld11/) and [schema.org](https://schema.org/) | Preserve JSON-LD syntax, contexts, identifiers, and complete type IRIs. Do not claim JSON-LD processing or schema interpretation inside Fleece. |
| [HTML Microdata](https://html.spec.whatwg.org/multipage/microdata.html) | Implement the item/property traversal accurately, while retaining Fleece's explicit raw-URL boundary. |
| [Open Graph protocol](https://ogp.me/) | Keep ordered raw properties and add a grouped projection so structured properties remain attached to the preceding root property. |
| [HTML tables](https://html.spec.whatwg.org/multipage/tables.html) | Preserve table structure and accessibility relationships rather than reducing semantics to `th` versus `td`. |
| [Web Linking](https://www.rfc-editor.org/rfc/rfc8288) and the [IANA relation registry](https://www.iana.org/assignments/link-relations/link-relations.xhtml) | Normalize relation tokens from DOM `<link>` elements. HTTP `Link` headers remain the fetcher's responsibility. |

Explicit skips remain the Web Annotation Protocol, the separate Selectors and
States Working Group Note, RDFa, CSVW, and any claim that `Article` itself is a
standard shape. Microformats2 belongs to Gazette. Robots exclusion belongs to
the crawling host. ARIA remains a watched future extraction substrate, not a
dependency of this plan.

## Authority and data boundaries

- Fleece owns one documented textual representation of the supplied DOM and
  source-relative selectors into that representation.
- The caller owns the source URL, content identity, revision, retrieval state,
  Annotation body and motivation, and persistence.
- Quote and position selectors are sibling alternatives over one source
  segment. Fleece must not expose a misleading `refined_by` relationship.
- A Text Fragment is generated from the same quote evidence. Navigation,
  activation policy, URL composition, and script-visible fragment stripping
  remain browser-host responsibilities.
- JSON-LD is harvested as page-carried syntax. Expansion, context loading,
  RDF dataset construction, vocabulary reasoning, and node minting belong to
  semantic consumers.
- URL-valued attributes remain raw. Resolution against the document base stays
  with the caller that possesses the source address.

## Target public shape

The exact Rust spelling is settled during Phase A, but the public concepts are:

- `TextNormalization`, initially a versioned `FleeceDomTextV1` profile;
- `TextPositionSelector { start, end }`, using non-negative Unicode code-point
  positions and a half-open range;
- `TextQuoteSelector { exact, prefix, suffix }`;
- `TextAnchor { position, quote }`;
- an anchored block wrapper so every recursive, text-bearing article block can
  carry `Option<TextAnchor>` without stuffing selector fields into every block
  variant; and
- extraction options with caller-configurable quote-context length. The
  existing convenience functions retain a documented default.

Changing `Article.blocks` to an anchored recursive shape is a public API break
and therefore belongs in Fleece 0.2. `Rule` and a `Figure` without literal
caption text may remain unanchored. Synthetic reader text must never be
presented as a quote from the source document.

Fleece remains serialization-neutral and keeps its render-free dependency
cone. A downstream Annotation serializer maps `position` and `quote` to two
values of the W3C `selector` property. Fleece itself does not need JSON-LD or
Serde merely to name these concepts.

## Parallel execution contract

### The serialized seam

Fleece is currently one implementation file: public types, extraction walks,
the recursive reader model, structured data, metadata, table handling, and the
20-page tests all live in `components/fleece/src/lib.rs`. The 0.2 block wrapper
and coordinate stream cross most of that file. One integration owner must
therefore own the following paths for the whole 0.2 wave:

- `components/fleece/src/lib.rs`;
- `components/fleece/Cargo.toml` and `components/fleece/README.md`;
- workspace manifests and `Cargo.lock`, if a reviewed dependency decision
  requires them; and
- this plan and `design_docs/DOC_README.md`.

That owner should be a Terra agent or the coordinating agent. Other agents do
not make opportunistic edits to those paths. The owner freezes the public
names, normalization rules, anchored-block shape, options/defaults, synthetic
text policy, and grapheme decision before dependent packets begin.

The serialized seam is an honest constraint, not a reason to serialize the
whole release. Fixtures, the pure Text Fragment encoder, reader lowering, and
consumer serialization have separate files and can proceed concurrently once
the contract is frozen.

### Agent profiles

- **Terra packets** own architectural seams, recursive data-shape migrations,
  DOM traversal, cross-crate lowering, and standards algorithms with subtle
  authority boundaries.
- **Luna packets** own bounded pure transformations, fixture sets, reference
  assertions, encoding vectors, and compile/test sweeps with an already-frozen
  interface.
- The labels select a useful working profile. They do not weaken file fences or
  validation requirements.

Use at most three active worker packets alongside the coordinator. A packet is
parallel only when its write fence is disjoint; read access can cross the whole
workspace.

### Fleece 0.2 packets

| Packet | Agent | Write fence | Depends on | Deliverable and local gate |
|---|---|---|---|---|
| **T0: anchor core and integration ownership** | Terra | Serialized seam above | pinned base | Implement Phases A and B, keep the existing corpus owned, and publish the exact public contract to the other packets. Gate: all Fleece unit tests and the unchanged corpus receipt. |
| **L0: coordinate conformance** | Luna | new `components/fleece/tests/anchor_conformance.rs`; new `components/fleece/tests/fixtures/anchors/**` | T0 contract freeze | Independent static-DOM fixtures for entities, whitespace, adjacent nodes, astral and combining text, bidirectional order, repeated quotes, and nested ranges. Gate after T0 is applied: the dedicated integration test. Static/scripted equivalence remains an integration-owner gate in `genet-scripted`, avoiding a circular dev-dependency. |
| **L1: Text Fragment projection** | Luna | new `components/fleece/src/text_fragment.rs`; new `components/fleece/tests/text_fragment.rs` | selector names frozen by T0 | Pure quote-to-directive encoding with the pinned WICG revision and edge-case vectors. It returns a fragment component, never a full URL, and makes no host-activation claim. Gate: dedicated unit and integration tests. |
| **T1: reader-model migration** | Terra | `components/genet-documents/src/reader.rs` only | anchored block shape frozen by T0 | Adapt the only direct in-repo exhaustive `fleece::Block` lowering while preserving rendered `EngineDocument` content. Gate: `genet-documents` reader tests. |
| **T2a: consumer serializer preparation** | Terra | one preselected Knot, Gazette, or Alembic package plus one new fixture; no Genet paths | selector names frozen by T0 | Build the `SpecificResource` serializer with consumer-owned source identity and sibling quote/position selectors. It may use the pinned source revision during development. Gate: consumer-local serialization and independent-resolution tests. |
| **T2b: packaged-source proof** | integration owner | dependency declaration/lockfile for the chosen consumer only | Fleece 0.2 published | Replace the development source with the crates.io package, rerun T2a without a local override, and record the receipt. This packet is necessarily serialized after publication. |

`genet-scripted` is an observer for this change because it returns `Article`
without exhaustively lowering its blocks. `genet-extract` is a wildcard
re-export. Both are compile/test gates, not independent implementation packets.
Pelt and Turnstone likewise remain verification consumers unless compilation
finds an actual API use that needs a narrowly fenced repair.

### Dependency graph and wave gates

```text
pin one origin/main commit
          |
          v
T0 contract freeze and core implementation
          |
          +----------+-----------+-------------+
          |          |           |             |
          v          v           v             v
     L0 anchors  L1 fragment  T1 lowering  T2a serializer
          |          |           |             |
          +----------+-----------+-------------+
                             |
                             v
                 I0 Genet integration gates
                             |
                             v
                    publish Fleece 0.2
                             |
                             v
                    T2b packaged proof
```

The contract freeze can be communicated before T0 finishes, so L0, L1, T1,
and T2a can develop in parallel. Their commits integrate only after T0. The
merge unit is the complete wave: an individual packet may require the T0 commit
to compile, but the assembled wave must be green before it is recorded as
progress.

The integration owner alone adds module declarations or public re-exports to
`lib.rs`. If a packet discovers that its contract is wrong, it reports the
change instead of crossing its fence. T0 either revises the contract once for
all packets or rejects the deviation.

### Later hardening packets

Phases D, E, and F are logically separate but are not file-independent today.
Do not overlap them with the 0.2 wave or pretend their current `lib.rs` regions
are safe concurrent ownership. Before parallel hardening, run one serialized
**H0 module split** that moves existing code without semantic change into:

- `components/fleece/src/structured.rs`;
- `components/fleece/src/metadata.rs`; and
- `components/fleece/src/table.rs`.

Keep `fleece::Metadata`, `fleece::StructuredData`, and the other public paths
stable through re-exports, and require the complete pre-split test receipt to
remain unchanged. After H0, these packets may run together:

**H0 done when:** the module move has no intentional public or behavioral
change, the Fleece and reader gates match their pre-split receipts, and each
later packet's write fence is physically separate from the others.

| Packet | Agent | Write fence | Phase |
|---|---|---|---|
| **TD: structured-data fidelity** | Terra | `structured.rs` plus new `tests/structured_data.rs` and `tests/fixtures/structured/**` | D |
| **LE: Open Graph and document links** | Luna | `metadata.rs` plus new `tests/metadata_links.rs` and `tests/fixtures/metadata/**` | E |
| **TF: table semantics** | Terra | `table.rs` plus new `tests/table_semantics.rs` and `tests/fixtures/tables/**` | F |

The integration owner retains any residual public re-export, `Block::Table`,
reader-lowering, manifest, and lockfile edits. Each hardening packet hands back
a narrow module commit; the owner performs those shared seams after applying
the three disjoint commits.

### Worktree, commit, and handoff rules

The shared Genet checkout is not an agent work surface while it contains
unrelated work. Pin one current `origin/main` commit and give every packet a
uniquely named disposable detached worktree at that exact base. Detached
worktrees avoid permanent topic branches. An agent may commit in detached HEAD
and return the commit hash; the integration owner cherry-picks it into a clean
detached integration worktree.

Before every packet commit, the agent records `git status --short` and
`git diff --cached --name-only` and confirms that every staged path is inside
its fence. A file fence is not evidence of a clean commit boundary. The handoff
must contain:

1. base commit and produced commit hash;
2. exact paths changed;
3. commands run and pass/fail counts;
4. unresolved deviations from the frozen contract; and
5. confirmation that the packet worktree is clean after commit.

After cherry-pick and verification, remove that packet's worktree immediately.
After the assembled wave passes, refresh `origin/main`. If it moved, replay the
focused commits on the new remote base and rerun affected gates before pushing
`HEAD:main`. Then remove the integration worktree and confirm that no temporary
worktree or branch remains.

## Phase A: one canonical source-text coordinate space

Build an internal normalized-text index in one logical DOM-order traversal.
It records the normalized full-document text and the half-open code-point range
contributed by each text-bearing node. Article construction consumes those
ranges instead of independently reconstructing offsets after the fact.

`FleeceDomTextV1` must state its rules exactly: which non-content subtrees are
excluded, that markup is absent, that DOM-decoded characters are used, how
whitespace runs and block boundaries are represented, and that text remains in
logical rather than visual order. `PageExtract.text` is derived from the same
stream. Existing page text and the 20-page corpus are compatibility evidence,
not a second coordinate system.

Grapheme-cluster boundaries are a release gate. The implementation first tests
whether block-level selection can uphold them without another runtime
dependency. If it cannot, the plan must record an explicit dependency decision
rather than silently splitting clusters or growing a private Unicode table.

**Done when:**

- the existing 20-page corpus retains byte-identical `PageExtract.text`, main
  text, article blocks, and precision/recall figures;
- positions are counted by Unicode code point, never UTF-8 byte or UTF-16 code
  unit;
- fixtures cover tags, entities, collapsed whitespace, astral characters,
  combining sequences, bidirectional text, and adjacent text nodes; and
- static and scripted backends yield the same coordinate stream for the same
  post-parse DOM.

## Phase B: anchored article blocks

Wrap recursive article blocks with optional `TextAnchor` values. Mint the quote
and position selectors together while source ranges are still available.
`exact` is the selected substring from `FleeceDomTextV1`; `prefix` and `suffix`
are the immediately adjacent, caller-sized context windows. Context truncation
must not split a grapheme cluster.

Nested list items, quotes, code, table text, headings, paragraphs, and literal
figure captions receive anchors. Parent container anchors may cover their full
contiguous source range, while their children retain narrower anchors. If a
reader block combines discontinuous DOM ranges, it must remain unanchored or
carry several explicitly ordered anchors; it must not invent a continuous
range.

**Done when:**

- for every position selector, slicing the canonical stream by code points
  returns the quote selector's `exact` value;
- repeated quotations are disambiguated by prefix/suffix when sufficient
  context exists, while genuine remaining ambiguity is preserved rather than
  guessed away;
- nested-block fixtures prove parent and child ranges; and
- existing Fleece, `genet-documents`, scripted extraction, Pelt, Turnstone, and
  the `genet-extract` compatibility shim compile against the 0.2 shape.

## Phase C: Text Fragment projection

Add a pure projection from `TextQuoteSelector` to a text directive. Pin the
WICG source revision in the module documentation and conformance fixtures.
Encode UTF-8 and the directive delimiters exactly; keep prefix and suffix as
context terms. The API returns a directive or fragment component, not a full
URL, because Fleece does not know the source address or an existing element-id
fragment.

Genet host support is a separate gate from generation. A host that activates a
text directive must honor browser/user activation policy and must not expose
the `:~:` directive through `document.URL` or `Location`. Until that gate
lands, Genet may share generated directives but must not claim complete user
agent support for Text Fragments.

**Done when:**

- fixtures cover spaces, commas, ampersands, hyphens, percent signs, non-ASCII
  text, bidirectional text, and repeated matches;
- the directive resolves to the same source range as both selectors on static
  and scripted fixtures; and
- any activation implementation has a headed receipt for scrolling/indication
  plus a script-visible URL privacy test.

## Phase D: structured-data fidelity

Replace the lossy single `StructuredData.kind` view with complete declared type
identities and an optional item identifier. JSON-LD collection keeps all
`@type` values and full IRIs, retains `@context`, walks explicit `@graph`
members, and continues to preserve uninterpreted fields. Documentation calls
this JSON-LD syntax harvesting, not JSON-LD 1.1 expansion.

Bring Microdata traversal up to the HTML item model: all `itemtype` tokens,
`itemid`, tokenized `itemprop`, `itemref` roots, nested item values, the
element-specific value rules, document-order output, duplicate suppression,
and cycle protection. Raw URL attributes remain an explicit deviation from
the fully resolved browser algorithm.

**Done when:** fixtures cover multiple types, `itemid`, out-of-subtree
`itemref`, repeated properties, nested items, cycles, every supported
value-carrying element, and preservation of non-schema.org identifiers. The
public documentation makes the JSON-LD and raw-URL limits impossible to mistake
for full semantic processing.

## Phase E: document links and Open Graph grouping

Retain the ordered raw Open Graph pairs as evidence and add a grouped view in
which structured properties attach to the most recent root property. Preserve
unknown properties. Add a document-link shape containing tokenized relations,
raw `href`, media type, language, title, media query, and other attributes that
Fleece can observe. Registered relation tokens compare case-insensitively;
extension relation IRIs retain their identity. `Metadata.canonical` becomes a
convenience projection from these links.

**Done when:** Open Graph image/audio/video property groups round-trip in
document order; unknown properties survive; mixed-case registered relations,
multiple relation tokens, and extension relation IRIs have fixtures; and HTTP
header links remain absent from the Fleece contract.

## Phase F: HTML table semantics

Replace `Block::Table { rows }`'s shallow cell model with a table value that
can retain caption, row groups, a declared source `id`, `scope`, `headers`,
`rowspan`, `colspan`, and computed grid coordinates. Implement the relevant
HTML table-forming and header-association behavior over the extracted table.
Keep presentation and layout measurements out of Fleece.

**Done when:** hand-labelled fixtures cover row and column spans, all `scope`
values, explicit `headers` references, row groups, captions, irregular tables,
and associated-header results for each data cell. Reader lowering still renders
the same text while accessibility/semantic consumers receive the richer shape.

## Phase G: consumer proof and publication

The selector release needs one real downstream proof, not only Fleece unit
tests. Knot, Gazette, or Alembic serializes an anchored block as a W3C
`SpecificResource` with the source identity supplied by that consumer and the
quote/position values emitted as sibling selectors. The serialized fixture is
then resolved against the same document independently of the extraction walk.

Before publishing 0.2:

1. Rebase or reconstruct the focused change on current `origin/main`.
2. Run `cargo test -p fleece --offline`, the corpus receipt, scripted
   extraction tests, `genet-documents` reader tests, the Inker reader-route
   test, the shim build, and Pelt/Turnstone reader checks.
3. Record `cargo tree -p fleece --edges normal` so the render-free cone remains
   visible, including any explicit Unicode dependency decision from Phase A.
4. Build the downstream proof against the packaged crates.io source rather
   than a gitignored local path override.
5. Publish Fleece 0.2, update consumers deliberately, push the focused commit,
   and remove any temporary integration worktree or branch.

**Done when:** the package and consumer both pass from published-source
resolution, the remote commit is named in this plan, and no integration branch
or worktree remains.

## Findings

### 2026-08-24

- Fleece 0.1.0 has one normal dependency, `layout_dom_api`; structured JSON is
  parsed by a local value/parser pair to keep extraction render-free
  ([manifest](../components/fleece/Cargo.toml),
  [structured value](../components/fleece/src/lib.rs#L70)).
- `PageExtract.text` is the full Fleece-visible, whitespace-collapsed page
  string, while `Article.blocks` currently carries no source coordinates
  ([public extract](../components/fleece/src/lib.rs#L193)).
- `StructuredData.kind` retains one shortened type name. JSON-LD collection
  selects the first type and Microdata selects the first `itemtype`; `itemid`
  and `itemref` are absent
  ([structured harvest](../components/fleece/src/lib.rs#L1131)).
- `TableCell` records only a `header` boolean and inline runs; table extraction
  therefore does not yet carry HTML header association or spanning semantics
  ([table types](../components/fleece/src/lib.rs#L132),
  [table walk](../components/fleece/src/lib.rs#L1090)).
- Ordered Open Graph pairs are already a sound raw carrier because order is
  retained. Grouping can be added without discarding source evidence
  ([metadata](../components/fleece/src/lib.rs#L58)).
- W3C Web Annotation requires Unicode code-point positions in logical order and
  recommends avoiding grapheme-cluster splits. Its multiple-selector rule, not
  `refinedBy`, supplies the fast position path and robust quote alternative.
- Text Fragment Directives remain a draft and deliberately have browser privacy
  and activation behavior beyond string generation. That behavior cannot be
  smuggled into Fleece's render-free extraction contract.

## Progress

- **2026-08-24:** Plan created from the cross-repository standards survey,
  corrected the selector-composition interpretation, audited Fleece 0.1's
  public shapes, and separated the 0.2 anchor release from later standards
  hardening.
- **2026-08-24:** Added a Luna/Terra execution map. It serializes the monolithic
  Fleece API break under one owner, parallelizes conformance, Text Fragment,
  lowering, and consumer packets behind a frozen contract, and requires a
  semantics-preserving module split before later standards lanes run together.
