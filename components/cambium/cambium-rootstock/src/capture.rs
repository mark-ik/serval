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

/// A presented frame, read back.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// Tightly packed RGBA8, `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

impl Frame {
    /// A cheap order-sensitive digest of the pixels — enough for a receipt to
    /// say "this frame differed from that one" without an image encoder.
    pub fn digest(&self) -> u64 {
        // FNV-1a over the bytes: stable across runs and platforms, which is
        // what a receipt compared over time needs.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in &self.rgba {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }

    /// Whether every pixel is fully transparent or black — the signature of a
    /// frame that presented nothing, which a receipt should never call a pass.
    pub fn is_blank(&self) -> bool {
        self.rgba
            .chunks_exact(4)
            .all(|p| p[3] == 0 || p[..3] == [0, 0, 0])
    }
}

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
    let rgba = read_texture_rgba(surface.device(), surface.queue(), &target, width, height)?;
    Some(Frame {
        width,
        height,
        rgba,
    })
}

/// Read a texture back as tightly packed RGBA8. Standard wgpu readback: copy
/// into a row-aligned buffer, map it, strip the padding each row carries.
fn read_texture_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    let unpadded = width * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded = unpadded.div_ceil(align) * align;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("cambium host capture readback"),
        size: (padded * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("cambium host capture readback"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    if device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .is_err()
    {
        eprintln!("[cambium-host] capture readback poll failed");
        return None;
    }
    let Ok(data) = slice.get_mapped_range() else {
        eprintln!("[cambium-host] capture readback get_mapped_range failed");
        return None;
    };
    let mut out = Vec::with_capacity((unpadded * height) as usize);
    for row in 0..height {
        let start = (row * padded) as usize;
        out.extend_from_slice(&data[start..start + unpadded as usize]);
    }
    drop(data);
    buffer.unmap();
    Some(out)
}
