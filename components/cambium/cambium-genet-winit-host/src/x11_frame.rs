//! X11 metadata for app-drawn frame effects.

use cambium_rootstock::AppFrameInsets;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, PropMode};
use x11rb::wrapper::ConnectionExt as _;

const GTK_FRAME_EXTENTS: &[u8] = b"_GTK_FRAME_EXTENTS";

fn device_extents(insets: AppFrameInsets, scale: f64) -> [u32; 4] {
    let scaled = |value: u32| {
        (f64::from(value) * scale)
            .round()
            .clamp(0.0, u32::MAX as f64) as u32
    };
    [
        scaled(insets.left),
        scaled(insets.right),
        scaled(insets.top),
        scaled(insets.bottom),
    ]
}

/// Publish the client-drawn transparent margins on an X11 window.
///
/// `None` means this is a Wayland window. Zeroes are published deliberately
/// for an app frame without an outer effect, clearing a property left by a
/// previous scale/configuration rather than leaving stale geometry behind.
pub(crate) fn publish_gtk_frame_extents(
    window: &Window,
    insets: AppFrameInsets,
) -> Result<Option<[u32; 4]>, String> {
    let handle = window
        .window_handle()
        .map_err(|error| format!("window handle unavailable: {error}"))?;
    let RawWindowHandle::Xlib(handle) = handle.as_raw() else {
        return Ok(None);
    };
    let xid = u32::try_from(handle.window)
        .map_err(|_| format!("X11 window id {} does not fit XID", handle.window))?;
    let extents = device_extents(insets, window.scale_factor());
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
        .map_err(|error| format!("could not set _GTK_FRAME_EXTENTS: {error}"))?;
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
}
