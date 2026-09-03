// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! C3 receipts: contextual colors become numeric only at CSSOM, animation,
//! and paint consumers, under each element's actual color context.

use genet_livery::{Device, InteractionStates, LiveryDocument, StyleSet, resolve_styles};
use genet_static_dom::StaticDocument;
use layout_dom_api::{LayoutDom, LocalName, Namespace, NodeKind};
use livery::{
    media::SystemPalette,
    selector::StatePseudoClass,
    values::{Color, ColorScheme, SystemColor},
};
use paint_list_api::{BorderDetails, ColorF, PaintCmd, PaintList};

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

fn red() -> ColorF {
    ColorF::new(1.0, 0.0, 0.0, 1.0)
}

#[test]
fn inherited_currentcolor_re_resolves_for_cssom_and_every_paint_consumer() {
    let document = StaticDocument::parse(
        r#"<html><body><div id="parent"><div id="child">color</div></div></body></html>"#,
    );
    let child = by_id(&document, document.document(), "child").expect("child");
    let styles = StyleSet::cambium(&[r#"
        #parent { color: rgb(0, 0, 255); }
        #child {
            display: block;
            width: 100px;
            height: 30px;
            color: rgb(255, 0, 0);
            background-color: inherit;
            background-image: linear-gradient(currentcolor, currentcolor);
            border: 2px solid currentcolor;
            box-shadow: 0 0 2px currentcolor;
            text-decoration-color: currentcolor;
        }
        #parent { background-color: color-mix(in srgb, currentcolor 100%, white); }
    "#]);
    let mut retained = LiveryDocument::new(document, styles, Device::screen(200.0, 100.0));

    assert_eq!(
        retained
            .computed_style(child, "background-color")
            .as_deref(),
        Some("color(srgb 1 0 0)")
    );
    assert_eq!(
        retained
            .computed_style(child, "text-decoration-color")
            .as_deref(),
        Some("rgb(255, 0, 0)")
    );
    let list = retained.frame(200, 100).expect("contextual frame");

    assert!(
        list.commands()
            .iter()
            .any(|command| { matches!(command, PaintCmd::DrawRect(rect) if rect.color == red()) })
    );
    assert!(list.commands().iter().any(|command| {
        matches!(
            command,
            PaintCmd::DrawLinearGradient(gradient)
                if gradient.gradient.stops.iter().all(|stop| stop.color == red())
        )
    }));
    assert!(list.commands().iter().any(|command| {
        matches!(
            command,
            PaintCmd::DrawBorder(border)
                if matches!(
                    &border.details,
                    BorderDetails::Normal(sides)
                        if sides.top.color == red()
                            && sides.right.color == red()
                            && sides.bottom.color == red()
                            && sides.left.color == red()
                )
        )
    }));
    assert!(list.commands().iter().any(|command| {
        matches!(command, PaintCmd::DrawShadow(shadow) if shadow.color == red())
    }));
    assert!(
        list.commands()
            .iter()
            .any(|command| { matches!(command, PaintCmd::DrawText(run) if run.color == red()) })
    );
}

#[test]
fn contrast_color_uses_each_elements_foreground_without_a_fallback() {
    let document = StaticDocument::parse(
        r#"<html><body><div id="dark"></div><div id="light"></div></body></html>"#,
    );
    let dark = by_id(&document, document.document(), "dark").expect("dark");
    let light = by_id(&document, document.document(), "light").expect("light");
    let styles = StyleSet::cambium(&[
        "#dark, #light { display: block; width: 40px; height: 20px; background-color: contrast-color(currentcolor); } \
         #dark { color: black; } #light { color: white; }",
    ]);
    let mut retained = LiveryDocument::new(document, styles, Device::screen(200.0, 100.0));

    let dark_css = retained
        .computed_style(dark, "background-color")
        .expect("dark contrast CSSOM");
    let light_css = retained
        .computed_style(light, "background-color")
        .expect("light contrast CSSOM");
    assert_ne!(dark_css, light_css);
    let list = retained.frame(200, 100).expect("contrast frame");
    assert!(list.commands().iter().any(|command| {
        matches!(command, PaintCmd::DrawRect(rect) if rect.color == ColorF::WHITE)
    }));
    assert!(list.commands().iter().any(|command| {
        matches!(command, PaintCmd::DrawRect(rect) if rect.color == ColorF::BLACK)
    }));
}

#[test]
fn scheme_and_palette_changes_invalidate_cssom_and_paint() {
    let document = StaticDocument::parse(r#"<html><body><div id="card"></div></body></html>"#);
    let card = by_id(&document, document.document(), "card").expect("card");
    let styles = StyleSet::cambium(&[
        "#card { display: block; width: 100px; height: 30px; color-scheme: light dark; background-color: Canvas; }",
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
    let mut device = Device::screen(200.0, 100.0);
    device.set_system_palette(palette);
    let mut retained = LiveryDocument::new(document, styles, device);

    let light = ColorF::new(
        f32::from(0x10_u8) / 255.0,
        f32::from(0x20_u8) / 255.0,
        f32::from(0x30_u8) / 255.0,
        1.0,
    );
    let dark = ColorF::new(
        f32::from(0xd0_u8) / 255.0,
        f32::from(0xe0_u8) / 255.0,
        f32::from(0xf0_u8) / 255.0,
        1.0,
    );
    assert!(
        retained
            .frame(200, 100)
            .unwrap()
            .commands()
            .iter()
            .any(|command| { matches!(command, PaintCmd::DrawRect(rect) if rect.color == light) })
    );
    assert_eq!(
        retained.computed_style(card, "background-color").as_deref(),
        Some("rgb(16, 32, 48)")
    );

    assert!(retained.set_preferred_color_scheme(ColorScheme::Dark));
    assert!(!retained.set_preferred_color_scheme(ColorScheme::Dark));
    assert_eq!(
        retained.computed_style(card, "background-color").as_deref(),
        Some("rgb(208, 224, 240)")
    );
    assert!(
        retained
            .frame(200, 100)
            .unwrap()
            .commands()
            .iter()
            .any(|command| { matches!(command, PaintCmd::DrawRect(rect) if rect.color == dark) })
    );

    let mut updated = palette;
    updated.set(
        ColorScheme::Dark,
        SystemColor::Canvas,
        "#224466".parse::<Color>().unwrap(),
    );
    let changed = ColorF::new(
        f32::from(0x22_u8) / 255.0,
        f32::from(0x44_u8) / 255.0,
        f32::from(0x66_u8) / 255.0,
        1.0,
    );
    assert!(retained.set_system_palette(updated));
    assert!(!retained.set_system_palette(updated));
    assert_eq!(
        retained.computed_style(card, "background-color").as_deref(),
        Some("rgb(34, 68, 102)")
    );
    assert!(
        retained
            .frame(200, 100)
            .unwrap()
            .commands()
            .iter()
            .any(|command| {
                matches!(command, PaintCmd::DrawRect(rect) if rect.color == changed)
            })
    );
}

#[test]
fn contextual_transition_interpolates_numeric_endpoints() {
    let document = StaticDocument::parse(r#"<html><body><div id="card">color</div></body></html>"#);
    let card = by_id(&document, document.document(), "card").expect("card");
    let styles = StyleSet::cambium(&[r#"
        #card {
            display: block;
            width: 100px;
            height: 30px;
            color: red;
            background-color: currentcolor;
            transition: all 100ms;
        }
        #card:hover { background-color: blue; }
    "#]);
    let mut retained = LiveryDocument::new(document, styles, Device::screen(200.0, 100.0));
    retained.frame(200, 100).expect("initial frame");
    retained
        .interactions_mut()
        .set(card, StatePseudoClass::Hover, true);
    retained.frame(200, 100).expect("transition start");
    retained.pump(50.0);
    let middle = retained.frame(200, 100).expect("transition sample");

    let rectangles = middle
        .commands()
        .iter()
        .filter_map(|command| match command {
            PaintCmd::DrawRect(rect) => Some(rect.color),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        rectangles.iter().any(|color| {
            let color = *color;
            matches!(
                color,
                color if (color.r - 0.5).abs() < 0.01
                    && color.g.abs() < 0.01
                    && (color.b - 0.5).abs() < 0.01
            )
        }),
        "middle colors: {rectangles:?}"
    );
}

#[test]
fn style_plane_cssom_uses_the_retained_element_context() {
    let document = StaticDocument::parse(
        r#"<html><body><div id="parent"><div id="child"></div></div></body></html>"#,
    );
    let child = by_id(&document, document.document(), "child").expect("child");
    let styles = StyleSet::cambium(&[
        "#parent { color: blue; background-color: color-mix(in srgb, currentcolor 100%, white); } \
         #child { color: red; background-color: inherit; }",
    ]);
    let plane = resolve_styles(
        &document,
        &styles,
        &Device::screen(200.0, 100.0),
        &InteractionStates::default(),
    );

    assert_eq!(
        plane.computed_style(child, "background-color").as_deref(),
        Some("color(srgb 1 0 0)")
    );
}

#[test]
fn cssom_distinguishes_direct_and_inherited_system_colors() {
    let document = StaticDocument::parse(
        r#"<html><body><div id="parent"><i id="direct"></i><i id="inherited"></i></div></body></html>"#,
    );
    let direct = by_id(&document, document.document(), "direct").expect("direct");
    let inherited = by_id(&document, document.document(), "inherited").expect("inherited");
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
    let mut device = Device::screen(200.0, 100.0);
    device.set_system_palette(palette);
    let plane = resolve_styles(&document, &styles, &device, &InteractionStates::default());

    assert_eq!(
        plane.computed_style(direct, "background-color").as_deref(),
        Some("rgb(208, 224, 240)")
    );
    assert_eq!(
        plane
            .computed_style(inherited, "background-color")
            .as_deref(),
        Some("rgb(16, 32, 48)")
    );
}
