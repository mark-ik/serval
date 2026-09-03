// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Retained table paint structure: the per-table paint model and plane,
//! collapsed-track spans, relative table-part offsets, and the commit
//! that publishes a table's structure after Buckram accepts its grid.

use super::*;

/// Retained paint data for one table whose geometry Buckram accepted.
///
/// The fragment vector is the paint-order authority for table-internal boxes;
/// the live fragment tree supplies the final physical coordinates. The clip
/// set records the CSS Tables 3 rendering rule for a cell that crosses a
/// collapsed track boundary.
#[derive(Clone, Debug)]
pub(crate) struct TablePaintModel {
    pub(in crate::layout) fragments: TableFragments,
    pub(in crate::layout) separated: bool,
    pub(in crate::layout) collapsed_geometry: Option<CollapsedBorderGeometry<ComputedColor>>,
    pub(in crate::layout) clipped_cells: HashSet<BoxId>,
}

impl TablePaintModel {
    pub(crate) fn fragments(&self) -> &[buckram::TableFragment] {
        self.fragments.fragments()
    }

    pub(crate) fn is_separated(&self) -> bool {
        self.separated
    }

    pub(crate) fn is_collapsed(&self) -> bool {
        self.collapsed_geometry.is_some()
    }

    pub(crate) fn collapsed_segments(
        &self,
    ) -> Option<&[buckram::CollapsedBorderPaintSegment<ComputedColor>]> {
        self.collapsed_geometry
            .as_ref()
            .map(|geometry| geometry.segments.as_slice())
    }

    pub(crate) fn collapsed_table(&self) -> Option<BoxId> {
        self.collapsed_geometry
            .as_ref()
            .map(|geometry| geometry.table)
    }

    pub(in crate::layout) fn manages(&self, box_id: BoxId) -> bool {
        self.fragments.fragments().iter().any(|fragment| {
            fragment.box_id == Some(box_id)
                && (self.is_collapsed()
                    || (self.separated && fragment.role != TableFragmentRole::Grid))
        })
    }

    pub(in crate::layout) fn clips_cell(&self, box_id: BoxId) -> bool {
        self.clipped_cells.contains(&box_id)
    }

    pub(in crate::layout) fn remap_box_ids(&mut self, identities: &buckram::LayoutIdentityMap) {
        self.fragments
            .remap_box_ids(|box_id| identities.box_id(box_id));
        if let Some(geometry) = &mut self.collapsed_geometry {
            geometry.table = identities.box_id(geometry.table);
            for segment in &mut geometry.segments {
                segment.table = identities.box_id(segment.table);
                segment.winner = identities.box_id(segment.winner);
            }
        }
        self.clipped_cells = std::mem::take(&mut self.clipped_cells)
            .into_iter()
            .map(|box_id| identities.box_id(box_id))
            .collect();
    }
}

/// The paint-side index of every table that completed Buckram's block phase.
#[derive(Clone, Debug, Default)]
pub(crate) struct TablePaintPlane {
    pub(in crate::layout) tables: HashMap<BoxId, TablePaintModel>,
}

impl TablePaintPlane {
    pub(in crate::layout) fn table(&self, grid: BoxId) -> Option<&TablePaintModel> {
        self.tables.get(&grid)
    }

    pub(in crate::layout) fn manages(&self, box_id: BoxId) -> bool {
        self.tables.values().any(|table| table.manages(box_id))
    }

    pub(in crate::layout) fn clips_cell(&self, box_id: BoxId) -> bool {
        self.tables.values().any(|table| table.clips_cell(box_id))
    }

    pub(in crate::layout) fn fragments(&self) -> TableFragmentPlane {
        self.tables
            .iter()
            .map(|(grid, table)| (*grid, table.fragments.clone()))
            .collect()
    }

    pub(in crate::layout) fn merge(&mut self, other: Self) {
        self.tables.extend(other.tables);
    }

