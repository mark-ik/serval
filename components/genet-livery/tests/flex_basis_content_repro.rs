//! Small native Livery geometry probes for `flex-basis: content`.
//!
//! These deliberately keep the flex container definite and compare a content
//! basis against an `auto` basis with the same preferred main size.  A content
//! basis must measure the child, while auto is allowed to use that preferred
//! size first.

use std::collections::HashMap;

use genet_livery::{
    Device, InteractionStates, StyleSet, TextSystem, ViewportSizes, layout_with_text_system,
    resolve_styles,
};
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

fn sizes(html: &str, css: &str, ids: &[&str]) -> Vec<(String, (f32, f32))> {
    let document = StaticDocument::parse(html);
    let styles = resolve_styles(
        &document,
        &StyleSet::cambium(&[css]),
        &Device::screen(640.0, 480.0),
        &InteractionStates::default(),
    );
    let mut text = TextSystem::new();
    let (_, fragments) = layout_with_text_system(
        &document,
        &styles,
        640.0,
        480.0,
        ViewportSizes::uniform(640.0, 480.0),
        &mut text,
        &HashMap::new(),
    )
    .expect("layout");
    ids.iter()
        .map(|id| {
            let node = find(&document, document.document(), id).expect(id);
            let rect = fragments
                .get(node)
                .map(|fragment| fragment.physical_rect())
                .expect("fragment");
            ((*id).to_string(), (rect.width, rect.height))
        })
        .collect()
}

