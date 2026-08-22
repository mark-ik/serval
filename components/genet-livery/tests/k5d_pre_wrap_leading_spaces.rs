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

#[test]
fn pre_wrap_leading_space_keeps_its_break_before_a_full_width_word() {
    let html = "<html><body><div id=target><span class=first>A</span>\n<span> </span><span class=word>XXXXX</span></div></body></html>";
    let css = "html, body { margin: 0; } \
               #target { width: 5ch; white-space: pre-wrap; font: 20px/20px Ahem; } \
               .first { color: blue; } .word { color: lime; }";
    let mut session = LiveryDocument::new(
        StaticDocument::parse(html),
        StyleSet::cambium(&[css]),
        Device::screen(320.0, 240.0),
    );
    session.set_font_resource(
        "/fonts/Ahem.ttf",
        include_bytes!("../../../tests/wpt/tests/fonts/Ahem.ttf").to_vec(),
    );
    let frame = session.frame(320, 240).expect("frame");
    let target = find(session.dom(), session.dom().document(), "target").expect("target");
    let [_, _, width, _] = session.fragment_rect(target).expect("target fragment");
    let baseline = |color| {
        frame.commands().iter().find_map(|command| match command {
            PaintCmd::DrawText(run) if run.color == color => {
                run.glyphs.first().map(|glyph| glyph.point.y)
            },
            _ => None,
        })
    };
    let first = baseline(ColorF::new(0.0, 0.0, 1.0, 1.0)).expect("first line paints");
    let word = baseline(ColorF::new(0.0, 1.0, 0.0, 1.0)).expect("word paints");

    assert!((width - 100.0).abs() <= 0.5, "5ch Ahem width: {width}");
    assert!(
        (word - first - 40.0).abs() <= 0.5,
        "the preserved leading space owns the line before the 5ch word: first={first}, word={word}"
    );
}
