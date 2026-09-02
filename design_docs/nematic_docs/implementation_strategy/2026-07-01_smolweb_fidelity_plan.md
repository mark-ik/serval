# Smolweb Fidelity Plan — enrich the ASTs, trust in the native lane, bespoke only where boxes fail

**Date**: 2026-07-01
**Status**: planning (with Mark). Extends the
[native smolweb rendering plan](2026-06-27_native_smolweb_rendering_plan.md)
(that effort shipped: transport → parse → native themed render → scene → window,
with scroll, link nav, per-site/app theming). This one recovers the
spec-faithfulness the flavour-neutral pipeline collapses, and closes the security
posture the native lane currently drops.

> **Home refinement, 2026-08-03**: WS1's AST enrichment lands wherever the
> grammar lives at the time, per the
> smolweb home decision (`smolweb/design_docs/technical_architecture/2026-08-03_smolweb_home_decision.md`)
> (spec-accurate grammars follow the wire crates into the smolweb workspace;
> WS2/WS3 are implementation-side and stay in genet/cambium-nematic).

**The three principles** (Mark's call, 2026-07-01):

1. **Share the synonymous parts through the box substrate.** A gemtext paragraph
   and an HTML paragraph are the same box. Render them the same way and inherit
   typography, wrapping, selection, focus order, and a11y once, from genet-layout,
   instead of re-deriving them per format.
2. **Go bespoke (regime B) only where a format's line model genuinely is not
   box-shaped.** Gopher's fixed-width typed column is the first real case. A
   paragraph is never a reason to go bespoke, because it is the synonymous case.
3. **Enrich the parse ASTs, and manage trust in the native lane.** Most of what we
   collapse is lost at parse time, not paint time. And the native genet lane
   bypasses `Block`, so it drops the trust posture the card lane models.

---

## 1. What we collapse today (audit, verified against code)

The important finding: almost every semantic loss happens at the **flavour-neutral
parse ASTs**, before any view exists. The box rendering is mostly innocent. Switching
render regimes would recover none of it, because the data is already gone.

### Semantic collapses (parse layer — fix by enriching the AST)

| Protocol | Spec distinction | Where it is lost |
| --- | --- | --- |
| RSS/Atom | `<summary>` (abstract) vs `<content>` (full body) | merged into one `summary` ([feed.rs:212](../../../components/errand/src/parse/feed.rs)); the article body is dropped |
| Atom | `published` vs `updated`; RSS `pubDate` | merged into one `date` ([feed.rs:207](../../../components/errand/src/parse/feed.rs)) |
| RSS/Atom | `<enclosure>` (podcast audio/media) | no field on `FeedEntry`; podcasts lose their payload |
| RSS/Atom | `<guid>` / `<id>` (stable entry identity) | dropped; read-state and dedup fall back to link+title |
| Atom | multiple `<link rel="alternate\|self\|enclosure">` | first-wins, `rel` ignored ([feed.rs:174](../../../components/errand/src/parse/feed.rs)) |
| Gopher | item-type family (RFC 1436 + gopher+) | `g`/`I`→Image, `9`→Binary, but `4`/`5`/`6`/`d`/`;`→Other; `8` (telnet)→Other while `T` (tn3270)→Telnet, an inversion ([gopher.rs:112-119](../../../components/errand/src/parse/gopher.rs)). The fetch/handling hint the type char exists to carry is flattened |
| Gopher | type `7` = search server (append a query with a TAB) | rendered as a plain link; the query-input step is gone |
| Spartan | `=:` prompt-upload line (its defining feature) | becomes body text; `GemLine` has no prompt variant |
| **all** | **trust posture** (gemini TOFU / spartan-unauthenticated / misfin-signed) | carried by neither the ASTs nor the native views |

### Presentation collapses (box-substrate artifacts — the regime-B candidates)

- **Gemtext text runs** join consecutive lines with a space, so hard line breaks
  vanish (poems, addresses, deliberate non-`pre` layout). "Paragraph" is not a
  gemtext concept; each line is discrete.
- **Gopher menus** render as proportional `p.gopher-itemline` rows, so the
  fixed-width column alignment gopher clients traditionally give (and that the
  ASCII-art info `pre` blocks assume) is broken.

### The trust gap (architectural)

The parse ASTs carry no trust state (verified: nothing in `errand/src/parse/`). The
native lane goes errand-parse → smolweb-views, **bypassing `Block`** and its
`DocumentTrustState`. So focused viewing via the genet lane surfaces no security
posture: a spartan page (unauthenticated by design), a gemini page (TOFU), and a
misfin message (signed sender) render with the same neutral chrome. The transport
already knows the outcome (Phase A installs `InMemoryTofu`; a pin mismatch fails the
load) and then discards it.

**Correction (2026-07-01 review): the card lane's trust is structurally present but
empty in practice.** Every nematic smolweb engine emits `trust:
DocumentTrustState::Unknown` today (verified in `gemtext.rs` / `gopher.rs` / `nex.rs`
/ `feed.rs`), so the `Block` lane *models* the posture without ever *populating* it.
WS2 therefore has to produce trust for **both** lanes from the one transport source,
not merely carry it into the native one; the done-condition ("same posture on card
and focused tile") already implies this, but the audit contrast above overstates the
card lane's current state.

---

## 2. The regime spectrum (design frame)

Three ways to get a format onto the screen. The plan keeps A as the default and
escalates one format at a time.

| | (A) element tree + CSS *(default)* | (B) own layout, shared shaper | (C) raw paint |
| --- | --- | --- | --- |
| Format idiom lives in | mapping + stylesheet | a bespoke line/layout tree | a bespoke layout + paint fn |
| Shaping | shared (genet) | shared (parley direct) | shared shaper |
| Line-break + stack | shared | you own it | you own it |
| Selection / find / a11y / zoom | free | re-earned per format | re-earned per format |
| Cross-format identity of a paragraph | guaranteed identical | drifts | drifts |
| Code per format | ~50 lines + CSS | ~300-800 (a mini typesetter) | ~800-2000 |

The rule that falls out: **share synonymous constructs through A; reach for B only
when the format's line model is genuinely not box-flow** (a fixed-width grid, hard
columns, terminal alignment). Lagrange is a real (B); the shipped native lane took
its philosophy and implemented it as (A) for the leverage.

---

## 3. Workstream 1 — enrich the parse ASTs (errand)

Recover the parse-layer losses. Each AST change pairs with the nematic lowering that
consumes it on the capture/`Block` side, so the two lanes stay in step.

**Feed** ([errand/src/parse/feed.rs](../../../components/errand/src/parse/feed.rs)) —
illustrative signatures:

```rust
// illustrative, not compile-ready
pub struct FeedEntry {
    pub title: Option<String>,
    pub id: Option<String>,            // <guid> / atom:id — stable identity
    pub link: Option<String>,          // rel="alternate" (the article)
    pub enclosure: Option<Enclosure>,  // <enclosure> / rel="enclosure" (media)
    pub published: Option<String>,     // first authored
    pub updated: Option<String>,       // last changed
    pub summary: Option<String>,       // <summary> / RSS <description>
    pub content: Option<String>,       // <content> / <content:encoded> (full body)
    pub author: Option<String>,
    pub categories: Vec<String>,
}
pub struct Enclosure { pub url: String, pub mime: Option<String>, pub length: Option<u64> }
```

Channel level gains `ttl: Option<u32>` (the poll-interval hint the real Subscribe
feature needs) and `image: Option<String>`. `feed_view` keeps showing summary+date;
the new fields feed the article reader, the podcast affordance, and read-state.

**Two flags on this workstream (2026-07-01 review):**

- **Lockstep + publish timing.** These are public-struct field changes on errand,
  breaking nematic's lowerings and `feed_view` simultaneously: one coordinated
  cross-repo pass (errand → genet → mere) per the established push choreography.
  And errand's manifest is publish-shaped (crates.io metadata, keywords, readme), so
  the field set should settle through WS1 *before* any crates.io publish; churning
  public struct fields post-publish is a semver treadmill.
- **`content` is an HTML fragment — the article reader needs a lane decision.** Feed
  bodies (`<content>`/`<content:encoded>`) are HTML. Rendering them inside
  `feed_view` would pull HTML rendering into smolweb-views, against the two-family
  split; the alternative is a cross-lane handoff (the entry card opens the article
  via the HTML/document lane, with `content` as the offline body). Decide before the
  article reader is built; the field itself is lane-neutral and can land first.

**Gopher** ([errand/src/parse/gopher.rs](../../../components/errand/src/parse/gopher.rs)):
add `raw_type: char` to `GopherItem` so the exact item type always survives even when
`kind` is coarse; fix the `8`/`T` inversion; add `Cso` (type 2, interactive query);
keep the coarse `kind` for the marker but let handling read `raw_type`. Type-7 search
stays `Search` here; the input affordance is Workstream 3.

**Gemtext / spartan**
([errand/src/parse/gemtext.rs](../../../components/errand/src/parse/gemtext.rs)): add
`GemLine::Prompt { url, label }` and classify `=:` (benign for pure gemtext, which
never carries it). Spartan then renders it as an upload affordance instead of body
text.

**Cross-repo note:** struct-field additions are visible to mere/genet through the
gitignored `.cargo/config.toml` path override at build time, so the local edit loop
works. A clean or CI build needs the errand push (unlike the *feature*-resolution
wall the native plan hit, plain field additions do not need a feature gate).

---

## 4. Workstream 2 — trust in the native lane

Trust originates at the **transport** (errand already knows it) and should be
surfaced as **tile chrome**, not document-body content, exactly like a browser's
address-bar posture. Both lanes converge on one trust type so a capsule looks the
same whether shown as a card or a focused tile.

- **Produce at transport.** errand returns a trust descriptor beside the bytes. Reuse
  the `DocumentTrustState` shape (Trusted / Tofu / Insecure / Broken / Unknown) so the
  two lanes share one vocabulary; misfin's signed-sender identity is an extra field,
  not a new ladder. Mapping: gemini matched-pin → Trusted or Tofu, mismatch → Broken;
  gopher / finger / nex / spartan → Insecure (unauthenticated by design); misfin →
  Trusted + `signer` when the sender identity verifies.

  > **Correction, 2026-08-04: that mapping is keyed on the wrong thing.** It reads
  > posture off the *scheme*, and posture is a property of the **carrier**. Once a
  > protocol can run over more than one carrier (see
  > carrier independence (`smolweb/design_docs/research/2026-08-04_protocol_carrier_independence.md`)),
  > the table above ships a falsehood in both directions: gopher over a Reticulum
  > link is **not** Insecure, because the link is encrypted and the peer is proven
  > by its destination key; and gemini over that same link is **not** Tofu, because
  > there is no certificate and no pin to have a state about. The descriptor must be
  > produced by the transport that actually carried the bytes, with the protocol
  > contributing only what it adds on top (misfin's signed sender, gemini's client
  > certificate). The mapping above stays correct for the TCP/TLS carrier, which is
  > the only one wired today; it must not be read as a scheme lookup.
- **Carry through the view.** `SmolwebDocument` gains `trust: DocumentTrustState`
  (+ optional `signer`). The native view body does not change; the host reads the
  field.
- **Surface in the host.** The meerkat smolweb lane (`ensure_smolweb` in
  `content/handlers.rs`) captures the fetch trust and exposes it so the tile chrome
  shows the posture. This is the host-integration touchpoint; see
  smolweb host integration plan (`mere/design_docs/mere_docs/implementation_strategy/2026-06-28_smolweb_host_integration_plan.md`).
- **Home for the shared type.** Decide with Mark whether `DocumentTrustState` moves to
  a small shared crate both errand and inker depend on, or errand defines its own and
  the host maps between them at the lane boundary. Default: errand defines a minimal
  `TransportTrust`, the host maps it to `DocumentTrustState`, avoiding a new shared
  crate until a second consumer wants one.
- **Precondition to verify (2026-07-01 review): the host's TOFU store.** errand
  defaults to `PermissiveTofu` (accept-any) unless a store is installed; pelt's
  fetcher installs an `InMemoryTofu`, but whether **meerkat's** fetch path does has
  not been checked. If it does not, gemini in Mere is silently accept-any today and
  "a gemini tile reads its TOFU pin state" has nothing to read — installing (and
  eventually persisting) the host trust store is part of this workstream, not an
  assumption. The card lane needs the same feed: nematic engines emit
  `DocumentTrustState::Unknown` unconditionally (see the §1 correction), so WS2's
  transport descriptor must reach the `EngineInput`/lowering side too.
  *Re-checked 2026-09-02, with the knot evaluation/export plan's open question
  rehomed here:* meerkat is gone (deleted 2026-07-18), so the hosts to check
  are Turnstone and mere. `genet-documents` installs an `InMemoryTofu`; mere's
  `fetch` exposes `install_smolweb_tofu` for a host-owned store; **no durable
  `TofuStore` exists anywhere in the workspace** (smolweb ships only the
  in-memory and permissive ones). The location question the knot plan left
  open — a file beside the profile, or eidetic engrams — is therefore still
  open and belongs to this workstream: start file-backed, migrate when
  persona/keys land fully.

---

## 5. Workstream 3 — bespoke where boxes fail (gopher first)

Only the presentation collapses, and only where the line model is genuinely not
box-shaped.

- **Gopher monospace grid.** Start **B-lite**: render the menu in a monospace grid
  (a `pre`-context or a CSS grid with a monospace font) so type marker, display, and
  the info ASCII-art columns align, while links stay focusable. This recovers the
  alignment without a bespoke typesetter and is unit-testable against the element
  tree. Half the ground is already held: `gopher_view` folds info runs into one
  monospace `pre.gopher-info` today; B-lite extends that treatment to the item rows
  rather than starting over. Escalate to **B-full** (a bespoke fixed-width line layout emitting to the
  paint list) only if terminal-precise columns demand it. Gopher is the one format
  that plausibly earns B-full.
- **Type-7 search affordance.** Render a `Search` item as an inline query input that
  appends the entered text (TAB-joined) before navigating, instead of a bare link.
- **Spartan `=:` prompt.** Consume Workstream 1's `GemLine::Prompt` and render it as
  an input-link (a labelled field that uploads on submit), the affordance the native
  plan named but never modelled.
- **Gemtext hard breaks.** Offer preservation of hard line breaks in text runs as a
  setting (reflow vs preserve), per the configurability-over-defaults principle,
  rather than always joining with a space. Small view tweak, not full regime B.

---

## 6. Sequencing and done-conditions

Targets, not dates.

- **WS1 (AST enrichment) is foundational.** It carries the trust field's neighbours
  and the gopher `raw_type` WS3 reads. Done when: feed round-trips
  published/updated + summary/content + enclosure + id; gopher preserves `raw_type`
  and the `8`/`T` fix; spartan `=:` parses to `Prompt`; the nematic lowerings and
  their tests are green; errand pushed.
- **WS2 (trust) follows WS1.** Done when: a spartan tile reads Insecure, a gemini tile
  reads its TOFU pin state, a misfin tile reads its signer, and the same posture shows
  identically on the card and the focused tile.
- **WS3 (regime B) is independent, gopher-scoped.** Done when: a gopher menu's columns
  align, a type-7 item takes a query, a spartan `=:` uploads, and gemtext hard-break
  preservation is a setting. B-full is entered only if B-lite alignment proves
  insufficient, and that decision is logged here.

---

## Findings

- **The lossy layer is the parse ASTs, not the renderer** (verified against
  `errand/src/parse/*`). This inverts the original "are we foisting HTML" worry: the
  risk is protocol semantics normalized away at parse time, not HTML semantics
  imposed at paint time. The fix is richer ASTs, not a different render regime.
- **The native lane carries no trust** (nothing trust-shaped in `errand/src/parse/`;
  `smolweb-views` emits no posture). The `Block` lane has `DocumentTrustState`; the
  genet lane, which the host uses for focused tiles, drops it.
- **Gopher is the sole clear regime-B candidate.** Gemtext, feed, nex, finger,
  spartan, guppy, scroll, misfin are all box-flow-shaped; gopher's fixed-width typed
  column is the one line model the box substrate visibly distorts.

## Progress

- **2026-07-01**: Plan created from the fidelity audit with Mark (the DocumentBlock →
  Block terminology sweep opened into a substrate/spec-faithfulness review). Collapse
  inventory verified against `errand/src/parse/*`, `smolweb-views/src/lib.rs`, and the
  paint-list API. Three workstreams scoped; A-default / B-for-non-box regime rule set.

## Cross-references

- [native smolweb rendering plan](2026-06-27_native_smolweb_rendering_plan.md) — the
  shipped transport → parse → native render this extends; the two-family model and the
  crate/dependency direction it defines.
- smolweb host integration plan (`mere/design_docs/mere_docs/implementation_strategy/2026-06-28_smolweb_host_integration_plan.md`)
  — the meerkat genet lane; Workstream 2's trust surfacing lands against its P3/P4.
- TERMINOLOGY.md (`mere/design_docs/TERMINOLOGY.md`) — the trust ladder and the
  protocol-faithfulness rule this plan operationalizes.
- errand (sibling repo `mark-ik/errand`) — owns the parse ASTs Workstream 1 enriches
  and the transport Workstream 2 reads trust from.
