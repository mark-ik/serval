// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! The canvas as a presentation surface, and as the frame the host asks about
//! its size.
//!
//! Both are thin. `RenderCore` already boots wgpu and owns the renderer
//! without caring what it presents onto, so the browser differs from the
//! desktop in one expression: the surface target is an `HtmlCanvasElement`
//! rather than a window handle.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cambium_rootstock::{HostWindow, Surface};
use genet_render_host::{RenderCore, WindowSurface};
use netrender::NetrenderOptions;
use web_sys::HtmlCanvasElement;

/// A `RenderCore` plus the canvas surface created from it.
pub struct WebSurface {
    core: RenderCore,
    surface: WindowSurface,
}

impl WebSurface {
    /// Boot wgpu and create this canvas's surface.
    ///
    /// Async because the browser's WebGPU device request is: there is no
    /// blocking path on wasm, which is why `RenderCore` has `boot_async` at
    /// all.
    pub async fn boot(
        canvas: HtmlCanvasElement,
        width: u32,
        height: u32,
        options: NetrenderOptions,
    ) -> Result<Self, String> {
        let core = RenderCore::boot_async(options).await?;
        // The one browser-specific line in the whole presentation path.
        let surface = core.create_surface(wgpu::SurfaceTarget::Canvas(canvas), width, height)?;
        Ok(Self { core, surface })
    }
}

impl Surface for WebSurface {
    fn core(&self) -> &RenderCore {
        &self.core
    }

    fn format(&self) -> wgpu::TextureFormat {
        self.surface.format()
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.surface.resize(&self.core, width, height);
    }

    fn acquire(&self) -> Option<wgpu::SurfaceTexture> {
        self.surface.acquire(&self.core)
    }
}

/// The canvas as the host's frame.
///
/// Answers the five things [`HostWindow`] asks, and nothing else: the trait
/// deliberately omits the window management a tab cannot do.
#[derive(Clone)]
pub struct WebWindow {
    canvas: HtmlCanvasElement,
    /// Set when a frame has been asked for and not yet drawn, so a burst of
    /// events in one turn schedules one frame rather than one each.
    ///
    /// The browser's `requestAnimationFrame` does not coalesce for us the way
    /// winit's `request_redraw` does: repeated calls queue repeated callbacks.
    /// The flag is what makes the two event sources behave alike.
    ///
    /// Atomic rather than a `Cell` because the host's wake handle requires
    /// `Send + Sync`, and that is a bound the compiler checks, not a statement
    /// about how many threads exist. It costs nothing here and keeps one
    /// waker shape across both event sources.
    pending_frame: Arc<AtomicBool>,
}

impl WebWindow {
    pub fn new(canvas: HtmlCanvasElement) -> Self {
        Self {
            canvas,
            pending_frame: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The canvas this presents onto.
    pub fn canvas(&self) -> &HtmlCanvasElement {
        &self.canvas
    }

    /// Whether a frame is owed, clearing the flag. Called by the frame loop.
    pub fn take_pending_frame(&self) -> bool {
        self.pending_frame.swap(false, Ordering::Relaxed)
    }

    /// The frame-owed flag, for a waker that must set it from outside.
    ///
    /// Shared rather than copied: a wake signal and a redraw request have to
    /// mean the same pending frame, or a worker's wake draws nothing.
    pub fn pending_frame_flag(&self) -> Arc<AtomicBool> {
        self.pending_frame.clone()
    }

    /// The size to configure the surface at, in physical pixels.
    pub fn physical_size(&self) -> (u32, u32) {
        let (w, h) = self.inner_size();
        (w.max(1), h.max(1))
    }
}

impl HostWindow for WebWindow {
    fn request_redraw(&self) {
        self.pending_frame.store(true, Ordering::Relaxed);
    }

    fn inner_size(&self) -> (u32, u32) {
        // The CSS box, scaled: the canvas backing store is sized to physical
        // pixels so a fragment lands on a device pixel, exactly as a desktop
        // surface is configured in physical pixels.
        let rect = self.canvas.get_bounding_client_rect();
        let scale = self.scale_factor();
        (
            (rect.width() * scale).round().max(1.0) as u32,
            (rect.height() * scale).round().max(1.0) as u32,
        )
    }

    fn scale_factor(&self) -> f64 {
        web_sys::window()
            .map(|w| w.device_pixel_ratio())
            .filter(|r| *r > 0.0)
            .unwrap_or(1.0)
    }

    fn set_ime_allowed(&self, _allowed: bool) {
        // A browser offers text input when an editable element has focus, which
        // the host cannot assert from here without owning a hidden input. Left
        // as a no-op rather than faked: composition still arrives through the
        // canvas's composition events, and the honest gap is documented at the
        // seam instead of hidden behind a lie.
    }

    fn set_ime_cursor_area(&self, _x: f64, _y: f64, _width: f64, _height: f64) {
        // Same gap. Positioning the candidate window needs the editing host to
        // be a real element; until this owns one, the browser places it itself.
    }
}
