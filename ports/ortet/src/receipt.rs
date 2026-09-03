// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! The receipt path: compose one frame into an owned texture, read it back,
//! write it as a PNG, and report its digest.
//!
//! Swapchain textures are presentation-only, so the capture cannot read the
//! backbuffer. Ortet composes the frame into a same-device target it owns,
//! reads *that*, and then presents the very same target — the captured pixels
//! and the pixels on screen are one image, not two renders of one scene.

use std::path::{Path, PathBuf};

use genet_winit_host::SurfaceHost;
use netrender::ExternalTexturePlacement;

/// A captured frame: the composed texture (kept alive so its view can still be
/// presented), where it was written, and its digest.
pub struct Capture {
    _texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub path: PathBuf,
    pub digest: u64,
}

pub fn capture(
    host: &SurfaceHost,
    source: &wgpu::TextureView,
    width: u32,
    height: u32,
    path: &Path,
) -> Result<Capture, String> {
    let width = width.max(1);
    let height = height.max(1);
    let texture = host.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("ortet receipt composition"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    host.renderer().compose_external_texture(
        source,
        &view,
        wgpu::TextureFormat::Rgba8Unorm,
        width,
        height,
        ExternalTexturePlacement::new([0.0, 0.0, width as f32, height as f32]),
    );

    let frame = host.core().read_rgba8_texture(&texture, width, height)?;
    // A blank frame is a failed receipt, not a receipt of a blank page: the
    // whole point of the artifact is that something was drawn.
    if frame.is_blank() {
        return Err(format!(
            "receipt captured a blank {width}x{height} frame; nothing was painted"
        ));
    }
    write_png(path, frame.width, frame.height, &frame.rgba)?;

    Ok(Capture {
        _texture: texture,
        view,
        path: path.to_owned(),
        digest: frame.digest(),
    })
}

fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!(
                "could not create the receipt directory {}: {error}",
                parent.display()
            )
        })?;
    }
    let file = std::fs::File::create(path)
        .map_err(|error| format!("could not create {}: {error}", path.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|error| format!("could not write the PNG header: {error}"))?;
    writer
        .write_image_data(rgba)
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    writer
        .finish()
        .map_err(|error| format!("could not finish {}: {error}", path.display()))
}
