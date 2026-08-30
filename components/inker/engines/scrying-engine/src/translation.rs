// Copyright 2026 Mark AB (markik)
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Type translations between `inker` surface-engine types and the
//! `scrying` producer trait's vocabulary.
//!
//! Inker stays portable to wasm32 / browser targets so its event types are
//! a portable subset. Scrying is platform-bound and richer (drag input,
//! download events, accelerator keys, etc.). These functions are the bridge:
//! they map the inker subset into scrying's superset on the way in (input
//! forwarding) and lossy-map scrying's events to inker's subset on the way
//! out (host polling). Events that have no inker equivalent are dropped at
//! the boundary today; a richer host can downcast to the concrete scrying
//! producer for the full event stream.

use inker::{
    Cookie as InkerCookie, CookieAttributeCapabilities, CookieCapabilities,
    CursorShape as InkerCursorShape, DocumentCapabilities, DragDropCapabilities,
    FocusReason as InkerFocusReason, FrameHandleOwnership, KeyboardEvent as InkerKeyboardEvent,
    KeyboardModifiers as InkerKeyboardModifiers, MouseButton as InkerMouseButton,
    MouseEvent as InkerMouseEvent, MouseEventKind as InkerMouseEventKind, NativeTextureHandle,
    NavigationEvent as InkerNavEvent, PointerButtons, PointerEvent as InkerPointerEvent,
    PointerInputCapabilities, PointerPhase as InkerPointerPhase, PointerType as InkerPointerType,
    SameSite as InkerSameSite, ScriptCapabilities, SurfaceError, SurfaceFrame, SurfaceSettings,
    SurfaceSyncHandle, SurfaceTextureFormat, WebFeatureStatus, WebFrameTransportMode, WebMessage,
    WebSurfaceCapabilities as InkerWebSurfaceCapabilities, WebSurfaceEvent,
};
use scrying::{
    CapabilityStatus as ScryingCapabilityStatus, CursorShape as ScryingCursorShape,
    FocusReason as ScryingFocusReason, KeyEventKind as ScryingKeyEventKind,
    KeyModifierFlags as ScryingKeyModifierFlags, KeyboardInput as ScryingKeyboardInput,
    MouseEventKind as ScryingMouseEventKind, MouseInput as ScryingMouseInput,
    MouseVirtualKeys as ScryingMouseVirtualKeys, NavigationEvent as ScryingNavEvent,
    PointerDevice as ScryingPointerDevice, PointerEventKind as ScryingPointerEventKind,
    PointerInput as ScryingPointerInput, SameSite as ScryingSameSite,
    SystemWebviewBackend as ScryingBackend, WebSurfaceCapabilities as ScryingCapabilities,
    WebSurfaceError, WebSurfaceFrame, WebSurfaceMode as ScryingSurfaceMode, WebSurfaceSettings,
    native_frame::NativeFrame as ScryingNativeFrame, native_frame::SyncMechanism,
};

// ── Errors ────────────────────────────────────────────────────────────

pub fn map_error(err: WebSurfaceError) -> SurfaceError {
    match err {
        WebSurfaceError::Unsupported(s) => SurfaceError::Unsupported(s.to_string()),
        WebSurfaceError::NotReady(s) => SurfaceError::FrameAcquisitionFailed(s.to_string()),
        WebSurfaceError::Interop(e) => SurfaceError::FrameAcquisitionFailed(format!("{e}")),
        WebSurfaceError::Platform(s) => SurfaceError::SpawnFailed(s),
    }
}

// ── Frames ────────────────────────────────────────────────────────────

/// Map scrying's frame enum to inker's. Only native-texture variants produce
/// a `Some(SurfaceFrame)`; CPU snapshots and overlay-only frames return
/// `None` (the host has no inker-shaped path for them in v1).
pub fn map_frame(frame: WebSurfaceFrame, fence_handle: Option<u64>) -> Option<SurfaceFrame> {
    match frame {
        WebSurfaceFrame::Native(native) => map_native_frame(native, fence_handle),
        WebSurfaceFrame::CpuRgba { .. }
        | WebSurfaceFrame::PngSnapshot { .. }
        | WebSurfaceFrame::OverlayOnly => None,
        // `WebSurfaceFrame` is `#[non_exhaustive]`; unknown future variants
        // have no inker-shaped path until they're explicitly mapped.
        _ => None,
    }
}

