use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fmt,
    hash::Hash,
};

use buckram::{
    AlgorithmAvailableSpace, AlgorithmKind, AlgorithmNodeId, AlgorithmSize, AlgorithmTree,
    Baselines, BlockBoxSizing, BlockCornerRadii, BlockCornerRadius, BlockDeferral, BlockDimensions,
    BlockPosition as BuckramBlockPosition, BlockSizeValue, BlockStyle, BoxId, BoxOrigin, ClearSide,
    CollapsedBorderGeometry, ContainingBlock, CssBox, DisplayInside, DisplayOutside,
    FloatContextProvenance, FloatLineConstraints, FloatReferenceBox, FloatSide, FlowAxes,
    FlowLength, FlowLengthAuto, FormattingContextKind, Fragment as TreeFragment, FragmentDraftTree,
    FragmentId, FragmentTree, InternalTableRole, IntrinsicSizeCache, IntrinsicSizeKind,
    IntrinsicSizeQuery, IntrinsicSizes, LayoutResult, LogicalAxis, LogicalRect,
    OverconstrainedInlineAlignment, PhysicalOffset, PhysicalRect, PhysicalSide, PhysicalSides,
    PhysicalSize, PositioningScheme, StaticPosition, StaticPositionSource, TableCell,
    TableCellInput, TableCellLayoutInput, TableCellLayoutOutput, TableCellLayoutPass,
    TableFragmentRole, TableFragments, TableGrid, TableGridInputs, TableGridLines,
    TableRowLayoutError, TableRowSpan, TableTrackInput, TableTrackVisibility,
    resolve_collapsed_border_geometry,
};
use layout_dom_api::{LayoutDom, LocalName, Namespace, NodeKind};
use livery::{
    ComputedValues,
    media::{Device, ViewportSizes},
    stylesheet::ContainerSnapshot,
    values::{
        Alignment as CssAlignment, BorderCollapse, BorderStyle, BorderWidth,
        BoxSizing as CssBoxSizing, CaptionSide, Clear as CssClear, ComputedColor, ContainerType,
        Direction as CssDirection, Display as CssDisplay, FlexBasis as CssFlexBasis,
        FlexDirection as CssFlexDirection, FlexWrap as CssFlexWrap, Float as CssFloat, FontSize,
        Gap as CssGap, GridAutoFlow as CssGridAutoFlow, GridPlacement as CssGridPlacement,
        GridTemplate as CssGridTemplate, GridTrack as CssGridTrack, Inset, Length,
        LengthPercentage as CssLengthPercentage, LineHeight, Margin, Overflow as CssOverflow,
        Position as CssPosition, Radius, RelativeLengthEnvironment,
        ShapeOutside as CssShapeOutside, Size as CssSize, VerticalAlign, WhiteSpaceCollapse,
        WritingMode as CssWritingMode,
    },
};
use taffy::{
    geometry::{Line, Point, Rect, Size},
    prelude::{
        Dimension, LengthPercentage, LengthPercentageAuto, auto, fr, length, line, max_content,
        min_content, percent, span,
    },
    style::{
        AlignContent, AlignContentKeyword, AlignItems, AlignItemsKeyword, BoxSizing,
        Direction as TaffyDirection, Display, FlexBasis as TaffyFlexBasis, FlexDirection, FlexWrap,
        Float as TaffyFloat, GridAutoFlow, GridPlacement, GridTemplateComponent, JustifyContent,
        Overflow, Position, Style,
    },
};

type ImageSources = HashMap<String, Vec<u8>>;

use crate::{
    InteractionStates, LegacyDescendantAlignment, StylePlane, StyleSet, TextSystem,
    box_tree::GeneratedBoxTree,
    style::resolve_styles_with_containers,
    table_block::{
        CellBlockInput, CellFormatter, buckram_table_block, cell_content_block_size,
        commit_table_block, table_block_inputs, verify_table_block,
    },
    table_shadow::{
        DetachedTablePart, LIVE_ROOT_FONT_SIZE, PendingTable, TablePositioningGap,
        TablePositioningGapRecord, TableShadowLedger, buckram_table_columns,
        verify_assigned_columns,
    },
    table_sizing::collapsed_table_borders,
    table_wrapper::{grid_style, wrapper_style},
    text::{InlineLayout, InlineRequest, TextFrame},
};

mod hit_testing;
mod positioned;
mod query;
mod tables;
mod taffy_style;
#[cfg(test)]
mod tests;

use positioned::*;
use tables::*;
use taffy_style::*;
// These four converters are crate vocabulary: text, paint, table sizing and
// the retained document all call them as `layout::<name>`. Re-exporting keeps
// those call sites exactly as they were.
pub(crate) use taffy_style::{
    border_width_px, length_percentage_px, line_height_px, signed_length_percentage_px,
};
// Crate and public surface the seams carried out with them: paint reads the
// table paint model and stacking order, and lib.rs re-exports both hit tests.
pub use hit_testing::{hit_test, hit_test_with_scroll};
pub(crate) use hit_testing::{order_modified_children, z_index_stacking_level};
pub(crate) use tables::TablePaintModel;

/// Physical geometry used at the DOM compatibility edge and by inline atoms.
pub(crate) type Fragment = PhysicalRect;

/// The static physical rectangle and content-scroll offset of one sticky
/// scrollport. The offset is added in layout space before ordinary scroll
/// painting translates its descendants back toward the viewport.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StickyScrollport {
    pub rect: PhysicalRect,
    pub offset: PhysicalOffset,
}

fn supports_retained_sticky_table_part(role: InternalTableRole) -> bool {
    matches!(
        role,
        InternalTableRole::Wrapper
            | InternalTableRole::Caption
            | InternalTableRole::RowGroup
            | InternalTableRole::HeaderGroup
            | InternalTableRole::FooterGroup
            | InternalTableRole::Row
            | InternalTableRole::Cell
    )
}

/// Table parts whose absolute/fixed fragments and static-position sources are
/// emitted outside K4d track sizing. Row groups, rows, and cells arrive from
/// the post-track zero-anchor formatter rather than from the table grid.
fn supports_shared_positioned_table_part(role: InternalTableRole) -> bool {
    matches!(
        role,
        InternalTableRole::Wrapper
            | InternalTableRole::Caption
            | InternalTableRole::RowGroup
            | InternalTableRole::HeaderGroup
            | InternalTableRole::FooterGroup
            | InternalTableRole::Row
            | InternalTableRole::Cell
    )
}

fn uses_zero_track_static_anchor(role: InternalTableRole) -> bool {
    matches!(
        role,
        InternalTableRole::RowGroup
            | InternalTableRole::HeaderGroup
            | InternalTableRole::FooterGroup
            | InternalTableRole::Row
            | InternalTableRole::Cell
    )
}

#[derive(Clone, Debug)]
struct AtomicSubtree {
    root: BoxId,
    fragments: Vec<AtomicFragment>,
    tables: TableFragmentPlane,
}

#[derive(Clone, Copy, Debug)]
struct AtomicFragment {
    box_id: BoxId,
    fragment: Fragment,
    static_fragment: Fragment,
    containing_block_area: Option<PhysicalRect>,
}

#[derive(Clone, Debug, Default)]
struct AtomicLayoutPlane {
    fragments: HashMap<BoxId, Fragment>,
    // An inline table participates in an ancestor intrinsic query with the
    // table grid's intrinsic pair, not the viewport-sized atomic fragment
    // produced for ordinary placement.
    intrinsic_inline: HashMap<BoxId, IntrinsicSizes>,
    // K4d5 table-grid first baselines, expressed from their inline-table
    // wrapper's margin-box block-start. Only inline-table wrappers populate
    // this map; other atomic boxes retain the existing block-end fallback.
    inline_baselines: HashMap<BoxId, f32>,
    subtrees: Vec<AtomicSubtree>,
    // Accumulated K4c5a shadow ledgers from each atomic root's BuildState.
    table_shadow: TableShadowLedger,
    table_paint: TablePaintPlane,
}

impl AtomicLayoutPlane {
    pub fn get(&self, box_id: BoxId) -> Option<&Fragment> {
        self.fragments.get(&box_id)
    }

    pub fn inline_baseline(&self, box_id: BoxId) -> Option<f32> {
        self.inline_baselines.get(&box_id).copied()
    }
}

impl<Id> crate::text::FragmentLookup<Id> for AtomicLayoutPlane
where
    Id: Copy + Eq + Hash,
{
    fn rect(&self, _id: Id) -> Option<&Fragment> {
        None
    }

    fn atomic_box_rect(&self, box_id: BoxId) -> Option<&Fragment> {
        self.get(box_id)
    }

    fn atomic_box_intrinsic_inline(&self, box_id: BoxId) -> Option<IntrinsicSizes> {
        self.intrinsic_inline.get(&box_id).copied()
    }

    fn atomic_box_baseline(&self, box_id: BoxId) -> Option<f32> {
        self.inline_baseline(box_id)
    }
}

/// Livery's retained wrapper around Buckram's standards-owned layout result.
#[derive(Clone, Debug)]
pub struct LiveryLayout<Id> {
    buckram: LayoutResult<Id>,
    text_frame: Option<TextFrame<Id>>,
    block_algorithms: BlockAlgorithmCounts,
    table_paint: TablePaintPlane,
    table_shadow: TableShadowLedger,
}

/// The outcome of attempting one bounded retained-root formatting pass.
/// `PromoteParent` is deliberately distinct from an unsupported root: only a
/// changed outer size can make the caller widen the retained replacement.
pub(crate) enum RetainedRootFormatting<Id> {
    Formatted(Box<LiveryLayout<Id>>),
    PromoteParent,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BlockAlgorithmCounts {
    pub buckram: usize,
    pub taffy: usize,
    /// Taffy block runs used only for intrinsic/backend scratch sizing.
    pub backend_sizing: usize,
}

impl BlockAlgorithmCounts {
    /// Taffy block runs caused by a CSS-facing deferral rather than scratch
    /// measurement. This is the admission metric for ancestor fallback.
    pub fn css_facing_taffy(self) -> usize {
        self.taffy.saturating_sub(self.backend_sizing)
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayoutError(String);

impl fmt::Display for LayoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl LayoutError {
    pub(crate) fn retained_state(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl Error for LayoutError {}

#[derive(Clone, Debug)]
struct TextMeasure {
    min_width: f32,
    max_width: f32,
    height: f32,
}

fn measure_text_algorithm_node(
    known: AlgorithmSize<Option<f32>>,
    available: AlgorithmSize<AlgorithmAvailableSpace>,
    context: Option<&mut TextMeasure>,
) -> AlgorithmSize<f32> {
    let Some(context) = context else {
        return AlgorithmSize::new(0.0, 0.0);
    };
    let available_width = match available.width {
        AlgorithmAvailableSpace::Definite(width) => width,
        AlgorithmAvailableSpace::MinContent => context.min_width,
        AlgorithmAvailableSpace::MaxContent => context.max_width,
    };
    AlgorithmSize::new(
        known
            .width
            .unwrap_or(context.max_width.min(available_width.max(0.0))),
        known.height.unwrap_or(context.height),
    )
}

struct BuildState<'a, D: LayoutDom> {
    dom: &'a D,
    styles: &'a StylePlane<D::NodeId>,
    boxes: &'a GeneratedBoxTree<D::NodeId>,
    tree: AlgorithmTree<Style, TextMeasure, Option<BoxId>>,
    image_sources: &'a ImageSources,
    text: Option<&'a mut TextSystem>,
    table_shadow: TableShadowLedger,
    pending_tables: Vec<PendingTable<D::NodeId>>,
}

struct InlineMeasure {
    owner: Option<BoxId>,
    roots: Vec<BoxId>,
    style: ComputedValues,
    width: f32,
    height: f32,
    /// Natural content-box dimensions for a directly measured replaced leaf.
    replaced_size: Option<(f32, f32)>,
    layouts: Vec<InlineLayoutEntry>,
    placement_constraints: Option<FloatLineConstraints>,
}

struct InlineLayoutEntry {
    width: f32,
    constraints: Option<FloatLineConstraints>,
    layout: InlineLayout<BoxId>,
}

#[derive(Clone, Copy)]
struct InlineMeasureGeometry<'a> {
    width: f32,
    intrinsic_kind: Option<IntrinsicSizeKind>,
    line_constraints: Option<&'a FloatLineConstraints>,
}

impl InlineMeasure {
    fn cached_size(
        &self,
        width: f32,
        constraints: Option<&FloatLineConstraints>,
    ) -> Option<(f32, f32)> {
        self.layouts
            .iter()
            .find(|entry| {
                (entry.width - width).abs() <= 0.01 && entry.constraints.as_ref() == constraints
            })
            .map(|entry| entry.layout.size())
    }

    fn remember(
        &mut self,
        width: f32,
        constraints: Option<&FloatLineConstraints>,
        layout: InlineLayout<BoxId>,
    ) -> (f32, f32) {
        let size = layout.size();
        self.layouts.push(InlineLayoutEntry {
            width,
            constraints: constraints.cloned(),
            layout,
        });
        size
    }

    fn layout_for_width(&self, width: f32) -> Option<&InlineLayout<BoxId>> {
        self.layouts
            .iter()
            .filter(|entry| entry.constraints.as_ref() == self.placement_constraints.as_ref())
            .min_by(|left, right| {
                (left.width - width)
                    .abs()
                    .total_cmp(&(right.width - width).abs())
            })
            .map(|entry| &entry.layout)
    }
}

fn measure_inline_context<D>(
    text: &mut TextSystem,
    dom: &D,
    styles: &StylePlane<D::NodeId>,
    boxes: &GeneratedBoxTree<D::NodeId>,
    atomic: &AtomicLayoutPlane,
    context: &mut InlineMeasure,
    geometry: InlineMeasureGeometry<'_>,
) -> (f32, f32)
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    if let Some(size) = context.replaced_size {
        return size;
    }
    let InlineMeasureGeometry {
        width,
        intrinsic_kind,
        line_constraints: constraints,
    } = geometry;
    if let Some(constraints) = constraints {
        context.placement_constraints = Some(constraints.clone());
    }
    context.cached_size(width, constraints).unwrap_or_else(|| {
        let formatted = text.format_inline_group(
            dom,
            styles,
            boxes,
            atomic,
            InlineRequest {
                roots: &context.roots,
                parent_style: &context.style,
                width,
                intrinsic_kind,
                line_constraints: constraints,
            },
        );
        formatted.map_or((context.width, context.height), |layout| {
            context.remember(width, constraints, layout)
        })
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "the measured inline context needs its formatter, DOM, style, box, and atomic inputs"
)]
fn measure_inline_algorithm_node<D>(
    text: &mut TextSystem,
    dom: &D,
    styles: &StylePlane<D::NodeId>,
    boxes: &GeneratedBoxTree<D::NodeId>,
    atomic: &AtomicLayoutPlane,
    intrinsic_sizes: &mut IntrinsicSizeCache,
    known: AlgorithmSize<Option<f32>>,
    available: AlgorithmSize<AlgorithmAvailableSpace>,
    context: Option<&mut InlineMeasure>,
    line_constraints: Option<&FloatLineConstraints>,
) -> AlgorithmSize<f32>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let Some(context) = context else {
        return AlgorithmSize::new(0.0, 0.0);
    };
    if let Some((width, height)) = context.replaced_size {
        return AlgorithmSize::new(known.width.unwrap_or(width), known.height.unwrap_or(height));
    }
    let (query_width, definite_cap, intrinsic_kind) = match available.width {
        AlgorithmAvailableSpace::Definite(width) => (width, Some(width), None),
        // A nearly-zero line asks Parley to break at every legal
        // opportunity while retaining each unbreakable item's width.
        AlgorithmAvailableSpace::MinContent => (0.01, None, Some(IntrinsicSizeKind::MinContent)),
        // An infinite line suppresses wrapping and yields max-content.
        AlgorithmAvailableSpace::MaxContent => {
            (f32::INFINITY, None, Some(IntrinsicSizeKind::MaxContent))
        },
    };
    let intrinsic_width = intrinsic_kind.and_then(|kind| {
        let owner = context.owner?;
        let query = IntrinsicSizeQuery::new(owner, LogicalAxis::Inline, kind);
        intrinsic_sizes.get(query).or_else(|| {
            let min_content = measure_inline_context(
                text,
                dom,
                styles,
                boxes,
                atomic,
                context,
                InlineMeasureGeometry {
                    width: 0.01,
                    intrinsic_kind: Some(IntrinsicSizeKind::MinContent),
                    line_constraints: None,
                },
            )
            .0;
            let max_content = measure_inline_context(
                text,
                dom,
                styles,
                boxes,
                atomic,
                context,
                InlineMeasureGeometry {
                    width: f32::INFINITY,
                    intrinsic_kind: Some(IntrinsicSizeKind::MaxContent),
                    line_constraints: None,
                },
            )
            .0;
            let sizes = IntrinsicSizes::new(min_content, max_content)?;
            let result = sizes.get(kind);
            intrinsic_sizes.insert(owner, LogicalAxis::Inline, sizes);
            Some(result)
        })
    });
    let requested_width = known.width.or(intrinsic_width).unwrap_or(query_width);
    let (measured_width, measured_height) = measure_inline_context(
        text,
        dom,
        styles,
        boxes,
        atomic,
        context,
        InlineMeasureGeometry {
            width: requested_width,
            intrinsic_kind,
            line_constraints: intrinsic_kind
                .is_none()
                .then_some(line_constraints)
                .flatten(),
        },
    );
    AlgorithmSize::new(
        known.width.unwrap_or_else(|| {
            intrinsic_width.unwrap_or_else(|| {
                definite_cap.map_or(measured_width, |cap| measured_width.min(cap.max(0.0)))
            })
        }),
        known.height.unwrap_or(measured_height),
    )
}

/// Compare one table's Buckram result against its painted fragments, in both
/// axes.
///
/// Buckram's rectangles are relative to the table grid's own origin, so the
/// block-axis comparison subtracts the grid's painted position. Without that
/// the table's place in the page would be reported as a table-layout
/// disagreement on every cell.
fn verify_one_table<Id>(
    pending: &PendingTable<Id>,
    live_rect_of: &impl Fn(BoxId) -> Option<Fragment>,
    ledger: &mut TableShadowLedger,
) {
    if let Some(assigned) = pending.assigned.as_ref().map(|inline| &inline.column_sizes) {
        let live = pending
            .grid
            .columns
            .iter()
            .enumerate()
            .map(|(index, _)| {
                pending
                    .grid
                    .cells
                    .iter()
                    .find(|cell| cell.column == index && cell.column_span == 1)
                    .and_then(|cell| live_rect_of(cell.source))
                    .map(|rect| rect.width)
            })
            .collect::<Vec<_>>();
        verify_assigned_columns(pending.table, assigned, &live, ledger);
    }
    let (Some(block), Some(grid)) = (pending.block.as_ref(), live_rect_of(pending.table)) else {
        return;
    };
    verify_table_block(
        pending.table,
        block,
        |box_id| live_rect_of(box_id).map(|rect| (rect.y - grid.y, rect.height)),
        &mut ledger.block,
    );
}

/// Format one table cell for Buckram's block pipeline.
///
/// The cell is laid out at exactly the content inline size K4c assigned, with
/// its own inline and block constraints neutralized: Buckram applies those
/// itself, and a floor applied here would come back as measured content. The
/// cell's specified block size reaches Buckram as a row constraint, never as
/// a taller content box, which is why the measurement pass leaves the height
/// automatic.
fn format_table_cell<Context, Source>(
    tree: &mut AlgorithmTree<Style, Context, Source>,
    node: AlgorithmNodeId,
    request: TableCellLayoutInput,
    cell: &CellBlockInput,
    mut measure: impl FnMut(&mut Context, InlineMeasureGeometry<'_>) -> (f32, f32),
) -> TableCellLayoutOutput {
    let offsets = cell.style.offsets;
    let style = tree.style_mut(node);
    let saved = (style.size, style.min_size, style.max_size, style.box_sizing);
    style.box_sizing = BoxSizing::ContentBox;
    style.size.width = Dimension::length(request.content_inline_size);
    style.min_size.width = Dimension::auto();
    style.max_size.width = Dimension::auto();
    style.min_size.height = Dimension::auto();
    style.max_size.height = Dimension::auto();
    style.size.height = match request.pass {
        TableCellLayoutPass::Measure => Dimension::auto(),
        // The percentage pass supplies a used border-box block size; the
        // cell's content box is that minus the offsets Buckram already knows.
        TableCellLayoutPass::ResolvePercentages { cell_block_size } => {
            Dimension::length(cell_content_block_size(cell_block_size, offsets))
        },
    };
    tree.compute_layout_with_measure_excluding_out_of_flow_children(
        node,
        AlgorithmSize::new(
            AlgorithmAvailableSpace::Definite(request.content_inline_size),
            AlgorithmAvailableSpace::MaxContent,
        ),
        |known, available, _, context, _| {
            let Some(context) = context else {
                return AlgorithmSize::new(0.0, 0.0);
            };
            let (width, intrinsic_kind) = match available.width {
                AlgorithmAvailableSpace::Definite(width) => (width, None),
                AlgorithmAvailableSpace::MinContent => (0.01, Some(IntrinsicSizeKind::MinContent)),
                AlgorithmAvailableSpace::MaxContent => {
                    (f32::INFINITY, Some(IntrinsicSizeKind::MaxContent))
                },
            };
            let (measured_width, measured_height) = measure(
                context,
                InlineMeasureGeometry {
                    width: known.width.unwrap_or(width),
                    intrinsic_kind,
                    line_constraints: None,
                },
            );
            AlgorithmSize::new(
                known.width.unwrap_or(measured_width),
                known.height.unwrap_or(measured_height),
            )
        },
    );
    let border_box = tree.unrounded_layout(node).height;
    let baselines = tree.baselines(node);
    let style = tree.style_mut(node);
    (style.size, style.min_size, style.max_size, style.box_sizing) = saved;
    TableCellLayoutOutput {
        content_block_size: cell_content_block_size(border_box, offsets),
        // CSS 2.1 section 10.7 leaves min-height and max-height undefined on
        // a table cell, and the K4d4c matrix measured both engines ignoring
        // them outright, so a cell carries no border-box floor of its own.
        border_box_min_block_size: 0.0,
        // Live descendants stay in the backend tree, which fragment
        // collection already walks. K4d6a's drafts exist for adapters with no
        // such tree, and a zero rectangle unions into the cell's own without
        // changing it.
        baselines,
        overflow: LogicalRect::default(),
        fragments: FragmentDraftTree::default(),
    }
}

/// Feed retained IFC line baselines into Buckram before fragment collection.
/// Parent block, flex, and grid contexts consume these declared outputs
/// through `AlgorithmTree::propagate_declared_baselines`; no post-layout
/// traversal of backend children is involved.
fn populate_inline_baselines(tree: &mut AlgorithmTree<Style, InlineMeasure, Vec<BoxId>>) {
    let direct = tree
        .node_ids()
        .filter_map(|node| {
            let width = tree.layout(node).width;
            tree.context(node)
                .and_then(|context| context.layout_for_width(width))
                .and_then(|layout| layout.baselines())
                .and_then(|(first, last)| Baselines::new(Some(first), Some(last)))
                .map(|baselines| (node, baselines))
        })
        .collect::<Vec<_>>();
    for (node, baselines) in direct {
        tree.set_baselines(node, baselines);
    }
    tree.propagate_declared_baselines();
}

/// Ask the formatter for the admitted intrinsic pair of each positioned
/// root. The map is keyed by Buckram box identity, never a backend node or a
/// completed normal-flow rectangle.
fn positioned_intrinsic_sizes<Context, Source>(
    tree: &mut AlgorithmTree<Style, Context, Source>,
    candidates: &[(BoxId, AlgorithmNodeId)],
    mut measure: impl FnMut(
        AlgorithmSize<Option<f32>>,
        AlgorithmSize<AlgorithmAvailableSpace>,
        AlgorithmNodeId,
        Option<&mut Context>,
        Option<&FloatLineConstraints>,
    ) -> AlgorithmSize<f32>,
) -> HashMap<BoxId, IntrinsicSizes> {
    candidates
        .iter()
        .filter_map(|(box_id, node)| {
            tree.positioned_intrinsic_inline_sizes(
                *node,
                |known, available, node, context, lines| {
                    measure(known, available, node, context, lines)
                },
            )
            .map(|sizes| (*box_id, sizes))
        })
        .collect()
}

struct InlineBuildState<'a, D: LayoutDom> {
    dom: &'a D,
    styles: &'a StylePlane<D::NodeId>,
    boxes: &'a GeneratedBoxTree<D::NodeId>,
    atomic: &'a AtomicLayoutPlane,
    tree: AlgorithmTree<Style, InlineMeasure, Vec<BoxId>>,
    image_sources: &'a ImageSources,
    table_shadow: TableShadowLedger,
    pending_tables: Vec<PendingTable<D::NodeId>>,
    /// The grid, in-flow cell nodes, and detached table-part nodes for the
    /// table `build_children` just processed, consumed by `build_box` when it
    /// creates the table's algorithm node.
    pending_table_handoff: Option<(
        TableGrid,
        Vec<Option<AlgorithmNodeId>>,
        Vec<DetachedTablePart>,
    )>,
}

type ResolvedLayout<Id> = (StylePlane<Id>, LiveryLayout<Id>);

#[derive(Clone, Copy, Debug, Default)]
struct ContainerBases {
    width: Option<f32>,
    height: Option<f32>,
    inline: Option<f32>,
    block: Option<f32>,
}

/// Lay out a Livery style plane through Buckram's scratch algorithm tree.
///
/// This stateless entry point uses deterministic text estimates. Retained
/// Livery sessions call [`layout_with_text_system`] so Parley's shaped line
/// height participates in parent block flow.
pub fn layout<D>(
    dom: &D,
    styles: &StylePlane<D::NodeId>,
    viewport_width: f32,
    viewport_height: f32,
) -> Result<LiveryLayout<D::NodeId>, LayoutError>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    taffy_style::reset_calc_scratch();
    let image_sources = ImageSources::new();
    let viewport = ViewportSizes::uniform(viewport_width, viewport_height);
    let resolved =
        resolve_container_relative_styles_with_images(dom, styles, viewport, &image_sources)?;
    layout_impl(
        dom,
        &resolved,
        viewport_width,
        viewport_height,
        &image_sources,
    )
}

/// Produce the layout bases needed by resolved-value CSSOM reads without
/// letting the queried element's own margin expression participate in the
/// measurement. This matters for percentage-bearing margin math: its basis is
/// the containing block, which must be known before the expression can be
/// evaluated.
pub fn used_value_context<D>(
    dom: &D,
    styles: &StylePlane<D::NodeId>,
    viewport_width: f32,
    viewport_height: f32,
    node: D::NodeId,
) -> Result<Option<crate::UsedValueContext>, LayoutError>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let mut measuring = styles.clone();
    if let Some(style) = measuring.get_mut(node) {
        let zero = Margin::Value(CssLengthPercentage::ZERO);
        style.margin_top = zero;
        style.margin_right = zero;
        style.margin_bottom = zero;
        style.margin_left = zero;
    }
    let fragments = layout(dom, &measuring, viewport_width, viewport_height)?;
    // K4e4: used `width` and `height` are properties of the principal box.
    // For a table element that is the grid, whose border box excludes the
    // captions the wrapper contains.
    let Some(fragment) = fragments.principal_fragment(node) else {
        return Ok(None);
    };
    let containing_inline_size = dom.parent(node).and_then(|parent| {
        let style = measuring.get(parent)?;
        let fragment = fragments.get(parent)?;
        Some(content_box_size(style, fragment).0)
    });
    Ok(Some(crate::UsedValueContext {
        border_box: (fragment.width, fragment.height),
        containing_inline_size,
    }))
}

