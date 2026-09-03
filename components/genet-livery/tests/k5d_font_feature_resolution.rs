// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

use genet_livery::{Device, LiveryDocument, StyleSet};
use genet_static_dom::StaticDocument;
use layout_dom_api::{LayoutDom, LocalName, Namespace, NodeKind};
use paint_list_api::{ColorF, PaintCmd, PaintList};

fn find(
    dom: &StaticDocument,
    node: <StaticDocument as LayoutDom>::NodeId,
    needle: &str,
) -> Option<<StaticDocument as LayoutDom>::NodeId> {
    if dom.kind(node) == NodeKind::Element
        && dom.attribute(node, &Namespace::from(""), &LocalName::from("id")) == Some(needle)
    {
        return Some(node);
    }
    dom.dom_children(node)
        .find_map(|child| find(dom, child, needle))
}

fn glyph_count(frame: &genet_livery::LiveryPaintList, color: ColorF) -> usize {
    frame
        .commands()
        .iter()
        .filter_map(|command| match command {
            PaintCmd::DrawText(run) if run.color == color => Some(run.glyphs.len()),
            _ => None,
        })
        .sum()
}

#[test]
fn font_feature_precedence_reaches_parley_with_authored_face_aliases() {
    let html = "<html><body>\
        <span class=face-off>fi</span>\
        <span class=variant-on>fi</span>\
        <span class=variant-off>fi</span>\
        <span class=spacing-off>fi</span>\
        <span class=explicit-on>fi</span>\
        <span class=dlig-face>st</span>\
        <span class=dlig-spacing>st</span>\
        <span class=dlig-explicit>st</span>\
        </body></html>";
    let css = "
        @font-face { font-family: face-off; src: url(/fonts/Lato-Medium-Liga.ttf);
                     font-feature-settings: 'liga' off; }
        @font-face { font-family: face-on; src: url(/fonts/Lato-Medium-Liga.ttf);
                     font-feature-settings: 'liga' on; }
        @font-face { font-family: dlig-on; src: url(/fonts/Lato-Medium-Liga.ttf);
                     font-feature-settings: 'dlig' on; }
        span { display: block; font-size: 32px; }
        .face-off { color: #010101; font-family: face-off; }
        .variant-on { color: #020202; font-family: face-off;
                      font-variant-ligatures: common-ligatures; }
        .variant-off { color: #030303; font-family: face-on;
                       font-variant-ligatures: no-common-ligatures; }
        .spacing-off { color: #040404; font-family: face-on; letter-spacing: 0.1em; }
        .explicit-on { color: #050505; font-family: face-on; letter-spacing: 0.1em;
                       font-feature-settings: 'liga' on; }
        .dlig-face { color: #060606; font-family: dlig-on; }
        .dlig-spacing { color: #070707; font-family: dlig-on; letter-spacing: 0.1em; }
        .dlig-explicit { color: #080808; font-family: dlig-on; letter-spacing: 0.1em;
                         font-feature-settings: 'dlig' on; }
    ";
    let mut session = LiveryDocument::new(
        StaticDocument::parse(html),
        StyleSet::cambium(&[css]),
        Device::screen(320.0, 400.0),
    );
    session.set_font_resource(
        "/fonts/Lato-Medium-Liga.ttf",
        include_bytes!("../../../tests/wpt/tests/fonts/Lato-Medium-Liga.ttf").to_vec(),
    );
    let frame = session.frame(320, 400).expect("font feature frame");
    let color = |channel: u8| {
        let channel = f32::from(channel) / 255.0;
        ColorF::new(channel, channel, channel, 1.0)
    };

    assert_eq!(
        glyph_count(&frame, color(1)),
        2,
        "face descriptor disables liga"
    );
    assert_eq!(
        glyph_count(&frame, color(2)),
        1,
        "variant overrides the face"
    );
    assert_eq!(glyph_count(&frame, color(3)), 2, "variant can disable liga");
    assert_eq!(
        glyph_count(&frame, color(4)),
        2,
        "letter spacing disables liga"
    );
    assert_eq!(glyph_count(&frame, color(5)), 1, "explicit liga wins last");
    assert_eq!(
        glyph_count(&frame, color(6)),
        1,
        "face descriptor enables dlig"
    );
    assert_eq!(
        glyph_count(&frame, color(7)),
        2,
        "letter spacing disables dlig"
    );
    assert_eq!(glyph_count(&frame, color(8)), 1, "explicit dlig wins last");
}

#[test]
fn join_controls_and_presentation_ligatures_keep_the_face_line_metrics() {
    let html = "<html><body><div id=plain>fi</div><div id=join>f&zwnj;i</div>\
                <div id=presentation>&#xfb01;</div><div id=spacing-plain class=spaced>st</div>\
                <div id=spacing-join class=spaced>s&zwnj;t</div></body></html>";
    let css = "@font-face { font-family: face; src: url(/fonts/Lato-Medium-Liga.ttf); } \
               div { position: absolute; top: 0; left: 0; \
                     font-family: face; font-size: 32px; } \
               .spaced { letter-spacing: 0.1em; font-feature-settings: 'dlig' off; }";
    let mut session = LiveryDocument::new(
        StaticDocument::parse(html),
        StyleSet::cambium(&[css]),
        Device::screen(320.0, 240.0),
    );
    session.set_font_resource(
        "/fonts/Lato-Medium-Liga.ttf",
        include_bytes!("../../../tests/wpt/tests/fonts/Lato-Medium-Liga.ttf").to_vec(),
    );
    session.frame(320, 240).expect("line metric frame");
    let height = |id| {
        let node = find(session.dom(), session.dom().document(), id).expect("fixture node");
        session.fragment_rect(node).expect("fixture fragment")[3]
    };

    assert_eq!(
        height("join"),
        height("plain"),
        "ZWNJ stays in the selected face"
    );
    assert_eq!(
        height("presentation"),
        height("plain"),
        "the presentation ligature keeps the selected face metrics"
    );
    let width = |id| {
        let node = find(session.dom(), session.dom().document(), id).expect("fixture node");
        session.fragment_rect(node).expect("fixture fragment")[2]
    };
    assert_eq!(
        width("spacing-join"),
        width("spacing-plain"),
        "ZWNJ does not receive letter spacing"
    );
}
