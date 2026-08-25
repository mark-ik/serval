//! K4c5b: Buckram owns live table inline sizing.
//!
//! Every table is noted at box-build time. Before the main
//! compute, [`buckram_table_columns`] lowers the table once, runs Buckram's
//! fixed or automatic algorithm, and the block pipeline commits the resulting
//! table geometry before backend dispatch.
//!
//! A table Buckram cannot size yet defers under a named skip, counted, never
//! silent. It remains on the Buckram table dispatcher. After fragment collection,
//! [`verify_assigned_columns`] asserts the painted single-span cell widths
//! match what Buckram assigned; a divergence there is an invariant violation
//! rather than a sizing disagreement.

use std::{collections::HashMap, hash::Hash};

use buckram::{
    AlgorithmNodeId, BoxId, BoxOrigin, CaptionMinContribution, CollapsedBorderMetrics,
    IntrinsicSizes, ResolvedTableBorderGrid, TableAutomaticColumnMeasureInput,
    TableAutomaticInlineSizingIndefinite, TableAutomaticInlineSizingInput,
    TableAutomaticInlineSizingOutcome, TableBlockLayout, TableCellInlineMeasure, TableDeferral,
    TableFixedInlineSizingInput, TableFixedInlineSizingOutcome, TableGrid,
    TableInlineBorderMetrics, TableInlineSizingError, TableInlineSizingResult,
    TableSeparatedBorderMetrics, TableTrackVisibility, TableTrackVisibilityState,
    measure_automatic_columns, size_automatic_table_inline, size_fixed_table_inline,
};
use livery::{
    ComputedValues,
    values::{BorderCollapse, ComputedColor, TableLayout as CssTableLayout, Visibility},
};

use crate::{
    StylePlane,
    box_tree::GeneratedBoxTree,
    table_block::TableBlockLedger,
    table_sizing::{
        CollapsedBorderLoweringError, automatic_table_track_inputs, collapsed_cell_inline_style,
        collapsed_table_inline_metrics, fixed_table_track_inputs, table_cell_inline_style,
        table_inline_constraints,
    },
};

/// The quantity a verification disagreed on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableSizingQuantity {
    ColumnCount,
    /// A painted single-span cell's width differs from the Buckram column it
    /// was assigned.
    ColumnSize(usize),
}

/// One disagreement between Buckram's assigned columns and the painted
/// fragments, attributable to a table box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TableSizingDivergence {
    pub table: BoxId,
    pub quantity: TableSizingQuantity,
    pub buckram: f32,
    pub livery: f32,
}

/// Why a table received no Buckram columns.
#[derive(Clone, Debug, PartialEq)]
pub enum TableShadowSkip {
    /// A named K4 gap. Never a silent fallback.
    Deferred(TableDeferral),
    /// A grid cell built no algorithm node, so its intrinsic pair cannot be
    /// measured.
    AutomaticIncompleteCells,
    /// Buckram declined a used size for an explicitly named missing basis.
    AutomaticIndefinite(TableAutomaticInlineSizingIndefinite),
    /// K4g2 could not retain a collapsed-border winner grid. This is distinct
    /// from a normal sizing deferral and is never silent.
    CollapsedBorder(CollapsedBorderLoweringError),
    Error(TableInlineSizingError),
}

/// A positioned table part whose containing-block and static-position rules
/// belong to K5. K4h keeps the table on Buckram dispatch and records the gap
/// instead of letting a backend table algorithm stand in for it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TablePositioningGap {
    Absolute,
    Fixed,
    Sticky,
}

/// One table part deferred to K5 positioning work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TablePositioningGapRecord {
    pub table: BoxId,
    pub part: BoxId,
    pub gap: TablePositioningGap,
}

