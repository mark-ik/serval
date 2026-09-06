// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

use genet_livery::{Device, InteractionStates, StyleSet, emit_paint_list, layout, resolve_styles};
use genet_static_dom::StaticDocument;
use paint_list_api::{PaintCmd, PaintList};

fn render(html: &str, css: &str) -> genet_livery::LiveryPaintList {
    let document = StaticDocument::parse(html);
    let styles = resolve_styles(
        &document,
        &StyleSet::cambium(&[css]),
        &Device::screen(320.0, 240.0),
        &InteractionStates::default(),
    );
    let fragments = layout(&document, &styles, 320.0, 240.0).unwrap();
    emit_paint_list(
        &document,
        &styles,
        &fragments,
        paint_list_api::DeviceIntSize::new(320, 240),
        1,
    )
}

fn command_signature(list: &genet_livery::LiveryPaintList) -> Vec<String> {
    list.commands()
        .iter()
        .map(|command| match command {
            PaintCmd::DrawRect(rect) => format!("rect:{rect:?}"),
            PaintCmd::DrawLinearGradient(gradient) => format!("linear-gradient:{gradient:?}"),
            PaintCmd::DrawBorder(border) => format!("border:{border:?}"),
            PaintCmd::PushClip(clip) => format!("push-clip:{clip:?}"),
            PaintCmd::PopClip => "pop-clip".to_owned(),
            other => format!("other:{other:?}"),
        })
        .collect()
}

fn painted_glyph_count(list: &genet_livery::LiveryPaintList) -> usize {
    list.commands()
        .iter()
        .filter_map(|command| match command {
            PaintCmd::DrawText(run) => Some(run.glyphs.len()),
            _ => None,
        })
        .sum()
}

#[test]
fn equivalent_inline_and_stylesheet_cases_share_a_native_paint_receipt() {
    let actual = render(
        r#"<html><body><div class="card"></div></body></html>"#,
        ".card { width: 80px; height: 40px; background-color: #101010; \
                 background-image: linear-gradient(red, blue); \
                 border: 2px solid white; border-radius: 8px; }",
    );
    let reference = render(
        r#"<html><body><div style="width: 80px; height: 40px; background-color: #101010; background-image: linear-gradient(red, blue); border: 2px solid white; border-radius: 8px;"></div></body></html>"#,
        "",
    );

    assert_eq!(command_signature(&actual), command_signature(&reference));
}

#[test]
fn inside_disc_markers_generate_before_each_list_item() {
    let html = "<html><body><ul><li>first</li><li>second</li></ul></body></html>";
    let with_markers = render(
        html,
        "ul { margin: 0; padding-left: 0; list-style-position: inside; }",
    );
    let without_markers = render(
        html,
        "ul { margin: 0; padding-left: 0; list-style-position: inside; } li { list-style-type: none; }",
    );

    // Both literal bullets are in the same inline runs as their items.
    assert_eq!(
        painted_glyph_count(&with_markers),
        painted_glyph_count(&without_markers) + 2
    );
}
