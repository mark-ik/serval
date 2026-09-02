# Polyglot Knot Design — Protocol-Faithful Clip Composition

> **Paths note, 2026-09-02.** `crates/middlenet-core/` is the pre-rename home
> of what is now nematic; the path is kept as written because this document is
> retained as rationale and format spec, not as a map of the tree.
**Date**: 2026-05-08
**Status**: Implemented 2026-05-08 / 2026-05-09. Doc retained as the design rationale + format spec; consult [`../../../components/nematic/src/knot/expand.rs`](../../../components/nematic/src/knot/expand.rs) and [`../../../components/inker/src/document/render.rs`](../../../components/inker/src/document/render.rs) for the live behavior, `../../mere_docs/implementation_strategy/2026-05-09_post_engine_layer_priorities.md` (`mere/design_docs/mere_docs/implementation_strategy/2026-05-09_post_engine_layer_priorities.md`) §2.4 for follow-ups.
**Scope**: Extend the [`nematic.knot`](https://crates.io/crates/nematic) note format from "frontmatter + markdown body" to a polyglot composition where every other `nematic.*` protocol's blocks can embed inside a knot, fenced-code-block-style, and round-trip back to the source protocol's native syntax.

**Related**:

- `../../mere_docs/implementation_strategy/2026-05-05_protocol_architecture_plan.md` (`mere/design_docs/mere_docs/implementation_strategy/2026-05-05_protocol_architecture_plan.md`) — protocol architecture
- `../../mere_docs/implementation_strategy/2026-05-09_post_engine_layer_priorities.md` (`mere/design_docs/mere_docs/implementation_strategy/2026-05-09_post_engine_layer_priorities.md`) — current forward-looking plan
- Archived: `../../archive_docs/2026-05-09_engine_layer_complete/2026-05-06_graphshell_migration_plan.md` (`mere/design_docs/archive_docs/2026-05-09_engine_layer_complete/2026-05-06_graphshell_migration_plan.md`) — engine layer + smolweb stub history
- Inherited: `SemanticDocument` (archived graphshell repo, `crates/middlenet-core/src/document.rs`) — donor's universal document model that this plan generalises

---

## 1. Why

The 2026-05-08 design conversation surfaced a richer use of knots than the v1 frontmatter-plus-markdown shape:

> "each protocol forms the grammar of a superset language, presented either as the context or in the broader context of a set of links related to each other (the graph, the idea of being able to open every link in a given page element or entire tile of a node, or being able to extract a clip of an element from the tile, which is then in .knot format; the block extracted can be represented as a gopher or gemtext or other nematic protocol block which can coexist among other protocol blocks in a knot. so in your notes, your knots, you get the richest view that contains the faithful protocol but isn't limited to it, indeed the whole knot can be really filled with anything in a beautiful way and saved for sharing too."

Two things follow from that:

1. A clip taken from a protocol-rendered tile must preserve that protocol's representation in the knot, not flatten it to plain markdown. Gopher menu items stay gopher items. Gemtext links stay gemtext links. Feed entries stay feed entries.
2. A knot is an *aggregate* — it can mix markdown prose authored by the user with protocol blocks clipped from any number of sources. The knot is the only Mere-defined format; the protocol blocks inside it stay spec-faithful per the protocol-faithfulness rule established during the engine-layer slice (now captured in `../../mere_docs/implementation_strategy/2026-05-09_post_engine_layer_priorities.md` (`mere/design_docs/mere_docs/implementation_strategy/2026-05-09_post_engine_layer_priorities.md`) §3).

The existing v1 knot engine already uses [`Block`](../../../components/inker/src/document.rs)'s semantic variants (`FeedEntry`, `MetadataRow`, etc.) for the *output* of parsing. What's missing is a way for those variants — or raw gemtext / gopher / nex content — to *appear in the knot's body* and round-trip cleanly.

---

## 2. Format Spec

### 2.1 Frontmatter (unchanged)

YAML subset between `---` markers at byte 0. Carries `title`, `source`, `captured`, `source_label`, `trust`, `note_kind`, `tags`. Already implemented; no changes.

### 2.2 Body — CommonMark plus protocol-fenced blocks

The body is CommonMark. Fenced code blocks (` ``` `) whose language tag matches a known protocol are **expanded** by the knot engine after the markdown parse: the code block is replaced with the blocks the named protocol's engine would produce from that content.

Recognised fence languages:

| Tag | Expands to | Producer |
| --- | --- | --- |
| `gemtext` | Heading / Paragraph / Link / Quote / List / CodeBlock blocks | `nematic::gemtext::GemtextEngine` |
| `gopher` | Paragraph (info merge) + Link blocks | `nematic::gopher::GopherEngine` |
| `nex` | List of Link blocks (directory) or Paragraphs (content) | `nematic::nex::NexEngine` |
| `feed-entry` | One `Block::FeedEntry` block | knot-local key:value parser |
| `feed-header` | One `Block::FeedHeader` block | knot-local key:value parser |
| `metadata-row` | One `Block::MetadataRow` per non-empty line (`label: value`) | knot-local line parser |
| `badge` | One `Block::Badge` per non-empty line | knot-local line parser |
| anything else | Unchanged — stays a `CodeBlock` with the original language hint | n/a |

The unknown-tag fall-through is important: a knot embedding `python` or `rust` code blocks must keep them as code blocks, not error.

Example knot body:

```text
# My research
A regular markdown paragraph with [a link](https://example.com).

` ` `gemtext
=> gemini://capsule.test/ a capsule
* a bullet
` ` `

` ` `feed-entry
title: Article I clipped
date: 2026-05-08
url: https://blog.test/post
source: https://blog.test/
summary: A summary stripped of HTML.
` ` `

` ` `metadata-row
Login: alice
Captured: 2026-05-08T14:23:00Z
Trust: tofu
` ` `

` ` `python
def hello(): print("stays as a code block")
` ` `

More markdown.
```

### 2.3 Inline parsers for semantic-only fences

Three of the fence languages (`feed-entry`, `feed-header`, `metadata-row`, `badge`) don't have a dedicated nematic engine — they describe single semantic blocks that exist only in the document model. The knot module owns small line-parsers for them:

- **`feed-entry` / `feed-header`**: lines of `key: value` form. Recognised keys for `feed-entry`: `title`, `date`, `summary`, `url` (→ `article_url`), `source` (→ `source_url`). For `feed-header`: `title`, `subtitle`, `summary`, `source`.
- **`metadata-row`**: each non-empty line is split on the first `: ` into `label` / `value`.
- **`badge`**: each non-empty line is one badge's text.

Unrecognised keys in `feed-entry` / `feed-header` are preserved on a sibling `MetadataRow` block emitted alongside the entry, so authors can add custom fields without losing them.

---

## 3. Engine Flow

```text
EngineInput { body: knot text }
       │
       ▼
split_frontmatter → (frontmatter, body)
       │
       ▼
MarkdownEngine.render(body) → EngineDocument with raw blocks
       │  (fenced protocol blocks appear as `CodeBlock { language, text }`)
       ▼
expand_fenced_blocks(&mut blocks)
       │  walk blocks; for each CodeBlock with a known language tag,
       │  call the matching parser, replace with the resulting blocks
       ▼
apply_frontmatter_overrides
       │  title, provenance, trust state, content_type, tags / kind rows
       ▼
EngineDocument
```

Recursive expansion: `expand_fenced_blocks` walks into `Quote.blocks` and `List.items` so a fence inside a quote or list expands too.

---

## 4. Round-Trip — `EngineDocument::to_knot()`

The dual of expansion. New method on `EngineDocument` that mirrors `to_markdown()` but emits fenced code blocks for protocol-specific variants instead of flattening them.

| Block | `to_knot()` output |
| --- | --- |
| Heading / Paragraph / List / Quote / CodeBlock / Image / Preformatted / Rule | Same as `to_markdown()` |
| `FeedHeader { … }` | ` ``` `feed-header\n…key:value lines…\n` ``` ` |
| `FeedEntry { … }` | ` ``` `feed-entry\n…key:value lines…\n` ``` ` |
| `MetadataRow { label, value }` | ` ``` `metadata-row\n{label}: {value}\n` ``` ` (consecutive rows may be merged into one fence) |
| `Badge { text }` | ` ``` `badge\n{text}\n` ``` ` |

**Gemtext / gopher / nex preservation**: blocks that were *originally* expanded from a `gemtext` / `gopher` / `nex` fence don't currently carry "I came from this fence" provenance, so a naive `to_knot()` would lose the original syntax and re-render via individual block paths. Two options:

- **(a) Round-trip-via-EngineDocument**: when serialising, the renderer walks blocks and emits each in its block-level form. Loses original gemtext/gopher syntax but preserves all semantic content.
- **(b) Source-segment preservation**: knot-engine post-process attaches the original raw fence text to a sidecar field on the resulting blocks (e.g. an opaque `provenance.source_segment` per block). `to_knot()` re-emits the original fence verbatim if present, falls back to (a) otherwise.

Option (b) is the right shape for clip workflows — a clipped gopher menu should round-trip byte-for-byte. Option (a) is simpler. **v1 lands (a); v2 adds (b) once the clip gesture exists.**

---

## 5. Provenance for Clipped Blocks

A knot's frontmatter carries whole-document provenance: where the clip came from, when it was captured, what the trust state was. v1 keeps provenance at the document level only — individual blocks do not carry per-block source attribution.

Future: per-block source URLs (which gopher selector did *this* item come from? which paragraph in the original capsule was this quote?) become useful when a knot aggregates from many sources. The slot exists in the document model already (`Block::FeedEntry.source_url`); a more general `block.provenance` field can be added when intelligence layers need it.

---

## 6. Graph-Side Gestures (Future, Host-Owned)

Two user gestures that consume this format. Both live in the future host crate (gpui / iced / etc.), not in nematic itself, but the semantics matter for what nematic must guarantee.

- **Open-all-links**: the user selects an element (a paragraph, a list, a section) on a tile. The host calls `EngineDocument::outgoing_links()` over the selected blocks (or a future `selection.outgoing_links()` once selections are first-class), then spawns one new graph node per URL. Already wired today — needs only the selection gesture and a "spawn from URL" action.
- **Clip-to-knot**: the user selects an element, takes the corresponding `Vec<Block>`, and calls a host-level `clip_to_knot(blocks, source_provenance) -> String` builder that:
  1. Serialises each block via `to_knot()` rules (FeedEntry → `feed-entry` fence, gemtext-derived blocks → `gemtext` fence with original raw, etc.).
  2. Wraps the result with frontmatter populated from the source tile's `EngineDocument.provenance`: `source = canonical_uri`, `captured = now()`, `trust = source.trust`, `source_label = source.source_kind`.
  3. Saves the knot file to Eidetic / a chosen workspace path.

The clip-to-knot helper itself can live in `nematic` as a free function (`nematic::knot::build_clip_knot`) since it's protocol-agnostic; the gesture wiring (select / capture / save UI) lives in the host.

---

## 7. Implementation Slice (Next)

Order:

1. **Inline parsers** for `feed-entry`, `feed-header`, `metadata-row`, `badge` in `nematic::knot` (~120 LOC). Tests cover each.
2. **`expand_fenced_blocks` post-process pass** that walks blocks (recursing into Quote / List), dispatches by language tag, splices results in place. (~80 LOC, plus tests.)
3. **Wire-up** in `KnotEngine::render`: call `expand_fenced_blocks` after the markdown parse, before frontmatter overrides.
4. **`EngineDocument::to_knot()`** method in `inker::document::render` (or a sibling `document/knot_render.rs` if it pushes the file over the ceiling). Mirrors `to_markdown()` but routes semantic variants to their fence forms. (~200 LOC.)
5. **`nematic::knot::build_clip_knot(blocks, provenance) -> String`** — assembles a knot file from raw blocks plus provenance, ready for save. (~50 LOC.)
6. Round-trip tests: every protocol's representative output, fed back through the knot engine, produces an equivalent `EngineDocument`.

Testing strategy: parameterised round-trip — for each protocol, pick a sample, render it via its engine, embed the source text inside a `gemtext` / `gopher` / `nex` fence in a knot, re-render via knot, assert the resulting blocks match the direct render.

File-size budget: knot.rs is at 429 LOC today; +200 LOC pushes near the 600 ceiling. If it crosses, split inline parsers into `knot/parsers.rs` ahead of time.

---

## 8. Open Questions

- **Wikilinks and hashtags inside knot bodies**: punted from v1. A natural follow-up is recognising `[[name]]` as `InlineSpan::Link { url: "graphshell://node/<name>" }` and `#tag` as a sibling Badge or per-paragraph annotation. Out of scope for this slice.
- **Should clip-to-knot batch consecutive markdown blocks into a single knot, or one knot per selected element?** Probably one knot per selection, with multi-block selections producing a single multi-block knot. UI gesture decides.
- **Cross-knot links**: if knot A links to knot B (saved at a known path), how is the link expressed? Likely `knot://workspace/path/to/b.knot` or similar. Defer until knot persistence has a concrete location strategy via Eidetic.
- **Per-block provenance** (§5 future). Slot exists; activate when an intelligence layer needs it.

---

## 9. Summary

Polyglot knot bodies make the format the **richest reasonable surface for graph-native notes** without breaking the protocol-faithfulness rule the protocol engines hold. Markdown remains the connective prose; fenced protocol blocks preserve clipped content in its original spec-shaped form; semantic-only blocks (feed entries, metadata rows, badges) get a small declarative fence syntax. Round-trip is symmetric: `to_knot()` is the dual of fence expansion, so a clip + save + re-open cycle reproduces the source's intent and (in v2) its bytes.

---

## 10. Djot substrate direction (added 2026-05-31)

A later design pass (prompted by "what about djot?") concludes that **djot is the
stronger long-term substrate** for the knot body, and that several of §2's custom
fence hacks become native djot constructs. CommonMark stays available as an
import/compat mode; djot becomes the authoring grammar.

### 10.1 Why djot

Three of knot's hardest requirements are djot's native strengths:

1. **Round-trip fidelity.** §4's `to_knot()` and the v2 byte-faithful goal are
   exactly what djot ([djot.net](https://djot.net), John MacFarlane) was designed
   for; CommonMark is notoriously hard to round-trip. The grammar is regular and
   unambiguous, with a clean event model.
2. **Typed blocks without a hack.** §2.2's semantic fences (`feed-entry`,
   `feed-header`, `metadata-row`, `badge`) abuse *code* blocks as typed data.
   Djot's generic **divs** (`:::class`) and **attributes** (`{#id .class key="v"}`)
   are the right primitive for "a block that means something."
3. **Attributes as statements.** Djot inline/block attributes are a human-writable
   syntax for the open-predicate `(subject, predicate, object)` model of the
   statements-over-schema stance (`mere/design_docs/mere_docs/technical_architecture/2026-05-22_statements_over_schema_stance.md`).
   `[[Topic]]{rel=schema:cites}` is a `Semantic` edge with a predicate;
   `# H {typeof=schema:Article}` is a node classification. CommonMark cannot say
   this without another fence.

The cost is the ergonomic knot leans on: "it is just markdown, paste anything."
Djot is markdown-*like*, not CommonMark-compatible. Mitigation: keep a CommonMark
import/compat mode (the existing engine) for pasted markdown; author new knots in
djot.

### 10.2 Construct mapping

| §2 today (CommonMark) | Djot |
| --- | --- |
| `feed-entry` / `feed-header` (key:value fence) | `:::feed-entry {title=… url=… date=…}` div; content = summary |
| `metadata-row` (key:value fence) | native **definition list** (`: Term` then an indented definition) |
| `badge` (line fence) | `[text]{.tag}` spans, or a `:::badge` div |
| `gemtext` / `gopher` / `nex` (protocol source) | unchanged — a code fence is the honest primitive for verbatim foreign source and round-trips byte-faithfully |
| `[[wikilink]]` → node | `[[wikilink]]{rel=…}` — wikilink stays a knot extension; the optional `rel` carries the predicate |
| `#hashtag` → Badge | `[tag]{.tag}` span, or keep `#tag` extraction as a convenience |

The migration *shrinks* knot's custom surface: `metadata-row` disappears into
definition lists, the semantic fences become attribute-carrying divs (now
schema-able), and the only irreducible custom pieces are the protocol code-fences
(honest), the `[[ ]]` wikilink, and YAML frontmatter.

### 10.3 Sketch

```text
# Proposed (djot + jotdown)
# My research {typeof=schema:Note}
A note about [[Topic A]]{rel=schema:about}.

` ` `gemtext
=> gemini://capsule.test/ a capsule
` ` `

{title="Article I clipped" url="https://blog.test/post" date="2026-05-08"}
::: feed-entry
A summary, as the div's content.
:::

: Trust

  tofu

[research]{.tag} [clip]{.tag}
```

### 10.4 What the field tells us (Markdoc / MyST / Obsidian / Logseq)

- **MyST** directives (` ```{note} ` with `:key: val` options) are structurally
  what §2's fences already are — knot is MyST-shaped without naming it. Djot
  divs + attributes express the same idea more cleanly.
- **Markdoc** (Stripe) contributes the missing piece: a declarative **schema** for
  the recognized blocks (node/tag/attribute types + validation). Knot's fence
  vocabulary is hardcoded; a small schema makes it data (recognized-core-plus-open-
  tail, per the stance).
- **Obsidian** validates the model the two-natured kernel brief (`mere/design_docs/mere_docs/research/2026-05-30_two_natured_kernel_brief.md`)
  draws: the file is truth, the graph is derived. A knot is the authoritative file;
  kernel nodes/edges are the projection.
- **Logseq** shows the backend: notes over a **datalog statement store**, with
  `key:: value` block properties that *are* statements. That is the kernel's
  content-core direction; knot is the authoring surface, the kernel is the store.
  Djot `{key=val}` and Logseq `key:: value` are the same move.

Through-line: **knot is the human authoring front-end for the kernel's
content-truth, and djot is the only substrate here where the authoring syntax can
express the same statements the kernel stores.**

### 10.5 Phased migration (additive; the shipped CommonMark knot stays working)

1. **PoC (additive):** add `jotdown` (the Rust djot pull-parser); a new
   `knot/djot.rs` parses a djot body into `Block`s for the key constructs
   (div-with-class → semantic block; definition list → `MetadataRow`; protocol
   code-fence → existing expansion; inline `rel`/`typeof` attribute → a predicate
   carried on the block/link). Behind a flag / experimental engine id; existing
   knot untouched. Tests. **Landed 2026-05-31** (`knot/djot.rs`; `jotdown =
   "0.10"`): definition list → `MetadataRow`, `::: feed-entry` / `::: badge` divs
   → semantic blocks (typed attributes read via `Attributes::get_value`), with
   headings / paragraphs / code mapped structurally; then wired as a reachable `DjotKnotEngine` (engine id
   `nematic.knot-djot`, re-exported; shares `apply_frontmatter` with `KnotEngine`;
   kept out of the default `engines()` registry since it shares the `text/x-knot`
   content-type). 44/44 knot + djot tests green. Deferred at PoC (since landed):
   inline `rel` predicates (Phase 4) and reusing the existing protocol-fence
   expansion (Phase 5); `typeof` → classification stays future. Note: jotdown emits a standalone
   `{…}` attribute line as `Event::Attributes` applying to the next block, so the
   verified div form is an attribute line *preceding* `::: feed-entry`.
2. **Round-trip:** `to_djot_knot()` as the dual; byte-faithful source-segment
   preservation for protocol fences (§4 option (b)). **Dual landed 2026-05-31** as
   `blocks_to_djot` (the inverse of `parse_djot_knot_body`): semantic blocks emit
   as native djot (`MetadataRow` → definition list; `FeedEntry` / `FeedHeader` /
   `Badge` → an attributed div); a parse → emit → parse round-trip test is green on
   the recognized subset. Byte-faithful protocol-fence preservation stays future.
3. **Schema:** a small declarative registry for recognized div-classes + attribute
   types (the Markdoc lesson); validate, store unknown divs/attributes generically
   (the open tail). **Landed 2026-05-31** as `KNOT_DIV_SCHEMA` + `validate_div`:
   the recognized classes (`feed-entry` / `feed-header` / `badge`) and their
   required attributes are declared data; `parse_djot_knot_body_validated` returns
   blocks + diagnostics (unknown class → `UnsupportedConstruct` + a generic
   `Quote`; recognized div missing a required attribute → `ParseWarning`),
   surfaced in the engine's `doc.diagnostics`; 3 tests. Unknown-*attribute*
   preservation (the rest of the open tail) needs jotdown attribute iteration and
   stays future.
4. **Statements wiring:** map `rel` to kernel `Semantic` edges with predicate
   IRIs — depends on the
   linked-data plan (`mere/design_docs/archive_docs/2026-06-09_completed_plans/2026-05-22_linked_data_ingest_export_plan.md`)
   Phase 0 (open the Semantic predicate). **Landed 2026-06-01** (Phase 0 itself
   landed 2026-05-31). Three pieces:
   - **`InlineSpan::Link.predicate`** (`#[serde(default)] Option<String>`): the
     open `rel` slot on a link. Added by a compiler-guided sweep — 15
     construction / match sites across 7 crates (every engine plus the
     document-canvas / uxtree / platen / render consumers); read-only consumers
     already destructured with `..`, so they were untouched.
   - **djot `rel` capture:** the djot parser now builds real `InlineSpan::Link`s
     (a small inline-span accumulator replacing the flat-text fold) and reads the
     link's `rel` into `predicate`. jotdown folds an inline `{rel=…}` into the
     link's `Start` attrs (verified empirically); a standalone-`Attributes`
     fallback covers the block path. `[Topic](mere://node/topic){rel="schema:cites"}`
     → `Link { url, predicate: Some("schema:cites"), … }`. (djot.rs passed the
     600-LOC ceiling, so its tests moved to `knot/djot/tests.rs`.)
   - **knot→graph ingest** (`inker::statements`): `link_statements` purely walks
     a document's blocks for predicate-bearing links; `resolve_rel` recognizes a
     `rel` (a bare slug like `cites` *or* a full Mere IRI) against the vocabulary;
     `apply_link_statements` asserts `EdgeAssertion::Semantic { sub_kind, … }` and
     stamps the canonical predicate IRI via `set_semantic_predicate`. Scope, matching
     the plan's deferrals: edges only to *existing* targets — node creation is the
     host's job and `add_node` is wasm-gated — so an absent target is reported as
     `pending_targets`; only Mere-vocabulary relations are edged, with a raw / CURIE
     `rel` (e.g. `schema:citation`) returned as `unrecognized` (the raw-predicate
     `Semantic` edge pairs with linked-data Phase 2), never dropped.
   nematic **157** + inker **72** green (5 statement tests + 3 djot link tests).
   Deferred: `typeof` → node classification (needs a doc-level type source);
   raw-predicate-only `Semantic` edges (linked-data Phase 2).
5. **Default flip:** once parity + ergonomics hold, djot becomes the knot grammar;
   CommonMark stays as an import/compat mode rather than the author grammar.
   **Landed 2026-05-31.** `DjotKnotEngine` gained CommonMark parity by reusing the
   engine-agnostic `expand_fenced_blocks` + `rewrite_inline_extensions` passes
   (protocol fences + wikilinks/hashtags — so CommonMark-style fenced knots still
   work under djot); registered in `engines()`; the `text/x-knot` content-type
   default flipped to `nematic.knot-djot` (`inker/routing.rs`), with `nematic.knot`
   available by pin for compat. nematic **154** + inker **67** green.

Open question: keep a permanent CommonMark compat-parse mode for pasted markdown,
or convert-on-paste markdown → djot? Likely the former (cheap; preserves "paste
anything" for import while djot is the native author format).

## HTML projection (added 2026-08-13)

`EngineDocument::to_html` landed in inker (now in genet:
`components/inker/src/document/render/html.rs`) as the universal-browser
projection beside `to_markdown` / `to_gemini` / `to_knot` / `to_gophermap` /
`to_text`: a body-fragment emitter, fully escaped, semantic variants keeping
their intent as class names, link predicates carried as `data-predicate`.
Motivation: knot as the container that lets a stock phone browser read
embedded gemtext/gopher/feed blocks idiomatically (the field-beacon concept
in retinue's `2026-08-13_iot_device_concepts.md`). The knot stays the
authored truth; HTML is a projection, not a round-trip format.

## Alignment — smolweb fidelity plan (2026-07-01)

The semantic variants this design puts in knot bodies (`FeedEntry`, `MetadataRow`,
`Badge`, gemtext/gopher fences) are populated from the same errand parse ASTs the
[smolweb fidelity plan](2026-07-01_smolweb_fidelity_plan.md)'s Workstream 1 enriches.
As feed gains content-vs-summary and enclosure, and gopher keeps its raw item-type,
a clip that lowers a capsule into a knot carries more of the source's meaning, not
less. The protocol-faithfulness rule this design states is what that workstream
operationalizes at the parse layer, so the two stay in agreement.
