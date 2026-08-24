//! Receipts for ordinary sibling tables in block flow.

use std::collections::HashMap;

use genet_livery::{
    Device, InteractionStates, LiveryLayout, LiveryPaintList, StyleSet, TextSystem, ViewportSizes,
    emit_paint_list_with_text_system, layout_with_text_system, resolve_styles,
};
use genet_static_dom::StaticDocument;
use layout_dom_api::{LayoutDom, LocalName, Namespace, NodeKind};
use paint_list_api::{DeviceIntSize, PaintCmd, PaintList};

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
    paint: LiveryPaintList,
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

    fn normalized_rect(&self, id: &str) -> (f32, f32, f32, f32) {
        let layer = self.rect("layer");
        let rect = self.rect(id);
        (rect.0 - layer.0, rect.1 - layer.1, rect.2, rect.3)
    }

    fn normalized_glyph_points(&self) -> Vec<(f32, f32)> {
        let layer = self.rect("layer");
        self.paint
            .commands()
            .iter()
            .flat_map(|command| match command {
                PaintCmd::DrawText(run) => run
                    .glyphs
                    .iter()
                    .map(|glyph| (glyph.point.x - layer.0, glyph.point.y - layer.1))
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            })
            .collect()
    }
}

fn table(row: &str) -> String {
    format!(
        "<table id='t{row}' cellpadding=0 cellspacing=0 style='margin:0;padding:0;border:none'>
         <tr>
           <td id='r{row}c1'>Row {row}, Col 1</td>
           <td id='r{row}c2'>Row {row}, Col 2</td>
           <td id='r{row}c3'>Row {row}, Col 3</td>
         </tr>
         </table>"
    )
}

fn render_with_body_style(position: &str, body_style: &str, content: &str) -> Rendered {
    let document = StaticDocument::parse(&format!(
        "<body style='margin:0;font-family:monospace;{body_style}'>\
         <div style='position:relative;width:784px;height:300px'>\
         <div id=layer style='position:{position};top:0;left:0;padding:1px;font-size:2em'>\
         {content}</div></div></body>",
    ));
    let styles = resolve_styles(
        &document,
        &StyleSet::cambium(&[]),
        &Device::screen(800.0, 600.0),
        &InteractionStates::default(),
    );
    let mut text = TextSystem::new();
    let (styles, layout) = layout_with_text_system(
        &document,
        &styles,
        800.0,
        600.0,
        ViewportSizes::uniform(800.0, 600.0),
        &mut text,
        &HashMap::new(),
    )
    .expect("layout");
    let paint = emit_paint_list_with_text_system(
        &document,
        &styles,
        &layout,
        DeviceIntSize::new(800, 600),
        1,
        &mut text,
    );
    Rendered {
        document,
        layout,
        paint,
    }
}

fn render(position: &str, content: &str) -> Rendered {
    render_with_body_style(position, "white-space:nowrap", content)
}

fn assert_glyph_points_match(left: Rendered, right: Rendered) {
    let mut left = left.normalized_glyph_points();
    let mut right = right.normalized_glyph_points();
    let by_position = |left: &(f32, f32), right: &(f32, f32)| {
        left.1
            .total_cmp(&right.1)
            .then_with(|| left.0.total_cmp(&right.0))
    };
    left.sort_by(by_position);
    right.sort_by(by_position);
    assert_eq!(left.len(), right.len(), "rendered glyph counts");
    for (index, (left, right)) in left.into_iter().zip(right).enumerate() {
        assert!(
            (left.0 - right.0).abs() <= 0.5 && (left.1 - right.1).abs() <= 0.5,
            "glyph {index}: left={left:?}, right={right:?}"
        );
    }
}

#[test]
fn consecutive_tables_match_between_block_and_shrink_to_fit_containers() {
    let tables = format!(
        "\n    {}\n    {}\n    {}\n",
        table("1"),
        table("22"),
        table("333")
    );
    let block = render("relative", &tables);
    let shrink = render("absolute", &tables);
    for row in ["1", "22", "333"] {
        for column in 1..=3 {
            let id = format!("r{row}c{column}");
            let block = block.normalized_rect(&id);
            let shrink = shrink.normalized_rect(&id);
            for (axis, block, shrink) in [
                ("x", block.0, shrink.0),
                ("y", block.1, shrink.1),
                ("width", block.2, shrink.2),
                ("height", block.3, shrink.3),
            ] {
                assert!(
                    (block - shrink).abs() <= 0.5,
                    "#{id} {axis}: block {block}, shrink-to-fit {shrink}"
                );
            }
        }
    }
}

