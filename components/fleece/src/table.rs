/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::collections::{HashMap, HashSet};

use layout_dom_api::{LayoutDom, NodeKind};

use crate::{Inline, attr, inline_runs};

/// An extracted HTML table, independent of CSS layout or presentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    /// The source table's raw `id` attribute, if present.
    pub id: Option<String>,
    /// Inline content from the first direct HTML `<caption>`, if present.
    pub caption: Option<Vec<Inline>>,
    /// Row groups in table-model order. Pending `tfoot` groups appear last.
    pub row_groups: Vec<TableRowGroup>,
    /// Every row in computed grid order, including source-empty rows.
    pub rows: Vec<TableRow>,
    /// Computed number of grid columns.
    pub width: u32,
    /// Computed number of grid rows.
    pub height: u32,
    /// Deterministic table-model errors found while assigning cell slots.
    pub errors: Vec<TableModelError>,
}

/// A recoverable error in HTML table-model formation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableModelError {
    /// A later cell covers a slot already covered by an earlier cell.
    OverlappingCells {
        /// The doubly-covered grid slot.
        x: u32,
        y: u32,
        /// Grid anchor of the cell that retains ownership of the slot.
        existing_x: u32,
        existing_y: u32,
        /// Grid anchor of the later overlapping cell.
        incoming_x: u32,
        incoming_y: u32,
    },
}

/// One table-model row group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRowGroup {
    pub kind: TableRowGroupKind,
    /// Grid row at which this group begins.
    pub start: u32,
    /// Indices into [`Table::rows`], in grid order.
    pub rows: Vec<u32>,
}

/// The HTML construct that supplied a row group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableRowGroupKind {
    Head,
    Body,
    Foot,
    /// Consecutive direct `<tr>` children of a table.
    Implicit,
}

/// The supported HTML `scope` states for header cells.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TableScope {
    #[default]
    Auto,
    Row,
    Column,
    RowGroup,
    ColumnGroup,
}

/// A header cell associated with a cell by HTML's header algorithm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableHeader {
    pub id: Option<String>,
    pub x: u32,
    pub y: u32,
    pub scope: TableScope,
}

/// One cell in the HTML table model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableCell {
    /// Whether the source used `<th>` rather than `<td>`.
    pub header: bool,
    pub runs: Vec<Inline>,
    /// Raw source `id`, if present.
    pub id: Option<String>,
    /// Parsed `scope` state. Non-header cells remain `Auto`.
    pub scope: TableScope,
    /// Raw `headers` tokens, split on HTML ASCII whitespace in declared order.
    ///
    /// Repeated tokens are preserved here. Resolution deduplicates only the
    /// resulting associated header cells.
    pub headers: Vec<String>,
    /// Parsed `colspan`, normalized to the HTML range (minimum one).
    pub colspan: u32,
    /// Parsed `rowspan`; zero is retained for "rest of row group".
    pub rowspan: u32,
    /// Grid anchor coordinates.
    pub x: u32,
    pub y: u32,
    /// Effective dimensions in the computed grid.
    pub width: u32,
    pub height: u32,
    /// Header cells associated by explicit `headers` or the automatic algorithm.
    pub associated_headers: Vec<TableHeader>,
}

/// One table row. A row is a header row when every non-empty source cell is a header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRow {
    pub header: bool,
    pub cells: Vec<TableCell>,
    /// Grid row coordinate.
    pub y: u32,
    /// Index into [`Table::row_groups`].
    pub row_group: Option<usize>,
}

