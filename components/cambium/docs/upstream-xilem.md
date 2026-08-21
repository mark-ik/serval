# Xilem provenance and patch ledger

## Recorded bases

- Serval extraction source: `6b955ff96ed8b2912d04f7a36a85a36b401bb780`
- `mark-ik/xilem` main at audit: `5d72ad41eb660fa620110e045d332fd95684ebae`
- `linebender/xilem` main at audit: `c5950bcb03d4f3d187a20d1159f6aa276fd056bf`

Meristem began as Linebender's Apache-2.0 `xilem_core`, vendored into Serval at
`10b557c3d27003288bd54b86bb5225b4d8127e82`. The extraction repository replays
the four Serval commits that touched that subtree, preserving their authors,
dates, and messages before moving the files to `crates/meristem`.

The upstream side is a path-only replay of the 113 commits that touched
`xilem_core` through the recorded `mark-ik/xilem` base. The filtered tip is
`7f61f0537c2d911498cf0e7c940b377cb7673a76`; merge
`51a8a6a72fc1021ffcaa4c4d7a7ca5dbebddb7bf` joins that lineage to the
Serval-derived extraction without replacing Cambium's live tree. Patch replay
rewrites commit hashes but retains original authors, dates, and messages.

## Semantic patches over the vendored core

The initial Cambium patch set adds three defaulted `ElementSplice` operations:

- `hoist_pending`: preserve a backing node during a same-parent reorder.
- `extract_pending`: park a backing node without destroying it.
- `adopt_pending`: place a parked node under a new parent.

These operations support keyed and portable views in the Serval backend. Their
default implementations preserve compatibility for other Meristem backends.

### 2026-08-12 scope cut

Removed vendored surfaces with zero consumers across the family (cambium,
sprigging, hosts, turnstone, woodshed, mere, hocket, isometry, retinue):

- `views/fork.rs`, `views/run_once.rs` and `view_sequences/without_elements.rs`
  (side-effect views; the family runs side effects imperatively through
  app-level effect queues instead)
- `views/one_of.rs` with `OneOfCtx` and `PhantomElementCtx`, and its
  integration test
- `views/memoize.rs` (`Memoize` and `Frozen`), which had no family consumers
- `message_proxy.rs` (`MessageProxy`, `RawProxy`, `ProxyError`)
- `environment.rs` (`Environment`, `Provides`, `WithContext`, `Resource` and
  friends), including the `ViewPathTracker::environment()` hook upstream
  marked temporary, the `Environment` parameter of
  `MessageCtx::new`/`finish`, and Cambium's take/restore threading (G2.2)
- the now-orphaned `hashbrown` dependency (the `Environment` slot map was its
  only consumer)
- `docs.rs` (`DocsView`, `DocsViewSequence`, `Fake`, and `Nothing`); its only
  external reference was one lens doctest, now an explicitly illustrative
  example

Public trait count fell from 16 to 10. Reconciling these files against a
future upstream pull means re-evaluating, not re-vendoring: see
`docs/2026-08-12_meristem_scope_cut_and_component_contract_brief.md` at the
repository root for the rationale and receipts. The workspace now versions
`meristem` as 0.2.0 for the removed public API; registry publication remains
a separate release operation.

## Update policy

Reconcile against a recorded Xilem release or commit. Compare the retained core
surface, update this ledger, and run the keyed and portable-move tests before
accepting an upstream change.

The `upstream-xilem` remote points to `mark-ik/xilem`. Fetching it does not
merge the wider Xilem workspace; updates are filtered to the core path first.