    /// Replace only the table paint models rooted in one reconciled fragment
    /// subtree. Their BoxIds already agree with the fresh layout; unrelated
    /// table models keep their existing structural fragments and paint order.
    pub(in crate::layout) fn replace_subtree_from<Id>(
        &mut self,
        fresh: &Self,
        boxes: &buckram::CssBoxTree<Id>,
        fresh_boxes: &buckram::CssBoxTree<Id>,
        root: BoxId,
        fresh_root: BoxId,
    ) where
        Id: Copy + Eq + Hash,
    {
        self.tables
            .retain(|grid, _| !box_is_descendant_of(boxes, *grid, root));
        self.tables.extend(
            fresh
                .tables
                .iter()
                .filter(|(grid, _)| box_is_descendant_of(fresh_boxes, **grid, fresh_root))
                .map(|(grid, table)| (*grid, table.clone())),
        );
    }

    pub(in crate::layout) fn remap_box_ids(&mut self, identities: &buckram::LayoutIdentityMap) {
        self.tables = std::mem::take(&mut self.tables)
            .into_iter()
            .map(|(grid, mut table)| {
                table.remap_box_ids(identities);
                (identities.box_id(grid), table)
            })
            .collect();
    }
}

pub(in crate::layout) fn table_cell_spans_collapsed_track(
    visibility: &TableTrackVisibility,
    cell: &TableCell,
) -> bool {
    let straddles = |collapsed: &dyn Fn(usize) -> bool, start: usize, span: usize| {
        let mut tracks = start..start.saturating_add(span);
        tracks.clone().any(collapsed) && tracks.any(|index| !collapsed(index))
    };
    straddles(
        &|index| visibility.column_is_collapsed(index),
        cell.column,
        cell.column_span,
    ) || straddles(
        &|index| visibility.row_is_collapsed(index),
        cell.row,
        cell.row_span,
    )
}

/// Resolve a table part's CSS relative-position offset. Inline percentages
/// resolve against the table grid's final inline size. Block percentages
/// resolve against the part's containing block only when that block size is
/// specified (CSS 2.1 §9.3.2); a cell inside an auto-height row, or a row in
/// an auto-height table, treats a percentage `top` or `bottom` as `auto`.
pub(in crate::layout) fn relative_table_part_offset(
    computed: &ComputedValues,
    font_size: f32,
    inline_basis: f32,
    block_basis: Option<f32>,
) -> (f32, f32) {
    if computed.position != CssPosition::Relative {
        return (0.0, 0.0);
    }
    let inline_inset = |value: Inset| match value {
        Inset::Auto => None,
        Inset::Value(value) => Some(signed_length_percentage_px(value, font_size, inline_basis)),
    };
    let block_inset = |value: Inset| match value {
        Inset::Auto => None,
        Inset::Value(value) => {
            FlowLengthAuto::Value(flow_length(value, font_size)).resolve_block(block_basis)
        },
    };
    let inline =
        inline_inset(computed.left).or_else(|| inline_inset(computed.right).map(|value| -value));
    let block =
        block_inset(computed.top).or_else(|| block_inset(computed.bottom).map(|value| -value));
    (inline.unwrap_or(0.0), block.unwrap_or(0.0))
}

/// The specified block-axis length of a table part's containing block: the
/// row for a cell, otherwise the table itself. Percentages and `auto` give no
/// basis, so the dependent percentage inset stays `auto`.
pub(in crate::layout) fn specified_table_block_basis<Id>(
    boxes: &GeneratedBoxTree<Id>,
    styles: &StylePlane<Id>,
    part: BoxId,
    table: BoxId,
    table_font_size: f32,
) -> Option<f32>
where
    Id: Copy + Eq + Hash,
{
    let owner = if boxes[part].display.internal_table == Some(InternalTableRole::Cell) {
        boxes[part].parent()?
    } else {
        table
    };
    let css_box = &boxes[owner];
    let computed = styles.get(css_box.origin.node()?)?;
    let font_size = font_size_px(&computed.font_size, table_font_size);
    let block_axis = if css_box.flow.is_horizontal() {
        computed.height
    } else {
        computed.width
    };
    match block_size_value(block_axis, font_size) {
        BlockSizeValue::Length(length) if length.percentage == 0.0 => Some(length.px),
        _ => None,
    }
}