/// Form the table model from one HTML `<table>` element.
pub fn extract_table<D: LayoutDom>(dom: &D, table: D::NodeId) -> Table {
    let caption = html_element_children(dom, table)
        .into_iter()
        .find(|child| html_name(dom, *child) == Some("caption"))
        .map(|caption| inline_runs(dom, caption));
    let groups = source_row_groups(dom, table);
    let columns = column_groups(dom, table);
    let document_ids = first_document_ids(dom);

    let mut cells = Vec::new();
    let mut row_cells = Vec::new();
    let mut row_group_indices = Vec::new();
    let mut group_starts = Vec::new();
    let mut group_rows = Vec::new();
    let mut grid = Vec::new();
    let mut errors = Vec::new();

    for (group_index, group) in groups.iter().enumerate() {
        let group_start = grid.len();
        group_starts.push(group_start);
        let row_group = (group.kind != TableRowGroupKind::Implicit).then_some(group_index);
        let mut grow_down = Vec::new();
        for (source_y, row) in group.rows.iter().enumerate() {
            let y = group_start + source_y;
            ensure_rows(
                &mut grid,
                &mut row_cells,
                &mut row_group_indices,
                y + 1,
                row_group,
            );
            let mut row_cells_for_source = Vec::new();
            let mut x = 0usize;
            for cell in html_element_children(dom, *row)
                .into_iter()
                .filter(|cell| matches!(html_name(dom, *cell), Some("th" | "td")))
            {
                while grid[y].get(x).is_some_and(Option::is_some) {
                    x += 1;
                }
                let colspan = cell_colspan(dom, cell) as usize;
                let rowspan = cell_rowspan(dom, cell);
                let height = if rowspan == 0 { 1 } else { rowspan as usize };
                ensure_rows(
                    &mut grid,
                    &mut row_cells,
                    &mut row_group_indices,
                    y + height,
                    row_group,
                );
                let index = cells.len();
                cells.push(CellWork {
                    node: cell,
                    header: html_name(dom, cell) == Some("th"),
                    id: attr(dom, cell, "id"),
                    scope: cell_scope(dom, cell),
                    headers: header_tokens(dom, cell),
                    headers_specified: attr(dom, cell, "headers").is_some(),
                    colspan: colspan as u32,
                    rowspan,
                    x,
                    y,
                    width: colspan,
                    height,
                    row_group,
                    runs: cell_inline_runs(dom, cell),
                    empty: cell_is_empty(dom, cell),
                });
                cover_cell(&mut grid, &cells, &mut errors, index);
                if rowspan == 0 {
                    grow_down.push(index);
                }
                row_cells_for_source.push(index);
                x += colspan;
            }
            row_cells[y] = row_cells_for_source;
        }

        let group_end = grid.len();
        for index in grow_down {
            let height = group_end.saturating_sub(cells[index].y).max(1);
            cells[index].height = height;
            cover_cell(&mut grid, &cells, &mut errors, index);
        }
        group_rows.push(group_end.saturating_sub(group_start));
    }

    let width = grid.iter().map(Vec::len).max().unwrap_or(0).max(
        columns
            .iter()
            .map(|group| group.start.saturating_add(group.width))
            .max()
            .unwrap_or(0),
    );
    for row in &mut grid {
        row.resize(width, None);
    }
    let header_by_node = cells
        .iter()
        .enumerate()
        .filter(|(_, cell)| cell.header)
        .map(|(index, cell)| (cell.node, index))
        .collect::<HashMap<_, _>>();
    let associations = associated_headers(&cells, &grid, &columns, &document_ids, &header_by_node);

    let rows = row_cells
        .into_iter()
        .enumerate()
        .map(|(y, source_cells)| TableRow {
            header: !source_cells.is_empty()
                && source_cells.iter().all(|index| cells[*index].header),
            cells: source_cells
                .into_iter()
                .map(|index| public_cell(&cells[index], &cells, &associations[index]))
                .collect(),
            y: y as u32,
            row_group: row_group_indices.get(y).copied().flatten(),
        })
        .collect::<Vec<_>>();
    let row_groups = groups
        .into_iter()
        .enumerate()
        .map(|(index, group)| TableRowGroup {
            kind: group.kind,
            start: group_starts[index] as u32,
            rows: (group_starts[index]..group_starts[index] + group_rows[index])
                .map(|row| row as u32)
                .collect(),
        })
        .collect();

    Table {
        id: attr(dom, table, "id"),
        caption,
        row_groups,
        rows,
        width: width as u32,
        height: grid.len() as u32,
        errors,
    }
}

#[derive(Debug, Clone)]
struct SourceGroup<N> {
    kind: TableRowGroupKind,
    rows: Vec<N>,
}

#[derive(Debug, Clone, Copy)]
struct ColumnGroup {
    start: usize,
    width: usize,
}

#[derive(Debug, Clone)]
struct CellWork<N> {
    node: N,
    header: bool,
    id: Option<String>,
    scope: TableScope,
    headers: Vec<String>,
    headers_specified: bool,
    colspan: u32,
    rowspan: u32,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    /// A real row-group element. Direct table rows have no group element.
    row_group: Option<usize>,
    runs: Vec<Inline>,
    empty: bool,
}

