// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! X11 metadata for app-drawn frame effects.

use cambium_rootstock::AppFrameInsets;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, PropMode};
use x11rb::wrapper::ConnectionExt as _;

const GTK_FRAME_EXTENTS: &[u8] = b"_GTK_FRAME_EXTENTS";

/// The property's own word order: left, right, top, bottom.
///
/// The scale is the **layout** scale, zoom included, because the insets are
/// CSS pixels and the application draws that margin out of its own stylesheet
/// — see [`AppFrameInsets::scaled`], where the rule and its test live, since
/// this module compiles on Linux only.
fn device_extents(insets: AppFrameInsets, layout_scale: f64) -> [u32; 4] {
    let device = insets.scaled(layout_scale);
    [device.left, device.right, device.top, device.bottom]
}

/// Publish the client-drawn transparent margins on an X11 window.
///
/// `None` means this is a Wayland window or the app reserved no transparent
/// margins. Property presence itself tells Mutter that the client owns outer
/// frame geometry, so an ordinary host-framed window must leave it absent.
///
/// `layout_scale` comes from the caller rather than off the window: the insets
/// carry zoom (see [`device_extents`]), and the window knows only its device
/// scale.
pub(crate) fn publish_gtk_frame_extents(
    window: &Window,
    insets: AppFrameInsets,
    layout_scale: f64,
) -> Result<Option<[u32; 4]>, String> {
    if insets.is_empty() {
        return Ok(None);
    }
    let handle = window
        .window_handle()
        .map_err(|error| format!("window handle unavailable: {error}"))?;
    let RawWindowHandle::Xlib(handle) = handle.as_raw() else {
        return Ok(None);
    };
    let xid = u32::try_from(handle.window)
        .map_err(|_| format!("X11 window id {} does not fit XID", handle.window))?;
    let extents = device_extents(insets, layout_scale);
    let (connection, _) =
        x11rb::connect(None).map_err(|error| format!("could not connect to DISPLAY: {error}"))?;
    let atom = connection
        .intern_atom(false, GTK_FRAME_EXTENTS)
        .map_err(|error| format!("could not intern _GTK_FRAME_EXTENTS: {error}"))?
        .reply()
        .map_err(|error| format!("could not read _GTK_FRAME_EXTENTS atom: {error}"))?
        .atom;
    connection
        .change_property32(PropMode::REPLACE, xid, atom, AtomEnum::CARDINAL, &extents)
        .map_err(|error| format!("could not set _GTK_FRAME_EXTENTS: {error}"))?
        .check()
        .map_err(|error| format!("X11 server rejected _GTK_FRAME_EXTENTS: {error}"))?;
    connection
        .flush()
        .map_err(|error| format!("could not flush _GTK_FRAME_EXTENTS: {error}"))?;
    Ok(Some(extents))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_insets_scale_to_x11_device_pixels_in_protocol_order() {
        assert_eq!(
            device_extents(
                AppFrameInsets {
                    left: 8,
                    right: 10,
                    top: 12,
                    bottom: 14,
                },
                1.5,
            ),
            [12, 15, 18, 21]
        );
    }

    #[test]
    fn published_extents_carry_zoom_because_the_app_draws_the_frame_in_css_pixels() {
        // Device scale 2, zoom 1.25: the stylesheet's 8 CSS px of transparent
        // margin is painted at the layout scale, so it lands on 20 device
        // pixels. Publishing at the device scale alone claims 16 and names a
        // boundary four pixels inside the one the application actually paints.
        let insets = AppFrameInsets::uniform(8);
        assert_eq!(device_extents(insets, 2.0 * 1.25), [20, 20, 20, 20]);
        assert_eq!(device_extents(insets, 2.0), [16, 16, 16, 16]);
    }
}
