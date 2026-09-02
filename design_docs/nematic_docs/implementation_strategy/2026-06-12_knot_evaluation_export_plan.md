# Knot Evaluation + Export Plan — live blocks in, any protocol out

> **Paths note, 2026-09-02.** Paths in this document are as of its last
> reconciliation (2026-07-27). `crates/meerkat/` was deleted 2026-07-18;
> `crates/script/lua` and `crates/probes/knot-lua` no longer exist (mere keeps
> `crates/script/rhai`); `src/fetch.rs` has moved. Read them as history.
**Date**: 2026-06-12
**Status**: reconciled and complete for the first production capability set
2026-07-27. K5, K1's pure resolve pass, K2's evaluation seam, the Knot
production effect bridge, Turnstone consent, network providers, sanitized HTML
fragments, and the sealed attributable resolve cache landed. Inker and Nematic
now live in Genet; Meerkat, the host named throughout the dated progress log,
was deleted 2026-07-18. The current ownership and completion sequence below
supersede the old Meerkat wiring assignments. The production Knot adapter now
supplies anonymous HTTP(S) plus read-only Gemini, Gopher, Finger, Spartan, Nex,
and Guppy fetches; Titan remains excluded because navigation is still a
zero-byte upload.
**What this is**: the effectful half the
[polyglot knot design](2026-05-08_polyglot_knot_design.md) deliberately left
host-side, plus the export dual it promised. Knots (CommonMark and djot
bodies alike) already render protocol-tagged fences inline through the real
engines (`expand_fenced_blocks`, shipped, recursive). This plan adds:
fences that **fetch** (transclusion), fences that **execute** (script
engines), **HTML clippings** rendered at two fidelity tiers, and exporters
so a knot can be served *as* gemtext (or a gophermap) from any server you
host. Engines stay pure throughout — evaluation is a host-driven pass over
descriptor blocks, in the same two-phase spirit as everything else.
**Trust rule (one sentence)**: your own SelfAsserted knots evaluate per
setting; anything received (a moot's flora, a peer's clip) renders inert
until explicit consent — fences degrade to visible source, never to silent
execution or silent fetches.
**Conflict posture**: nematic + inker + netfetcher (the smolweb clients) +
a dev bin — no genet-layout, no meerkat render/input/frame_ops, no pelt.
The consent *UI* and any genet-fragment rendering are named and gated.

---

## Reconciliation (2026-07-27)

This is no longer one monolithic Knot plan. It is a pure document engine with
an effectful Knot service and a product-host consent surface.

| Original slice | Current status | Current owner |
| --- | --- | --- |
| K5 exporters | Complete: gemtext already existed; gophermap and text exporters landed | Genet `components/inker/src/document/render/export.rs` |
| K1 transclusion | Complete: pure pass plus production rooted-file, anonymous HTTP(S), and read-only smolweb providers; Knot policy, Graphshell intent, and Turnstone Ask/Auto receipt complete | Pure pass in Genet Inker. Knot owns document trust and invokes an injected fetch capability. |
| K2 evaluation | `BlockEvaluator`, registry, policy, Rhai backend, bounded Piccolo proof, nested render, production registration, and consent complete | Seam in Genet Inker; evaluators remain capability providers. Knot chooses and invokes them under document policy. |
| K4 sanitized HTML clips | Complete: optional html5ever-backed reader fragment engine plus Knot transclusion routing and hostile-fragment receipt | Genet Nematic/inker owns the pure parse, sanitize, and lower lane |
| K3 cache and consent | Complete for fetched results; evaluation remains opt-in and no shipped evaluator declares cacheability | Knot owns sealed derived caches and policy. Graphshell/Turnstone presents declared intents, age, and user consent. |

### Boundary ruling

- **Genet stays pure.** Inker and Nematic parse, lower, export, resolve with
  caller closures, and evaluate through a trait. They do not acquire network,
  vault, journal, or product-UI dependencies.
- **Knot owns effects over documents.** It holds the source, trust state,
  encryption profile, grants, and derived cache. Fetch and evaluator
  capabilities are injected into the endpoint; their authority is checked
  there. This is why document sync, transclusion policy, and evaluation policy
  belong in Knot even though transport and script engines are supplied
  elsewhere.
- **The product host owns consent presentation only.** Graphshell/Turnstone may
  render Run, Resolve, Refresh, and Save affordances and send declared intents.
  It does not receive a vault key, silently fetch, or execute a document block.
