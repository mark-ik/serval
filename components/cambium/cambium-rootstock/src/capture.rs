//! Reading a presented frame back.
//!
//! Every self-driving Cambium app needs this and every one had written it: the
//! standard wgpu readback (compose into an owned `COPY_SRC` target, copy to a
//! row-aligned buffer, map it, strip the per-row padding). It lives here so a
//! scenario receipt is an in-process readback of the frame that was actually
//! presented — no compositor, no foreground window, and no chance of
//! photographing the wrong window.
//!
//! What the bytes become is the application's business: woodshed writes a PNG,
//! the host's own smoke example digests them.

use crate::Surface;
use netrender::ExternalTexturePlacement;

/// A presented frame, read back through the shared render-host machinery.
pub type Frame = genet_render_host::RgbaFrame;

/// Compose `view` — the rasterized frame the host just presented — into an
/// owned target and read it back. `None` if the readback failed.
pub fn read_frame(
    surface: &dyn Surface,
    view: &wgpu::TextureView,
    width: u32,
    height: u32,
) -> Option<Frame> {
    let target = surface.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("cambium host frame capture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    surface.renderer().compose_external_texture(
        view,
        &target_view,
        wgpu::TextureFormat::Rgba8Unorm,
        width,
        height,
        ExternalTexturePlacement::new([0.0, 0.0, width as f32, height as f32]),
    );
    surface
        .core()
        .read_rgba8_texture(&target, width, height)
        .map_err(|error| eprintln!("[cambium-host] {error}"))
        .ok()
}
