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

fn positioned_rects(body: &str) -> ([f32; 4], [f32; 4]) {
    let html =
        format!("<html><body style=\"margin:0;font:50px/3 sans-serif\">{body}</body></html>");
    let mut session = LiveryDocument::new(
        StaticDocument::parse(&html),
        StyleSet::cambium(&[]),
        Device::screen(800.0, 600.0),
    );
    session.frame(800, 600).expect("frame");
    let rect = |name| {
        let node = find(session.dom(), session.dom().document(), name).expect(name);
        session
            .fragment_rect(node)
            .unwrap_or_else(|| panic!("{name} fragment"))
    };
    (rect("rel"), rect("abs"))
}

#[test]
fn nested_inline_wrappers_preserve_the_positioned_ancestors_line_rectangle() {
    let reference = positioned_rects(
        "X<span id=\"rel\" style=\"position:relative\"><span id=\"abs\" style=\"position:absolute;left:0;top:-1em\">X</span></span>",
    );
    let nested = positioned_rects(
        "X<span><span><span id=\"rel\" style=\"position:relative\"><span id=\"abs\" style=\"position:absolute;left:0;top:-1em\">X</span></span></span></span>",
    );

    assert_eq!(nested, reference);
}