fn map_native_frame(frame: ScryingNativeFrame, fence_handle: Option<u64>) -> Option<SurfaceFrame> {
    match frame {
        ScryingNativeFrame::Dx12SharedTexture(tex) => {
            #[cfg(target_os = "windows")]
            let handle = tex.handle as u64;
            #[cfg(not(target_os = "windows"))]
            let handle = 0_u64;
            let sync = match (tex.producer_sync, tex.fence_value, fence_handle) {
                (SyncMechanism::ExplicitFence, value, Some(handle)) if value > 0 => {
                    SurfaceSyncHandle::D3d12Fence { handle, value }
                },
                _ => SurfaceSyncHandle::None,
            };
            Some(SurfaceFrame {
                texture: NativeTextureHandle::D3d12Shared {
                    handle,
                    // WebView2CompositionFrame transfers the one NT handle
                    // emitted for each resource epoch to its consumer. Later
                    // frames reuse the imported resource and carry handle 0.
                    ownership: FrameHandleOwnership::Transferred,
                },
                sync,
                width: tex.size.width,
                height: tex.size.height,
                format: map_texture_format(tex.format),
                resource_epoch: tex.generation,
            })
        },
        ScryingNativeFrame::MetalTextureRef(tex) => {
            #[cfg(target_os = "macos")]
            let handle = tex.raw_metal_texture as u64;
            #[cfg(not(target_os = "macos"))]
            let handle = 0_u64;
            Some(SurfaceFrame {
                texture: NativeTextureHandle::IoSurface(handle),
                sync: SurfaceSyncHandle::None,
                width: tex.size.width,
                height: tex.size.height,
                format: map_texture_format(tex.format),
                resource_epoch: tex.generation,
            })
        },
        ScryingNativeFrame::DmaBufImage(img) => {
            let primary_fd = img.planes.first().map(|p| p.fd as i64).unwrap_or(-1);
            Some(SurfaceFrame {
                texture: NativeTextureHandle::DmaBuf(primary_fd),
                sync: SurfaceSyncHandle::None,
                width: img.size.width,
                height: img.size.height,
                format: map_texture_format(img.format),
                resource_epoch: img.generation,
            })
        },
        // `NativeFrame` is `#[non_exhaustive]`; future variants drop until mapped.
        _ => None,
    }
}

fn map_texture_format(format: wgpu::TextureFormat) -> SurfaceTextureFormat {
    match format {
        wgpu::TextureFormat::Rgba8Unorm => SurfaceTextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Rgba8UnormSrgb => SurfaceTextureFormat::Rgba8UnormSrgb,
        wgpu::TextureFormat::Bgra8Unorm => SurfaceTextureFormat::Bgra8Unorm,
        wgpu::TextureFormat::Bgra8UnormSrgb => SurfaceTextureFormat::Bgra8UnormSrgb,
        other => SurfaceTextureFormat::Other(format!("{other:?}")),
    }
}

// ── Cookies ───────────────────────────────────────────────────────────

pub fn map_cookie(cookie: &InkerCookie) -> scrying::Cookie {
    scrying::Cookie {
        name: cookie.name.clone(),
        value: cookie.value.clone(),
        domain: cookie.domain.clone(),
        path: cookie.path.clone(),
        expires_at: cookie.expires,
        is_secure: cookie.secure,
        is_http_only: cookie.http_only,
        same_site: cookie.same_site.map(map_same_site),
        partitioned: cookie.partitioned,
    }
}

fn map_same_site(same_site: InkerSameSite) -> ScryingSameSite {
    match same_site {
        InkerSameSite::Strict => ScryingSameSite::Strict,
        InkerSameSite::Lax => ScryingSameSite::Lax,
        InkerSameSite::None => ScryingSameSite::None,
    }
}

// ── Input ─────────────────────────────────────────────────────────────