/// Deferral counters and the honoring-verification record for one layout.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableShadowLedger {
    /// Collapsed tables whose K4g3 winners and metrics were lowered before
    /// the still-explicit K4g4 sizing deferral.
    pub collapsed_metrics: usize,
    /// Tables whose columns Buckram assigned.
    pub assigned: usize,
    /// Assigned tables whose painted fragments were verified.
    pub verified: usize,
    /// Verified tables whose fragments matched the assignment.
    pub honored: usize,
    pub divergences: Vec<TableSizingDivergence>,
    pub skipped: Vec<(BoxId, TableShadowSkip)>,
    /// Positioned table parts that remain a K5 containing-block and
    /// static-positioning concern.
    pub positioning_gaps: Vec<TablePositioningGapRecord>,
    /// K4d6b's block-axis counters. Nested rather than parallel: a table's
    /// two axes are one dispatch decision, and two ledgers threaded through
    /// three routes would drift apart.
    pub block: TableBlockLedger,
    /// Exact table-owned contributions. The aggregate fields above remain the
    /// public whole-layout receipt, while retained relayout can replace one
    /// entry without invalidating an unrelated table's verification record.
    per_table: HashMap<BoxId, Box<TableShadowLedger>>,
}

impl TableShadowLedger {
    pub fn is_silent(&self) -> bool {
        self.divergences.is_empty()
    }

    /// Fold another ledger in. Atomic subtrees each build under their own
    /// `BuildState`, and dropping their ledgers would leave tables reached
    /// through the text path unaccounted.
    pub fn merge(&mut self, other: Self) {
        if other.per_table.is_empty() {
            self.merge_summary(&other);
        } else {
            self.per_table.extend(other.per_table);
            self.rebuild_summary();
        }
    }

    pub(crate) fn remap_box_ids(&mut self, mut id_of: impl FnMut(BoxId) -> BoxId) {
        self.remap_summary_box_ids(&mut id_of);
        self.per_table = std::mem::take(&mut self.per_table)
            .into_iter()
            .map(|(table, mut entry)| {
                entry.remap_summary_box_ids(&mut id_of);
                (id_of(table), entry)
            })
            .collect();
    }

    fn remap_summary_box_ids(&mut self, id_of: &mut dyn FnMut(BoxId) -> BoxId) {
        for divergence in &mut self.divergences {
            divergence.table = id_of(divergence.table);
        }
        for (table, _) in &mut self.skipped {
            *table = id_of(*table);
        }
        for gap in &mut self.positioning_gaps {
            gap.table = id_of(gap.table);
            gap.part = id_of(gap.part);
        }
        self.block.remap_box_ids(id_of);
    }

    /// Publish a completed one-table contribution into the aggregate receipt.
    pub(crate) fn record_table(&mut self, table: BoxId, entry: Self) {
        self.per_table.insert(table, Box::new(entry));
        self.rebuild_summary();
    }

    /// Take one contribution while it receives its post-fragment verification.
    pub(crate) fn take_table(&mut self, table: BoxId) -> Self {
        let entry = self
            .per_table
            .remove(&table)
            .map(|entry| *entry)
            .unwrap_or_default();
        self.rebuild_summary();
        entry
    }

    pub(crate) fn can_replace_subtree<Id>(
        &self,
        fresh: &Self,
        boxes: &buckram::CssBoxTree<Id>,
        fresh_boxes: &buckram::CssBoxTree<Id>,
        root: BoxId,
        fresh_root: BoxId,
    ) -> bool
    where
        Id: Copy + Eq + Hash,
    {
        self.per_table
            .keys()
            .any(|table| table_is_descendant_of(boxes, *table, root))
            && fresh
                .per_table
                .keys()
                .any(|table| table_is_descendant_of(fresh_boxes, *table, fresh_root))
    }

    pub(crate) fn replace_subtree_from<Id>(
        &mut self,
        fresh: &Self,
        boxes: &buckram::CssBoxTree<Id>,
        fresh_boxes: &buckram::CssBoxTree<Id>,
        root: BoxId,
        fresh_root: BoxId,
    ) where
        Id: Copy + Eq + Hash,
    {
        self.per_table
            .retain(|table, _| !table_is_descendant_of(boxes, *table, root));
        self.per_table.extend(
            fresh
                .per_table
                .iter()
                .filter(|(table, _)| table_is_descendant_of(fresh_boxes, **table, fresh_root))
                .map(|(table, entry)| (*table, entry.clone())),
        );
        self.rebuild_summary();
    }

