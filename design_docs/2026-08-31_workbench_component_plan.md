# Workbench Component Plan

**Date**: 2026-08-31
**Status (2026-08-31)**: W1 through W3 are implemented on coordinated Genet
and Mere feature branches. Focused core and cross-repo receipts pass. W4
remains gated on headed host and second-product receipts.

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

`components/genet-host-api/tile.rs` is a zero-dependency split/tab tree with a
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

`genet-host-api::tile` may temporarily re-export `workbench` so consumers can
move without a flag day. It contains no second definition. The compatibility
module is removed after the owned consumers and pinned external consumers use
the new crate directly.

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
- `genet-host-api::tile` re-exports the exact Workbench types.

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
Woodshed, can use Workbench with an open content lane. Remove
`genet-host-api::tile` only after every pinned family consumer has migrated.

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
  published MPL-2.0 package `workbench`. `genet-host-api::tile` is now a real
  dependency-backed compatibility re-export. The `Workbench` wrapper returns a
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
