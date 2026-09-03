// Copyright 2026 Mark AB (markik)
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The engine-id namespace and the genet render-rung ladder.
//!
//! Engine ids are facts an engine states about itself: a kept Genet session
//! lane returns one from `SessionEngine::engine_id`. Which engine a host
//! selects for an address is routing policy and lives with the controller
//! in `inker::routing`; these names are what that policy is written in.

/// The genet HTML engine's **static** rung (the profile ladder's base): parse →
/// style/layout → paint, with no JS in its dependency graph. The default HTML route
/// and the id existing per-node pins persist, so it keeps the legacy `genet.web`
/// value rather than `genet.static`. See [`GenetRung`].
pub const ENGINE_GENET_WEB: &str = "genet.web";
/// Opt-in static rung rendered by the clean-room Livery CSS engine. This is a
/// sibling implementation of [`ENGINE_GENET_WEB`], not the default route.
pub const ENGINE_GENET_LIVERY: &str = "genet.livery";
/// Render-free fleece extraction lowered into the portable document engine.
/// Selectable for held HTML by explicit per-node pin; never the automatic HTML
/// route, which remains the ordinary Genet renderer.
pub const ENGINE_GENET_READER: &str = "genet.reader";
/// The genet HTML rungs above static (the profile ladder; see [`GenetRung`]). A node
/// pins one of these to escalate capability. Additive, and gated by host registration:
/// until a rung is registered in the host's `EngineRegistry`, it is not `is_available`,
/// so a pin to it falls back to the static route (`route_filtered`). genet scales
/// internally from the static composition up to a full browser
/// (genet `docs/2026-05-12_genet_profile_ladder_plan.md`); these ids select the rung.
pub const ENGINE_GENET_INTERACTIVE: &str = "genet.interactive";
pub const ENGINE_GENET_SCRIPTED: &str = "genet.scripted";
/// The scripted genet rung backed by Nova instead of Boa. Same ladder rung,
/// distinct host-visible engine id so a node can pin the JS backend explicitly.
pub const ENGINE_GENET_SCRIPTED_NOVA: &str = "genet.scripted.nova";
pub const ENGINE_GENET_FULLWEB: &str = "genet.fullweb";
/// Mere-managed system-WebView tile driven by the in-house `scrying`
/// library. Embedded-frame composition into the host's wgpu surface
/// (frames captured via `webview2-com` on Windows / `objc2-web-kit` +
/// ScreenCaptureKit on macOS / WebKitGTK+DMABUF on Linux).
///
/// Preferred non-Servo path. Not in the default routing policy —
/// opt-in per tile via `EngineRouteRequest::pinned_engine` or a
/// per-host override. Auto-fallback rule (genet rendering failure
/// → propose `scrying.web`) is a follow-up; the routing surface
/// already supports it via `pinned_engine`.
///
/// See `design_docs/mere_docs/research/2026-05-11_engine_peers_and_scrying_library_brief.md`.
pub const ENGINE_SCRYING_WEB: &str = "scrying.web";
/// Embedded Servo via the wgpu-graft producer (GL-FBO / DX12-shared / Vulkan
/// external-memory / IOSurface interop). Tier-2 surface engine; opt-in per tile
/// like [`ENGINE_SCRYING_WEB`], not in the default policy.
pub const ENGINE_GRAFT_SERVO: &str = "graft.servo";
/// Bundled Chromium via the wgpu-weld CEF accelerated-OSR producer. Tier-2
/// surface engine; opt-in per tile like [`ENGINE_SCRYING_WEB`].
pub const ENGINE_WELD_CHROMIUM: &str = "weld.chromium";
pub const ENGINE_NEMATIC_FEED: &str = "nematic.feed";
pub const ENGINE_NEMATIC_FILE: &str = "nematic.file";
pub const ENGINE_NEMATIC_FINGER: &str = "nematic.finger";
pub const ENGINE_NEMATIC_GEMTEXT: &str = "nematic.gemtext";
pub const ENGINE_NEMATIC_GOPHER: &str = "nematic.gopher";
pub const ENGINE_NEMATIC_GUPPY: &str = "nematic.guppy";
pub const ENGINE_NEMATIC_KNOT: &str = "nematic.knot";
pub const ENGINE_NEMATIC_KNOT_DJOT: &str = "nematic.knot-djot";
pub const ENGINE_NEMATIC_MARKDOWN: &str = "nematic.markdown";
pub const ENGINE_NEMATIC_MISFIN: &str = "nematic.misfin";
pub const ENGINE_NEMATIC_NEX: &str = "nematic.nex";
pub const ENGINE_NEMATIC_SCROLL: &str = "nematic.scroll";
pub const ENGINE_NEMATIC_TEXT: &str = "nematic.text";
/// Titan (`titan://`) response bodies — gemtext re-tagged with titan provenance.
pub const ENGINE_NEMATIC_TITAN: &str = "nematic.titan";
/// The one host-handled id inker keeps: the neutral "hand this address to the
/// OS" target every host has, and the default policy's `fallback` rule needs
/// one. App-flavored host-handled ids (internal pages, graph-contribution
/// ingest markers) are the host's own vocabulary, defined app-side and layered
/// onto the policy at construction (mere: `mere::routing`); registry keys are
/// plain strings, so a host id costs nothing here.
pub const ENGINE_EXTERNAL_PROTOCOL: &str = "host.external-protocol";