/// Lay out a retained live document through the caller-owned text system and
/// image ledger. `LiveryDocument` uses this internally; scripted hosts use the
/// same entry when their runtime owns the DOM.
pub fn layout_with_text_system<D>(
    dom: &D,
    styles: &StylePlane<D::NodeId>,
    viewport_width: f32,
    viewport_height: f32,
    viewport: ViewportSizes,
    text: &mut TextSystem,
    image_sources: &ImageSources,
) -> Result<ResolvedLayout<D::NodeId>, LayoutError>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    taffy_style::reset_calc_scratch();
    let styles =
        resolve_container_relative_styles_with_images(dom, styles, viewport, image_sources)?;
    let boxes = GeneratedBoxTree::from_dom(dom, &styles);
    let atomic = layout_atomic_subtrees(
        dom,
        &styles,
        &boxes,
        viewport_width,
        viewport_height,
        text,
        image_sources,
    )?;
    let fragments = layout_inline_groups(
        dom,
        &styles,
        boxes,
        (viewport_width, viewport_height),
        text,
        &atomic,
        image_sources,
    )?;
    Ok((styles, fragments))
}

/// Reformat exactly one retained block, flex, or grid root against its
/// existing parent content box. This is intentionally narrower than complete
/// layout:
/// tables, inline atoms, floats, and positioned descendants retain the
/// full-document path until their side planes have an equivalent replacement
/// primitive. Its local text frame is shaped with the document's retained
/// text system, then merged into the outside frame at publication.
pub(crate) fn layout_retained_formatting_root<D>(
    dom: &D,
    styles: &StylePlane<D::NodeId>,
    previous_styles: &StylePlane<D::NodeId>,
    previous: &LiveryLayout<D::NodeId>,
    node: D::NodeId,
    text: &mut TextSystem,
    image_sources: &ImageSources,
) -> Result<RetainedRootFormatting<D::NodeId>, LayoutError>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    if styles.get(node) != previous_styles.get(node) {
        return Ok(RetainedRootFormatting::Unsupported);
    }

    let boxes = GeneratedBoxTree::from_dom(dom, styles);
    let Some(root_box) = retained_root_box(&boxes, node) else {
        return Ok(RetainedRootFormatting::Unsupported);
    };
    let table_root = boxes[root_box].display.internal_table == Some(InternalTableRole::Wrapper);
    if !(table_root && supports_retained_table_root_formatting(&boxes, root_box)
        || !table_root && supports_retained_root_formatting(&boxes, root_box))
        || !retained_ancestor_styles_unchanged(&boxes, styles, previous_styles, root_box)
    {
        return Ok(RetainedRootFormatting::Unsupported);
    }

    let Some(previous_root_box) = retained_root_box(previous.boxes(), node) else {
        return Ok(RetainedRootFormatting::Unsupported);
    };
    let [previous_root] = previous.fragments().fragment_ids_for_box(previous_root_box) else {
        return Ok(RetainedRootFormatting::Unsupported);
    };
    let Some(previous_root_fragment) = previous.fragments().get(*previous_root) else {
        return Ok(RetainedRootFormatting::Unsupported);
    };
    let Some(parent_box) = previous.boxes()[previous_root_box].parent() else {
        return Ok(RetainedRootFormatting::Unsupported);
    };
    let Some(parent_node) = previous.boxes().origin_node(parent_box) else {
        return Ok(RetainedRootFormatting::Unsupported);
    };
    let Some(parent_style) = previous_styles.get(parent_node) else {
        return Ok(RetainedRootFormatting::Unsupported);
    };
    let Some(parent_fragment) = previous_root_fragment
        .parent()
        .and_then(|parent| previous.fragments().get(parent))
    else {
        return Ok(RetainedRootFormatting::Unsupported);
    };
    let containing_size = content_box_size(parent_style, parent_fragment);
    if !containing_size.0.is_finite()
        || !containing_size.1.is_finite()
        || containing_size.0 < 0.0
        || containing_size.1 < 0.0
    {
        return Ok(RetainedRootFormatting::Unsupported);
    }

    let atomic = AtomicLayoutPlane::default();
    let mut intrinsic_sizes = IntrinsicSizeCache::default();
    let mut state = InlineBuildState {
        dom,
        styles,
        boxes: &boxes,
        atomic: &atomic,
        tree: {
            let mut tree = AlgorithmTree::new();
            tree.set_calc_resolver(resolve_taffy_calc);
            tree
        },
        image_sources,
        table_shadow: TableShadowLedger::default(),
        pending_tables: Vec::new(),
        pending_table_handoff: None,
    };
    let parent_font_size = inherited_font_size(&boxes, styles, root_box);
    let Some(formatted_root) = state.build_box(
        root_box,
        None,
        parent_font_size,
        (Some(containing_size.0), Some(containing_size.1)),
    )?
    else {
        return Ok(RetainedRootFormatting::Unsupported);
    };
    let formatter_root = state.tree.new_with_children_and_block_style(
        AlgorithmKind::Block,
        BlockStyle {
            size: BlockDimensions::new(
                BlockSizeValue::Length(FlowLength::px(containing_size.0)),
                BlockSizeValue::Length(FlowLength::px(containing_size.1)),
            ),
            ..BlockStyle::default()
        },
        Style {
            display: Display::Block,
            size: Size {
                width: Dimension::length(containing_size.0),
                height: Dimension::length(containing_size.1),
            },
            ..Style::default()
        },
        &[formatted_root],
        Vec::new(),
    );
    state.apply_buckram_table_layout(text);
    state.tree.compute_layout_with_measure(
        formatter_root,
        AlgorithmSize::new(
            AlgorithmAvailableSpace::Definite(containing_size.0),
            AlgorithmAvailableSpace::Definite(containing_size.1),
        ),
        |known, available, _, context, line_constraints| {
            measure_inline_algorithm_node(
                text,
                dom,
                styles,
                &boxes,
                &atomic,
                &mut intrinsic_sizes,
                known,
                available,
                context,
                line_constraints,
            )
        },
    );
    populate_inline_baselines(&mut state.tree);
    let (buckram_blocks, taffy_blocks) = state.tree.block_algorithm_counts();
    let backend_sizing_blocks = state
        .tree
        .block_deferral_count(BlockDeferral::BackendSizingMode);
    let mut fragments = FragmentTree::default();
    let mut text_frame = TextFrame::default();
    let mut output = FragmentOutput {
        fragments: &mut fragments,
    };
    let table_paint = state.table_paint_plane();
    let tables = table_paint.fragments();
    collect_inline_fragments(
        &state.tree,
        &boxes,
        formatter_root,
        FragmentCursor {
            origin: Point { x: 0.0, y: 0.0 },
            containing: Fragment {
                x: 0.0,
                y: 0.0,
                width: containing_size.0,
                height: containing_size.1,
            },
            parent: None,
        },
        &tables,
        &mut output,
        &mut text_frame,
        styles,
    )?;
    state.verify_table_layout(|box_id| {
        fragments
            .fragments_for_box(box_id)
            .next()
            .map(|fragment| Fragment {
                x: fragment.x,
                y: fragment.y,
                width: fragment.width,
                height: fragment.height,
            })
    });
    let table_shadow = std::mem::take(&mut state.table_shadow);
    let [local_root] = fragments.fragment_ids_for_box(root_box) else {
        return Ok(RetainedRootFormatting::Unsupported);
    };
    let Some(local_root_fragment) = fragments.get(*local_root) else {
        return Ok(RetainedRootFormatting::Unsupported);
    };
    let local_rect = local_root_fragment.physical_rect();
    let retained_rect = previous_root_fragment.physical_rect();
    if !same_retained_root_size(local_rect, retained_rect) {
        return Ok(RetainedRootFormatting::PromoteParent);
    }
    fragments.translate_subtree(
        *local_root,
        PhysicalOffset {
            x: retained_rect.x - local_rect.x,
            y: retained_rect.y - local_rect.y,
        },
    );
    drop(state);

    Ok(RetainedRootFormatting::Formatted(Box::new(
        LiveryLayout::new(
            LayoutResult::new(boxes.into_tree(), fragments),
            Some(text_frame),
            BlockAlgorithmCounts {
                buckram: buckram_blocks,
                taffy: taffy_blocks,
                backend_sizing: backend_sizing_blocks,
            },
            table_paint,
            table_shadow,
        ),
    )))
}

fn supports_retained_root_formatting<Id>(boxes: &GeneratedBoxTree<Id>, root: BoxId) -> bool
where
    Id: Copy + Eq + Hash,
{
    fn visit<Id>(boxes: &GeneratedBoxTree<Id>, box_id: BoxId) -> bool
    where
        Id: Copy + Eq + Hash,
    {
        let css_box = &boxes[box_id];
        if !matches!(
            css_box.origin,
            BoxOrigin::Element(_) | BoxOrigin::Text(_) | BoxOrigin::Anonymous { .. }
        ) {
            return false;
        }
        if css_box.positioning != PositioningScheme::Static {
            return false;
        }
        if css_box.float != FloatSide::None {
            return false;
        }
        if css_box.display.outside == Some(DisplayOutside::Inline)
            && !matches!(css_box.origin, BoxOrigin::Text(_))
        {
            return false;
        }
        if css_box.display.internal_table.is_some() {
            return false;
        }
        css_box
            .children()
            .iter()
            .copied()
            .all(|child| visit(boxes, child))
    }

    visit(boxes, root)
}

fn retained_root_box<Id>(boxes: &buckram::CssBoxTree<Id>, node: Id) -> Option<BoxId>
where
    Id: Copy + Eq + Hash,
{
    let principal = boxes.principal_box(node)?;
    if boxes[principal].display.internal_table == Some(InternalTableRole::Grid) {
        let wrapper = boxes[principal].parent()?;
        (boxes[wrapper].display.internal_table == Some(InternalTableRole::Wrapper))
            .then_some(wrapper)
    } else if matches!(
        boxes[principal].formatting_context,
        Some(
            FormattingContextKind::Block
                | FormattingContextKind::Flex
                | FormattingContextKind::Grid
        )
    ) {
        Some(principal)
    } else {
        None
    }
}

/// A table row, group, or cell mutation is owned by the element whose grid is
/// wrapped into the formatting root. The damaged part cannot be spliced on its
/// own because the table paint plane and wrapper width belong to that owner.
pub(crate) fn retained_table_owner<Id>(boxes: &buckram::CssBoxTree<Id>, node: Id) -> Option<Id>
where
    Id: Copy + Eq + Hash,
{
    for source in boxes.boxes_for_node(node) {
        let mut current = Some(*source);
        while let Some(box_id) = current {
            if boxes[box_id].display.internal_table == Some(InternalTableRole::Grid)
                && let BoxOrigin::Element(owner) = boxes[box_id].origin
            {
                return Some(owner);
            }
            current = boxes[box_id].parent();
        }
    }
    None
}

fn box_is_descendant_of<Id>(boxes: &buckram::CssBoxTree<Id>, box_id: BoxId, root: BoxId) -> bool
where
    Id: Copy + Eq + Hash,
{
    let mut current = Some(box_id);
    while let Some(box_id) = current {
        if box_id == root {
            return true;
        }
        current = boxes[box_id].parent();
    }
    false
}

fn supports_retained_table_root_formatting<Id>(boxes: &GeneratedBoxTree<Id>, root: BoxId) -> bool
where
    Id: Copy + Eq + Hash,
{
    fn visit<Id>(boxes: &GeneratedBoxTree<Id>, box_id: BoxId, root: BoxId) -> bool
    where
        Id: Copy + Eq + Hash,
    {
        let css_box = &boxes[box_id];
        if !matches!(
            css_box.origin,
            BoxOrigin::Element(_) | BoxOrigin::Text(_) | BoxOrigin::Anonymous { .. }
        ) || css_box.positioning != PositioningScheme::Static
            || css_box.float != FloatSide::None
            || (css_box.display.outside == Some(DisplayOutside::Inline)
                && !matches!(css_box.origin, BoxOrigin::Text(_)))
        {
            return false;
        }
        if box_id != root && css_box.display.internal_table == Some(InternalTableRole::Wrapper) {
            return false;
        }
        css_box
            .children()
            .iter()
            .copied()
            .all(|child| visit(boxes, child, root))
    }

    boxes[root].display.internal_table == Some(InternalTableRole::Wrapper)
        && visit(boxes, root, root)
}

fn retained_ancestor_styles_unchanged<Id>(
    boxes: &GeneratedBoxTree<Id>,
    styles: &StylePlane<Id>,
    previous_styles: &StylePlane<Id>,
    root: BoxId,
) -> bool
where
    Id: Copy + Eq + Hash,
{
    let mut current = boxes[root].parent();
    while let Some(box_id) = current {
        if let Some(node) = boxes.origin_node(box_id)
            && styles.get(node) != previous_styles.get(node)
        {
            return false;
        }
        current = boxes[box_id].parent();
    }
    true
}

fn inherited_font_size<Id>(
    boxes: &GeneratedBoxTree<Id>,
    styles: &StylePlane<Id>,
    box_id: BoxId,
) -> f32
where
    Id: Copy + Eq + Hash,
{
    let mut ancestors = Vec::new();
    let mut current = boxes[box_id].parent();
    while let Some(parent) = current {
        ancestors.push(parent);
        current = boxes[parent].parent();
    }
    ancestors.reverse();
    ancestors.into_iter().fold(16.0, |font_size, ancestor| {
        boxes
            .origin_node(ancestor)
            .and_then(|node| styles.get(node))
            .map_or(font_size, |style| font_size_px(&style.font_size, font_size))
    })
}

fn same_retained_root_size(left: PhysicalRect, right: PhysicalRect) -> bool {
    (left.width - right.width).abs() <= 0.01 && (left.height - right.height).abs() <= 0.01
}

/// Resolve deferred container-relative units from the nearest eligible
/// ancestor content boxes. A fallback pass supplies small-viewport values so
/// Taffy can establish those boxes without consuming unresolved units.
pub fn resolve_container_relative_styles<D>(
    dom: &D,
    styles: &StylePlane<D::NodeId>,
    viewport: ViewportSizes,
) -> Result<StylePlane<D::NodeId>, LayoutError>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    resolve_container_relative_styles_with_images(dom, styles, viewport, &ImageSources::new())
}

/// Iterate size-query cascade and container-unit resolution until the style
/// plane stabilizes. The pass is bounded so cyclic queries cannot hang a
/// frame; the final bounded state is laid out normally.
pub fn resolve_container_query_styles<D>(
    dom: &D,
    styles: &StylePlane<D::NodeId>,
    style_set: &StyleSet,
    device: &Device,
    interactions: &InteractionStates<D::NodeId>,
) -> Result<StylePlane<D::NodeId>, LayoutError>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    resolve_container_query_styles_with_images(
        dom,
        styles,
        style_set,
        device,
        interactions,
        &ImageSources::new(),
    )
}

/// Resolve container-query styles while retaining the caller-owned image
/// ledger for intrinsic-size resolution.
pub fn resolve_container_query_styles_with_images<D>(
    dom: &D,
    styles: &StylePlane<D::NodeId>,
    style_set: &StyleSet,
    device: &Device,
    interactions: &InteractionStates<D::NodeId>,
    image_sources: &ImageSources,
) -> Result<StylePlane<D::NodeId>, LayoutError>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    if !style_set.has_container_queries() {
        return resolve_container_relative_styles_with_images(
            dom,
            styles,
            device.viewport_sizes,
            image_sources,
        );
    }
    let mut current = styles.clone();
    for _ in 0..8 {
        let resolved = resolve_container_relative_styles_with_images(
            dom,
            &current,
            device.viewport_sizes,
            image_sources,
        )?;
        let fragments = layout_impl(
            dom,
            &resolved,
            device.viewport_width,
            device.viewport_height,
            image_sources,
        )?;
        let containers = container_snapshots(dom, &resolved, &fragments);
        let next =
            resolve_styles_with_containers(dom, style_set, device, interactions, &containers);
        if next == current {
            return Ok(resolved);
        }
        current = next;
    }
    resolve_container_relative_styles_with_images(
        dom,
        &current,
        device.viewport_sizes,
        image_sources,
    )
}

fn container_snapshots<D>(
    dom: &D,
    styles: &StylePlane<D::NodeId>,
    fragments: &LiveryLayout<D::NodeId>,
) -> HashMap<D::NodeId, Vec<ContainerSnapshot>>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let mut snapshots = HashMap::new();
    collect_container_snapshots(dom, dom.document(), styles, fragments, &[], &mut snapshots);
    snapshots
}

fn collect_container_snapshots<D>(
    dom: &D,
    id: D::NodeId,
    styles: &StylePlane<D::NodeId>,
    fragments: &LiveryLayout<D::NodeId>,
    ancestors: &[ContainerSnapshot],
    snapshots: &mut HashMap<D::NodeId, Vec<ContainerSnapshot>>,
) where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let mut descendants = ancestors.to_vec();
    if dom.kind(id) == NodeKind::Element {
        snapshots.insert(id, ancestors.to_vec());
        if let (Some(style), Some(fragment)) = (styles.get(id), fragments.get(id))
            && style.container_type != ContainerType::Normal
        {
            let (width, height) = content_box_size(style, fragment);
            let (inline_size, block_size) = if style.writing_mode.is_vertical() {
                (height, width)
            } else {
                (width, height)
            };
            descendants.insert(
                0,
                ContainerSnapshot {
                    names: style.container_name.names().to_vec(),
                    container_type: style.container_type,
                    writing_mode: style.writing_mode,
                    width,
                    height,
                    inline_size,
                    block_size,
                },
            );
        }
    }
    for child in dom.dom_children(id) {
        collect_container_snapshots(dom, child, styles, fragments, &descendants, snapshots);
    }
}

fn resolve_container_relative_styles_with_images<D>(
    dom: &D,
    styles: &StylePlane<D::NodeId>,
    viewport: ViewportSizes,
    image_sources: &ImageSources,
) -> Result<StylePlane<D::NodeId>, LayoutError>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let mut fallback = styles.clone();
    resolve_relative_subtree(
        dom,
        dom.document(),
        &mut fallback,
        RelativeLengthEnvironment::container_fallback(viewport),
    );
    if fallback == *styles {
        return Ok(styles.clone());
    }
    let fragments = layout_impl(
        dom,
        &fallback,
        viewport.dynamic.width,
        viewport.dynamic.height,
        image_sources,
    )?;

    let mut resolved = styles.clone();
    resolve_container_subtree(
        dom,
        dom.document(),
        &mut resolved,
        &fragments,
        viewport,
        ContainerBases::default(),
    );
    Ok(resolved)
}