- **Recorded fact and derived state stay distinct.** Included content and
  evaluation output are recomputable projections by default. A cache is sealed,
  attributable derived state, not a silent edit to the source journal.

The live `KnotEndpoint` now advertises effects only for writable documents with
injected capabilities. It validates the target, grant, observed scene revision,
document base token, consent mode, trust posture, and allowlists before
invocation. The
Knot authoring consumer plan (`mere/design_docs/mere_docs/implementation_strategy/2026-07-27_knot_authoring_consumer_plan.md`)
holds the retained consumer and mutation receipts.

### Completion sequence

1. **E1, Knot effect service. Complete.** Explicit
   Resolve and Run intents, injected fetch/evaluator registries, document trust
   and grant checks, scheme/language allowlists, recursion and operation
   limits, and user-configured `auto` / `ask` / `never` modes are live. The
   production adapter routes rooted files, anonymous HTTP(S), and read-only
   Gemini/Gopher/Finger/Spartan/Nex/Guppy through the same injected fetch seam.
   HTTP effects cannot borrow the browser cookie jar; bodies use the endpoint's
   source-byte cap. Gemini pins are in-memory for the endpoint lifetime, so
   durable pin storage remains a separate trust-store improvement. Titan is
   rejected because its nominal fetch is an upload. Received Commons documents
   require explicit confirmation even under Auto.
2. **E2, product consent. Complete.** Graphshell advertises strict versioned
   actions. Turnstone presents only advertised Resolve/Run controls, sends the
   user's confirmation, queues Auto on open, and renders the result as derived
   text tied to the current base token. A sealed endpoint may restore an
   attributable resolve result; it never becomes authored source. Stale
   invocations refuse before fetch or evaluation.
3. **E3, sanitized HTML. Complete.** Nematic's optional, default-enabled
   `html-fragment` feature parses through the existing html5ever-backed static
   DOM and lowers only passive reader structure. Scripts, event handlers,
   iframes, forms, style authority, and active URL schemes are removed by
   tests. Knot routes fetched HTML/XHTML through this engine before splicing
   derived blocks; an endpoint receipt proves hostile content stays out while
   headings and safe links survive. The semantic sibling remains the export
   path.
4. **E4, sealed derived cache. Complete.** Successful resolve results from
   sealed vault sources persist inside Knot's encryption profile with source
   URLs, policy fingerprint, fetcher version and relevant configuration,
   fetched-at time, source revision, and source base token. Personal results
   use Personae sealed-record storage.
   Commons results are additionally wrapped by the current group-data epoch.
   Lock, revocation, source or policy change, provider change, and Commons epoch
   rotation make restoration a cache miss; deleting a projected document
   collects its cache record. Directory results remain memory-only. Evaluation
   remains uncached because no shipped evaluator yet declares an explicit
   cacheability contract. A refresh re-runs from authored source; complete
   fetch failure leaves a still-valid cached document visible and reports the
   failed refresh.
5. **E5, end-to-end receipts. Complete for this capability set.** Own-document Ask and Auto,
   source immutability, derived revision refresh, operation-budget exhaustion,
   and stale-consent rejection are proved; the real Turnstone process receipt
   crosses both actions. A second real-process fixture starts from a
   differently signed Commons operation under the group-data encryption
   profile: Auto is rejected, explicit Run succeeds, authored source stays
   unchanged, and plaintext is absent from the fixture store. The E4 receipt
   proves encrypted persistence, reopen restoration, revocation and policy
   invalidation, and Commons current-epoch invalidation. Graphshell 1.3 carries
   fetched-time attribution and Turnstone renders its real age. Sanitized HTML
   is proved through Knot's endpoint lane. Faithful selected-range clip capture
   remains a separate Genet producer seam; Knot already accepts explicit
   selector provenance.

JavaScript backends, public serving, canvas outputs, and additional block kinds
remain separate consumer-pulled work. They are not closure conditions for this
plan.

---

## Execution order

**Historical order: K5 → K1 → K2 → K4 → K3.** K5 (exporters) was pure
functions with zero new dependencies. E1 through E5 above are the completed
production sequence; the original K sections remain the design and progress
record.

## K5 — protocol exporters (`to_gemtext` and friends)

The dual of fence expansion, extended beyond `to_markdown()`/`to_knot()`
(both shipped, tested):