    pub(crate) fn skip(&mut self, table: BoxId, reason: TableShadowSkip) {
        self.skipped.push((table, reason));
    }

    fn merge_summary(&mut self, other: &Self) {
        self.collapsed_metrics += other.collapsed_metrics;
        self.assigned += other.assigned;
        self.verified += other.verified;
        self.honored += other.honored;
        self.divergences.extend(other.divergences.iter().copied());
        self.skipped.extend(other.skipped.iter().cloned());
        self.positioning_gaps.extend(other.positioning_gaps.iter().copied());
        self.block.merge(other.block.clone());
    }

    fn rebuild_summary(&mut self) {
        let mut summary = Self::default();
        for entry in self.per_table.values() {
            summary.merge_summary(entry);
        }
        self.collapsed_metrics = summary.collapsed_metrics;
        self.assigned = summary.assigned;
        self.verified = summary.verified;
        self.honored = summary.honored;
        self.divergences = summary.divergences;
        self.skipped = summary.skipped;
        self.positioning_gaps = summary.positioning_gaps;
        self.block = summary.block;
    }

    fn deferrals(&self) -> impl Iterator<Item = (BoxId, TableDeferral)> + '_ {
        self.skipped.iter().filter_map(|(table, skip)| match skip {
            TableShadowSkip::Deferred(deferral) => Some((*table, *deferral)),
            _ => None,
        })
    }

    /// Counts per named K4 gap, so a deferral can never be read as support.
    pub fn deferral_count(&self, deferral: TableDeferral) -> usize {
        self.deferrals().filter(|(_, one)| *one == deferral).count()
    }
}

fn table_is_descendant_of<Id>(
    boxes: &buckram::CssBoxTree<Id>,
    table: BoxId,
    root: BoxId,
) -> bool
where
    Id: Copy + Eq + Hash,
{
    let mut current = Some(table);
    while let Some(box_id) = current {
        if box_id == root {
            return true;
        }
        current = boxes[box_id].parent();
    }
    false
}

/// Tolerance for verifying assigned columns against painted fragments.
/// Fragments are pixel-rounded cumulatively (three 28.9px tracks paint as
/// 29, 29, 28), while Buckram's output is unrounded arithmetic, so 1px is
/// the honest unit of agreement.
const FRAGMENT_TOLERANCE: f32 = 1.0;

/// The live path resolves `rem` against a hardcoded 16px rather than the root
/// element's computed font size, in `length_percentage_px` and
/// `border_width_px`. The lowering matches that constant so Buckram's columns
/// and the rest of live layout resolve font-relative units identically.
/// Fixing the live assumption is its own change.
pub(crate) const LIVE_ROOT_FONT_SIZE: f32 = 16.0;

/// A flattenable table noted at box-build time. Its columns are computed
/// before the main compute and verified against fragments after collection.
pub struct PendingTable<Id> {
    pub table: BoxId,
    pub node: Option<Id>,
    pub table_style: ComputedValues,
    pub table_node: AlgorithmNodeId,
    /// The table wrapper box's node, once K4e1's wrapper has been built. It is
    /// built after the grid, so it registers itself here rather than arriving
    /// with the rest.
    pub wrapper: Option<AlgorithmNodeId>,
    /// Each caption's node and the horizontal margins around it, resolved when
    /// the wrapper was built. K4e3 measures the node and adds the margins to
    /// get the floor a caption puts under the table's inline size.
    pub captions: Vec<(AlgorithmNodeId, f32)>,
    pub grid: TableGrid,
    /// K4g2's logical atomic winners. K4g3 turns them into metrics before
    /// sizing consumes them.
    pub collapsed_borders: Option<ResolvedTableBorderGrid<ComputedColor>>,
    /// K4g3's exact segment-backed metric projection. K4g4 is its first
    /// sizing consumer; retaining it beside the winner grid prevents either
    /// K4c or K4d from selecting a side scalar on its own.
    pub collapsed_border_metrics: Option<CollapsedBorderMetrics>,
    /// One entry per K4b grid cell, in topology order.
    pub cell_nodes: Vec<Option<AlgorithmNodeId>>,
    /// Absolute and fixed table-part roots formatted after K4d has emitted
    /// every in-flow track. Their static anchors are zero-track placeholders,
    /// so they cannot re-enter table sizing.
    pub out_of_flow_parts: Vec<DetachedTablePart>,
    pub font_size: f32,
    pub containing_width: Option<f32>,
    pub containing_height: Option<f32>,
    /// The inline result Buckram assigned, once `buckram_table_columns` has
    /// run. The whole result is retained, not just its columns: K4d's block
    /// pipeline takes it as its inline input, and re-deriving the grid width
    /// or the undistributable remainder from the column vector alone would
    /// reintroduce exactly the arithmetic K4c owns.
    pub assigned: Option<TableInlineSizingResult>,
    /// The block-axis result, once the K4d pipeline has run.
    pub block: Option<TableBlockLayout>,
}

