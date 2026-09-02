# Verso — the compatibility-view charter

> **Consolidation note, 2026-09-02.** The crates this document names —
> `verso`, `verso-api`, `verso-scry`, `verso-genet` — were consolidated into
> the single `components/verso-tile` crate on 2026-07-09 (its `api`, `flip`
> and `scry` modules, plus the `genet-donor` feature). The paths below are
> as of writing; the design they record is unchanged.
**Date**: 2026-06-10
**Status**: Charter decision (Mark, 2026-06-10 conversation). Pre-implementation;
nothing consumes this yet, and nothing should until the sequencing gate below.
**Supersedes**: verso's original realization charter (composition spine §7/§14.3),
which was decomposed and absorbed by shipped layers. See the spine's 2026-06-10
banner for the absorption record:
composition spine (`mere/design_docs/mere_docs/technical_architecture/2026-05-21_mere_composition_spine.md`).

---

## 1. The reframe

Recto/verso is the front and back of one printed leaf: same leaf, two faces.
Verso is reborn as exactly that: **the same page, re-presented through another
engine, state carried across**. The compatibility view (a site genet cannot
yet render faithfully, flipped to the system WebView with the user's place and
session intact) is the first face of it; the general capability is moving live
content from a primary engine to a compatible secondary one.

A leaf has no third side. That is the no-chain invariant (§4), built into the
name rather than bolted on.

In-product vocabulary stays plain: the UI says "compatibility view" / "flip" /
`open in <engine>`. `verso` is the crate/seam name only.

## 2. Ownership split (what verso is NOT)

Three concerns, three owners. Verso takes only the third:

- **Picking engines** is `inker` (routing by pin / type / scheme / override).
  A per-node engine picker is an inker affordance and fits the
  configurability-over-defaults rule. Prerequisite knock-on: the
  register-viewer vs `inker::routing` dual-routing reconcile
  (integration plan (`mere/design_docs/mere_docs/implementation_strategy/2026-06-02_modular_integration_plan.md`) §1.9)
  must land before a user-facing picker.
- **Texture plumbing** is already owned and stays put: the wgpu sibling libs
  (scry / graft / weld) get foreign renderings into our device, netrender's
  external-texture compose pass places them, constellation actors own per-tile
  lifecycle. Verso must not re-absorb any of this; that is how the old charter
  became a grab-bag.
- **The flip** — what survives a presentation moving between engines over a
  tile that keeps its identity — is unowned ground. Every shipped layer
  assumes one engine per tile for the tile's life; nothing today can swap a
  tile's backing engine mid-life. That gap is verso.

## 3. The charter

Verso owns what survives a flip and how it is carried.

1. **A portable view-state type**, layered so it degrades gracefully:
   - navigation state (URL, history cursor, scroll)
   - form state
   - session state (cookie / storage scope)
   - document snapshot (serialized DOM)
   - visual snapshot (the donor's last frame as a texture, so a flip
     cross-fades instead of flashing)

   Each carrier declares which layers it supports; missing layers degrade,
   never block.
2. **Carriers per engine pair**: capture-from-donor and inject-into-receiver.
   Illustrative-signature-only (not compile-ready):

   ```rust
   trait FlipCarrier {
       fn layers(&self) -> LayerSet;
       fn capture(&self, donor: &dyn EngineView) -> PortableViewState;
       fn inject(&self, receiver: &mut dyn EngineView, state: PortableViewState);
   }
   ```

3. **Flip choreography** over a tile: freeze the visual snapshot, boot the
   receiver actor, inject, swap the backing texture, park or retire the donor.
   Tile identity (node id, pane, lineage) is stable across the flip, and the
   flip is recorded as a node-lineage event ("presented via genet, flipped to
   scrying") — provenance for free.

## 4. Invariants

- **One hop.** A secondary never donates to another secondary. Every hop is
  lossy; chains compound loss and make state authority ambiguous. Re-root at
  the lossless roots instead: engines are byte-consuming and never own
  networking, so source bytes (netfetcher / eidetic) plus the primary's
  snapshot are always available.
- **Asymmetric fidelity is the nature of the engines, not policy.** genet and
  nematic are glass-box (full state export). System WebViews are black-box:
  inject almost everything; extract only partially (URL, title, outerHTML via
  eval, cookies via API; never the JS heap). The primary-to-secondary
  directionality encodes this.
- **Flip-back is allowed along the same pair**, lossier in the black-box
  direction: re-fetch plus navigation-state carry (the cheap path).

## 5. Ceilings (set expectations early)

- "State and all" tops out at **same page, same session, same place** — never
  *same running program*. JS heap state does not cross engines.
- **The session substrate splits.** netfetcher/eidetic is one cookie world;
  the system WebView brings its own network stack. V1 is one-shot export at
  flip time. Continuous mirroring is a tarpit; and if sync state between the
  two worlds is ever surfaced in UI, it must be genuine status, never a
  placebo (house rule).

## 6. Why this matters ecosystem-wide

The compatibility view is what makes genet's W3C-knockout strategy shippable:
capabilities can be cut aggressively because anything a cut breaks has a
one-gesture escape hatch with place and session intact. It is also the
adoption ramp — mere works on the real web from day one through scrying, while
genet takes over content classes as it matures. The pressure-release valve
buys genet time to stay minimal.

## 7. Sequencing and disposition

Gate order (done-conditions, not dates):

1. **P4 first, no verso needed**: the scrying tile lands as a constellation
   actor + `ExternalTextureItem` placement (live home: integration plan S6).
2. **Inker picker**: engines become user-visible; forces the dual-routing
   reconcile.
3. **Mint verso at the first flip**: one pair (genet → scrying), one carrier,
   flip choreography on one tile. That is the consumer-pull moment.

**V1 done when**: a page rendered by genet flips to scrying with URL, scroll,
and cookies carried, the tile identity and lineage intact, and a flip-back
that re-fetches with navigation-state carry.

**Current crates**: retired 2026-06-10 (same day, after this charter landed).
`crates/verso/` is deleted, `SurfaceTargetId` lives in `inker::routing`, and
the dead deps are gone — receipt in the
topology doc §9 (`mere/design_docs/mere_docs/technical_architecture/2026-05-19_workspace_topology_status.md`).
The name is designated for this charter and is minted at step 3.
