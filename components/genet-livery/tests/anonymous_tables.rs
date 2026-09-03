// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Live receipts for CSS 2.1 anonymous table construction.

use genet_livery::{
    Device, InteractionStates, LiveryLayout, StyleSet, emit_paint_list, layout, resolve_styles,
};
use genet_static_dom::StaticDocument;
use layout_dom_api::{LayoutDom, LocalName, Namespace, NodeKind};
use paint_list_api::{ColorF, DeviceIntSize, PaintCmd, PaintList};

type NodeId = <StaticDocument as LayoutDom>::NodeId;

fn find(dom: &StaticDocument, node: NodeId, id: &str) -> Option<NodeId> {
    if dom.kind(node) == NodeKind::Element
        && dom.attribute(node, &Namespace::from(""), &LocalName::from("id")) == Some(id)
    {
        return Some(node);
    }
    dom.dom_children(node)
        .find_map(|child| find(dom, child, id))
}

struct Rendered {
    document: StaticDocument,
    layout: LiveryLayout<NodeId>,
}

impl Rendered {
    fn rect(&self, id: &str) -> (f32, f32, f32, f32) {
        let node = find(&self.document, self.document.document(), id)
            .unwrap_or_else(|| panic!("missing #{id}"));
        let fragment = self
            .layout
            .get(node)
            .or_else(|| self.layout.fragments_for_node(node).next())
            .unwrap_or_else(|| panic!("missing fragment for #{id}"));
        let rect = fragment.physical_rect();
        (rect.x, rect.y, rect.width, rect.height)
    }
}

fn render(html: &str) -> Rendered {
    let document = StaticDocument::parse(html);
    let styles = resolve_styles(
        &document,
        &StyleSet::cambium(&["body { font-family: monospace; } .host { font-size: 2em; }"]),
        &Device::screen(800.0, 600.0),
        &InteractionStates::default(),
    );
    let layout = layout(&document, &styles, 800.0, 600.0).expect("layout");
    Rendered { document, layout }
}

fn cell(prefix: &str, row: &str, column: u8, display: &str) -> String {
    format!(
        "<span id=\"{prefix}{row}{column}\" style=\"display:{display}\">Row {row}, Col {column}</span>"
    )
}

fn generated_structure() -> String {
    format!(
        "{} {} {} <span style=\"display:table-row-group\">\
         <span style=\"display:table-row\">{} {} {}</span>\
         <span style=\"display:table-row\">{} {} {}</span></span>",
        cell("c", "1", 1, "table-cell"),
        cell("c", "1", 2, "table-cell"),
        cell("c", "1", 3, "table-cell"),
        cell("c", "22", 1, "table-cell"),
        cell("c", "22", 2, "table-cell"),
        cell("c", "22", 3, "table-cell"),
        cell("c", "333", 1, "table-cell"),
        cell("c", "333", 2, "table-cell"),
        cell("c", "333", 3, "table-cell"),
    )
}

fn html_table() -> String {
    let td =
        |row: &str, column: u8| format!("<td id=\"h{row}{column}\">Row {row}, Col {column}</td>");
    format!(
        "<table cellpadding=0 cellspacing=0 style=\"margin:0;padding:0;border:none\">\
         <tr>{}{}{}</tr><tr>{}{}{}</tr><tr>{}{}{}</tr></table>",
        td("1", 1),
        td("1", 2),
        td("1", 3),
        td("22", 1),
        td("22", 2),
        td("22", 3),
        td("333", 1),
        td("333", 2),
        td("333", 3),
    )
}

fn assert_generated_table_matches_html(container_style: &str) {
    let actual = render(&format!(
        "<div class=host style=\"position:relative\"><div style=\"{container_style}\">{}</div></div>",
        generated_structure()
    ));
    let expected = render(&format!(
        "<div class=host style=\"position:relative\"><div style=\"{container_style}\">{}</div></div>",
        html_table()
    ));
    for row in ["1", "22", "333"] {
        for column in 1..=3 {
            let actual = actual.rect(&format!("c{row}{column}"));
            let expected = expected.rect(&format!("h{row}{column}"));
            for (actual, expected) in [
                (actual.0, expected.0),
                (actual.1, expected.1),
                (actual.2, expected.2),
                (actual.3, expected.3),
            ] {
                assert!(
                    (actual - expected).abs() <= 0.5,
                    "cell {row}/{column}: generated {actual:?}, HTML {expected:?}"
                );
            }
        }
    }
}