fn source_row_groups<D: LayoutDom>(dom: &D, table: D::NodeId) -> Vec<SourceGroup<D::NodeId>> {
    let mut groups = Vec::new();
    let mut implicit = Vec::new();
    let mut pending_foot = Vec::new();
    let flush_implicit = |groups: &mut Vec<SourceGroup<D::NodeId>>,
                          implicit: &mut Vec<D::NodeId>| {
        if !implicit.is_empty() {
            groups.push(SourceGroup {
                kind: TableRowGroupKind::Implicit,
                rows: std::mem::take(implicit),
            });
        }
    };

    for child in html_element_children(dom, table) {
        match html_name(dom, child) {
            Some("tr") => implicit.push(child),
            Some("thead" | "tbody") => {
                flush_implicit(&mut groups, &mut implicit);
                let kind = if html_name(dom, child) == Some("thead") {
                    TableRowGroupKind::Head
                } else {
                    TableRowGroupKind::Body
                };
                groups.push(SourceGroup {
                    kind,
                    rows: html_element_children(dom, child)
                        .into_iter()
                        .filter(|row| html_name(dom, *row) == Some("tr"))
                        .collect(),
                });
            },
            Some("tfoot") => {
                flush_implicit(&mut groups, &mut implicit);
                pending_foot.push(child);
            },
            _ => {},
        }
    }
    flush_implicit(&mut groups, &mut implicit);
    groups.extend(pending_foot.into_iter().map(|foot| {
        SourceGroup {
            kind: TableRowGroupKind::Foot,
            rows: html_element_children(dom, foot)
                .into_iter()
                .filter(|row| html_name(dom, *row) == Some("tr"))
                .collect(),
        }
    }));
    groups
}

fn column_groups<D: LayoutDom>(dom: &D, table: D::NodeId) -> Vec<ColumnGroup> {
    let mut groups = Vec::new();
    let mut start = 0usize;
    for group in html_element_children(dom, table)
        .into_iter()
        .filter(|child| html_name(dom, *child) == Some("colgroup"))
    {
        let columns = html_element_children(dom, group)
            .into_iter()
            .filter(|column| html_name(dom, *column) == Some("col"))
            .collect::<Vec<_>>();
        let width = if columns.is_empty() {
            positive_span(attr(dom, group, "span"), 1000)
        } else {
            columns
                .into_iter()
                .map(|column| positive_span(attr(dom, column, "span"), 1000))
                .sum::<usize>()
        };
        groups.push(ColumnGroup { start, width });
        start = start.saturating_add(width);
    }
    groups
}

fn first_document_ids<D: LayoutDom>(dom: &D) -> HashMap<String, D::NodeId> {
    fn visit<D: LayoutDom>(dom: &D, id: D::NodeId, out: &mut HashMap<String, D::NodeId>) {
        if dom.kind(id) == NodeKind::Element {
            if let Some(value) = attr(dom, id, "id") {
                out.entry(value).or_insert(id);
            }
        }
        for child in dom.dom_children(id) {
            visit(dom, child, out);
        }
    }

    let mut ids = HashMap::new();
    visit(dom, dom.document(), &mut ids);
    ids
}

fn ensure_rows(
    grid: &mut Vec<Vec<Option<usize>>>,
    row_cells: &mut Vec<Vec<usize>>,
    row_group_indices: &mut Vec<Option<usize>>,
    len: usize,
    row_group: Option<usize>,
) {
    while grid.len() < len {
        grid.push(Vec::new());
        row_cells.push(Vec::new());
        row_group_indices.push(row_group);
    }
}

fn cover_cell<N>(
    grid: &mut [Vec<Option<usize>>],
    cells: &[CellWork<N>],
    errors: &mut Vec<TableModelError>,
    cell: usize,
) {
    let x = cells[cell].x;
    let y = cells[cell].y;
    let width = cells[cell].width;
    let height = cells[cell].height;
    for (row_index, row) in grid.iter_mut().enumerate().skip(y).take(height) {
        row.resize((x + width).max(row.len()), None);
        for (column, slot) in row.iter_mut().enumerate().skip(x).take(width) {
            match *slot {
                Some(existing) if existing != cell => {
                    errors.push(TableModelError::OverlappingCells {
                        x: column as u32,
                        y: row_index as u32,
                        existing_x: cells[existing].x as u32,
                        existing_y: cells[existing].y as u32,
                        incoming_x: cells[cell].x as u32,
                        incoming_y: cells[cell].y as u32,
                    })
                },
                None => *slot = Some(cell),
                Some(_) => {},
            }
        }
    }
}

