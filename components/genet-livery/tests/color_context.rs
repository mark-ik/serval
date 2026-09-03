// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! C2 retained-style receipt: element scheme and host palette meet before
//! values enter the style plane.

use genet_livery::{InteractionStates, StyleSet, resolve_styles};
use genet_static_dom::StaticDocument;
use layout_dom_api::{LayoutDom, LocalName, Namespace, NodeKind};
use livery::{
    media::{Device, SystemPalette},
    values::{Color, ColorScheme, SystemColor},
};

fn by_id(
    dom: &StaticDocument,
    node: <StaticDocument as LayoutDom>::NodeId,
    expected: &str,
) -> Option<<StaticDocument as LayoutDom>::NodeId> {
    if dom.kind(node) == NodeKind::Element
        && dom.attribute(node, &Namespace::from(""), &LocalName::from("id")) == Some(expected)
    {
        return Some(node);
    }
    dom.dom_children(node)
        .find_map(|child| by_id(dom, child, expected))
}

#[test]
fn style_plane_uses_the_elements_scheme_without_changing_host_preference() {
    let document = StaticDocument::parse(
        "<html><body><div id=parent><i id=direct></i><i id=inherited></i></div></body></html>",
    );
    let styles = StyleSet::cambium(&[
        "#parent { color-scheme: light; background-color: Canvas; } \
         #direct { color-scheme: only dark; background-color: Canvas; } \
         #inherited { color-scheme: dark; background-color: inherit; }",
    ]);
    let mut palette = SystemPalette::default();
    palette.set(
        ColorScheme::Light,
        SystemColor::Canvas,
        "#102030".parse().unwrap(),
    );
    palette.set(
        ColorScheme::Dark,
        SystemColor::Canvas,
        "#d0e0f0".parse().unwrap(),
    );
    let mut device = Device::screen(320.0, 200.0);
    device.set_preferred_color_scheme(ColorScheme::Light);
    device.set_system_palette(palette);

    let plane = resolve_styles(&document, &styles, &device, &InteractionStates::default());
    let id = |expected| by_id(&document, document.document(), expected).expect("fixture id");

    assert_eq!(
        plane.get(id("parent")).unwrap().background_color,
        "#102030".parse::<Color>().unwrap()
    );
    assert_eq!(
        plane.get(id("direct")).unwrap().background_color,
        "#d0e0f0".parse::<Color>().unwrap()
    );
    assert_eq!(
        plane.get(id("inherited")).unwrap().background_color,
        "#102030".parse::<Color>().unwrap()
    );
    assert_eq!(device.preferred_color_scheme(), ColorScheme::Light);
}