pub fn map_mouse(ev: InkerMouseEvent) -> ScryingMouseInput {
    let (kind, mouse_data) = mouse_kind_to_scrying(ev.kind);
    ScryingMouseInput {
        kind,
        virtual_keys: mouse_buttons_to_scrying(ev.button),
        mouse_data,
        point: (ev.position.x as i32, ev.position.y as i32),
    }
}

fn mouse_kind_to_scrying(kind: InkerMouseEventKind) -> (ScryingMouseEventKind, i32) {
    match kind {
        InkerMouseEventKind::Moved => (ScryingMouseEventKind::Move, 0),
        InkerMouseEventKind::Pressed => (ScryingMouseEventKind::LeftButtonDown, 0),
        InkerMouseEventKind::Released => (ScryingMouseEventKind::LeftButtonUp, 0),
        InkerMouseEventKind::ScrollPixels { delta_y, .. } => {
            (ScryingMouseEventKind::Wheel, delta_y as i32)
        },
        InkerMouseEventKind::ScrollLines { delta_y, .. } => {
            // Convert lines to wheel deltas (120 per line, Win32 convention).
            (ScryingMouseEventKind::Wheel, (delta_y * 120.0) as i32)
        },
    }
}

fn mouse_buttons_to_scrying(button: Option<InkerMouseButton>) -> ScryingMouseVirtualKeys {
    let mut vk = ScryingMouseVirtualKeys::default();
    match button {
        Some(InkerMouseButton::Left) => vk.left_button = true,
        Some(InkerMouseButton::Middle) => vk.middle_button = true,
        Some(InkerMouseButton::Right) => vk.right_button = true,
        Some(InkerMouseButton::Back) => vk.x_button1 = true,
        Some(InkerMouseButton::Forward) => vk.x_button2 = true,
        None => {},
    }
    vk
}

pub fn map_pointer(ev: InkerPointerEvent) -> Result<ScryingPointerInput, SurfaceError> {
    let pointer_id = u32::try_from(ev.pointer_id).map_err(|_| {
        SurfaceError::InputFailed(format!(
            "Pointer Events pointerId must be non-negative for scrying: {}",
            ev.pointer_id
        ))
    })?;
    let kind = match ev.phase {
        InkerPointerPhase::Down => ScryingPointerEventKind::Down,
        InkerPointerPhase::Move => ScryingPointerEventKind::Update,
        InkerPointerPhase::Up => ScryingPointerEventKind::Up,
        InkerPointerPhase::Cancel => ScryingPointerEventKind::CaptureChanged,
    };
    let device = match ev.pointer_type {
        InkerPointerType::Mouse => ScryingPointerDevice::Mouse,
        InkerPointerType::Pen => ScryingPointerDevice::Pen,
        InkerPointerType::Touch => ScryingPointerDevice::Touch,
        InkerPointerType::Unknown => {
            return Err(SurfaceError::Unsupported(
                "scrying cannot dispatch an unidentified pointer type".into(),
            ));
        },
    };
    Ok(ScryingPointerInput {
        kind,
        device,
        pointer_id,
        point: (ev.position.x as i32, ev.position.y as i32),
        pressure: ev
            .pressure
            .unwrap_or(if ev.buttons != PointerButtons::NONE {
                0.5
            } else {
                0.0
            }),
        tilt: (
            ev.tilt_x.unwrap_or(0.0).to_radians(),
            ev.tilt_y.unwrap_or(0.0).to_radians(),
        ),
    })
}

pub fn map_keyboard(ev: InkerKeyboardEvent) -> ScryingKeyboardInput {
    ScryingKeyboardInput {
        kind: if ev.pressed {
            ScryingKeyEventKind::Down
        } else {
            ScryingKeyEventKind::Up
        },
        virtual_key_code: ev.key_code,
        characters: ev.text.clone().unwrap_or_default(),
        characters_ignoring_modifiers: ev.text.unwrap_or_default(),
        modifiers: map_modifiers(ev.modifiers),
        is_repeat: false,
    }
}

fn map_modifiers(m: InkerKeyboardModifiers) -> ScryingKeyModifierFlags {
    ScryingKeyModifierFlags {
        shift: m.shift,
        control: m.ctrl,
        alt: m.alt,
        meta: m.meta,
        caps_lock: false,
    }
}

