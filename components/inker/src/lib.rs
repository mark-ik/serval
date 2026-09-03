// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! # Inker
//!
//! Modular engine/renderer controller for the
//! [`mere`](https://crates.io/crates/mere) browser — selects and orchestrates
//! content engines (Wry system webview, Genet,
//! [`nematic`](https://crates.io/crates/nematic) smolweb, file/media viewers).
//!
//! In the printing-press metaphor that organizes Mere's architecture, the
//! Inker pairs each engine to its content and applies the engine's "ink" to
//! the [`platen`](https://crates.io/crates/platen) press. Routing URI schemes
//! to engines, lifecycle management, and engine-output piping all live here.
//! The contracts an engine implements to be hosted (session traits, the
//! accessibility projection, capabilities, page capture, the engine-id
//! namespace) are `document-session-api`, re-exported here.
//! (*Verso* names the engine-flip / compatibility-view seam — see
//! `design_docs/verso_docs/` — not a pipeline stage below platen.)
//!
//! ## Status
//!
//! Pre-1.0. This 0.0.x release reserves the crate name and documents intent;
//! implementation is in progress within the
//! [Mere workspace](https://crates.io/crates/mere).

#![doc(html_root_url = "https://docs.rs/inker/0.0.1")]

// The engine-facing contract half lives in `document-session-api` (platform
// boundary plan, P1) and is re-exported here module for module, so every
// `inker::` path a consumer wrote still resolves.
pub use document_session_api::{a11y, capabilities, page_capture, session_engine};

/// Host-neutral engine routing contracts.
pub mod routing;

/// Portable document model — what engines produce.
pub mod document;

/// Engine trait and registry.
pub mod engine;

/// Surface-engine traits and registry — parallel dispatch path for
/// long-lived, frame-streaming engines (e.g. `scrying.web`).
pub mod surface_engine;

/// Content-type sniffing for unlabelled byte streams.
pub mod sniff;

/// Statement extraction — the pure walk collecting knot `rel` links. The
/// graph-side apply lives in mere's `linked-data` crate (kernel-free split).
pub mod statements;

pub use a11y::{
    A11yCapability, DocumentA11yAction, DocumentA11yActionData, DocumentA11yActionRequest,
    DocumentA11yBounds, DocumentA11yClickTarget, DocumentA11yHasPopup, DocumentA11yLive,
    DocumentA11yNode, DocumentA11yNodeId, DocumentA11yOrientation, DocumentA11yPoint,
    DocumentA11yProjection, DocumentA11yRole, DocumentA11yState, DocumentA11ySupport,
    DocumentA11ySupportError, DocumentA11yToggled,
};
pub use capabilities::{
    CapabilityStatus, DocumentCapabilities, DocumentCapabilityStatus, WebFeatureStatus,
};
pub use document::{
    Block, BlockEvaluator, BlockEvaluators, BlockProvenance, BlockProvenanceMap,
    DocumentDiagnostic, DocumentProvenance, DocumentTrustState, EngineDocument, EvalOutcome,
    EvalOutput, EvaluationPolicy, Fetched, GophermapContext, InlineSpan, ResolvedProvenance,
    TableAlignment, TranscludeOutcome, TransclusionPolicy, evaluate_blocks, inline_text,
    parse_eval, parse_include, resolve_transclusions,
};
pub use engine::{Engine, EngineError, EngineInput, EngineRegistry};
pub use page_capture::{
    CssExtent, CssPoint, PageCaptureImageArtifact, PageCaptureOutput, PageCaptureRequest,
    PageCaptureRequestId, PageCaptureScope, PageCaptureViewportFacts,
};
pub use routing::{
    EngineRouteDecision, EngineRoutePolicy, EngineRouteRequest, EngineRouteRule, SurfaceContract,
    SurfaceContractMode, SurfaceTargetId, WorkspaceRouteId,
};
pub use session_engine::{
    ContentLineage, ContentReport, DocumentClip, DocumentClipArtifact, DocumentClipArtifactRole,
    DocumentFindDirection, DocumentFindMatch, DocumentFindQuery, DocumentFindReveal,
    DocumentFindState, DocumentSession, DocumentZoomState, EngineKindIndex, EngineKinds,
    OutlineEntry, SessionButtonState, SessionClick, SessionCursor, SessionEffect, SessionEngine,
    SessionError, SessionFocusDirection, SessionFormMethod, SessionFormSubmission, SessionIme,
    SessionInput, SessionInputResult, SessionKey, SessionLink, SessionModifiers,
    SessionNavigationCommand, SessionPointerButton, SessionRegistry, SessionScrollKey,
    SessionSpawnRequest, SessionTextTarget,
};
pub use sniff::sniff_content_type;
pub use statements::{LinkStatement, link_statements};
pub use surface_engine::{
    Cookie, CookieAttributeCapabilities, CookieCapabilities, CursorShape, DataTransfer,
    DataTransferItem, DragDropCapabilities, DragEvent, DragOperationSet, DragPhase,
    EngineProfileBinding, FocusReason, FrameHandleOwnership, HttpAuthenticationAnswer,
    HttpAuthenticationChallenge, HttpCredentials, HttpProtectionSpace, KeyboardEvent,
    KeyboardModifiers, MouseButton, MouseEvent, MouseEventKind, NativeSurfaceHost,
    NativeTextureHandle, NavigationEvent, PermissionAnswer, PermissionDescriptor,
    PermissionRequest, PermissionState, PhysicalPosition, PointerButtons, PointerEvent,
    PointerInputCapabilities, PointerPhase, PointerType, SameSite, ScriptCapabilities,
    SurfaceEngine, SurfaceEngineRegistry, SurfaceError, SurfaceFrame, SurfaceProducer,
    SurfaceSettings, SurfaceSpawnRequest, SurfaceSyncHandle, SurfaceTextureFormat,
    UserAgentRequestId, WebFrameTransportMode, WebMessage, WebSurface, WebSurfaceCapabilities,
    WebSurfaceEvent,
};

/// Crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Lifecycle stage marker.
pub const STAGE: &str = "pre-alpha";