/// Preserve relative positioning after table row and row-group boxes have
/// been flattened from the algorithm tree. Buckram owns their structural
/// fragments; the backend still owns a cell's contents, so the same cumulative
/// offsets must be applied to both representations before table dispatch.
pub(in crate::layout) fn apply_relative_table_part_offsets<Id>(
    block: &mut buckram::TableBlockLayout,
    table: BoxId,
    boxes: &GeneratedBoxTree<Id>,
    styles: &StylePlane<Id>,
    table_font_size: f32,
    inline_basis: f32,
    positioning_gaps: &mut Vec<TablePositioningGapRecord>,
) where
    Id: Copy + Eq + Hash,
{
    for fragment in block.fragments.fragments() {
        let Some(part) = fragment.box_id else {
            continue;
        };
        let BoxOrigin::Element(node) = boxes[part].origin else {
            continue;
        };
        let Some(computed) = styles.get(node) else {
            continue;
        };
        let gap = match computed.position {
            // CSS Tables transfers root positioning to the wrapper. K5d
            // resolves that wrapper through the shared positioned-fragment
            // path, so the grid itself must not retain a duplicate table gap.
            CssPosition::Absolute | CssPosition::Fixed if part == table => None,
            CssPosition::Absolute => Some(TablePositioningGap::Absolute),
            CssPosition::Fixed => Some(TablePositioningGap::Fixed),
            CssPosition::Sticky
                if part == table
                    || boxes[part]
                        .display
                        .internal_table
                        .is_some_and(supports_retained_sticky_table_part) =>
            {
                None
            },
            CssPosition::Sticky => Some(TablePositioningGap::Sticky),
            CssPosition::Static | CssPosition::Relative => None,
        };
        if let Some(gap) = gap {
            let record = TablePositioningGapRecord { table, part, gap };
            if !positioning_gaps.contains(&record) {
                positioning_gaps.push(record);
            }
        }
    }
    let offsets = block.fragments.apply_relative_offsets(|box_id| {
        table_part_relative_offset(box_id, table, boxes, styles, table_font_size, inline_basis)
    });

    for placement in &mut block.alignment.cells {
        let Some((_, (inline, block))) = offsets
            .iter()
            .find(|(box_id, _)| *box_id == placement.box_id)
        else {
            continue;
        };
        placement.rect.inline_start += inline;
        placement.rect.block_start += block;
    }
}

pub(in crate::layout) fn table_part_relative_offset<Id>(
    box_id: BoxId,
    table: BoxId,
    boxes: &GeneratedBoxTree<Id>,
    styles: &StylePlane<Id>,
    table_font_size: f32,
    inline_basis: f32,
) -> (f32, f32)
where
    Id: Copy + Eq + Hash,
{
    // The table grid remains in the ordinary tree, where its own relative
    // position is handled at the containing-block boundary.
    if box_id == table {
        return (0.0, 0.0);
    }
    let BoxOrigin::Element(node) = boxes[box_id].origin else {
        return (0.0, 0.0);
    };
    let Some(computed) = styles.get(node) else {
        return (0.0, 0.0);
    };
    let font_size = font_size_px(&computed.font_size, table_font_size);
    let block_basis = specified_table_block_basis(boxes, styles, box_id, table, table_font_size);
    relative_table_part_offset(computed, font_size, inline_basis, block_basis)
}

