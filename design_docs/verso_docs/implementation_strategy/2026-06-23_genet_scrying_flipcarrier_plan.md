# Genet ↔ Scrying FlipCarrier — first-flip plan

> **Consolidation note, 2026-09-02.** The crates this document names —
> `verso`, `verso-api`, `verso-scry`, `verso-genet` — were consolidated into
> the single `components/verso-tile` crate on 2026-07-09 (its `api`, `flip`
> and `scry` modules, plus the `genet-donor` feature). The paths below are
> as of writing; the design they record is unchanged.
**Date**: 2026-06-23
**Status**: Design resolved; `verso-api` + genet donor primitives shipped (2026-06-23).
Both charter prerequisites are **done** (verified in code 2026-06-23): P4 (the scry
tile) and the inker picker (the engine-picker plan's Phases 0-3 — `engine_pins`
routing through `EngineRoutePolicy`, `is_surface_engine`, the apparatus engine
manager, and the per-node picker) both shipped 2026-06-15. **Verso is unblocked.**
The picker already flips a node to `scrying.web` as a *stateless* engine-switch (a
fresh WebView); verso is the state-carry layer that turns that switch into a flip.
Next: the carrier + the `verso-genet`/`verso-scry` adapters, hooking the existing
`engine_pins` pin-switch in the ScryingHost.
**Extends**: [compatibility-view charter](../technical_architecture/2026-06-10_compatibility_view_charter.md)
(§3 the charter, §7.3 "mint verso at the first flip").

The first verso flip: a genet-rendered page re-presented through the scrying
system WebView, place and session carried across, the tile keeping its identity.
This is the consumer-pull moment that mints the verso crates (charter §7.3). One
engine pair, both directions.

---

## 1. Findings — capability verification (2026-06-23)

### Scrying (black-box receiver): ready, no new engine work

- **Navigation lifecycle**: `NavigationEvent::{NavigationStarted,
  NavigationFinished { success }, DownloadStarted/Progress/Finished, TextInput*}`
  via `poll_navigation_event`, plus blocking navigate variants. `NavigationFinished`
  is the load-complete hook for post-load inject.
- **Inject toolkit**: `navigate_to_url`, `navigate_to_string` (raw HTML),
  `set_cookie` (+ `set_cookie_change_handler`), `execute_script_with_result`.
- **Session control**: `non_persistent()`, `with_header()`.
- **Downloads already exist** (`with_download_dir`, the Download* events). Out of
  scope for the flip; it means the broader scry capability layer is host-side
  consumption, not new engine work.
- **The receiver is multi-frame, not a sync one-shot** (verified 2026-06-23): the
  `WebSurfaceProducer` trait's `navigate` blocks (timeout-bounded), so the flip must
  use the non-blocking concrete path — `set_cookie` + `load_url`, then the host frame
  loop polls `poll_navigation_event` for `NavigationFinished { success }`, then
  `execute_script_with_result` restores scroll/forms. Consequence: `FlipReceiver` is
  two-phase (begin → restore-on-load), and `verso-scry`'s impl couples to the host
  frame loop and the concrete (WebView2) producer (`inner_mut`). It is **not** a
  standalone crate buildable in isolation — it lands with the carrier + host wiring.
  That host wiring is no longer gated: the inker picker shipped 2026-06-15, so the
  genet→`scrying.web` pin-switch already exists; `verso-scry` lands over it whenever
  the carrier does. Flip-back's reads are likewise host-tracked: URL
  from nav-event tracking, scroll via `execute_script`, cookies need a read path
  (`set_cookie` is write-only today).

### Genet (glass-box donor): state reachable, export API missing

- **URL**: `rt.base_url()` ✓.
- **Scroll**: `ScriptedDocument.scroll` (owned, private) — needs a public accessor.
- **DOM serialization**: not on `genet-scripted-dom`. Build a serializer (outerHTML).
- **Form values**: not present. Build DOM traversal collecting field values.

Net: receiver is done; the donor needs three small, verso-unaware export accessors.

## 2. Crate layering (resolved)

The dependency points one way so the heavy engines never stack on each other:

- **`verso-api`** — traits + `PortableViewState` / `BackState` / `LayerSet`. Tiny,
  engine-agnostic.
