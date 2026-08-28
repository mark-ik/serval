use genet_livery::{Device, InteractionStates, StyleSet, layout, resolve_styles};
use genet_static_dom::StaticDocument;
use layout_dom_api::{LayoutDom, LocalName, Namespace, NodeKind};

fn find(
    document: &StaticDocument,
    node: <StaticDocument as LayoutDom>::NodeId,
    id: &str,
) -> Option<<StaticDocument as LayoutDom>::NodeId> {
    if document.kind(node) == NodeKind::Element
        && document.attribute(node, &Namespace::from(""), &LocalName::from("id")) == Some(id)
    {
        return Some(node);
    }
    document
        .dom_children(node)
        .find_map(|child| find(document, child, id))
}

#[test]
fn vertical_rl_row_wrap_reverse_keeps_logical_start_and_reverses_flex_lines() {
    let document = StaticDocument::parse(
        r#"<html><body><div id="flex">
          <div id="one"></div><div id="two"></div>
          <div id="three"></div><div id="four"></div>
        </div></body></html>"#,
    );
    let styles = resolve_styles(
        &document,
        &StyleSet::cambium(&[r#"
          html, body { margin: 0; }
          #flex {
            display: flex;
            writing-mode: vertical-rl;
            flex-flow: row wrap-reverse;
            align-items: start;
            width: 30px;
            height: 90px;
            border: 2px solid black;
          }
          #flex > div { flex: 0 0 40px; width: 10px; height: 40px; }
        "#]),
        &Device::screen(320.0, 240.0),
        &InteractionStates::default(),
    );
    let fragments = layout(&document, &styles, 320.0, 240.0).expect("layout");
    let rect = |id: &str| {
        let node = find(&document, document.document(), id).expect(id);
        let rect = fragments.get(node).expect("fragment").physical_rect();
        (rect.x, rect.y, rect.width, rect.height)
    };

    let flex = rect("flex");
    assert_eq!(flex.2 - 4.0, 30.0, "flex content width");
    assert_eq!((flex.2, flex.3), (34.0, 94.0));

    let relative = |id| {
        let child = rect(id);
        (child.0 - flex.0, child.1 - flex.1, child.2, child.3)
    };
    assert_eq!(relative("one"), (7.0, 2.0, 10.0, 40.0));
    assert_eq!(relative("two"), (7.0, 42.0, 10.0, 40.0));
    assert_eq!(relative("three"), (22.0, 2.0, 10.0, 40.0));
    assert_eq!(relative("four"), (22.0, 42.0, 10.0, 40.0));
}

#[test]
fn rtl_vertical_rl_column_wrap_uses_the_bottom_cross_start() {
    let document = StaticDocument::parse(
        r#"<html><body><div id="flex"><div id="one"></div><div id="two"></div><div id="three"></div><div id="four"></div></div></body></html>"#,
    );
    let styles = resolve_styles(
        &document,
        &StyleSet::cambium(&["html, body { margin: 0; } #flex { display: flex; direction: rtl; writing-mode: vertical-rl; flex-flow: column wrap; width: 40px; height: 30px; border: 1px solid black; } #flex > div { width: 20px; height: 15px; }"]),
        &Device::screen(320.0, 240.0),
        &InteractionStates::default(),
    );
    let fragments = layout(&document, &styles, 320.0, 240.0).expect("layout");
    let rect = |id: &str| {
        let node = find(&document, document.document(), id).expect(id);
        let rect = fragments.get(node).expect("fragment").physical_rect();
        (rect.x, rect.y)
    };
    assert_eq!(rect("one"), (21.0, 16.0));
    assert_eq!(rect("two"), (1.0, 16.0));
    assert_eq!(rect("three"), (21.0, 1.0));
    assert_eq!(rect("four"), (1.0, 1.0));
}
