// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

use fleece::{Block, ExtractionOptions, Inline, extract_document_with_options};
use genet_static_dom::StaticDocument;

fn table(html: &str) -> fleece::Table {
    let document = StaticDocument::parse(html);
    let extracted =
        extract_document_with_options(&document, ExtractionOptions { quote_context: 8 });
    extracted
        .article
        .expect("fixture should select its main content")
        .blocks
        .into_iter()
        .find_map(|block| match block.block {
            Block::Table { table } => Some(table),
            _ => None,
        })
        .expect("fixture should produce one table block")
}

#[test]
fn reader_projection_keeps_spans_coordinates_and_excludes_nested_table_cells() {
    let table = table(
        r#"<html><body><main><h1>Regional totals</h1>
        <p>This introduction gives the reader selector enough ordinary prose to retain the semantic table below.</p>
        <table><thead><tr><th id=region scope=col>Region</th><th id=total scope=col colspan=2>Total</th></tr></thead>
        <tbody><tr><th scope=row>West</th><td headers='total total' rowspan=2>20</td><td>units <table><tr><td>nested</td></tr></table></td></tr>
        <tr><th scope=row>East</th><td>10</td></tr></tbody></table>
        <p>This closing paragraph also supplies enough article-grade prose for stable reader selection.</p>
        </main></body></html>"#,
    );
    let rows = &table.rows;
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].cells[1].colspan, 2);
    assert_eq!(rows[0].cells[1].width, 2);
    assert_eq!(rows[1].cells[1].rowspan, 2);
    assert_eq!(rows[1].cells[1].height, 2);
    assert_eq!(rows[1].cells[2].x, 2);
    assert_eq!(
        rows[1].cells.len(),
        3,
        "nested table cells stay out of the outer row"
    );
    assert_eq!(rows[1].cells[1].headers, ["total", "total"]);
    assert_eq!(
        rows[1].cells[1].associated_headers[0].id.as_deref(),
        Some("total")
    );
    assert_eq!(rows[1].cells[2].runs, [Inline::Text("units".to_string())]);
}