- **`verso-genet` / `verso-scry` / `verso-weld` / `verso-graft`** — per-engine
  adapters. Each depends on its engine crate + `verso-api` and does the bridging;
  the engine crates (`genet-scripted-dom`, `scrying-engine`) stay verso-unaware.
- **`verso`** — the orchestrator (carrier + flip choreography + registry). Depends
  on `verso-api` only, never on an engine, so it never drags Servo or CEF in.
- **host** (meerkat/pelt) — pulls `verso` + whichever `verso-*` adapters its build
  features enable, and wires them in. `meerkat` = verso-genet + verso-scry;
  `meerkat-graft` adds verso-graft. The adapter crates are the per-engine feature
  seam, so a heavy engine only lands in the variant that asked for it.

## 3. Traits (in `verso-api`; illustrative-signature-only)

No-chain (charter §4) is encoded in which traits an engine implements:

```rust
trait FlipDonor    { fn donates(&self) -> LayerSet; fn capture(&self) -> PortableViewState; } // primaries: genet, nematic
trait FlipBack     { fn extract(&self) -> BackState; }                                         // secondaries: scry, weld, graft
trait FlipReceiver { fn receives(&self) -> LayerSet; fn present(&mut self, carry: Carry); }    // all engines
enum  Carry        { Forward(PortableViewState), Back(BackState) }
```

Primaries impl `FlipDonor` (full live capture). Secondaries impl `FlipBack` (lean
extract) and never `FlipDonor`, so there is no type path to forward-donate a
document to anyone. A primary's `FlipReceiver` consumes a `Back` by re-fetching; a
secondary's consumes a `Forward` by navigating. The registry only wires
(primary→secondary) and (secondary→primary). A leaf has two faces.

## 4. Forward flip: genet → scrying

### PortableViewState (layered; each layer degrades, never blocks)

1. navigation: URL, history cursor, scroll
2. form: field values keyed by a stable selector
3. session: cookies for the origin (+ storage scope)
4. DOM snapshot: serialized outerHTML
5. visual snapshot: donor's last frame as a texture (cross-fade, no flash)

### Layer mapping: capture (genet) → inject (scry) → fidelity

| Layer | Capture from genet | Inject into scry | Fidelity |
| --- | --- | --- | --- |
| navigation | `base_url`, history cursor, `ScriptedDocument.scroll` | `navigate_to_url`, then `execute_script("scrollTo")` post-load | URL faithful; scroll best-effort |
| form | walk scripted-DOM, collect values | `execute_script` to refill post-load | best-effort |
| session | origin cookies from netfetcher/eidetic | `set_cookie(...)` once, **before** navigation | one-shot |
| DOM snapshot | serialize outerHTML | `navigate_to_string(html)` — **degrade path** | static, loses live JS |
| visual snapshot | last genet frame (the snapshot-card texture) | hold as receiver's first frame | cosmetic cross-fade |

### Inject choreography (order matters)

1. Freeze the visual snapshot (the snapshot-card texture already is this).
2. Boot the scrying receiver actor (the P4 constellation actor).
3. `set_cookie` for each exported cookie — **before** navigating.
4. `navigate_to_url(url)` — faithful path (WebView re-fetches, runs real JS).
   `navigate_to_string(dom)` only when there is no refetchable URL.
5. On `NavigationFinished { success: true }`: `execute_script` to restore scroll
   and refill forms.
6. Swap the tile's backing texture genet→scrying when the first **post-nav**
   captured frame arrives (cross-fade out of the held card frame).
7. Park or retire genet. Record the flip as a node-lineage event (provenance).

## 5. Flip-back: scrying → genet (re-root, not reverse capture)

A black-box WebView can only surface a *dead* snapshot (post-JS outerHTML, no live
document, no JS heap). So flip-back re-roots at the lossless source rather than
transferring a DOM:

1. scry extracts the cheap locator (`BackState`): current URL, scroll, form?,
   cookies — via the WebView API + `execute_script`.
2. genet re-fetches the URL from the lossless root (netfetcher live, or eidetic if
   cached) and renders it *live* with its own engine. Fresh JS run, real document.
3. Apply the carried nav state: scroll, best-effort form refill.
4. Cookies flow back into the netfetcher/eidetic cookie world (one-shot reverse, so
   a login made *inside* scry comes home).