fn associated_headers<N: Copy + Eq + std::hash::Hash>(
    cells: &[CellWork<N>],
    grid: &[Vec<Option<usize>>],
    columns: &[ColumnGroup],
    document_ids: &HashMap<String, N>,
    header_by_node: &HashMap<N, usize>,
) -> Vec<Vec<usize>> {
    cells
        .iter()
        .enumerate()
        .map(|(principal, cell)| {
            let mut headers = if cell.headers_specified {
                cell.headers
                    .iter()
                    .filter_map(|id| document_ids.get(id))
                    .filter_map(|node| header_by_node.get(node).copied())
                    .collect()
            } else {
                automatic_headers(principal, cells, grid, columns)
            };
            headers.retain(|header| {
                *header != principal && cells[*header].header && !cells[*header].empty
            });
            stable_dedup(&mut headers);
            headers
        })
        .collect()
}

fn automatic_headers<N>(
    principal: usize,
    cells: &[CellWork<N>],
    grid: &[Vec<Option<usize>>],
    columns: &[ColumnGroup],
) -> Vec<usize> {
    let cell = &cells[principal];
    let mut headers = Vec::new();
    for y in cell.y..cell.y + cell.height {
        scan_headers(principal, (cell.x, y), (-1, 0), cells, grid, &mut headers);
    }
    for x in cell.x..cell.x + cell.width {
        scan_headers(principal, (x, cell.y), (0, -1), cells, grid, &mut headers);
    }
    for (index, header) in cells.iter().enumerate() {
        if header.header
            && header.scope == TableScope::RowGroup
            && header.row_group.is_some()
            && header.row_group == cell.row_group
            && header.x < cell.x + cell.width
            && header.y < cell.y + cell.height
        {
            headers.push(index);
        }
        if header.header
            && header.scope == TableScope::ColumnGroup
            && shares_column_group(header, cell, columns)
            && header.x < cell.x + cell.width
            && header.y < cell.y + cell.height
        {
            headers.push(index);
        }
    }
    headers
}

fn scan_headers<N>(
    principal: usize,
    initial: (usize, usize),
    direction: (isize, isize),
    cells: &[CellWork<N>],
    grid: &[Vec<Option<usize>>],
    headers: &mut Vec<usize>,
) {
    let (dx, dy) = direction;
    let mut x = initial.0 as isize;
    let mut y = initial.1 as isize;
    let mut opaque = Vec::new();
    let mut in_header_block = cells[principal].header;
    let mut current_block = if in_header_block {
        vec![principal]
    } else {
        Vec::new()
    };
    loop {
        x += dx;
        y += dy;
        if x < 0 || y < 0 {
            return;
        }
        let Some(current) = grid
            .get(y as usize)
            .and_then(|row| row.get(x as usize))
            .and_then(|slot| *slot)
        else {
            continue;
        };
        if cells[current].header {
            in_header_block = true;
            current_block.push(current);
            let blocked = if dx == 0 {
                opaque.iter().any(|opaque: &usize| {
                    cells[*opaque].x == cells[current].x
                        && cells[*opaque].width == cells[current].width
                }) || !is_column_header(current, cells, grid)
            } else {
                opaque.iter().any(|opaque: &usize| {
                    cells[*opaque].y == cells[current].y
                        && cells[*opaque].height == cells[current].height
                }) || !is_row_header(current, cells, grid)
            };
            if !blocked {
                headers.push(current);
            }
        } else if in_header_block {
            in_header_block = false;
            opaque.append(&mut current_block);
        }
    }
}

fn is_column_header<N>(index: usize, cells: &[CellWork<N>], grid: &[Vec<Option<usize>>]) -> bool {
    let cell = &cells[index];
    cell.header
        && (cell.scope == TableScope::Column
            || (cell.scope == TableScope::Auto && !data_in_rows(cell, cells, grid)))
}

fn is_row_header<N>(index: usize, cells: &[CellWork<N>], grid: &[Vec<Option<usize>>]) -> bool {
    let cell = &cells[index];
    cell.header
        && (cell.scope == TableScope::Row
            || (cell.scope == TableScope::Auto
                && !is_column_header(index, cells, grid)
                && !data_in_columns(cell, cells, grid)))
}

fn data_in_rows<N>(cell: &CellWork<N>, cells: &[CellWork<N>], grid: &[Vec<Option<usize>>]) -> bool {
    grid.iter()
        .skip(cell.y)
        .take(cell.height)
        .flatten()
        .filter_map(|slot| *slot)
        .any(|index| !cells[index].header)
}

