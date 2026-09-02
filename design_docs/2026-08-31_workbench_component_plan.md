# Workbench Component Plan

**Date**: 2026-08-31
**Status (2026-09-01)**: W1 through W3 are implemented on coordinated Genet
and Mere feature branches. W4 now has captured native Pelt acceptance and
cancellation receipts, a headed Graphshell browser save/mutate/reload receipt,
and a durable Woodshed open-lane consumer with full view and host receipts.
The temporary `genet-host-api::tile` compatibility module is now removed.
Pinned products that still need their own current-Genet port retain that work
outside the shared component contract.

## Ruling

Workbench is Genet's reusable workspace-organization component. It owns the
presentation-grade split tree, tab stacks, stable tile identities, arrangement
commands, and host effects such as a tearout request. It does not own browser
sessions, graph arrangements, source data, operating-system windows, or
projection definitions.

Pelt and Graphshell are parallel hosts of Workbench. Pelt attaches document
sessions, engine routing, history, and browser chrome. Graphshell attaches the
projection editor, inspectors, previews, and local or remote endpoint actions.
Mere's Forme remains durable graph-arrangement authority; Platen compiles a
Forme arrangement into a Workbench presentation and translates accepted edits
back through Mere policy.

The crate is named `workbench`. The visible Graphshell authoring tool uses the
plain UI name **Projection Editor**. The editor may be arranged by Workbench,
but Workbench itself stays reusable workspace furniture.

## Findings

### 2026-08-31: the reusable core already exists under the wrong owner

`components/genet-host-api/tile.rs` — since moved to
`components/workbench/lib.rs`, the shim removed — was a zero-dependency split/tab tree with a
shared reducer. Its documentation already says that Pelt owns its local tree
while Mere projects Forme onto it. `DropTarget::Outside` already identifies the
tearout boundary and deliberately leaves the tree unchanged for the embedding
host. The missing piece is a typed host effect and a crate home that matches the
contract's actual responsibility.

`pelt-core::PeltWorkspace` adds browser controllers and routing to that tree.
Cambium's `frisket` renders the tree and emits commands. Neither layer should
become the generic workspace owner.

Mere currently also publishes a package named `workbench`, but that package is
only an AccessKit projection over `platen::Workbench`. The reusable package
name should move here. Mere can keep the small projection function inside
Platen and use a compatibility type alias while its graph-specific layout type
is renamed.

## Boundaries

| Layer | Owns | Explicitly outside it |
| --- | --- | --- |
| `workbench` | Tile tree, tab stacks, split fractions, commands, validation, typed host effects | Documents, graphs, windows, persistence policy |
| Cambium `frisket` | Retained realization, hit regions, accessibility targets, command emission | Authoritative workspace state |
| Pelt | Browser content, controller lifetime, engine routes, history, native shell response | Generic workspace vocabulary |
| Mere Forme/Platen | Durable graph arrangement and projection into Workbench | Generic window organization |
| Graphshell Projection Editor | Projection draft, validation, preview, typed endpoint requests | Source authority and portable scene mutation |
| Native/web host | Window creation, tearout acceptance, storage choice, process lifecycle | Reusable reducer semantics |

`genet-host-api::tile` temporarily re-exported `workbench` during the migration
without a second definition. It is now removed: Genet host contracts import the
shared types directly, as do the audited migrated consumers.

## Phases

### W1. Establish the component

Move the existing tile vocabulary and reducer into `components/workbench`.
Add a `Workbench` state wrapper whose command result distinguishes an applied
tree change, an unchanged command, and a `TearOut` host request. An outside drop
must not remove the tile before a host accepts custody.

Validation:

- Existing reducer tests move with the contract and remain green.
- New tests prove that an existing tile produces a tearout request without a
  tree mutation and an unknown tile produces no request.
- `genet-host-api` imports shared Workbench types directly, without an alias.

Done when there is one implementation of every tile type and reducer.

### W2. Adopt from Genet hosts

Cambium Frisket and Pelt import the `workbench` crate directly. Pelt stores the
`Workbench` state wrapper while preserving its existing `tree()` and boolean
`apply()` compatibility methods. A richer apply method exposes host effects to
desktop shells without making Pelt create a window.

Validation:

- `workbench`, `genet-host-api`, Cambium, `pelt-core`, and `pelt-desktop`
  compile against one Workbench package.
- Existing Pelt workspace tests retain their behavior.
- A focused Pelt test observes a tearout request and unchanged controller
  custody.