fn resolve_relative_subtree<D>(
    dom: &D,
    id: D::NodeId,
    styles: &mut StylePlane<D::NodeId>,
    environment: RelativeLengthEnvironment,
) where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let environment = environment.with_vertical_writing(
        styles
            .get(id)
            .is_some_and(|style| style.writing_mode.is_vertical()),
    );
    styles.resolve_relative_lengths(id, environment);
    for child in dom.dom_children(id) {
        resolve_relative_subtree(dom, child, styles, environment);
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_container_subtree<D>(
    dom: &D,
    id: D::NodeId,
    styles: &mut StylePlane<D::NodeId>,
    fragments: &LiveryLayout<D::NodeId>,
    viewport: ViewportSizes,
    bases: ContainerBases,
) where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let vertical_writing = styles
        .get(id)
        .is_some_and(|style| style.writing_mode.is_vertical());
    styles.resolve_relative_lengths(
        id,
        RelativeLengthEnvironment::container_axes(
            viewport,
            bases.width,
            bases.height,
            bases.inline,
            bases.block,
            vertical_writing,
        ),
    );

    let mut next = bases;
    if let (Some(style), Some(fragment)) = (styles.get(id), fragments.get(id)) {
        let (width, height) = content_box_size(style, fragment);
        let vertical = style.writing_mode.is_vertical();
        let (inline_size, block_size) = if vertical {
            (height, width)
        } else {
            (width, height)
        };
        match style.container_type {
            ContainerType::Normal => {},
            ContainerType::InlineSize => {
                next.inline = Some(inline_size);
                if vertical {
                    next.height = Some(height);
                } else {
                    next.width = Some(width);
                }
            },
            ContainerType::Size => {
                next.width = Some(width);
                next.height = Some(height);
                next.inline = Some(inline_size);
                next.block = Some(block_size);
            },
        }
    }

    for child in dom.dom_children(id) {
        resolve_container_subtree(dom, child, styles, fragments, viewport, next);
    }
}

/// Return a fragment's physical content-box size after its computed padding
/// and borders are removed.
pub fn content_box_size(style: &ComputedValues, fragment: &TreeFragment) -> (f32, f32) {
    let em = match style.font_size {
        FontSize::Value(CssLengthPercentage::Length(Length {
            value,
            unit: livery::values::LengthUnit::Px,
        })) => value,
        _ => 16.0,
    };
    let padding_left = length_percentage_px(style.padding_left.0, em, fragment.width);
    let padding_right = length_percentage_px(style.padding_right.0, em, fragment.width);
    let padding_top = length_percentage_px(style.padding_top.0, em, fragment.width);
    let padding_bottom = length_percentage_px(style.padding_bottom.0, em, fragment.width);
    let border_left = border_width_px(style.border_left_style, style.border_left_width, em);
    let border_right = border_width_px(style.border_right_style, style.border_right_width, em);
    let border_top = border_width_px(style.border_top_style, style.border_top_width, em);
    let border_bottom = border_width_px(style.border_bottom_style, style.border_bottom_width, em);
    (
        (fragment.width - padding_left - padding_right - border_left - border_right).max(0.0),
        (fragment.height - padding_top - padding_bottom - border_top - border_bottom).max(0.0),
    )
}

fn layout_impl<D>(
    dom: &D,
    styles: &StylePlane<D::NodeId>,
    viewport_width: f32,
    viewport_height: f32,
    image_sources: &ImageSources,
) -> Result<LiveryLayout<D::NodeId>, LayoutError>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let boxes = GeneratedBoxTree::from_dom(dom, styles);
    let mut state = BuildState {
        dom,
        styles,
        boxes: &boxes,
        tree: {
            let mut tree = AlgorithmTree::new();
            tree.set_calc_resolver(resolve_taffy_calc);
            tree
        },
        image_sources,
        text: None,
        table_shadow: TableShadowLedger::default(),
        pending_tables: Vec::new(),
    };
    let children = boxes
        .roots()
        .iter()
        .filter_map(|box_id| {
            state
                .build_box(
                    *box_id,
                    None,
                    16.0,
                    (Some(viewport_width), Some(viewport_height)),
                )
                .transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;
    // This synthetic box is the initial containing block, not an ordinary
    // auto-height document box. Its definite viewport dimensions are the
    // percentage basis for the root element and its definite-height chain.
    let initial_containing_block = BlockStyle {
        size: BlockDimensions::new(
            BlockSizeValue::Length(FlowLength::px(viewport_width)),
            BlockSizeValue::Length(FlowLength::px(viewport_height)),
        ),
        ..BlockStyle::default()
    };
    let root = state.tree.new_with_children_and_block_style(
        AlgorithmKind::Block,
        initial_containing_block,
        Style {
            display: Display::Block,
            size: Size {
                width: Dimension::length(viewport_width),
                height: Dimension::length(viewport_height),
            },
            ..Style::default()
        },
        &children,
        None,
    );

    state.apply_buckram_table_layout();
    state.tree.compute_layout_with_measure(
        root,
        AlgorithmSize::new(
            AlgorithmAvailableSpace::Definite(viewport_width),
            AlgorithmAvailableSpace::Definite(viewport_height),
        ),
        |known, available, _, context, _| measure_text_algorithm_node(known, available, context),
    );
    let (buckram_blocks, taffy_blocks) = state.tree.block_algorithm_counts();
    let backend_sizing_blocks = state
        .tree
        .block_deferral_count(BlockDeferral::BackendSizingMode);
    let table_paint = state.table_paint_plane();
    let tables = table_paint.fragments();
    let mut fragments = FragmentTree::default();
    let mut output = FragmentOutput {
        fragments: &mut fragments,
    };
    collect_fragments(
        &state.tree,
        &boxes,
        root,
        FragmentCursor {
            origin: Point { x: 0.0, y: 0.0 },
            containing: Fragment {
                x: 0.0,
                y: 0.0,
                width: viewport_width,
                height: viewport_height,
            },
            parent: None,
        },
        &tables,
        &mut output,
    )?;
    state.collect_out_of_flow_table_parts(&mut fragments, &tables)?;
    let positioned = state
        .tree
        .node_ids()
        .filter_map(|node| state.tree.source(node).map(|box_id| (box_id, node)))
        .filter(|(box_id, _)| {
            matches!(
                boxes[*box_id].positioning,
                PositioningScheme::Absolute | PositioningScheme::Fixed
            ) && boxes[*box_id].display.internal_table.is_none()
        })
        .collect::<Vec<_>>();
    let positioned_intrinsics = positioned_intrinsic_sizes(
        &mut state.tree,
        &positioned,
        |known, available, _, context, _| measure_text_algorithm_node(known, available, context),
    );
    let placements = positioned_placements(
        &fragments,
        &boxes,
        styles,
        dom,
        image_sources,
        &positioned_intrinsics,
        viewport_width,
        viewport_height,
    );
    if apply_admitted_positioned_inline_sizes(
        &mut state.tree,
        &positioned,
        &placements,
        &positioned_intrinsics,
    ) {
        state.tree.compute_layout_with_measure(
            root,
            AlgorithmSize::new(
                AlgorithmAvailableSpace::Definite(viewport_width),
                AlgorithmAvailableSpace::Definite(viewport_height),
            ),
            |known, available, _, context, _| {
                measure_text_algorithm_node(known, available, context)
            },
        );
        fragments = FragmentTree::default();
        let mut output = FragmentOutput {
            fragments: &mut fragments,
        };
        collect_fragments(
            &state.tree,
            &boxes,
            root,
            FragmentCursor {
                origin: Point { x: 0.0, y: 0.0 },
                containing: Fragment {
                    x: 0.0,
                    y: 0.0,
                    width: viewport_width,
                    height: viewport_height,
                },
                parent: None,
            },
            &tables,
            &mut output,
        )?;
        state.collect_out_of_flow_table_parts(&mut fragments, &tables)?;
    }
    state.verify_table_layout(|box_id| {
        fragments
            .fragments_for_box(box_id)
            .next()
            .map(|fragment| Fragment {
                x: fragment.x,
                y: fragment.y,
                width: fragment.width,
                height: fragment.height,
            })
    });
    let table_shadow = std::mem::take(&mut state.table_shadow);
    drop(state);
    apply_relative_positioning(
        &mut fragments,
        &boxes,
        styles,
        dom,
        None,
        PhysicalSize {
            width: viewport_width,
            height: viewport_height,
        },
    );
    apply_absolute_and_fixed_positioning(
        &mut fragments,
        &boxes,
        styles,
        dom,
        None,
        image_sources,
        &positioned_intrinsics,
        viewport_width,
        viewport_height,
    );
    Ok(LiveryLayout::new(
        LayoutResult::new(boxes.into_tree(), fragments),
        None,
        BlockAlgorithmCounts {
            buckram: buckram_blocks,
            taffy: taffy_blocks,
            backend_sizing: backend_sizing_blocks,
        },
        table_paint,
        table_shadow,
    ))
}

fn layout_atomic_subtrees<D>(
    dom: &D,
    styles: &StylePlane<D::NodeId>,
    boxes: &GeneratedBoxTree<D::NodeId>,
    viewport_width: f32,
    viewport_height: f32,
    text: &mut TextSystem,
    image_sources: &ImageSources,
) -> Result<AtomicLayoutPlane, LayoutError>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let roots = boxes
        .iter()
        .filter_map(|(box_id, css_box)| {
            let BoxOrigin::Element(node) = css_box.origin else {
                return None;
            };
            if boxes.principal_box(node) != Some(box_id)
                || css_box.display.outside != Some(DisplayOutside::Inline)
                || !is_atomic_inline_box(dom, styles, node)
            {
                return None;
            }
            if has_atomic_inline_ancestor(dom, styles, boxes, node) {
                return None;
            }
            // K4e4: an inline-table's principal box is the grid, but its atom
            // is the wrapper above it - the box that carries the element's
            // margins and contains its captions.
            if css_box.display.internal_table == Some(InternalTableRole::Grid)
                && let Some(wrapper) = css_box.parent().filter(|parent| {
                    boxes[*parent].display.internal_table == Some(InternalTableRole::Wrapper)
                })
            {
                return Some(wrapper);
            }
            Some(box_id)
        })
        .collect::<Vec<_>>();
    let mut plane = AtomicLayoutPlane::default();

    for box_id in roots {
        let mut state = BuildState {
            dom,
            styles,
            boxes,
            tree: {
                let mut tree = AlgorithmTree::new();
                tree.set_calc_resolver(resolve_taffy_calc);
                tree
            },
            image_sources,
            text: Some(&mut *text),
            table_shadow: TableShadowLedger::default(),
            pending_tables: Vec::new(),
        };
        let built = state.build_box(
            box_id,
            None,
            16.0,
            (Some(viewport_width), Some(viewport_height)),
        )?;
        // Harvest before any continue below: the shadow already ran inside
        // build_box, and both skip paths would otherwise drop its ledger.
        plane
            .table_shadow
            .merge(std::mem::take(&mut state.table_shadow));
        let Some(atomic_root) = built else {
            // No layout will run for a root that built nothing, but noted
            // tables still record their deferrals.
            state.apply_buckram_table_layout();
            plane
                .table_shadow
                .merge(std::mem::take(&mut state.table_shadow));
            continue;
        };
        // An inline replaced root contributes its natural box to the line.
        // Formatting it against the viewport first turns an auto canvas into
        // a viewport-wide atomic fragment, and that stale rectangle is then
        // also reused by flex-basis: content's max-content query.
        let replaced_atomic_root = matches!(
            boxes[box_id].origin,
            BoxOrigin::Element(node) if is_replaced_element(dom, node)
        );
        // An admitted atomic inline root needs a containing block so its
        // shrink-to-fit query runs as a child formatting context. Keep the
        // established direct-root path for the deferred cases, whose inline
        // placement may depend on unsupported vertical alignment behavior.
        //
        // A replaced root is excluded. CSS 2.1 10.3.2 gives an inline replaced
        // element with `width: auto` its intrinsic width outright; there is no
        // shrink-to-fit step to run, so it needs no containing block to run one
        // in. Wrapping it was actively harmful: the wrapper is viewport-sized,
        // the very next statement formats it under MaxContent, and Buckram's
        // block algorithm then bails with an indefinite inline size and hands
        // the subtree to Taffy's generic block path, which stretches the leaf
        // to the wrapper's width and derives its height from the natural ratio.
        // A `display: inline-block` image therefore painted at viewport width
        // times its ratio while `display: inline` on the same bytes was correct.
        let root = if state.tree.uses_intrinsic_shrink_to_fit(atomic_root) && !replaced_atomic_root
        {
            state.tree.new_with_children_and_block_style(
                AlgorithmKind::Block,
                BlockStyle {
                    size: BlockDimensions::new(
                        BlockSizeValue::Length(FlowLength::px(viewport_width)),
                        BlockSizeValue::Length(FlowLength::px(viewport_height)),
                    ),
                    ..BlockStyle::default()
                },
                Style {
                    display: Display::Block,
                    size: Size {
                        width: Dimension::length(viewport_width),
                        height: Dimension::length(viewport_height),
                    },
                    ..Style::default()
                },
                &[atomic_root],
                None,
            )
        } else {
            atomic_root
        };
        state.apply_buckram_table_layout();
        if let Some(intrinsic) = state.pending_tables.iter().find_map(|pending| {
            (pending.grid.wrapper == Some(box_id))
                .then_some(pending.assigned.as_ref()?.intrinsic_sizes)
        }) {
            plane.intrinsic_inline.insert(box_id, intrinsic);
        }
        let available = if replaced_atomic_root {
            AlgorithmSize::new(
                AlgorithmAvailableSpace::MaxContent,
                AlgorithmAvailableSpace::MaxContent,
            )
        } else {
            AlgorithmSize::new(
                AlgorithmAvailableSpace::Definite(viewport_width),
                AlgorithmAvailableSpace::Definite(viewport_height),
            )
        };
        state.tree.compute_layout_with_measure(
            root,
            available,
            |known, available, _, context, _| {
                let Some(context) = context else {
                    return AlgorithmSize::new(0.0, 0.0);
                };
                let available_width = match available.width {
                    AlgorithmAvailableSpace::Definite(width) => width,
                    AlgorithmAvailableSpace::MinContent => context.min_width,
                    AlgorithmAvailableSpace::MaxContent => context.max_width,
                };
                AlgorithmSize::new(
                    known
                        .width
                        .unwrap_or(context.max_width.min(available_width.max(0.0))),
                    known.height.unwrap_or(context.height),
                )
            },
        );

        let table_paint = state.table_paint_plane();
        let tables = table_paint.fragments();
        let mut fragments = Vec::new();
        collect_atomic_fragments(&state.tree, root, Point { x: 0.0, y: 0.0 }, &mut fragments);
        let Some(root_rect) = fragments
            .iter()
            .find_map(|candidate| (candidate.box_id == box_id).then_some(candidate.fragment))
        else {
            // Widths are stable across the origin shift below, so verification
            // can read them either side of it. It consumes the pending list,
            // which does not matter for a root that supplied no fragment.
            state.verify_table_layout(|needle| {
                fragments
                    .iter()
                    .find(|candidate| candidate.box_id == needle)
                    .map(|candidate| candidate.fragment)
            });
            plane
                .table_shadow
                .merge(std::mem::take(&mut state.table_shadow));
            continue;
        };
        // The wrapper can have captions before the grid, so derive the
        // baseline from the actual grid origin rather than assuming that the
        // grid begins at the atomic root. Buckram's K4d5 baseline remains
        // grid-relative; text layout receives the wrapper-relative value.
        for pending in &state.pending_tables {
            let Some(wrapper) = pending.grid.wrapper else {
                continue;
            };
            let Some(grid_rect) = fragments.iter().find_map(|candidate| {
                (candidate.box_id == pending.grid.grid).then_some(candidate.fragment)
            }) else {
                continue;
            };
            let Some(first) = state.tree.baselines(pending.table_node).first else {
                continue;
            };
            let baseline = grid_rect.y - root_rect.y + first;
            if baseline.is_finite() && baseline >= 0.0 {
                plane.inline_baselines.insert(wrapper, baseline);
            }
        }
        // Verify after the baseline handoff: verification consumes the pending
        // list, while the handoff needs the same grid node and K4d5 output.
        state.verify_table_layout(|needle| {
            fragments
                .iter()
                .find(|candidate| candidate.box_id == needle)
                .map(|candidate| candidate.fragment)
        });
        plane
            .table_shadow
            .merge(std::mem::take(&mut state.table_shadow));
        plane.table_paint.merge(table_paint);
        for candidate in &mut fragments {
            candidate.fragment.x -= root_rect.x;
            candidate.fragment.y -= root_rect.y;
            plane.fragments.insert(candidate.box_id, candidate.fragment);
        }
        plane.subtrees.push(AtomicSubtree {
            root: box_id,
            fragments,
            tables,
        });
    }
    Ok(plane)
}

fn collect_atomic_fragments(
    tree: &AlgorithmTree<Style, TextMeasure, Option<BoxId>>,
    node: AlgorithmNodeId,
    parent_origin: Point<f32>,
    output: &mut Vec<AtomicFragment>,
) {
    let computed = tree.unrounded_layout(node);
    let static_computed = tree.static_layout(node);
    let origin = Point {
        x: parent_origin.x + computed.x,
        y: parent_origin.y + computed.y,
    };
    if let Some(box_id) = *tree.source(node) {
        output.push(AtomicFragment {
            box_id,
            fragment: Fragment {
                x: origin.x,
                y: origin.y,
                width: computed.width,
                height: computed.height,
            },
            // Unlike the final fragment above, a static rectangle is local to
            // the formatting-context parent that emitted it. Preserve that
            // backend record separately so the atomic-inline handoff does not
            // relabel a completed absolute child location as its K5b input.
            static_fragment: Fragment {
                x: static_computed.x,
                y: static_computed.y,
                width: static_computed.width,
                height: static_computed.height,
            },
            containing_block_area: tree.grid_positioned_area(node),
        });
    }
    for child in tree.children(node) {
        collect_atomic_fragments(tree, *child, origin, output);
    }
}

fn merge_atomic_subtrees<Id>(
    atomic: &AtomicLayoutPlane,
    boxes: &GeneratedBoxTree<Id>,
    fragments: &mut FragmentTree,
) where
    Id: Copy + Eq + Hash,
{
    for subtree in &atomic.subtrees {
        let Some(root_id) = fragments
            .fragment_ids_for_box(subtree.root)
            .first()
            .copied()
        else {
            continue;
        };
        let Some(root_fragment) = fragments.get(root_id) else {
            continue;
        };
        let final_root = root_fragment.physical_rect();
        let local_root = subtree
            .fragments
            .iter()
            .find_map(|candidate| (candidate.box_id == subtree.root).then_some(candidate.fragment))
            .unwrap_or_default();
        let offset = (final_root.x - local_root.x, final_root.y - local_root.y);

        // First materialize the atom's wrapper, captions, and table grids.
        // A table-internal child waits for `commit_table_structure` below so
        // its normal content can later attach to the emitted structural cell
        // rather than to an accidental root fallback.
        let grids = subtree.tables.keys().copied().collect::<HashSet<_>>();
        for atomic_fragment in &subtree.fragments {
            let box_id = atomic_fragment.box_id;
            let mut parent = boxes[box_id].parent();
            let mut inside_grid = false;
            while let Some(ancestor) = parent {
                if grids.contains(&ancestor) {
                    inside_grid = true;
                    break;
                }
                parent = boxes[ancestor].parent();
            }
            if inside_grid && !grids.contains(&box_id) {
                continue;
            }
            append_atomic_fragment(
                boxes,
                fragments,
                subtree.root,
                root_id,
                offset,
                *atomic_fragment,
            );
        }

        // Atomic inline roots bypass the ordinary fragment collector, so
        // commit the same Buckram-owned structural subtree here once the
        // atomic boxes have their final page-relative origin.
        for (grid, emitted) in &subtree.tables {
            let Some(grid_id) = fragments.fragment_ids_for_box(*grid).first().copied() else {
                continue;
            };
            let Some(grid_fragment) = fragments.get(grid_id) else {
                continue;
            };
            let origin = Point {
                x: grid_fragment.x,
                y: grid_fragment.y,
            };
            let mut output = FragmentOutput { fragments };
            commit_table_structure(emitted, origin, grid_id, boxes, &mut output);
        }

        // The second pass fills in ordinary descendants. Grid and cell boxes
        // already exist, so their text and replaced content inherit the
        // structural parent just committed above.
        for atomic_fragment in &subtree.fragments {
            append_atomic_fragment(
                boxes,
                fragments,
                subtree.root,
                root_id,
                offset,
                *atomic_fragment,
            );
        }
    }
}

fn append_atomic_fragment<Id>(
    boxes: &GeneratedBoxTree<Id>,
    fragments: &mut FragmentTree,
    root_box: BoxId,
    root_id: FragmentId,
    offset: (f32, f32),
    atomic_fragment: AtomicFragment,
) where
    Id: Copy + Eq + Hash,
{
    let box_id = atomic_fragment.box_id;
    if box_id == root_box {
        return;
    }
    let existing = fragments.fragment_ids_for_box(box_id).first().copied();
    let rect = Fragment {
        x: atomic_fragment.fragment.x + offset.0,
        y: atomic_fragment.fragment.y + offset.1,
        width: atomic_fragment.fragment.width,
        height: atomic_fragment.fragment.height,
    };
    let parent = boxes[box_id]
        .parent()
        .and_then(|parent_box| fragments.fragment_ids_for_box(parent_box).last().copied())
        .or(Some(root_id));
    let output = FragmentOutput { fragments };
    let static_position = static_position_record(
        boxes,
        box_id,
        parent,
        LogicalRect::from_horizontal_physical(atomic_fragment.static_fragment),
        atomic_fragment.containing_block_area,
        output.fragments,
    );
    if let Some(existing) = existing {
        output.fragments.reconcile_parent(existing, parent);
        if let Some(position) = static_position {
            // The outer inline tree keeps a duplicate positioned node for
            // intrinsic sizing, but its source rectangle is only provisional.
            // The atomic block formatter owns the descendant's real K5b
            // coordinate space and reconciles that record at the handoff.
            output.fragments.reconcile_static_position(position);
        }
        return;
    }
    if let Some(position) = static_position {
        output.fragments.record_static_position(position);
    }
    output.fragments.push(
        TreeFragment::from_horizontal_physical(box_id, rect)
            .with_baselines(Baselines::synthesized_from_block_end(rect.height)),
        parent,
        parent,
    );
}