/// One table-internal absolute/fixed root detached from K4b/K4d track work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DetachedTablePart {
    pub box_id: BoxId,
    pub node: AlgorithmNodeId,
}

/// Compute Buckram's authoritative inline result for one live table.
///
/// `cell_border_box_intrinsics` are min/max-content border-box widths
/// measured through the live intrinsic machinery, per K4b cell; the cell's
/// lowered offsets convert them to the content pairs Buckram's contract
/// expects. They are consulted by the automatic algorithm, including the
/// CSS 2.1 fallback of a fixed-layout table whose width is not definite.
/// `None` means Buckram declined; the reason is recorded in the ledger.
#[expect(clippy::too_many_arguments, reason = "one call site per route")]
pub(crate) fn buckram_table_columns<Id>(
    boxes: &GeneratedBoxTree<Id>,
    styles: &StylePlane<Id>,
    grid: &TableGrid,
    table: BoxId,
    computed: &ComputedValues,
    collapsed_border_metrics: Option<&CollapsedBorderMetrics>,
    font_size: f32,
    containing_width: Option<f32>,
    caption_min: Option<f32>,
    cell_border_box_intrinsics: &[Option<IntrinsicSizes>],
    ledger: &mut TableShadowLedger,
) -> Option<TableInlineSizingResult>
where
    Id: Copy + Eq + Hash,
{
    // K4g produces the winner grid before either Buckram algorithm runs. A
    // lowering failure is already recorded as `CollapsedBorder` by the caller;
    // never reinterpret that table as `border-collapse: separate` here.
    if computed.border_collapse == BorderCollapse::Collapse && collapsed_border_metrics.is_none() {
        return None;
    }

    if computed.table_layout == CssTableLayout::Fixed {
        let input = match fixed_input(
            boxes,
            styles,
            grid,
            computed,
            collapsed_border_metrics,
            font_size,
            containing_width,
            caption_min,
        ) {
            Ok(input) => input,
            Err(error) => {
                ledger.skip(table, classify(error));
                return None;
            },
        };
        match size_fixed_table_inline(&input) {
            Ok(TableFixedInlineSizingOutcome::Fixed(result)) => {
                ledger.assigned += 1;
                return Some(result);
            },
            // CSS 2.1 17.5.2.1: fixed layout with an indefinite width uses
            // the automatic algorithm. Fall through.
            Ok(TableFixedInlineSizingOutcome::Automatic(_)) => {},
            Err(error) => {
                ledger.skip(table, classify(error));
                return None;
            },
        }
    }
    automatic_columns(
        boxes,
        styles,
        grid,
        table,
        computed,
        collapsed_border_metrics,
        font_size,
        containing_width,
        caption_min,
        cell_border_box_intrinsics,
        ledger,
    )
}