- `EngineDocument::to_gemtext()` — downgrade rules documented beside the
  code: headings → `#`/`##`/`###`; paragraph inline links → `=>` link
  lines after the paragraph; images → `=>` lines with alt text; nested
  lists flatten with indent markers in the text; tables → preformatted;
  quotes → `>`. **Protocol fences pass through verbatim where the target
  format matches** — a gemtext fence was already gemtext, zero loss.
- `to_gophermap(ctx)` — gophermaps need server context (host, port,
  selector base), so the signature carries a small context struct.
- `to_text()` — trivial flattening, completes the set.
- Fences with no faithful mapping (e.g. an `html` fence) export their
  semantic sibling blocks (see K4) or a marked omission — loss is visible,
  never silent.
- A `knot-render` dev example bin (the rehearsal-bin pattern):
  `knot-render <file.knot> --as gemtext|gophermap|markdown|text`. Serving
  is any existing server (agate etc.); a Mere-native gemini server is
  explicitly out of scope.

**Done when**: a djot knot with prose, links, a list, and a gemtext fence
exports to spec-valid gemtext with the fence byte-identical; the round-trip
property holds where lossless (fence → export → re-render ≈ direct render);
exporters are pure and dependency-free.

## K1 — transclusion fences (fetch + render inline)

- **Fence form**: info string `include <url>` (plain word, one verb; the
  fence body is optional **fallback content**, rendered when unresolved —
  offline, denied, or pre-consent — so degradation is authored, not
  invented). Works identically in CommonMark and djot knot bodies.
- **Parse (nematic, pure)**: both knot engines emit a new
  `Block::Transclusion { url, fallback }` descriptor.
  `to_knot`/`to_markdown` round-trip it; exporters render the fallback.
- **Resolve pass (inker, host-driven)**: an async
  `resolve_transclusions(doc, fetcher, registry, policy)` walk: policy
  check → fetch → route the response bytes through the engine registry by
  content type (plus sniff) → splice the produced blocks in place →
  record each spliced block's origin in `BlockProvenanceMap` (built for
  this) → optional source-marker badge (a setting). Recursion capped
  (transcluded knots may transclude; default depth 2, configurable) with a
  URL cycle guard.
