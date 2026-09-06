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

## Second wave: inside decimal markers and live canvas resize

Validated source is `b4af4f9d473f6dc1645dbd119ce27847c188c36a`, with
the marker-only native gate run at its ancestor
`163ac89e50f04c06ca4a88c34a90feb460658441`. Eight marker comparisons,
eight WebGL binding tests, two GPU backend tests and both Boa/Vano host tests
pass. Boa, Vano and Piccolo repositories required no changes for this slice.

Inside decimal markers now follow direct HTML `ol > li` ordinals, including
`start`, `value`, hidden-item exclusion and independent nested lists. The
marker-only inline run before a nested block contributes line height in
stateless layout and reconstructs paint at its own content origin. The
ordinal native fixtures fix line height at 19px; the separate nested
marker-only fixture exercises normal line height with border and padding.
General normal-line-height rounding remains open.

The release WPT baseline is `d424a689518a53e38d4c6f89c5c792bebb439248`.
Both runners use the same frozen lock, manifest, 708 input-file hashes,
Livery renderer, device scale 1 and exact expectation policy. The existing
GPU pixel tolerance and authored WPT fuzz are unchanged.

| WPT scope | Before pass/fail/skip | After pass/fail/skip |
|---|---|---|
| `css/CSS2/lists` | 61 / 94 / 131 | 62 / 93 / 131 |
| `css/css-lists` | 70 / 70 / 78 | 70 / 70 / 78 |

All 504 result-map members are identical. The sole status change is
`css/CSS2/lists/list-style-position-023.xht`, fail to pass; there are zero
pass-to-fail changes and zero errors. Its candidate test and reference PNGs
are byte-identical (SHA-256
`732ef33e44fcc82272dd695fa7503593f175299de871ecb5c9f568dbb12ae093`)
and were visually inspected. The frozen runners, logs, maps, input hashes,
comparison and PNGs are under the workspace testing root at
`testing/genet/wpt-ledger/2026-09-06_inside-decimal-markers/`.
This is one measured WPT gain; Row 17 remains in progress.

Canvas width/height reflection and ordinary attribute set/remove now resize
the existing host context, including redundant assignments. The host reports
actual dimensions back to JavaScript. Resizing clears the replacement buffer
and preserves the authored viewport and scissor state. The GPU backend rejects
dimensions above its device limit before texture allocation. Pelt replaces
the registered texture under the existing key and supplies a caller-selected
live-resize bound (default 4096 pixels per axis).

The live-resize receipt does not cover initial context size negotiation,
namespace-aware attribute mutations, the existing large unsigned IDL
reflection quirk, context loss or the wider Khronos corpus. The earlier
composition limits still apply. Reversed/outside markers, general CSS
counters, generated `content`, marker images and other inline pseudo-element
anchoring remain open.

Formatting and diff checks pass. Strict Clippy remains blocked: the existing
eight-argument replaced sizing helper in Livery, and seven existing WebGL
warnings in `state.rs`, `draw.rs`, `pipeline.rs` and `programs.rs`. Those WebGL
files are unchanged from the baseline. The logs preserve these failures;
this receipt does not claim lint closure.

The isolated `script-runtime-api` strict Clippy command exits successfully;
it still reports an existing invalid disallowed-type configuration warning.
The frozen WPT artifact manifest has SHA-256
`f3da58407d796434fa7c15c89c67ec98bb11ac00854469d3df534e449b72c712`.

### Next measured slices

The read-only follow-up audit identifies these separate candidate gates:

1. RTL inside decimal placement: `css/CSS2/lists/list-style-position-024.xht`,
   with an independent nested RTL marker-only geometry fixture and 023 kept
   passing.
2. Quoted inside string markers: `css/css-lists/list-style-type-string-001a.html`,
   preserving the string through parsing and computed style before shaping
   its literal glyphs. Bidi, multicolumn and float variants remain separate.
3. Outside disc marker placement: `css/css-lists/list-marker-alignment.html`,
   aligning a separate marker fragment with the first baseline of a block
   child in both stateless and retained layout.

These are proposals, not implementation receipts. Some outside/reversed
tests currently match because neither image paints its marker. Therefore the
reported maps are raw runner statuses, not proof that every passing list test
demonstrates implemented behavior. The 023 gain was separately inspected and
does paint the expected nested decimal markers. Table/row/cell list-item
tests and general counter/generated-content tests require other owners or
broader slices.

### Published host integration receipt

Mere source `ec35fd6ceb02469fcc47292635ea687316bc12fb` consumes the tested
Genet source and Knot `e2294ed94faa9f5d046e11ba9e6b0861b6621cbd`. Full
metadata with `pelt-desktop/scripted` enabled resolves 41 Genet packages at
the same pin and one registry wgpu 30.0.1. No local source override or
candidate source map is used. All 43 Mere Genet selectors agree, including
excluded web examples. The narrow Knot update preserves its application
code and registry versions; its standalone application remains a separate
gate because its unchanged Mere pin still introduces older Genet packages.

All five Pelt scripted tests pass, bringing this wave to 25 passing native
tests across Genet and Mere. The GPU fixture reports a host-bounded 32 x 6
buffer for an oversized live width, restores 8 x 6, checks a cleared buffer
after redundant width assignment, retains 32 x 32 CSS bounds, verifies red,
blue and white composition pixels, and checks registry retirement. The
published build first exposed a test-only call to a nonexistent `title()`
method; the final fixture asserts the same literal result through the public
script-evaluation API. The initial failed log is retained alongside the
passing log.

`cargo check -p knot-editor --lib --locked --offline -j1` passes from Mere's
workspace against the published graph. This validates the embedded library,
not the standalone app. Mere strict Clippy reports 16 existing errors in
`workspace_viewer.rs`, `workspace_viewer/accessibility.rs` and
`scripted_viewer.rs`; those files are unchanged from the first-wave Mere
base. The modified graphics file passes formatting and diff checks.

Frozen host logs, locks, full metadata, commands and source identities live
under `testing/genet/host-receipts/2026-09-06-scripted-host-wave2/` in the
workspace testing root. Its manifest also pins the separate WPT artifact
manifest. The original first-wave receipt remains unchanged.
