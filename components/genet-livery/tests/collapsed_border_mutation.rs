use genet_livery::{Device, LiveryDocument, StyleSet};
use genet_scripted_dom::{NodeId, ScriptedDom};
use layout_dom_api::{LayoutDom, LayoutDomMut, LocalName, Namespace, NodeKind, QualName};
use paint_list_api::{ColorF, PaintCmd, PaintList};

fn attr(name: &str) -> QualName {
    QualName::new(None, Namespace::from(""), LocalName::from(name))
}

fn by_id(dom: &ScriptedDom, expected: &str) -> NodeId {
    fn find(dom: &ScriptedDom, node: NodeId, expected: &str) -> Option<NodeId> {
        if dom.kind(node) == NodeKind::Element
            && dom.attribute(node, &Namespace::from(""), &LocalName::from("id")) == Some(expected)
        {
            return Some(node);
        }
        dom.dom_children(node)
            .find_map(|child| find(dom, child, expected))
    }

    find(dom, dom.document(), expected).expect("fixture id")
}

fn border_rectangles(list: &genet_livery::LiveryPaintList, color: ColorF) -> Vec<[f32; 4]> {
    list.commands()
        .iter()
        .filter_map(|command| match command {
            PaintCmd::DrawRect(rect) if rect.color == color => Some([
                rect.placement.bounds.min.x,
                rect.placement.bounds.min.y,
                rect.placement.bounds.max.x,
                rect.placement.bounds.max.y,
            ]),
            _ => None,
        })
        .collect()
}

fn collapsed_metrics(document: &LiveryDocument<ScriptedDom>) -> usize {
    document
        .table_shadow_ledger()
        .expect("completed frame has a table ledger")
        .collapsed_metrics
}

fn assert_fresh_collapsed_frame(document: &mut LiveryDocument<ScriptedDom>, label: &str) {
    let frame = document.frame(220, 100).expect(label);
    assert_eq!(frame.generation_id(), document.generation(), "{label}");
    assert!(collapsed_metrics(document) > 0, "{label}");
}

fn collapsed_document() -> LiveryDocument<ScriptedDom> {
    let mut dom = ScriptedDom::from_serialized_document(
        "<html><body><table id='table'><colgroup id='group'><col id='column'></colgroup>\
         <tbody id='body'><tr id='row'><td id='left'>left</td><td id='right'>right</td></tr></tbody>\
         </table></body></html>",
    );
    let mut initial_mutations = Vec::new();
    dom.drain_mutations(&mut initial_mutations);
    LiveryDocument::new(
        dom,
        StyleSet::cambium(&[
            "body { margin: 0; } table { border-collapse: collapse; color: black; } \
             td { width: 60px; height: 30px; border: 4px solid currentcolor; }",
        ]),
        Device::screen(220.0, 100.0),
    )
}