Done when Pelt and Cambium no longer import tile types through
`genet-host-api`.

### W3. Adopt from Mere and found the Projection Editor

Move Mere's AccessKit-only `project_workbench` helper into Platen and retire
the extra package that previously held the `workbench` name. Rename the
graph-specific `platen::Workbench` implementation to `TileLayout`, retaining a
short compatibility alias while downstream products migrate. Platen imports
the new Workbench presentation contract directly.

Add a Graphshell `ProjectionEditor` component that owns an editor workspace
and a typed projection draft. Its initial panels are source, reading,
encoding, arrangement, interaction, preview, and provenance. Saving produces a
typed request for a host-provided sink; it does not write graph or endpoint
authority directly.

Validation:

- Platen round-trips Forme arrangement plus tree geometry unchanged.
- Graphshell can construct the editor, apply workspace commands, validate a
  draft, and hand the validated definition to a fixture sink.
- Graphshell's local/remote projection client remains independent of editor
  state.

Done when Graphshell hosts a real authoring component and Mere has only one
package called `workbench` in its dependency graph.

### W4. Host receipts and compatibility removal

Wire tearout acceptance in one native host and embed the Projection Editor in
one headed Graphshell surface. Prove a second non-browser consumer, preferably
Woodshed, can use Workbench with an open content lane. The temporary
`genet-host-api::tile` alias is removed after an audit confirms its real users
have imported `workbench` directly.

Validation:

- A headed native receipt tears a tile into a second window without losing
  content custody or focus identity.
- Pelt and one non-browser product serialize and restore their own workspace
  policy over the same core snapshot.
- A Graphshell-authored projection previews, saves, reloads, and retains its
  source/provenance account.

Done when the compatibility re-export is unused and the same component has two
heterogeneous headed consumers.

## Progress

- 2026-08-31: ownership ruling recorded after checking `TileTree`,
  `PeltWorkspace`, Cambium Frisket, Mere Platen/Forme, and Graphshell's current
  local/remote host. W1 and W2 started in an isolated Genet worktree because the
  shared checkout acquired concurrent edits during the review.
- 2026-08-31: W1 moved the sole tile vocabulary and raw `TileTree` reducer into
  published MPL-2.0 package `workbench`. During migration,
  `genet-host-api::tile` was a dependency-backed compatibility re-export. The
  `Workbench` wrapper returns a
  typed `TearOut` effect for a valid outside drop without changing the tree, and
  treats an unknown tile as unchanged.
- 2026-08-31: W2 migrated Cambium Frisket and Pelt core/desktop imports to
  `workbench`. `PeltWorkspace` now retains the wrapper, preserves `tree()` and
  boolean `apply()` behavior, and exposes a richer Pelt outcome so desktop hosts
  can observe a tearout request before deciding custody. Focused unit tests cover
  core tearout and Pelt controller custody. `cargo test -p workbench -p
  genet-host-api -p cambium --offline -j 1` passed (15, 9, and 179 tests), as
  did the focused Pelt custody test and Pelt desktop checks with and without the
  `livery` feature.
- 2026-08-31: W3 landed on Mere branch
  `codex/workbench-integration-20260831` at `565285020ca26dcc63dbc6c256f4b9372d503de2`.
  Mere pins this package at Genet revision
  `d25ef444d216cc71f6897d122c55a92530d5a6ca`, Platen projects directly into
  the shared types, and the former Mere `workbench` package is retired.
  Graphshell's Projection Editor hosts its seven tools as open-lane Workbench
  tiles while draft validation and persistence remain editor and host concerns.
  A standalone cross-repo harness passed eight focused tests. The full Mere
  workspace gate still encounters its older `genet-taffy =0.13.1` patch mismatch
  and workspace-wide resolver fan-out, which are outside this component slice.