#[expect(clippy::too_many_arguments, reason = "one call site")]
fn automatic_columns<Id>(
    boxes: &GeneratedBoxTree<Id>,
    styles: &StylePlane<Id>,
    grid: &TableGrid,
    table: BoxId,
    computed: &ComputedValues,
    collapsed_border_metrics: Option<&CollapsedBorderMetrics>,
    font_size: f32,
    containing_width: Option<f32>,
    caption_min: Option<f32>,
    cell_border_box_intrinsics: &[Option<IntrinsicSizes>],
    ledger: &mut TableShadowLedger,
) -> Option<TableInlineSizingResult>
where
    Id: Copy + Eq + Hash,
{
    let sizing = match sizing_input(
        boxes,
        styles,
        grid,
        computed,
        collapsed_border_metrics,
        font_size,
        containing_width,
        caption_min,
    ) {
        Ok(sizing) => sizing,
        Err(error) => {
            ledger.skip(table, classify(error));
            return None;
        },
    };
    let (columns, column_groups) = automatic_table_track_inputs(grid, |source| {
        boxes
            .origin_node(source)
            .and_then(|node| styles.get(node))
            .map_or_else(Default::default, |style| {
                table_inline_constraints(style, font_size, LIVE_ROOT_FONT_SIZE)
            })
    });
    let cells = match lowered_cells(
        boxes,
        styles,
        grid,
        font_size,
        collapsed_border_metrics,
        |index, source, lowered| {
            let raw = cell_border_box_intrinsics
                .get(index)
                .copied()
                .flatten()
                .ok_or(TableInlineSizingError::InvalidResultSize)?;
            if lowered.offsets.needs_percentage_basis() {
                return Err(TableInlineSizingError::Deferral(
                    TableDeferral::PercentagePaddingPendingBasis,
                ));
            }
            let offsets = lowered
                .offsets
                .absolute_total()
                .ok_or(TableInlineSizingError::InvalidOffsets { box_id: source })?;
            // Live layout measures border boxes; Buckram's contract carries the
            // content pair and adds offsets itself.
            IntrinsicSizes::new(
                (raw.min_content - offsets).max(0.0),
                (raw.max_content - offsets).max(0.0),
            )
            .ok_or(TableInlineSizingError::InvalidResultSize)
        },
    ) {
        Ok(cells) => cells,
        Err(TableInlineSizingError::InvalidResultSize) => {
            ledger.skip(table, TableShadowSkip::AutomaticIncompleteCells);
            return None;
        },
        Err(error) => {
            ledger.skip(table, classify(error));
            return None;
        },
    };

    let input = TableAutomaticColumnMeasureInput {
        sizing,
        columns,
        column_groups,
        cells,
    };
    let measures = match measure_automatic_columns(&input) {
        Ok(measures) => measures,
        Err(error) => {
            ledger.skip(table, classify(error));
            return None;
        },
    };
    match size_automatic_table_inline(&TableAutomaticInlineSizingInput {
        sizing: input.sizing,
        measures: &measures,
    }) {
        Ok(TableAutomaticInlineSizingOutcome::Sized(result)) => {
            ledger.assigned += 1;
            Some(result)
        },
        Ok(TableAutomaticInlineSizingOutcome::Indefinite(reason)) => {
            ledger.skip(table, TableShadowSkip::AutomaticIndefinite(reason));
            None
        },
        Err(error) => {
            ledger.skip(table, classify(error));
            None
        },
    }
}

/// Assert the painted fragments honored an assigned column vector. `live`
/// holds one painted single-span cell width per K4b column, where a fragment
/// answers for it.
pub(crate) fn verify_assigned_columns(
    table: BoxId,
    assigned: &[f32],
    live: &[Option<f32>],
    ledger: &mut TableShadowLedger,
) {
    if assigned.len() != live.len() {
        ledger.verified += 1;
        ledger.divergences.push(TableSizingDivergence {
            table,
            quantity: TableSizingQuantity::ColumnCount,
            buckram: assigned.len() as f32,
            livery: live.len() as f32,
        });
        return;
    }
    let mut comparable = 0usize;
    let mut honored = true;
    for (index, (one, other)) in assigned.iter().zip(live).enumerate() {
        let Some(other) = other else { continue };
        comparable += 1;
        if (one - other).abs() > FRAGMENT_TOLERANCE {
            honored = false;
            ledger.divergences.push(TableSizingDivergence {
                table,
                quantity: TableSizingQuantity::ColumnSize(index),
                buckram: *one,
                livery: *other,
            });
        }
    }
    if comparable == 0 {
        return;
    }
    ledger.verified += 1;
    if honored {
        ledger.honored += 1;
    }
}

