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
fn horizontal_block_flow_sizes_vertical_rl_row_wrap_reverse_by_its_flex_cross_size() {
    for (direction, items, expected) in [
        (
            "row",
            r#"<div id="one" class="item" style="background: grey"></div>
              <div id="two" class="item" style="background: yellow"></div>
              <div id="three" class="item" style="background: orange"></div>
              <div id="four" class="item" style="background: blue"></div>"#,
            [(2.0, 2.0), (2.0, 47.0), (17.0, 2.0), (17.0, 47.0)],
        ),
        (
            "row-reverse",
            r#"<div id="one" class="item" style="background: yellow"></div>
              <div id="two" class="item" style="background: grey"></div>
              <div id="three" class="item" style="background: blue"></div>
              <div id="four" class="item" style="background: orange"></div>"#,
            [(2.0, 47.0), (2.0, 2.0), (17.0, 47.0), (17.0, 2.0)],
        ),
    ] {
        let document = StaticDocument::parse(&format!(
            r#"<html><body>
              <p>Pass condition: 4 rectangles, with colors in clockwise order starting from top-left: grey, orange, blue, yellow.
              <div id="flex" class="container">{items}</div>
            </body></html>"#
        ));
        let css = format!(
            ".container {{ display: flex; flex-flow: {direction} wrap-reverse; writing-mode: vertical-rl; border: 2px solid black; height: 90px; }} .item {{ width: 15px; height: 45px; float: right; }}"
        );
        let styles = resolve_styles(
            &document,
            &StyleSet::cambium(&[css.as_str()]),
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
        assert_eq!((flex.2, flex.3), (34.0, 94.0), "{direction} border box");
        for (id, (x, y)) in ["one", "two", "three", "four"].into_iter().zip(expected) {
            let child = rect(id);
            assert_eq!(
                (child.0 - flex.0, child.1 - flex.1, child.2, child.3),
                (x, y, 15.0, 45.0),
                "{direction} {id}"
            );
        }
    }
}

#[test]
fn rtl_vertical_rl_column_wrap_uses_the_bottom_cross_start() {
    let document = StaticDocument::parse(
        r#"<html><body><div id="flex"><div id="one"></div><div id="two"></div><div id="three"></div><div id="four"></div></div></body></html>"#,
    );
    let styles = resolve_styles(
        &document,
        &StyleSet::cambium(&[
            "html, body { margin: 0; } #flex { display: flex; direction: rtl; writing-mode: vertical-rl; flex-flow: column wrap; width: 40px; height: 30px; border: 1px solid black; } #flex > div { width: 20px; height: 15px; }",
        ]),
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

#[test]
fn rtl_vertical_columns_project_explicit_align_self_across_wrap_states() {
    let document = StaticDocument::parse(
        r#"<html><body>
          <div id="nowrap"><div id="nowrap-start"></div><div id="nowrap-flex"></div></div>
          <div id="wrap"><div id="wrap-start"></div><div id="wrap-flex"></div></div>
          <div id="vertical-lr"><div id="vertical-lr-end"></div></div>
        </body></html>"#,
    );
    let styles = resolve_styles(
        &document,
        &StyleSet::cambium(&[r#"
          html, body { margin: 0; }
          #nowrap, #wrap, #vertical-lr {
            display: flex;
            direction: rtl;
            width: 40px;
            height: 60px;
            border: 1px solid black;
          }
          #nowrap { writing-mode: vertical-rl; flex-flow: column nowrap; }
          #wrap { writing-mode: vertical-rl; flex-flow: column wrap-reverse; margin-top: 10px; }
          #vertical-lr { writing-mode: vertical-lr; flex-flow: column nowrap; margin-top: 10px; }
          #nowrap > div, #wrap > div, #vertical-lr > div {
            flex: 0 0 25px;
            width: 20px;
            height: 20px;
          }
          #nowrap-start, #wrap-start { align-self: start; }
          #nowrap-flex, #wrap-flex { align-self: flex-start; }
          #vertical-lr-end { align-self: end; }
        "#]),
        &Device::screen(320.0, 240.0),
        &InteractionStates::default(),
    );
    let fragments = layout(&document, &styles, 320.0, 240.0).expect("layout");
    let rect = |id: &str| {
        let node = find(&document, document.document(), id).expect(id);
        fragments.get(node).expect("fragment").physical_rect()
    };
    let relative = |container: &str, child: &str| {
        let container = rect(container);
        let child = rect(child);
        (child.x - container.x, child.y - container.y)
    };

    // CSS column's cross axis is the vertical inline axis. RTL vertical text
    // starts at the physical bottom, so `start` and no-wrap `flex-start`
    // lower to taffy's physical end for the projected physical row.
    assert_eq!(relative("nowrap", "nowrap-start").1, 41.0);
    assert_eq!(relative("nowrap", "nowrap-flex").1, 41.0);
    // In a wrapped reverse cross axis, flex-start remains flex-relative. The
    // complemented wrap bit owns that reversal, while `start` stays logical.
    assert_eq!(relative("wrap", "wrap-start").1, 11.0);
    assert_eq!(relative("wrap", "wrap-flex").1, 31.0);
    // Vertical-lr has the same RTL inline cross start, exercising the other
    // physical main-direction projection.
    assert_eq!(relative("vertical-lr", "vertical-lr-end").1, 1.0);
}

#[test]
fn flex_self_start_uses_the_subject_writing_mode() {
    let document = StaticDocument::parse(
        r#"<html><body><div id="flex">
          <div id="horizontal-start"></div><div id="vertical-start"></div>
          <div id="horizontal-end"></div><div id="vertical-end"></div>
        </div></body></html>"#,
    );
    let styles = resolve_styles(
        &document,
        &StyleSet::cambium(&[r#"
          html, body { margin: 0; }
          #flex {
            display: flex;
            writing-mode: vertical-rl;
            flex-flow: row nowrap;
            width: 60px;
            height: 60px;
            border: 1px solid black;
          }
          #flex > div { flex: 0 0 10px; width: 20px; height: 10px; }
          #horizontal-start, #vertical-start { align-self: self-start; }
          #horizontal-end, #vertical-end { align-self: self-end; }
          #horizontal-start, #horizontal-end { writing-mode: horizontal-tb; }
          #vertical-start, #vertical-end { writing-mode: vertical-rl; }
        "#]),
        &Device::screen(320.0, 240.0),
        &InteractionStates::default(),
    );
    let fragments = layout(&document, &styles, 320.0, 240.0).expect("layout");
    let rect = |id: &str| {
        let node = find(&document, document.document(), id).expect(id);
        fragments.get(node).expect("fragment").physical_rect()
    };
    let flex = rect("flex");
    let horizontal_start = rect("horizontal-start");
    let vertical_start = rect("vertical-start");
    let horizontal_end = rect("horizontal-end");
    let vertical_end = rect("vertical-end");

    // The parent cross start is right. The horizontal subject's inline start
    // is left, while the vertical-rl subject's block start is right.
    assert_eq!(horizontal_start.x - flex.x, 1.0);
    assert_eq!(vertical_start.x - flex.x, 41.0);
    assert_eq!(horizontal_end.x - flex.x, 41.0);
    assert_eq!(vertical_end.x - flex.x, 1.0);
}

#[test]
fn anonymous_flex_item_does_not_reuse_its_owners_self_alignment() {
    let document = StaticDocument::parse(
        r#"<html><body><div id="outer"><div id="flex">word<div id="block"></div></div></div></body></html>"#,
    );
    let styles = resolve_styles(
        &document,
        &StyleSet::cambium(&[r#"
          html, body { margin: 0; }
          #outer { display: flex; }
          #flex {
            display: flex;
            writing-mode: vertical-rl;
            direction: rtl;
            flex-flow: column nowrap;
            align-items: start;
            align-self: end;
            width: 80px;
            height: 60px;
            border: 1px solid black;
          }
          #block { flex: 0 0 10px; width: 10px; height: 10px; }
        "#]),
        &Device::screen(320.0, 240.0),
        &InteractionStates::default(),
    );
    let fragments = layout(&document, &styles, 320.0, 240.0).expect("layout");
    let flex_node = find(&document, document.document(), "flex").expect("flex");
    let text_node = document
        .dom_children(flex_node)
        .find(|node| {
            document.kind(*node) == NodeKind::Text
                && document
                    .text(*node)
                    .is_some_and(|text| text.contains("word"))
        })
        .expect("raw text child");
    let flex = fragments
        .get(flex_node)
        .expect("flex fragment")
        .physical_rect();
    let text = fragments
        .get(text_node)
        .expect("text fragment")
        .physical_rect();
    let block = fragments
        .get(find(&document, document.document(), "block").expect("block"))
        .expect("block fragment")
        .physical_rect();

    // The raw text becomes an anonymous flex item. Its own `align-self` is
    // auto, so it follows the container's bottom logical cross start just as
    // the ordinary block item does. Reusing `#flex`'s `align-self: end` for
    // that anonymous item incorrectly moves the text to the top edge.
    assert!(text.y > flex.y + flex.height / 2.0);
    assert!(block.y > flex.y + flex.height / 2.0);
}