fn layout_inline_groups<D>(
    dom: &D,
    styles: &StylePlane<D::NodeId>,
    boxes: GeneratedBoxTree<D::NodeId>,
    viewport: (f32, f32),
    text: &mut TextSystem,
    atomic: &AtomicLayoutPlane,
    image_sources: &ImageSources,
) -> Result<LiveryLayout<D::NodeId>, LayoutError>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let (viewport_width, viewport_height) = viewport;
    let mut state = InlineBuildState {
        dom,
        styles,
        boxes: &boxes,
        atomic,
        tree: {
            let mut tree = AlgorithmTree::new();
            tree.set_calc_resolver(resolve_taffy_calc);
            tree
        },
        image_sources,
        table_shadow: TableShadowLedger::default(),
        pending_tables: Vec::new(),
        pending_table_handoff: None,
    };
    let children = boxes
        .roots()
        .iter()
        .filter_map(|box_id| {
            state
                .build_box(
                    *box_id,
                    None,
                    16.0,
                    (Some(viewport_width), Some(viewport_height)),
                )
                .transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;
    let root = state.tree.new_with_children_and_block_style(
        AlgorithmKind::Block,
        BlockStyle {
            size: BlockDimensions::new(
                BlockSizeValue::Length(FlowLength::px(viewport_width)),
                BlockSizeValue::Length(FlowLength::px(viewport_height)),
            ),
            ..BlockStyle::default()
        },
        Style {
            display: Display::Block,
            size: Size {
                width: Dimension::length(viewport_width),
                height: Dimension::length(viewport_height),
            },
            ..Style::default()
        },
        &children,
        Vec::new(),
    );

    state.apply_buckram_table_layout(text);
    let mut intrinsic_sizes = IntrinsicSizeCache::default();
    state.tree.compute_layout_with_measure(
        root,
        AlgorithmSize::new(
            AlgorithmAvailableSpace::Definite(viewport_width),
            AlgorithmAvailableSpace::Definite(viewport_height),
        ),
        |known, available, _, context, line_constraints| {
            measure_inline_algorithm_node(
                text,
                dom,
                styles,
                &boxes,
                atomic,
                &mut intrinsic_sizes,
                known,
                available,
                context,
                line_constraints,
            )
        },
    );
    populate_inline_baselines(&mut state.tree);
    let (buckram_blocks, taffy_blocks) = state.tree.block_algorithm_counts();
    let backend_sizing_blocks = state
        .tree
        .block_deferral_count(BlockDeferral::BackendSizingMode);
    let mut table_paint = state.table_paint_plane();
    let tables = table_paint.fragments();
    let mut text_frame = TextFrame::default();
    let mut fragments = FragmentTree::default();
    let mut output = FragmentOutput {
        fragments: &mut fragments,
    };
    collect_inline_fragments(
        &state.tree,
        &boxes,
        root,
        FragmentCursor {
            origin: Point { x: 0.0, y: 0.0 },
            containing: Fragment {
                x: 0.0,
                y: 0.0,
                width: viewport_width,
                height: viewport_height,
            },
            parent: None,
        },
        &tables,
        &mut output,
        &mut text_frame,
        styles,
    )?;
    state.collect_out_of_flow_table_parts(
        text,
        &mut fragments,
        &tables,
        &mut text_frame,
        &mut intrinsic_sizes,
    )?;
    let positioned = state
        .tree
        .node_ids()
        .filter_map(|node| match state.tree.source(node).as_slice() {
            [box_id] => Some((*box_id, node)),
            _ => None,
        })
        .filter(|(box_id, _)| {
            matches!(
                boxes[*box_id].positioning,
                PositioningScheme::Absolute | PositioningScheme::Fixed
            ) && boxes[*box_id].display.internal_table.is_none()
        })
        .collect::<Vec<_>>();
    let positioned_intrinsics = {
        let InlineBuildState {
            tree,
            dom,
            styles,
            boxes,
            atomic,
            ..
        } = &mut state;
        positioned_intrinsic_sizes(
            tree,
            &positioned,
            |known, available, _, context, line_constraints| {
                measure_inline_algorithm_node(
                    text,
                    *dom,
                    *styles,
                    *boxes,
                    atomic,
                    &mut intrinsic_sizes,
                    known,
                    available,
                    context,
                    line_constraints,
                )
            },
        )
    };
    let placements = positioned_placements(
        &fragments,
        &boxes,
        styles,
        dom,
        image_sources,
        &positioned_intrinsics,
        viewport_width,
        viewport_height,
    );
    if apply_admitted_positioned_inline_sizes(
        &mut state.tree,
        &positioned,
        &placements,
        &positioned_intrinsics,
    ) {
        state.tree.compute_layout_with_measure(
            root,
            AlgorithmSize::new(
                AlgorithmAvailableSpace::Definite(viewport_width),
                AlgorithmAvailableSpace::Definite(viewport_height),
            ),
            |known, available, _, context, line_constraints| {
                measure_inline_algorithm_node(
                    text,
                    dom,
                    styles,
                    &boxes,
                    atomic,
                    &mut intrinsic_sizes,
                    known,
                    available,
                    context,
                    line_constraints,
                )
            },
        );
        populate_inline_baselines(&mut state.tree);
        fragments = FragmentTree::default();
        text_frame = TextFrame::default();
        let mut output = FragmentOutput {
            fragments: &mut fragments,
        };
        collect_inline_fragments(
            &state.tree,
            &boxes,
            root,
            FragmentCursor {
                origin: Point { x: 0.0, y: 0.0 },
                containing: Fragment {
                    x: 0.0,
                    y: 0.0,
                    width: viewport_width,
                    height: viewport_height,
                },
                parent: None,
            },
            &tables,
            &mut output,
            &mut text_frame,
            styles,
        )?;
        state.collect_out_of_flow_table_parts(
            text,
            &mut fragments,
            &tables,
            &mut text_frame,
            &mut intrinsic_sizes,
        )?;
    }
    state.verify_table_layout(|box_id| {
        fragments
            .fragments_for_box(box_id)
            .next()
            .map(|fragment| Fragment {
                x: fragment.x,
                y: fragment.y,
                width: fragment.width,
                height: fragment.height,
            })
    });
    // Tables on the inline route record into the state's own ledger; tables
    // inside atomic subtrees accumulated into the plane's. Both survive.
    let mut table_shadow = std::mem::take(&mut state.table_shadow);
    table_shadow.merge(atomic.table_shadow.clone());
    drop(state);
    merge_atomic_subtrees(atomic, &boxes, &mut fragments);
    table_paint.merge(atomic.table_paint.clone());
    apply_relative_positioning(
        &mut fragments,
        &boxes,
        styles,
        dom,
        Some(&mut text_frame),
        PhysicalSize {
            width: viewport_width,
            height: viewport_height,
        },
    );
    apply_absolute_and_fixed_positioning(
        &mut fragments,
        &boxes,
        styles,
        dom,
        Some(&mut text_frame),
        image_sources,
        &positioned_intrinsics,
        viewport_width,
        viewport_height,
    );
    Ok(LiveryLayout::new(
        LayoutResult::new(boxes.into_tree(), fragments),
        Some(text_frame),
        BlockAlgorithmCounts {
            buckram: buckram_blocks,
            taffy: taffy_blocks,
            backend_sizing: backend_sizing_blocks,
        },
        table_paint,
        table_shadow,
    ))
}

impl<D> InlineBuildState<'_, D>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    fn build_box(
        &mut self,
        box_id: BoxId,
        inherited: Option<&ComputedValues>,
        parent_font_size: f32,
        containing_size: (Option<f32>, Option<f32>),
    ) -> Result<Option<AlgorithmNodeId>, LayoutError> {
        match self.boxes[box_id].origin {
            BoxOrigin::Element(node) => {
                let computed = self.styles.get(node).cloned().unwrap_or_default();
                // K4e1: the wrapper above this grid took the properties
                // CSS 2.1 section 17.4 assigns to it; the grid sees them unset.
                let (computed, table_style) =
                    if self.boxes[box_id].display.internal_table == Some(InternalTableRole::Grid) {
                        (grid_style(&computed, containing_size), Some(computed))
                    } else {
                        (computed, None)
                    };
                debug_assert!(
                    self.pending_table_handoff.is_none(),
                    "a table handoff must be consumed by its own build_box call"
                );
                let font_size = font_size_px(&computed.font_size, parent_font_size);
                let child_containing_size =
                    resolved_child_containing_size(&computed, font_size, containing_size);
                let mut inline_container_style = computed.clone();
                if matches!(
                    computed.position,
                    CssPosition::Absolute | CssPosition::Fixed
                ) {
                    // Once positioned, an inline element establishes a block
                    // container; its own vertical-align does not offset the
                    // text inside that container.
                    inline_container_style.vertical_align = VerticalAlign::Baseline;
                }
                let children = self.build_children(
                    box_id,
                    &inline_container_style,
                    font_size,
                    child_containing_size,
                )?;
                let table_handoff = self.pending_table_handoff.take();
                let mut taffy_style = to_taffy_style(&computed, font_size);
                let replaced_size = apply_replaced_intrinsic_style(
                    &mut taffy_style,
                    self.dom,
                    node,
                    &computed,
                    self.image_sources,
                    font_size,
                    matches!(
                        self.boxes[box_id].display.outside,
                        Some(buckram::DisplayOutside::Block)
                    ) && !stretched_by_ancestor_context(self.boxes, box_id),
                    // Percentage padding against an indefinite basis is zero.
                    containing_size.0.unwrap_or(0.0),
                );
                // Taffy exempts a compressible replaced element from block
                // stretch-sizing (CSS 2.1 10.3.4) and from grid `normal`
                // stretching (css-grid-1 6.2). Two conditions narrow it.
                //
                // It is armed only for a box that actually becomes a measured
                // leaf: a `<canvas>` with fallback content is a block container,
                // and arming it there would let Taffy shrink-wrap the fallback
                // instead of laying it out.
                //
                // And only under `content-box`. Arming it for a border-box
                // replaced element changes which path applies CSS 2.1 10.4's
                // ratio-preserving min/max clamp, and box-sizing-replaced-001,
                // -002 and -003 fail when it does. The cost is named: a
                // border-box replaced element still stretches, in a block
                // container and as a grid item alike.
                // Since taffy's block path stopped reading this flag, arming it
                // reaches only the grid `normal` exemption, so border-box leaves
                // are safe to include: a border-box replaced grid item no longer
                // stretches either.
                taffy_style.item_is_replaced = replaced_size.is_some() && children.is_empty();
                let block_style =
                    to_block_style(self.boxes, self.styles, box_id, &computed, font_size);
                let kind = algorithm_kind(&self.boxes[box_id], children.is_empty());
                let dom_node = node;
                let node =
                    if let Some((width, height)) = replaced_size.filter(|_| children.is_empty()) {
                        self.tree.new_leaf_with_context_and_block_style(
                            block_style,
                            taffy_style,
                            InlineMeasure {
                                owner: Some(box_id),
                                roots: vec![box_id],
                                style: computed.clone(),
                                width,
                                height,
                                replaced_size: Some((width, height)),
                                layouts: Vec::new(),
                                placement_constraints: None,
                            },
                            vec![box_id],
                        )
                    } else {
                        self.tree.new_with_children_and_block_style(
                            kind,
                            block_style,
                            taffy_style,
                            &children,
                            vec![box_id],
                        )
                    };
                enable_flex_grid_static_position_provider(
                    &mut self.tree,
                    self.styles,
                    self.boxes,
                    box_id,
                    node,
                );
                if let Some((grid, cell_nodes, out_of_flow_parts)) = table_handoff {
                    self.pending_tables.push(PendingTable {
                        table: box_id,
                        node: Some(dom_node),
                        table_style: table_style.unwrap_or_default(),
                        table_node: node,
                        wrapper: None,
                        captions: Vec::new(),
                        grid,
                        collapsed_borders: None,
                        collapsed_border_metrics: None,
                        cell_nodes,
                        out_of_flow_parts,
                        font_size,
                        containing_width: containing_size.0,
                        containing_height: containing_size.1,
                        assigned: None,
                        block: None,
                    });
                }
                if supports_nested_float_state(&self.boxes[box_id], block_style, kind) {
                    self.tree.enable_nested_float_state(node);
                }
                if block_style.float != FloatSide::None
                    && self.boxes[box_id].float_context == FloatContextProvenance::Inline
                {
                    self.tree.mark_inline_context_float(node);
                }
                if supports_float_avoidance(&self.boxes[box_id], block_style, kind) {
                    self.tree.enable_float_avoidance(node);
                }
                if supports_intrinsic_shrink_to_fit(
                    &self.tree,
                    node,
                    self.boxes,
                    box_id,
                    &computed,
                    block_style,
                    kind,
                ) {
                    self.tree.enable_intrinsic_shrink_to_fit(node);
                }
                Ok(Some(node))
            },
            BoxOrigin::Text(_) => {
                let style = inherited.cloned().unwrap_or_default();
                self.build_inline_group(Some(box_id), &[box_id], &style, parent_font_size)
                    .map(Some)
            },
            BoxOrigin::Pseudo { .. } | BoxOrigin::Anonymous { .. } => {
                if let Some(grid) = (self.boxes[box_id].display.internal_table
                    == Some(InternalTableRole::Wrapper))
                .then(|| wrapped_table_grid(self.boxes, box_id))
                .flatten()
                {
                    // K4e1: the wrapper is the box that participates in flow.
                    // Its children keep the *table's* inherited context, so
                    // they are built against the parent's font size and
                    // containing block, not the wrapper's.
                    let table = match legacy_origin_node(self.boxes, grid) {
                        Some(element) => self.styles.get(element).cloned().unwrap_or_default(),
                        None => anonymous_table_style(inherited),
                    };
                    let computed = wrapper_style(&table);
                    let font_size = font_size_px(&computed.font_size, parent_font_size);
                    let mut caption_nodes = Vec::new();
                    let mut children = Vec::new();
                    for child in wrapper_children_in_caption_order(self.boxes, self.styles, box_id)
                    {
                        let Some(child_node) =
                            self.build_box(child, inherited, parent_font_size, containing_size)?
                        else {
                            continue;
                        };
                        if self.boxes[child].display.internal_table
                            == Some(InternalTableRole::Caption)
                            && matches!(
                                self.boxes[child].positioning,
                                PositioningScheme::Static
                                    | PositioningScheme::Relative
                                    | PositioningScheme::Sticky
                            )
                        {
                            let caption = self
                                .boxes
                                .origin_node(child)
                                .and_then(|node| self.styles.get(node))
                                .cloned()
                                .unwrap_or_default();
                            let em = font_size_px(&caption.font_size, font_size);
                            caption_nodes.push((
                                child_node,
                                caption_horizontal_margins(&caption, em, containing_size.0),
                            ));
                        }
                        children.push(child_node);
                    }
                    let mut taffy_style = to_taffy_style(&computed, font_size);
                    let logical_wrapper =
                        wrapper_uses_logical_block_axis(&mut taffy_style, self.boxes[box_id].flow);
                    if wrapper_needs_float_fallback(self.boxes, box_id, &taffy_style) {
                        taffy_style.float = TaffyFloat::Left;
                    }
                    let wrapper_grid_width = wrapper_width_from_grid(&to_taffy_style(
                        &grid_style(&table, containing_size),
                        font_size,
                    ));
                    if let Some(width) = wrapper_grid_width {
                        taffy_style.size.width = width;
                    }
                    let block_style =
                        to_block_style(self.boxes, self.styles, box_id, &computed, font_size);
                    let kind = if logical_wrapper {
                        AlgorithmKind::Flex
                    } else {
                        algorithm_kind(&self.boxes[box_id], children.is_empty())
                    };
                    let node = self.tree.new_with_children_and_block_style(
                        kind,
                        block_style,
                        taffy_style,
                        &children,
                        vec![box_id],
                    );
                    if let Some(width) = wrapper_grid_width.and_then(Dimension::into_option) {
                        self.tree.set_table_wrapper_inline_size(node, width);
                    }
                    enable_flex_grid_static_position_provider(
                        &mut self.tree,
                        self.styles,
                        self.boxes,
                        box_id,
                        node,
                    );
                    if let Some(pending) = self
                        .pending_tables
                        .iter_mut()
                        .find(|pending| pending.table == grid)
                    {
                        pending.wrapper = Some(node);
                        pending.captions = caption_nodes;
                    }
                    return Ok(Some(node));
                }
                if self.boxes[box_id].display.internal_table == Some(InternalTableRole::Grid) {
                    return self.build_anonymous_table_grid(
                        box_id,
                        inherited,
                        parent_font_size,
                        containing_size,
                    );
                }
                let computed = inherited.cloned().unwrap_or_default();
                let computed = match computed.display {
                    CssDisplay::Table | CssDisplay::InlineTable => ComputedValues {
                        display: CssDisplay::Block,
                        ..computed
                    },
                    _ => computed,
                };
                let children =
                    self.build_children(box_id, &computed, parent_font_size, containing_size)?;
                let block_style = anonymous_block_style(self.boxes, box_id);
                let kind = algorithm_kind(&self.boxes[box_id], children.is_empty());
                let node = self.tree.new_with_children_and_block_style(
                    kind,
                    block_style,
                    anonymous_taffy_style(&self.boxes[box_id]),
                    &children,
                    vec![box_id],
                );
                enable_flex_grid_static_position_provider(
                    &mut self.tree,
                    self.styles,
                    self.boxes,
                    box_id,
                    node,
                );
                if supports_nested_float_state(&self.boxes[box_id], block_style, kind) {
                    self.tree.enable_nested_float_state(node);
                }
                Ok(Some(node))
            },
        }
    }

    /// Measure one cell's border-box intrinsic pair through the inline
    /// measure contract. Width sizing is neutralized for the query, because
    /// Buckram applies those constraints itself, and restored afterwards.
    fn measure_cell_intrinsics(
        &mut self,
        text: &mut TextSystem,
        cell_node: AlgorithmNodeId,
    ) -> Option<IntrinsicSizes> {
        let (dom, styles, boxes, atomic) = (self.dom, self.styles, self.boxes, self.atomic);
        let style = self.tree.style_mut(cell_node);
        let saved = (style.size.width, style.min_size.width, style.max_size.width);
        style.size.width = Dimension::auto();
        style.min_size.width = Dimension::auto();
        style.max_size.width = Dimension::auto();
        let mut measure = |available| {
            self.tree
                .compute_layout_with_measure_excluding_out_of_flow_children(
                    cell_node,
                    AlgorithmSize::new(available, AlgorithmAvailableSpace::MaxContent),
                    |known, available, _, context, _| {
                        let Some(context) = context else {
                            return AlgorithmSize::new(0.0, 0.0);
                        };
                        let (width, intrinsic_kind) = match available.width {
                            AlgorithmAvailableSpace::Definite(width) => (width, None),
                            // A nearly-zero line breaks at every opportunity; an
                            // infinite one suppresses wrapping, as in the main
                            // measure closure.
                            AlgorithmAvailableSpace::MinContent => {
                                (0.01, Some(IntrinsicSizeKind::MinContent))
                            },
                            AlgorithmAvailableSpace::MaxContent => {
                                (f32::INFINITY, Some(IntrinsicSizeKind::MaxContent))
                            },
                        };
                        let (measured_width, measured_height) = measure_inline_context(
                            text,
                            dom,
                            styles,
                            boxes,
                            atomic,
                            context,
                            InlineMeasureGeometry {
                                width: known.width.unwrap_or(width),
                                intrinsic_kind,
                                line_constraints: None,
                            },
                        );
                        AlgorithmSize::new(
                            known.width.unwrap_or(measured_width),
                            known.height.unwrap_or(measured_height),
                        )
                    },
                );
            // A block child with a definite width contains its own sizing
            // contribution even when one of its descendants overflows it.
            // Taffy's intrinsic cell box expands to that overflow here, but
            // CSS table sizing takes the cell's in-flow child boxes instead.
            // Read the direct child border boxes so Buckram receives the
            // cell's actual border-box contribution, not descendant ink.
            self.tree
                .children(cell_node)
                .iter()
                .filter(|child| !self.tree.block_style(**child).is_out_of_flow())
                .map(|child| self.tree.unrounded_layout(*child).width)
                .reduce(f32::max)
                .unwrap_or_else(|| self.tree.unrounded_layout(cell_node).width)
        };
        let min = measure(AlgorithmAvailableSpace::MinContent);
        let max = measure(AlgorithmAvailableSpace::MaxContent);
        let style = self.tree.style_mut(cell_node);
        (style.size.width, style.min_size.width, style.max_size.width) = saved;
        IntrinsicSizes::new(min, max.max(min))
    }

    /// The floor a caption puts under the table's inline size.
    ///
    /// Its own min-content width plus its horizontal margins, which is what
    /// C5 and C6 of the K4e1 interop matrix pin. Unlike a cell measurement
    /// this does *not* neutralize the caption's own `width`: C7 shows a
    /// specified caption width participating like any other box, so a
    /// `width: 300px` caption puts a floor of 300 under the table. Several
    /// captions each put their own floor down and the widest one wins.
    fn measure_caption_min(
        &mut self,
        text: &mut TextSystem,
        captions: &[(AlgorithmNodeId, f32)],
    ) -> Option<f32> {
        let (dom, styles, boxes, atomic) = (self.dom, self.styles, self.boxes, self.atomic);
        captions
            .iter()
            .map(|(caption, margins)| {
                self.tree.compute_layout_with_measure(
                    *caption,
                    AlgorithmSize::new(
                        AlgorithmAvailableSpace::MinContent,
                        AlgorithmAvailableSpace::MaxContent,
                    ),
                    |known, available, _, context, _| {
                        let Some(context) = context else {
                            return AlgorithmSize::new(0.0, 0.0);
                        };
                        let (width, intrinsic_kind) = match available.width {
                            AlgorithmAvailableSpace::Definite(width) => (width, None),
                            AlgorithmAvailableSpace::MinContent => {
                                (0.01, Some(IntrinsicSizeKind::MinContent))
                            },
                            AlgorithmAvailableSpace::MaxContent => {
                                (f32::INFINITY, Some(IntrinsicSizeKind::MaxContent))
                            },
                        };
                        let (measured_width, measured_height) = measure_inline_context(
                            text,
                            dom,
                            styles,
                            boxes,
                            atomic,
                            context,
                            InlineMeasureGeometry {
                                width: known.width.unwrap_or(width),
                                intrinsic_kind,
                                line_constraints: None,
                            },
                        );
                        AlgorithmSize::new(
                            known.width.unwrap_or(measured_width),
                            known.height.unwrap_or(measured_height),
                        )
                    },
                );
                self.tree.layout(*caption).width + margins
            })
            .reduce(f32::max)
            .filter(|minimum| minimum.is_finite() && *minimum >= 0.0)
    }

    /// K4c5b and K4d6b: compute Buckram's columns for every noted table and
    /// pin them as explicit grid tracks, then lay out the block axis through
    /// the pipeline Buckram owns. Runs before the main layout pass; the
    /// formatting queries only scribble on scratch layout state the main pass
    /// recomputes.
    fn apply_buckram_table_layout(&mut self, text: &mut TextSystem) {
        let mut pendings = std::mem::take(&mut self.pending_tables);
        let mut aggregate = std::mem::take(&mut self.table_shadow);
        for pending in &mut pendings {
            self.table_shadow = TableShadowLedger::default();
            {
                let computed = pending.table_style.clone();
                pending.collapsed_border_metrics = None;
                pending.collapsed_borders = if computed.border_collapse == BorderCollapse::Collapse
                {
                    match collapsed_table_borders(
                        self.boxes,
                        self.styles,
                        &pending.grid,
                        pending.table,
                        &computed,
                        pending.font_size,
                    ) {
                        Ok(borders) => {
                            pending.collapsed_border_metrics = Some(borders.metrics);
                            self.table_shadow.collapsed_metrics += 1;
                            Some(borders.winners)
                        },
                        Err(error) => {
                            self.table_shadow.skip(
                                pending.table,
                                crate::table_shadow::TableShadowSkip::CollapsedBorder(error),
                            );
                            None
                        },
                    }
                } else {
                    None
                };
                let intrinsics = pending
                    .cell_nodes
                    .clone()
                    .into_iter()
                    .map(|cell_node| {
                        cell_node.and_then(|node| self.measure_cell_intrinsics(text, node))
                    })
                    .collect::<Vec<_>>();
                let caption_min = self.measure_caption_min(text, &pending.captions.clone());
                let columns = buckram_table_columns(
                    self.boxes,
                    self.styles,
                    &pending.grid,
                    pending.table,
                    &computed,
                    pending.collapsed_border_metrics.as_ref(),
                    pending.font_size,
                    pending.containing_width,
                    caption_min,
                    &intrinsics,
                    &mut self.table_shadow,
                );
                pending.assigned = columns;
                self.size_wrapper_from_grid(pending);
            }
            self.apply_buckram_table_rows(text, std::slice::from_mut(pending));
            aggregate.record_table(pending.table, std::mem::take(&mut self.table_shadow));
        }
        self.table_shadow = aggregate;
        self.pending_tables = pendings;
    }

    /// Give the wrapper the grid's border-edge width, which is CSS Tables 3
    /// section 2.2.1: "the width of the table wrapper box is the border-edge
    /// width of the table grid box inside it."
    ///
    /// Buckram's table inline sizing has just produced that width, so the rule
    /// is an assignment rather than a measurement, and an `auto` table width is
    /// no harder than a specified one - the shrink-wrapping already happened,
    /// inside the table algorithm that owns it.
    ///
    /// A table Buckram deferred has no such width. Its wrapper falls back to
    /// the `float: left` shrink-to-fit that stood in for this rule before
    /// K4e2, whose domain is now exactly the deferral set.
    fn size_wrapper_from_grid(&mut self, pending: &PendingTable<D::NodeId>) {
        let (Some(wrapper), Some(inline)) = (pending.wrapper, pending.assigned.as_ref()) else {
            return;
        };
        // The fallback float was applied when the tree was built, before this
        // width existed. Retire it here rather than leaving both in play - but
        // only where it was this route that put it there, never where the
        // author wrote `float` on the table and K4e1 migrated it.
        let authored_float = pending
            .node
            .and_then(|node| self.styles.get(node))
            .is_some_and(|computed| computed.float != CssFloat::None);
        let style = self.tree.style_mut(wrapper);
        style.size.width = Dimension::length(inline.used_grid_inline_size);
        if !authored_float {
            style.float = TaffyFloat::None;
        }
        self.tree
            .set_table_wrapper_inline_size(wrapper, inline.used_grid_inline_size);
    }

    /// Run Buckram's block pipeline for every table whose columns it assigned.
    ///
    /// Split from the inline pass so the shared borrows the formatter needs
    /// begin only after column assignment has released them.
    fn apply_buckram_table_rows(
        &mut self,
        text: &mut TextSystem,
        pendings: &mut [PendingTable<D::NodeId>],
    ) {
        let mut ledger = std::mem::take(&mut self.table_shadow.block);
        let Self {
            tree,
            dom,
            styles,
            boxes,
            atomic,
            table_shadow,
            ..
        } = self;
        for pending in pendings {
            let Some(inline) = pending.assigned.as_ref() else {
                continue;
            };
            let computed = &pending.table_style;
            let Some(inputs) = table_block_inputs(
                boxes,
                styles,
                &pending.grid,
                pending.table,
                computed,
                pending.collapsed_border_metrics.as_ref(),
                pending.font_size,
                pending.containing_height,
                &mut ledger,
            ) else {
                continue;
            };
            let mut formatter = CellFormatter(|request: TableCellLayoutInput| {
                let index = pending
                    .grid
                    .cells
                    .iter()
                    .position(|cell| cell.source == request.box_id)
                    .ok_or(TableRowLayoutError::InvalidCellOutput {
                        box_id: request.box_id,
                    })?;
                let node =
                    pending.cell_nodes[index].ok_or(TableRowLayoutError::InvalidCellOutput {
                        box_id: request.box_id,
                    })?;
                Ok(format_table_cell(
                    tree,
                    node,
                    request,
                    &inputs.cells[index],
                    |context, geometry| {
                        measure_inline_context(text, *dom, styles, boxes, atomic, context, geometry)
                    },
                ))
            });
            pending.block = buckram_table_block(
                &pending.grid,
                pending.table,
                inline,
                &inputs,
                pending.containing_height,
                &mut formatter,
                &mut ledger,
            );
            if let Some(block) = &mut pending.block {
                apply_relative_table_part_offsets(
                    block,
                    pending.table,
                    boxes,
                    styles,
                    pending.font_size,
                    inline.used_grid_inline_size,
                    &mut table_shadow.positioning_gaps,
                );
                commit_table_block(tree, pending.table_node, block, inline, |box_id| {
                    pending
                        .grid
                        .cells
                        .iter()
                        .position(|cell| cell.source == box_id)
                        .and_then(|index| pending.cell_nodes[index])
                });
            }
        }
        table_shadow.block = ledger;
    }

    /// The retained structural paint model for every table Buckram laid out.
    fn table_paint_plane(&self) -> TablePaintPlane {
        table_paint_plane(&self.pending_tables, self.boxes, self.styles)
    }

    /// Assert the painted fragments honored every assigned column vector, and
    /// record how far the painted cells sit from Buckram's block rectangles.
    fn verify_table_layout(&mut self, live_rect_of: impl Fn(BoxId) -> Option<Fragment>) {
        let pendings = std::mem::take(&mut self.pending_tables);
        for pending in pendings {
            let mut ledger = self.table_shadow.take_table(pending.table);
            verify_one_table(&pending, &live_rect_of, &mut ledger);
            self.table_shadow.record_table(pending.table, ledger);
        }
    }

    /// Format each detached table part only after K4d has emitted its
    /// in-flow structural parent. The parent fragment is the zero-track
    /// static-position source; the local root itself never joins the table
    /// algorithm tree or changes a row/column measurement.
    fn collect_out_of_flow_table_parts(
        &mut self,
        text: &mut TextSystem,
        fragments: &mut FragmentTree,
        tables: &TableFragmentPlane,
        text_frame: &mut TextFrame<D::NodeId>,
        intrinsic_sizes: &mut IntrinsicSizeCache,
    ) -> Result<(), LayoutError> {
        let Self {
            dom,
            styles,
            boxes,
            atomic,
            tree,
            pending_tables,
            ..
        } = self;
        let parts = pending_tables
            .iter()
            .flat_map(|table| table.out_of_flow_parts.iter().copied())
            .collect::<Vec<_>>();
        for part in parts {
            let Some(parent_box) = boxes[part.box_id].parent() else {
                continue;
            };
            let Some(parent) = fragments.fragment_ids_for_box(parent_box).last().copied() else {
                continue;
            };
            let Some(containing) = fragments.get(parent).map(TreeFragment::physical_rect) else {
                continue;
            };
            tree.compute_layout_with_measure(
                part.node,
                AlgorithmSize::new(
                    AlgorithmAvailableSpace::Definite(containing.width),
                    AlgorithmAvailableSpace::Definite(containing.height),
                ),
                |known, available, _, context, line_constraints| {
                    measure_inline_algorithm_node(
                        text,
                        *dom,
                        *styles,
                        *boxes,
                        atomic,
                        intrinsic_sizes,
                        known,
                        available,
                        context,
                        line_constraints,
                    )
                },
            );
            let mut output = FragmentOutput { fragments };
            collect_inline_fragments(
                tree,
                *boxes,
                part.node,
                FragmentCursor {
                    origin: Point {
                        x: containing.x,
                        y: containing.y,
                    },
                    containing,
                    parent: Some(parent),
                },
                tables,
                &mut output,
                text_frame,
                *styles,
            )?;
        }
        Ok(())
    }

    fn build_anonymous_table_grid(
        &mut self,
        box_id: BoxId,
        inherited: Option<&ComputedValues>,
        parent_font_size: f32,
        containing_size: (Option<f32>, Option<f32>),
    ) -> Result<Option<AlgorithmNodeId>, LayoutError> {
        let table_style = anonymous_table_style(inherited);
        let computed = grid_style(&table_style, containing_size);
        debug_assert!(
            self.pending_table_handoff.is_none(),
            "a table handoff must be consumed by its own build_box call"
        );
        let font_size = font_size_px(&computed.font_size, parent_font_size);
        let child_containing_size =
            resolved_child_containing_size(&computed, font_size, containing_size);
        let children = self.build_children(box_id, &computed, font_size, child_containing_size)?;
        let table_handoff = self.pending_table_handoff.take();
        let taffy_style = to_taffy_style(&computed, font_size);
        let block_style = to_block_style(self.boxes, self.styles, box_id, &computed, font_size);
        let kind = algorithm_kind(&self.boxes[box_id], children.is_empty());
        let node = self.tree.new_with_children_and_block_style(
            kind,
            block_style,
            taffy_style,
            &children,
            vec![box_id],
        );
        enable_flex_grid_static_position_provider(
            &mut self.tree,
            self.styles,
            self.boxes,
            box_id,
            node,
        );
        if let Some((grid, cell_nodes, out_of_flow_parts)) = table_handoff {
            self.pending_tables.push(PendingTable {
                table: box_id,
                node: None,
                table_style,
                table_node: node,
                wrapper: None,
                captions: Vec::new(),
                grid,
                collapsed_borders: None,
                collapsed_border_metrics: None,
                cell_nodes,
                out_of_flow_parts,
                font_size,
                containing_width: containing_size.0,
                containing_height: containing_size.1,
                assigned: None,
                block: None,
            });
        }
        Ok(Some(node))
    }

    fn build_children(
        &mut self,
        parent: BoxId,
        parent_style: &ComputedValues,
        parent_font_size: f32,
        containing_size: (Option<f32>, Option<f32>),
    ) -> Result<Vec<AlgorithmNodeId>, LayoutError> {
        // A `display: table` box takes its flattened cells directly, matching
        // the precomputed atomic subtree.
        if matches!(
            parent_style.display,
            CssDisplay::Table | CssDisplay::InlineTable
        ) {
            let table = build_table_grid(self.boxes, self.dom, parent);
            let mut cell_nodes = Vec::with_capacity(table.cells.len());
            let mut children = Vec::with_capacity(table.cells.len());
            for cell in &table.cells {
                let built = self.build_box(
                    cell.source,
                    Some(parent_style),
                    parent_font_size,
                    containing_size,
                )?;
                cell_nodes.push(built);
                let Some(node) = built else {
                    continue;
                };
                children.push(node);
            }
            let mut out_of_flow_parts = Vec::with_capacity(table.out_of_flow_parts.len());
            for part in &table.out_of_flow_parts {
                let Some(node) =
                    self.build_box(*part, Some(parent_style), parent_font_size, containing_size)?
                else {
                    continue;
                };
                out_of_flow_parts.push(DetachedTablePart {
                    box_id: *part,
                    node,
                });
            }
            // K4c5b: hand the grid to build_box, which creates the table's
            // algorithm node and notes the table for Buckram column
            // assignment before the main layout pass.
            self.pending_table_handoff = Some((table, cell_nodes, out_of_flow_parts));
            return Ok(children);
        }
        self.build_flow_children(
            parent,
            self.boxes[parent].children().to_vec(),
            parent_style,
            parent_font_size,
            containing_size,
        )
    }

    fn build_flow_children(
        &mut self,
        parent: BoxId,
        child_ids: Vec<BoxId>,
        parent_style: &ComputedValues,
        parent_font_size: f32,
        containing_size: (Option<f32>, Option<f32>),
    ) -> Result<Vec<AlgorithmNodeId>, LayoutError> {
        let intrinsic_owner = intrinsic_owner_for_flow_children(self.boxes, parent, &child_ids);
        let mut children = Vec::new();
        let mut inline_group = Vec::new();
        for child in child_ids {
            if box_is_inline(self.boxes, child) {
                inline_group.push(child);
                continue;
            }
            if !self.inline_group_is_blank(&inline_group, parent_style) {
                children.push(self.build_inline_group(
                    intrinsic_owner,
                    &inline_group,
                    parent_style,
                    parent_font_size,
                )?);
                self.build_positioned_inline_descendants(
                    &mut children,
                    &inline_group,
                    parent_style,
                    containing_size,
                )?;
            }
            inline_group.clear();
            if let Some(node) =
                self.build_box(child, Some(parent_style), parent_font_size, containing_size)?
            {
                children.push(node);
            }
        }
        if !self.inline_group_is_blank(&inline_group, parent_style) {
            children.push(self.build_inline_group(
                intrinsic_owner,
                &inline_group,
                parent_style,
                parent_font_size,
            )?);
            self.build_positioned_inline_descendants(
                &mut children,
                &inline_group,
                parent_style,
                containing_size,
            )?;
        }
        Ok(children)
    }

    /// Inline formatting omits absolute and fixed descendants entirely. They
    /// remain structural descendants in the fragment tree, but their local
    /// block formatting root must sit beside the inline measure leaf so K5d
    /// can query its intrinsic size and reformat it at the resolved width.
    fn build_positioned_inline_descendants(
        &mut self,
        children: &mut Vec<AlgorithmNodeId>,
        roots: &[BoxId],
        parent_style: &ComputedValues,
        containing_size: (Option<f32>, Option<f32>),
    ) -> Result<(), LayoutError> {
        for positioned in positioned_roots_in_inline_group(self.boxes, roots) {
            let parent_font_size = inherited_font_size(self.boxes, self.styles, positioned);
            if let Some(node) = self.build_box(
                positioned,
                Some(parent_style),
                parent_font_size,
                containing_size,
            )? {
                children.push(node);
            }
        }
        Ok(())
    }

    /// Whether a pending inline run generates no box at all.
    ///
    /// css-flexbox section 4 and css-grid section 6 both say a run of
    /// collapsible white space between two items generates no anonymous item.
    /// That matters because a flex or grid container turns every in-flow
    /// child into an item, so the ordinary newline-and-indent between two
    /// items would otherwise consume a cell and shift every following item by
    /// one position.
    ///
    /// **Deliberately scoped to those two container types.** White-space
    /// Buckram has already removed whitespace-only anonymous items before
    /// this lowering step.
    fn inline_group_is_blank(&self, roots: &[BoxId], _parent_style: &ComputedValues) -> bool {
        roots.is_empty()
    }

    fn build_inline_group(
        &mut self,
        owner: Option<BoxId>,
        roots: &[BoxId],
        parent_style: &ComputedValues,
        _parent_font_size: f32,
    ) -> Result<AlgorithmNodeId, LayoutError> {
        let width = roots
            .iter()
            .filter_map(|box_id| self.atomic.get(*box_id))
            .map(|fragment| fragment.width)
            .sum();
        let height = roots
            .iter()
            .filter_map(|box_id| self.atomic.get(*box_id))
            .map(|fragment| fragment.height)
            .fold(0.0_f32, f32::max);
        let flow = roots
            .first()
            .map_or(FlowAxes::HORIZONTAL_LTR, |root| self.boxes[*root].flow);
        let containing_flow = roots
            .first()
            .and_then(|root| self.boxes[*root].parent())
            .map_or(flow, |parent| self.boxes[parent].flow);
        let node = self.tree.new_leaf_with_context_and_block_style(
            BlockStyle::anonymous(flow, containing_flow),
            Style {
                display: Display::Block,
                ..Style::default()
            },
            InlineMeasure {
                owner,
                roots: roots.to_vec(),
                style: parent_style.clone(),
                width,
                height,
                replaced_size: None,
                layouts: Vec::new(),
                placement_constraints: None,
            },
            roots.to_vec(),
        );
        // The inline formatter owns the distinction between wrapped and
        // no-wrap lines. Both still need the current float band to choose a
        // line origin and, when possible, the next wider band.
        self.tree.enable_float_line_constraints(node);
        Ok(node)
    }
}

