use genet_livery::{Device, LiveryDocument, StyleSet};
use genet_static_dom::StaticDocument;
use layout_dom_api::{LayoutDom, LocalName, Namespace, NodeKind};

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

fn document_rects(html: &str, ids: &[&str]) -> Vec<[f32; 4]> {
    let mut session = LiveryDocument::new(
        StaticDocument::parse(html),
        StyleSet::cambium(&[]),
        Device::screen(800.0, 600.0),
    );
    session.frame(800, 600).expect("frame");
    ids.iter()
        .map(|name| {
            let id = find(session.dom(), session.dom().document(), name).expect(name);
            session
                .fragment_rect(id)
                .unwrap_or_else(|| panic!("{name} has a fragment"))
        })
        .collect()
}

fn assert_rect(actual: [f32; 4], expected: [f32; 4], what: &str) {
    assert!(
        actual
            .into_iter()
            .zip(expected)
            .all(|(actual, expected)| (actual - expected).abs() <= 0.5),
        "{what}: expected {expected:?}, got {actual:?}"
    );
}

#[test]
fn abspos_max_height_transfers_through_the_aspect_ratio_to_the_inline_size() {
    let item =
        "<div style=\"width:20px;height:10px;display:inline-block;vertical-align:bottom\"></div>";
    let html = format!(
        "<html><body style=\"margin:0\">\
         <div id=\"parent\" style=\"position:relative;height:100px\">\
           <div id=\"abs\" style=\"position:absolute;aspect-ratio:1/1;max-height:100%;background:green\">\
             {}\
           </div>\
         </div></body></html>",
        item.repeat(10)
    );
    let rects = document_rects(&html, &["parent", "abs"]);

    assert_rect(rects[0], [0.0, 0.0, 800.0, 100.0], "relative parent");
    assert_rect(rects[1], [0.0, 0.0, 100.0, 100.0], "aspect-ratio abspos");
}

#[test]
fn size_contained_abspos_uses_its_contain_intrinsic_size_before_ratio_clamping() {
    let html = "<html><body style=\"margin:0\">\
         <div id=\"parent\" style=\"position:relative;height:100px\">\
           <div id=\"abs\" style=\"position:absolute;aspect-ratio:1/1;max-height:100%;\
                min-height:0;contain:size;contain-intrinsic-size:500px 500px\"></div>\
         </div></body></html>";
    let rects = document_rects(html, &["parent", "abs"]);

    assert_rect(rects[0], [0.0, 0.0, 800.0, 100.0], "relative parent");
    assert_rect(
        rects[1],
        [0.0, 0.0, 100.0, 100.0],
        "size-contained aspect-ratio abspos",
    );
}