- **Fetch lanes**: http(s) rides netfetcher; smolweb rides **`errand`**
  (the standalone lib that already speaks gemini / gopher / finger /
  spartan / nex / guppy / misfin / titan and is already wired into
  meerkat's fetch actor). *No new client crate is needed* — the
  2026-06-12 correction below records that the smolweb clients already
  existed; the only gap was real cert pinning, harvested into errand's
  `TofuStore` (gemini now pins per host with a pluggable store —
  in-memory now, durable/eidetic-backed later).
- **Policy is declarative data** (the untrusted-policy rule): a
  `TransclusionPolicy` struct — scheme allowlist × the containing
  document's trust state × mode (`auto` / `ask` / `never`), all settings.
  Default: own SelfAsserted knots auto-resolve; everything else inert.
- Hygiene rider (same files): resolve the `DjotKnotEngine` inconsistency —
  two doc comments say it is *not* in `engines()` (shared `text/x-knot`
  content type), but it is registered; pick one story and make docs and
  code agree.

**Done when**: a knot with `include gemini://…` renders the remote page's
blocks inline with per-block provenance; offline renders the authored
fallback; a received (non-SelfAsserted) knot stays inert; recursion and
cycles capped; policy branches covered by tests with a stub fetcher; the
`knot-render` bin grows `--resolve`.

## K2 — script fences (execute + render inline)

- **Fence form**: `<lang> eval` (e.g. ` ```lua eval `) — a plain ` ```lua `
  fence stays what it is today, a code sample. Evaluation is opt-in **per
  fence** on top of the per-document trust gate.
- **Parse (pure)** → `Block::Evaluation { language, source }`.
- **Evaluate pass (host)**: an inker-level `BlockEvaluator` seam the host
  implements; first backend is **piccolo Lua** through the DOM-neutral
  ScriptEngine seam (pure Rust, no JIT constraint, the fork is in-tree,
  budgets + microtask pumping built in), run constellation-style (panic
  isolation, instruction/time budget, no ambient I/O — piccolo's sandbox
  default). JS (Nova native / Boa wasm) follows through the same seam once
  the Lua lap proves the contract.
- **Output contract (v1)**: the script returns `(format, text)` — `plain`
  becomes a paragraph/preformatted block; `gemtext` / `markdown` / `djot`
  nested-render through the registry (the org-babel move). Canvas-swatch
  outputs (drawing scripts → platen) are named for later, not built.
- **Trust**: same gate as K1, stricter default — own notes `ask` (a
  setting can relax to `auto`), received notes never auto-run.

**Done when**: a `lua eval` fence in an own note renders its output inline,
including a gemtext-returning script nested-rendering; budget exhaustion
yields a visible error block, not a hang; a panicking script isolates; a
received knot's script fences render as inert source.

**Progress — 2026-06-12.** K2a landed and K2b resolved by reuse (the same
"check before you build" lesson as the errand correction):

- **K2a — the inker evaluate pass landed** (`inker::document::evaluate`, 7
  tests). `evaluate_blocks(doc, evaluate, render, policy)` mirrors the
  transclude pass exactly: closure-driven (decoupled from any script engine
  *and* the routing layer), top-level `<lang> eval` fences only, plain
  output → `Paragraph`/`Preformatted`, `gemtext`/`markdown`/`djot` output →
  nested-render via the registry closure, generated-block provenance
  (`evaluated:<lang>`), and the full gate: `EvaluationPolicy`
  (`deny_all` floor, `for_own_notes`), a plain ` ```lua ` sample left
  untouched, denied/failed reported, source fence kept on refusal or error.
  Same no-new-`Block`-variant decision as K1a (the descriptor is a
  `CodeBlock` with `language = "lua eval"`), so inert rendering is free
  everywhere.
- **K2b — the Lua engine already exists; do NOT build a new crate.** Survey
  (applying the errand lesson) found genet's **`script-engine-api`** (the
  DOM-neutral ScriptEngine seam this plan already named) and its
  **`script-engine-piccolo`** backend (the gc-arena DOM plan's G4): `new()`
  / `eval(source)` / `value_to_string(value)` / `Budget` / `pump`, on the
  vendored piccolo fork. Confirmed building + green here (8 tests). The
  abandoned first instinct — a new `crates/script/lua` — would have been a
  second redundant build; reverted before writing it.
- **K2c — the host bridge (deferred, scoped).** Joining the two halves is a
  ~15-line adapter: `|lang, source| { engine.eval(source).and_then(|v|
  engine.value_to_string(&v)).map(EvalOutput::plain) }` (plus a
  `return format, text` convention later). But it pulls genet + piccolo +
  gc-arena, which must not couple to pure nematic — so it lives where
  genet is already linked (meerkat), deferred with the rest of the shell
  wiring, or in a `crates/probes/` spike if a standalone demo is wanted
  first. **One real gap found**: `PiccoloEngine::eval` runs `finish()`
  (unbounded), so it hangs on `while true do end`; the seam's `Budget` is
  on `pump` (microtasks), not the main eval. So "budget, not a hang" needs
  either (a) a bounded-eval method added to the seam (the harvest pattern —
  a defaulted `eval_bounded(source, Budget)` on `ScriptEngine`, piccolo
  overriding with a fuel loop; non-breaking) or (b) the host running eval
  in a wall-clock-bounded worker. Decision deferred to Mark with K2c.

- **K2c + the bounded-eval gap — both landed (Mark's call: harvest + probe).**
  **Harvest** (genet, the errand pattern): added `eval_bounded(source,
  Budget)` to `script-engine-api::ScriptEngine` with a default that runs the
  existing unbounded `eval` (non-breaking — Nova/Boa unchanged), and an
  override in `script-engine-piccolo` that steps the executor with metered
  `Fuel`, the bounded mirror of `Lua::finish`; a `Budget::Steps(n)` cap
  returns "budget exhausted" instead of looping forever. script-engine-api +
  piccolo green (10 tests; the runaway `while true do end` proven caught,
  the engine still usable after). **Probe** (`crates/probes/knot-lua/`, a
  standalone excluded workspace so piccolo + gc-arena never touch the mere
  graph — and sidestepping the sibling `markdown-v0` probe's stale paths):
  renders a knot, runs each `lua eval` fence through a fresh bounded
  `PiccoloEngine`, nested-renders the output. Ran end to end, all four
  done-conditions met with real Lua: a `for`-loop fence rendered
  "the sum of 1..10 is 55"; a gemtext-returning fence nested-rendered as
  live gemtext (heading + `=>` line); the `while true do end` fence was
  **caught** ("budget exhausted after 100000 steps") and the render exited
  cleanly without hanging; a plain ` ```lua ` sample stayed untouched.
  Two notes recorded: `PiccoloEngine::new()` uses `Lua::full()` (io
  included — fine for the trusted own-note demo; sandbox-hardening, e.g.
  `Lua::core()` for untrusted knots, is a follow-up), and the production
  bridge home is meerkat (the probe is the spike). **K2 is functionally
  complete** end to end; the remaining work is the meerkat host wiring +
  consent UI (K3, gated) and the JS backend through the same seam (the Lua
  lap having proven the contract). Next plan slice: K4 (HTML clip tiers) or
  K3 (caching + consent), per priorities.

- **2026-06-13** — **The eval lane got a real polyglot menu: the thin
  `BlockEvaluator` trait + a Rhai backend (8 tests).** Question that drove
  it (Mark): given knot eval needs only a slice of the JS-shaped
  `ScriptEngine` seam, what about Rhai/Rune as pluggable backends? Decision:
  a **thin `BlockEvaluator`** (in inker: `eval_block(source, max_ops) ->
  EvalOutput`, plus a `BlockEvaluators` registry keyed by language tag) is
  the knot-eval contract, deliberately distinct from genet's full DOM
  seam (reflectors, promises) which mod/DOM scripting keeps. `script-rhai`
  (`crates/script/rhai`, pure Rust, no genet) implements it: Rhai is
  sandboxed by default (no file/network) and has a **native operation
  budget** (`set_max_operations`), so the runaway cap is first-class (a
  `loop {}` is caught, not hung) rather than the fuel loop piccolo needed.
  Output convention: a string (format-detected) or an explicit
  `#{format, text}` map. The full path is proven by an integration test (a
  `rhai eval` fence routed through the registry + the inker pass renders
  "sum is 55"). On the scripting-map question: this is the **option-module**
  tier (like piccolo Lua), not a first-party substrate — the Rust+JS
  shipping decision stands, and Rhai-for-*policy* stays superseded by
  declarative data. Rune remains gated on its own trigger (1.0 + sandbox
  warranty). The broader "how polyglot" question spun out a dedicated plan:
  [polyglot block resolver](2026-06-13_polyglot_block_resolver_plan.md)
  (one registry; query / diagram / wasm block kinds beyond more languages).

## K4 — HTML clippings, two fidelity tiers

Djot forbids raw HTML *in prose*; it does not forbid explicit fenced
blocks — clippings stay format-clean.

- **Semantic tier (exists)**: `build_clip_knot` already serializes
  selected blocks from a genet-rendered tile with provenance (per-block
  overrides included). This is the default clip and the export-friendly
  representation.
- **Faithful tier (new)**: clip-time *additionally* captures the source
  fragment in an `html` fence. Rendering it inline needs the
  **reader-mode HTML fragment lane** nematic's own status already names as
  its one pending lane: an `HtmlFragmentEngine` rendering a sanitized
  subset — text, headings, lists, tables, images; scripts, event
  handlers, iframes, and styles stripped (sanitization proven by test,
  not by intention). Parser choice: **html5ever** (spec-grade tokenizer,
  the standards-correct pick and the lineage genet already trusts) over
  lighter non-spec parsers; the heavier dep is confined to nematic's
  optional feature.
- Full-fidelity genet-fragment rendering inside a knot stays a named
  registry slot for post-reshape — the fence and the routing seam are the
  same either way, so upgrading fidelity later touches no format.
- Export: `html` fences export via their semantic sibling (the tiers
  travel together in the knot), never raw HTML into gemtext.
- Distinct from (and feeding) the *browsing* reader-mode lane: rendering
  whole `text/html` pages through nematic is a separate later slice; the
  fragment engine is its seed.

**Done when**: a clipped fragment renders inline matching its source's
semantics (fixture pairs against the genet-rendered original); a
script/onclick-bearing fragment provably renders with them stripped; clip
round-trip keeps both tiers intact; exporting a knot with an `html` fence
produces the semantic downgrade.

## K3 — caching + consent surfaces (gated last)

- Resolved transclusions from sealed sources persist as Knot-owned sealed
  records with source URL, fetched-at, source revision, provider version, and
  policy attribution. They do not become authored engrams or enter a
  Graphshell cache. Directory sources remain memory-only because they have no
  endpoint sealing profile.
- Script results may opt into the same cache (`eval` fences declaring
  deterministic intent), default off. No shipped evaluator currently opts in.
- The consent surfaces — the "resolve" / "run" affordance on inert fences
  in received knots — are product-host UX over declared Knot endpoint intents.
  E1/E2 now provide that path for writable own documents and require explicit
  confirmation for received Commons documents. The `knot-render` bin remains
  an engine rehearsal; the Knot endpoint is the authority-bearing product
  path.

**Done when**: a reopened sealed endpoint serves the cached transclusion with
its real age available to the product host; invalid source, policy, provider,
grant, or current Commons epoch cannot restore it; and the product path can
resolve with consent end to end.

## Out of scope (named)

A Mere-native gemini *server* (exporters produce files; serve with
anything); full readability/article-extraction browsing mode (separate
lane seeded by K4's fragment engine); JS-engine wiring beyond the seam
contract (follows the Lua lap); canvas-swatch script outputs (platen,
later); genet full-fidelity fragments (registry slot, post-reshape);
any change to `mooting`/flora formats (a shared knot is just an engram —
K-lanes read its trust state, nothing more).

## Open questions

- Fence verb spelling: `include` chosen for plainness — `transclude` is
  the precise word but jargon; revisit if `include` collides with a
  future preprocessor sense.
- Whether the source-marker badge on spliced blocks defaults on (visible
  provenance) or off (clean reading) — a setting either way; default
  leans visible-until-trusted.
- TOFU cert-pin store location (file beside the profile vs eidetic
  engrams) — start file-backed, migrate when persona/keys land fully.
- **Resolved 2026-07-27:** evaluator implementations stay with their engine
  providers. Knot consumes the thin `BlockEvaluator` capability and owns
  registration, trust policy, and invocation for a document. The product host
  owns neither the evaluator nor the document authority.

## Progress

- **2026-06-12** — Plan written. Grounding: `expand_fenced_blocks` +
  inline extensions shipped and wired in **both** knot engines;
  `to_markdown`/`to_knot` shipped with tests; `BlockProvenanceMap` +
  `DocumentTrustState` exist (the policy hooks); the protocol registry
  routes smolweb *schemes* but no smolweb *clients* exist yet (netfetcher
  is http(s)-shaped) — K1's one real dependency; djot raw-block semantics
  confirm fenced HTML is format-clean.
- **2026-06-12** — **K5 landed (inker 76 tests, nematic 157 + the bin's 3,
  all green).** Survey first: `to_gemini()` already existed with the
  planned downgrade rules — K5 narrowed to the rest. Added
  `to_gophermap(ctx)` (RFC 1436: prose as `i` info lines, gopher links
  decomposed into native menu entries, other schemes as `URL:` items on
  the serving host, `.` terminator) and `to_text()` in
  `document/render/export.rs`; `GophermapContext` re-exported at inker's
  top level. The `knot-render` bin (nematic example, embedded tests):
  `<file.knot> --as gemtext|gophermap|markdown|text|knot [--djot]`. The
  **verbatim-fence property holds by architecture**: `expand_fenced_blocks`
  turns a gemtext fence into real blocks at parse time, so export restores
  them as live gemtext — proven by the bin's round-trip test (heading +
  link line byte-faithful, no fence wrapper). The live rehearsal caught
  two output bugs unit tests missed: link-only paragraphs double-rendered
  (text line + `=>` line) in both the gemtext and gophermap writers —
  fixed with a shared `is_link_only` rule; and the new exporters printed
  the frontmatter title where the established ones never do — dropped.
  The "serve knots as gemtext" pipeline works end to end: djot knot →
  `knot-render --as gemtext` → `.gmi`. Next slice: K1.
- **2026-06-12** — **K1a landed: the transclusion resolve pass (inker 81
  tests; nematic 157 + 3 green; rehearsed through the bin).** One design
  deviation from the plan, recorded: **no new `Block` variant.**
  The blast-radius survey found exhaustive `Block` matches across
  document-canvas, uxtree, gloss, and platen (meerkat's have catchalls) —
  so the descriptor *is* the existing `CodeBlock` with
  `language = "include <url>"` (the full fence info string already
  survives parsing). Payoff: unresolved fences render as visible source +
  authored fallback **everywhere, automatically** — the trust rule's inert
  rendering with zero cross-crate churn. The dedicated variant waits for
  the consent-affordance slice (K3), where renderers genuinely need it.
  `inker::document::transclude`: `TransclusionPolicy` (enabled / scheme
  allowlist / max depth; `deny_all` floor + `for_own_notes`),
  `resolve_transclusions(doc, fetch, render, policy)` with fetch/render
  as caller closures (decoupled from netfetcher and the routing layer;
  sync in this cut, async adapts at the closure boundary), cycle guard,
  per-pass depth, per-spliced-block `BlockProvenanceMap` records, and a
  faithful `TranscludeOutcome` (resolved / denied-with-reason /
  failed-with-error). v1 limit stated in-code: top-level fences only.
  The bin grew `--resolve` with a `file://` fetcher (paths relative to
  the knot; content type by extension): unresolved output shows the
  fallback fence, resolved output splices the included gemtext in place.
  **Remaining for K1**: K1b — the real network lanes (http(s) via
  netfetcher; the `smol` clients: gemini TLS+TOFU, gopher, finger,
  spartan, guppy, nex) and wiring them as the host's fetch closure.
- **2026-06-12** — **K1b correction: smolweb already had a home — `errand`
  — and the plan was wrong to put it in netfetcher.** Mark caught it: was
  meerkat not already fetching `gemini://`/`gopher://`? It was. Survey
  confirmed [`errand`](https://github.com/sgtmark/errand) (Mark's
  standalone smolweb-transport lib, pulled as a git dep) already speaks
  gemini / gopher / finger / spartan / nex / **guppy / misfin / titan**
  (more than the five I built), exposes `errand::fetch(url) -> Response`,
  and is **already wired** into meerkat's fetch actor (`crates/meerkat/
  src/fetch.rs`: http(s)→netfetcher, smolweb→errand). So the netfetcher
  `smol` module I wrote was fully redundant. **Reverted it entirely**
  (netfetcher back to its baseline, 65 tests, clean tree). The one real
  bit of value in what I wrote was *true* cert pinning — errand's TLS was
  TOFU-**permissive** (accept-any; its `tls.rs` even noted "a later
  revision will pin certificates per host … behind a pluggable trust
  policy"). **Harvested that into errand** as the intended upgrade: a
  `tofu` module (`TofuStore` trait + `InMemoryTofu` + `PermissiveTofu`
  default + process `set_trust_store`), a `PinningVerifier` that checks
  the leaf SHA-256 against the host's pin *inside* the handshake (so the
  request is never sent on a mismatch), `Error::CertificateChanged`, and
  the gemini wiring (lookup → pin-on-first-contact → reject-on-change).
  Default behaviour is unchanged (no store installed = permissive), so
  existing errand users see nothing new; a host opts into pinning with one
  `set_trust_store` call. errand: 38 unit tests + the full TOFU loop
  proven against the **live** `geminiprotocol.net` capsule (pin, match,
  then `CertificateChanged` on a corrupted pin). Titan/misfin (write
  companions) stay permissive — a noted follow-up. **Lesson recorded**:
  check the workspace's existing crates before planning a "new module"
  for a capability — the plan even said "netfetcher is http(s)-shaped,"
  which was true *because* smolweb already lived in errand. **Remaining
  for K1 fully**: the host bridge — meerkat already has errand wired for
  *page* fetches; the transclusion path reuses it (an `errand::Response`→
  `Fetched` map in the resolve closure), meerkat-deferred like all shell
  wiring. Next plan slice: K2 (`lua eval` script fences).

## Alignment — smolweb fidelity plan (2026-07-01)

Two touchpoints with the [smolweb fidelity plan](2026-07-01_smolweb_fidelity_plan.md).

- **Round-trip fidelity.** The `to_gemtext` / gophermap / `to_knot` exporters are only
  as faithful as the parse ASTs feeding them. That plan's Workstream 1 enriches those
  ASTs (feed content-vs-summary and published-vs-updated, enclosure, guid; gopher
  `raw_type`), so the exporters gain fields they must preserve on the round-trip rather
  than silently drop.
- **Trust.** K1 here already built the errand-transport TOFU half (the `set_trust_store`
  / pin-match loop proven against the live capsule). The fidelity plan's Workstream 2
  carries that transport trust up into the native view's tile chrome, so the card lane
  and the focused-tile lane surface one posture.