impl<D> BuildState<'_, D>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    /// One intrinsic inline query through the same measure contract the main
    /// layout uses. Only sound once painted fragments have been collected,
    /// because it recomputes the subtree's scratch layout.
    fn measure_intrinsic_width(
        &mut self,
        node: AlgorithmNodeId,
        available: AlgorithmAvailableSpace,
    ) -> f32 {
        self.tree
            .compute_layout_with_measure_excluding_out_of_flow_children(
                node,
                AlgorithmSize::new(available, AlgorithmAvailableSpace::MaxContent),
                |known, available, _, context, _| {
                    let Some(context) = context else {
                        return AlgorithmSize::new(0.0, 0.0);
                    };
                    let available_width = match available.width {
                        AlgorithmAvailableSpace::Definite(width) => width,
                        AlgorithmAvailableSpace::MinContent => context.min_width,
                        AlgorithmAvailableSpace::MaxContent => context.max_width,
                    };
                    AlgorithmSize::new(
                        known
                            .width
                            .unwrap_or(context.max_width.min(available_width.max(0.0))),
                        known.height.unwrap_or(context.height),
                    )
                },
            );
        self.tree.layout(node).width
    }

    /// Measure one cell's border-box intrinsic pair through the live measure
    /// contract. The cell's own width sizing is neutralized for the query,
    /// because Buckram applies those constraints itself, and restored
    /// afterwards so the main layout pass sees the real style.
    fn measure_cell_intrinsics(&mut self, cell_node: AlgorithmNodeId) -> Option<IntrinsicSizes> {
        let style = self.tree.style_mut(cell_node);
        let saved = (style.size.width, style.min_size.width, style.max_size.width);
        style.size.width = Dimension::auto();
        style.min_size.width = Dimension::auto();
        style.max_size.width = Dimension::auto();
        let direct_child_width = |tree: &AlgorithmTree<Style, TextMeasure, Option<BoxId>>| {
            tree.children(cell_node)
                .iter()
                .filter(|child| !tree.block_style(**child).is_out_of_flow())
                .map(|child| tree.unrounded_layout(*child).width)
                .reduce(f32::max)
                .unwrap_or_else(|| tree.unrounded_layout(cell_node).width)
        };
        self.measure_intrinsic_width(cell_node, AlgorithmAvailableSpace::MinContent);
        let min = direct_child_width(&self.tree);
        self.measure_intrinsic_width(cell_node, AlgorithmAvailableSpace::MaxContent);
        let max = direct_child_width(&self.tree);
        let style = self.tree.style_mut(cell_node);
        (style.size.width, style.min_size.width, style.max_size.width) = saved;
        IntrinsicSizes::new(min, max.max(min))
    }

    /// The floor a caption puts under the table's inline size.
    ///
    /// Its own min-content width plus its horizontal margins, which is what
    /// C5 and C6 of the K4e1 interop matrix pin. Unlike a cell measurement
    /// this does *not* neutralize the caption's own `width`: C7 shows a
    /// specified caption width participating like any other box, so a
    /// `width: 300px` caption puts a floor of 300 under the table. Several
    /// captions each put their own floor down and the widest one wins.
    fn measure_caption_min(&mut self, captions: &[(AlgorithmNodeId, f32)]) -> Option<f32> {
        captions
            .iter()
            .map(|(caption, margins)| {
                self.measure_intrinsic_width(*caption, AlgorithmAvailableSpace::MinContent)
                    + margins
            })
            .reduce(f32::max)
            .filter(|minimum| minimum.is_finite() && *minimum >= 0.0)
    }

    fn build_anonymous_table_grid(
        &mut self,
        box_id: BoxId,
        inherited: Option<&ComputedValues>,
        parent_font_size: f32,
        containing_size: (Option<f32>, Option<f32>),
    ) -> Result<Option<AlgorithmNodeId>, LayoutError> {
        let table_style = anonymous_table_style(inherited);
        let computed = grid_style(&table_style, containing_size);
        let font_size = font_size_px(&computed.font_size, parent_font_size);
        let child_containing_size =
            resolved_child_containing_size(&computed, font_size, containing_size);
        let table = build_table_grid(self.boxes, self.dom, box_id);
        let mut cell_nodes = Vec::with_capacity(table.cells.len());
        let mut children = Vec::with_capacity(table.cells.len());
        for cell in &table.cells {
            let built = self.build_box(
                cell.source,
                Some(&computed),
                font_size,
                child_containing_size,
            )?;
            cell_nodes.push(built);
            if let Some(node) = built {
                children.push(node);
            }
        }
        let mut out_of_flow_parts = Vec::with_capacity(table.out_of_flow_parts.len());
        for part in &table.out_of_flow_parts {
            let Some(node) =
                self.build_box(*part, Some(&computed), font_size, child_containing_size)?
            else {
                continue;
            };
            out_of_flow_parts.push(DetachedTablePart {
                box_id: *part,
                node,
            });
        }
        let taffy_style = to_taffy_style(&computed, font_size);
        let block_style = to_block_style(self.boxes, self.styles, box_id, &computed, font_size);
        let kind = algorithm_kind(&self.boxes[box_id], children.is_empty());
        let node = self.tree.new_with_children_and_block_style(
            kind,
            block_style,
            taffy_style,
            &children,
            Some(box_id),
        );
        enable_flex_grid_static_position_provider(
            &mut self.tree,
            self.styles,
            self.boxes,
            box_id,
            node,
        );
        self.pending_tables.push(PendingTable {
            table: box_id,
            node: None,
            table_style,
            table_node: node,
            wrapper: None,
            captions: Vec::new(),
            grid: table,
            collapsed_borders: None,
            collapsed_border_metrics: None,
            cell_nodes,
            out_of_flow_parts,
            font_size,
            containing_width: containing_size.0,
            containing_height: containing_size.1,
            assigned: None,
            block: None,
        });
        Ok(Some(node))
    }

    /// K4c5b and K4d6b: compute Buckram's columns for every noted table and
    /// pin them as explicit grid tracks, then lay out the block axis. Runs
    /// after the tree is built and before the main layout pass; the queries
    /// only scribble on scratch layout state that the main pass recomputes.
    fn apply_buckram_table_layout(&mut self) {
        let mut pendings = std::mem::take(&mut self.pending_tables);
        let mut aggregate = std::mem::take(&mut self.table_shadow);
        for pending in &mut pendings {
            self.table_shadow = TableShadowLedger::default();
            {
                let computed = pending.table_style.clone();
                pending.collapsed_border_metrics = None;
                pending.collapsed_borders = if computed.border_collapse == BorderCollapse::Collapse
                {
                    match collapsed_table_borders(
                        self.boxes,
                        self.styles,
                        &pending.grid,
                        pending.table,
                        &computed,
                        pending.font_size,
                    ) {
                        Ok(borders) => {
                            pending.collapsed_border_metrics = Some(borders.metrics);
                            self.table_shadow.collapsed_metrics += 1;
                            Some(borders.winners)
                        },
                        Err(error) => {
                            self.table_shadow.skip(
                                pending.table,
                                crate::table_shadow::TableShadowSkip::CollapsedBorder(error),
                            );
                            None
                        },
                    }
                } else {
                    None
                };
                let intrinsics = pending
                    .cell_nodes
                    .clone()
                    .into_iter()
                    .map(|cell_node| cell_node.and_then(|node| self.measure_cell_intrinsics(node)))
                    .collect::<Vec<_>>();
                let caption_min = self.measure_caption_min(&pending.captions.clone());
                let columns = buckram_table_columns(
                    self.boxes,
                    self.styles,
                    &pending.grid,
                    pending.table,
                    &computed,
                    pending.collapsed_border_metrics.as_ref(),
                    pending.font_size,
                    pending.containing_width,
                    caption_min,
                    &intrinsics,
                    &mut self.table_shadow,
                );
                pending.assigned = columns;
                self.size_wrapper_from_grid(pending);
            }
            self.apply_buckram_table_rows(std::slice::from_mut(pending));
            aggregate.record_table(pending.table, std::mem::take(&mut self.table_shadow));
        }
        self.table_shadow = aggregate;
        self.pending_tables = pendings;
    }

    /// Give the wrapper the grid's border-edge width, which is CSS Tables 3
    /// section 2.2.1: "the width of the table wrapper box is the border-edge
    /// width of the table grid box inside it."
    ///
    /// Buckram's table inline sizing has just produced that width, so the rule
    /// is an assignment rather than a measurement, and an `auto` table width is
    /// no harder than a specified one - the shrink-wrapping already happened,
    /// inside the table algorithm that owns it.
    ///
    /// A table Buckram deferred has no such width. Its wrapper falls back to
    /// the `float: left` shrink-to-fit that stood in for this rule before
    /// K4e2, whose domain is now exactly the deferral set.
    fn size_wrapper_from_grid(&mut self, pending: &PendingTable<D::NodeId>) {
        let (Some(wrapper), Some(inline)) = (pending.wrapper, pending.assigned.as_ref()) else {
            return;
        };
        // The fallback float was applied when the tree was built, before this
        // width existed. Retire it here rather than leaving both in play - but
        // only where it was this route that put it there, never where the
        // author wrote `float` on the table and K4e1 migrated it.
        let authored_float = pending
            .node
            .and_then(|node| self.styles.get(node))
            .is_some_and(|computed| computed.float != CssFloat::None);
        let style = self.tree.style_mut(wrapper);
        style.size.width = Dimension::length(inline.used_grid_inline_size);
        if !authored_float {
            style.float = TaffyFloat::None;
        }
        self.tree
            .set_table_wrapper_inline_size(wrapper, inline.used_grid_inline_size);
    }

    /// Run Buckram's block pipeline for every table whose columns it assigned.
    fn apply_buckram_table_rows(&mut self, pendings: &mut [PendingTable<D::NodeId>]) {
        let mut ledger = std::mem::take(&mut self.table_shadow.block);
        let Self {
            tree,
            styles,
            boxes,
            table_shadow,
            ..
        } = self;
        for pending in pendings {
            let Some(inline) = pending.assigned.as_ref() else {
                continue;
            };
            let computed = &pending.table_style;
            let Some(inputs) = table_block_inputs(
                boxes,
                styles,
                &pending.grid,
                pending.table,
                computed,
                pending.collapsed_border_metrics.as_ref(),
                pending.font_size,
                pending.containing_height,
                &mut ledger,
            ) else {
                continue;
            };
            let mut formatter = CellFormatter(|request: TableCellLayoutInput| {
                let index = pending
                    .grid
                    .cells
                    .iter()
                    .position(|cell| cell.source == request.box_id)
                    .ok_or(TableRowLayoutError::InvalidCellOutput {
                        box_id: request.box_id,
                    })?;
                let node =
                    pending.cell_nodes[index].ok_or(TableRowLayoutError::InvalidCellOutput {
                        box_id: request.box_id,
                    })?;
                Ok(format_table_cell(
                    tree,
                    node,
                    request,
                    &inputs.cells[index],
                    |context: &mut TextMeasure, geometry| {
                        (
                            context.max_width.min(geometry.width.max(0.0)),
                            context.height,
                        )
                    },
                ))
            });
            pending.block = buckram_table_block(
                &pending.grid,
                pending.table,
                inline,
                &inputs,
                pending.containing_height,
                &mut formatter,
                &mut ledger,
            );
            if let Some(block) = &mut pending.block {
                apply_relative_table_part_offsets(
                    block,
                    pending.table,
                    boxes,
                    styles,
                    pending.font_size,
                    inline.used_grid_inline_size,
                    &mut table_shadow.positioning_gaps,
                );
                commit_table_block(tree, pending.table_node, block, inline, |box_id| {
                    pending
                        .grid
                        .cells
                        .iter()
                        .position(|cell| cell.source == box_id)
                        .and_then(|index| pending.cell_nodes[index])
                });
            }
        }
        table_shadow.block = ledger;
    }

    /// The retained structural paint model for every table Buckram laid out.
    fn table_paint_plane(&self) -> TablePaintPlane {
        table_paint_plane(&self.pending_tables, self.boxes, self.styles)
    }

    /// Assert the painted fragments honored every assigned column vector, and
    /// record how far the painted cells sit from Buckram's block rectangles.
    /// Runs after fragment collection.
    fn verify_table_layout(&mut self, live_rect_of: impl Fn(BoxId) -> Option<Fragment>) {
        let pendings = std::mem::take(&mut self.pending_tables);
        for pending in pendings {
            let mut ledger = self.table_shadow.take_table(pending.table);
            verify_one_table(&pending, &live_rect_of, &mut ledger);
            self.table_shadow.record_table(pending.table, ledger);
        }
    }

    /// Format each detached table part only after K4d has emitted its
    /// in-flow structural parent. The parent fragment is the zero-track
    /// static-position source; the local root itself never joins the table
    /// algorithm tree or changes a row/column measurement.
    fn collect_out_of_flow_table_parts(
        &mut self,
        fragments: &mut FragmentTree,
        tables: &TableFragmentPlane,
    ) -> Result<(), LayoutError> {
        let parts = self
            .pending_tables
            .iter()
            .flat_map(|table| table.out_of_flow_parts.iter().copied())
            .collect::<Vec<_>>();
        for part in parts {
            let Some(parent_box) = self.boxes[part.box_id].parent() else {
                continue;
            };
            let Some(parent) = fragments.fragment_ids_for_box(parent_box).last().copied() else {
                continue;
            };
            let Some(containing) = fragments.get(parent).map(TreeFragment::physical_rect) else {
                continue;
            };
            self.tree.compute_layout_with_measure(
                part.node,
                AlgorithmSize::new(
                    AlgorithmAvailableSpace::Definite(containing.width),
                    AlgorithmAvailableSpace::Definite(containing.height),
                ),
                |known, available, _, context, _| {
                    measure_text_algorithm_node(known, available, context)
                },
            );
            let mut output = FragmentOutput { fragments };
            collect_fragments(
                &self.tree,
                self.boxes,
                part.node,
                FragmentCursor {
                    origin: Point {
                        x: containing.x,
                        y: containing.y,
                    },
                    containing,
                    parent: Some(parent),
                },
                tables,
                &mut output,
            )?;
        }
        Ok(())
    }

    fn build_box(
        &mut self,
        box_id: BoxId,
        inherited: Option<&ComputedValues>,
        parent_font_size: f32,
        containing_size: (Option<f32>, Option<f32>),
    ) -> Result<Option<AlgorithmNodeId>, LayoutError> {
        match self.boxes[box_id].origin {
            BoxOrigin::Element(node) => {
                let computed = self.styles.get(node).cloned().unwrap_or_default();
                // K4e1: the wrapper above this grid took the properties
                // CSS 2.1 section 17.4 assigns to it; the grid sees them unset.
                let (computed, table_style) =
                    if self.boxes[box_id].display.internal_table == Some(InternalTableRole::Grid) {
                        (grid_style(&computed, containing_size), Some(computed))
                    } else {
                        (computed, None)
                    };
                let font_size = font_size_px(&computed.font_size, parent_font_size);
                let mut child_containing_size =
                    resolved_child_containing_size(&computed, font_size, containing_size);
                if self
                    .dom
                    .parent(node)
                    .is_some_and(|parent| self.dom.kind(parent) == NodeKind::Document)
                {
                    // The root element's containing block is the initial
                    // containing block. Preserve its definite block size for
                    // percentage-height descendants even when the root's own
                    // height is auto.
                    child_containing_size.1 = child_containing_size.1.or(containing_size.1);
                }
                // A `display: table` box takes its flattened cells directly,
                // so the row-group and row boxes never enter the tree.
                let table = (matches!(
                    computed.display,
                    CssDisplay::Table | CssDisplay::InlineTable
                ))
                .then(|| build_table_grid(self.boxes, self.dom, box_id));
                let mut table_cell_nodes = Vec::new();
                let mut table_out_of_flow_parts = Vec::new();
                let children = if let Some(table) = table.as_ref() {
                    let mut children = Vec::with_capacity(table.cells.len());
                    for cell in &table.cells {
                        let built = self.build_box(
                            cell.source,
                            Some(&computed),
                            font_size,
                            child_containing_size,
                        )?;
                        table_cell_nodes.push(built);
                        let Some(taffy_node) = built else {
                            continue;
                        };
                        children.push(taffy_node);
                    }
                    for part in &table.out_of_flow_parts {
                        let Some(node) = self.build_box(
                            *part,
                            Some(&computed),
                            font_size,
                            child_containing_size,
                        )?
                        else {
                            continue;
                        };
                        table_out_of_flow_parts.push(DetachedTablePart {
                            box_id: *part,
                            node,
                        });
                    }
                    children
                } else {
                    self.boxes[box_id]
                        .children()
                        .iter()
                        .filter_map(|child| {
                            self.build_box(
                                *child,
                                Some(&computed),
                                font_size,
                                child_containing_size,
                            )
                            .transpose()
                        })
                        .collect::<Result<Vec<_>, _>>()?
                };
                let mut taffy_style = to_taffy_style(&computed, font_size);
                taffy_style.size.width =
                    dimension_with_basis(computed.width, font_size, containing_size.0);
                taffy_style.size.height =
                    dimension_with_basis(computed.height, font_size, containing_size.1);
                taffy_style.min_size.width =
                    dimension_with_basis(computed.min_width, font_size, containing_size.0);
                taffy_style.min_size.height =
                    dimension_with_basis(computed.min_height, font_size, containing_size.1);
                taffy_style.max_size.width =
                    dimension_with_basis(computed.max_width, font_size, containing_size.0);
                taffy_style.max_size.height =
                    dimension_with_basis(computed.max_height, font_size, containing_size.1);
                let replaced_size = apply_replaced_intrinsic_style(
                    &mut taffy_style,
                    self.dom,
                    node,
                    &computed,
                    self.image_sources,
                    font_size,
                    matches!(
                        self.boxes[box_id].display.outside,
                        Some(buckram::DisplayOutside::Block)
                    ) && !stretched_by_ancestor_context(self.boxes, box_id),
                    // Percentage padding against an indefinite basis is zero.
                    containing_size.0.unwrap_or(0.0),
                );
                // Taffy exempts a compressible replaced element from block
                // stretch-sizing (CSS 2.1 10.3.4) and from grid `normal`
                // stretching (css-grid-1 6.2). Two conditions narrow it.
                //
                // It is armed only for a box that actually becomes a measured
                // leaf: a `<canvas>` with fallback content is a block container,
                // and arming it there would let Taffy shrink-wrap the fallback
                // instead of laying it out.
                //
                // And only under `content-box`. Arming it for a border-box
                // replaced element changes which path applies CSS 2.1 10.4's
                // ratio-preserving min/max clamp, and box-sizing-replaced-001,
                // -002 and -003 fail when it does. The cost is named: a
                // border-box replaced element still stretches, in a block
                // container and as a grid item alike.
                // Since taffy's block path stopped reading this flag, arming it
                // reaches only the grid `normal` exemption, so border-box leaves
                // are safe to include: a border-box replaced grid item no longer
                // stretches either.
                taffy_style.item_is_replaced = replaced_size.is_some() && children.is_empty();
                let block_style =
                    to_block_style(self.boxes, self.styles, box_id, &computed, font_size);
                let kind = algorithm_kind(&self.boxes[box_id], children.is_empty());
                let dom_node = node;
                let node =
                    if let Some((width, height)) = replaced_size.filter(|_| children.is_empty()) {
                        self.tree.new_leaf_with_context_and_block_style(
                            block_style,
                            taffy_style,
                            TextMeasure {
                                min_width: width,
                                max_width: width,
                                height,
                            },
                            Some(box_id),
                        )
                    } else {
                        self.tree.new_with_children_and_block_style(
                            kind,
                            block_style,
                            taffy_style,
                            &children,
                            Some(box_id),
                        )
                    };
                enable_flex_grid_static_position_provider(
                    &mut self.tree,
                    self.styles,
                    self.boxes,
                    box_id,
                    node,
                );
                // K4c5b: Buckram owns this table's columns. They are computed
                // before the main layout pass, once the whole tree exists and
                // intrinsic queries can run, and pinned as explicit tracks.
                if let Some(grid) = table {
                    self.pending_tables.push(PendingTable {
                        table: box_id,
                        node: Some(dom_node),
                        table_style: table_style.unwrap_or_default(),
                        table_node: node,
                        wrapper: None,
                        captions: Vec::new(),
                        grid,
                        collapsed_borders: None,
                        collapsed_border_metrics: None,
                        cell_nodes: std::mem::take(&mut table_cell_nodes),
                        out_of_flow_parts: std::mem::take(&mut table_out_of_flow_parts),
                        font_size,
                        containing_width: containing_size.0,
                        containing_height: containing_size.1,
                        assigned: None,
                        block: None,
                    });
                }
                if supports_nested_float_state(&self.boxes[box_id], block_style, kind) {
                    self.tree.enable_nested_float_state(node);
                }
                if block_style.float != FloatSide::None
                    && self.boxes[box_id].float_context == FloatContextProvenance::Inline
                {
                    self.tree.mark_inline_context_float(node);
                }
                if supports_float_avoidance(&self.boxes[box_id], block_style, kind) {
                    self.tree.enable_float_avoidance(node);
                }
                if supports_intrinsic_shrink_to_fit(
                    &self.tree,
                    node,
                    self.boxes,
                    box_id,
                    &computed,
                    block_style,
                    kind,
                ) {
                    self.tree.enable_intrinsic_shrink_to_fit(node);
                }
                Ok(Some(node))
            },
            BoxOrigin::Text(node) => {
                let text = self.dom.text(node).unwrap_or("");
                let preserves_whitespace = inherited.is_some_and(|style| {
                    matches!(
                        style.white_space_collapse,
                        WhiteSpaceCollapse::Preserve | WhiteSpaceCollapse::BreakSpaces
                    )
                });
                if text.is_empty() || (!preserves_whitespace && is_collapsible_whitespace(text)) {
                    return Ok(None);
                }
                let font_size = parent_font_size;
                let line_height = inherited
                    .map(|style| line_height_px(&style.line_height, font_size))
                    .unwrap_or(font_size * 1.2);
                let mut min_width = if preserves_whitespace {
                    text.lines()
                        .map(|line| line.chars().count())
                        .max()
                        .unwrap_or(0)
                } else {
                    collapsed_word_width(text)
                } as f32
                    * font_size
                    * 0.6;
                let mut max_width = if preserves_whitespace {
                    min_width
                } else {
                    collapsed_text_width(text) as f32 * font_size * 0.6
                };
                let line_count = if preserves_whitespace {
                    text.lines().count().max(1)
                } else {
                    1
                };
                let mut height = line_count as f32 * line_height;
                if let Some(text_system) = self.text.as_deref_mut()
                    && let Some(parent_style) = inherited
                {
                    let fragments = AtomicLayoutPlane::default();
                    let roots = [box_id];
                    let minimum = text_system
                        .format_inline_group(
                            self.dom,
                            self.styles,
                            self.boxes,
                            &fragments,
                            InlineRequest {
                                roots: &roots,
                                parent_style,
                                width: 0.01,
                                intrinsic_kind: Some(IntrinsicSizeKind::MinContent),
                                line_constraints: None,
                            },
                        )
                        .map(|layout| layout.size());
                    let maximum = text_system
                        .format_inline_group(
                            self.dom,
                            self.styles,
                            self.boxes,
                            &fragments,
                            InlineRequest {
                                roots: &roots,
                                parent_style,
                                width: f32::INFINITY,
                                intrinsic_kind: Some(IntrinsicSizeKind::MaxContent),
                                line_constraints: None,
                            },
                        )
                        .map(|layout| layout.size());
                    if let Some((minimum, _)) = minimum {
                        min_width = minimum;
                    }
                    if let Some((maximum, maximum_height)) = maximum {
                        max_width = maximum.max(min_width);
                        height = maximum_height;
                    }
                }
                let node = self.tree.new_leaf_with_context_and_block_style(
                    anonymous_block_style(self.boxes, box_id),
                    Style {
                        display: Display::Block,
                        ..Style::default()
                    },
                    TextMeasure {
                        min_width,
                        max_width,
                        height,
                    },
                    Some(box_id),
                );
                Ok(Some(node))
            },
            BoxOrigin::Pseudo { .. } | BoxOrigin::Anonymous { .. } => {
                if let Some(grid) = (self.boxes[box_id].display.internal_table
                    == Some(InternalTableRole::Wrapper))
                .then(|| wrapped_table_grid(self.boxes, box_id))
                .flatten()
                {
                    // See InlineBuildState's corresponding K4e1 wrapper.
                    let table = match legacy_origin_node(self.boxes, grid) {
                        Some(element) => self.styles.get(element).cloned().unwrap_or_default(),
                        None => anonymous_table_style(inherited),
                    };
                    let computed = wrapper_style(&table);
                    let font_size = font_size_px(&computed.font_size, parent_font_size);
                    let mut caption_nodes = Vec::new();
                    let mut children = Vec::new();
                    for child in wrapper_children_in_caption_order(self.boxes, self.styles, box_id)
                    {
                        let Some(child_node) =
                            self.build_box(child, inherited, parent_font_size, containing_size)?
                        else {
                            continue;
                        };
                        if self.boxes[child].display.internal_table
                            == Some(InternalTableRole::Caption)
                            && matches!(
                                self.boxes[child].positioning,
                                PositioningScheme::Static
                                    | PositioningScheme::Relative
                                    | PositioningScheme::Sticky
                            )
                        {
                            let caption = self
                                .boxes
                                .origin_node(child)
                                .and_then(|node| self.styles.get(node))
                                .cloned()
                                .unwrap_or_default();
                            let em = font_size_px(&caption.font_size, font_size);
                            caption_nodes.push((
                                child_node,
                                caption_horizontal_margins(&caption, em, containing_size.0),
                            ));
                        }
                        children.push(child_node);
                    }
                    let mut taffy_style = to_taffy_style(&computed, font_size);
                    let logical_wrapper =
                        wrapper_uses_logical_block_axis(&mut taffy_style, self.boxes[box_id].flow);
                    if wrapper_needs_float_fallback(self.boxes, box_id, &taffy_style) {
                        taffy_style.float = TaffyFloat::Left;
                    }
                    let wrapper_grid_width = wrapper_width_from_grid(&to_taffy_style(
                        &grid_style(&table, containing_size),
                        font_size,
                    ));
                    if let Some(width) = wrapper_grid_width {
                        taffy_style.size.width = width;
                    }
                    let block_style =
                        to_block_style(self.boxes, self.styles, box_id, &computed, font_size);
                    let kind = if logical_wrapper {
                        AlgorithmKind::Flex
                    } else {
                        algorithm_kind(&self.boxes[box_id], children.is_empty())
                    };
                    let node = self.tree.new_with_children_and_block_style(
                        kind,
                        block_style,
                        taffy_style,
                        &children,
                        Some(box_id),
                    );
                    if let Some(width) = wrapper_grid_width.and_then(Dimension::into_option) {
                        self.tree.set_table_wrapper_inline_size(node, width);
                    }
                    enable_flex_grid_static_position_provider(
                        &mut self.tree,
                        self.styles,
                        self.boxes,
                        box_id,
                        node,
                    );
                    if let Some(pending) = self
                        .pending_tables
                        .iter_mut()
                        .find(|pending| pending.table == grid)
                    {
                        pending.wrapper = Some(node);
                        pending.captions = caption_nodes;
                    }
                    return Ok(Some(node));
                }
                if self.boxes[box_id].display.internal_table == Some(InternalTableRole::Grid) {
                    return self.build_anonymous_table_grid(
                        box_id,
                        inherited,
                        parent_font_size,
                        containing_size,
                    );
                }
                let computed = inherited.cloned().unwrap_or_default();
                let children = self.boxes[box_id]
                    .children()
                    .iter()
                    .filter_map(|child| {
                        self.build_box(*child, Some(&computed), parent_font_size, containing_size)
                            .transpose()
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let block_style = anonymous_block_style(self.boxes, box_id);
                let kind = algorithm_kind(&self.boxes[box_id], children.is_empty());
                let node = self.tree.new_with_children_and_block_style(
                    kind,
                    block_style,
                    anonymous_taffy_style(&self.boxes[box_id]),
                    &children,
                    Some(box_id),
                );
                enable_flex_grid_static_position_provider(
                    &mut self.tree,
                    self.styles,
                    self.boxes,
                    box_id,
                    node,
                );
                if supports_nested_float_state(&self.boxes[box_id], block_style, kind) {
                    self.tree.enable_nested_float_state(node);
                }
                Ok(Some(node))
            },
        }
    }
}

struct FragmentOutput<'a> {
    fragments: &'a mut FragmentTree,
}

