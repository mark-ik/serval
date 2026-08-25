// Copyright 2026 Mark AB (markik)
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Surface-engine traits and registry — parallel dispatch path for
//! long-lived, frame-streaming engines alongside [`crate::engine`].
//!
//! Document engines are request/response: fetch → render → `EngineDocument`.
//! Surface engines are lifecycle-bound: spawn → long-lived session producing
//! a composited-frame stream + events until torn down. Both registries
//! coexist; the host dispatches through whichever holds the resolved engine ID
//! (document registry for `nematic.*` / `genet.web`; surface registry for
//! `scrying.web`).

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::a11y::A11yCapability;
use crate::routing::EngineRouteDecision;

// ── User-agent requests ────────────────────────────────────────────────────

/// Correlates one user-agent policy request with exactly one later answer.
///
/// The id is scoped to the [`WebSurface`] that emitted it. Hosts which drive
/// several surfaces must retain the surface identity beside this value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct UserAgentRequestId(u64);

impl UserAgentRequestId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A Permissions API descriptor in engine-neutral web-platform terms.
///
/// Chromium-only values are not allowed to become shared enum variants. A
/// producer which cannot project a request precisely uses `Other` and keeps
/// the stable standard or implementation name it received.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PermissionDescriptor {
    Camera,
    Microphone,
    Geolocation,
    Notifications,
    ClipboardRead,
    Midi { sysex: bool },
    PointerLock,
    KeyboardLock,
    IdleDetection,
    LocalFonts,
    StorageAccess,
    ProtectedMediaIdentifier,
    DisplayCapture { audio: bool, video: bool },
    Other(String),
}

/// The three states exposed by the Permissions API.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PermissionState {
    Prompt,
    Granted,
    Denied,
}

/// One answer to a pending permission request.
///
/// `Dismiss` returns the request to `Prompt` and is deliberately distinct
/// from a retained denial.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionAnswer {
    Grant,
    Deny,
    Dismiss,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub id: UserAgentRequestId,
    pub origin: String,
    pub descriptors: Vec<PermissionDescriptor>,
}

/// RFC 9110 protection-space data. It identifies credential lookup without
/// carrying a username or password.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct HttpProtectionSpace {
    /// URL whose request provoked the challenge.
    pub origin_url: String,
    pub host: String,
    pub port: u16,
    pub realm: Option<String>,
    /// Authentication scheme token, normalized to ASCII lowercase.
    pub scheme: String,
    pub is_proxy: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpAuthenticationChallenge {
    pub id: UserAgentRequestId,
    pub protection_space: HttpProtectionSpace,
}

/// A credential-provider result. Deliberately neither serializable nor
/// printable: secrets may cross the answer call but cannot enter events,
/// facets, traces, or persisted registries through a derive.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpCredentials {
    pub username: String,
    pub password: String,
}

impl fmt::Debug for HttpCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpCredentials")
            .field("username", &"<redacted>")
            .field("password", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum HttpAuthenticationAnswer {
    Credentials(HttpCredentials),
    Cancel,
}

impl fmt::Debug for HttpAuthenticationAnswer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Credentials(_) => f.write_str("Credentials(<redacted>)"),
            Self::Cancel => f.write_str("Cancel"),
        }
    }
}

// ── Errors ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SurfaceError {
    EngineNotFound(String),
    SpawnFailed(String),
    NavigationFailed(String),
    InputFailed(String),
    FrameAcquisitionFailed(String),
    Unsupported(String),
}

impl fmt::Display for SurfaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EngineNotFound(id) => write!(f, "surface engine not registered: {id}"),
            Self::SpawnFailed(reason) => write!(f, "spawn failed: {reason}"),
            Self::NavigationFailed(reason) => write!(f, "navigation failed: {reason}"),
            Self::InputFailed(reason) => write!(f, "input failed: {reason}"),
            Self::FrameAcquisitionFailed(reason) => write!(f, "frame acquisition: {reason}"),
            Self::Unsupported(reason) => write!(f, "unsupported: {reason}"),
        }
    }
}

impl std::error::Error for SurfaceError {}

// ── Spawn request ──────────────────────────────────────────────────────────

/// Persona/session binding passed to the surface engine at spawn time.
///
/// The host resolves `user_data_dir` from persona + graph context before
/// constructing the request. The engine plumbs it to the producer's data-store
/// config (e.g. `WebView2CompositionConfig::user_data_dir` on Windows).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineProfileBinding {
    pub user_data_dir: String,
}