fn classify(error: TableInlineSizingError) -> TableShadowSkip {
    match error {
        TableInlineSizingError::Deferral(deferral) => TableShadowSkip::Deferred(deferral),
        other => TableShadowSkip::Error(other),
    }
}

/// K4f: which row and column tracks `visibility: collapse` removes.
///
/// CSS 2.1 section 17.5.5 applies the value to rows, row groups, columns, and
/// column groups; a group collapses every track in its range. The mask is
/// built from track and group identity rather than from cells, so a track with
/// no cell in it still collapses.
pub(crate) fn track_visibility<Id>(
    boxes: &GeneratedBoxTree<Id>,
    styles: &StylePlane<Id>,
    grid: &TableGrid,
) -> TableTrackVisibility
where
    Id: Copy + Eq + Hash,
{
    let collapsed = |source: BoxId| {
        boxes
            .origin_node(source)
            .and_then(|node| styles.get(node))
            .is_some_and(|computed| computed.visibility == Visibility::Collapse)
    };
    let mask = |tracks: &[buckram::TableTrack], groups: &[buckram::TableTrackGroup]| {
        let mut states = tracks
            .iter()
            .map(|track| match track.source.is_some_and(&collapsed) {
                true => TableTrackVisibilityState::Collapsed,
                false => TableTrackVisibilityState::Visible,
            })
            .collect::<Vec<_>>();
        for group in groups.iter().filter(|group| collapsed(group.source)) {
            for state in states.iter_mut().skip(group.start).take(group.span) {
                *state = TableTrackVisibilityState::Collapsed;
            }
        }
        states
    };
    TableTrackVisibility {
        rows: mask(&grid.rows, &grid.row_groups),
        columns: mask(&grid.columns, &grid.column_groups),
    }
}

/// Lower the table box's own geometry into the shared sizing input.
#[expect(
    clippy::too_many_arguments,
    reason = "the shared lowering both algorithms call; every argument is a               distinct CSS input rather than a group with a name"
)]
fn sizing_input<'a, Id>(
    boxes: &GeneratedBoxTree<Id>,
    styles: &StylePlane<Id>,
    grid: &'a TableGrid,
    computed: &ComputedValues,
    collapsed_border_metrics: Option<&CollapsedBorderMetrics>,
    font_size: f32,
    containing_width: Option<f32>,
    caption_min: Option<f32>,
) -> Result<buckram::TableInlineSizingInput<'a>, TableInlineSizingError>
where
    Id: Copy + Eq + Hash,
{
    let axes = buckram::FlowAxes::HORIZONTAL_LTR;
    let root_font_size = LIVE_ROOT_FONT_SIZE;
    let border_metrics = match computed.border_collapse {
        BorderCollapse::Collapse => {
            let metrics = collapsed_border_metrics
                .expect("the caller rejects a collapsed table whose winner lowering failed");
            TableInlineBorderMetrics::Collapsed(collapsed_table_inline_metrics(
                computed,
                axes,
                font_size,
                root_font_size,
                metrics,
            )?)
        },
        BorderCollapse::Separate => {
            let spacing = computed.border_spacing.horizontal;
            TableInlineBorderMetrics::Separated(TableSeparatedBorderMetrics {
                table_offsets: table_cell_inline_style(computed, axes, font_size, root_font_size)?
                    .offsets,
                inline_spacing: spacing.unit.to_px(spacing.value, font_size, root_font_size),
            })
        },
    };

    // K4e3 measures every in-flow caption through the live intrinsic
    // machinery before table sizing. The generated table topology, not the
    // DOM display value, decides participation: an absolute or fixed caption
    // is out of flow and therefore cannot put a sizing floor under the grid.
    let caption_min = if grid.captions.is_empty() {
        CaptionMinContribution::NoCaption
    } else {
        CaptionMinContribution::Measured(
            caption_min.expect("K4e supplies a caption minimum before Buckram table sizing"),
        )
    };

    Ok(buckram::TableInlineSizingInput {
        grid,
        available_inline_size: containing_width,
        table_constraints: table_inline_constraints(computed, font_size, root_font_size),
        border_metrics,
        caption_min,
        track_visibility: track_visibility(boxes, styles, grid),
    })
}

