//! Windows-only registered Scrying producer used by Pelt's P4 forcing receipt.

use std::ffi::c_void;
use std::path::Path;
use std::sync::{Arc, Mutex};

use inker::{
    Cookie, FocusReason, KeyboardEvent, MouseEvent, PointerEvent, SurfaceEngine, SurfaceError,
    SurfaceFrame, SurfaceProducer, SurfaceSettings, SurfaceSpawnRequest, WebSurface,
    WebSurfaceCapabilities, WebSurfaceEvent,
};
use scrying_engine::scrying::{PlatformWebSurfaceConfig, PlatformWebSurfaceProducer};
use scrying_engine::{ScryingProducer, translation::map_error};

use crate::dx12_surface::Dx12SharedFence;

#[derive(Clone)]
pub(crate) struct ScryingReceiptHost {
    state: Arc<Mutex<Option<ScryingHostState>>>,
}

#[derive(Clone)]
struct ScryingHostState {
    hwnd: usize,
    fence: Arc<Dx12SharedFence>,
}

impl ScryingReceiptHost {
    pub(crate) fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) fn install(&self, hwnd: usize, device: &wgpu::Device) -> Result<(), String> {
        let fence = Arc::new(Dx12SharedFence::new(device)?);
        *self
            .state
            .lock()
            .map_err(|_| "Pelt Scrying host-state lock was poisoned".to_owned())? =
            Some(ScryingHostState { hwnd, fence });
        Ok(())
    }

    fn current(&self) -> Result<ScryingHostState, SurfaceError> {
        self.state
            .lock()
            .map_err(|_| {
                SurfaceError::SpawnFailed("Pelt Scrying host-state lock was poisoned".into())
            })?
            .clone()
            .ok_or_else(|| {
                SurfaceError::FrameAcquisitionFailed(
                    "Pelt's window and D3D12 fence are not ready".into(),
                )
            })
    }
}

pub(crate) struct ScryingReceiptEngine {
    host: ScryingReceiptHost,
}

impl ScryingReceiptEngine {
    pub(crate) fn new(host: ScryingReceiptHost) -> Self {
        Self { host }
    }
}

impl SurfaceEngine for ScryingReceiptEngine {
    fn engine_id(&self) -> &str {
        inker::routing::ENGINE_SCRYING_WEB
    }

    fn spawn(
        &self,
        request: &SurfaceSpawnRequest,
    ) -> Result<Box<dyn SurfaceProducer>, SurfaceError> {
        Ok(Box::new(PeltScryingProducer {
            host: self.host.clone(),
            url: request.url.clone(),
            profile: request.profile.user_data_dir.clone(),
            size: (request.width.max(1), request.height.max(1)),
            offset: (0, 0),
            inner: None,
        }))
    }
}

/// Defers HWND- and device-dependent Scrying construction until Pelt's winit
/// host is resumed, while preserving the normal Scrying Inker control plane.
struct PeltScryingProducer {
    host: ScryingReceiptHost,
    url: String,
    profile: String,
    size: (u32, u32),
    offset: (i32, i32),
    inner: Option<ScryingProducer>,
}

impl PeltScryingProducer {
    fn ensure(&mut self) -> Result<&mut ScryingProducer, SurfaceError> {
        if self.inner.is_none() {
            let host = self.host.current()?;
            let config = PlatformWebSurfaceConfig::new(
                dpi::PhysicalSize::new(self.size.0, self.size.1),
                &self.profile,
            )
            .non_persistent()
            .with_offset(self.offset.0 as f32, self.offset.1 as f32)
            .with_diagnostic_backdrop((24, 32, 48))
            .with_fence_shared_handle(host.fence.raw_handle() as usize as *mut c_void);
            // SAFETY: WorkspaceApp retains the live top-level window for longer
            // than every routed producer and installs its HWND into this host.
            let producer =
                unsafe { PlatformWebSurfaceProducer::new(host.hwnd as *mut c_void, config) }
                    .map_err(map_error)?;
            let path = Path::new(&self.url);
            if path.is_file() {
                let html = std::fs::read_to_string(path).map_err(|error| {
                    SurfaceError::SpawnFailed(format!(
                        "could not read Scrying receipt fixture {}: {error}",
                        path.display()
                    ))
                })?;
                producer.load_html(&html).map_err(map_error)?;
            } else {
                producer.load_url(&self.url).map_err(map_error)?;
            }
            self.inner = Some(ScryingProducer::new(
                Box::new(producer),
                Some(host.fence.raw_handle()),
            ));
        }
        Ok(self.inner.as_mut().expect("producer initialized above"))
    }
}