#[derive(Clone, Copy)]
struct FragmentCursor {
    origin: Point<f32>,
    containing: Fragment,
    parent: Option<FragmentId>,
}

fn intrinsic_owner_for_flow_children<Id>(
    boxes: &GeneratedBoxTree<Id>,
    parent: BoxId,
    children: &[BoxId],
) -> Option<BoxId>
where
    Id: Copy + Eq + Hash,
{
    let mut groups = 0;
    let mut inside_group = false;
    for child in children {
        if box_is_inline(boxes, *child) {
            if !inside_group {
                groups += 1;
                inside_group = true;
            }
        } else {
            inside_group = false;
        }
    }
    (groups == 1).then_some(parent)
}

fn box_is_inline<Id>(boxes: &GeneratedBoxTree<Id>, box_id: BoxId) -> bool
where
    Id: Copy + Eq + Hash,
{
    let css_box = &boxes[box_id];
    // K4e4 deleted K4a's wrapper/grid exclusion here: an inline-table's
    // wrapper is inline-level and rides the atomic-inline lane. The grid
    // never reaches this test, because a wrapper's children are built
    // directly rather than through flow-children grouping.
    css_box.display.outside == Some(DisplayOutside::Inline)
        && css_box.float == FloatSide::None
        && matches!(
            css_box.positioning,
            PositioningScheme::Static | PositioningScheme::Relative | PositioningScheme::Sticky
        )
}

/// Return each outermost absolute or fixed descendant of an inline run.
///
/// The run itself remains with the inline formatter, which supplies the line
/// fragment used as the positioned root's static-position source. The root
/// must be built separately, because out-of-flow contents neither occupy that
/// line nor inherit its measured width.
fn positioned_roots_in_inline_group<Id>(boxes: &GeneratedBoxTree<Id>, roots: &[BoxId]) -> Vec<BoxId>
where
    Id: Copy + Eq + Hash,
{
    fn visit<Id>(boxes: &GeneratedBoxTree<Id>, box_id: BoxId, positioned: &mut Vec<BoxId>)
    where
        Id: Copy + Eq + Hash,
    {
        for child in boxes[box_id].children() {
            if matches!(
                boxes[*child].positioning,
                PositioningScheme::Absolute | PositioningScheme::Fixed
            ) {
                positioned.push(*child);
                continue;
            }
            visit(boxes, *child, positioned);
        }
    }

    let mut positioned = Vec::new();
    for root in roots {
        visit(boxes, *root, &mut positioned);
    }
    positioned
}

fn anonymous_block_style<Id>(boxes: &GeneratedBoxTree<Id>, box_id: BoxId) -> BlockStyle
where
    Id: Copy + Eq + Hash,
{
    let flow = boxes[box_id].flow;
    let containing_flow = boxes[box_id]
        .parent()
        .map_or(flow, |parent| boxes[parent].flow);
    BlockStyle::anonymous(flow, containing_flow)
}

