# Meristem scope cut and the component contract question

2026-08-12. Prompted by Olivier Faure's "A Critical Review of Xilem in 2026"
(hackmd.io/@s_haMSbyTAOWfoXc1aYNUg/Hka74gCwZg), read against this tree rather
than doc-to-doc. Companion ledger entry:
`components/cambium/docs/upstream-xilem.md` (2026-08-12 scope cut).

## Where the critique lands on this stack

Faure's diagnosis of xilem: the `State` generic on every view and the
disjoint-borrow ambition behind it are the root architectural mistake; the
higher-order-component zoo (lens, map_state, fork, worker) is baroque;
side-effect views conflict with reactive principles; the project solved every
problem by adding a generic parameter until every change had a wide blast
radius. His remedy: type-erased first-class components with string-keyed
identity, imperative effect scheduling, and aggressive scope cuts.

Verified standing before this pass:

| Criticism | Status here |
| --- | --- |
| Tokio dependency | Never present. Meristem deps: tracing, anymore, optional kurbo. |
| Dual-backend trait explosion | Structurally absent; meristem serves one element domain (genet's neutral DOM seam). 16 public traits vs upstream's ~30. |
| Reactive window views | Never adopted; the winit host boots its window imperatively (single-root doctrine). |
| Side-effect views (fork, run_once, worker, task) | Present in the vendored core, zero consumers. Turnstone routes side effects through an imperative `Effect` enum processed by actors with generation counters, which is Faure's proposed replacement, built before he proposed it. |
| State/Action generic plumbing | Fully retained and costing real adoption. See below. |
| HOC zoo | lens, map_state, map_action, memoize live and exported; leaf-scale use is fine, composition-scale use hurts. |

The organizational half of the critique (identity oscillation, complexity
addiction) does not transfer: cambium's job is settled. The blast-radius
arithmetic does transfer: cambium is at 78 `View` impls and growing. Any
surgery on the `View` signature gets more expensive per month deferred.

## The scope cut (done in this pass)

Zero-consumer surfaces deleted from meristem, verified by symbol sweep across
cambium, sprigging, both hosts, turnstone, woodshed, mere, hocket, isometry,
and retinue, then by compile and test:

- `views/fork.rs` (117 lines), `views/run_once.rs` (126),
  `view_sequences/without_elements.rs` (137): the side-effect-view lane
- `views/one_of.rs` (667) with `OneOfCtx` and `PhantomElementCtx`, plus its
  integration test (304)
- `views/memoize.rs` (284): `Memoize` and `Frozen`, both zero-consumer HOCs
- `message_proxy.rs` (139): `MessageProxy`, `RawProxy`, `ProxyError`
- `environment.rs` (740): `Environment`, `Provides`, `WithContext`,
  `Resource`; with it the `ViewPathTracker::environment()` hook (upstream's
  own comment: "here on a temporary basis"), the `Environment` argument of
  `MessageCtx::new`/`finish`, `GenetCtx`'s field, and the take/restore dance
  in every runner dispatch path (G2.2)
- `hashbrown` from meristem's manifest; the Environment slot map was its only
  consumer
- `docs.rs` (116 at the recorded base): `DocsView`, `DocsViewSequence`, `Fake`,
  and the replacement `Nothing` view. Its only external reference was the lens
  doctest, which is now explicitly illustrative instead of maintaining a public
  fake backend.

Meristem's scoped diff is 2,682 net lines smaller. Public traits fell from 16
to 10. Receipts: `cargo check -p meristem -p cambium -p sprigging
-p cambium-genet-winit-host -p cambium-nematic -p cambium-winit` passes with
pre-existing workspace warnings; the meristem suite passes and its five ignored
doctests compile; cambium's 176 tests pass, including the component proof and
the keyed and portable-move update-policy gates; the component-catalog example's
two acceptance tests pass with all features enabled.

Kept deliberately:

- `Count`: part of the `ViewSequence` contract here, unlike Faure's xilem
  candidate.
- `meristem` is versioned 0.2.0 in the workspace for the removed public API;
  crates.io publication remains a separate release operation.

## The lesson that still lands: the composition boundary

`View<State, Action, Context>` is intact, and the cost shows up exactly where
Faure says it does. The receipt is `turnstone/src/chrome_view.rs`: adoption of
cambium's own `caret_text_field` is documented as rejected because `lens`
shares the `Action` type, so a `()`-typed field would need an unreachable
`map_action` arm to sit in the `ChromeIntent` tree. The flagship consumer
declining the flagship control over type plumbing is the disease presenting.
Secondary friction: boxed lenses in settings and publish panes, ~52 `+ use<>`
bounds.

The evidence also bounds the lesson. Turnstone uses `lens` contentedly in half
a dozen leaf sites; the tower only hurts at component boundaries. So the
sized-to-this-stack conclusion is a three-level doctrine:

1. leaves: `lens`/`map_action` wiring is fine;
2. component boundary: erased contract, control owns its state, typed events
   out;
3. app: single-owner state plus intent enums plus an effect queue (the
   turnstone shape).

## Implemented feature: Cambium component boundary

A `component` boundary now lets a control own its state and emit typed events,
without threading the parent's state and action vocabulary through the child
tree.

Context. This is the piece of Faure's Component proposal worth taking. The
rest solves problems this stack has less of: string-keyed identity duplicates
what the hoist/extract/adopt patch set already provides at the element level,
and full type erasure trades compile-time errors for runtime key discipline
across the whole tree instead of at one seam.

The public construction shape is:

```rust
component(
    props,
    init_local,
    reconcile_local,
    body,       // (&Props, &Local) -> ComponentView<Local, Event>
    on_event,   // (&mut State, Event) -> (), Action, or Option<Action>
)
.probe_id("caller-owned-id")
```

`ComponentState` retains `Local`, the previous erased child view, and that
child's view state. `AnyView` erases the concrete child tree and view-state
representation; it does not erase `Local` or `Event`. `DynMessage` remains the
inbound raw-message envelope. The child returns `MessageResult<Event>` and the
boundary maps that result through Cambium's `OptionalAction`: a returned action
becomes `MessageResult<Action>`; `()` or `None` becomes `MessageResult::Nop`.

Settled design points:

- Rebuild semantics: `reconcile` receives previous and current props, then
  `body` re-runs. `Local` persists across rebuilds and drops with the retained
  component state.
- Identity: the boundary participates in keyed/portable moves like any view;
  `keyed` and Meristem routing remain authoritative. Probe identity is the
  separate, optional `data-cambium-component` attribute supplied by the caller.
- Events vs actions: `Event` is the child's public vocabulary; `on_event`
  translating or swallowing it is the only place the child event and parent
  action types meet.

Validation receipts:

- The command-picker proof moves local selection, reconciles a changed parent
  label without resetting it, emits a typed activation into parent state and
  action, swallows Dismiss without minting a no-op action, and exposes the
  caller-owned probe id on the root.
- The catalog counter specimen (2026-08-12) is the boundary's second consumer
  and its acceptance-surface receipt: component-owned count survives a
  parent-controlled step change, a typed Report event lowers into catalog
  state, unmount drops the local state, remount reinitializes it, and the
  probe id is asserted at both specimen widths. The probe stamp is now
  idempotent on rebuild, so a probed component no longer records a spurious
  attribute mutation per parent rebuild. A direct mutation-stream test proves
  zero writes for an unchanged stamp and exactly one for change and removal.
- Turnstone disproved `caret_text_field` as the consumer: its omnibar is a
  controlled app-truth mirror whose keys must remain on the Action spine.
  Cambium instead exposes `caret_field_children`, the non-editing projection
  beneath the editable field; Turnstone consumes it without acquiring a second
  text authority.
- Compile-time was measured twice and improved neither time: wall-clock
  against the turnstone settings pane, then `cargo llvm-lines` against the
  graph canvas, which showed the boundary relocates monomorphization rather
  than reducing it. No compile-time claim is made for the boundary; see the
  settings-pane section for both measurements.

## Memo boundary (landed 2026-08-12)

`Component::memo()` (requires `Props: PartialEq`) skips `reconcile`, `body`,
and the child rebuild when props are unchanged and no message has touched the
subtree since the last rebuild. A delivered message marks the component's
local state dirty, so an interaction always renders even under equal props.
This recovers what the deleted `Memoize` HOC did, as a property of the
boundary rather than a freestanding view, at the layer where the equality
comparison is meaningful. The equality check is a stored fn pointer, so
un-memoized components keep working with non-comparable props. Receipt: the
`memo_skips_body_on_equal_props_until_a_message_dirties_local_state` unit
test counts body runs across a clean rebuild, a props change, and a dispatch.

## setting_row (landed 2026-08-12)

`cambium::setting_row` renders one `genet-host-api` `SettingSpec` as a
labelled control with an Apply button. The draft is component-local; the only
value crossing the boundary is the applied `SettingValue`. An externally
changed committed value or control re-derives the draft (the
parent-controlled axis); local edits never reset it. Rows are memoized on
`(spec, label)`. The draft controls keep their `Action = ()` signatures
inside a `map_message_result`-silenced cluster; the Apply button is the row's
only emitter. The catalog's settings-form specimen now renders through
`setting_row` from four inline provider-shaped specs, and its headless
contract proves draft-edit-then-apply: toggling the control applies nothing,
Apply carries `Boolean(false)` from the edited draft into catalog state.

## Uncontrolled variants: no wrapper

A generic `uncontrolled(control)` wrapper family is rejected. The
interaction/value split is per-control knowledge (a select's open state is
interaction; a disclosure's expansion IS its value), so a generic wrapper
cannot place it and would double the API surface into React's documented
controlled/uncontrolled confusion. The alternative is what `setting_row`
demonstrates: when a consumer pulls, that control gains a component-shaped
signature (value props in, typed events out, interaction local) built on
`component` internally, one control at a time. No wrapper vocabulary is ever
public; the split is decided once, correctly, by the control author.

## Turnstone settings pane: adopted, and the compile-time claim retracted

`settings_pane.rs` now renders through `cambium::setting_row`. The four
draft HashMaps (`text_inputs`, `number_inputs`, `toggles`, `choices`) and the
four boxed lenses that initialized them inside accessor closures are gone,
along with the per-control apply lane (`apply_text`/`apply_number`/
`apply_toggle`/`apply_choice`) and the draft-derivation helpers, which now
live once in Cambium. `SettingsState` drops from 9 fields to 5; the file
loses 241 lines and gains 25. The pane keeps `apply_value`, the provider,
and the live-settings publish path: `on_apply` forwards the applied
`SettingValue` under the id the row was given.

DOM compatibility receipt: all 9 turnstone settings tests pass unchanged,
including `pane_renders_controls_from_setting_control_not_setting_ids`,
which asserts 5 rows, 5 labels, 5 applies, 1 slider track, 1 toggle, and 7
radios. The component-shaped row reproduces the hand-rolled row's structure
exactly.

**Monomorphization measurement (2026-08-13, `cargo llvm-lines`): the
compile-time case is dead, and now for a reason rather than a null result.**

The wall-clock experiment below was inconclusive because a per-crate rebuild
swamps one pane. `cargo llvm-lines` measures the thing directly, and it says
the component boundary does not reduce monomorphization at all — it *relocates*
it.

The clean A/B is inside `woodshed-views`, which today uses both forms of the
same control: the Set graph through the component `graph_canvas`, and Related
through the callback-per-axis `graph_canvas_swatch`. The swatch entry point is
instantiated 11 times in each form and costs almost exactly the same:

| Form | Instantiated at | Copies | LLVM lines |
| --- | --- | --- | --- |
| callback (`graph_canvas_swatch`) | `UiState` (the app's state) | 11 | 1,670 |
| component (`graph_canvas`) | `GraphCanvasLocal<CardId>` | 11 | 1,636 |

Two percent apart, which is nothing. The boundary moves the instantiation off
the app's state type and onto a Cambium-owned local, and the hoped-for saving
was that consumers would then *share* that one copy. They do not, for two
independent reasons: the local is still `GraphCanvasLocal<Id>` with the
application's own id type (`CardId` here, `Uuid` in Turnstone), and the
applications are separate binaries that never share a monomorphization
anyway. Sharing would require the same binary *and* the same `Id`.

For scale, the catalog binary (434,501 lines, 13,855 copies) puts **61% of all
instantiation at `CatalogState`**, and the graph-swatch family alone accounts
for 109,003 lines with 96% of that at `CatalogState`. That is where a
consumer's compile time actually goes: its own view tree, monomorphized over
its own state type. No component boundary touches it. The only lever that
would is type erasure through `Box<dyn AnyView>` — which Cambium already
offers, and which trades compile time for dynamic dispatch at every erased
node.

So: adopt the boundary for state ownership and for keeping app structs free of
other people's interaction state. Do not adopt it expecting faster builds.
That question is now closed with numbers.

A second finding from the same pass, worth more than the measurement:
`related.rs` cannot adopt the component form, because its `related_hover` is
shared with the neighbor list — the graph and the rows cross-highlight each
other. That is exactly the "an application that genuinely wants to own
emphasis" case the callback family was kept for, and it is now a live example
rather than a hypothetical.

**Wall-clock measurement: no improvement, claim retracted.** Protocol:
touch `settings_pane.rs`, time a warm `cargo build`, three runs each side.
Baseline 71.8 / 73.2 / 72.6 (mean 72.5s); adopted 69.2 / 73.8 / 72.6 (mean
71.9s). The 0.6s difference is inside the run-to-run spread and is not a
result. The instrument is wrong for the question: Rust rebuilds per crate,
so touching one file recompiles all of turnstone's lib, and one pane's
generic instantiations are swamped by the rest. Measuring the type-plumbing
cost honestly would need a finer instrument (monomorphization counts via
`cargo llvm-lines`, or `-Zself-profile` on a crate containing only the
pane). Until someone runs that, the boundary's case rests on state
ownership and deleted plumbing, not on build times. The brief's earlier
"compile-time delta measured before and after" validation item is retired
as measured-and-negative rather than pending.

## Graph canvas (landed 2026-08-12)

`cambium::graph_canvas` is the second component-shaped control and the first
with two consumers. Selection arrives as parent truth in the props (a node is
selected because the graph says so); hover, keyboard-focus emphasis, and
relation hover live in the component. The parent receives only
`GraphCanvasEvent` (Activate, Expand, Drag, RelationActivate).

Enabling change: the activating handlers on the `graph_canvas_swatch*` family
now return `OptionalAction` rather than unit. This is backward compatible,
because `()` already implements `OptionalAction<A>` for every `A`, so all
existing callback-per-axis consumers compile untouched. Hover and focus
handlers deliberately did **not** widen: emphasis is presentation state and
must never be an app-facing event. The callback family remains for an
application that genuinely wants to own emphasis (mirroring it across two
views); the component is the default.

Adopters, both of which were storing emphasis purely to route it back into
the view on the next rebuild:

- Turnstone's swatch pane: `state.swatch.hovered = id` is gone. Its hover
  test now reads the emphasis from the rendered node's class, where it is
  now observable, rather than asserting an app-state field that should not
  exist.
- Woodshed's Set graph: `UiState::set_graph_hover` and `set_graph_focus` are
  gone (two fields off a struct that carries the whole app's UI state). Its
  probe scenario's `graph-focus` field now reads the focused node's
  `data-key` from the DOM; left on the UiState field it would have reported
  "none" forever, which is exactly the quietly-lying diagnostic the scenario
  exists to prevent.

## Effect contract: the gate opened, and the answer is still no crate

**Update, later on 2026-08-12.** Woodshed's nine request flags were refactored
into a typed queue (`woodshed_core::audio::AudioRequest` +
`UiState::audio_requests`, drained in one place in `sync.rs`). The refactor
stands on its own merits, independent of any shared contract: a boolean per
request coalesced two presses in a frame into one action (and
`preview_note_requested: Option<f32>` silently dropped the earlier note),
lost the order the user pressed things in, cleared inconsistently
(`= false` at two sites, `mem::take` at five), and left no single list of
what the app can ask its host to do. Two tests now hold the properties a
flag could not provide: repeated requests all survive in order, and one
drain site owns consumption. `midi.refresh_requested` deliberately stayed a
flag — a port rescan is idempotent, so coalescing is correct there, which is
exactly what is not true of two Preview presses.

So the two applications' effect shapes now genuinely converge: a typed enum
of commands, pushed in order onto a `Vec`, drained by one processor.

**And that is precisely why no `effects` module should be lifted.** Ask what
code a shared contract would contain, and the answer is `Vec::push` and
`std::mem::take`. Everything with substance is app-specific: the effect types
(`Effect` vs `AudioRequest`), the processors (generation-counted actors vs a
backend match), and the correlation discipline (turnstone only, because only
its answers arrive late and can be stale). A trait wrapping a push and a take
would add ceremony without removing duplication, which is the complexity
addiction this brief opened by diagnosing.

What is worth sharing is the doctrine, and it now has two receipts:

1. Continuous state is realized idempotently from app state every frame;
   one-shot commands are queued and drained exactly once.
2. Queue when repeats are meaningful (two previews must sound twice); a flag
   is correct only when the request is idempotent (rescan the MIDI ports).
3. Correlation identity belongs in the effect payload, not the queue, and
   only when answers can arrive after the requester moved on.

Revisit lifting code only if a third consumer needs turnstone's *correlation*
machinery — generation-counted staleness is the one non-trivial piece here,
and it is still single-consumer.

### Applying the doctrine without a trait: a stack-wide audit

Doctrine with no crate behind it is only worth something if it is actually
applied, so the whole stack was swept for the flag-shaped smell (`*_requested:
bool`, `*_requested: Option<T>`, one-shot `pending_*` booleans) across genet's
components and ports, cambium, turnstone, mere, woodshed, isometry, hocket,
and mesocosm.

It found exactly **one** more instance, which is the useful result: the rule
has teeth in both directions rather than licensing a sweep.

Converted — `turnstone/src/knot_authoring.rs`: four toolbar flags
(`save`/`resolve`/`run`/`reload`) became one `Vec<AuthoringRequest>` drained
once. This instance was worse than woodshed's, because each flag was cleared
through `runner.update`, and in Cambium that rebuilds the whole view tree; a
click that set several verbs paid a rebuild per verb purely to reset booleans.
The queue takes them in one `update`, so one rebuild, in press order.

Deliberately left as flags, each for a reason the doctrine names:

- `GenetCtx::focus_request: Option<NodeId>` (and the `focus_request` /
  `focus_requested` fields in `radio.rs` and `disclosure.rs`): single-valued
  by nature — only one node can hold focus — and consumed once per dispatch
  through `take()`. "Last request wins" is the correct semantics, not a lost
  repeat.
- `cambium-genet-winit-host`'s `close_requested`: closing twice is closing
  once. Idempotent, so coalescing is correct.
- `woodshed`'s `midi.refresh_requested`: a port rescan is idempotent, same
  reason.
- `mere`'s `CommandPaletteViewModel::toggle_requested`: one flag behind an
  authority type with an explicit `clear()`, not a sprawl with inconsistent
  clearing. Converting it would be churn in another session's live area for
  no property gained.

The test for "is this flag wrong" is therefore not "is it a bool" but: **can
two of these arrive before the drain, and would losing one or reordering them
be a bug?** Nine audio commands and four toolbar verbs failed that test. A
focus request and a window close pass it.

## Effect contract: the earlier gate check (superseded by the update above)

Woodshed was the second consumer that would arm the shared effect contract.
It does not: the two applications solved effects differently, and for
defensible reasons.

- Turnstone: a typed `Effect` enum pushed onto a queue, processed by actors,
  with generation counters for staleness. Its effects are **correlated async
  requests** whose answers can arrive after the requester moved on (fetch a
  page for a node that has since navigated away).
- Woodshed: nine one-shot `bool` / `Option<f32>` request flags on `UiState`
  (`preview_requested`, `calib_start_requested`, `song_record_toggle_requested`,
  …), set by the view and consumed by the host after dispatch. Its effects are
  **fire-and-forget host commands** with no answer to correlate.

A shared contract would have to span both, and neither app is currently
paying for the other's problem. So the gate stays shut and no `effects`
module is lifted. The signal to revisit is woodshed growing a correlated
async effect (an answer that can arrive stale), not the flag count rising.
Woodshed's nine flags may still be worth a typed queue for its own sake;
that is woodshed's refactor to make, not a Cambium contract.

## Layer ownership

- **Meristem:** retained diff/message primitives only. No component, probe, or
  effect policy was added to the core.
- **Cambium:** owns the state-owning component boundary and reusable field
  projection because both are GUI composition behavior over Genet elements.
- **Genet:** unchanged. It supplies the neutral DOM mutation substrate used by
  the probe attribute and the existing selector resolver, not application
  component semantics.
- **Turnstone:** keeps its `Action`, `Effect`, omnibar truth, key lowering, and
  actor generation policy. Only the reusable caret projection moved out.
- **Mere:** receives nothing from this pass. Lift an orchestration/effect
  contract only when a second application duplicates Turnstone's shape; put it
  in Mere only if the shared vocabulary is specifically graph-browser host
  orchestration.

## Effect contract: original reasoning (superseded by the gate finding above)

Turnstone's `Effect` enum (fetch, persist, session ops processed by actors
with generation counters) is the validated in-house shape, and
`GenetAppRunner` already returns `Vec<Action>` outward, which proves the event
seam but not an effect queue.
The consumer-pull gate says: write the pattern down as doctrine (component
catalog growth plan is the home) and lift a shared contract only when a
second app (woodshed or signalman) actually duplicates turnstone's shape.
Candidate owning layer at that point: a cambium `effects` module or the
Mere host/application orchestration seam, depending on whether the duplicated
contract is GUI-general or graph-browser-specific. Never meristem.

Window doctrine stays as-is: imperative host boot now, and when multi-window
lands (one app state, N windows as lenses), window lifecycle remains
imperative host effects, never reactive window views.

## Fork posture watch

linebender/xilem as of 2026-08-12 is active but incremental (widget fixes,
a11y, VirtualScroll migrated onto understory_virtual_list; no component
rewrite in flight). Faure's document is a personal roadmap, not a committed
direction. If upstream does take the turn, the `xilem_core` this fork tracks
ceases to exist upstream and the reconcile-against-releases policy in
`components/cambium/docs/upstream-xilem.md` becomes dead letter; the posture
then shifts from tracked fork to owned core. Decide that deliberately when
the signal appears, not by default. This pass already widens the divergence
on purpose; the ledger records it.
