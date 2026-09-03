// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Windows 11 Snap Layout hit testing for an application-drawn title bar.
//!
//! Microsoft keys the hover flyout to `WM_NCHITTEST == HTMAXBUTTON`. Winit
//! does not expose that answer, so the host subclasses its existing HWND and
//! supplies the one native result the application-owned frame knows.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    HTMAXBUTTON, IsZoomed, SW_MAXIMIZE, SW_RESTORE, ShowWindow, WM_NCHITTEST, WM_NCLBUTTONDOWN,
    WM_NCLBUTTONUP,
};
use winit::window::Window;

const SUBCLASS_ID: usize = 0x4341_4D34;

#[derive(Default)]
struct HitRect {
    enabled: AtomicBool,
    left: AtomicI32,
    top: AtomicI32,
    right: AtomicI32,
    bottom: AtomicI32,
}

impl HitRect {
    fn get(&self) -> Option<[i32; 4]> {
        self.enabled.load(Ordering::Acquire).then(|| {
            [
                self.left.load(Ordering::Relaxed),
                self.top.load(Ordering::Relaxed),
                self.right.load(Ordering::Relaxed),
                self.bottom.load(Ordering::Relaxed),
            ]
        })
    }

    fn replace(&self, rect: Option<[i32; 4]>) -> bool {
        let before = self.get();
        if before == rect {
            return false;
        }
        self.enabled.store(false, Ordering::Release);
        if let Some([left, top, right, bottom]) = rect {
            self.left.store(left, Ordering::Relaxed);
            self.top.store(top, Ordering::Relaxed);
            self.right.store(right, Ordering::Relaxed);
            self.bottom.store(bottom, Ordering::Relaxed);
            self.enabled.store(true, Ordering::Release);
        }
        true
    }

    fn contains(&self, point: POINT) -> bool {
        self.get().is_some_and(|[left, top, right, bottom]| {
            point.x >= left && point.x < right && point.y >= top && point.y < bottom
        })
    }
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    data: usize,
) -> LRESULT {
    if message == WM_NCHITTEST && data != 0 {
        let mut point = POINT {
            x: lparam as i16 as i32,
            y: (lparam >> 16) as i16 as i32,
        };
        // SAFETY: `hwnd` is the live window currently dispatching this
        // message and `point` is writable for the duration of the call.
        if unsafe { ScreenToClient(hwnd, &mut point) } != 0 {
            // SAFETY: `data` names the boxed HitRect retained by the bridge;
            // Drop removes this subclass before freeing it.
            let rect = unsafe { &*(data as *const HitRect) };
            if rect.contains(point) {
                return HTMAXBUTTON as LRESULT;
            }
        }
    }
    // Answering HTMAXBUTTON reroutes the button's own click into the
    // non-client path, and DefWindowProc does nothing with it on a borderless
    // window. The bridge owns the click it advertised: press swallowed, the
    // release performs the toggle.
    if (message == WM_NCLBUTTONDOWN || message == WM_NCLBUTTONUP)
        && wparam == HTMAXBUTTON as WPARAM
        && data != 0
    {
        if message == WM_NCLBUTTONUP {
            // SAFETY: `hwnd` is the live window dispatching this message.
            unsafe {
                let verb = if IsZoomed(hwnd) != 0 {
                    SW_RESTORE
                } else {
                    SW_MAXIMIZE
                };
                let _ = ShowWindow(hwnd, verb);
            }
        }
        return 0;
    }
    // SAFETY: Forward every message outside the declared maximize rect to the
    // next procedure in the HWND's subclass chain.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

pub struct SnapLayoutBridge {
    hwnd: HWND,
    rect: Box<HitRect>,
}

fn device_rect(logical: Option<(f32, f32, f32, f32)>, scale: f64) -> Option<[i32; 4]> {
    logical.and_then(|(x, y, width, height)| {
        let values = [x, y, x + width, y + height];
        values.iter().all(|value| value.is_finite()).then(|| {
            [
                (f64::from(x) * scale).floor() as i32,
                (f64::from(y) * scale).floor() as i32,
                (f64::from(x + width) * scale).ceil() as i32,
                (f64::from(y + height) * scale).ceil() as i32,
            ]
        })
    })
}

impl SnapLayoutBridge {
    pub fn attach(window: &Window) -> Result<Self, String> {
        let handle = window
            .window_handle()
            .map_err(|error| format!("window handle unavailable: {error}"))?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return Err("window is not a Win32 HWND".into());
        };
        let hwnd = handle.hwnd.get() as HWND;
        let rect = Box::<HitRect>::default();
        let data = (&*rect as *const HitRect) as usize;
        // SAFETY: Called on the winit thread that created `hwnd`. The callback
        // and `data` remain valid until Drop removes this exact subclass.
        if unsafe { SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, data) } == 0 {
            return Err(format!(
                "SetWindowSubclass failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self { hwnd, rect })
    }

    /// Publish a logical CSS box as an integer device-pixel hit rectangle.
    /// Returns the effective rect only when it changed.
    pub fn update(
        &self,
        logical: Option<(f32, f32, f32, f32)>,
        scale: f64,
    ) -> Option<Option<[i32; 4]>> {
        let device = device_rect(logical, scale);
        self.rect.replace(device).then_some(device)
    }
}

impl Drop for SnapLayoutBridge {
    fn drop(&mut self) {
        // SAFETY: The same HWND, callback and id passed to SetWindowSubclass.
        // The native window is retained by WinitHost until after this field.
        unsafe {
            RemoveWindowSubclass(self.hwnd, Some(subclass_proc), SUBCLASS_ID);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_control_rect_scales_outward_to_device_pixels() {
        assert_eq!(
            device_rect(Some((10.25, 4.5, 31.5, 20.0)), 1.5),
            Some([15, 6, 63, 37])
        );
    }
}
