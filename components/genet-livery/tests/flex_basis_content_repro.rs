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

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<(String, (f32, f32, f32, f32))> {
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
            ((*id).to_string(), (rect.x, rect.y, rect.width, rect.height))
        })
        .collect()
}

fn sizes(html: &str, css: &str, ids: &[&str]) -> Vec<(String, (f32, f32))> {
    rects(html, css, ids)
        .into_iter()
        .map(|(id, (_, _, width, height))| (id, (width, height)))
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

#[test]
fn content_basis_uses_the_cross_sized_canvas_ratio_before_space_between() {
    let geometry = rects(
        r#"<html><body>
          <div id="content" class="container content">
            <div id="content-small" class="small">a b</div>
            <div id="content-big" class="big">c</div>
            <div id="content-spacer" class="spacer"></div>
            <div id="content-padding" class="padding"></div>
            <canvas id="content-canvas" width="20"></canvas>
          </div>
          <div id="auto" class="container">
            <div id="auto-small" class="small">a b</div>
            <div id="auto-big" class="big">c</div>
            <div id="auto-spacer" class="spacer"></div>
            <div id="auto-padding" class="padding"></div>
            <canvas id="auto-canvas" width="20"></canvas>
          </div>
        </body></html>"#,
        r#"
          .container {
            display: flex; justify-content: space-between;
            border: 2px solid; padding: 2px; width: 200px; height: 50px;
          }
          .container > * { flex-shrink: 0; min-width: 0; border: 2px solid; }
          .content > * { flex-basis: content; }
          .small { font: 10px/10px sans-serif; height: 0; }
          .big { font: 20px/20px sans-serif; height: 40px; }
          .spacer { height: 20px; }
          .padding { height: 10px; padding: 5px; }
          canvas { height: 8px; }
        "#,
        &[
            "content",
            "content-small",
            "content-big",
            "content-spacer",
            "content-padding",
            "content-canvas",
            "auto",
            "auto-small",
            "auto-big",
            "auto-spacer",
            "auto-padding",
            "auto-canvas",
        ],
    );
    let normalize = |container: usize, children: std::ops::Range<usize>| {
        let origin = geometry[container].1.0;
        children
            .map(|index| {
                let (_, (_, _, width, height)) = &geometry[index];
                (geometry[index].1.0 - origin, *width, *height)
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(normalize(0, 1..6), normalize(6, 7..12));
}

#[test]
fn content_basis_uses_an_inline_subtree_max_content_width() {
    let widths = sizes(
        r#"<html><body><div id="row">
          <div id="content" class="item"><span></span><span></span><span></span></div>
          <div id="auto" class="item"><span></span><span></span><span></span></div>
        </div></body></html>"#,
        r#"
          html, body { margin: 0; }
          #row { display: flex; width: 1px; align-items: flex-start; }
          .item { flex: 0 0 content; min-width: 0; border: 2px solid; }
          #auto { flex-basis: auto; }
          span { display: inline-block; width: 15px; height: 10px; border: 1px solid; }
        "#,
        &["content", "auto"],
    );
    assert_eq!(
        widths,
        vec![
            ("content".to_owned(), (55.0, 16.0)),
            ("auto".to_owned(), (55.0, 16.0))
        ]
    );
}

#[test]
fn column_content_basis_uses_fit_content_width_and_formats_float_line() {
    let geometry = rects(
        r#"<html><body>
          <div id="column">
            <div id="content" class="item"><float id="content-1"></float><float id="content-2"></float><float id="content-3"></float></div>
            <div id="auto" class="item"><float id="auto-1"></float><float id="auto-2"></float><float id="auto-3"></float></div>
          </div>
        </body></html>"#,
        r#"
          html, body { margin: 0; }
          #column { display: flex; flex-direction: column; align-items: flex-start; width: 800px; height: 1px; }
          .item { flex: 0 0 content; min-width: 0; border: 2px solid teal; }
          #auto { flex-basis: auto; }
          float { float: left; background: fuchsia; border: 1px solid gray; width: 15px; height: 10px; }
        "#,
        &[
            "content",
            "content-1",
            "content-2",
            "content-3",
            "auto",
            "auto-1",
            "auto-2",
            "auto-3",
        ],
    );
    assert_eq!(
        geometry,
        vec![
            ("content".to_owned(), (0.0, 0.0, 55.0, 16.0)),
            ("content-1".to_owned(), (2.0, 2.0, 17.0, 12.0)),
            ("content-2".to_owned(), (19.0, 2.0, 17.0, 12.0)),
            ("content-3".to_owned(), (36.0, 2.0, 17.0, 12.0)),
            ("auto".to_owned(), (0.0, 16.0, 55.0, 16.0)),
            ("auto-1".to_owned(), (2.0, 18.0, 17.0, 12.0)),
            ("auto-2".to_owned(), (19.0, 18.0, 17.0, 12.0)),
            ("auto-3".to_owned(), (36.0, 18.0, 17.0, 12.0)),
        ]
    );
}

#[test]
fn column_auto_basis_with_a_definite_cross_size_keeps_its_content_height() {
    let geometry = rects(
        r#"<html><body><div id="column">
          <div id="auto" class="item"><float></float><float></float><float></float></div>
        </div></body></html>"#,
        r#"
          html, body { margin: 0; }
          #column { display: flex; flex-direction: column; align-items: flex-start; width: 800px; height: 1px; }
          .item { flex: 0 0 auto; width: 100px; min-height: 0; border: 2px solid teal; }
          float { float: left; background: fuchsia; border: 1px solid gray; width: 15px; height: 10px; }
        "#,
        &["auto"],
    );
    assert_eq!(geometry, vec![("auto".to_owned(), (0.0, 0.0, 104.0, 16.0))]);
}

#[test]
fn column_content_basis_sizes_direct_canvas_and_nested_flex() {
    let geometry = sizes(
        r#"<html><body>
          <div class="column">
            <canvas id="canvas-content" class="item" width="25" height="10"></canvas>
            <canvas id="canvas-auto" class="item auto" width="25" height="10"></canvas>
          </div>
          <div class="column">
            <div id="flex-content" class="item inner-flex"><inner-item></inner-item><inner-item></inner-item><inner-item></inner-item></div>
            <div id="flex-auto" class="item auto inner-flex"><inner-item></inner-item><inner-item></inner-item><inner-item></inner-item></div>
          </div>
        </body></html>"#,
        r#"
          html, body { margin: 0; }
          .column { display: flex; flex-direction: column; align-items: flex-start; width: 800px; height: 1px; }
          .item { flex: 0 0 content; min-height: 0; border: 2px solid teal; }
          .auto { flex-basis: auto; }
          canvas { background: brown; border: 1px solid gray; }
          .inner-flex { display: flex; flex-direction: column; }
          inner-item { background: salmon; border: 1px solid gray; height: 10px; width: 15px; flex: none; }
        "#,
        &["canvas-content", "canvas-auto", "flex-content", "flex-auto"],
    );
    assert_eq!(
        geometry,
        vec![
            ("canvas-content".to_owned(), (29.0, 14.0)),
            ("canvas-auto".to_owned(), (29.0, 14.0)),
            ("flex-content".to_owned(), (21.0, 40.0)),
            ("flex-auto".to_owned(), (21.0, 40.0)),
        ]
    );
}