pub fn map_focus_reason(reason: InkerFocusReason) -> ScryingFocusReason {
    match reason {
        InkerFocusReason::Mouse | InkerFocusReason::Programmatic => {
            ScryingFocusReason::Programmatic
        },
        InkerFocusReason::Tab => ScryingFocusReason::Next,
        InkerFocusReason::ShiftTab => ScryingFocusReason::Previous,
    }
}

// ── Settings ──────────────────────────────────────────────────────────

pub fn map_settings(s: &SurfaceSettings) -> WebSurfaceSettings {
    WebSurfaceSettings {
        zoom_factor: Some(s.zoom_factor),
        user_agent: None,
        devtools_enabled: Some(s.dev_tools),
        javascript_enabled: None,
        default_context_menus_enabled: None,
        builtin_accelerator_keys_enabled: None,
        inactive_scheduling_policy: None,
    }
}

// ── Capabilities ──────────────────────────────────────────────────────

pub fn map_capabilities(caps: ScryingCapabilities) -> InkerWebSurfaceCapabilities {
    let transport_supported = capability_status(caps.imported_texture);
    let overlay_supported = capability_status(caps.native_child_overlay);
    let basic_cookie_attrs = CookieAttributeCapabilities {
        same_site: WebFeatureStatus::Supported,
        partitioned: WebFeatureStatus::Partial {
            detail: "supported only on backends exposing partitioned-cookie setters".into(),
        },
        http_only: WebFeatureStatus::Supported,
        secure: WebFeatureStatus::Supported,
        expires: WebFeatureStatus::Supported,
    };
    InkerWebSurfaceCapabilities {
        backend_name: backend_name(caps.backend).into(),
        backend_version: None,
        frame_transport: map_surface_mode(caps.preferred_mode),
        cookie: CookieCapabilities {
            read: WebFeatureStatus::Partial {
                detail: "cookie reads are backend-specific and URL-scoped where exposed".into(),
            },
            write: WebFeatureStatus::Supported,
            delete: WebFeatureStatus::Partial {
                detail: "cookie delete support exists on platform stores but is not in inker's v1 WebSurface trait".into(),
            },
            change_events: WebFeatureStatus::Partial {
                detail: "cookie change observation is wired on supported platform producers".into(),
            },
            attributes: basic_cookie_attrs,
        },
        script: ScriptCapabilities {
            execute: WebFeatureStatus::Supported,
            result: WebFeatureStatus::Supported,
            exceptions: WebFeatureStatus::Partial {
                detail: "script exceptions are returned through the serialized engine result where available".into(),
            },
        },
        pointer: PointerInputCapabilities {
            mouse: WebFeatureStatus::Supported,
            pen: WebFeatureStatus::Partial {
                detail: "device kind, id, pressure, and tilt survive; concrete system webviews vary".into(),
            },
            touch: WebFeatureStatus::Partial {
                detail: "multi-touch ids survive; concrete system webviews vary".into(),
            },
            contact_geometry: WebFeatureStatus::unsupported(
                "scrying's portable PointerInput has no contact geometry",
            ),
            pressure: WebFeatureStatus::Supported,
            tangential_pressure: WebFeatureStatus::unsupported(
                "scrying's portable PointerInput has no tangential pressure",
            ),
            tilt: WebFeatureStatus::Supported,
            twist: WebFeatureStatus::unsupported(
                "scrying's portable PointerInput has no twist",
            ),
            altitude_azimuth: WebFeatureStatus::unsupported(
                "scrying's portable PointerInput has no altitude/azimuth angles",
            ),
        },
        document: DocumentCapabilities {
            find_in_page: WebFeatureStatus::Partial {
                detail: "find support depends on the concrete system webview backend".into(),
            },
            page_zoom: WebFeatureStatus::Partial {
                detail: "zoom settings are forwarded to the concrete system webview backend".into(),
            },
            // P1 defines a correlated capture protocol. The legacy CPU
            // snapshot capability cannot satisfy it until this adapter accepts
            // requests and emits correlated completions.
            page_capture: WebFeatureStatus::unsupported(
                "scrying CPU snapshots are not wired to Inker's page-capture protocol",
            ),
            navigation: WebFeatureStatus::Partial {
                detail: "navigation controls are forwarded to the concrete system webview backend".into(),
            },
        },
        pdf: WebFeatureStatus::Partial {
            detail: "PDF handling depends on the concrete system webview backend".into(),
        },
        downloads: WebFeatureStatus::Partial {
            detail: "download events are exposed by backends that surface native download callbacks".into(),
        },
        devtools: WebFeatureStatus::Partial {
            detail: "devtools can be enabled/opened where the platform webview exposes it".into(),
        },
        popups: WebFeatureStatus::Supported,
        permissions: WebFeatureStatus::Partial {
            detail: "permission prompts are backend-specific".into(),
        },
        auth: WebFeatureStatus::Supported,
        context_menus: WebFeatureStatus::Supported,
        drag_drop: DragDropCapabilities {
            host_to_page: WebFeatureStatus::unsupported(
                "scrying's portable DragInput does not carry DataTransfer items",
            ),
            page_to_host: WebFeatureStatus::unsupported(
                "scrying does not expose a portable page drag-start event",
            ),
            file_items: WebFeatureStatus::unsupported(
                "file payload forwarding requires a backend-native drag object",
            ),
            string_items: WebFeatureStatus::unsupported(
                "string payload forwarding is not present in scrying's portable drag API",
            ),
        },
        ime_observability: WebFeatureStatus::Supported,
        accessibility: WebFeatureStatus::Partial {
            detail: "system-webview accessibility remains owned by the platform view".into(),
        },
        degradation_reasons: vec![
            caps.reason.into(),
            format!("imported_texture={transport_supported:?}"),
            format!("native_child_overlay={overlay_supported:?}"),
        ],
    }
}

