// Copyright 2026 Mark AB (markik)
// SPDX-License-Identifier: MIT OR Apache-2.0

#![cfg(feature = "smolweb")]

use mere_document_lanes::{SmolwebDocument, SmolwebTheme};
use netrender::{ColorLoad, NetrenderOptions, boot, create_netrender_instance};

const WIDTH: u32 = 614;
const HEIGHT: u32 = 600;

fn target(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("smolweb streaming render receipt"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        format: Some(wgpu::TextureFormat::Rgba8Unorm),
        ..Default::default()
    });
    (texture, view)
}

fn dark_pixels(bytes: &[u8], top: u32, bottom: u32) -> usize {
    (top..bottom)
        .flat_map(|y| (0..WIDTH).map(move |x| ((y * WIDTH + x) * 4) as usize))
        .filter(|&i| bytes[i] < 100 && bytes[i + 1] < 100 && bytes[i + 2] < 100)
        .count()
}

#[test]
fn keyed_render_of_complete_stream_keeps_prefix_and_tail() {
    let url = "gemini://x.test/streaming.gmi";
    let prefix = "# Streaming prefix visible\nThis arrived before connection close.\n";
    let complete = concat!(
        "# Streaming prefix visible\n",
        "This arrived before connection close.\n",
        "## Streaming tail arrived\n",
        "The terminal body is complete.\n",
    );
    let mut document = SmolwebDocument::parse(url, prefix, SmolwebTheme::Plain);
    let handles = boot().expect("wgpu boot");
    let renderer = create_netrender_instance(
        handles.clone(),
        NetrenderOptions {
            tile_cache_size: Some(64),
            enable_vello: true,
            ..Default::default()
        },
    )
    .expect("renderer");
    let surface = 17;

    let mut foreign = netrender::Scene::new(WIDTH, HEIGHT);
    foreign.push_rect(0.0, 0.0, 120.0, 120.0, [0.0, 0.0, 0.0, 1.0]);
    let render_foreign = || {
        let (_target, view) = target(&handles.device);
        renderer.render_vello_scaled_for(
            99,
            &foreign,
            &view,
            ColorLoad::Clear(wgpu::Color::WHITE),
            1.0,
        );
    };

    let prefix_scene = document.frame(WIDTH, HEIGHT);
    render_foreign();
    let (prefix_target, prefix_view) = target(&handles.device);
    renderer.render_vello_scaled_for(
        surface,
        &prefix_scene,
        &prefix_view,
        ColorLoad::Clear(wgpu::Color::WHITE),
        1.0,
    );
    render_foreign();
    let prefix_bytes = renderer
        .wgpu_device
        .read_rgba8_texture(&prefix_target, WIDTH, HEIGHT);
    assert!(dark_pixels(&prefix_bytes, 20, 125) > 100);
    for _ in 0..60 {
        render_foreign();
        let settled_prefix = document.frame(WIDTH, HEIGHT);
        let (_target, view) = target(&handles.device);
        renderer.render_vello_scaled_for(
            surface,
            &settled_prefix,
            &view,
            ColorLoad::Clear(wgpu::Color::WHITE),
            1.0,
        );
        render_foreign();
    }

    document.replace_body(url, complete);
    assert!(renderer.invalidate_surface_tiles(surface));
    let complete_scene = document.frame(WIDTH, HEIGHT);
    render_foreign();
    let (complete_target, complete_view) = target(&handles.device);
    renderer.render_vello_scaled_for(
        surface,
        &complete_scene,
        &complete_view,
        ColorLoad::Clear(wgpu::Color::WHITE),
        1.0,
    );
    render_foreign();
    let complete_bytes = renderer
        .wgpu_device
        .read_rgba8_texture(&complete_target, WIDTH, HEIGHT);

    assert!(
        dark_pixels(&complete_bytes, 20, 125) > 100,
        "complete render lost the prefix"
    );
    assert!(
        dark_pixels(&complete_bytes, 125, 240) > 100,
        "complete render omitted the tail"
    );
}