5. Swap the tile texture scry→genet, cross-fade, retire the scry actor.

Result: same page, same place, same session — never the same running program (the
JS heap cannot cross). That is the charter ceiling, stated honestly.

### `BackState` — the lean locator a black-box can give (illustrative)

```rust
struct BackState {
    url: String,              // current; may differ from flip-out if the user navigated in scry
    scroll: (f32, f32),       // via execute_script
    form: Option<FormValues>, // via execute_script, best-effort
    cookies: Vec<Cookie>,     // via the WebView cookie API
}
```

### Primary re-fetch receiver path

genet's `FlipReceiver`, on `Carry::Back(b)`: fetch `b.url` (netfetcher/eidetic) →
build a fresh `ScriptedDocument` → on its first frame apply `b.scroll` and refill
`b.form` → push `b.cookies` into the netfetcher/eidetic jar. No DOM injection;
genet re-renders from source.

### Why this is the no-chain mechanism

Re-rooting requires rendering source bytes live, which is a *primary's* capability
(engines are byte-consuming and never own networking, so source is always
reachable). Secondary→secondary has no clean re-root: a dead-snapshot shuttle that
compounds loss with ambiguous session authority. So the only sane hops are
primary→secondary (compat view) and secondary→primary (flip-back). One hop.

## 6. Build phases

1. **Genet export accessors** — `url()`/`scroll()` public on `ScriptedDocument`;
   a DOM serializer + form-value extraction on `genet-scripted-dom`. Verso-unaware.
2. **`verso-api`** — the traits + `PortableViewState`/`BackState`/`LayerSet`.
3. **`verso-genet` + `verso-scry`** — the `FlipDonor`/`FlipBack`/`FlipReceiver`
   impls over the plain engine APIs.
4. **`verso`** — the `GenetToScrying` carrier + flip choreography + registry. Lean,
   well under the 600-LOC ceiling (the engines do the heavy lifting).
5. **Host wiring** — meerkat/pelt build the carrier from the variant's adapters; the
   tile texture cross-fade. Both prerequisites shipped 2026-06-15: P4 (the scry tile)
   and the inker picker, which folded the old ad-hoc `compat_pins` path into routing
   (`engine_pins` through `EngineRoutePolicy`). So the flip *trigger* already exists —
   pinning a node to `scrying.web` flips it *statelessly* today; this phase intercepts
   that `engine_pins` genet→`scrying.web` transition to capture the donor and inject
   into the receiver, turning the switch into a flip.
6. **Flip-back** lands alongside forward (same carrier, the `Back` direction).

## 7. Inherited invariants (charter)

One hop (no secondary→secondary). Asymmetric fidelity is the engines' nature, not
policy. Session is one-shot at flip time (continuous mirror is a tarpit). Ceiling:
same page, same session, same place — never the same running program.

## 8. Progress

- **2026-06-23**: verified scry receiver (ready) and genet donor (three export
  gaps). Resolved all four open questions — genet export as plain verso-unaware
  methods on `genet-scripted-dom`; traits standalone in `verso-api`; crate layering
  as `verso-api` + per-engine `verso-*` adapters + a `verso` orchestrator
  (host-wired, feature-gated); texture swap rides the capture cadence after
  `NavigationFinished`. Designed the flip-back re-root path + `BackState`.
- **2026-06-23 (impl)**: corrected the status — *both* charter prerequisites shipped
  2026-06-15 (P4's X1+ scry tile, and the inker picker's `engine_pins` routing), so
  the live flip is unblocked, not picker-gated. Minted `crates/verso-api`
  — `PortableViewState`/`BackState`/`LayerSet` + `FlipDonor`/`FlipBack`/`FlipReceiver`,
  engine-agnostic with zero deps; `cargo test -p verso-api` green.
- **2026-06-23 (phase 1)**: genet-side donor DOM extraction landed and tested —
  `ScriptedDom::outer_html`/`inner_html` via html5ever's serializer (no hand-rolled
  escaping; genet-scripted-dom `fdac70f2b10`) and `form_values` keyed by name/id
  (`3a35b5cc4aa`). The remaining phase-1 url/scroll are host-side and belong with the
  `verso-genet` adapter.