fn capability_status(status: ScryingCapabilityStatus) -> WebFeatureStatus {
    match status {
        ScryingCapabilityStatus::Supported => WebFeatureStatus::Supported,
        ScryingCapabilityStatus::Unsupported(reason) => WebFeatureStatus::Unsupported {
            reason: format!("{reason:?}"),
        },
    }
}

fn map_surface_mode(mode: ScryingSurfaceMode) -> WebFrameTransportMode {
    match mode {
        ScryingSurfaceMode::ImportedTexture => WebFrameTransportMode::ImportedTexture,
        ScryingSurfaceMode::NativeChildOverlay => WebFrameTransportMode::NativeChildOverlay,
        ScryingSurfaceMode::CpuSnapshot => WebFrameTransportMode::CpuSnapshot,
        ScryingSurfaceMode::Unsupported => WebFrameTransportMode::Unsupported,
        _ => WebFrameTransportMode::Unsupported,
    }
}

fn backend_name(backend: ScryingBackend) -> &'static str {
    match backend {
        ScryingBackend::WebView2 => "scrying.webview2",
        ScryingBackend::WkWebView => "scrying.wkwebview",
        ScryingBackend::Wpe => "scrying.wpe",
        ScryingBackend::WebKitGtk => "scrying.webkitgtk",
        ScryingBackend::Unknown => "scrying.unknown",
        _ => "scrying.unknown",
    }
}

// ── Events ────────────────────────────────────────────────────────────

/// Lossy map scrying's rich event vocabulary to inker's portable subset.
/// Returns `None` for events that have no inker equivalent today (downloads,
/// permissions, drag, context menu, etc.); they're dropped at the boundary.
pub fn map_navigation_event(ev: ScryingNavEvent) -> Option<InkerNavEvent> {
    match ev {
        ScryingNavEvent::Starting { url } => Some(InkerNavEvent::Started { url }),
        ScryingNavEvent::SourceChanged { url } => Some(InkerNavEvent::Committed { url }),
        ScryingNavEvent::Completed { url, success } => {
            if success {
                Some(InkerNavEvent::Finished { url, title: None })
            } else {
                Some(InkerNavEvent::Failed {
                    url,
                    reason: "navigation failed".into(),
                })
            }
        },
        ScryingNavEvent::TitleChanged { title } => Some(InkerNavEvent::Finished {
            url: String::new(),
            title: Some(title),
        }),
        _ => None,
    }
}

