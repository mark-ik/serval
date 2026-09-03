// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Ignored K6 continuation contracts for sequential-fill multicol.
//!
//! These fixtures pin the first structural and retained-session receipts for
//! block, inline, and table roots. They compile before the K6 continuation
//! kernel exists and are unignored gate by gate as that kernel and its
//! consumers land.

use genet_livery::{
    BoxId, Device, Fragment, FragmentationContextId, InteractionStates, InternalTableRole,
    LiveryDocument, LiveryLayout, StyleSet, layout, resolve_styles,
};
use genet_static_dom::StaticDocument;
use layout_dom_api::{LayoutDom, LocalName, Namespace, NodeKind};

type NodeId = <StaticDocument as LayoutDom>::NodeId;

const COLUMNS: &str =
    "width:220px; height:100px; column-count:2; column-gap:20px; column-fill:auto";

fn find(dom: &StaticDocument, node: NodeId, needle: &str) -> Option<NodeId> {
    if dom.kind(node) == NodeKind::Element
        && dom.attribute(node, &Namespace::from(""), &LocalName::from("id")) == Some(needle)
    {
        return Some(node);
    }
    dom.dom_children(node)
        .find_map(|child| find(dom, child, needle))
}

fn rect(fragment: &Fragment) -> (f32, f32, f32, f32) {
    let rect = fragment.physical_rect();
    (rect.x, rect.y, rect.width, rect.height)
}

struct Fixture {
    document: StaticDocument,
    layout: LiveryLayout<NodeId>,
}

impl Fixture {
    fn new(html: &str) -> Self {
        let document = StaticDocument::parse(html);
        let styles = resolve_styles(
            &document,
            &StyleSet::cambium(&[]),
            &Device::screen(800.0, 600.0),
            &InteractionStates::default(),
        );
        let layout = layout(&document, &styles, 800.0, 600.0).expect("layout");
        Self { document, layout }
    }

    fn node(&self, id: &str) -> NodeId {
        find(&self.document, self.document.document(), id).expect(id)
    }

    fn only_box(&self, id: &str) -> BoxId {
        let boxes = self.layout.boxes().boxes_for_node(self.node(id));
        assert_eq!(boxes.len(), 1, "{id} keeps one CSS box: {boxes:?}");
        boxes[0]
    }

    fn fragments_of(&self, box_id: BoxId) -> Vec<&Fragment> {
        self.layout.fragments().fragments_for_box(box_id).collect()
    }

    fn two_column_chain(&self, id: &str) -> Vec<&Fragment> {
        let box_id = self.only_box(id);
        let chain = self.fragments_of(box_id);
        assert_eq!(chain.len(), 2, "{id} emits one fragment per column");
        assert!(chain.iter().all(|fragment| fragment.box_id() == box_id));
        assert!(
            chain[0].continuation.is_some(),
            "the first fragment resumes"
        );
        assert!(
            chain[1].continuation.is_none(),
            "the last fragment completes"
        );
        let context = chain[0].fragmentation_context();
        assert_ne!(context, FragmentationContextId::INITIAL);
        assert_eq!(chain[1].fragmentation_context(), context);
        let first_column = chain[0]
            .containing_fragment()
            .expect("the first column contains the first fragment");
        let second_column = chain[1]
            .containing_fragment()
            .expect("the second column contains the resumed fragment");
        assert_ne!(first_column, second_column);
        chain
    }
}

fn session(html: &str) -> LiveryDocument<StaticDocument> {
    LiveryDocument::new(
        StaticDocument::parse(html),
        StyleSet::cambium(&[]),
        Device::screen(800.0, 600.0),
    )
}

fn session_node(session: &LiveryDocument<StaticDocument>, id: &str) -> NodeId {
    find(session.dom(), session.dom().document(), id).expect(id)
}

fn block_html() -> String {
    let band = |id: &str| format!("<div id='{id}' style='height:50px;background:#0f0'></div>");
    format!(
        "<html><body style='margin:0'>\
         <div id='columns' style='{COLUMNS}'>\
           <div id='continued'>{}{}{}{}</div>\
         </div><div id='after' style='height:10px'></div>\
         </body></html>",
        band("band1"),
        band("band2"),
        band("band3"),
        band("band4"),
    )
}

fn inline_html() -> String {
    format!(
        "<html><body style='margin:0'>\
         <div id='columns' style='{COLUMNS};line-height:25px;font-size:16px'>\
           <p id='lines' style='margin:0'>one<br>two<br>three<br>\
             <span id='spanning'>four<br>five</span><br>six<br>seven<br>eight</p>\
         </div><div id='after' style='height:10px'></div>\
         </body></html>"
    )
}