- 2026-08-31: W4's native Pelt route now keeps one shared `RenderCore` and
  creates an OS-decorated secondary `WindowSurface` rather than booting a
  second device. A document-controller outside drop creates a hidden
  destination at that window's own size/DPI, composes and presents a
  source-owned frame, then transfers the stable `TileId`, controller, route,
  and model focus through `accept_tearout`; only then does it show/focus the
  destination. The accepted window redraws, resizes, routes pointer, key, IME,
  and focus input, and closes independently. Pre-accept cancellation,
  configure/present failure, and the rare acceptance race preserve source tree
  membership, controller custody, and model focus, then restore its primary
  geometry/DPI. Native surface-producer import is a Pelt-owned follow-up: it
  now accepts custody only after a typed shared-device import or cache-transfer
  receipt has composed the hidden destination. Secondary-window AccessKit is recorded below.

  The headed acceptance command was captured at exit 0:
  `pelt.exe --workspace-tearout-receipt --size 960x640
  ports/pelt/examples/workspace/p5-fallback/index.html`. Its stdout recorded
  `window=true redraws=3 size=960x640 tiles=1 tearout_receipt=true
  tearout_cancellation_receipt=false routes=2=genet.livery:document`.
  The remaining primary tile is correctly tile 2 after transferred source
  custody. The separate cancellation command with the same fixture exited 0
  and recorded `window=true redraws=2 tiles=2 tearout_receipt=false
  tearout_cancellation_receipt=true routes=1=genet.livery:document,2=genet.livery:document`.
  It proves that the real hidden native preflight was declined before
  `accept_tearout` and retained source custody. Computer Use separately
  captured the live interactive `Pelt — Pelt fallback receipt` window with two
  fallback tiles and splitter; its accessibility tree exposed both tab items,
  content regions, and the separator.
- 2026-08-31: Graphshell's authoring surface advanced after the initial
  `ProjectionEditor` boundary. Mere commits
  `7323c703bf3c989ce0fe8c240cd047a4bfbd2fcc` (headed editor surface),
  `f01dd1914937e4cf5bbc8cc1a052e26922ba54fc` (authoring loop), and
  `86eb4331f179862a4a5e8c02faea7f7b3af2e972` (keyboard guard) attach the
  editor to Graphshell's web entry point while retaining draft, save-sink, and
  endpoint authority in Graphshell. The earlier standalone cross-repo harness
  remains the eight-test component receipt. The headed browser receipt is now
  green at Mere `49f9b99ed52e543c992a1a45e599dc6760d2ab41`: the stripped
  `wasm32-unknown-unknown` bundle is 34,679,440 bytes with SHA-256
  `A6A43EA0D1FB510E9EEF897B6C9232BB66F5D1F5927DF54DADE3463F55D3DB85`,
  and the browser completed the full source/provenance save, mutate, and reload
  loop.
- 2026-08-31: Woodshed's final second-consumer branch
  `codex/workbench-consumer-20260831` is at `1611201`.
  `woodshed-views` owns a four-panel stable-ID `ContentSource::Open` workspace
  over existing Practice, Set, Related, and Settings surfaces. Its active
  panel is host-global across split-local stacks; divider fractions are
  validated; and its JSON workspace snapshot is embedded in the existing
  `PersistedSession` with a safe fallback. Woodshed translates the shared
  typed tearout request into a host effect and keeps product/persistence
  authority local. The root Parley patch is present, with lock pins for
  Workbench `d25ef444d216cc71f6897d122c55a92530d5a6ca`, Parley Genet
  `583266`, and `wasm-bindgen` `0.2.127`.

  The final locked view receipt,
  `cargo test -p woodshed-views --locked --offline -j1 --config …`, passed
  11/11. The exact host restore receipt,
  `cargo test -p woodshed-genet host_session_restores_the_workspace_policy
  --locked --offline -j1 --config …`, passed 1/1 after 15m16s. Formatting and
  diff checks passed. This closes the non-browser open-lane and Woodshed
  persistence evidence, but is not a Woodshed headed workspace receipt.
- 2026-09-01: Turnstone's direct Workbench consumer migration landed at
  `75af890`; its full offline `cargo check` is green. Hocket's direct import
  migration landed at `bc98cc8`, but its focused compile remains blocked before
  compilation by Hocket's pre-existing dependency on removed `genet-layout`.
  That is a separate product port, not a Workbench alias use.
- 2026-09-01: the remaining `genet-host-api::tile` compatibility module and its
  re-export test were deleted after the Genet source audit found no real alias
  consumers. `genet-host-api::settings` imports `workbench::SettingsRef`
  directly and retains its real Workbench dependency. W4's native Pelt
  acceptance/cancellation, Woodshed open-lane/persistence, and Graphshell
  wasm/browser-headed evidence are captured. The native surface-producer
  follow-up remains Pelt-owned and does not change the Workbench contract.