#[test]
fn inherited_nowrap_aligns_sibling_table_glyphs_with_block_rows() {
    let tables = format!(
        "\n    {}\n    {}\n    {}\n",
        table("1"),
        table("22"),
        table("333")
    );
    let rows = "\n    <div id=d1>Row 1, Col 1Row 1, Col 2Row 1, Col 3</div>\
                <div id=d22>Row 22, Col 1Row 22, Col 2Row 22, Col 3</div>\
                <div id=d333>Row 333, Col 1Row 333, Col 2Row 333, Col 3</div>\n    ";
    let tables = render("relative", &tables);
    let rows = render("relative", rows);
    let tables = tables.normalized_glyph_points();
    let rows = rows.normalized_glyph_points();
    assert_eq!(tables.len(), rows.len(), "table and block glyph counts");
    for (index, (table, row)) in tables.into_iter().zip(rows).enumerate() {
        assert!(
            (table.0 - row.0).abs() <= 0.5 && (table.1 - row.1).abs() <= 0.5,
            "glyph {index}: table={table:?}, block={row:?}"
        );
    }
}

#[test]
fn inferred_cells_with_nested_tables_match_html_table_glyphs() {
    let generated = render_with_body_style(
        "relative",
        "",
        "<span style='display:table-row'>\
           <span>Row 1, </span><span>Col 1</span>\
           <span style='display:table-cell'>Row 1, Col 2</span>\
           <span style='display:table'>Row 1, Col 3</span>\
         </span>\
         <span style='display:table-row'>\
           <span style='display:table-cell'>Row 22, Col 1</span>\
           <span>Row </span><span>22, </span><span>Col </span><span>2</span>\
           <span style='display:table-cell'>Row 22, Col 3</span>\
         </span>\
         <span style='display:table-row'>\
           <span style='display:inline-table'>Row 333, Col 1</span>\
           <span style='display:table-cell'>Row 333, Col 2</span>\
           <span>Row </span><span>333, </span><span>Col </span><span>3</span>\
         </span>",
    );
    let html = render_with_body_style(
        "relative",
        "",
        "<table cellpadding=0 cellspacing=0 style='margin:0;padding:0;border:none'>\
           <tr><td>Row 1, Col 1</td><td>Row 1, Col 2</td><td>Row 1, Col 3</td></tr>\
           <tr><td>Row 22, Col 1</td><td>Row 22, Col 2</td><td>Row 22, Col 3</td></tr>\
           <tr><td>Row 333, Col 1</td><td>Row 333, Col 2</td><td>Row 333, Col 3</td></tr>\
         </table>",
    );
    assert_glyph_points_match(generated, html);
}

#[test]
fn inferred_cell_block_child_matches_sibling_html_table_glyphs() {
    let generated = render(
        "relative",
        "<span style='display:table-row'>\
           <span>Row 1, </span><span>Col 1Row 1, </span>\
           <span>Col 2Row 1, </span><span>Col 3</span>\
         </span>\
         <span style='display:table-row'>\
           <span style='display:block'>Row 22, Col 1Row 22, Col 2Row 22, Col 3</span>\
         </span>\
         <span style='display:table-row'>\
           <span>Row 333, Col 1</span><span>Row 333, Col 2</span><span>Row 333, Col 3</span>\
         </span>",
    );
    let html = render(
        "relative",
        &format!("{}{}{}", table("1"), table("22"), table("333")),
    );
    assert_glyph_points_match(generated, html);
}

#[test]
fn nowrap_spanning_cell_does_not_steal_width_from_percentage_column() {
    let cell = "border:1px solid black;padding:0";
    let row = "<tr><td id=left style='{cell};width:90%'>\
               <div id=marker style='width:100%;height:20px'></div></td>\
               <td id=right style='{cell};{second}'></td></tr>\
               <tr><td id=span colspan=2 style='{cell};white-space:nowrap'>\
               Lorem ipsum dolor sit amet consectetuer adipiscing</td></tr>";
    let automatic = render(
        "relative",
        &format!(
            "<table id=t style='border-collapse:collapse;width:400px;font:16px sans-serif'>{}</table>",
            row.replace("{cell}", cell).replace("{second}", "")
        ),
    );
    let explicit = render(
        "relative",
        &format!(
            "<table id=t style='border-collapse:collapse;width:400px;font:16px sans-serif'>{}</table>",
            row.replace("{cell}", cell).replace("{second}", "width:10%")
        ),
    );
    let automatic = automatic.rect("left");
    let explicit = explicit.rect("left");
    assert!(
        (automatic.2 - explicit.2).abs() <= 0.5,
        "90% + auto cell width {}, 90% + 10% cell width {}",
        automatic.2,
        explicit.2
    );
}