fn table_html() -> String {
    let row = |row: &str, cell: &str, height: u32| {
        format!("<tr id='{row}'><td id='{cell}' style='height:{height}px;padding:0'></td></tr>")
    };
    format!(
        "<html><body style='margin:0'>\
         <div id='columns' style='{COLUMNS}'>\
           <table id='table' style='border-collapse:collapse;border-spacing:0;width:100px'>\
             <thead id='head'>{}</thead><tbody id='body'>{}{}{}{}</tbody>\
             <tfoot id='foot'>{}</tfoot>\
           </table>\
         </div></body></html>",
        row("head-row", "head-cell", 20),
        row("row1", "cell1", 30),
        row("row2", "cell2", 30),
        row("row3", "cell3", 30),
        row("row4", "cell4", 30),
        row("foot-row", "foot-cell", 20),
    )
}

#[test]
#[ignore = "K6: block continuation kernel not implemented"]
fn continued_block_root_resumes_across_two_columns() {
    let fixture = Fixture::new(&block_html());
    let chain = fixture.two_column_chain("continued");
    assert_eq!(rect(chain[0]), (0.0, 0.0, 100.0, 100.0));
    assert_eq!(rect(chain[1]), (120.0, 0.0, 100.0, 100.0));
    for (band, column) in [("band1", 0), ("band2", 0), ("band3", 1), ("band4", 1)] {
        let fragments = fixture.fragments_of(fixture.only_box(band));
        assert_eq!(fragments.len(), 1, "{band} remains unbroken");
        assert_eq!(fragments[0].parent(), Some(chain[column].id()));
    }
}

#[test]
#[ignore = "K6: fragmented block session consumers not implemented"]
fn document_session_hosts_a_continued_block_root() {
    let mut session = session(&block_html());
    session.frame(800, 600).expect("frame");
    assert_eq!(session.content_height(600), 110);
    assert_eq!(
        session.hit_test(170.0, 25.0),
        Some(session_node(&session, "band3"))
    );
    let [_, after_y, _, _] = session
        .fragment_rect(session_node(&session, "after"))
        .expect("following sibling");
    assert_eq!(after_y, 100.0);
}

#[test]
#[ignore = "K6: inline continuation kernel not implemented"]
fn continued_inline_root_resumes_its_lines_in_the_next_column() {
    let fixture = Fixture::new(&inline_html());
    let chain = fixture.two_column_chain("lines");
    assert_eq!(rect(chain[0]), (0.0, 0.0, 100.0, 100.0));
    assert_eq!(rect(chain[1]), (120.0, 0.0, 100.0, 100.0));
    let spanning = fixture.fragments_of(fixture.only_box("spanning"));
    assert!(spanning.iter().any(|fragment| rect(fragment).0 < 100.0));
    assert!(spanning.iter().any(|fragment| rect(fragment).0 >= 120.0));
}

#[test]
#[ignore = "K6: fragmented inline session consumers not implemented"]
fn document_session_hosts_a_continued_inline_root() {
    let mut session = session(&inline_html());
    session.frame(800, 600).expect("frame");
    assert_eq!(session.content_height(600), 110);
    let ([four_x, four_y], _) = session.text_target("four").expect("four");
    let ([five_x, five_y], _) = session.text_target("five").expect("five");
    assert!(four_x < 100.0 && four_y < 100.0);
    assert!(five_x >= 120.0 && five_y < 100.0);
}

#[test]
#[ignore = "K6: table continuation kernel not implemented"]
fn continued_table_root_repeats_its_header_and_footer_in_each_column() {
    let fixture = Fixture::new(&table_html());
    let table = fixture.node("table");
    let boxes = fixture.layout.boxes().boxes_for_node(table);
    assert_eq!(boxes.len(), 2, "the table keeps one wrapper and one grid");
    assert_eq!(
        fixture.layout.boxes()[boxes[0]].display.internal_table,
        Some(InternalTableRole::Wrapper)
    );
    assert_eq!(
        fixture.layout.boxes()[boxes[1]].display.internal_table,
        Some(InternalTableRole::Grid)
    );
    for id in ["head", "body", "foot"] {
        assert_eq!(
            fixture.fragments_of(fixture.only_box(id)).len(),
            2,
            "{id} emits into both columns"
        );
    }
}

#[test]
#[ignore = "K6: fragmented table session consumers not implemented"]
fn document_session_hosts_a_continued_table_root() {
    let mut session = session(&table_html());
    session.frame(800, 600).expect("frame");
    assert_eq!(session.content_height(600), 100);
    for (x, y, id) in [
        (50.0, 10.0, "head-cell"),
        (170.0, 10.0, "head-cell"),
        (50.0, 35.0, "cell1"),
        (170.0, 35.0, "cell3"),
        (50.0, 90.0, "foot-cell"),
        (170.0, 90.0, "foot-cell"),
    ] {
        assert_eq!(session.hit_test(x, y), Some(session_node(&session, id)));
    }
}