pub(in crate::layout) fn table_paint_plane<Id>(
    pending_tables: &[PendingTable<Id>],
    boxes: &GeneratedBoxTree<Id>,
    styles: &StylePlane<Id>,
) -> TablePaintPlane
where
    Id: Copy + Eq + Hash,
{
    let mut tables = HashMap::new();
    for pending in pending_tables {
        let Some(block) = &pending.block else {
            continue;
        };
        let visibility = crate::table_shadow::track_visibility(boxes, styles, &pending.grid);
        let clipped_cells = pending
            .grid
            .cells
            .iter()
            .filter(|cell| table_cell_spans_collapsed_track(&visibility, cell))
            .map(|cell| cell.source)
            .collect();
        let separated = pending.table_style.border_collapse == BorderCollapse::Separate;
        let collapsed = pending.table_style.border_collapse == BorderCollapse::Collapse;
        let collapsed_geometry = if !collapsed {
            None
        } else {
            let winners = pending.collapsed_borders.as_ref().expect(
                "a K4g4 collapsed table with emitted fragments retains its resolved winner grid",
            );
            // Relative table parts move only at paint time. Collapsed-border
            // tracks still belong to the unshifted table grid; deriving lines
            // from translated rows can make an otherwise ordered grid appear
            // decreasing when an early row moves past a later one.
            let mut grid_fragments = block.fragments.clone();
            let inline_basis = grid_fragments
                .grid()
                .map_or(0.0, |grid| grid.rect.inline_size);
            grid_fragments.apply_relative_offsets(|box_id| {
                let (inline, block) = table_part_relative_offset(
                    box_id,
                    pending.table,
                    boxes,
                    styles,
                    pending.font_size,
                    inline_basis,
                );
                (-inline, -block)
            });
            let lines = TableGridLines::from_fragments(&grid_fragments)
                .expect("K4d6 table fragments provide finite final lines for K4g5");
            Some(
                resolve_collapsed_border_geometry(pending.table, &lines, winners)
                    .expect("K4g2 winners lower once against K4g4 final table lines"),
            )
        };
        tables.insert(
            pending.table,
            TablePaintModel {
                fragments: block.fragments.clone(),
                separated,
                collapsed_geometry,
                clipped_cells,
            },
        );
    }
    TablePaintPlane { tables }
}

/// Commit every table-internal fragment Buckram emitted.
///
/// B5 makes the emitted cell fragment authoritative too: an empty cell may
/// have no ordinary algorithm fragment, but still owns a background, border,
/// and an `empty-cells` decision. The ordinary walk reuses this fragment when
/// it reaches a cell so text, baselines, and descendants retain their normal
/// path without registering a second cell box.
///
/// These are pushed before the walk descends into the cells, so each cell's
/// structural-parent lookup finds its own row rather than falling back to the
/// grid. Buckram guarantees parents precede children, so one forward pass
/// resolves every parent.
///
/// Rectangles are logical and the live path is horizontal LTR throughout, so
/// inline maps to x and block to y.
pub(in crate::layout) fn commit_table_structure<Id>(
    emitted: &TableFragments,
    grid_origin: Point<f32>,
    grid_fragment: FragmentId,
    boxes: &GeneratedBoxTree<Id>,
    output: &mut FragmentOutput<'_>,
) where
    Id: Copy + Eq + Hash,
{
    let mut ids: Vec<Option<FragmentId>> = vec![None; emitted.fragments().len()];
    for (index, fragment) in emitted.fragments().iter().enumerate() {
        // The walk already pushed the grid's own fragment; record it so
        // children can hang from it.
        if fragment.role == TableFragmentRole::Grid {
            ids[index] = Some(grid_fragment);
            output.fragments.set_overflow(
                grid_fragment,
                LogicalRect {
                    inline_start: grid_origin.x + fragment.overflow.inline_start,
                    block_start: grid_origin.y + fragment.overflow.block_start,
                    inline_size: fragment.overflow.inline_size,
                    block_size: fragment.overflow.block_size,
                },
            );
            continue;
        }
        // A track created implicitly by placement has no CSS box, so there is
        // no identity to attribute a fragment to.
        let Some(box_id) = fragment.box_id else {
            continue;
        };
        let parent = fragment
            .parent
            .and_then(|at| ids.get(at).copied().flatten())
            .unwrap_or(grid_fragment);
        let rect = Fragment {
            x: grid_origin.x + fragment.rect.inline_start,
            y: grid_origin.y + fragment.rect.block_start,
            width: fragment.rect.inline_size,
            height: fragment.rect.block_size,
        };
        record_static_position(boxes, box_id, Some(parent), fragment.rect, None, output);
        ids[index] = Some(output.fragments.push(
            TreeFragment::from_horizontal_physical(box_id, rect),
            Some(parent),
            Some(parent),
        ));
    }
}
