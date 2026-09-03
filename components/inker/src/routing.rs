// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Host-neutral engine routing vocabulary and default policy.

use serde::{Deserialize, Serialize};

mod ids;
pub use ids::{NodeKey, RouteViewId};
// The engine-id namespace and the genet rung ladder are engine facts; they
// live in the contract crate and are re-exported here so every
// `inker::routing::ENGINE_*` path resolves unchanged.
pub use document_session_api::engine_ids::*;

/// Opaque engine-output target key (`node:<idx>` / `view:<uuid>` /
/// `route:...`), minted by [`surface_target_for`]. Inlined from the retired
/// `verso-core` (its sole external export; verso is reborn as the flip seam,
/// see `design_docs/verso_docs/` charter 2026-06-10).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SurfaceTargetId(pub String);

impl SurfaceTargetId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkspaceRouteId(pub String);

impl WorkspaceRouteId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineRouteRequest {
    pub workspace_id: WorkspaceRouteId,
    pub view: Option<RouteViewId>,
    pub node: Option<NodeKey>,
    pub address: String,
    /// Known content type from a fetch response, file extension, or content
    /// sniff. Routing prefers content-type rules to scheme rules when this
    /// is set, so the same address can re-route after the host learns the
    /// MIME type from a response (e.g. an HTTPS URL serving `text/markdown`
    /// re-routes from Genet to the markdown engine on the second pass).
    #[serde(default)]
    pub content_type: Option<String>,
    /// Per-node engine pin. When set, routing uses this engine ID directly
    /// (provided the engine is available per the active filter). Wins over
    /// content-type, per-domain, and scheme rules — explicit user pin is
    /// the most authoritative signal.
    #[serde(default)]
    pub pinned_engine: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineRouteDecision {
    pub engine_id: String,
    pub surface_contract: SurfaceContract,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineRoutePolicy {
    pub rules: Vec<EngineRouteRule>,
    pub fallback: EngineRouteRule,
    /// Per-host overrides. Key is a host string (case-insensitive,
    /// matched against the address authority — e.g. `example.com`,
    /// `blog.test`). When the request's address has a matching host AND
    /// the override engine is available, the override wins over scheme
    /// rules. Content-type rules and pinned-engine still win over
    /// per-host overrides.
    #[serde(default)]
    pub per_host_overrides: std::collections::HashMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineRouteRule {
    pub schemes: Vec<String>,
    /// MIME types this rule matches. Empty means "scheme-only rule." A rule
    /// with any content types is preferred over scheme rules when the
    /// request carries a `content_type`.
    #[serde(default)]
    pub content_types: Vec<String>,
    pub engine_id: String,
    pub mode: SurfaceContractMode,
}

impl EngineRoutePolicy {
    pub fn route(&self, request: &EngineRouteRequest) -> EngineRouteDecision {
        self.route_filtered(request, |_| true)
    }

    /// Route, considering only rules whose `engine_id` passes `is_available`.
    /// When the matched rule's engine isn't available, the policy walks
    /// through to the next rule rather than producing a decision pointing at
    /// an unregistered engine.
    ///
    /// Pair with [`crate::EngineRegistry::contains`] to route to whatever
    /// engines are actually registered on this host:
    ///
    /// ```ignore
    /// policy.route_filtered(&request, |id| registry.contains(id))
    /// ```
    pub fn route_filtered(
        &self,
        request: &EngineRouteRequest,
        is_available: impl Fn(&str) -> bool,
    ) -> EngineRouteDecision {
        let scheme = address_scheme(&request.address);

        // 1. Pinned engine wins over everything else when available. This is
        //    the most explicit user signal ("this node always uses X").
        if let Some(pin) = request.pinned_engine.as_deref()
            && is_available(pin)
        {
            return EngineRouteDecision {
                engine_id: pin.to_string(),
                surface_contract: SurfaceContract {
                    target: surface_target_for_request(request, scheme),
                    // Pinned routes don't carry surface mode metadata;
                    // fall through to a sensible default. CompositedTexture
                    // is right for visible engines; pinned headless engines
                    // can override with their own contract layer.
                    mode: SurfaceContractMode::CompositedTexture,
                },
            };
        }

        // 2. Content-type-positive rules win when the request carries a known
        //    content type. This lets a fetch response steer the second-pass
        //    route after the initial scheme route picked a fetcher.
        let by_content_type = request.content_type.as_deref().and_then(|ct| {
            self.rules
                .iter()
                .find(|rule| rule.matches_content_type(ct) && is_available(&rule.engine_id))
        });

        // 3. Per-host overrides win over generic scheme rules but lose to
        //    content-type — server-claimed content type beats user's
        //    domain-level preference (which may be stale).
        let by_host = host_from_address(&request.address).and_then(|host| {
            let host_lower = host.to_ascii_lowercase();
            self.per_host_overrides
                .iter()
                .find(|(domain, engine_id)| {
                    domain.eq_ignore_ascii_case(&host_lower) && is_available(engine_id)
                })
                .map(|(_, engine_id)| engine_id.as_str())
        });

        // 4. Scheme rules — the cheapest base.
        let by_scheme = scheme.and_then(|scheme| {
            self.rules
                .iter()
                .find(|rule| rule.matches_scheme(scheme) && is_available(&rule.engine_id))
        });

        if let Some(rule) = by_content_type {
            return EngineRouteDecision {
                engine_id: rule.engine_id.clone(),
                surface_contract: SurfaceContract {
                    target: surface_target_for_request(request, scheme),
                    mode: rule.mode,
                },
            };
        }
        if let Some(host_engine) = by_host {
            return EngineRouteDecision {
                engine_id: host_engine.to_string(),
                surface_contract: SurfaceContract {
                    target: surface_target_for_request(request, scheme),
                    mode: SurfaceContractMode::CompositedTexture,
                },
            };
        }

        let rule = by_scheme.unwrap_or(&self.fallback);
        EngineRouteDecision {
            engine_id: rule.engine_id.clone(),
            surface_contract: SurfaceContract {
                target: surface_target_for_request(request, scheme),
                mode: rule.mode,
            },
        }
    }
}

impl Default for EngineRoutePolicy {
    fn default() -> Self {
        Self {
            rules: vec![
                EngineRouteRule::new(
                    ["http", "https"],
                    ENGINE_GENET_WEB,
                    SurfaceContractMode::CompositedTexture,
                ),
                EngineRouteRule::new(
                    ["gemini", "spartan"],
                    ENGINE_NEMATIC_GEMTEXT,
                    SurfaceContractMode::CompositedTexture,
                ),
                EngineRouteRule::new(
                    ["gopher"],
                    ENGINE_NEMATIC_GOPHER,
                    SurfaceContractMode::CompositedTexture,
                ),
                EngineRouteRule::new(
                    ["finger"],
                    ENGINE_NEMATIC_FINGER,
                    SurfaceContractMode::CompositedTexture,
                ),
                EngineRouteRule::new(
                    ["scroll"],
                    ENGINE_NEMATIC_SCROLL,
                    SurfaceContractMode::CompositedTexture,
                ),
                EngineRouteRule::new(
                    ["misfin"],
                    ENGINE_NEMATIC_MISFIN,
                    SurfaceContractMode::CompositedTexture,
                ),
                EngineRouteRule::new(
                    ["nex"],
                    ENGINE_NEMATIC_NEX,
                    SurfaceContractMode::CompositedTexture,
                ),
                EngineRouteRule::new(
                    ["guppy"],
                    ENGINE_NEMATIC_GUPPY,
                    SurfaceContractMode::CompositedTexture,
                ),
                EngineRouteRule::new(
                    ["titan"],
                    ENGINE_NEMATIC_TITAN,
                    SurfaceContractMode::CompositedTexture,
                ),
                EngineRouteRule::new(
                    ["file"],
                    ENGINE_NEMATIC_FILE,
                    SurfaceContractMode::CompositedTexture,
                ),
                // Content-type rules: when a fetcher learns the MIME type,
                // these win over scheme rules so an HTTPS response of
                // `text/markdown` re-routes to the markdown engine instead
                // of staying on Genet.
                EngineRouteRule::content_type(
                    ["text/markdown", "text/x-markdown"],
                    ENGINE_NEMATIC_MARKDOWN,
                    SurfaceContractMode::CompositedTexture,
                ),
                EngineRouteRule::content_type(
                    ["text/gemini"],
                    ENGINE_NEMATIC_GEMTEXT,
                    SurfaceContractMode::CompositedTexture,
                ),
                EngineRouteRule::content_type(
                    ["text/plain"],
                    ENGINE_NEMATIC_TEXT,
                    SurfaceContractMode::CompositedTexture,
                ),
                EngineRouteRule::content_type(
                    [
                        "application/rss+xml",
                        "application/atom+xml",
                        "application/feed+xml",
                    ],
                    ENGINE_NEMATIC_FEED,
                    SurfaceContractMode::CompositedTexture,
                ),
                EngineRouteRule::content_type(
                    ["text/x-knot", "application/x-knot"],
                    // djot is the default knot grammar (knot design doc §10.5
                    // Phase 5); the CommonMark `nematic.knot` engine stays
                    // available by explicit pin for import/compat.
                    ENGINE_NEMATIC_KNOT_DJOT,
                    SurfaceContractMode::CompositedTexture,
                ),
                // HTML by content-type routes to genet regardless of scheme, so a
                // local `file://` page or an HTTPS response that turns out to be HTML
                // both land on the web engine. (Web-standard content → genet.)
                EngineRouteRule::content_type(
                    ["text/html", "application/xhtml+xml"],
                    ENGINE_GENET_WEB,
                    SurfaceContractMode::CompositedTexture,
                ),
                // Smolweb content-type refinements: a fetch that learns one of these
                // routes to the matching nematic engine even when the scheme alone
                // would not (e.g. an HTTPS endpoint serving a gophermap or a feed).
                EngineRouteRule::content_type(
                    ["application/gopher-menu"],
                    ENGINE_NEMATIC_GOPHER,
                    SurfaceContractMode::CompositedTexture,
                ),
                EngineRouteRule::content_type(
                    ["text/x-finger"],
                    ENGINE_NEMATIC_FINGER,
                    SurfaceContractMode::CompositedTexture,
                ),
                EngineRouteRule::content_type(
                    ["application/x-nex"],
                    ENGINE_NEMATIC_NEX,
                    SurfaceContractMode::CompositedTexture,
                ),
                EngineRouteRule::content_type(
                    ["application/x-guppy"],
                    ENGINE_NEMATIC_GUPPY,
                    SurfaceContractMode::CompositedTexture,
                ),
                EngineRouteRule::content_type(
                    ["application/x-titan"],
                    ENGINE_NEMATIC_TITAN,
                    SurfaceContractMode::CompositedTexture,
                ),
                EngineRouteRule::content_type(
                    ["message/x-misfin"],
                    ENGINE_NEMATIC_MISFIN,
                    SurfaceContractMode::CompositedTexture,
                ),
                // `application/feed+json` (JSON Feed) alongside the XML feed types above.
                EngineRouteRule::content_type(
                    ["application/feed+json"],
                    ENGINE_NEMATIC_FEED,
                    SurfaceContractMode::CompositedTexture,
                ),
            ],
            fallback: EngineRouteRule::new(
                Vec::<&str>::new(),
                ENGINE_EXTERNAL_PROTOCOL,
                SurfaceContractMode::Headless,
            ),
            per_host_overrides: std::collections::HashMap::new(),
        }
    }
}

impl EngineRouteRule {
    pub fn new(
        schemes: impl IntoIterator<Item = impl Into<String>>,
        engine_id: impl Into<String>,
        mode: SurfaceContractMode,
    ) -> Self {
        Self {
            schemes: schemes.into_iter().map(Into::into).collect(),
            content_types: Vec::new(),
            engine_id: engine_id.into(),
            mode,
        }
    }

    /// Build a content-type-only rule (no scheme matching).
    pub fn content_type(
        types: impl IntoIterator<Item = impl Into<String>>,
        engine_id: impl Into<String>,
        mode: SurfaceContractMode,
    ) -> Self {
        Self {
            schemes: Vec::new(),
            content_types: types.into_iter().map(Into::into).collect(),
            engine_id: engine_id.into(),
            mode,
        }
    }

    pub fn matches_scheme(&self, scheme: &str) -> bool {
        self.schemes
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(scheme))
    }

    /// Match the type/subtype portion of a MIME header, ignoring any
    /// `; parameter` suffix (e.g. `text/markdown; charset=utf-8` matches
    /// a rule listing `text/markdown`).
    pub fn matches_content_type(&self, content_type: &str) -> bool {
        let primary = content_type
            .split(';')
            .next()
            .unwrap_or(content_type)
            .trim();
        self.content_types
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(primary))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceContract {
    pub target: SurfaceTargetId,
    pub mode: SurfaceContractMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SurfaceContractMode {
    CompositedTexture,
    NativeOverlay,
    EmbeddedHost,
    Headless,
}

/// Extract the host (authority without port or userinfo) from an address.
/// Returns `None` when the address has no `://` separator (`mailto:`,
/// `data:`, `about:` etc. don't have a host).
pub fn host_from_address(address: &str) -> Option<&str> {
    let after_scheme = address.split_once("://")?.1;
    // Authority ends at the first '/', '?', or '#'.
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme);
    // Strip userinfo before '@' if present.
    let host_with_port = authority.rsplit('@').next().unwrap_or(authority);
    // Strip port after ':' (handles `host:port`; doesn't try IPv6 brackets).
    let host = host_with_port.split(':').next().unwrap_or(host_with_port);
    if host.is_empty() { None } else { Some(host) }
}

pub fn address_scheme(address: &str) -> Option<&str> {
    let (scheme, _) = address.split_once(':')?;
    let first = scheme.as_bytes().first()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    if scheme
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
    {
        Some(scheme)
    } else {
        None
    }
}

fn surface_target_for_request(
    request: &EngineRouteRequest,
    scheme: Option<&str>,
) -> SurfaceTargetId {
    if let Some(node) = request.node {
        return SurfaceTargetId::new(format!("node:{}", node.index()));
    }
    if let Some(view) = request.view {
        return SurfaceTargetId::new(format!("view:{}", view.as_uuid()));
    }
    SurfaceTargetId::new(format!(
        "workspace:{}:{}",
        request.workspace_id.as_str(),
        scheme.unwrap_or("unknown")
    ))
}

// Tests live in `routing/tests.rs` to keep this file under the 600-LOC ceiling.
#[cfg(test)]
mod tests;