fn to_block_style<Id>(
    boxes: &buckram::CssBoxTree<Id>,
    styles: &StylePlane<Id>,
    box_id: BoxId,
    computed: &ComputedValues,
    font_size: f32,
) -> BlockStyle
where
    Id: Copy + Eq + Hash,
{
    let css_box = &boxes[box_id];
    let containing_flow = css_box
        .parent()
        .map_or(FlowAxes::HORIZONTAL_LTR, |parent| boxes[parent].flow);
    let mut size_containment = match computed.container_type {
        ContainerType::Normal => BlockDimensions::new(false, false),
        ContainerType::InlineSize if computed.writing_mode.is_vertical() => {
            BlockDimensions::new(false, true)
        },
        ContainerType::InlineSize => BlockDimensions::new(true, false),
        ContainerType::Size => BlockDimensions::new(true, true),
    };
    if computed.contain.has_size() {
        size_containment = BlockDimensions::new(true, true);
    } else if computed.contain.has_inline_size() {
        if computed.writing_mode.is_vertical() {
            size_containment.height = true;
        } else {
            size_containment.width = true;
        }
    }
    let establishes_bfc = matches!(
        css_box.display.inside,
        Some(DisplayInside::FlowRoot | DisplayInside::Flex | DisplayInside::Grid)
    ) || matches!(
        computed.display,
        CssDisplay::InlineBlock
            | CssDisplay::Table
            | CssDisplay::InlineTable
            | CssDisplay::TableCell
            | CssDisplay::TableCaption
    ) || matches!(
        computed.position,
        CssPosition::Absolute | CssPosition::Fixed
    ) || computed.float != CssFloat::None
        || computed.overflow_x != CssOverflow::Visible
        || computed.overflow_y != CssOverflow::Visible
        || computed.contain.has_layout()
        || computed.contain.has_paint();
    let shape_reference_box = shape_outside_reference_box(computed);
    let nonlinear_shape_radius = shape_outside_has_nonlinear_radius(computed);

    BlockStyle {
        flow: css_box.flow,
        containing_flow,
        size: BlockDimensions::new(
            block_size_value(computed.width, font_size),
            block_size_value(computed.height, font_size),
        ),
        min_size: BlockDimensions::new(
            block_size_value(computed.min_width, font_size),
            block_size_value(computed.min_height, font_size),
        ),
        max_size: BlockDimensions::new(
            block_size_value(computed.max_width, font_size),
            block_size_value(computed.max_height, font_size),
        ),
        margin: PhysicalSides {
            top: block_margin(computed.margin_top, font_size),
            right: block_margin(computed.margin_right, font_size),
            bottom: block_margin(computed.margin_bottom, font_size),
            left: block_margin(computed.margin_left, font_size),
        },
        inset: PhysicalSides {
            top: block_inset(computed.top, font_size),
            right: block_inset(computed.right, font_size),
            bottom: block_inset(computed.bottom, font_size),
            left: block_inset(computed.left, font_size),
        },
        padding: PhysicalSides {
            top: flow_length(computed.padding_top.0, font_size),
            right: flow_length(computed.padding_right.0, font_size),
            bottom: flow_length(computed.padding_bottom.0, font_size),
            left: flow_length(computed.padding_left.0, font_size),
        },
        border: PhysicalSides {
            top: border_width_px(
                computed.border_top_style,
                computed.border_top_width,
                font_size,
            ),
            right: border_width_px(
                computed.border_right_style,
                computed.border_right_width,
                font_size,
            ),
            bottom: border_width_px(
                computed.border_bottom_style,
                computed.border_bottom_width,
                font_size,
            ),
            left: border_width_px(
                computed.border_left_style,
                computed.border_left_width,
                font_size,
            ),
        },
        box_sizing: match computed.box_sizing {
            CssBoxSizing::ContentBox => BlockBoxSizing::ContentBox,
            CssBoxSizing::BorderBox => BlockBoxSizing::BorderBox,
        },
        position: match computed.position {
            CssPosition::Static => BuckramBlockPosition::Static,
            CssPosition::Relative => BuckramBlockPosition::Relative,
            CssPosition::Absolute => BuckramBlockPosition::Absolute,
            CssPosition::Fixed => BuckramBlockPosition::Fixed,
            CssPosition::Sticky => BuckramBlockPosition::Sticky,
        },
        float: match computed.float {
            CssFloat::None => FloatSide::None,
            CssFloat::Left => FloatSide::Left,
            CssFloat::Right => FloatSide::Right,
        },
        float_reference_box: shape_reference_box.unwrap_or(FloatReferenceBox::MarginBox),
        has_shape_outside_box: shape_reference_box.is_some() && !nonlinear_shape_radius,
        corner_radii: BlockCornerRadii {
            top_left: block_corner_radius(computed.border_top_left_radius, font_size),
            top_right: block_corner_radius(computed.border_top_right_radius, font_size),
            bottom_right: block_corner_radius(computed.border_bottom_right_radius, font_size),
            bottom_left: block_corner_radius(computed.border_bottom_left_radius, font_size),
        },
        clear: match computed.clear {
            CssClear::None => ClearSide::None,
            CssClear::Left => ClearSide::Left,
            CssClear::Right => ClearSide::Right,
            CssClear::Both => ClearSide::Both,
        },
        establishes_bfc,
        shrink_to_fit: matches!(computed.width, CssSize::Auto)
            && (computed.display == CssDisplay::InlineBlock || computed.float != CssFloat::None),
        replaced: css_box.replaced,
        aspect_ratio: computed.aspect_ratio.preferred_ratio(),
        size_containment,
        has_nonlinear_lengths: block_style_has_nonlinear_lengths(computed),
        overconstrained_inline_alignment: boxes
            .origin_node(box_id)
            .and_then(|id| styles.legacy_descendant_alignment(id))
            .map(|alignment| match alignment {
                LegacyDescendantAlignment::LineLeft => OverconstrainedInlineAlignment::LineLeft,
                LegacyDescendantAlignment::Center => OverconstrainedInlineAlignment::Center,
                LegacyDescendantAlignment::LineRight => OverconstrainedInlineAlignment::LineRight,
            }),
        is_root_element: css_box.parent().is_none()
            && matches!(css_box.origin, BoxOrigin::Element(_)),
    }
}

fn shape_outside_reference_box(computed: &ComputedValues) -> Option<FloatReferenceBox> {
    match computed.shape_outside {
        CssShapeOutside::None => None,
        CssShapeOutside::MarginBox => Some(FloatReferenceBox::MarginBox),
        CssShapeOutside::BorderBox => Some(FloatReferenceBox::BorderBox),
        CssShapeOutside::PaddingBox => Some(FloatReferenceBox::PaddingBox),
        CssShapeOutside::ContentBox => Some(FloatReferenceBox::ContentBox),
    }
}

fn block_corner_radius(radius: Radius, em: f32) -> BlockCornerRadius {
    let value = flow_length(radius.0, em);
    BlockCornerRadius {
        horizontal: value,
        vertical: value,
    }
}

fn shape_outside_has_nonlinear_radius(computed: &ComputedValues) -> bool {
    shape_outside_reference_box(computed).is_some()
        && [
            computed.border_top_left_radius,
            computed.border_top_right_radius,
            computed.border_bottom_right_radius,
            computed.border_bottom_left_radius,
        ]
        .into_iter()
        .any(|radius| length_has_math(radius.0))
}

fn block_size_value(value: CssSize, em: f32) -> BlockSizeValue {
    match value {
        CssSize::Auto => BlockSizeValue::Auto,
        CssSize::None => BlockSizeValue::None,
        CssSize::MinContent => BlockSizeValue::MinContent,
        CssSize::MaxContent => BlockSizeValue::MaxContent,
        CssSize::FitContent(value) => BlockSizeValue::FitContent(flow_length(value, em)),
        CssSize::Value(value) => BlockSizeValue::Length(flow_length(value, em)),
    }
}

fn block_margin(value: Margin, em: f32) -> FlowLengthAuto {
    match value {
        Margin::Auto => FlowLengthAuto::Auto,
        Margin::Value(value) => FlowLengthAuto::Value(flow_length(value, em)),
    }
}

fn block_inset(value: Inset, em: f32) -> FlowLengthAuto {
    match value {
        Inset::Auto => FlowLengthAuto::Auto,
        Inset::Value(value) => FlowLengthAuto::Value(flow_length(value, em)),
    }
}

fn flow_length(value: CssLengthPercentage, em: f32) -> FlowLength {
    let px = absolute_length_percentage(value, em, 16.0, 0.0);
    let with_unit_basis = absolute_length_percentage(value, em, 16.0, 1.0);
    FlowLength {
        px,
        percentage: with_unit_basis - px,
    }
}

fn block_style_has_nonlinear_lengths(computed: &ComputedValues) -> bool {
    let size_has_math = |size| match size {
        CssSize::FitContent(value) | CssSize::Value(value) => length_has_math(value),
        CssSize::Auto | CssSize::None | CssSize::MinContent | CssSize::MaxContent => false,
    };
    let margin_has_math = |margin| match margin {
        Margin::Value(value) => length_has_math(value),
        Margin::Auto => false,
    };

    [
        computed.width,
        computed.height,
        computed.min_width,
        computed.min_height,
        computed.max_width,
        computed.max_height,
    ]
    .into_iter()
    .any(size_has_math)
        || [
            computed.margin_top,
            computed.margin_right,
            computed.margin_bottom,
            computed.margin_left,
        ]
        .into_iter()
        .any(margin_has_math)
        || [
            computed.padding_top.0,
            computed.padding_right.0,
            computed.padding_bottom.0,
            computed.padding_left.0,
        ]
        .into_iter()
        .any(length_has_math)
}

fn length_has_math(value: CssLengthPercentage) -> bool {
    matches!(value, CssLengthPercentage::Math(_))
}

fn supports_nested_float_state<Id>(
    css_box: &CssBox<Id>,
    block_style: BlockStyle,
    kind: AlgorithmKind,
) -> bool {
    // A relative block remains in normal flow. Livery translates its retained
    // fragment subtree only after Buckram has resolved the shared float state.
    kind == AlgorithmKind::Block
        && css_box.display.outside == Some(DisplayOutside::Block)
        && css_box.display.internal_table.is_none()
        && !block_style.establishes_bfc
        && matches!(
            block_style.position,
            BuckramBlockPosition::Static | BuckramBlockPosition::Relative
        )
        && block_style.float == FloatSide::None
        && !block_style.replaced
        // Buckram mirrors exclusions across horizontal direction changes.
        // Vertical writing modes still need a full axis transform.
        && (block_style.flow == block_style.containing_flow
            || block_style.flow.is_horizontal() && block_style.containing_flow.is_horizontal())
}

fn supports_float_avoidance<Id>(
    css_box: &CssBox<Id>,
    block_style: BlockStyle,
    kind: AlgorithmKind,
) -> bool {
    matches!(
        kind,
        AlgorithmKind::Leaf | AlgorithmKind::Block | AlgorithmKind::Flex | AlgorithmKind::Grid
    ) && (css_box.display.outside == Some(DisplayOutside::Block)
        || (css_box.display.outside == Some(DisplayOutside::Inline) && block_style.shrink_to_fit))
        && matches!(
            css_box.display.inside,
            Some(
                DisplayInside::Flow
                    | DisplayInside::FlowRoot
                    | DisplayInside::Flex
                    | DisplayInside::Grid
            ) | None
        )
        && css_box.display.internal_table.is_none()
        && block_style.establishes_bfc
        && block_style.position == BuckramBlockPosition::Static
        && block_style.float == FloatSide::None
        && !block_style.replaced
        && block_style.flow.is_horizontal()
        && block_style.containing_flow.is_horizontal()
}

fn supports_intrinsic_shrink_to_fit<Id, Context, Source>(
    tree: &AlgorithmTree<Style, Context, Source>,
    node: AlgorithmNodeId,
    boxes: &GeneratedBoxTree<Id>,
    box_id: BoxId,
    computed: &ComputedValues,
    block_style: BlockStyle,
    kind: AlgorithmKind,
) -> bool {
    let css_box = &boxes[box_id];
    let float_root = css_box.display.outside == Some(DisplayOutside::Block)
        && block_style.float != FloatSide::None;
    let atomic_inline_root = css_box.display.outside == Some(DisplayOutside::Inline)
        && block_style.float == FloatSide::None;
    // A flex item is blockified before its contents are formatted. It must
    // not enter the atomic inline shrink-to-fit lane merely because its
    // authored outside display was inline (for example, a canvas flex item).
    let direct_flex_item = css_box
        .parent()
        .is_some_and(|parent| boxes[parent].display.inside == Some(DisplayInside::Flex));
    matches!(
        kind,
        AlgorithmKind::Block | AlgorithmKind::Leaf | AlgorithmKind::Flex | AlgorithmKind::Grid
    ) && matches!(
        css_box.display.inside,
        Some(
            DisplayInside::Flow
                | DisplayInside::FlowRoot
                | DisplayInside::Flex
                | DisplayInside::Grid
        )
    ) && css_box.display.internal_table.is_none()
        && block_style.position == BuckramBlockPosition::Static
        && block_style.shrink_to_fit
        && computed.vertical_align == VerticalAlign::Baseline
        && block_style.flow.is_horizontal()
        && block_style.containing_flow.is_horizontal()
        && (float_root || (atomic_inline_root && !direct_flex_item))
        && tree.supports_intrinsic_shrink_to_fit(node)
}

fn algorithm_kind<Id>(css_box: &CssBox<Id>, leaf: bool) -> AlgorithmKind {
    if leaf {
        return AlgorithmKind::Leaf;
    }
    match (css_box.formatting_context, css_box.display.internal_table) {
        (_, Some(InternalTableRole::Grid)) => AlgorithmKind::Table,
        (Some(FormattingContextKind::Flex), _) => AlgorithmKind::Flex,
        (Some(FormattingContextKind::Grid), _) => AlgorithmKind::Grid,
        _ => AlgorithmKind::Block,
    }
}

/// The grid box a table wrapper splits its computed values with.
fn wrapped_table_grid<Id>(boxes: &GeneratedBoxTree<Id>, wrapper: BoxId) -> Option<BoxId>
where
    Id: Copy + Eq + Hash,
{
    boxes[wrapper]
        .children()
        .iter()
        .copied()
        .find(|child| boxes[*child].display.internal_table == Some(InternalTableRole::Grid))
}

/// Anonymous tables inherit inheritable values from their parent and take
/// initial values for everything else.
fn anonymous_table_style(inherited: Option<&ComputedValues>) -> ComputedValues {
    let mut style = inherited.map(ComputedValues::for_child).unwrap_or_default();
    style.display = CssDisplay::Table;
    style
}

/// The wrapper's width under CSS Tables 3 section 2.2.1: "the width of the
/// table wrapper box is the border-edge width of the table grid box inside
/// it."
///
/// A definite table width makes that computable before layout, which is what
/// stops a wrapper from stretching when it is a flex or grid item - it is
/// never `auto` in the sense stretching means, because the grid decides it.
/// K4e2 resolves the `auto` case from Buckram's own table sizing instead of
/// guessing at it, but that runs after the tree is built, and a flex item's
/// style has to be right before it.
fn wrapper_width_from_grid(grid: &Style) -> Option<Dimension> {
    let width = grid.size.width.into_option()?;
    if grid.box_sizing == BoxSizing::BorderBox {
        return Some(Dimension::length(width));
    }
    let outside = [
        grid.padding.left,
        grid.padding.right,
        grid.border.left,
        grid.border.right,
    ]
    .into_iter()
    .map(|edge| Dimension::from(edge).into_option())
    .sum::<Option<f32>>()?;
    Some(Dimension::length(width + outside))
}

/// The wrapper's children in the order they are laid out.
///
/// CSS 2.1 section 17.4.1 puts a caption above or below the table grid inside
/// the wrapper's margins, according to `caption-side`. Buckram's box tree
/// keeps every caption before the grid, which is the order the source implies;
/// this is the order the page shows. Within a side, source order is kept, so
/// two top captions stack in the order they were written.
fn wrapper_children_in_caption_order<Id>(
    boxes: &GeneratedBoxTree<Id>,
    styles: &StylePlane<Id>,
    wrapper: BoxId,
) -> Vec<BoxId>
where
    Id: Copy + Eq + Hash,
{
    let below = |child: BoxId| {
        boxes[child].display.internal_table == Some(InternalTableRole::Caption)
            && boxes
                .origin_node(child)
                .and_then(|node| styles.get(node))
                .is_some_and(|computed| computed.caption_side == CaptionSide::Bottom)
    };
    let children = boxes[wrapper].children();
    children
        .iter()
        .copied()
        .filter(|child| !below(*child))
        .chain(children.iter().copied().filter(|child| below(*child)))
        .collect()
}

/// Taffy's block algorithm stacks physical top to bottom. A table wrapper is
/// responsible for caption order, so a vertical writing mode must select the
/// matching physical main axis before the generic backend sees its children.
/// The grid remains a distinct child and still owns table tracks; this is not
/// a table-row projection or a fragmentation rule.
fn wrapper_uses_logical_block_axis(style: &mut Style, flow: FlowAxes) -> bool {
    style.flex_direction = match flow.block_start() {
        PhysicalSide::Top => return false,
        PhysicalSide::Bottom => FlexDirection::ColumnReverse,
        PhysicalSide::Left => FlexDirection::Row,
        PhysicalSide::Right => FlexDirection::RowReverse,
    };
    style.display = Display::Flex;
    true
}

/// The horizontal margins a caption carries into its contribution.
///
/// C5 of the K4e1 interop matrix pins that both engines include them: a
/// 176-wide caption with `margin-left: 30px` puts a floor of 206 under the
/// table, not 176.
fn caption_horizontal_margins(computed: &ComputedValues, em: f32, basis: Option<f32>) -> f32 {
    let resolve = |margin: Margin| match margin {
        // An auto margin on a caption resolves against a width the table does
        // not have yet, and neither engine lets it widen the table.
        Margin::Auto => 0.0,
        Margin::Value(value) => {
            absolute_length_percentage(value, em, LIVE_ROOT_FONT_SIZE, basis.unwrap_or(0.0))
        },
    };
    resolve(computed.margin_left) + resolve(computed.margin_right)
}

/// Whether a wrapper needs the `float: left` shrink-to-fit compatibility route.
///
/// Only a block-level in-flow wrapper does. The wrapper of an inline-table is
/// an atomic inline, an absolutely positioned wrapper is shrink-to-fit under
/// CSS 2.1 section 10.3.7, and a flex or grid item is sized by its container -
/// all three already shrink-wrap, and floating them instead would take them
/// out of the formatting context they belong to.
fn wrapper_needs_float_fallback<Id>(
    boxes: &GeneratedBoxTree<Id>,
    wrapper: BoxId,
    style: &Style,
) -> bool
where
    Id: Copy + Eq + Hash,
{
    let laid_out_as_an_item = boxes[wrapper].parent().is_some_and(|parent| {
        matches!(
            boxes[parent].formatting_context,
            Some(FormattingContextKind::Flex | FormattingContextKind::Grid)
        )
    });
    boxes[wrapper].display.outside == Some(DisplayOutside::Block)
        && !laid_out_as_an_item
        && style.position != Position::Absolute
        && style.float == TaffyFloat::None
}

fn anonymous_taffy_style<Id>(css_box: &CssBox<Id>) -> Style {
    let display = match (css_box.formatting_context, css_box.display.internal_table) {
        (Some(FormattingContextKind::Flex), _) => Display::Flex,
        (Some(FormattingContextKind::Grid), _) => Display::Grid,
        _ => Display::Block,
    };
    Style {
        display,
        ..Style::default()
    }
}

fn legacy_origin_node<Id>(boxes: &GeneratedBoxTree<Id>, box_id: BoxId) -> Option<Id>
where
    Id: Copy + Eq + Hash,
{
    match boxes[box_id].origin {
        BoxOrigin::Element(node) if boxes.principal_box(node) == Some(box_id) => Some(node),
        BoxOrigin::Text(node) => Some(node),
        BoxOrigin::Element(_) | BoxOrigin::Pseudo { .. } | BoxOrigin::Anonymous { .. } => None,
    }
}

/// Record flow-relative fragment geometry without changing the physical
/// rectangle consumed by the retained text and paint lanes.
fn fragment_for_box<Id>(
    boxes: &GeneratedBoxTree<Id>,
    box_id: BoxId,
    physical: Fragment,
    relative: Fragment,
    containing: Fragment,
) -> TreeFragment
where
    Id: Copy + Eq + Hash,
{
    let flow = boxes[box_id].flow;
    if flow.is_horizontal() {
        TreeFragment::from_horizontal_physical(box_id, physical)
    } else {
        let logical = flow.logical_rect(
            relative,
            PhysicalSize {
                width: containing.width,
                height: containing.height,
            },
        );
        TreeFragment::from_physical_with_logical(box_id, physical, logical, flow)
    }
}

/// The structural fragments Buckram emitted for each live table, keyed by the
/// grid box that owns them.
type TableFragmentPlane = HashMap<BoxId, TableFragments>;

