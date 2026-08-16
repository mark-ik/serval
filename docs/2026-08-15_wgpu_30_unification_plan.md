# wgpu 30 Unification Plan

Bring the active product stack onto a single wgpu 30 row. Compatibility
surfaces and retired checkouts are deliberately excluded: wgpu-graft,
wgpu-scry and wgpu-weld keep their multi-row feature sets.

Done means the active product packages resolve exactly one `wgpu`,
`wgpu-core`, `wgpu-hal`, `wgpu-types` and `naga` row at 30, followed by
platform receipts (Windows headed composition, CubeCL compute and resident
buffers, wasm WebGPU, Linux DMA-BUF, macOS IOSurface).

## The wgpu 30 break set

Five patterns cover every site found across seven repositories:

1. `BufferSlice::get_mapped_range` and `get_mapped_range_mut` return
   `Result<_, MapRangeError>`.
2. `VertexState::buffers` is `&[Option<VertexBufferLayout>]`. Gaps are
   unbound slots, per the WebGPU spec. Occupied slots wrap in `Some`, and
   slot indices are unchanged.
3. `SurfaceConfiguration` gained `color_space`. `SurfaceColorSpace::Auto`
   preserves pre-30 behaviour.
4. `RequestAdapterOptions` gained `apply_limit_buckets`. See the open
   question below.
5. Presentation moved from `SurfaceTexture::present` to
   `Queue::present(frame)`.

A sixth is not a wgpu API change but blocks every wasm build: wgpu-types 30
names `web_sys::VideoFrame`, which is unstable-gated in web-sys 0.3.91. The
floor for any wasm32 target in the stack is web-sys 0.3.92 and wasm-bindgen
0.2.115, and the wasm-bindgen family pins each other exactly, so it moves as
a unit.

## Phases

| # | Step | State |
|---|---|---|
| 1 | Vello fork | Done |
| 2 | Netrender | Done |
| 3 | Genet | Done except the three inker engines |
| 4 | CubeCL / Burn, then Mere and Quint | Done, via the backport after all |
| 5 | Renderling and Crabslab | Done, plus mesocosm and paredros |
| 6 | Downstream applications | Done for all five, plus mesocosm |
| 7 | wgpu-graft / scry / weld defaults | weld flipped; scry and graft still default 29 |

### 1. Vello fork

