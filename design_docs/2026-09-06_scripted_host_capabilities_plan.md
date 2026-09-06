# Scripted host capabilities and first conformance slices

**Status (2026-09-06): in progress.** The first wave connects ordinary scripted
pages to host services while completing one independent generated-content
slice. Implementation starts at `44b650590359f96e3f9b8c4240d539a077d172ea`
in detached worktrees. Existing shared-checkout work and the K6 continuation
remain separate.

## Objective and ownership

A hosted page must be able to execute script, request data asynchronously,
mutate its live DOM, and present the resulting frame. Script runtime tests
alone do not establish this path. Host construction, completion delivery,
navigation retirement, and rendering must participate in the receipt.

Genet owns the runtime's engine-neutral web API behavior, document lifecycle,
style/layout and render output. The host supplies transport policy and the
shared GPU device. Boa and Vano implement the JavaScript backend contract;
Piccolo exercises shared engine operations where Lua has an equivalent. A
JavaScript bootstrap passing on Boa does not establish Piccolo support.

The runtime remains free of transport and GPU dependencies. Loading source
resources continues through `genet-host-api::ResourceFetcher`. Page `fetch()`
uses the existing `FetchHandler`, which carries request mode, credentials,
headers, bodies, response filtering and cancellation. A byte-only resource
fetcher cannot silently substitute for those semantics.

## N1: ordinary scripted networking

1. Add a per-document options value with optional Fetch and WebGL capabilities.
   Install it before the first authored script. A session factory creates a
   fresh value for each navigation; document-local request IDs never address a
   different document's completions.
2. Extend the existing deferred Fetch handler with bounded completion polling,
   pending-work reporting and whole-document cancellation. Runtime methods
   consume replies through the established Fetch task/microtask checkpoint.
   The document pump observes its existing hidden/frozen lifecycle rules.
3. Supply a real network adapter and register it through the ordinary host
   construction path. Reuse the network engine's origin and response policy;
   do not infer authorization from successful byte loading.
4. Exercise success, network failure, mid-flight abort, origin handling and
   replacement with a request outstanding. Include a local HTTP server and a
   visible DOM change through the normal host.

**Done:** independently asserted runtime and document fixtures pass on Boa and
Vano, the local-server/host receipt shows fetched content, and retirement
prevents stale delivery. Each receipt states its source, dependency lock,
features and host. This does not close the Fetch specification or HTTP corpus.

## G1: generated text

The existing Row 17 inventory owns counters, markers and generated content.
The admitted first slice is inherited `list-style-position: inside` with
`list-style-type: disc`, producing a literal marker before item text. The
initial position remains `outside`. Outside placement, decimal counters,
`content` strings and authored pseudo styles remain separate work.

**Native gate:** compare independently authored literal bullet text with the
generated paint output, including order and marker suppression. **WPT gate:**
run an exact upstream fixture on frozen baseline and candidate runners. The
currently found multi-case fixture also requires square, Roman and decimal
markers plus dynamic mutation; it cannot establish this narrow slice. Until
a suitable receipt exists, record native coverage and claim zero WPT gains.

## W1: WebGL session and presentation

Trace the context factory from host construction to page script, then trace
the producer texture through Livery paint, translated-frame metadata and
host composition. Preserve the host's single device and page draw order.
Use the existing implementation as the starting point; changes to the
shared checkout's dirty WebGL work are excluded from the candidate.

**Done:** an ordinary scripted page creates a context and draws verified pixels
through the host, with focused resize and context-lifecycle coverage. If a
lower connection lands first, record its exact receipt and keep host closure
open. Broader shader/API correctness belongs to the Khronos corpus.

## Validation and integration

Each worker owns a detached checkout and an isolated target directory. Freeze
the generated Cargo.lock and its digest with runner receipts, because the
repository ignores that file. Use the committed dependency graph; local
overrides require an explicit recorded reason. Build with bounded concurrency
alongside the already-running K6 and sibling tasks.

Root owns shared constructors and consolidates scoped commits. Run affected
crate checks, meaningful behavior tests, scoped strict Clippy, formatting and
diff checks. Compare named WPT directories where a rendering behavior changes.
Explain every pass-to-fail movement. Integrate without staging another task's
dirty files, then refresh the parent plans and canonical work map.

## Findings

- **2026-09-06:** `genet-documents/src/engines/scripted.rs` supplies the normal
  Livery scripted session with resource loading, but does not install a page
  Fetch handler. `LiveryScriptedDocument::build` evaluates authored scripts
  immediately, so installing a handler after construction misses first-script
  requests.
- **2026-09-06:** `script-runtime-api` already has deferred start, cancellation,
  manual settlement and streaming entry points. The missing polling and
  lifecycle bridge should extend that seam rather than introduce a second
  Fetch implementation.
- **2026-09-06:** legacy `ScriptedDocument` exposes WebGL factory constructors,
  while `LiveryScriptedDocument` does not. Its scene-only frame translation
  also discards external-texture metadata that the render translator can
  preserve. Host consumption needs its own proof.

## Progress

- **2026-09-06:** Sol owns deferred networking, Terra the generated-text slice,
  Luna WebGL host tracing and integration. Root owns document/session options,
  retirement hooks and consolidated validation. No conformance gain is claimed
  by this planning entry.
