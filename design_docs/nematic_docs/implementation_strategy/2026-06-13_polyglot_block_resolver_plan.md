# Polyglot Block Resolver Plan — every knot block, one seam

**Date**: 2026-06-13
**Status**: Planned. Extends the
[knot evaluation + export plan](2026-06-12_knot_evaluation_export_plan.md)
(K1 transclude, K2 eval, K5 export, all landed) with the forward
architecture: a single registry that resolves *any* fenced block by its tag,
and the new resolver kinds that registry makes pluggable.
**Why now**: K1/K2 proved the parts but left three separate passes
(`expand_fenced_blocks`, `resolve_transclusions`, `evaluate_blocks`), each
with its own dispatch and its own trust handling. The eval lane just gained a
real menu (the `BlockEvaluator` registry, Rhai + Lua backends), which makes
the "blocks are arbitrarily polyglot" claim concrete and surfaces the
question Mark asked: how polyglot, and how do we keep it coherent.
**Conflict posture**: inker + nematic for the spine; the new kinds reach into
kernel (graph query), eidetic (search), platen (diagram swatches), and the
existing `register-mod-loader`/extism seam (wasm). Shell surfacing stays
gated; the probe pattern is the demo home until then.

---

## The frame: a block tag routes to a handler kind

A fenced knot block carries a tag (its info string). Today that tag means one
of three things depending on which pass sees it:

- **Render** (`expand` + K5): the body is content in a format an inker
  `Engine` renders (`gemtext`, `gopher`, `nex`, markdown, feeds).
- **Fetch** (K1 transclude): `include <url>` pulls a remote document and
  renders it (errand schemes + http(s)).
- **Eval** (K2): `<lang> eval` runs code and renders its output (JS, Lua,
  Rhai).

The ceiling is the union of those registries. Making it coherently polyglot
means **one resolver registry**: a tag maps to a `BlockHandler`, the trust
gate and provenance marking apply once, and new kinds register the same way
languages do today.

## P0 — the unified block-resolver registry (the spine)

Refactor the three passes into one `resolve_blocks(doc, registry, policy)`
walk. A `BlockHandler` answers "can I resolve this tag, and what does it
produce":

```rust
// Illustrative signature, not final.
enum BlockResolution {
    Blocks(Vec<Block>),   // splice these (rendered/fetched/evaluated)
    Leave,                        // not mine; pass through
    Denied(String),               // mine, but policy refused
    Failed(String),               // mine, but it errored (keep source + report)
}
trait BlockHandler {
    fn kind(&self) -> BlockKind;            // Render | Fetch | Eval | Query | Diagram | Wasm
    fn handles(&self, tag: &str) -> bool;   // e.g. "rhai eval", "include …", "graph", "dot"
    fn resolve(&mut self, tag: &str, body: &str, cx: &ResolveCx) -> BlockResolution;
}
```