#[test]
fn flex_basis_content_geometry_covers_each_content_family() {
    let text = sizes(
        r#"<html><body><div id="row">
          <div id="text-content">intrinsic text</div>
          <div id="text-auto">intrinsic text</div>
        </div></body></html>"#,
        r#"
          html, body { margin: 0; }
          #row { display: flex; width: 320px; height: 80px; align-items: flex-start; }
          #text-content { flex: 0 0 content; width: 240px; font: 16px/20px sans-serif; }
          #text-auto { flex: 0 0 auto; width: 240px; font: 16px/20px sans-serif; }
        "#,
        &["text-content", "text-auto"],
    );
    assert_eq!(text[1], ("text-auto".to_owned(), (240.0, 20.0)));
    assert_eq!(text[0].0, "text-content");
    assert_eq!(text[0].1.1, 20.0);
    assert!(text[0].1.0 < text[1].1.0);

    let canvas = sizes(
        r#"<html><body><div id="row">
          <canvas id="canvas-content" width="72" height="24"></canvas>
          <canvas id="canvas-auto" width="72" height="24"></canvas>
        </div></body></html>"#,
        r#"
          html, body { margin: 0; }
          #row { display: flex; width: 320px; height: 80px; align-items: flex-start; }
          #canvas-content { flex: 0 0 content; width: 240px; }
          #canvas-auto { flex: 0 0 auto; width: 240px; }
        "#,
        &["canvas-content", "canvas-auto"],
    );
    assert_eq!(
        canvas,
        vec![
            ("canvas-content".to_owned(), (72.0, 80.0)),
            ("canvas-auto".to_owned(), (240.0, 80.0)),
        ]
    );

    let canvas_natural = sizes(
        r#"<html><body><div id="row">
          <canvas id="canvas-natural-content" width="72" height="24"></canvas>
          <canvas id="canvas-natural-auto" width="72" height="24"></canvas>
        </div></body></html>"#,
        r#"
          html, body { margin: 0; }
          #row { display: flex; width: 320px; height: 80px; align-items: flex-start; }
          #canvas-natural-content { flex: 0 0 content; }
          #canvas-natural-auto { flex: 0 0 auto; }
        "#,
        &["canvas-natural-content", "canvas-natural-auto"],
    );
    assert_eq!(
        canvas_natural,
        vec![
            ("canvas-natural-content".to_owned(), (72.0, 24.0)),
            ("canvas-natural-auto".to_owned(), (72.0, 24.0)),
        ]
    );

    let canvas_cross = sizes(
        r#"<html><body><div id="row">
          <canvas id="canvas-cross-content" width="72" height="24"></canvas>
          <canvas id="canvas-cross-auto" width="72" height="24"></canvas>
        </div></body></html>"#,
        r#"
          html, body { margin: 0; }
          #row { display: flex; width: 320px; height: 80px; align-items: flex-start; }
          #canvas-cross-content { flex: 0 0 content; width: 240px; height: 40px; }
          #canvas-cross-auto { flex: 0 0 auto; width: 240px; height: 40px; }
        "#,
        &["canvas-cross-content", "canvas-cross-auto"],
    );
    assert_eq!(
        canvas_cross,
        vec![
            ("canvas-cross-content".to_owned(), (120.0, 40.0)),
            ("canvas-cross-auto".to_owned(), (240.0, 40.0)),
        ]
    );

    let canvas_column = sizes(
        r#"<html><body><div id="column">
          <canvas id="canvas-column-content" width="72" height="24"></canvas>
          <canvas id="canvas-column-auto" width="72" height="24"></canvas>
        </div></body></html>"#,
        r#"
          html, body { margin: 0; }
          #column { display: flex; flex-direction: column; width: 80px; height: 320px; align-items: flex-start; }
          #canvas-column-content { flex: 0 0 content; height: 240px; }
          #canvas-column-auto { flex: 0 0 auto; height: 240px; }
        "#,
        &["canvas-column-content", "canvas-column-auto"],
    );
    assert_eq!(
        canvas_column,
        vec![
            ("canvas-column-content".to_owned(), (720.0, 24.0)),
            ("canvas-column-auto".to_owned(), (720.0, 240.0)),
        ]
    );

    let nested = sizes(
        r#"<html><body><div id="row">
          <div id="nested-content"><div class="marker"></div></div>
          <div id="nested-auto"><div class="marker"></div></div>
        </div></body></html>"#,
        r#"
          html, body { margin: 0; }
          #row { display: flex; width: 320px; height: 80px; align-items: flex-start; }
          #nested-content { flex: 0 0 content; width: 240px; }
          #nested-auto { flex: 0 0 auto; width: 240px; }
          .marker { width: 72px; height: 24px; }
        "#,
        &["nested-content", "nested-auto"],
    );
    assert_eq!(
        nested,
        vec![
            ("nested-content".to_owned(), (72.0, 24.0)),
            ("nested-auto".to_owned(), (240.0, 24.0)),
        ]
    );

    let nested_flex = sizes(
        r#"<html><body><div id="row">
          <div id="flex-content"><div class="inner"><div class="marker"></div></div></div>
          <div id="flex-auto"><div class="inner"><div class="marker"></div></div></div>
        </div></body></html>"#,
        r#"
          html, body { margin: 0; }
          #row { display: flex; width: 320px; height: 80px; align-items: flex-start; }
          #flex-content { display: flex; flex: 0 0 content; width: 240px; }
          #flex-auto { display: flex; flex: 0 0 auto; width: 240px; }
          .inner { display: flex; }
          .marker { width: 72px; height: 24px; }
        "#,
        &["flex-content", "flex-auto"],
    );
    assert_eq!(
        nested_flex,
        vec![
            ("flex-content".to_owned(), (72.0, 24.0)),
            ("flex-auto".to_owned(), (240.0, 24.0)),
        ]
    );

    let column = sizes(
        r#"<html><body><div id="column">
          <div id="column-content"><div class="marker"></div></div>
          <div id="column-auto"><div class="marker"></div></div>
        </div></body></html>"#,
        r#"
          html, body { margin: 0; }
          #column { display: flex; flex-direction: column; width: 80px; height: 320px; align-items: flex-start; }
          #column-content { flex: 0 0 content; height: 240px; }
          #column-auto { flex: 0 0 auto; height: 240px; }
          .marker { width: 24px; height: 72px; }
        "#,
        &["column-content", "column-auto"],
    );
    assert_eq!(
        column,
        vec![
            ("column-content".to_owned(), (24.0, 72.0)),
            ("column-auto".to_owned(), (24.0, 240.0)),
        ]
    );
}
