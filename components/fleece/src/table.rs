/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use layout_dom_api::LayoutDom;

use crate::{Inline, inline_runs, local_name};

/// One table cell. `header` records whether the source used `<th>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableCell {
    pub header: bool,
    pub runs: Vec<Inline>,
}

/// One table row. A row is a header row when every non-empty cell is a header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRow {
    pub header: bool,
    pub cells: Vec<TableCell>,
}

pub(crate) fn table_rows<D: LayoutDom>(dom: &D, table: D::NodeId) -> Vec<TableRow> {
    fn walk<D: LayoutDom>(dom: &D, id: D::NodeId, rows: &mut Vec<TableRow>) {
        if local_name(dom, id) == Some("tr") {
            let cells = dom
                .dom_children(id)
                .filter(|cell| matches!(local_name(dom, *cell), Some("th" | "td")))
                .map(|cell| TableCell {
                    header: local_name(dom, cell) == Some("th"),
                    runs: inline_runs(dom, cell),
                })
                .collect::<Vec<_>>();
            if !cells.is_empty() {
                let header = cells.iter().all(|cell| cell.header);
                rows.push(TableRow { header, cells });
            }
            return;
        }
        for child in dom.dom_children(id) {
            walk(dom, child, rows);
        }
    }
    let mut rows = Vec::new();
    walk(dom, table, &mut rows);
    rows
}
