/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! In-process capture for bounded Pelt product receipts.
//!
//! Swapchain textures are presentation-only. Receipt runs therefore compose
//! the final Pelt frame into an owned same-device target, read that target back,
//! write it as PNG, then present the very same target to the window.

use std::path::{Path, PathBuf};

use genet_winit_host::SurfaceHost;
use netrender::ExternalTexturePlacement;

pub(crate) struct CapturedComposition {
    // Keep the texture alive while its view is presented.
    pub _texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub path: PathBuf,
    pub digest: u64,
}

pub(crate) fn capture_composition(
    host: &SurfaceHost,
    source: &wgpu::TextureView,
    width: u32,
    height: u32,
    path: &Path,
) -> Result<CapturedComposition, String> {
    let width = width.max(1);
    let height = height.max(1);
    let texture = host.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("pelt product receipt composition"),
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
    if frame.is_blank() {
        return Err("product receipt captured a blank frame".to_owned());
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!(
                "could not create product receipt directory {}: {error}",
                parent.display()
            )
        })?;
    }
    image::save_buffer_with_format(
        path,
        &frame.rgba,
        frame.width,
        frame.height,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|error| {
        format!(
            "could not write product receipt {}: {error}",
            path.display()
        )
    })?;

    Ok(CapturedComposition {
        _texture: texture,
        view,
        path: path.to_owned(),
        digest: frame.digest(),
    })
}
