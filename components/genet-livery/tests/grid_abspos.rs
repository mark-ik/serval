use genet_livery::{Device, InteractionStates, StyleSet, layout, resolve_styles};
use genet_static_dom::StaticDocument;
use layout_dom_api::{LayoutDom, LocalName, Namespace, NodeKind};

#[test]
fn probe_positioned_grid_items() {
    // css/css-grid/abspos/positioned-grid-items-001.html, reduced.
    let html = r#"<html><body>
      <div id="grid">
        <div id="first">First item</div>
        <div id="second">Second item</div>
        <div id="third">Third item</div>
        <div id="fourth">Fourth item</div>
      </div>
    </body></html>"#;
    let css = r#"
      #grid {
        display: grid;
        grid-template-rows: 150px 100px;
        grid-template-columns: 200px 300px;
        margin: 1px 2px 3px 4px;
        padding: 20px 15px 10px 5px;
        border-width: 9px 3px 12px 6px;
        border-style: solid;
        width: 550px;
        height: 400px;
        position: relative;
      }
      #grid > div { position: absolute; }
      #first  { grid-column: 1 / 2; grid-row: 1 / 2; }
      #second { grid-column: 2 / 3; grid-row: 1 / 2; }
      #third  { grid-column: 1 / 2; grid-row: 2 / 3; }
      #fourth { grid-column: 2 / 3; grid-row: 2 / 3; }
    "#;
    let document = StaticDocument::parse(html);
    let styles = resolve_styles(
        &document,
        &StyleSet::cambium(&[css]),
        &Device::screen(800.0, 600.0),
        &InteractionStates::default(),
    );
    let fragments = layout(&document, &styles, 800.0, 600.0).unwrap();

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
    let by_id = |needle: &str| find(&document, document.document(), needle).expect(needle);
    // css-grid section 9: a grid-placed absolutely positioned item's
    // containing block is its grid area, and with auto insets its static
    // position is the grid-area origin. The grid's content origin is its
    // border (6,9) plus padding (5,20) inside a border box at (12,8); the
    // column tracks are 200/300 and the row tracks 150/100.
    for (name, x, y) in [
        ("first", 23.0, 37.0),
        ("second", 223.0, 37.0),
        ("third", 23.0, 187.0),
        ("fourth", 223.0, 187.0),
    ] {
        let fragment = fragments
            .get(by_id(name))
            .map(|fragment| fragment.physical_rect())
            .unwrap_or_default();
        assert_eq!(
            (fragment.x, fragment.y),
            (x, y),
            "{name}: abspos grid item must sit at its grid-area origin",
        );
    }
}

#[test]
fn static_grid_parent_uses_content_static_rectangle_unless_it_is_the_containing_block() {
    let html = r#"<html><body>
      <div id="outer">
        <div id="static-grid"><div id="from-static"></div></div>
        <div id="relative-grid"><div id="from-relative"></div></div>
      </div>
    </body></html>"#;
    let css = r#"
      html, body, div { margin: 0; }
      #outer { position: relative; width: 1000px; height: 1500px; }
      #static-grid, #relative-grid {
        display: grid;
        width: 100px;
        height: 500px;
        padding: 13px;
        padding-top: 74px;
        padding-bottom: 42px;
        border: 23px solid black;
        border-bottom-width: 45px;
        align-items: center;
      }
      #relative-grid { position: relative; }
      #static-grid > div, #relative-grid > div {
        position: absolute;
        width: 50px;
        height: 100px;
      }
    "#;
    let document = StaticDocument::parse(html);
    let styles = resolve_styles(
        &document,
        &StyleSet::cambium(&[css]),
        &Device::screen(1200.0, 1800.0),
        &InteractionStates::default(),
    );
    let fragments = layout(&document, &styles, 1200.0, 1800.0).expect("layout");

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
    let rect = |id| {
        fragments
            .get(find(&document, document.document(), id).expect(id))
            .map(|fragment| fragment.physical_rect())
            .expect("fragment")
    };

    let static_grid = rect("static-grid");
    let from_static = rect("from-static");
    let relative_grid = rect("relative-grid");
    let from_relative = rect("from-relative");

    assert_eq!(
        from_static.y - static_grid.y,
        297.0,
        "a grid that is only the static-position parent aligns in its content box"
    );
    assert_eq!(
        from_relative.y - relative_grid.y,
        281.0,
        "a grid selected by K5a keeps its padding-edge grid area"
    );
}