pub fn map_web_event(ev: ScryingNavEvent) -> Option<WebSurfaceEvent> {
    match ev {
        ScryingNavEvent::Starting { url } => {
            Some(WebSurfaceEvent::Navigation(InkerNavEvent::Started { url }))
        },
        ScryingNavEvent::SourceChanged { url } => {
            Some(WebSurfaceEvent::AddressChanged { url: url.clone() }).or(Some(
                WebSurfaceEvent::Navigation(InkerNavEvent::Committed { url }),
            ))
        },
        ScryingNavEvent::Completed { url, success } => {
            let nav = if success {
                InkerNavEvent::Finished { url, title: None }
            } else {
                InkerNavEvent::Failed {
                    url,
                    reason: "navigation failed".into(),
                }
            };
            Some(WebSurfaceEvent::Navigation(nav))
        },
        ScryingNavEvent::TitleChanged { title } => Some(WebSurfaceEvent::TitleChanged { title }),
        ScryingNavEvent::NewWindowRequested { url } => {
            Some(WebSurfaceEvent::NewWindowRequested { url })
        },
        ScryingNavEvent::ContentProcessTerminated => Some(WebSurfaceEvent::ProcessCrashed {
            reason: "web content process terminated".into(),
        }),
        ScryingNavEvent::AuthChallenged { url, host, .. } => {
            Some(WebSurfaceEvent::BackendDiagnostic {
                severity: "info".into(),
                message: format!(
                    "authentication challenge handled by the system provider: {}",
                    if host.is_empty() { url } else { host }
                ),
            })
        },
        ScryingNavEvent::DownloadStarted {
            url,
            suggested_filename,
            ..
        } => Some(WebSurfaceEvent::DownloadRequested {
            url,
            suggested_name: Some(suggested_filename),
        }),
        ScryingNavEvent::DownloadProgress { .. } => Some(WebSurfaceEvent::BackendDiagnostic {
            severity: "info".into(),
            message: "download progressed".into(),
        }),
        ScryingNavEvent::DownloadFinished { error, .. } => {
            Some(WebSurfaceEvent::BackendDiagnostic {
                severity: if error.is_some() { "warn" } else { "info" }.into(),
                message: error.unwrap_or_else(|| "download finished".into()),
            })
        },
        ScryingNavEvent::DownloadCancelled { .. } => Some(WebSurfaceEvent::BackendDiagnostic {
            severity: "warn".into(),
            message: "download cancelled".into(),
        }),
        ScryingNavEvent::DropDetected {
            x, y, primary_url, ..
        } => Some(WebSurfaceEvent::BackendDiagnostic {
            severity: "info".into(),
            message: format!("drop detected at {x},{y}; url={primary_url:?}"),
        }),
        ScryingNavEvent::MediaCaptureStateChanged {
            audio_active_tracks,
            video_active_tracks,
        } => Some(WebSurfaceEvent::BackendDiagnostic {
            severity: "info".into(),
            message: format!(
                "media capture changed: audio={audio_active_tracks}; video={video_active_tracks}"
            ),
        }),
        ScryingNavEvent::ContextMenuRequested {
            x,
            y,
            link_url,
            image_url,
            ..
        } => Some(WebSurfaceEvent::ContextMenuRequested {
            x,
            y,
            link_url,
            image_url,
        }),
        ScryingNavEvent::AcceleratorKeyPressed { .. } => Some(WebSurfaceEvent::BackendDiagnostic {
            severity: "info".into(),
            message: "browser accelerator key pressed".into(),
        }),
        ScryingNavEvent::TextInputFocused { .. } => Some(WebSurfaceEvent::BackendDiagnostic {
            severity: "info".into(),
            message: "text input focused".into(),
        }),
        ScryingNavEvent::TextInputChanged { .. } => Some(WebSurfaceEvent::BackendDiagnostic {
            severity: "info".into(),
            message: "text input changed".into(),
        }),
        ScryingNavEvent::TextInputBlurred => Some(WebSurfaceEvent::BackendDiagnostic {
            severity: "info".into(),
            message: "text input blurred".into(),
        }),
        _ => None,
    }
}