fn collect_fragments<Id>(
    tree: &AlgorithmTree<Style, TextMeasure, Option<BoxId>>,
    boxes: &GeneratedBoxTree<Id>,
    node: AlgorithmNodeId,
    cursor: FragmentCursor,
    tables: &TableFragmentPlane,
    output: &mut FragmentOutput<'_>,
) -> Result<(), LayoutError>
where
    Id: Copy + Eq + Hash,
{
    let computed = tree.layout(node);
    let static_computed = tree.static_layout(node);
    let origin = Point {
        x: cursor.origin.x + computed.x,
        y: cursor.origin.y + computed.y,
    };
    let rect = Fragment {
        x: origin.x,
        y: origin.y,
        width: computed.width,
        height: computed.height,
    };
    let mut child_parent = cursor.parent;
    {
        let source = *tree.source(node);
        let origin_node = match source {
            Some(box_id) => {
                if let Some(existing) = output
                    .fragments
                    .fragment_ids_for_box(box_id)
                    .last()
                    .copied()
                {
                    child_parent = Some(existing);
                } else {
                    let structural_parent = boxes[box_id].parent().and_then(|parent_box| {
                        output
                            .fragments
                            .fragment_ids_for_box(parent_box)
                            .last()
                            .copied()
                    });
                    let parent = structural_parent.or(cursor.parent);
                    let flow = boxes[box_id].flow;
                    let static_rect = flow.logical_rect(
                        PhysicalRect {
                            x: static_computed.x,
                            y: static_computed.y,
                            width: static_computed.width,
                            height: static_computed.height,
                        },
                        PhysicalSize {
                            width: cursor.containing.width,
                            height: cursor.containing.height,
                        },
                    );
                    let fragment = if flow.is_horizontal() {
                        TreeFragment::from_horizontal_physical(box_id, rect)
                    } else {
                        let logical_rect = flow.logical_rect(
                            PhysicalRect {
                                x: computed.x,
                                y: computed.y,
                                width: computed.width,
                                height: computed.height,
                            },
                            PhysicalSize {
                                width: cursor.containing.width,
                                height: cursor.containing.height,
                            },
                        );
                        TreeFragment::from_physical_with_logical(box_id, rect, logical_rect, flow)
                    };
                    record_static_position(
                        boxes,
                        box_id,
                        parent,
                        static_rect,
                        tree.grid_positioned_area(node),
                        output,
                    );
                    let id = output.fragments.push(
                        fragment
                            .with_baselines(fragment_baselines(tree, boxes, node, box_id, rect)),
                        parent,
                        parent,
                    );
                    child_parent = Some(id);
                    if let Some(emitted) = tables.get(&box_id) {
                        commit_table_structure(emitted, origin, id, boxes, output);
                    }
                }
                legacy_origin_node(boxes, box_id)
            },
            None => None,
        };
        let _ = origin_node;
    }
    for child in tree.children(node) {
        collect_fragments(
            tree,
            boxes,
            *child,
            FragmentCursor {
                origin,
                containing: rect,
                parent: child_parent,
            },
            tables,
            output,
        )?;
    }
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "one recursive walk; six of the eight are context invariant across               the whole walk and the other two are the per-node cursor"
)]
fn collect_inline_fragments<Id>(
    tree: &AlgorithmTree<Style, InlineMeasure, Vec<BoxId>>,
    boxes: &GeneratedBoxTree<Id>,
    node: AlgorithmNodeId,
    cursor: FragmentCursor,
    tables: &TableFragmentPlane,
    output: &mut FragmentOutput<'_>,
    text_frame: &mut TextFrame<Id>,
    styles: &StylePlane<Id>,
) -> Result<(), LayoutError>
where
    Id: Copy + Eq + Hash,
{
    let computed = tree.layout(node);
    let static_computed = tree.static_layout(node);
    let rounded_rect = Fragment {
        x: cursor.origin.x + computed.x,
        y: cursor.origin.y + computed.y,
        width: computed.width,
        height: computed.height,
    };
    // K4d's structural table fragments retain Buckram's subpixel cell
    // positions. Taffy's final rounding is still the ordinary algorithm-tree
    // geometry, but using it as the origin for cell descendants accumulates a
    // pixel of text drift across sibling columns. Descendants of an emitted
    // cell therefore start from the authoritative structural rectangle that
    // already exists in the fragment tree.
    let rect = tree
        .source(node)
        .iter()
        .find(|box_id| boxes[**box_id].display.internal_table == Some(InternalTableRole::Cell))
        .and_then(|box_id| {
            output
                .fragments
                .fragment_ids_for_box(*box_id)
                .last()
                .and_then(|id| output.fragments.get(*id))
                .map(TreeFragment::physical_rect)
        })
        .unwrap_or(rounded_rect);
    let origin = Point {
        x: rect.x,
        y: rect.y,
    };
    let relative_rect = Fragment {
        x: rect.x - cursor.origin.x,
        y: rect.y - cursor.origin.y,
        width: rect.width,
        height: rect.height,
    };
    let placement = if let Some(context) = tree.context(node)
        && let Some(layout) = context.layout_for_width(computed.width)
    {
        Some(layout.place(
            text_frame,
            styles,
            |box_id| boxes.origin_node(box_id),
            (origin.x, origin.y),
            computed.width,
        ))
    } else {
        None
    };
    let mut child_parent = cursor.parent;
    {
        let mut source_ids = tree.source(node).clone();
        if let Some(placement) = &placement {
            source_ids.extend(placement.fragments.keys().copied());
        }
        source_ids.sort_unstable();
        source_ids.dedup();
        for box_id in source_ids {
            if let Some(existing) = output
                .fragments
                .fragment_ids_for_box(box_id)
                .last()
                .copied()
            {
                child_parent.get_or_insert(existing);
                continue;
            }
            let structural_parent = boxes[box_id].parent().and_then(|parent_box| {
                output
                    .fragments
                    .fragment_ids_for_box(parent_box)
                    .last()
                    .copied()
            });
            let parent = structural_parent.or(cursor.parent);
            let line_fragments = placement
                .as_ref()
                .and_then(|placement| placement.fragments.get(&box_id))
                .filter(|fragments| !fragments.is_empty());
            if let Some(line_fragments) = line_fragments {
                for line_fragment in line_fragments {
                    let relative_line = Fragment {
                        x: line_fragment.x - cursor.origin.x,
                        y: line_fragment.y - cursor.origin.y,
                        width: line_fragment.width,
                        height: line_fragment.height,
                    };
                    let flow = boxes[box_id].flow;
                    let static_rect = flow.logical_rect(
                        relative_line,
                        PhysicalSize {
                            width: cursor.containing.width,
                            height: cursor.containing.height,
                        },
                    );
                    record_static_position(boxes, box_id, parent, static_rect, None, output);
                    let fragment_id = output.fragments.push(
                        fragment_for_box(
                            boxes,
                            box_id,
                            *line_fragment,
                            relative_line,
                            cursor.containing,
                        )
                        .with_baselines(fragment_baselines(
                            tree,
                            boxes,
                            node,
                            box_id,
                            *line_fragment,
                        )),
                        parent,
                        parent,
                    );
                    child_parent.get_or_insert(fragment_id);
                }
            } else {
                let flow = boxes[box_id].flow;
                let static_rect = flow.logical_rect(
                    Fragment {
                        x: static_computed.x,
                        y: static_computed.y,
                        width: static_computed.width,
                        height: static_computed.height,
                    },
                    PhysicalSize {
                        width: cursor.containing.width,
                        height: cursor.containing.height,
                    },
                );
                record_static_position(
                    boxes,
                    box_id,
                    parent,
                    static_rect,
                    tree.grid_positioned_area(node),
                    output,
                );
                let fragment_id = output.fragments.push(
                    fragment_for_box(boxes, box_id, rect, relative_rect, cursor.containing)
                        .with_baselines(fragment_baselines(tree, boxes, node, box_id, rect)),
                    parent,
                    parent,
                );
                if let Some(emitted) = tables.get(&box_id) {
                    commit_table_structure(emitted, origin, fragment_id, boxes, output);
                }
                child_parent.get_or_insert(fragment_id);
            }
        }
    }
    for child in tree.children(node) {
        collect_inline_fragments(
            tree,
            boxes,
            *child,
            FragmentCursor {
                origin,
                containing: rect,
                parent: child_parent,
            },
            tables,
            output,
            text_frame,
            styles,
        )?;
    }
    Ok(())
}

/// Normalize HTML table attributes at the Livery boundary, then ask Buckram
/// to derive one topology from the generated CSS boxes. CSS-display tables
/// receive the same model with the default one-by-one spans.
fn build_table_grid<D>(boxes: &GeneratedBoxTree<D::NodeId>, dom: &D, grid: BoxId) -> TableGrid
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    fn visit<D>(
        boxes: &GeneratedBoxTree<D::NodeId>,
        dom: &D,
        box_id: BoxId,
        inputs: &mut TableGridInputs,
    ) where
        D: LayoutDom,
        D::NodeId: Copy + Eq + Hash,
    {
        if let Some(node) = boxes.origin_node(box_id) {
            match boxes[box_id].display.internal_table {
                Some(InternalTableRole::Cell) if html_element(dom, node, &["td", "th"]) => {
                    inputs.set_cell(box_id, html_cell_input(dom, node));
                },
                Some(InternalTableRole::Column) if html_element(dom, node, &["col"]) => {
                    inputs.set_column(
                        box_id,
                        TableTrackInput {
                            span: html_limited_span(html_attribute(dom, node, "span"), 1_000),
                        },
                    );
                },
                Some(InternalTableRole::ColumnGroup) if html_element(dom, node, &["colgroup"]) => {
                    inputs.set_column_group(
                        box_id,
                        TableTrackInput {
                            span: html_limited_span(html_attribute(dom, node, "span"), 1_000),
                        },
                    );
                },
                _ => {},
            }
        }
        for child in boxes[box_id].children() {
            visit(boxes, dom, *child, inputs);
        }
    }

    let mut inputs = TableGridInputs::default();
    visit(boxes, dom, grid, &mut inputs);
    TableGrid::from_box_tree(&**boxes, grid, &inputs)
}

fn html_element<D>(dom: &D, node: D::NodeId, names: &[&str]) -> bool
where
    D: LayoutDom,
{
    dom.element_name(node).is_some_and(|name| {
        name.ns.as_ref() == "http://www.w3.org/1999/xhtml"
            && names
                .iter()
                .any(|candidate| name.local.as_ref().eq_ignore_ascii_case(candidate))
    })
}

fn html_attribute<'dom, D>(dom: &'dom D, node: D::NodeId, local: &str) -> Option<&'dom str>
where
    D: LayoutDom,
{
    dom.attribute(node, &Namespace::from(""), &LocalName::from(local))
}

fn html_limited_span(value: Option<&str>, maximum: usize) -> usize {
    value
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .map(|value| value.min(maximum))
        .unwrap_or(1)
}

fn html_cell_input<D>(dom: &D, node: D::NodeId) -> TableCellInput
where
    D: LayoutDom,
{
    let row_span = match html_attribute(dom, node, "rowspan")
        .and_then(|value| value.trim().parse::<usize>().ok())
    {
        Some(0) => TableRowSpan::ToEndOfGroup,
        Some(value) => TableRowSpan::Count(value.min(65_534)),
        None => TableRowSpan::Count(1),
    };
    TableCellInput {
        column_span: html_limited_span(html_attribute(dom, node, "colspan"), 1_000),
        row_span,
    }
}

/// Whether every character is CSS collapsible white space.
///
/// CSS collapsible white space is exactly space, tab, line feed, carriage
/// return, and form feed (css-text-3 section 3). It is deliberately *not*
/// Rust's `char::is_whitespace`, which also matches U+00A0 no-break space
/// and the other Unicode spaces. Those generate content: `&nbsp;` is the
/// standard way a test forces a line box to exist, so trimming it away
/// silently deletes the line.
fn is_collapsible_whitespace(text: &str) -> bool {
    text.chars()
        .all(|ch| matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{c}'))
}

fn is_atomic_inline_box<D>(dom: &D, styles: &StylePlane<D::NodeId>, id: D::NodeId) -> bool
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    styles.get(id).is_some_and(|style| {
        style.display == CssDisplay::InlineBlock
            // K4e4: an inline-table is an atomic inline like an inline-block;
            // its wrapper occupies line space as a unit.
            || style.display == CssDisplay::InlineTable
            || (style.display == CssDisplay::Inline && is_replaced_element(dom, id))
            // CSS 2.1 17.2.1: a replaced element given an internal table
            // display is demoted to inline by box generation, so it is an
            // atomic inline here too. Reading the computed value alone would
            // leave it laid out as an inline container and stretched.
            || (is_replaced_element(dom, id)
                && crate::box_tree::is_internal_table_display(style.display))
    })
}

fn has_atomic_inline_ancestor<D>(
    dom: &D,
    styles: &StylePlane<D::NodeId>,
    boxes: &GeneratedBoxTree<D::NodeId>,
    id: D::NodeId,
) -> bool
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let mut ancestor = dom.parent(id);
    while let Some(candidate) = ancestor {
        if boxes
            .principal_box(candidate)
            .is_some_and(|box_id| boxes[box_id].display.outside == Some(DisplayOutside::Inline))
            && is_atomic_inline_box(dom, styles, candidate)
        {
            return true;
        }
        ancestor = dom.parent(candidate);
    }
    false
}

fn is_replaced_element<D>(dom: &D, id: D::NodeId) -> bool
where
    D: LayoutDom,
    D::NodeId: Copy,
{
    dom.kind(id) == NodeKind::Element
        && dom.element_name(id).is_some_and(|name| {
            name.local.as_ref().eq_ignore_ascii_case("img")
                || name.local.as_ref().eq_ignore_ascii_case("canvas")
        })
}

/// Whether the nearest non-anonymous ancestor establishes a flex or grid
/// formatting context, in which `auto` sizing may stretch. Anonymous boxes
/// are skipped: an inline-level image in a grid container sits inside an
/// anonymous grid item, and it is the container's context that governs.
fn stretched_by_ancestor_context<Id>(boxes: &GeneratedBoxTree<Id>, box_id: BoxId) -> bool
where
    Id: Copy + Eq + Hash,
{
    let mut parent = boxes[box_id].parent();
    while let Some(id) = parent {
        let css_box = &boxes[id];
        if matches!(css_box.origin, BoxOrigin::Anonymous { .. }) {
            parent = css_box.parent();
            continue;
        }
        return matches!(
            css_box.formatting_context,
            Some(FormattingContextKind::Flex | FormattingContextKind::Grid)
        );
    }
    false
}

// CSS 2.1 10.4: the used size of a replaced element with an intrinsic size
// and ratio whose width and height are both `auto`, once min/max constraints
// apply. Inputs and outputs are CONTENT-BOX lengths; the caller converts a
// border-box constraint by subtracting the box's edges first and adds them
// back to the result.
//
// The table is the spec's, row for row. Verified against all sixty images of
// css-sizing/box-sizing-replaced-001..003 (border-box with padding,
// border-box with padding and border, content-box), every one resolving to
// the 75x75 content its reference expects, before this was ported.
pub(in crate::layout) fn replaced_min_max(
    natural: (f32, f32),
    min_width: Option<f32>,
    max_width: Option<f32>,
    min_height: Option<f32>,
    max_height: Option<f32>,
) -> (f32, f32) {
    let (w, h) = natural;
    let min_w = min_width.unwrap_or(0.0);
    let min_h = min_height.unwrap_or(0.0);
    // A max below its min is treated as that min.
    let max_w = max_width.unwrap_or(f32::INFINITY).max(min_w);
    let max_h = max_height.unwrap_or(f32::INFINITY).max(min_h);
    if w <= 0.0 || h <= 0.0 {
        return (w.clamp(min_w, max_w), h.clamp(min_h, max_h));
    }
    let ratio = w / h;
    match (w > max_w, w < min_w, h > max_h, h < min_h) {
        (true, _, true, _) => {
            if max_w / w <= max_h / h {
                (max_w, (max_w / ratio).max(min_h))
            } else {
                ((max_h * ratio).max(min_w), max_h)
            }
        },
        (_, true, _, true) => {
            if min_w / w <= min_h / h {
                ((min_h * ratio).min(max_w), min_h)
            } else {
                (min_w, (min_w / ratio).min(max_h))
            }
        },
        (_, true, true, _) => (min_w, max_h),
        (true, _, _, true) => (max_w, min_h),
        (true, _, _, _) => (max_w, (max_w / ratio).max(min_h)),
        (_, true, _, _) => (min_w, (min_w / ratio).min(max_h)),
        (_, _, true, _) => ((max_h * ratio).max(min_w), max_h),
        (_, _, _, true) => ((min_h * ratio).min(max_w), min_h),
        _ => (w, h),
    }
}

fn apply_replaced_intrinsic_style<D>(
    style: &mut Style,
    dom: &D,
    id: D::NodeId,
    computed: &ComputedValues,
    image_sources: &ImageSources,
    font_size: f32,
    block_level_flow: bool,
    containing_width: f32,
) -> Option<(f32, f32)>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let intrinsic = replaced_intrinsic_size(dom, id, image_sources);
    let natural_ratio = intrinsic
        .filter(|(width, height)| *width > 0.0 && *height > 0.0)
        .map(|(width, height)| width / height);

    // Attribute-derived dimensions already reached `computed` through the
    // presentational-hint origin. Layout owns only natural-size resolution.
    let width = definite_size(computed.width, font_size);
    let height = definite_size(computed.height, font_size);
    if computed.aspect_ratio.uses_natural_ratio()
        && natural_ratio.is_some()
        && !(width.is_some() && height.is_some())
    {
        // Taffy's aspect-ratio input participates in sizing even when both
        // axes are definite. CSS's natural ratio does not, so only expose it
        // while at least one axis still needs intrinsic resolution.
        style.aspect_ratio = natural_ratio;
    }

    // CSS 2.1 10.3.4: a block-level replaced element in normal flow resolves
    // `width: auto` to its intrinsic width instead of stretching to its
    // containing block, so the natural size has to reach Taffy as a definite
    // length. Without it a `display: block` canvas fills the
    // container and the natural ratio then stretches its height to match,
    // which is how a 100x100 canvas laid out at 200x200 inside a 200px parent.
    //
    // The rule is narrow on purpose; each exclusion below is a reftest that
    // failed when it was wider.
    //
    // - It is 10.3.4, block-level only. An inline replaced element already
    //   contributes its natural box through the inline atomic-root path
    //   (10.3.2), and both halves of a reftest render through this engine,
    //   so touching inline sizing moved the *references*.
    // - A flex or grid item is the other way round: `auto` there means the
    //   item may stretch or grow, and Taffy already feeds the measured
    //   intrinsic size in as the content size.
    // - An author `aspect-ratio` owns the transfer between the axes.
    // - `min-content`, `max-content` and `fit-content` are keywords, not
    //   `auto`; their replaced contribution is the ratio-transferred size.
    // - Any non-auto height with a natural ratio transfers to the width
    //   (10.3.2), so the intrinsic width only stands when nothing else can
    //   decide it. This is `!height_auto`, not "definite": a percentage
    //   height is indefinite to `definite_size` but Taffy still resolves it
    //   against a definite container, and the width must follow that.
    // - Under `box-sizing: border-box` a replaced element with min/max
    //   constraints needs CSS 2.1 10.4's ratio-preserving clamp run in
    //   border-box space. Taffy's leaf measure already does that; a forced
    //   `size` bypasses it (box-sizing-replaced-001..003), so border-box
    //   stays on the measure path. A block-level border-box replaced element
    //   therefore still stretches; that gap is narrower and is left named.
    let width_auto = matches!(computed.width, CssSize::Auto);
    let height_auto = matches!(computed.height, CssSize::Auto);
    let author_ratio = !computed.aspect_ratio.uses_natural_ratio();
    let content_box = matches!(computed.box_sizing, CssBoxSizing::ContentBox);
    if let Some((natural_width, natural_height)) =
        intrinsic.filter(|_| block_level_flow && !author_ratio && content_box)
    {
        if width_auto && natural_width > 0.0 && !(!height_auto && natural_ratio.is_some()) {
            style.size.width = Dimension::length(natural_width);
        }
        if height_auto && natural_ratio.is_none() && natural_height > 0.0 {
            style.size.height = Dimension::length(natural_height);
        }
    }

    // Under `box-sizing: border-box` the box takes the full CSS 2.1 10.4 route
    // here, with both axes resolved and handed to Taffy as definite border-box
    // lengths. Taffy's leaf path is not 10.4: it clamps, then transfers height
    // from width, and forcing only the width bypassed even that -- which is how
    // box-sizing-replaced-001..003 failed twice. Resolving the whole table in
    // content space and adding the edges back keeps every min/max interaction
    // ratio-preserving. The natural ratio is cleared so leaf.rs cannot re-derive
    // a height over a resolved one. Percentage min/max stay with Taffy, as
    // `definite_size` leaves them indefinite; the three reftests use px only.
    if let Some(natural) = intrinsic.filter(|(w, h)| {
        block_level_flow
            && !author_ratio
            && !content_box
            && width_auto
            && height_auto
            && *w > 0.0
            && *h > 0.0
    }) {
        let px = |value: CssLengthPercentage| {
            absolute_length_percentage(value, font_size, 16.0, containing_width)
        };
        let edge_x = px(computed.padding_left.0)
            + px(computed.padding_right.0)
            + border_width_px(
                computed.border_left_style,
                computed.border_left_width,
                font_size,
            )
            + border_width_px(
                computed.border_right_style,
                computed.border_right_width,
                font_size,
            );
        let edge_y = px(computed.padding_top.0)
            + px(computed.padding_bottom.0)
            + border_width_px(
                computed.border_top_style,
                computed.border_top_width,
                font_size,
            )
            + border_width_px(
                computed.border_bottom_style,
                computed.border_bottom_width,
                font_size,
            );
        let content = |size: CssSize, edge: f32| definite_size(size, font_size).map(|v| v - edge);
        let (used_width, used_height) = replaced_min_max(
            natural,
            content(computed.min_width, edge_x),
            content(computed.max_width, edge_x),
            content(computed.min_height, edge_y),
            content(computed.max_height, edge_y),
        );
        style.size.width = Dimension::length(used_width + edge_x);
        style.size.height = Dimension::length(used_height + edge_y);
        style.aspect_ratio = None;
    }
    intrinsic
}

/// Pass natural replaced dimensions across the browser-facing K5d edge.
/// Attribute-derived dimensions already live in the computed style through
/// presentational hints, so this path does not reread DOM width or height.
fn positioned_replaced_input<D>(
    dom: &D,
    id: D::NodeId,
    image_sources: &ImageSources,
    style: &BlockStyle,
) -> Option<buckram::ReplacedSize>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    if !style.replaced {
        return None;
    }
    let intrinsic_size = replaced_intrinsic_size(dom, id, image_sources).map(|(width, height)| {
        style
            .containing_flow
            .logical_size(PhysicalSize { width, height })
    });
    Some(buckram::ReplacedSize { intrinsic_size })
}

fn replaced_intrinsic_size<D>(
    dom: &D,
    id: D::NodeId,
    image_sources: &ImageSources,
) -> Option<(f32, f32)>
where
    D: LayoutDom,
    D::NodeId: Copy,
{
    if dom.kind(id) != NodeKind::Element {
        return None;
    }
    let local = dom.element_name(id)?.local.as_ref().to_ascii_lowercase();
    if local == "canvas" {
        return Some((
            canvas_intrinsic_dimension(dom, id, "width", 300.0),
            canvas_intrinsic_dimension(dom, id, "height", 150.0),
        ));
    }
    if local != "img" {
        return None;
    }
    let source = dom.attributes(id).find_map(|attribute| {
        (attribute.name.ns.as_ref().is_empty()
            && attribute.name.local.as_ref().eq_ignore_ascii_case("src"))
        .then_some(attribute.value)
    })?;
    let bytes = if let Ok(data_url) = data_url::DataUrl::process(source) {
        data_url.decode_to_vec().ok()?.0
    } else {
        image_sources.get(source)?.clone()
    };
    let image = image::load_from_memory(&bytes).ok()?;
    Some((image.width() as f32, image.height() as f32))
}

fn canvas_intrinsic_dimension<D>(dom: &D, id: D::NodeId, attribute: &str, default: f32) -> f32
where
    D: LayoutDom,
    D::NodeId: Copy,
{
    dom.attributes(id)
        .find_map(|candidate| {
            (candidate.name.ns.as_ref().is_empty()
                && candidate
                    .name
                    .local
                    .as_ref()
                    .eq_ignore_ascii_case(attribute))
            .then_some(candidate.value)
        })
        .and_then(crate::presentational_hints::parse_non_negative_integer_px)
        .unwrap_or(default)
}

fn definite_size(size: CssSize, font_size: f32) -> Option<f32> {
    let CssSize::Value(value) = size else {
        return None;
    };
    match value {
        CssLengthPercentage::Length(length) => Some(absolute_length(length, font_size, 16.0)),
        CssLengthPercentage::Calc(calc) if calc.percentage == 0.0 => {
            Some(calc.px + calc.em * font_size + calc.rem * 16.0)
        },
        _ => None,
    }
}

fn collapsed_word_width(text: &str) -> usize {
    let mut maximum = 0;
    let mut current = 0;
    for character in text.chars() {
        if matches!(
            character,
            '\u{0009}' | '\u{000A}' | '\u{000C}' | '\u{000D}' | ' '
        ) {
            maximum = maximum.max(current);
            current = 0;
        } else {
            current += 1;
        }
    }
    maximum.max(current)
}

fn collapsed_text_width(text: &str) -> usize {
    let mut width = 0;
    let mut pending_space = false;
    for character in text.chars() {
        if matches!(
            character,
            '\u{0009}' | '\u{000A}' | '\u{000C}' | '\u{000D}' | ' '
        ) {
            pending_space = width != 0;
        } else {
            if pending_space {
                width += 1;
                pending_space = false;
            }
            width += 1;
        }
    }
    width
}