#[test]
fn generated_table_grid_reaches_table_layout_in_flow() {
    assert_generated_table_matches_html("position:relative;padding:1px");
}

#[test]
fn generated_table_grid_sizes_its_absolute_wrapper() {
    assert_generated_table_matches_html("position:absolute;top:0;padding:1px");
}

#[test]
fn html_column_backgrounds_paint_in_an_absolute_table() {
    let document = StaticDocument::parse(
        "<div><table id=red><colgroup><col style='background:yellow'>\
         <col style='background:cyan'><col style='background:lime'></colgroup>\
         <tr><td>Row 1, Col 1</td><td>Row 1, Col 2</td><td>Row 1, Col 3</td></tr>\
         <tr><td>Row 22, Col 1</td><td>Row 22, Col 2</td><td>Row 22, Col 3</td></tr>\
         </table><table id=green>\
         <tr><td>Row 1, Col 1</td><td>Row 1, Col 2</td><td>Row 1, Col 3</td></tr>\
         <tr><td>Row 22, Col 1</td><td>Row 22, Col 2</td><td>Row 22, Col 3</td></tr>\
         </table></div>",
    );
    let styles = resolve_styles(
        &document,
        &StyleSet::cambium(
            &["body { font-family:monospace } div { position:relative } \
             table { font-size:2em; border-spacing:0; position:absolute; top:1px; left:1px; right:1px } \
             td { padding:0 } #red { color:red } #green { color:green }"],
        ),
        &Device::screen(800.0, 600.0),
        &InteractionStates::default(),
    );
    let fragments = layout(&document, &styles, 800.0, 600.0).expect("layout");
    let list = emit_paint_list(
        &document,
        &styles,
        &fragments,
        DeviceIntSize::new(800, 600),
        1,
    );
    let bounds = |color| {
        list.commands().iter().find_map(|command| match command {
            PaintCmd::DrawRect(rect) if rect.color == color => Some(rect.placement.bounds),
            _ => None,
        })
    };
    let (Some(first), Some(second), Some(third)) = (
        bounds(ColorF::new(1.0, 1.0, 0.0, 1.0)),
        bounds(ColorF::new(0.0, 1.0, 1.0, 1.0)),
        bounds(ColorF::new(0.0, 1.0, 0.0, 1.0)),
    ) else {
        panic!("all three HTML column backgrounds must paint");
    };
    assert!(first.max.x <= second.min.x + 0.5);
    assert!(second.max.x <= third.min.x + 0.5);
    assert!(first.max.y - first.min.y > 30.0);
}

#[test]
fn overlapping_absolute_tables_keep_the_earlier_text_run() {
    let document = StaticDocument::parse(
        "<div><table id=red><tr><td>same text</td></tr></table>\
         <table id=green><tr><td>same text</td></tr></table></div>",
    );
    let styles = resolve_styles(
        &document,
        &StyleSet::cambium(
            &["body { font-family:monospace } div { position:relative } \
             table { font-size:2em; border-spacing:0; position:absolute; top:1px; left:1px } \
             td { padding:0 } #red { color:red } #green { color:green }"],
        ),
        &Device::screen(800.0, 600.0),
        &InteractionStates::default(),
    );
    let fragments = layout(&document, &styles, 800.0, 600.0).expect("layout");
    let list = emit_paint_list(
        &document,
        &styles,
        &fragments,
        DeviceIntSize::new(800, 600),
        1,
    );
    let text_colors: Vec<_> = list
        .commands()
        .iter()
        .filter_map(|command| match command {
            PaintCmd::DrawText(run) => Some(run.color),
            _ => None,
        })
        .collect();
    assert!(
        text_colors
            .iter()
            .any(|color| *color == ColorF::new(1.0, 0.0, 0.0, 1.0)),
        "the earlier table's red text run must remain in the paint list"
    );
    assert!(
        text_colors.iter().any(|color| {
            color.r == 0.0 && (color.g - 128.0 / 255.0).abs() < f32::EPSILON && color.b == 0.0
        }),
        "the later table's green text run must remain in the paint list"
    );
}
