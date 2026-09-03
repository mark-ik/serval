// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

use genet_livery::{Device, InteractionStates, StyleSet, layout, resolve_styles};
use genet_static_dom::StaticDocument;
use layout_dom_api::{LayoutDom, LocalName, Namespace, NodeKind};
use taffy::{
    prelude::{Dimension, auto, length, percent, zero},
    style::FlexBasis,
};

#[test]
fn taffy_flex_basis_helpers_keep_auto_and_numeric_construction_distinct() {
    let auto_basis: FlexBasis = auto();
    let zero_basis: FlexBasis = zero();
    let length_basis: FlexBasis = length(12.0);
    let percent_basis: FlexBasis = percent(0.5);

    assert!(auto_basis.is_auto());
    assert_eq!(zero_basis, FlexBasis::from(Dimension::length(0.0)));
    assert_eq!(length_basis, FlexBasis::from(Dimension::length(12.0)));
    assert_eq!(percent_basis, FlexBasis::from(Dimension::percent(0.5)));
    assert!(FlexBasis::from(Dimension::auto()).is_auto());
}

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

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<(f32, f32)> {
    let document = StaticDocument::parse(html);
    let styles = resolve_styles(
        &document,
        &StyleSet::cambium(&[css]),
        &Device::screen(320.0, 240.0),
        &InteractionStates::default(),
    );
    let fragments = layout(&document, &styles, 320.0, 240.0).expect("layout");
    ids.iter()
        .map(|id| {
            let node = find(&document, document.document(), id).expect(id);
            let rect = fragments
                .get(node)
                .map(|fragment| fragment.physical_rect())
                .expect("fragment");
            (rect.width, rect.height)
        })
        .collect()
}

#[test]
fn flex_basis_content_bypasses_the_preferred_main_size_in_rows_and_columns() {
    let row = rects(
        r#"<html><body><div class="row">
          <div id="content"><div class="row-marker"></div></div>
          <div id="auto"><div class="row-marker"></div></div>
          <div id="content-auto"><div class="row-marker"></div></div>
          <div id="auto-auto"><div class="row-marker"></div></div>
        </div></body></html>"#,
        r#"
          html, body { margin: 0; }
          .row { display: flex; width: 200px; height: 40px; align-items: flex-start; }
          #content { flex: 0 0 content; width: 70px; }
          #auto { flex: 0 0 auto; width: 70px; }
          #content-auto { flex: 0 0 content; }
          #auto-auto { flex: 0 0 auto; }
          .row-marker { width: 30px; height: 10px; }
        "#,
        &["content", "auto", "content-auto", "auto-auto"],
    );
    assert_eq!(
        row,
        vec![(30.0, 10.0), (70.0, 10.0), (30.0, 10.0), (30.0, 10.0)]
    );

    let column = rects(
        r#"<html><body><div class="column">
          <div id="content"><div class="column-marker"></div></div>
          <div id="auto"><div class="column-marker"></div></div>
          <div id="content-auto"><div class="column-marker"></div></div>
          <div id="auto-auto"><div class="column-marker"></div></div>
        </div></body></html>"#,
        r#"
          html, body { margin: 0; }
          .column { display: flex; flex-direction: column; width: 40px; height: 200px; align-items: flex-start; }
          #content { flex-basis: content; flex-grow: 0; flex-shrink: 0; height: 70px; }
          #auto { flex-basis: auto; flex-grow: 0; flex-shrink: 0; height: 70px; }
          #content-auto { flex-basis: content; flex-grow: 0; flex-shrink: 0; }
          #auto-auto { flex-basis: auto; flex-grow: 0; flex-shrink: 0; }
          .column-marker { width: 10px; height: 30px; }
        "#,
        &["content", "auto", "content-auto", "auto-auto"],
    );
    assert_eq!(
        column,
        vec![(10.0, 30.0), (10.0, 70.0), (10.0, 30.0), (10.0, 30.0)]
    );
}