#[test]
fn retained_document_rebuilds_collapsed_geometry_and_paint_from_one_mutation_batch() {
    let mut document = collapsed_document();

    let initial = document.frame(220, 100).expect("initial collapsed frame");
    let black = border_rectangles(&initial, ColorF::BLACK);
    assert!(
        !black.is_empty(),
        "initial table has collapsed border paint"
    );
    assert_eq!(initial.generation_id(), document.generation());
    assert!(collapsed_metrics(&document) > 0);

    let (_, color_stats) = document.mutate_dom(|dom| {
        let table = by_id(dom, "table");
        dom.set_attribute(table, attr("style"), "color: red");
    });
    assert!(color_stats.restyled_elements > 0);
    let recolored = document.frame(220, 100).expect("color mutation frame");
    let red = border_rectangles(&recolored, ColorF::new(1.0, 0.0, 0.0, 1.0));
    assert_eq!(red, black, "color-only mutation preserves border geometry");
    assert_eq!(recolored.generation_id(), document.generation());
    assert!(collapsed_metrics(&document) > 0);

    let (_, width_stats) = document.mutate_dom(|dom| {
        let left = by_id(dom, "left");
        dom.set_attribute(left, attr("style"), "border: 12px solid red");
    });
    assert!(width_stats.restyled_elements > 0);
    let wider = document
        .frame(220, 100)
        .expect("border-width mutation frame");
    let wider_red = border_rectangles(&wider, ColorF::new(1.0, 0.0, 0.0, 1.0));
    assert_ne!(wider_red, red, "winning-width mutation rebuilds geometry");
    assert_eq!(wider.generation_id(), document.generation());
    assert!(collapsed_metrics(&document) > 0);

    document.mutate_dom(|dom| {
        let right = by_id(dom, "right");
        dom.remove(right);
    });
    let one_cell = document.frame(220, 100).expect("cell removal frame");
    let one_cell_red = border_rectangles(&one_cell, ColorF::new(1.0, 0.0, 0.0, 1.0));
    assert_ne!(
        one_cell_red, wider_red,
        "removed cells do not retain old border paint"
    );

    document.mutate_dom(|dom| {
        let column = by_id(dom, "column");
        dom.remove(column);
    });
    document.frame(220, 100).expect("column removal frame");

    document.mutate_dom(|dom| {
        let row = by_id(dom, "row");
        dom.remove(row);
    });
    let no_rows = document.frame(220, 100).expect("row removal frame");
    assert!(
        border_rectangles(&no_rows, ColorF::new(1.0, 0.0, 0.0, 1.0)).is_empty(),
        "removed rows cannot leave stale collapsed-border paint"
    );

    document.mutate_dom(|dom| {
        let group = by_id(dom, "group");
        dom.remove(group);
    });
    let no_groups = document.frame(220, 100).expect("group removal frame");
    assert!(
        border_rectangles(&no_groups, ColorF::new(1.0, 0.0, 0.0, 1.0)).is_empty(),
        "removed table groups cannot revive stale border paint"
    );
    assert_eq!(no_groups.generation_id(), document.generation());
}

#[test]
fn retained_document_rebuilds_every_dynamic_collapsed_candidate_field() {
    let mut document = collapsed_document();
    assert_fresh_collapsed_frame(&mut document, "initial candidate frame");

    document.mutate_dom(|dom| {
        let table = by_id(dom, "table");
        dom.set_attribute(table, attr("style"), "direction: rtl");
    });
    assert_fresh_collapsed_frame(&mut document, "direction mutation frame");

    document.mutate_dom(|dom| {
        let table = by_id(dom, "table");
        dom.set_attribute(
            table,
            attr("style"),
            "direction: rtl; writing-mode: vertical-rl",
        );
    });
    assert_fresh_collapsed_frame(&mut document, "writing-mode mutation frame");

    document.mutate_dom(|dom| {
        let row = by_id(dom, "row");
        dom.set_attribute(row, attr("style"), "border: 7px double green");
        let group = by_id(dom, "group");
        dom.set_attribute(group, attr("style"), "border: 8px solid blue");
        let column = by_id(dom, "column");
        dom.set_attribute(column, attr("style"), "border: 9px dashed red");
    });
    assert_fresh_collapsed_frame(&mut document, "origin-role mutation frame");

    document.mutate_dom(|dom| {
        let left = by_id(dom, "left");
        dom.set_attribute(left, attr("colspan"), "2");
        let right = by_id(dom, "right");
        dom.set_attribute(right, attr("style"), "border: 12px hidden red");
    });
    assert_fresh_collapsed_frame(&mut document, "span and hidden-style mutation frame");

    document.mutate_dom(|dom| {
        let row = by_id(dom, "row");
        let third = dom.create_element(QualName::new(
            None,
            Namespace::from(""),
            LocalName::from("td"),
        ));
        dom.set_attribute(third, attr("id"), "third");
        dom.set_attribute(third, attr("style"), "border: 10px solid purple");
        let text = dom.create_text("third");
        dom.append_child(third, text);
        dom.append_child(row, third);
    });
    assert_fresh_collapsed_frame(&mut document, "cell insertion frame");

    document.mutate_dom(|dom| {
        let row = by_id(dom, "row");
        let left = by_id(dom, "left");
        dom.move_before(row, left, None);
    });
    assert_fresh_collapsed_frame(&mut document, "cell move frame");

    document.mutate_dom(|dom| {
        let column = by_id(dom, "column");
        dom.set_attribute(column, attr("style"), "visibility: collapse");
    });
    assert_fresh_collapsed_frame(&mut document, "track-visibility mutation frame");
}