/// Input to [`SurfaceEngine::spawn`].
///
/// Bypasses the inker fetch path entirely — the underlying WebView manages
/// its own HTTP stack; there is no raw body to hand in.
#[derive(Clone, Debug)]
pub struct SurfaceSpawnRequest {
    pub url: String,
    pub width: u32,
    pub height: u32,
    pub profile: EngineProfileBinding,
    /// Platform fence share-handle for explicit GPU sync. `None` falls back
    /// to the producer's barrier/cache path. Windows: D3D12 fence HANDLE cast
    /// to u64. Other platforms: reserved.
    pub fence_handle: Option<u64>,
}

// ── Frame vocabulary ───────────────────────────────────────────────────────

/// Platform-specific texture handle emitted by [`SurfaceProducer::acquire_frame`].
#[non_exhaustive]
#[derive(Debug)]
pub enum NativeTextureHandle {
    /// Windows: D3D12 shared texture HANDLE cast to u64.
    ///
    /// The host must obey [`FrameHandleOwnership`]. A `Transferred` handle is
    /// closed by the host after `OpenSharedHandle`; a `Borrowed` handle remains
    /// the producer's property and must not be closed by the host. A zero handle
    /// means "reuse the resource already imported for this `resource_epoch`";
    /// it is invalid for a new epoch. Cross-API shared textures use D3D12's
    /// simultaneous-access rules. The host imports them from
    /// `D3D12_RESOURCE_STATE_COMMON` and must finish sampling and return them to
    /// COMMON before it asks the producer for another frame.
    D3d12Shared {
        handle: u64,
        ownership: FrameHandleOwnership,
    },
    /// macOS: IOSurface ref (opaque u64; downcast on the host side).
    IoSurface(u64),
    /// Linux: DMA-BUF fd (negative means absent/invalid).
    DmaBuf(i64),
}

/// Who closes a native handle carried in a [`SurfaceFrame`].
///
/// Frame handles cannot all use the same rule: a system-webview producer may
/// retain and reuse its shared handle across paints, while Weld deliberately
/// copies CEF's callback-scoped texture into a fresh application-owned handle
/// and transfers that handle to the host. This is explicit so a generic host
/// cannot either leak the latter or close the former underneath its producer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameHandleOwnership {
    /// The producer owns the handle for the lifetime of this resource epoch.
    Borrowed,
    /// The host owns this one-shot handle and closes it after importing.
    Transferred,
}

/// Pixel format for a native surface frame.
///
/// Inker deliberately models the small cross-platform texture vocabulary
/// rather than depending on wgpu. Hosts map the known variants to their GPU
/// API and must reject `Other` until they add an explicit import mapping.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SurfaceTextureFormat {
    Rgba8Unorm,
    Rgba8UnormSrgb,
    Bgra8Unorm,
    Bgra8UnormSrgb,
    Other(String),
}

/// Synchronization handle accompanying a [`SurfaceFrame`].
#[non_exhaustive]
#[derive(Debug)]
pub enum SurfaceSyncHandle {
    /// Windows: D3D12 fence + signal value. The fence HANDLE is borrowed; the
    /// producer must keep it valid until the producer is dropped. A host may
    /// open and retain its own COM reference instead of reopening it per frame.
    D3d12Fence { handle: u64, value: u64 },
    /// Synchronization already complete before the handle was emitted.
    None,
}

/// A composited frame from a surface producer.
///
/// `texture` is a raw platform handle, not a `wgpu::Texture` — the host imports it
/// on its own device, which is what keeps inker wgpu-free. Because importing a
/// shared handle every frame is wasteful (and some producers, e.g. WebView2, reuse
/// one allocation and overwrite it in place), [`resource_epoch`](Self::resource_epoch)
/// lets the host import once and re-sample.
#[derive(Debug)]
pub struct SurfaceFrame {
    pub texture: NativeTextureHandle,
    pub sync: SurfaceSyncHandle,
    pub width: u32,
    pub height: u32,
    /// Exact pixel format of `texture`. The host must use this value when it
    /// creates or imports the GPU resource; assuming BGRA silently corrupts
    /// RGBA CEF frames.
    pub format: SurfaceTextureFormat,
    /// Monotonic generation of the underlying GPU allocation. Bumps when the
    /// producer (re)allocates the shared resource (first frame, resize, realloc);
    /// stays constant while it overwrites the same allocation in place. The host's
    /// import cache keys on this: re-import (releasing the previous handle) when it
    /// changes, re-sample the already-imported texture when it doesn't. This is the
    /// import-once signal a type-erased producer would otherwise lose (the reason
    /// scrying once held its concrete producer outside the registry).
    pub resource_epoch: u64,
}

