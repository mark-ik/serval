# Scripted host capabilities and first conformance slices

**Status (2026-09-06): first native host slice validated; program in progress.**
Ordinary scripted pages now receive per-document Fetch and WebGL services.
Inside-disc markers have independent stateless and retained native receipts.
WPT comparison, wider generated content, and the remaining WebGL cases stay
open. Implementation starts at `44b650590359f96e3f9b8c4240d539a077d172ea`
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

## First native receipt, 2026-09-06

Source: Genet `1b8054d258dd4a942f9d94cb0aa9e77a551b3dcc` and Mere
`ac254fc41c9b499907e655b41133b38c82eb27fa`, on isolated
`codex/scripted-host-capabilities-20260906` branches. Boa and Vano needed no
engine changes. Piccolo's Lua surface was not exercised by this JavaScript
wave.

| Gate | Result |
|---|---|
| Boa/Vano page Fetch, including failure, abort and replacement isolation | 6 passed |
| Boa/Vano WebGL factory, CSS bounds, trusted keys and immediate retirement | 2 passed |
| Session construction, fresh capabilities and redirected final origin | 3 passed |
| Canvas paint, replaced sizing, aspect-ratio regressions and marker comparisons | 11 passed |
| Integrated Mere resource/Fetch adapter, including opaque local origin and shared limits | 11 passed |
| Pelt scripted routing, real HTTP constructor path and real GPU composition | 5 passed |

The 38 unique native tests pass on Windows x86_64 with Rust 1.97.1. The
Pelt GPU receipt asserts literal red canvas pixels, an opaque blue later
sibling, white surrounding pixels, and registry retirement after document
drop. Its Fetch receipt uses a local HTTP server through the ordinary deferred
viewer constructor. Separately, the bounded headed P5 scripted receipt creates
a window, presents one 960 x 640 frame, and verifies parser/timer mutations,
prevented navigation, navigation to the next page, and Back replay. The PNG
was inspected; receipt digest is `9fdce1c66a85e37a`.

Frozen binaries, logs, locks, source map and PNG live under
`testing/genet/host-receipts/2026-09-06-scripted-host/` in the workspace testing
root. `manifest.json` has SHA-256
`30552b2e26a23937b85e9b034e15cab6b0381df4f822635ec8573783dcf96c4d`.
Genet lock SHA-256 is
`38e08a2781b1ca4579f5e1e06deb2efd7e570f57f68bea8902394009c27bc8a3`;
Mere candidate lock SHA-256 is
`77314cabaaca49672d9248fe2d48685b1c90ee1b87c9fcf6ec4a4cadeece0d1c`.
All cargo runs used `--locked --offline -j1` and debug information disabled.
Mere's receipt uses the recorded explicit source map into the isolated Genet
checkout. It is a candidate integration receipt, not a config-free build from
the final published pins. The headed process used hidden startup while still
creating and presenting through the native window/surface path.

The tests forced repairs to explicit canvas sizing, stateless marker shaping
and owner emission, deferred viewer startup errors/title, opaque origins for
bare local paths, explicit runtime capability retirement, and the final GPU
target's render-attachment usage. They also corrected two fixture assumptions:
Text Directive removal retains an empty `#`, and semantic clipping is not a
reliable assertion for a tiny paragraph. Session tests now assert laid-out
fetched text directly.

Formatting and diff checks pass. Strict Clippy is **not green**: the broad
Genet run stops on existing paint-types `derivable_impls`; the dependency-free
scope stops on the existing eight-argument replaced sizing helper; Mere stops
on existing Meristem `type_complexity`. Their definitions are present at the
accepted bases. The frozen logs retain these failures; no lint suppression or
unrelated cleanup was added.

No frozen WPT run was produced, so this receipt claims **zero WPT gains**.
Outside markers, counters and authored generated content remain open. The
WebGL presentation proof admits untransformed, unclipped canvases with opaque,
ungrouped later scene operations. Ancestor grouping/clipping/transforms,
translucent tail composition, HiDPI raster quality, canvas drawing-buffer
resize, context loss and the wider Khronos API corpus still need their own
receipts. Fetch remains a bounded full-buffer host bridge; this does not close
streaming or the full Fetch corpus.