/// Whether `engine_id` names a tier-2 **surface** engine — one that produces GPU
/// frames (a system WebView via [`ENGINE_SCRYING_WEB`]; CEF via weld and Servo via
/// graft when those land) rather than a portable `EngineDocument`. Surface
/// engines go through the `SurfaceEngineRegistry` / producer path; document
/// engines go through the `EngineRegistry`. A host branches on this to pick
/// the lane.
pub fn is_surface_engine(engine_id: &str) -> bool {
    matches!(
        engine_id,
        ENGINE_SCRYING_WEB | ENGINE_GRAFT_SERVO | ENGINE_WELD_CHROMIUM
    )
}

/// A rung of the genet HTML render ladder. genet is one engine that scales from a
/// static, JS-free composition up to a full browser; the rung selects *how much of the
/// web stack* a page is given. Each rung is **additive** over the one below, and each
/// is a principled composition — the static rung carries no JS in its dependency graph
/// (attack-surface + bundle-size + DOM-as-library), so a higher rung is a deliberate
/// escalation, never the default. The default HTML route is [`Static`](Self::Static);
/// a node pins a higher rung to opt in. Ordered by capability (the derived `Ord`).
///
/// Canonical: genet `docs/2026-05-12_genet_profile_ladder_plan.md`; Mere framing:
/// `design_docs/mere_docs/implementation_strategy/2026-06-23_render_ladder_and_extraction_plan.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GenetRung {
    /// parse → style/layout → paint. No JS. The default, and the safe/fast base.
    Static,
    /// + forms / focus / input / accessibility. Still no JS.
    Interactive,
    /// + JS engine + DOM bindings + event routing.
    Scripted,
    /// + navigation / workers / storage / media / WebGL / devtools.
    FullWeb,
}

impl GenetRung {
    /// Every rung, ascending by capability — the ladder a picker offers.
    pub const ALL: [GenetRung; 4] = [
        GenetRung::Static,
        GenetRung::Interactive,
        GenetRung::Scripted,
        GenetRung::FullWeb,
    ];

    /// The engine id that selects this rung. Static keeps the legacy [`ENGINE_GENET_WEB`]
    /// id so existing pins resolve; the higher rungs have their own ids.
    pub fn engine_id(self) -> &'static str {
        match self {
            GenetRung::Static => ENGINE_GENET_WEB,
            GenetRung::Interactive => ENGINE_GENET_INTERACTIVE,
            GenetRung::Scripted => ENGINE_GENET_SCRIPTED,
            GenetRung::FullWeb => ENGINE_GENET_FULLWEB,
        }
    }

    /// A short label for the picker UI.
    pub fn label(self) -> &'static str {
        match self {
            GenetRung::Static => "Static",
            GenetRung::Interactive => "Interactive",
            GenetRung::Scripted => "Scripted",
            GenetRung::FullWeb => "Full Web",
        }
    }
}

/// The genet render rung an `engine_id` selects, or `None` when it is not a genet
/// HTML rung (a nematic, surface, or marker engine).
pub fn genet_rung(engine_id: &str) -> Option<GenetRung> {
    match engine_id {
        ENGINE_GENET_WEB | ENGINE_GENET_LIVERY => Some(GenetRung::Static),
        ENGINE_GENET_INTERACTIVE => Some(GenetRung::Interactive),
        ENGINE_GENET_SCRIPTED | ENGINE_GENET_SCRIPTED_NOVA => Some(GenetRung::Scripted),
        ENGINE_GENET_FULLWEB => Some(GenetRung::FullWeb),
        _ => None,
    }
}

/// Whether `engine_id` names any rung of the genet HTML render ladder. The tier-1
/// counterpart to [`is_surface_engine`]: a genet rung produces a portable
/// `EngineDocument`, not a GPU surface frame.
pub fn is_genet_rung(engine_id: &str) -> bool {
    genet_rung(engine_id).is_some()
}