// ── Input vocabulary ───────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhysicalPosition {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    Back,
    Forward,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum MouseEventKind {
    Moved,
    Pressed,
    Released,
    ScrollPixels { delta_x: f32, delta_y: f32 },
    ScrollLines { delta_x: f32, delta_y: f32 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MouseEvent {
    pub position: PhysicalPosition,
    pub button: Option<MouseButton>,
    pub kind: MouseEventKind,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyboardModifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

/// The Pointer Events `pointerType` values. `Unknown` represents the empty
/// string or a device kind the host cannot identify; it must not be guessed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PointerType {
    Mouse,
    Pen,
    Touch,
    Unknown,
}

/// Native contact phases from which the engine fires DOM Pointer Events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PointerPhase {
    Down,
    Move,
    Up,
    Cancel,
}

/// Pointer Events `buttons` bitmask. Values follow the DOM allocation rather
/// than a backend's native flag values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PointerButtons(pub u16);

impl PointerButtons {
    pub const NONE: Self = Self(0);
    pub const PRIMARY: Self = Self(1);
    pub const SECONDARY: Self = Self(2);
    pub const AUXILIARY: Self = Self(4);
    pub const BACK: Self = Self(8);
    pub const FORWARD: Self = Self(16);

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for PointerButtons {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

/// Hardware-neutral input following the W3C Pointer Events fields. Optional
/// sensor values distinguish "not reported by the host" from a real zero;
/// the engine applies the specification's DOM defaults when it dispatches.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PointerEvent {
    pub pointer_id: i32,
    pub pointer_type: PointerType,
    pub is_primary: bool,
    pub phase: PointerPhase,
    pub position: PhysicalPosition,
    /// The button whose state changed for Down/Up, if any.
    pub button: Option<MouseButton>,
    /// All buttons held after this native event.
    pub buttons: PointerButtons,
    /// Contact geometry in CSS-pixel shape, expressed here in physical pixels
    /// at the host boundary. Engines perform their normal device-scale mapping.
    pub width: f32,
    pub height: f32,
    pub pressure: Option<f32>,
    pub tangential_pressure: Option<f32>,
    /// Degrees in the Pointer Events `-90..=90` convention.
    pub tilt_x: Option<f32>,
    pub tilt_y: Option<f32>,
    /// Clockwise degrees in `0..=359`.
    pub twist: Option<f32>,
    /// Radians in the Pointer Events conventions.
    pub altitude_angle: Option<f32>,
    pub azimuth_angle: Option<f32>,
    pub modifiers: KeyboardModifiers,
}

/// Standard drag effects shared by HTML `DataTransfer.effectAllowed` and the
/// host toolkit. Backend-only effects do not enter this mask.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DragOperationSet(pub u8);

impl DragOperationSet {
    pub const NONE: Self = Self(0);
    pub const COPY: Self = Self(1);
    pub const LINK: Self = Self(2);
    pub const MOVE: Self = Self(4);

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for DragOperationSet {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

/// One HTML `DataTransferItem`. The enum makes the standard `kind` value
/// (`string` or `file`) structural, while `mime_type` carries its lowercase
/// type string. A file path is host-private transport data, not page-visible
/// identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataTransferItem {
    String {
        mime_type: String,
        data: String,
    },
    File {
        mime_type: String,
        path: std::path::PathBuf,
        display_name: Option<String>,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataTransfer {
    pub items: Vec<DataTransferItem>,
    pub allowed_operations: DragOperationSet,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DragPhase {
    Enter,
    Over,
    Leave,
    Drop,
}

/// One host-to-page HTML drag lifecycle event. HTML exposes one `DataTransfer`
/// throughout the drag; engines may only materialize its native payload on
/// Enter, but the allowed effects remain available for every phase.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DragEvent {
    pub phase: DragPhase,
    pub position: PhysicalPosition,
    pub modifiers: KeyboardModifiers,
    /// DOM `MouseEvent.buttons` bitfield inherited by `DragEvent`.
    pub buttons: PointerButtons,
    pub data_transfer: DataTransfer,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeyboardEvent {
    /// Host-framework virtual key code (gpui key code on the main path).
    pub key_code: u32,
    /// Hardware scan code; zero when absent.
    pub scan_code: u32,
    pub modifiers: KeyboardModifiers,
    pub pressed: bool,
    /// Composed text for printable keys; `None` for non-printable and key-up.
    pub text: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FocusReason {
    Mouse,
    Tab,
    ShiftTab,
    Programmatic,
}

// ── Producer event vocabulary ──────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NavigationEvent {
    Started { url: String },
    Committed { url: String },
    Finished { url: String, title: Option<String> },
    Failed { url: String, reason: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CursorShape {
    Default,
    Text,
    Pointer,
    Grab,
    Grabbing,
    Crosshair,
    Move,
    ResizeNs,
    ResizeEw,
    ResizeNesw,
    ResizeNwse,
    NotAllowed,
    Hidden,
}

/// A message posted from the page via the JS bridge (postMessage-style).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebMessage {
    pub tag: String,
    pub payload: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SameSite {
    Strict,
    Lax,
    None,
}

/// HTTP cookie payload used at the generic web-surface boundary.
///
/// This mirrors the engine-agnostic verso cookie shape so a compatibility flip
/// does not lose cookie metadata before it reaches a concrete web backend.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: Option<SameSite>,
    pub expires: Option<f64>,
    pub partitioned: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WebFeatureStatus {
    Supported,
    Unsupported { reason: String },
    Partial { detail: String },
}

impl WebFeatureStatus {
    pub fn unsupported(reason: impl Into<String>) -> Self {
        Self::Unsupported {
            reason: reason.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WebFrameTransportMode {
    ImportedTexture,
    NativeChildOverlay,
    CpuSnapshot,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CookieAttributeCapabilities {
    pub same_site: WebFeatureStatus,
    pub partitioned: WebFeatureStatus,
    pub http_only: WebFeatureStatus,
    pub secure: WebFeatureStatus,
    pub expires: WebFeatureStatus,
}

impl Default for CookieAttributeCapabilities {
    fn default() -> Self {
        Self {
            same_site: WebFeatureStatus::unsupported("cookie SameSite support is unknown"),
            partitioned: WebFeatureStatus::unsupported("partitioned cookie support is unknown"),
            http_only: WebFeatureStatus::unsupported("HttpOnly cookie support is unknown"),
            secure: WebFeatureStatus::unsupported("secure cookie support is unknown"),
            expires: WebFeatureStatus::unsupported("cookie expiry support is unknown"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CookieCapabilities {
    pub read: WebFeatureStatus,
    pub write: WebFeatureStatus,
    pub delete: WebFeatureStatus,
    pub change_events: WebFeatureStatus,
    pub attributes: CookieAttributeCapabilities,
}

impl Default for CookieCapabilities {
    fn default() -> Self {
        Self {
            read: WebFeatureStatus::unsupported("cookie reads are not wired"),
            write: WebFeatureStatus::unsupported("cookie writes are not wired"),
            delete: WebFeatureStatus::unsupported("cookie deletes are not wired"),
            change_events: WebFeatureStatus::unsupported("cookie change events are not wired"),
            attributes: CookieAttributeCapabilities::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptCapabilities {
    pub execute: WebFeatureStatus,
    pub result: WebFeatureStatus,
    pub exceptions: WebFeatureStatus,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PointerInputCapabilities {
    pub mouse: WebFeatureStatus,
    pub pen: WebFeatureStatus,
    pub touch: WebFeatureStatus,
    pub contact_geometry: WebFeatureStatus,
    pub pressure: WebFeatureStatus,
    pub tangential_pressure: WebFeatureStatus,
    pub tilt: WebFeatureStatus,
    pub twist: WebFeatureStatus,
    pub altitude_azimuth: WebFeatureStatus,
}

impl Default for PointerInputCapabilities {
    fn default() -> Self {
        Self {
            mouse: WebFeatureStatus::unsupported("pointer mouse input is not wired"),
            pen: WebFeatureStatus::unsupported("pen input is not wired"),
            touch: WebFeatureStatus::unsupported("touch input is not wired"),
            contact_geometry: WebFeatureStatus::unsupported("contact geometry is not wired"),
            pressure: WebFeatureStatus::unsupported("pointer pressure is not wired"),
            tangential_pressure: WebFeatureStatus::unsupported("tangential pressure is not wired"),
            tilt: WebFeatureStatus::unsupported("pointer tilt is not wired"),
            twist: WebFeatureStatus::unsupported("pointer twist is not wired"),
            altitude_azimuth: WebFeatureStatus::unsupported(
                "pointer altitude/azimuth are not wired",
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DragDropCapabilities {
    pub host_to_page: WebFeatureStatus,
    pub page_to_host: WebFeatureStatus,
    pub file_items: WebFeatureStatus,
    pub string_items: WebFeatureStatus,
}

impl Default for DragDropCapabilities {
    fn default() -> Self {
        Self {
            host_to_page: WebFeatureStatus::unsupported("host-to-page drag is not wired"),
            page_to_host: WebFeatureStatus::unsupported("page-to-host drag is not wired"),
            file_items: WebFeatureStatus::unsupported("dragged files are not wired"),
            string_items: WebFeatureStatus::unsupported("dragged strings are not wired"),
        }
    }
}

/// Runtime feature descriptor for web-surface capabilities that vary by
/// backend instance rather than by the Rust type alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebSurfaceCapabilities {
    pub backend_name: String,
    pub backend_version: Option<String>,
    pub frame_transport: WebFrameTransportMode,
    pub cookie: CookieCapabilities,
    pub script: ScriptCapabilities,
    pub pointer: PointerInputCapabilities,
    pub find_in_page: WebFeatureStatus,
    pub pdf: WebFeatureStatus,
    pub downloads: WebFeatureStatus,
    pub devtools: WebFeatureStatus,
    pub popups: WebFeatureStatus,
    pub permissions: WebFeatureStatus,
    pub auth: WebFeatureStatus,
    pub context_menus: WebFeatureStatus,
    pub drag_drop: DragDropCapabilities,
    pub ime_observability: WebFeatureStatus,
    pub accessibility: WebFeatureStatus,
    pub snapshot: WebFeatureStatus,
    pub degradation_reasons: Vec<String>,
}

impl Default for WebSurfaceCapabilities {
    fn default() -> Self {
        Self {
            backend_name: "unknown".into(),
            backend_version: None,
            frame_transport: WebFrameTransportMode::Unsupported,
            cookie: CookieCapabilities::default(),
            script: ScriptCapabilities {
                execute: WebFeatureStatus::unsupported("script execution is not wired"),
                result: WebFeatureStatus::unsupported("script results are not wired"),
                exceptions: WebFeatureStatus::unsupported(
                    "script exception reporting is not wired",
                ),
            },
            pointer: PointerInputCapabilities::default(),
            find_in_page: WebFeatureStatus::unsupported("find in page is not wired"),
            pdf: WebFeatureStatus::unsupported("PDF handling is not wired"),
            downloads: WebFeatureStatus::unsupported("download handling is not wired"),
            devtools: WebFeatureStatus::unsupported("devtools are not wired"),
            popups: WebFeatureStatus::unsupported("popup routing is not wired"),
            permissions: WebFeatureStatus::unsupported("permission prompts are not wired"),
            auth: WebFeatureStatus::unsupported("auth prompts are not wired"),
            context_menus: WebFeatureStatus::unsupported("context menu events are not wired"),
            drag_drop: DragDropCapabilities::default(),
            ime_observability: WebFeatureStatus::unsupported("IME observability is not wired"),
            accessibility: WebFeatureStatus::unsupported("surface accessibility is opaque"),
            snapshot: WebFeatureStatus::unsupported("snapshots are not wired"),
            degradation_reasons: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum WebSurfaceEvent {
    Navigation(NavigationEvent),
    TitleChanged {
        title: String,
    },
    AddressChanged {
        url: String,
    },
    LoadProgress {
        value: f32,
    },
    ConsoleMessage {
        level: String,
        text: String,
        source: Option<String>,
        line: Option<u32>,
    },
    ScriptException {
        text: String,
        source: Option<String>,
        line: Option<u32>,
    },
    PermissionRequested(PermissionRequest),
    AuthenticationRequested(HttpAuthenticationChallenge),
    DownloadRequested {
        url: String,
        suggested_name: Option<String>,
    },
    NewWindowRequested {
        url: String,
    },
    ContextMenuRequested {
        x: f64,
        y: f64,
        link_url: Option<String>,
        image_url: Option<String>,
    },
    /// A page began a drag. The host owns placement and the native drag loop;
    /// it must eventually answer through `finish_drag_source`.
    PageDragStarted {
        data_transfer: DataTransfer,
        position: PhysicalPosition,
    },
    CookieStoreChanged,
    ProcessCrashed {
        reason: String,
    },
    BackendDiagnostic {
        severity: String,
        message: String,
    },
    WebMessage(WebMessage),
}

// ── Settings ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfaceSettings {
    /// Background fill color (RGBA). Informs pre-composited transparency.
    pub background_color: [u8; 4],
    /// Zoom factor (1.0 = 100 %).
    pub zoom_factor: f64,
    pub dev_tools: bool,
}

impl Default for SurfaceSettings {
    fn default() -> Self {
        Self {
            background_color: [255, 255, 255, 255],
            zoom_factor: 1.0,
            dev_tools: false,
        }
    }
}

// ── Traits ─────────────────────────────────────────────────────────────────

/// Factory for [`SurfaceProducer`] instances.
///
/// Parallel to [`crate::Engine`] for surface-producing engines. A single
/// `SurfaceEngine` may spawn many producers (one per tile).
pub trait SurfaceEngine: Send + Sync {
    /// Stable engine identifier. Must match the `engine_id` of the
    /// [`EngineRouteDecision`] that selected this engine.
    fn engine_id(&self) -> &str;

    /// Spawn a new producer for the given request.
    fn spawn(
        &self,
        request: &SurfaceSpawnRequest,
    ) -> Result<Box<dyn SurfaceProducer>, SurfaceError>;

    /// This surface's accessibility capability (see [`crate::a11y`]).
    /// Frame-streaming surfaces default to [`A11yCapability::Opaque`] — a raw
    /// GPU frame / system WebView has no semantics the host can read. A surface
    /// that *bridges* its content (e.g. scrying's DOM bridge) overrides this to
    /// declare [`A11yCapability::Partial`], per the non-silent-degradation rule.
    fn a11y_capability(&self) -> A11yCapability {
        A11yCapability::Opaque
    }
}

/// Long-lived surface producer. Owns a WebView control until dropped.
///
/// All methods take `&mut self`: the producer is single-owner, driven
/// sequentially by the host's render loop. Input flows in through `send_*`
/// and `move_focus`; output flows out through `acquire_frame` and `poll_*`.
///
/// Not `Send`: producers may be STA-bound (Windows WebView2 COM) or
/// main-thread-only (macOS WKWebView, gpui main thread). The host drives them
/// from a single thread per producer.
pub trait SurfaceProducer {
    // ── Layout ──────────────────────────────────────────────────────────────
    fn resize(&mut self, width: u32, height: u32) -> Result<(), SurfaceError>;
    fn set_offset(&mut self, x: i32, y: i32) -> Result<(), SurfaceError>;

    // ── Frame acquisition ────────────────────────────────────────────────────
    fn acquire_frame(&mut self) -> Result<Option<SurfaceFrame>, SurfaceError>;

    // ── Input ────────────────────────────────────────────────────────────────
    fn send_mouse_input(&mut self, ev: MouseEvent) -> Result<(), SurfaceError>;
    fn send_pointer_input(&mut self, ev: PointerEvent) -> Result<(), SurfaceError>;
    fn send_drag_input(&mut self, _ev: DragEvent) -> Result<(), SurfaceError> {
        Err(SurfaceError::Unsupported(
            "host-to-page drag input is not wired for this surface".into(),
        ))
    }
    fn finish_drag_source(
        &mut self,
        _position: PhysicalPosition,
        _operation: DragOperationSet,
    ) -> Result<(), SurfaceError> {
        Err(SurfaceError::Unsupported(
            "page-to-host drag completion is not wired for this surface".into(),
        ))
    }
    fn send_keyboard_input(&mut self, ev: KeyboardEvent) -> Result<(), SurfaceError>;
    fn move_focus(&mut self, reason: FocusReason) -> Result<(), SurfaceError>;

    // ── Events ───────────────────────────────────────────────────────────────
    fn poll_cursor_shape(&mut self) -> Option<CursorShape>;

    // ── Settings ─────────────────────────────────────────────────────────────
    fn apply_settings(&mut self, settings: &SurfaceSettings) -> Result<(), SurfaceError>;

    // ── Snapshot ─────────────────────────────────────────────────────────────
    fn capture_snapshot_png(&mut self) -> Result<Vec<u8>, SurfaceError>;

    // ── Optional web control plane ───────────────────────────────────────────
    fn as_web_surface(&mut self) -> Option<&mut dyn WebSurface> {
        None
    }
}

/// Web-specific control plane layered over the raw surface transport.
///
/// Navigation methods start work and return promptly. Completion and every
/// other web-facing notification are observed through one ordered event
/// stream from the driving frame loop. Consumers must drain
/// [`WebSurface::poll_web_event`] directly: filtered convenience pollers can
/// discard intervening events and therefore are deliberately not part of this
/// contract.
pub trait WebSurface: SurfaceProducer {
    fn capabilities(&self) -> WebSurfaceCapabilities {
        WebSurfaceCapabilities::default()
    }

    // ── Navigation ───────────────────────────────────────────────────────────
    fn navigate_to_url(&mut self, url: &str) -> Result<(), SurfaceError>;
    fn navigate_to_string(&mut self, html: &str) -> Result<(), SurfaceError>;
    fn reload(&mut self) -> Result<(), SurfaceError>;
    fn stop(&mut self) -> Result<(), SurfaceError>;
    fn go_back(&mut self) -> Result<(), SurfaceError>;
    fn go_forward(&mut self) -> Result<(), SurfaceError>;
    fn can_go_back(&self) -> bool;
    fn can_go_forward(&self) -> bool;

    // ── Session/script/events ────────────────────────────────────────────────
    fn set_cookie(&mut self, cookie: &Cookie) -> Result<(), SurfaceError>;
    fn get_cookies_for_url(&mut self, url: &str) -> Result<Vec<Cookie>, SurfaceError> {
        let _ = url;
        Err(SurfaceError::Unsupported(
            "cookie reads are not wired for this web surface".into(),
        ))
    }
    fn delete_cookie(&mut self, cookie: &Cookie) -> Result<(), SurfaceError> {
        let _ = cookie;
        Err(SurfaceError::Unsupported(
            "cookie delete is not wired for this web surface".into(),
        ))
    }
    /// Answer a held Permissions API request emitted on this surface.
    fn answer_permission(
        &mut self,
        _id: UserAgentRequestId,
        _answer: PermissionAnswer,
    ) -> Result<(), SurfaceError> {
        Err(SurfaceError::Unsupported(
            "permission answers are not wired for this web surface".into(),
        ))
    }
    /// Answer or cancel a held RFC 9110 authentication challenge.
    fn answer_http_authentication(
        &mut self,
        _id: UserAgentRequestId,
        _answer: &HttpAuthenticationAnswer,
    ) -> Result<(), SurfaceError> {
        Err(SurfaceError::Unsupported(
            "authentication answers are not wired for this web surface".into(),
        ))
    }
    fn execute_script_with_result(&mut self, script: &str) -> Result<String, SurfaceError>;
    /// Return the next event in producer order.
    ///
    /// An implementation must not inspect and discard events of another kind
    /// while answering this call. Asynchronous commands will add correlation
    /// ids to this stream as their shared contracts are introduced.
    fn poll_web_event(&mut self) -> Option<WebSurfaceEvent> {
        None
    }
}

// ── Registry ───────────────────────────────────────────────────────────────

/// Engine ID → `SurfaceEngine` instance dispatch. Parallel to
/// [`crate::EngineRegistry`] for the surface dispatch path.
#[derive(Default)]
pub struct SurfaceEngineRegistry {
    engines: HashMap<String, Box<dyn SurfaceEngine>>,
}

impl SurfaceEngineRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, engine: Box<dyn SurfaceEngine>) {
        let id = engine.engine_id().to_string();
        self.engines.insert(id, engine);
    }

    pub fn engine(&self, id: &str) -> Option<&dyn SurfaceEngine> {
        self.engines.get(id).map(|e| e.as_ref())
    }

    pub fn contains(&self, id: &str) -> bool {
        self.engines.contains_key(id)
    }

    pub fn engine_ids(&self) -> impl Iterator<Item = &str> {
        self.engines.keys().map(String::as_str)
    }

    /// Spawn a producer using the engine selected by `decision.engine_id`.
    #[tracing::instrument(
        level = "debug",
        skip(self, decision, request),
        fields(engine_id = %decision.engine_id, url = %request.url),
    )]
    pub fn spawn(
        &self,
        decision: &EngineRouteDecision,
        request: &SurfaceSpawnRequest,
    ) -> Result<Box<dyn SurfaceProducer>, SurfaceError> {
        let engine = self.engine(&decision.engine_id).ok_or_else(|| {
            tracing::warn!(
                engine_id = %decision.engine_id,
                "surface engine not registered"
            );
            SurfaceError::EngineNotFound(decision.engine_id.clone())
        })?;
        engine.spawn(request)
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::routing::{SurfaceContract, SurfaceContractMode, SurfaceTargetId};

    struct StubProducer;

    impl SurfaceProducer for StubProducer {
        fn resize(&mut self, _: u32, _: u32) -> Result<(), SurfaceError> {
            Ok(())
        }
        fn set_offset(&mut self, _: i32, _: i32) -> Result<(), SurfaceError> {
            Ok(())
        }
        fn acquire_frame(&mut self) -> Result<Option<SurfaceFrame>, SurfaceError> {
            Ok(None)
        }
        fn send_mouse_input(&mut self, _: MouseEvent) -> Result<(), SurfaceError> {
            Ok(())
        }
        fn send_pointer_input(&mut self, _: PointerEvent) -> Result<(), SurfaceError> {
            Ok(())
        }
        fn send_keyboard_input(&mut self, _: KeyboardEvent) -> Result<(), SurfaceError> {
            Ok(())
        }
        fn move_focus(&mut self, _: FocusReason) -> Result<(), SurfaceError> {
            Ok(())
        }
        fn poll_cursor_shape(&mut self) -> Option<CursorShape> {
            None
        }
        fn apply_settings(&mut self, _: &SurfaceSettings) -> Result<(), SurfaceError> {
            Ok(())
        }
        fn capture_snapshot_png(&mut self) -> Result<Vec<u8>, SurfaceError> {
            Err(SurfaceError::Unsupported("stub".into()))
        }
    }

    struct StubSurfaceEngine;

    impl SurfaceEngine for StubSurfaceEngine {
        fn engine_id(&self) -> &str {
            "test.surface"
        }
        fn spawn(&self, _: &SurfaceSpawnRequest) -> Result<Box<dyn SurfaceProducer>, SurfaceError> {
            Ok(Box::new(StubProducer))
        }
    }

    struct EventQueueSurface {
        events: VecDeque<WebSurfaceEvent>,
    }

    impl SurfaceProducer for EventQueueSurface {
        fn resize(&mut self, _: u32, _: u32) -> Result<(), SurfaceError> {
            Ok(())
        }
        fn set_offset(&mut self, _: i32, _: i32) -> Result<(), SurfaceError> {
            Ok(())
        }
        fn acquire_frame(&mut self) -> Result<Option<SurfaceFrame>, SurfaceError> {
            Ok(None)
        }
        fn send_mouse_input(&mut self, _: MouseEvent) -> Result<(), SurfaceError> {
            Ok(())
        }
        fn send_pointer_input(&mut self, _: PointerEvent) -> Result<(), SurfaceError> {
            Ok(())
        }
        fn send_keyboard_input(&mut self, _: KeyboardEvent) -> Result<(), SurfaceError> {
            Ok(())
        }
        fn move_focus(&mut self, _: FocusReason) -> Result<(), SurfaceError> {
            Ok(())
        }
        fn poll_cursor_shape(&mut self) -> Option<CursorShape> {
            None
        }
        fn apply_settings(&mut self, _: &SurfaceSettings) -> Result<(), SurfaceError> {
            Ok(())
        }
        fn capture_snapshot_png(&mut self) -> Result<Vec<u8>, SurfaceError> {
            Err(SurfaceError::Unsupported("stub".into()))
        }
        fn as_web_surface(&mut self) -> Option<&mut dyn WebSurface> {
            Some(self)
        }
    }

    impl WebSurface for EventQueueSurface {
        fn navigate_to_url(&mut self, _: &str) -> Result<(), SurfaceError> {
            Ok(())
        }
        fn navigate_to_string(&mut self, _: &str) -> Result<(), SurfaceError> {
            Ok(())
        }
        fn reload(&mut self) -> Result<(), SurfaceError> {
            Ok(())
        }
        fn stop(&mut self) -> Result<(), SurfaceError> {
            Ok(())
        }
        fn go_back(&mut self) -> Result<(), SurfaceError> {
            Ok(())
        }
        fn go_forward(&mut self) -> Result<(), SurfaceError> {
            Ok(())
        }
        fn can_go_back(&self) -> bool {
            false
        }
        fn can_go_forward(&self) -> bool {
            false
        }
        fn set_cookie(&mut self, _: &Cookie) -> Result<(), SurfaceError> {
            Ok(())
        }
        fn execute_script_with_result(&mut self, _: &str) -> Result<String, SurfaceError> {
            Ok(String::new())
        }
        fn poll_web_event(&mut self) -> Option<WebSurfaceEvent> {
            self.events.pop_front()
        }
    }

    fn decision(id: &str) -> EngineRouteDecision {
        EngineRouteDecision {
            engine_id: id.to_string(),
            surface_contract: SurfaceContract {
                target: SurfaceTargetId::new("test:1"),
                mode: SurfaceContractMode::CompositedTexture,
            },
        }
    }

    fn stub_request() -> SurfaceSpawnRequest {
        SurfaceSpawnRequest {
            url: "https://example.com".into(),
            width: 800,
            height: 600,
            profile: EngineProfileBinding {
                user_data_dir: "/tmp/test-profile".into(),
            },
            fence_handle: None,
        }
    }

    #[test]
    fn registry_contains_registered_engine() {
        let mut reg = SurfaceEngineRegistry::new();
        reg.register(Box::new(StubSurfaceEngine));
        assert!(reg.contains("test.surface"));
        assert!(!reg.contains("absent.engine"));
    }

    #[test]
    fn registry_spawns_registered_engine() {
        let mut reg = SurfaceEngineRegistry::new();
        reg.register(Box::new(StubSurfaceEngine));
        // `Box<dyn SurfaceProducer>` doesn't implement Debug, so avoid .expect()
        assert!(
            reg.spawn(&decision("test.surface"), &stub_request())
                .is_ok()
        );
    }

    #[test]
    fn registry_reports_missing_engine() {
        let reg = SurfaceEngineRegistry::new();
        let result = reg.spawn(&decision("absent.engine"), &stub_request());
        assert!(matches!(result, Err(SurfaceError::EngineNotFound(_))));
    }

    #[test]
    fn mixed_web_events_are_observed_once_in_producer_order() {
        let expected = vec![
            WebSurfaceEvent::TitleChanged {
                title: "First".into(),
            },
            WebSurfaceEvent::Navigation(NavigationEvent::Committed {
                url: "https://example.com/next".into(),
            }),
            WebSurfaceEvent::WebMessage(WebMessage {
                tag: "receipt".into(),
                payload: "done".into(),
            }),
        ];
        let mut surface = EventQueueSurface {
            events: expected.clone().into(),
        };
        let mut observed = Vec::new();
        while let Some(event) = surface.poll_web_event() {
            observed.push(event);
        }
        assert_eq!(observed, expected);
    }
}
