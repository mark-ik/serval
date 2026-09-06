/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::collections::HashMap;

use genet_livery::{
    Device, InteractionStates, StyleSet,
    emit_paint_list_with_text_system_scrolled_with_images_and_external_textures, layout,
    resolve_styles,
};
use genet_static_dom::StaticDocument;
use layout_dom_api::{LayoutDom, NodeKind};
use paint_list_api::{DeviceIntSize, PaintCmd, PaintList};

fn canvas_id(document: &StaticDocument) -> genet_static_dom::StaticNodeId {
    fn visit(
        document: &StaticDocument,
        node: genet_static_dom::StaticNodeId,
    ) -> Option<genet_static_dom::StaticNodeId> {
        if document.kind(node) == NodeKind::Element
            && document
                .element_name(node)
                .is_some_and(|name| name.local.as_ref().eq_ignore_ascii_case("canvas"))
        {
            return Some(node);
        }
        document
            .dom_children(node)
            .into_iter()
            .find_map(|child| visit(document, child))
    }
    visit(document, document.document()).expect("canvas fixture")
}

fn render(
    document: &StaticDocument,
    css: &str,
    trusted: &HashMap<genet_static_dom::StaticNodeId, u64>,
) -> genet_livery::LiveryPaintList {
    let styles = resolve_styles(
        document,
        &StyleSet::cambium(&[css]),
        &Device::screen(320.0, 240.0),
        &InteractionStates::default(),
    );
    let fragments = layout(document, &styles, 320.0, 240.0).expect("layout");
    let mut text = genet_livery::TextSystem::new();
    emit_paint_list_with_text_system_scrolled_with_images_and_external_textures(
        document,
        &styles,
        &fragments,
        DeviceIntSize::new(320, 240),
        1,
        &mut text,
        &HashMap::new(),
        &HashMap::new(),
        trusted,
    )
}

#[test]
fn trusted_canvas_emits_content_box_draw_in_dom_paint_order() {
    let html = r#"<html><body><div class=back></div><canvas data-genet-external-texture-key="41"></canvas><div class=front></div></body></html>"#;
    let document = StaticDocument::parse(html);
    let canvas = canvas_id(&document);
    let mut trusted = HashMap::new();
    trusted.insert(canvas, 41);
    let list = render(
        &document,
        "body { margin: 0; } .back,.front { display: block; width: 20px; height: 20px; } .back { background: red; } .front { background: blue; } canvas { display: block; width: 20px; height: 20px; padding: 2px; border: 1px solid black; }",
        &trusted,
    );
    let external = list
        .commands()
        .iter()
        .position(|command| matches!(command, PaintCmd::DrawExternalTexture(_)))
        .expect("trusted canvas draw");
    let PaintCmd::DrawExternalTexture(item) = &list.commands()[external] else {
        unreachable!()
    };
    assert_eq!(item.texture_key, 41);
    assert_eq!(
        (item.placement.bounds.min.x, item.placement.bounds.min.y),
        (3.0, 23.0)
    );
    assert_eq!(
        (item.placement.bounds.max.x, item.placement.bounds.max.y),
        (23.0, 43.0)
    );
    assert!(
        list.commands()[..external]
            .iter()
            .any(|command| matches!(command, PaintCmd::DrawRect(_)))
    );
    assert!(
        list.commands()[external + 1..]
            .iter()
            .any(|command| matches!(command, PaintCmd::DrawRect(_)))
    );
}

#[test]
fn missing_or_forged_canvas_keys_emit_no_external_draw() {
    let html =
        r#"<html><body><canvas data-genet-external-texture-key="41"></canvas></body></html>"#;
    let document = StaticDocument::parse(html);
    let canvas = canvas_id(&document);
    for trusted in [HashMap::new(), HashMap::from([(canvas, 99)])] {
        let list = render(&document, "canvas { width: 20px; height: 20px; }", &trusted);
        assert!(
            !list
                .commands()
                .iter()
                .any(|command| matches!(command, PaintCmd::DrawExternalTexture(_)))
        );
    }
}

#[test]
fn canvas_external_draw_carries_element_opacity() {
    let html = r#"<html><body><canvas data-genet-external-texture-key="7"></canvas></body></html>"#;
    let document = StaticDocument::parse(html);
    let canvas = canvas_id(&document);
    let list = render(
        &document,
        "canvas { width: 20px; height: 20px; opacity: .4; }",
        &HashMap::from([(canvas, 7)]),
    );
    let PaintCmd::DrawExternalTexture(item) = list
        .commands()
        .iter()
        .find(|command| matches!(command, PaintCmd::DrawExternalTexture(_)))
        .expect("external draw")
    else {
        unreachable!()
    };
    assert!((item.opacity - 0.4).abs() < 0.001);
}