pub fn map_cursor_shape(shape: ScryingCursorShape) -> InkerCursorShape {
    match shape {
        ScryingCursorShape::Default => InkerCursorShape::Default,
        ScryingCursorShape::Pointer => InkerCursorShape::Pointer,
        ScryingCursorShape::Text => InkerCursorShape::Text,
        ScryingCursorShape::Crosshair => InkerCursorShape::Crosshair,
        ScryingCursorShape::Move | ScryingCursorShape::ResizeAll => InkerCursorShape::Move,
        ScryingCursorShape::NotAllowed => InkerCursorShape::NotAllowed,
        ScryingCursorShape::ResizeNs => InkerCursorShape::ResizeNs,
        ScryingCursorShape::ResizeEw => InkerCursorShape::ResizeEw,
        ScryingCursorShape::ResizeNeSw => InkerCursorShape::ResizeNesw,
        ScryingCursorShape::ResizeNwSe => InkerCursorShape::ResizeNwse,
        ScryingCursorShape::Grab => InkerCursorShape::Grab,
        ScryingCursorShape::Grabbing => InkerCursorShape::Grabbing,
        // Wait / Help / Progress / ZoomIn / ZoomOut / Custom — no inker
        // equivalent; map to Default. Hosts wanting full fidelity can
        // downcast to the concrete producer.
        _ => InkerCursorShape::Default,
    }
}