fn data_in_columns<N>(
    cell: &CellWork<N>,
    cells: &[CellWork<N>],
    grid: &[Vec<Option<usize>>],
) -> bool {
    grid.iter().any(|row| {
        row.iter()
            .skip(cell.x)
            .take(cell.width)
            .filter_map(|slot| *slot)
            .any(|index| !cells[index].header)
    })
}

fn shares_column_group<N>(left: &CellWork<N>, right: &CellWork<N>, groups: &[ColumnGroup]) -> bool {
    groups.iter().any(|group| {
        let contains = |x: usize| x >= group.start && x < group.start + group.width;
        contains(left.x) && contains(right.x)
    })
}

fn public_cell<N>(cell: &CellWork<N>, cells: &[CellWork<N>], headers: &[usize]) -> TableCell {
    TableCell {
        header: cell.header,
        runs: cell.runs.clone(),
        id: cell.id.clone(),
        scope: cell.scope,
        headers: cell.headers.clone(),
        colspan: cell.colspan,
        rowspan: cell.rowspan,
        x: cell.x as u32,
        y: cell.y as u32,
        width: cell.width as u32,
        height: cell.height as u32,
        associated_headers: headers
            .iter()
            .map(|index| TableHeader {
                id: cells[*index].id.clone(),
                x: cells[*index].x as u32,
                y: cells[*index].y as u32,
                scope: cells[*index].scope,
            })
            .collect(),
    }
}

fn cell_scope<D: LayoutDom>(dom: &D, cell: D::NodeId) -> TableScope {
    if html_name(dom, cell) != Some("th") {
        return TableScope::Auto;
    }
    match attr(dom, cell, "scope")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "row" => TableScope::Row,
        "col" => TableScope::Column,
        "rowgroup" => TableScope::RowGroup,
        "colgroup" => TableScope::ColumnGroup,
        _ => TableScope::Auto,
    }
}

fn header_tokens<D: LayoutDom>(dom: &D, cell: D::NodeId) -> Vec<String> {
    let Some(headers) = attr(dom, cell, "headers") else {
        return Vec::new();
    };
    headers
        .split(is_html_ascii_space)
        .filter(|header| !header.is_empty())
        .map(str::to_string)
        .collect()
}

fn cell_colspan<D: LayoutDom>(dom: &D, cell: D::NodeId) -> u32 {
    positive_span(attr(dom, cell, "colspan"), 1000) as u32
}

fn cell_rowspan<D: LayoutDom>(dom: &D, cell: D::NodeId) -> u32 {
    parsed_non_negative_integer(attr(dom, cell, "rowspan"))
        .map(|span| span.min(65534) as u32)
        .unwrap_or(1)
}

fn positive_span(value: Option<String>, maximum: usize) -> usize {
    parsed_non_negative_integer(value)
        .map(|span| span.clamp(1, maximum))
        .unwrap_or(1)
}

/// HTML's parser for a non-negative integer: only leading HTML ASCII space is
/// skipped and the first non-digit ends the integer.
fn parsed_non_negative_integer(value: Option<String>) -> Option<usize> {
    let value = value?;
    let bytes = value.as_bytes();
    let mut index = 0;
    while bytes
        .get(index)
        .is_some_and(|byte| is_html_ascii_space(*byte as char))
    {
        index += 1;
    }
    if !bytes.get(index).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let mut result = 0usize;
    while let Some(byte) = bytes.get(index).filter(|byte| byte.is_ascii_digit()) {
        result = result
            .saturating_mul(10)
            .saturating_add((byte - b'0') as usize);
        index += 1;
    }
    Some(result)
}

fn is_html_ascii_space(character: char) -> bool {
    matches!(
        character,
        '\u{0009}' | '\u{000A}' | '\u{000C}' | '\u{000D}' | '\u{0020}'
    )
}

/// Inline extraction for one outer table cell. An HTML table starts a new table
/// model, so its descendants cannot contribute runs to the containing cell.
fn cell_inline_runs<D: LayoutDom>(dom: &D, cell: D::NodeId) -> Vec<Inline> {
    let mut runs = Vec::new();
    for child in dom.dom_children(cell) {
        collect_cell_inline(dom, child, &mut runs);
    }
    trim_cell_inline_edges(&mut runs);
    runs
}