- **Uniform trust + provenance**: one `BlockPolicy` (per-kind enable + the
  document's standing) gates every handler; every spliced block gets a
  `BlockProvenance` marking its origin (`fetched:<url>`, `evaluated:<lang>`,
  `queried:<source>`). The per-pass duplication of K1/K2 collapses here.
- **The existing passes become handlers**: `EvalHandler` wraps the
  `BlockEvaluators` registry; `FetchHandler` wraps the transclusion resolve;
  `RenderHandler` wraps the engine registry. No behavior lost, one spine
  gained. The standalone `resolve_transclusions` / `evaluate_blocks` stay as
  the handlers' innards (already tested).
- **Recursion + budget**: one place to cap depth and total work across kinds
  (a query that returns a knot that transcludes …).

**Done when**: the three existing passes run through one registry with one
policy, the K1/K2 tests pass unchanged behind the handlers, and a knot mixing
a transclude fence, an eval fence, and a render fence resolves in one walk
with correct per-block provenance.

## P1 — graph / eidetic query blocks (derive from your own stuff)

The highest-value new kind, and the closest fit with the recall work: a block
that *asks a question of your own data* and renders the answer inline.

- **Tags**: `graph` (a query over the kernel graph), `recall` (a query over
  eidetic's lexical + vector index), `trail` (a browsing-memory query).
- **Handler**: `QueryHandler` holds read-only handles to the kernel graph and
  the eidetic index (the host injects them; the handler never mutates).
  Output is a rendered result: a list of links (graph neighbors, search
  hits), a small table (counts, top domains), or a corridor.
- **Query surface, kept small and safe**: not a general query language at
  first. A handful of named, parameterized queries (`neighbors-of <node>`,
  `recall <text> [n]`, `co-occurring-with <url>`, `visits-in <window>`), so
  the surface is bounded and the determinism story is clean. A richer query
  DSL is a later refinement, and it stays *data-shaped* (the policy rule:
  declarative, re-runnable to a hash), never a Turing-complete script.
- **The payoff**: a knot that is a live dashboard over your trail. "What did
  I read about X" and "my top domains this month" become blocks, not a
  separate tool. This is the eidetic recall arc, surfaced where you write.

**Done when**: a `recall` block renders ranked hits from the eidetic index
inline; a `graph neighbors-of …` block renders the node's neighbors as
links; queries are read-only and bounded; a received knot's query blocks
stay inert until consent.

## P2 — diagram DSLs (describe it, see it)

A block whose body is a diagram description, rendered to a visual swatch.

- **Tags**: `dot` (graphviz), `mermaid`, `abc` (music notation, the Strophe
  tie), more as backends land.
- **Handler**: `DiagramHandler` routes a tag to a renderer that produces a
  scene (a platen canvas swatch) rather than text blocks. This is the first
  resolver whose output is *visual*, not `Block` text, so it needs a
  block variant or a swatch-embedding seam (a `Block::Canvas`-shaped
  hole, decided with platen). This hole is the non-box escape hatch the
  [smolweb fidelity plan](2026-07-01_smolweb_fidelity_plan.md) names: box-structured
  text shares the genet box substrate, but genuinely *visual* output (a diagram, a
  canvas) skips it and paints its own scene. A `dot` block is the same shape of case
  as gopher's fixed-width grid there: a line/output model boxes cannot honestly carry,
  so it earns bespoke rendering rather than being flattened into `Block` text.
- **Backend choice, no heavy native deps**: prefer pure-Rust or
  wasm-deliverable renderers (a Rust DOT layout, or the diagram engine
  compiled to wasm and run through the P3 wasm handler) over shelling to
  graphviz. The browser-reach rule applies: no native binaries.
- **Determinism**: layout is deterministic given the input, so a diagram
  swatch can cache as an engram (the K3 caching story extends to it).

**Done when**: a `dot` block renders its graph as a swatch inline; the swatch
embeds in the knot's rendered output through the platen seam; the renderer is
wasm/pure-Rust (no native graphviz).

## P3 — wasm module blocks (the universal, sandboxed any-language)

The real answer to "any language": a block that runs a wasm module, sandboxed,
through the seam the workspace already has.

- **Tag**: `wasm <module>` (a module by name/CID) or an inline `wat` body for
  tiny cases.
- **Handler**: `WasmHandler` over the existing `register-mod-loader`
  (extism + wasmtime + WASI) seam. The module gets a narrow host interface
  (text in, text/blocks out), no ambient capability. This is the path for a
  language with no Rust interpreter: compile it (AssemblyScript, Grain,
  TinyGo, Rust itself) to wasm and run it here.
- **Browser caveat (recorded)**: wasmtime is a JIT and is *out* of the
  browser delivery (the no-JIT rule, browser/PWA memory). So the wasm block
  kind is **native/desktop-only** for now; the browser target keeps the
  interpreter backends (Lua/Rhai/JS-via-Boa). A wasm-interpreter (no-JIT)
  backend for the browser is a separate research question, not this plan.
- **Trust**: wasm is sandboxed by construction (no syscalls without explicit
  WASI grants), so it is the *safest* untrusted-code kind, but it still rides
  the same consent gate as eval (received knots inert until consent).

**Done when**: a `wasm` block runs a named module (text in, blocks out) on
native, sandboxed with no ambient capability; the browser build cleanly
reports the kind as unavailable rather than failing.

## Cross-cutting (the rules every kind inherits)

- **Trust**: own SelfAsserted notes resolve per setting; received knots
  (flora, a peer's clip) render every resolvable block as inert source until
  explicit consent. One gate in P0, inherited by all kinds.
- **Determinism stays for policy, not content**: scripts and queries
  *generate content*; they never make trust decisions. Acceptance/brokering
  policy stays declarative data evaluated by Rust (the 2026-06-10 rule). A
  query block is data-shaped and re-runnable; a script block is content.
- **Provenance is uniform**: every spliced block carries where it came from,
  so a reader (and the a11y tree) can always see "this was evaluated /
  fetched / queried," never silent.
- **Caching (K3) generalizes**: a resolved transclusion, a query result, and
  a diagram swatch all cache as LocalOnly engrams with a staleness mark, so
  an offline render shows cached content honestly.

## Sequencing

P0 first (the spine; everything else registers into it). Then P1 (highest
value, reuses kernel + eidetic which exist), P2 (needs the platen swatch
seam), P3 (native-first, the extism seam exists). Each is independently
landable behind the registry, and each demos in a probe before the shell
surfaces it (the gated K3/E5 pattern).

## Out of scope (named)

A general graph query *language* (P1 starts with named queries); browser wasm
(P3 is native-first; a no-JIT wasm interpreter is separate research); shelling
to native diagram binaries (P2 is pure-Rust/wasm); the consent *UI* (shell,
gated); policy-as-script (stays data, forever, per the 2026-06-10 decision).

## Progress

- **2026-06-13** — Plan written after the K2 follow-on (the
  `BlockEvaluator` thin slice + Rhai backend) made the polyglot ceiling
  concrete. The eval kind now has a working menu (Rhai + Lua); this plan is
  the registry that makes the other kinds peers of it. No spine code yet;
  P0 is the next build when prioritized.