impl SurfaceProducer for PeltScryingProducer {
    fn resize(&mut self, width: u32, height: u32) -> Result<(), SurfaceError> {
        self.size = (width.max(1), height.max(1));
        if self.inner.is_some() {
            let (width, height) = self.size;
            self.ensure()?.resize(width, height)?;
        }
        Ok(())
    }

    fn set_offset(&mut self, x: i32, y: i32) -> Result<(), SurfaceError> {
        self.offset = (x, y);
        if self.inner.is_some() {
            self.ensure()?.set_offset(x, y)?;
        }
        Ok(())
    }

    fn acquire_frame(&mut self) -> Result<Option<SurfaceFrame>, SurfaceError> {
        self.ensure()?.acquire_frame()
    }

    fn send_mouse_input(&mut self, event: MouseEvent) -> Result<(), SurfaceError> {
        self.ensure()?.send_mouse_input(event)
    }

    fn send_pointer_input(&mut self, event: PointerEvent) -> Result<(), SurfaceError> {
        self.ensure()?.send_pointer_input(event)
    }

    fn send_keyboard_input(&mut self, event: KeyboardEvent) -> Result<(), SurfaceError> {
        self.ensure()?.send_keyboard_input(event)
    }

    fn move_focus(&mut self, reason: FocusReason) -> Result<(), SurfaceError> {
        self.ensure()?.move_focus(reason)
    }

    fn poll_cursor_shape(&mut self) -> Option<inker::CursorShape> {
        self.ensure().ok()?.poll_cursor_shape()
    }

    fn apply_settings(&mut self, settings: &SurfaceSettings) -> Result<(), SurfaceError> {
        self.ensure()?.apply_settings(settings)
    }

    fn capture_snapshot_png(&mut self) -> Result<Vec<u8>, SurfaceError> {
        self.ensure()?.capture_snapshot_png()
    }

    fn as_web_surface(&mut self) -> Option<&mut dyn WebSurface> {
        Some(self)
    }
}

impl WebSurface for PeltScryingProducer {
    fn capabilities(&self) -> WebSurfaceCapabilities {
        self.inner
            .as_ref()
            .map(WebSurface::capabilities)
            .unwrap_or_default()
    }

    fn navigate_to_url(&mut self, url: &str) -> Result<(), SurfaceError> {
        self.ensure()?.navigate_to_url(url)
    }

    fn navigate_to_string(&mut self, html: &str) -> Result<(), SurfaceError> {
        self.ensure()?.navigate_to_string(html)
    }

    fn reload(&mut self) -> Result<(), SurfaceError> {
        self.ensure()?.reload()
    }

    fn stop(&mut self) -> Result<(), SurfaceError> {
        self.ensure()?.stop()
    }

    fn go_back(&mut self) -> Result<(), SurfaceError> {
        self.ensure()?.go_back()
    }

    fn go_forward(&mut self) -> Result<(), SurfaceError> {
        self.ensure()?.go_forward()
    }

    fn can_go_back(&self) -> bool {
        self.inner.as_ref().is_some_and(WebSurface::can_go_back)
    }

    fn can_go_forward(&self) -> bool {
        self.inner.as_ref().is_some_and(WebSurface::can_go_forward)
    }

    fn set_cookie(&mut self, cookie: &Cookie) -> Result<(), SurfaceError> {
        self.ensure()?.set_cookie(cookie)
    }

    fn execute_script_with_result(&mut self, script: &str) -> Result<String, SurfaceError> {
        self.ensure()?.execute_script_with_result(script)
    }

    fn poll_web_event(&mut self) -> Option<WebSurfaceEvent> {
        self.ensure().ok()?.poll_web_event()
    }
}