- 2026-09-01: the coordinated consumer-first remote landing completed.
  Woodshed `main` is `1611201c903`, Turnstone `main` is `75af89070bb`, and
  Hocket `main` is `bc98cc8ee83`. Mere's `main` advanced concurrently, so the
  Workbench branch merged it without conflicts before landing as
  `2f85051245f`; the incoming Djinn, Distillery, Pandect, and Castellan paths
  had no changed-path intersection with the Platen or Projection Editor lane.
  The earlier headed Graphshell browser receipt remains attached to feature
  parent `49f9b99ed52`. A repeated post-merge focused test was stopped while
  Cargo remained in workspace resolution before starting rustc, so it is not
  an additional green receipt. Audits of all four remote consumer `main`
  sources found no remaining `genet-host-api::tile` uses. Hocket's broader
  retired Genet patch/layout port remains separate from this migration.
- 2026-09-02: Pelt's native surface-producer tearout follow-up is implemented
  on Genet branch `codex/pelt-surface-producer-20260902`. A destination-owned
  `SurfaceTearoutImportReceipt` either imports the first D3D12 shared frame on
  the existing `RenderCore` device or provisionally transfers the source's
  already-imported cache for a reused resource epoch. The receipt waits,
  composes, and returns the resource to `COMMON` before `accept_tearout`; every
  cancelled, import-failed, configure-failed, or pre-accept close path restores
  the source cache, viewport, tree membership, controller/surface custody, and
  model focus. Later destination frames refresh the same receipt. The isolated
  Windows gate `CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=C:\\t\\genet-pelt-surface-producer-target cargo test -p pelt-desktop --lib -j 1 --message-format=short`
  passed 53/53, including the D3D12 importer tests and retained-source tearout
  test; `git diff --check` passed. This is a native unit/compile receipt, not a
  new headed Scrying surface-tearout receipt. Secondary-window AccessKit was
  implemented in the parallel lane recorded next.

- 2026-09-02: Pelt's deferred secondary-window accessibility slice is
  implemented on branch `codex/pelt-secondary-accesskit-20260902`. Each
  accepted tearout owns a fresh `WorkspaceAccessibility` bridge, high-range
  child namespace, virtual focus state, and action map. The hidden preflight
  installs the adapter before reveal; every secondary redraw reprojects its
  stable tile/content aperture and document subtree, AccessKit wakes only that
  window, and closing the tearout drops its adapter with the window entry.
  Controller actions continue through Pelt's ordinary input and document
  session seams; Workbench remains unaware of OS accessibility. The focused
  Pelt suite passed 27/27. The headed workspace-tearout command exited 0 and
  logged independent `accessibility Installed` events with 20 primary nodes
  and 18 secondary nodes, followed by `window=true redraws=3 size=960x640
  tiles=1 tearout_receipt=true routes=2=genet.livery:document`. Physical
  screen-reader verification remains open.
- 2026-09-02: the deferred Graphshell post-merge test is closed on current Mere
  `main` `4d68c465e58`. The first native test link exposed a Windows archive
  limit: debug-heavy `libcanvas.rlib` was 4,266,610,132 bytes, MSVC rejected it
  with `LNK4003`, and the resulting link reported 94 unresolved Canvas
  symbols. Repeating the identical seven Projection Editor tests with
  `CARGO_INCREMENTAL=0` and `--config profile.test.debug=0` reduced that
  archive to 347,000,108 bytes; the executable linked and all 7/7 tests passed.
- 2026-09-02: the surface-producer and secondary-accessibility branches were
  merged as Genet `7092898fc02`. A cold isolated `cargo test -p pelt-desktop
  --lib --offline -j 1` passed 54/54. The combined headed tearout command also
  exited 0, installing independent AccessKit trees with 20 primary nodes and
  18 secondary nodes before reporting `window=true redraws=3 size=960x640
  tiles=1 tearout_receipt=true routes=2=genet.livery:document`.
- 2026-09-02: Hocket's separate current-Genet product port landed on Hocket
  `main` as `e8b4b137583`. It now uses `cambium-genet-winit-host`; Hocket keeps
  state, Firewheel ticking, workers, custom leaves, scenario capture, and
  update policy while the shared host owns lifecycle, layout, paint, input,
  and accessibility. The focused check passed, as did 55/55 tests after
  excluding `identity::tests::every_home_says_which_situation_the_user_is_in`.
  That assertion still fails because it requires the word `DPAPI`; its
  `identity.rs` is unchanged from Hocket's preceding `origin/main`.