fn collect_cell_inline<D: LayoutDom>(dom: &D, id: D::NodeId, out: &mut Vec<Inline>) {
    if let Some(text) = dom.text(id) {
        let collapsed = collapse_cell_inline_text(text);
        if !collapsed.is_empty() {
            push_cell_text_run(out, collapsed);
        }
        return;
    }
    if html_name(dom, id) == Some("table") {
        return;
    }
    let name = dom.element_name(id).map(|name| name.local.as_ref());
    if name.is_some_and(is_cell_chrome_or_non_rendered) {
        return;
    }
    let nested = |dom: &D, id| {
        let mut runs = Vec::new();
        for child in dom.dom_children(id) {
            collect_cell_inline(dom, child, &mut runs);
        }
        trim_cell_inline_edges(&mut runs);
        runs
    };
    match name {
        Some("a") => {
            let runs = nested(dom, id);
            if let Some(href) = attr(dom, id, "href")
                && !runs.is_empty()
            {
                out.push(Inline::Link { href, runs });
            } else {
                out.extend(runs);
            }
        },
        Some("strong" | "b" | "em" | "i") => {
            let runs = nested(dom, id);
            if !runs.is_empty() {
                out.push(Inline::Emphasis {
                    strong: matches!(name, Some("strong" | "b")),
                    runs,
                });
            }
        },
        Some("code") => {
            let text = cell_text(dom, id);
            if !text.is_empty() {
                out.push(Inline::Code(text));
            }
        },
        Some("br") => push_cell_text_run(out, "\n".to_string()),
        _ => out.extend(nested(dom, id)),
    }
}

fn is_cell_chrome_or_non_rendered(name: &str) -> bool {
    matches!(
        name,
        "script"
            | "style"
            | "template"
            | "noscript"
            | "head"
            | "nav"
            | "header"
            | "footer"
            | "aside"
    )
}

fn collapse_cell_inline_text(text: &str) -> String {
    let leading = text.chars().next().is_some_and(char::is_whitespace);
    let trailing = text.chars().next_back().is_some_and(char::is_whitespace);
    let core = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if core.is_empty() {
        return if leading {
            " ".to_string()
        } else {
            String::new()
        };
    }
    format!(
        "{}{}{}",
        if leading { " " } else { "" },
        core,
        if trailing { " " } else { "" }
    )
}

fn push_cell_text_run(out: &mut Vec<Inline>, text: String) {
    if let Some(Inline::Text(previous)) = out.last_mut() {
        previous.push_str(&text);
    } else {
        out.push(Inline::Text(text));
    }
}

fn trim_cell_inline_edges(runs: &mut Vec<Inline>) {
    if let Some(Inline::Text(first)) = runs.first_mut() {
        *first = first.trim_start().to_string();
    }
    if let Some(Inline::Text(last)) = runs.last_mut() {
        *last = last.trim_end().to_string();
    }
    runs.retain(|run| !matches!(run, Inline::Text(text) if text.is_empty()));
}

