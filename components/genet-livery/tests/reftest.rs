// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

use genet_livery::{
    Device, InteractionStates, LiveryDocument, StyleSet, emit_paint_list, layout, resolve_styles,
};
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

fn render_retained(
    html: &str,
    css: &str,
) -> (genet_livery::LiveryPaintList, genet_livery::LiveryPaintList) {
    let mut document = LiveryDocument::new(
        StaticDocument::parse(html),
        StyleSet::cambium(&[css]),
        Device::screen(320.0, 240.0),
    );
    let first = document.frame(320, 240).unwrap();
    let cached = document.frame(320, 240).unwrap();
    (first, cached)
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

fn painted_glyph_signature(list: &genet_livery::LiveryPaintList) -> Vec<String> {
    list.commands()
        .iter()
        .filter_map(|command| match command {
            PaintCmd::DrawText(run) => Some(run),
            _ => None,
        })
        .flat_map(|run| {
            run.glyphs.iter().map(move |glyph| {
                format!(
                    "font:{:?};size:{:?};color:{:?};glyph:{};point:{:?}",
                    run.font_instance, run.font_size, run.color, glyph.index, glyph.point
                )
            })
        })
        .collect()
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

    // Each generated literal is a bullet plus its following space.
    assert_eq!(
        painted_glyph_count(&with_markers),
        painted_glyph_count(&without_markers) + 4
    );
}

#[test]
fn inside_disc_markers_match_literal_bullet_text_in_item_order() {
    let candidate = render(
        "<html><body><ul><li>first</li><li>second</li></ul></body></html>",
        "* { margin: 0; padding: 0; } ul { list-style-position: inside; }",
    );
    let reference = render(
        "<html><body><div>• first</div><div>• second</div></body></html>",
        "* { margin: 0; padding: 0; }",
    );

    // Generated and authored text can be split into different DrawText runs.
    // Compare the ordered glyph stream, retaining font, color, and absolute
    // placement rather than accepting matching character counts alone.
    assert_eq!(
        painted_glyph_signature(&candidate),
        painted_glyph_signature(&reference)
    );
}

#[test]
fn retained_inside_disc_markers_match_literal_and_cached_frame() {
    let (candidate, candidate_cached) = render_retained(
        "<html><body><ul><li>first</li><li>second</li></ul></body></html>",
        "* { margin: 0; padding: 0; } ul { list-style-position: inside; }",
    );
    let (reference, _) = render_retained(
        "<html><body><div>• first</div><div>• second</div></body></html>",
        "* { margin: 0; padding: 0; }",
    );

    assert_eq!(
        painted_glyph_signature(&candidate),
        painted_glyph_signature(&reference)
    );
    assert_eq!(
        command_signature(&candidate_cached),
        command_signature(&candidate)
    );
}

#[test]
fn inside_decimal_markers_match_html_ordinals_and_nested_block_order() {
    let candidate = render(
        r#"<html><body><ol start=" -4legacy"><li>negative four</li><li value="-2legacy">negative two</li><li>negative one<ol><li>inner one</li><li value="4">inner four</li><li>inner five</li></ol></li><li class="hidden" value="500">hidden</li><li>zero</li></ol></body></html>"#,
        "* { margin: 0; padding: 0; } ol { list-style-position: inside; } .hidden { display: none; }",
    );
    let reference = render(
        "<html><body><div>-4. negative four</div><div>-2. negative two</div><div>-1. negative one<div>1. inner one</div><div>4. inner four</div><div>5. inner five</div></div><div>0. zero</div></body></html>",
        "* { margin: 0; padding: 0; }",
    );

    assert_eq!(
        painted_glyph_signature(&candidate),
        painted_glyph_signature(&reference)
    );
}

#[test]
fn retained_inside_decimal_markers_match_nested_literal_and_cached_frame() {
    let (candidate, candidate_cached) = render_retained(
        r#"<html><body><ol start=" -4legacy"><li>negative four</li><li value="-2legacy">negative two</li><li>negative one<ol><li>inner one</li><li value="4">inner four</li><li>inner five</li></ol></li><li class="hidden" value="500">hidden</li><li>zero</li></ol></body></html>"#,
        "* { margin: 0; padding: 0; } ol { list-style-position: inside; } .hidden { display: none; }",
    );
    let (reference, _) = render_retained(
        "<html><body><div>-4. negative four</div><div>-2. negative two</div><div>-1. negative one<div>1. inner one</div><div>4. inner four</div><div>5. inner five</div></div><div>0. zero</div></body></html>",
        "* { margin: 0; padding: 0; }",
    );

    assert_eq!(
        painted_glyph_signature(&candidate),
        painted_glyph_signature(&reference)
    );
    assert_eq!(
        command_signature(&candidate_cached),
        command_signature(&candidate)
    );
}

#[test]
fn reversed_ordered_lists_do_not_receive_ascending_decimal_markers() {
    let candidate = render(
        "<html><body><ol reversed><li>last</li><li>first</li></ol></body></html>",
        "* { margin: 0; padding: 0; } ol { list-style-position: inside; }",
    );
    let reference = render(
        "<html><body><div>last</div><div>first</div></body></html>",
        "* { margin: 0; padding: 0; }",
    );

    assert_eq!(
        painted_glyph_signature(&candidate),
        painted_glyph_signature(&reference)
    );
}