Upstream vello 0.10.0 shipped 2026-08-14 and still requires wgpu 29. Their
own migration, [linebender/vello#1754](https://github.com/linebender/vello/pull/1754),
has been open since 2026-07-19, is untouched since 2026-08-01, and is now
conflicted against main.

`mark-ik/vello` branch `mark-ik/wgpu-30` is the v0.10.0 tag plus exactly the
changes in that PR, so the fork retires by deletion when a wgpu-30 vello
ships. v0.10.0 was chosen as the base over v0.9.0 because the release is
small where it matters: 337 of its 374 changed files are the experimental
sparse-strips crates, and the classic `vello` crate changed two files.

Three fixes were needed beyond the PR, which is stale enough to have missed
them: `examples/simple_sdl2` (added upstream after the PR opened), the
`wgpu_webgl` example (wasm32 only, so a host-target check never compiles
it), and the wasm-bindgen family bump.

Native and wasm32 both build, all targets.

### 2. Netrender

wgpu 30, vello 0.10, and skrifa 0.42 to 0.44. The skrifa pin tracks vello's
transitive because `vello_rasterizer::emit` hands skrifa-resolved variation
coordinates straight to vello. parley 0.10 still wants skrifa 0.42, so a
second skrifa resolves for netrender_text. That is bloat rather than a
break, since no skrifa type crosses the parley to `netrender::Scene` adapter
boundary.

Workspace check clean, full test suite passes including the native GPU
tests.

### 3. Genet

The workspace pins moved, plus six crates that declared wgpu or naga
directly with their own feature sets. Two dependency-identity bugs surfaced
and were fixed:

- `pelt-desktop` took `sprigging` from the registry at `version = "0.1.0"`
  while cambium path-dep'd the workspace copy at 0.2.x, so two sprigging
  crates landed in one graph. That pin also held the last vello 0.9. It is a
  path dep now, for the same reason `cambium` already was; the comment on
  cambium describes the identical bug from 2026-07-14.
- genet's machine-local `.cargo/config.toml` reached netrender through a
  `paths` override. A `paths` entry may not change the overridden crate's own
  dependency list, and netrender's had changed, so cargo warned on every
  command that this "is known to produce buggy behavior with spurious
  recompiles and changes to the crate graph" and will become a hard error.
  It is a `[patch]` table now, matching what mere, woodshed and hocket
  already do.

Verified: servo-webgl-wgpu, servo-webgl-essl, servo-paint-api, servo-paint,
genet-render-host, genet-winit-host, genet-render, sprigging, pelt-desktop,
cambium-genet-winit-host, genet-wpt and script-runtime-api, all targets. The
65 WebGL GPU tests pass in 5.9s.

Verifying only the focused rendering row missed several crates, because much
of the remaining surface is platform- or feature-gated (the three pelt
platform smokes, the vulkan timeline interop, the wasm example) and a
host-target check never compiles it. The second pass grepped the tree for
the five break patterns instead of following build errors. Prefer that
order.

**Blocked:** the three inker engines. `scrying-engine` fails with two
`wgpu_types` in the graph, because `scrying` comes from wgpu-scry main,
still on wgpu 29. This is an ordering correction: step 7 is a prerequisite
for finishing step 3, not a tail step.

### 6. Downstream applications

All five are clean: turnstone, cleromancy, woodshed, hocket and isometry.
mesocosm was left alone, since it had uncommitted work at the time.

The last three were each blocked first by something that had nothing to do
with wgpu, and each blocker had to be cleared before the migration could
even be verified:

- woodshed: two `personae` in one graph, so `Roster` and `ProfileId` did not
  match themselves. The `[patch]` was correct and applying; the lock had
  merely retained a second git-sourced `personae 0.1.1` at mere rev
  7b6b4130. `mere-persona-picker` reaches personae by sibling path inside
  mere's workspace and so always had the local 0.2.0, which is what put the
  two sides of one call on different types.
- hocket: the workspace dep carried `version = "0.1.0"` next to the mere git
  branch. That is a real requirement, so personae 0.2.0 satisfied nothing
  and resolution failed outright. This one genuinely needed the requirement
  moved.
- isometry: `graphshell-protocol` was renamed to `chirograph` on 2026-08-14.
  Separately its lock pinned mark-ik/p2panda before the
  use-core-traits-in-auth merge, so every `--all-features` build failed on
  trait bounds inside mere's own gemot. mere was unaffected because its
  config resolves p2panda by local path.

Three of those four are stale lock entries rather than wrong manifests. The
sibling lattice reaches most of its dependencies through git branches, and a
lock pins a branch to a rev, so a repo goes stale silently whenever a
sibling moves and nothing in that repo changes. It presents as a version
skew in the manifests, and the manifests are fine. Check whether a duplicate
or a missing symbol is merely locked before editing requirements to chase
it: `cargo update -p <name>@<version>` for a duplicate, `cargo update -p
<name>` to move a git source.

### 4. CubeCL, Burn, Mere and Quint

The 2026-08-15 progress note below says this step "no longer needs the narrow
CubeCL 0.10 backport" because CubeCL and Burn had crossed to wgpu 30 on their
own. **That was wrong, and the backport is what shipped.**

Two independent obstacles killed the prerelease row:

1. Mere's own [burn 0.22 migration plan](../../mere/design_docs/mere_docs/implementation_strategy/2026-08-09_burn_0_22_migration_plan.md)
   is release-gated on stable 0.22, which still is not published, with an
   explicit "unless Mark explicitly reopens that gate". He was asked and did
   reopen it.
2. It does not resolve regardless. On `cfg(any(windows, linux, macos,
   android))`, `cubecl-runtime 0.11.0-pre.2` depends on `cubecl-environment`
   with the `cache` feature **non-optionally** — a persistent autotune cache —
   pulling `rusqlite ^0.40` and so `libsqlite3-sys ^0.38`. Mere's
   `p2panda-store` reaches `sqlite` through its `groups`/`encryption`
   features, which `gemot` needs, pulling `sqlx` and so
   `libsqlite3-sys >=0.30.1, <0.38`. No released sqlx, 0.9.0 included, reaches
   0.38, and `libsqlite3-sys` sets `links = "sqlite3"`, so exactly one may
   exist. Version selection cannot fix it.

So the original plan's step 4 was right. `cubecl-wgpu` is the **only** crate in
the entire burn 0.21 / cubecl 0.10 stack that names wgpu (cubecl, cubecl-core,
cubecl-runtime and cubecl-common have no wgpu dependency at all), so one
vendored crate carries the whole resident lane across the major. It lives at
`mere/support/patches/cubecl-wgpu`, three call sites changed, burn stays at
stable 0.21.

Quint's own resident lane needed one non-obvious fix beyond the usual set.
wgpu 30 added `entry_points` to `ShaderModuleDescriptorPassthrough`, and its
`Default` is an **empty list**, because passthrough skips naga and therefore
cannot reflect entry points. Leaving it defaulted compiles cleanly and then
fails at pipeline creation with "Unable to find entry point 'repulse'". Any
other SPIR-V passthrough site in the family will hit this the same way.

### 5. Renderling, Crabslab, and the games wing

Both had uncommitted wgpu-29 work in the tree. Neither was buildable as
found: crabslab's missed its own `wgpu_slab` test helper, and renderling's
left `crates/example` and `tests/wasm.rs` on the wgpu 26 API. So there was no
sound wgpu-29 checkpoint to commit, and both went to 30 in one commit each,
on branches (`mark-ik/wgpu-30`). Neither repo has a fork remote, so both
branches are local only.

Two things were worth the care the plan asked for:

- crabslab's uncommitted work sat on the **wrong base**. The checkout was
  detached at tag v0.6.6, whose craballoc is 0.3.0, and the work relabelled
  it 0.3.1 to satisfy renderling's `craballoc = "0.3.1"`. Published 0.3.1
  came a week after that tag, so renderling was being fed older code wearing
  a newer label. Commit `1ae313c` is byte-identical to published 0.3.1 across
  all five source files; `122a96f`, one commit later, already differs by 172
  lines in `value.rs`. The port is rebased onto `1ae313c`.
- the work also carried a real fix worth keeping: `SlabBuffer`'s
  `source_slab_buffer` is `Weak` rather than `Arc`, because the allocator
  stores a `SlabBuffer` in that very slot and a strong pointer is a
  self-referential cycle that leaks the GPU buffer.

**The interesting bug.** With everything ported, renderling's whole 95-test
GPU suite failed with

    Validation Error: Copy at offset 17179869180 bytes would end up
    overrunning the bounds of the Destination buffer of size 64

17179869180 is `u32::MAX * 4`. An empty `SlabUpdate` carries `Array::NONE`,
whose starting index is `u32::MAX`, and craballoc turned that into a
`write_buffer` at that offset with **zero bytes** of data. wgpu 26 ignored
the offset for an empty write; wgpu 30 validates it regardless of length. A
long-standing no-op became fatal. Guarding on `elements.is_empty()` in both
runtimes fixes it, and the suite goes green.

Worth generalising: the first three suspects were all wrong, and each was
eliminated by a control rather than by reading code. A pristine wgpu 26
worktree passing 95/95 is what established there was a real regression at
all; reverting the `Weak` change on the correct base is what cleared it;
and the probe that printed the offending update (`starting_index=4294967295
len=0 elements=0 buffer_size=64`) is what actually named the cause. Keep the
control until the cause is named.

Downstream of those two: mesocosm and paredros both moved. mesocosm's own
HEAD commit had recorded this exact split as a blocker on lane F ("a
`wgpu::Buffer` does not cross a major version"), diagnosed it correctly, and
concluded the repair is to move the row. Its lock was also holding netrender
at a pre-30 commit, which is the only reason it still built. paredros needed
mesocosm first, since `mesocosm-render` is a path dep there and held the last
wgpu 29.

**Test parallelism.** At wgpu 30, four of renderling's heavier tests fail
with wgpu "Out of Memory" under the default test parallelism, where the wgpu
26 control passes all 95 in parallel repeatably. Single-threaded is clean at
both. wgpu 30 wants more VRAM under concurrent device load; upstream crabslab
already moved its CI to single-threaded, and renderling should follow.

### 7. The compat repos' defaults

All three carry feature-selected wgpu rows (28/29/30) — that multi-row surface
is the point of these crates and it stays. Only the *default* row is in
question.

**welding: flipped to 30 on 2026-08-16.** Its workspace `wgpu` pin moved with
it, and has to: the demos take `wgpu.workspace = true` *and* `welding` with
default features, so a mismatch puts two majors in one demo graph. Flipping
surfaced the usual API breaks in demo code written against 29. Verified on
Windows, macOS and Linux targets, so the Metal and DMABUF arms are checked
rather than assumed; demo-weld-mac is unverified because cef-dll-sys needs a
Darwin C/C++ toolchain a Windows host cannot provide.

**grafting: deliberately still 29, and the reason is at the pin** — "wgpu-29
stays the default while the GUI ecosystem straddles 29/30 (bevy 0.19 and slint
1.17 are on 29; eframe 0.36 is on 30)." grafting is consumed one-way by
outside GUI hosts, so its default serves that ecosystem rather than this
stack. Our own consumers set `default-features = false` and forward the row
they want, which is exactly why welding was unaffected by it. Flipping this
one is a call about who the crate is for, not a migration step.

**scrying: still 29, with no stated rationale** beyond the generic row
comment. It looks like a plain leftover.

## Open question: `apply_limit_buckets`

wgpu 30's new `RequestAdapterOptions::apply_limit_buckets` maps an adapter's
limits onto pre-defined buckets. Upstream's stated purpose is to reduce
fingerprinting where wgpu is exposed to untrusted content, and the docs are
explicit that it must be set in trusted code, not left reachable by the
content it protects against.

Every site in this migration was set to `false`, preserving pre-30
behaviour, because a migration should not change behaviour silently. That is
a placeholder, not a decision. Genet is a browser: it exposes WebGL and
WebGPU to arbitrary pages, which is precisely the case the flag exists for.
The real decision belongs with the host policy layer and should weigh
bucketed limits against WebGL and WebGPU conformance expectations.

The sites are netrender_device's `boot`, genet's webgl-wgpu (two test-device
helpers, which correctly want real limits), the paint vulkan timeline
interop, genet-wpt's conformance runner, and pelt's three platform smokes.

## Progress

**2026-08-16 (step 4).** Mere is on wgpu 30, by the narrow CubeCL backport
rather than the prerelease row, for the two reasons in §4 above. One
wgpu/wgpu-core/wgpu-hal/wgpu-types/naga row at 30, vello 0.10, stable burn
0.21, one libsqlite3-sys. Quint's full suite passes on 30 including the four
resident GPU tests and the committed-SPIR-V receipt. Every step except 7 is
now done, and 7 is in progress elsewhere; the three genet inker engines land
when it does.

Known red, pre-existing and unrelated: mere's `--all-targets` fails in
mere-canvas's test on a missing `NodeMaterial::gravity_scale`, from commit
8d14c12c.

**2026-08-16 (later).** Step 5 finished, and it pulled mesocosm and paredros
in with it. crabslab, renderling, mesocosm and paredros each resolve one wgpu
row at 30; renderling's 95-test GPU suite passes single-threaded, matching
its wgpu 26 control exactly, and mesocosm's 440 tests pass. Only step 4
(CubeCL/Burn, then Mere and Quint) and the inker engines' dependency on step
7 remain. Note quint's resident lane is still wgpu 29, which is step 4's
work.

**2026-08-16.** Step 6 finished. woodshed, hocket and isometry are verified
green after clearing the three non-wgpu blockers described above; isometry
was checked with `--all-features --all-targets` per its own standing rule,
and its full all-features suite passes (297 tests, 0 failures). Every
migrated repo now resolves a single wgpu row at 30.

**2026-08-15.** Steps 1, 2 and 3 landed and pushed; step 6 landed for five
apps, verified for two. Upstream survey found that CubeCL and Burn crossed to
wgpu 30 on their own on 2026-08-10 (cubecl-wgpu 0.11.0-pre.2 requires
wgpu ^30.0.0, burn 0.22.0-pre.2 rides it), so step 4 no longer needs the
narrow CubeCL 0.10 backport; it becomes a crates.io bump to the exact
`=0.11.0-pre.2` and `=0.22.0-pre.2` row. The backport branch is worth keeping
as insurance while those are pre-releases.

Renderling and crabslab have no upstream wgpu-30 work to wait for: their
mains are still on wgpu 26 and last moved 2026-03-21, so the local dirty
wgpu-29 checkouts are ahead of upstream, and step 5 is entirely local work.

One wgpu item to watch before the Linux receipt: the v30 branch carries an
unreleased backport, "use `vk::Fence::null` on non-Windows swapchain"
([#9918](https://github.com/gfx-rs/wgpu/pull/9918)), which lands on the
Fedora and RADV lane. Either git-pin `branch = "v30"` for that receipt or
re-run it on 30.0.1. Separately, 29.0.4 shipped a GLES XCB window-handle fix
(#9271) that does not appear in the v30 changelog; confirm its status before
assuming 30.0.0 supersedes 29.0.4 on the X11 and GLES lane.