fn cell_text<D: LayoutDom>(dom: &D, id: D::NodeId) -> String {
    fn collect<D: LayoutDom>(dom: &D, id: D::NodeId, out: &mut String) {
        if html_name(dom, id) == Some("table") {
            return;
        }
        if let Some(text) = dom.text(id) {
            out.push_str(text);
            out.push(' ');
        }
        for child in dom.dom_children(id) {
            collect(dom, child, out);
        }
    }

    let mut raw = String::new();
    collect(dom, id, &mut raw);
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn cell_is_empty<D: LayoutDom>(dom: &D, cell: D::NodeId) -> bool {
    !has_element_descendant(dom, cell)
        && text_content(dom, cell)
            .bytes()
            .all(|byte| is_html_ascii_space(byte as char))
}

fn has_element_descendant<D: LayoutDom>(dom: &D, id: D::NodeId) -> bool {
    dom.dom_children(id)
        .any(|child| dom.kind(child) == NodeKind::Element || has_element_descendant(dom, child))
}

fn text_content<D: LayoutDom>(dom: &D, id: D::NodeId) -> String {
    fn collect<D: LayoutDom>(dom: &D, id: D::NodeId, out: &mut String) {
        if dom.kind(id) == NodeKind::Text {
            if let Some(text) = dom.text(id) {
                out.push_str(text);
            }
        }
        for child in dom.dom_children(id) {
            collect(dom, child, out);
        }
    }

    let mut text = String::new();
    collect(dom, id, &mut text);
    text
}

fn html_element_children<D: LayoutDom>(dom: &D, id: D::NodeId) -> Vec<D::NodeId> {
    dom.dom_children(id)
        .filter(|child| html_name(dom, *child).is_some())
        .collect()
}

fn html_name<D: LayoutDom>(dom: &D, id: D::NodeId) -> Option<&str> {
    let name = dom.element_name(id)?;
    (name.ns.as_ref() == "http://www.w3.org/1999/xhtml").then_some(name.local.as_ref())
}

fn stable_dedup<T: Eq + std::hash::Hash + Copy>(values: &mut Vec<T>) {
    let mut seen = HashSet::new();
    values.retain(|value| seen.insert(*value));
}

#[cfg(test)]
mod tests {
    use super::*;
    use genet_static_dom::StaticDocument;

    fn table(html: &str) -> Table {
        let dom = StaticDocument::parse(html);
        let table = find_html_element(&dom, dom.document(), "table").expect("table");
        extract_table(&dom, table)
    }

    fn xml_table(body: &str) -> Table {
        let dom = StaticDocument::parse_xml(&format!(
            "<table xmlns='http://www.w3.org/1999/xhtml'>{body}</table>"
        ));
        let table = find_html_element(&dom, dom.document(), "table").expect("table");
        extract_table(&dom, table)
    }

    fn find_html_element<D: LayoutDom>(dom: &D, id: D::NodeId, name: &str) -> Option<D::NodeId> {
        if html_name(dom, id) == Some(name) {
            return Some(id);
        }
        dom.dom_children(id)
            .find_map(|child| find_html_element(dom, child, name))
    }

    #[test]
    fn retains_table_shape_and_rowspan_zero_with_groups() {
        let table = table(
            "<table id=totals><caption>Totals</caption><thead><tr><th id=h scope=col>Total</th></tr></thead><tbody><tr><th id=g scope=rowgroup rowspan=0>Group</th><td headers=h>1</td></tr><tr><td>2</td></tr></tbody></table>",
        );
        assert_eq!(table.id.as_deref(), Some("totals"));
        assert_eq!(table.caption.as_ref().map(Vec::len), Some(1));
        assert_eq!(table.row_groups.len(), 2);
        assert_eq!(table.rows[1].cells[0].rowspan, 0);
        assert_eq!(table.rows[1].cells[0].height, 2);
        assert_eq!(
            table.rows[1].cells[1].associated_headers[0].id.as_deref(),
            Some("h")
        );
        assert!(
            table.rows[2].cells[0]
                .associated_headers
                .iter()
                .any(|header| header.id.as_deref() == Some("g"))
        );
    }

    #[test]
    fn excludes_nested_table_rows_and_uses_explicit_headers() {
        let table = table(
            "<table><tbody><tr><th id=outer>Outer</th><td headers='outer outer'>value <table><tr><td>nested</td></tr></table></td></tr></tbody></table>",
        );
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0].cells.len(), 2);
        let data = &table.rows[0].cells[1];
        assert_eq!(data.headers, ["outer", "outer"]);
        assert_eq!(data.associated_headers.len(), 1);
        assert_eq!(data.associated_headers[0].id.as_deref(), Some("outer"));
        assert_eq!(data.runs, [Inline::Text("value".to_string())]);
    }

    #[test]
    fn retains_all_scope_states_and_an_irregular_grid() {
        let table = table(
            "<table><colgroup span=2><thead><tr><th scope=row>row</th><th scope=col>col</th><th scope=rowgroup>rowgroup</th><th scope=colgroup>colgroup</th></tr></thead><tbody><tr><td colspan=2>a</td><td rowspan=2>b</td><td>c</td></tr><tr><td>d</td><td>e</td><td>f</td></tr></tbody></table>",
        );
        assert_eq!(
            table.rows[0]
                .cells
                .iter()
                .map(|cell| cell.scope)
                .collect::<Vec<_>>(),
            [
                TableScope::Row,
                TableScope::Column,
                TableScope::RowGroup,
                TableScope::ColumnGroup,
            ]
        );
        assert_eq!(table.rows[1].cells[0].width, 2);
        assert_eq!(table.rows[1].cells[1].height, 2);
        assert_eq!(table.rows[2].cells[0].x, 0);
        assert_eq!(table.width, 4);
    }

    #[test]
    fn puts_tfoot_last_and_creates_implied_rows_for_rowspans() {
        let table = table(
            "<table><tfoot><tr><td>foot</td></tr></tfoot><tbody><tr><td rowspan=3>body</td></tr></tbody></table>",
        );
        assert_eq!(
            table
                .row_groups
                .iter()
                .map(|group| group.kind)
                .collect::<Vec<_>>(),
            [TableRowGroupKind::Body, TableRowGroupKind::Foot]
        );
        assert_eq!(table.row_groups[0].rows, [0, 1, 2]);
        assert_eq!(table.rows.len(), 4);
        assert!(table.rows[1].cells.is_empty());
        assert!(table.rows[2].cells.is_empty());
        assert_eq!(
            table.rows[3].cells[0].runs,
            [Inline::Text("foot".to_string())]
        );
    }

    #[test]
    fn parses_html_spans_without_unicode_trim_and_clamps_their_ranges() {
        let dom = StaticDocument::parse(
            "<table><tr><td id=zero colspan=0 rowspan=0></td><td id=wide colspan=1001 rowspan=70000></td><td id=bad colspan='\u{00a0}2' rowspan='\u{00a0}2'></td></tr></table>",
        );
        let zero = find_html_element(&dom, dom.document(), "td").expect("zero cell");
        let wide = dom
            .dom_children(find_html_element(&dom, dom.document(), "tr").expect("row"))
            .nth(1)
            .expect("wide cell");
        let bad = dom
            .dom_children(find_html_element(&dom, dom.document(), "tr").expect("row"))
            .nth(2)
            .expect("bad cell");
        assert_eq!(cell_colspan(&dom, zero), 1);
        assert_eq!(cell_rowspan(&dom, zero), 0);
        assert_eq!(cell_colspan(&dom, wide), 1000);
        assert_eq!(cell_rowspan(&dom, wide), 65534);
        assert_eq!(cell_colspan(&dom, bad), 1);
        assert_eq!(cell_rowspan(&dom, bad), 1);
    }

    #[test]
    fn explicit_headers_preserve_tokens_but_deduplicate_resolved_headers() {
        let table = table(
            "<table><tr><th id=h1>one</th><th id=h2>two</th><td headers='h2 h1 h2 missing h1'>value</td><td headers=missing>other</td></tr></table>",
        );
        let explicit = &table.rows[0].cells[2];
        assert_eq!(explicit.headers, ["h2", "h1", "h2", "missing", "h1"]);
        assert_eq!(
            explicit
                .associated_headers
                .iter()
                .map(|header| header.id.as_deref())
                .collect::<Vec<_>>(),
            [Some("h2"), Some("h1")]
        );
        assert!(
            table.rows[0].cells[3].associated_headers.is_empty(),
            "an invalid explicit list suppresses automatic headers"
        );
    }

    #[test]
    fn header_token_and_empty_header_rules_use_html_ascii_space_only() {
        let table = table(
            "<table><tr><th id=empty>\t\n\u{000c}\r </th><th id=vt>\u{000b}</th><td headers='empty vt vt'>value</td></tr></table>",
        );
        let data = &table.rows[0].cells[2];
        assert_eq!(data.headers, ["empty", "vt", "vt"]);
        assert_eq!(
            data.associated_headers
                .iter()
                .map(|header| header.id.as_deref())
                .collect::<Vec<_>>(),
            [Some("vt")]
        );
        let dom = StaticDocument::parse("<table><tr><td headers='a\u{000b}b c'></td></tr></table>");
        let cell = find_html_element(&dom, dom.document(), "td").expect("cell");
        assert_eq!(header_tokens(&dom, cell), ["a\u{000b}b", "c"]);
    }

    #[test]
    fn first_duplicate_id_shadows_a_later_same_table_header() {
        let table = table(
            "<svg><g id=h></g></svg><table><tr><th id=h>header</th><td headers=h>value</td></tr></table>",
        );
        assert!(table.rows[0].cells[1].associated_headers.is_empty());
    }

    #[test]
    fn implicit_rows_do_not_supply_a_rowgroup_scope() {
        let table = xml_table(
            "<tr><th id='group' scope='rowgroup'>group</th><td>one</td></tr><tr><td>two</td></tr>",
        );
        assert_eq!(table.rows[0].row_group, None);
        assert_eq!(table.rows[1].row_group, None);
        assert!(table.rows[1].cells[0].associated_headers.is_empty());
    }

    #[test]
    fn reports_overlapping_cells_without_reassigning_the_existing_slot() {
        let table = table(
            "<table><tbody><tr><td rowspan=2>a</td><td>b</td><td rowspan=2>c</td></tr><tr><td colspan=2>overlap</td></tr></tbody></table>",
        );
        assert_eq!(
            table.errors,
            [TableModelError::OverlappingCells {
                x: 2,
                y: 1,
                existing_x: 2,
                existing_y: 0,
                incoming_x: 1,
                incoming_y: 1,
            }]
        );
    }
}