/// Lower every grid cell, supplying each content pair from `content_for` by
/// K4b cell index.
fn lowered_cells<Id>(
    boxes: &GeneratedBoxTree<Id>,
    styles: &StylePlane<Id>,
    grid: &TableGrid,
    font_size: f32,
    collapsed_border_metrics: Option<&CollapsedBorderMetrics>,
    mut content_for: impl FnMut(
        usize,
        BoxId,
        &buckram::TableCellInlineStyle,
    ) -> Result<IntrinsicSizes, TableInlineSizingError>,
) -> Result<Vec<TableCellInlineMeasure>, TableInlineSizingError>
where
    Id: Copy + Eq + Hash,
{
    let axes = buckram::FlowAxes::HORIZONTAL_LTR;
    let mut cells = Vec::with_capacity(grid.cells.len());
    for (index, cell) in grid.cells.iter().enumerate() {
        let style = match boxes[cell.source].origin {
            BoxOrigin::Anonymous { .. } => ComputedValues::default(),
            _ => boxes
                .origin_node(cell.source)
                .and_then(|node| styles.get(node))
                .cloned()
                .ok_or(TableInlineSizingError::InvalidOffsets {
                    box_id: cell.source,
                })?,
        };
        let lowered = match collapsed_border_metrics {
            Some(metrics) => collapsed_cell_inline_style(
                &style,
                axes,
                font_size,
                LIVE_ROOT_FONT_SIZE,
                metrics,
                cell.source,
            )?,
            None => table_cell_inline_style(&style, axes, font_size, LIVE_ROOT_FONT_SIZE)?,
        };
        cells.push(TableCellInlineMeasure {
            box_id: cell.source,
            content: content_for(index, cell.source, &lowered)?,
            preferred: lowered.constraints.preferred,
            minimum: lowered.constraints.minimum,
            maximum: lowered.constraints.maximum,
            box_sizing: lowered.constraints.box_sizing,
            offsets: lowered.offsets,
        });
    }
    Ok(cells)
}

/// Lower the live table once into Buckram's fixed input.
#[expect(
    clippy::too_many_arguments,
    reason = "lowering takes the whole context"
)]
fn fixed_input<'a, Id>(
    boxes: &GeneratedBoxTree<Id>,
    styles: &StylePlane<Id>,
    grid: &'a TableGrid,
    computed: &ComputedValues,
    collapsed_border_metrics: Option<&CollapsedBorderMetrics>,
    font_size: f32,
    containing_width: Option<f32>,
    caption_min: Option<f32>,
) -> Result<TableFixedInlineSizingInput<'a>, TableInlineSizingError>
where
    Id: Copy + Eq + Hash,
{
    let root_font_size = LIVE_ROOT_FONT_SIZE;
    let sizing = sizing_input(
        boxes,
        styles,
        grid,
        computed,
        collapsed_border_metrics,
        font_size,
        containing_width,
        caption_min,
    )?;
    let style_of = |source: BoxId| {
        boxes
            .origin_node(source)
            .and_then(|node| styles.get(node))
            .cloned()
    };
    let (columns, column_groups) = fixed_table_track_inputs(grid, |source| {
        style_of(source).map_or_else(Default::default, |style| {
            table_inline_constraints(&style, font_size, root_font_size)
        })
    });
    // Fixed layout never consults content, by definition of the algorithm.
    let cells = lowered_cells(
        boxes,
        styles,
        grid,
        font_size,
        collapsed_border_metrics,
        |_, _, _| IntrinsicSizes::new(0.0, 0.0).ok_or(TableInlineSizingError::InvalidResultSize),
    )?;

    Ok(TableFixedInlineSizingInput {
        sizing,
        columns,
        column_groups,
        cells,
    })
}