pub fn wrap_web_message(payload: String) -> WebMessage {
    WebMessage {
        tag: String::new(),
        payload,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inker::{CapabilityStatus, PhysicalPosition, PointerButtons};
    use scrying::native_frame::{Dx12SharedTexture, NativeFrame};

    fn touch_pointer(pointer_id: i32, phase: InkerPointerPhase) -> InkerPointerEvent {
        InkerPointerEvent {
            pointer_id,
            pointer_type: InkerPointerType::Touch,
            is_primary: pointer_id == 2,
            phase,
            position: PhysicalPosition { x: 12.0, y: 34.0 },
            button: None,
            buttons: PointerButtons::PRIMARY,
            width: 8.0,
            height: 10.0,
            pressure: None,
            tangential_pressure: None,
            tilt_x: None,
            tilt_y: None,
            twist: None,
            altitude_angle: None,
            azimuth_angle: None,
            modifiers: InkerKeyboardModifiers::default(),
        }
    }

    #[test]
    fn dx12_epoch_handle_is_transferred_and_reuse_may_omit_it() {
        let frame = |handle| {
            WebSurfaceFrame::Native(NativeFrame::Dx12SharedTexture(Dx12SharedTexture {
                size: dpi::PhysicalSize::new(320, 200),
                format: wgpu::TextureFormat::Bgra8Unorm,
                generation: 7,
                producer_sync: SyncMechanism::ExplicitFence,
                fence_value: 11,
                handle,
            }))
        };
        let first = map_frame(frame(0x1234_usize as *mut _), Some(0x5678)).unwrap();
        let reused = map_frame(frame(std::ptr::null_mut()), Some(0x5678)).unwrap();
        assert!(matches!(
            first.texture,
            NativeTextureHandle::D3d12Shared {
                handle: 0x1234,
                ownership: FrameHandleOwnership::Transferred,
            }
        ));
        assert!(matches!(
            reused.texture,
            NativeTextureHandle::D3d12Shared {
                handle: 0,
                ownership: FrameHandleOwnership::Transferred,
            }
        ));
        assert_eq!(first.resource_epoch, reused.resource_epoch);
    }

    #[test]
    fn touch_pointer_preserves_contact_id_and_uses_pointer_events_default_pressure() {
        let first = map_pointer(touch_pointer(2, InkerPointerPhase::Down)).unwrap();
        let second = map_pointer(touch_pointer(3, InkerPointerPhase::Down)).unwrap();
        assert_eq!(first.pointer_id, 2);
        assert_eq!(second.pointer_id, 3);
        assert_eq!(first.device, ScryingPointerDevice::Touch);
        assert_eq!(first.pressure, 0.5);
    }

    #[test]
    fn negative_pointer_id_is_rejected_instead_of_wrapping() {
        assert!(matches!(
            map_pointer(touch_pointer(-1, InkerPointerPhase::Down)),
            Err(SurfaceError::InputFailed(_))
        ));
    }

    #[test]
    fn mouse_press_maps_to_left_button_down() {
        let ev = InkerMouseEvent {
            position: PhysicalPosition { x: 10.0, y: 20.0 },
            button: Some(InkerMouseButton::Left),
            kind: InkerMouseEventKind::Pressed,
        };
        let out = map_mouse(ev);
        assert_eq!(out.kind, ScryingMouseEventKind::LeftButtonDown);
        assert_eq!(out.point, (10, 20));
        assert!(out.virtual_keys.left_button);
    }

    #[test]
    fn scroll_lines_converts_to_wheel_deltas() {
        let ev = InkerMouseEvent {
            position: PhysicalPosition { x: 0.0, y: 0.0 },
            button: None,
            kind: InkerMouseEventKind::ScrollLines {
                delta_x: 0.0,
                delta_y: 2.0,
            },
        };
        let out = map_mouse(ev);
        assert_eq!(out.kind, ScryingMouseEventKind::Wheel);
        assert_eq!(out.mouse_data, 240);
    }

    #[test]
    fn keyboard_pressed_maps_to_down() {
        let ev = InkerKeyboardEvent {
            key_code: 65,
            scan_code: 0,
            modifiers: InkerKeyboardModifiers {
                shift: true,
                ..Default::default()
            },
            pressed: true,
            text: Some("A".into()),
        };
        let out = map_keyboard(ev);
        assert_eq!(out.kind, ScryingKeyEventKind::Down);
        assert_eq!(out.virtual_key_code, 65);
        assert_eq!(out.characters, "A");
        assert!(out.modifiers.shift);
    }

    #[test]
    fn focus_tab_maps_to_next() {
        assert_eq!(
            map_focus_reason(InkerFocusReason::Tab),
            ScryingFocusReason::Next
        );
        assert_eq!(
            map_focus_reason(InkerFocusReason::ShiftTab),
            ScryingFocusReason::Previous
        );
    }

    #[test]
    fn settings_zoom_and_devtools_pass_through() {
        let s = SurfaceSettings {
            background_color: [0, 0, 0, 255],
            zoom_factor: 1.25,
            dev_tools: true,
        };
        let out = map_settings(&s);
        assert_eq!(out.zoom_factor, Some(1.25));
        assert_eq!(out.devtools_enabled, Some(true));
    }

    #[test]
    fn capabilities_keep_hosted_document_limits_explicit() {
        let caps = map_capabilities(ScryingCapabilities {
            backend: ScryingBackend::Unknown,
            preferred_mode: ScryingSurfaceMode::NativeChildOverlay,
            imported_texture: ScryingCapabilityStatus::Unsupported(
                scrying::native_frame::UnsupportedReason::PlatformNotImplemented,
            ),
            native_child_overlay: ScryingCapabilityStatus::Supported,
            cpu_snapshot: ScryingCapabilityStatus::Unsupported(
                scrying::native_frame::UnsupportedReason::PlatformNotImplemented,
            ),
            supported_frames: Vec::new(),
            reason: "test stub",
        })
        .document;
        assert!(matches!(
            caps.find_in_page,
            CapabilityStatus::Partial { .. }
        ));
        assert!(matches!(caps.page_zoom, CapabilityStatus::Partial { .. }));
        assert!(matches!(
            caps.page_capture,
            CapabilityStatus::Unsupported { .. }
        ));
        assert!(matches!(caps.navigation, CapabilityStatus::Partial { .. }));
    }

    #[test]
    fn nav_completed_success_maps_to_finished() {
        let out = map_navigation_event(ScryingNavEvent::Completed {
            url: "https://example.com".into(),
            success: true,
        });
        assert!(matches!(out, Some(InkerNavEvent::Finished { .. })));
    }

    #[test]
    fn nav_completed_failure_maps_to_failed() {
        let out = map_navigation_event(ScryingNavEvent::Completed {
            url: "https://example.com".into(),
            success: false,
        });
        assert!(matches!(out, Some(InkerNavEvent::Failed { .. })));
    }

    #[test]
    fn nav_download_event_drops() {
        // Events that have no inker equivalent return None.
        let out = map_navigation_event(ScryingNavEvent::ContentProcessTerminated);
        assert!(out.is_none());
    }
}