- **2026-06-23 (phase 3, donor)**: minted `crates/verso-genet` — `GenetDonor`, the
  genet `FlipDonor`. Fills the FORM + DOM layers itself from the scripted DOM
  (`form_values` / `outer_html` over `LayoutDom::document`) and takes the NAV
  (url+scroll), SESSION (cookies), and VISUAL (frame) layers host-fed via `with_*`
  setters — those live in the script runtime / netfetcher / compositor, not the DOM,
  so the crate depends only on `genet-scripted-dom` + `verso-api` (no runtime, no GPU
  layer). `donates()` advertises FORM|DOM always plus the fed host layers. 3 tests
  green.
- **2026-06-23 (phase 4, carrier)**: minted `crates/verso` — the engine-agnostic
  orchestrator (`verso-api` only, no engine). `flip_forward(donor, receiver)` masks
  the captured state to `donates() ∩ receives()` and presents a `Carry::Forward`;
  `flip_back(source, primary)` masks the `BackState` to the primary's appetite and
  presents a `Carry::Back`, declining (returns `false`) when the primary can't
  re-root (no NAV). `forward_carried` previews the crossing layers for the host.
  Degrade-never-block and one-hop are both encoded in the signatures (forward takes a
  `FlipDonor`, back takes a `FlipBack`); 4 tests green. The non-host-coupled half of
  verso is now done (`verso-api` + `verso-genet` + `verso`). The single remaining
  chunk is host-coupled: `verso-scry` (FlipReceiver/FlipBack over the WebView2 producer
  and the host frame loop) and the meerkat `ScryingHost` hook that fires the carrier on
  the `engine_pins` genet→`scrying.web` transition (§1, §6.5).
- **2026-06-23 (phase 3+5, scry receiver + host wiring)**: the forward flip is live.
  Minted `crates/verso-scry` — `ScryForward`, a two-phase forward-inject state machine
  (set cookies → navigate → restore scroll/forms on `Completed`) over a thin
  `ScrySurface` seam the host implements. **Refinement of §2**: rather than depend on
  the heavy Windows-only `scrying` crate, `verso-scry` stays `verso-api`-only and the
  host bridges its concrete producer to `ScrySurface`. That keeps it light, portable,
  and *unit-testable* — closing the §1 "not testable in isolation" gap (6 tests green
  with a mock surface). Added a const `LayerSet::union` to `verso-api` for the
  adapter's `RECEIVES`. Host wiring in `meerkat::scrying_host`: a `ProducerSurface`
  bridge (maps `verso-api::Cookie` → `scrying::Cookie`, runs the restore via
  `execute_script_with_result`), a `pending_flips` map + per-tile `flip`, and the
  `drive` loop now runs the flip's cookies-then-navigate on spawn (skipping the blank
  load) and pumps `NavigationEvent::Completed` into the restore. The trigger is
  `node_ops::toggle_focus_compat`: on the genet→`scrying.web` pin it stages a
  `PortableViewState` (URL + scroll, host-reachable). **v1 carries URL + scroll** (same
  page, same place — better than the stateless switch that loads blank at the top);
  SESSION (cookies) and FORM degrade until a host-side cookie jar and a synchronous
  genet-DOM read are wired (the donor's deeper layers). `cargo check -p meerkat`
  green. Remaining: cookie-jar wiring (the high-value SESSION layer), the full
  `verso-genet` donor capture (DOM/forms, needs the off-thread genet document), the
  visual cross-fade, and flip-back (§5).
- **2026-06-23 (SESSION layer)**: the flip now carries the login, not just the place.
  Root cause of the "no host-side cookie jar" gap: meerkat built a throwaway
  `FetchContext` per fetch, so no session ever persisted. Fixed with a process-wide
  shared jar (`fetch::session_jar`) injected into every fetch, and the trigger reads
  `fetch::session_cookies_for(url)` into `PortableViewState.cookies`. Upgraded
  `verso-api::Cookie` to the RFC 6265bis shape (`same_site` / `expires` / `partitioned`)
  so the carry is lossless. Full model + the durable/partitioned/script-integration
  follow-ons in the
  native session store plan (`mere/design_docs/mere_docs/implementation_strategy/2026-06-23_native_session_store_plan.md`).
  **v1 now carries URL + scroll + SESSION**; FORM still degrades.
