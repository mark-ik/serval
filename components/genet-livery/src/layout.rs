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
        Display as CssDisplay, FlexDirection as CssFlexDirection, FlexWrap as CssFlexWrap,
        Float as CssFloat, FontSize, Gap as CssGap, GridAutoFlow as CssGridAutoFlow,
        GridPlacement as CssGridPlacement, GridTemplate as CssGridTemplate,
        GridTrack as CssGridTrack, Inset, Length, LengthPercentage as CssLengthPercentage,
        LineHeight, Margin, Overflow as CssOverflow, Position as CssPosition, Radius,
        RelativeLengthEnvironment, ShapeOutside as CssShapeOutside, Size as CssSize, VerticalAlign,
        WhiteSpaceCollapse,
    },
};
use taffy::{
    geometry::{Line, Point, Rect, Size},
    prelude::{
        Dimension, LengthPercentage, LengthPercentageAuto, auto, fr, length, line, max_content,
        min_content, percent, span,
    },
    style::{
        AlignContent, AlignContentKeyword, AlignItems, AlignItemsKeyword, BoxSizing, Display,
        FlexDirection, FlexWrap, Float as TaffyFloat, GridAutoFlow, GridPlacement,
        GridTemplateComponent, JustifyContent, Overflow, Position, Style,
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
impl<Id> LiveryLayout<Id>
where
    Id: Copy + Eq + Hash,
{
    fn new(
        buckram: LayoutResult<Id>,
        text_frame: Option<TextFrame<Id>>,
        block_algorithms: BlockAlgorithmCounts,
        table_paint: TablePaintPlane,
        table_shadow: TableShadowLedger,
    ) -> Self {
        Self {
            buckram,
            text_frame,
            block_algorithms,
            table_paint,
            table_shadow,
        }
    }

    pub fn buckram(&self) -> &LayoutResult<Id> {
        &self.buckram
    }

    pub fn boxes(&self) -> &buckram::CssBoxTree<Id> {
        self.buckram.boxes()
    }

    pub fn fragments(&self) -> &FragmentTree {
        self.buckram.fragments()
    }

    /// Retain stable Buckram identities across a freshly recomputed layout.
    /// Geometry, text shaping, and paint inputs remain from the new pass.
    pub(crate) fn reconcile_identifiers(&mut self, previous: &Self) {
        let identities = self.buckram.reconcile_identifiers(&previous.buckram);
        self.table_paint.remap_box_ids(&identities);
        self.table_shadow
            .remap_box_ids(|box_id| identities.box_id(box_id));
    }

    /// Every DOM source attached to the selected generated-box subtree. The
    /// retained text frame needs this old set as well as the final DOM set so
    /// a removed text node cannot retain a prepared run or selection cluster.
    pub(crate) fn generated_subtree_nodes(&self, node: Id) -> HashSet<Id> {
        fn visit<Id>(boxes: &buckram::CssBoxTree<Id>, box_id: BoxId, nodes: &mut HashSet<Id>)
        where
            Id: Copy + Eq + Hash,
        {
            if let Some(node) = boxes.origin_node(box_id) {
                nodes.insert(node);
            }
            for child in boxes[box_id].children() {
                visit(boxes, *child, nodes);
            }
        }

        let mut nodes = HashSet::new();
        if let Some(root) = self.buckram.boxes().principal_box(node) {
            visit(self.buckram.boxes(), root, &mut nodes);
        }
        nodes
    }

    /// Publish one freshly formatted, reconciled flex or grid root into this
    /// retained layout. Its root box must retain identity, but descendants
    /// may gain or retire boxes; the fresh box tree replaces node ownership
    /// only after the fragment splice has accepted that compatible root.
    ///
    /// The fragment splice preserves the selected root identity but gives its
    /// descendants fresh identities. Fresh text and table planes accompany it
    /// as one publication unit, so paint cannot read a stale side model.
    pub(crate) fn replace_reconciled_formatting_subtree_from(
        &mut self,
        fresh: &Self,
        node: Id,
    ) -> bool {
        let Some(root_box) = self
            .buckram
            .boxes()
            .boxes_for_node(node)
            .iter()
            .copied()
            .find(|box_id| {
                matches!(
                    self.buckram.boxes()[*box_id].formatting_context,
                    Some(FormattingContextKind::Flex | FormattingContextKind::Grid)
                )
            })
        else {
            return false;
        };
        let Some(fresh_root_box) = fresh
            .buckram
            .boxes()
            .boxes_for_node(node)
            .iter()
            .copied()
            .find(|box_id| *box_id == root_box)
        else {
            return false;
        };
        let root = match self.buckram.fragments().fragment_ids_for_box(root_box) {
            [root] => *root,
            _ => return false,
        };
        let fresh_root = match fresh
            .buckram
            .fragments()
            .fragment_ids_for_box(fresh_root_box)
        {
            [root] => *root,
            _ => return false,
        };
        if self
            .buckram
            .fragments_mut()
            .replace_subtree(root, fresh.buckram.fragments(), fresh_root)
            .is_none()
        {
            return false;
        }
        self.buckram.replace_box_tree(fresh.buckram.boxes().clone());
        self.text_frame = fresh.text_frame.clone();
        self.block_algorithms = fresh.block_algorithms;
        self.table_paint = fresh.table_paint.clone();
        self.table_shadow = fresh.table_shadow.clone();
        true
    }

    /// Publish a local formatting result. Unlike the complete publication
    /// route, `fresh` contains one selected subtree only, so table planes
    /// outside that subtree stay authoritative while its text frame replaces
    /// only the selected DOM sources.
    pub(crate) fn replace_reconciled_local_formatting_subtree_from(
        &mut self,
        fresh: &Self,
        node: Id,
        replaced_nodes: &HashSet<Id>,
        dom_text_order: &[Id],
    ) -> bool {
        let Some(root_box) = retained_root_box(self.buckram.boxes(), node) else {
            return false;
        };
        let Some(fresh_root_box) = retained_root_box(fresh.buckram.boxes(), node) else {
            return false;
        };
        if fresh_root_box != root_box {
            return false;
        }
        let table_root = self.buckram.boxes()[root_box].display.internal_table
            == Some(InternalTableRole::Wrapper);
        if table_root
            && (self.table_paint.tables.is_empty()
                || fresh.table_paint.tables.is_empty()
                || !self
                    .table_paint
                    .tables
                    .keys()
                    .any(|grid| box_is_descendant_of(self.buckram.boxes(), *grid, root_box))
                || !fresh
                    .table_paint
                    .tables
                    .keys()
                    .all(|grid| box_is_descendant_of(fresh.buckram.boxes(), *grid, fresh_root_box))
                || !fresh
                    .table_paint
                    .tables
                    .keys()
                    .any(|grid| box_is_descendant_of(fresh.buckram.boxes(), *grid, fresh_root_box))
                || !self.table_shadow.can_replace_subtree(
                    &fresh.table_shadow,
                    self.buckram.boxes(),
                    fresh.buckram.boxes(),
                    root_box,
                    fresh_root_box,
                ))
        {
            return false;
        }
        let root = match self.buckram.fragments().fragment_ids_for_box(root_box) {
            [root] => *root,
            _ => return false,
        };
        let fresh_root = match fresh
            .buckram
            .fragments()
            .fragment_ids_for_box(fresh_root_box)
        {
            [root] => *root,
            _ => return false,
        };
        if self.text_frame.is_none() || fresh.text_frame.is_none() {
            return false;
        }
        if self
            .buckram
            .fragments_mut()
            .replace_subtree(root, fresh.buckram.fragments(), fresh_root)
            .is_none()
        {
            return false;
        }
        if table_root {
            self.table_paint.replace_subtree_from(
                &fresh.table_paint,
                self.buckram.boxes(),
                fresh.buckram.boxes(),
                root_box,
                fresh_root_box,
            );
            self.table_shadow.replace_subtree_from(
                &fresh.table_shadow,
                self.buckram.boxes(),
                fresh.buckram.boxes(),
                root_box,
                fresh_root_box,
            );
        }
        self.buckram.replace_box_tree(fresh.buckram.boxes().clone());
        self.text_frame
            .as_mut()
            .expect("a checked retained text frame is present")
            .replace_subtree_from(
                fresh
                    .text_frame
                    .as_ref()
                    .expect("a checked fresh text frame is present"),
                replaced_nodes,
                dom_text_order,
            );
        true
    }

    /// Publish a disjoint K5h damage set as one retained-layout update. A
    /// failed root leaves `self` untouched, so callers can safely fall back
    /// to the complete fresh result without exposing a partial publication.
    pub(crate) fn replace_reconciled_formatting_subtrees_from(
        &mut self,
        fresh: &Self,
        roots: &[Id],
    ) -> bool {
        if roots.is_empty() {
            return false;
        }
        let mut replacement = self.clone();
        for root in roots {
            if !replacement.replace_reconciled_formatting_subtree_from(fresh, *root) {
                return false;
            }
        }
        *self = replacement;
        true
    }

    /// Apply retained scroll-dependent sticky constraints to this otherwise
    /// normal-flow layout snapshot. Callers clone the static layout first, so
    /// scroll changes never accumulate into the next frame's base geometry.
    pub(crate) fn apply_sticky_positioning(
        &mut self,
        styles: &StylePlane<Id>,
        viewport_width: f32,
        viewport_height: f32,
        mut scrollport_for: impl FnMut(Id) -> Option<StickyScrollport>,
    ) {
        let placements = self
            .buckram
            .boxes()
            .iter()
            .filter_map(|(box_id, css_box)| {
                if css_box.positioning != PositioningScheme::Sticky
                    || css_box
                        .display
                        .internal_table
                        .is_some_and(|role| !supports_retained_sticky_table_part(role))
                {
                    return None;
                }
                let node = css_box.origin.node()?;
                let scrollport = scrollport_for(node)?;
                let root = self
                    .buckram
                    .fragments()
                    .fragment_ids_for_box(box_id)
                    .iter()
                    .copied()
                    .find(|fragment_id| {
                        self.buckram
                            .fragments()
                            .get(*fragment_id)
                            .and_then(TreeFragment::parent)
                            .and_then(|parent| self.buckram.fragments().get(parent))
                            .is_none_or(|parent| parent.box_id() != box_id)
                    })?;
                let current = self.buckram.fragments().get(root)?.physical_rect();
                // A table-internal box's generated parent can be a row or
                // row group that is only as tall as that part. It would clamp
                // a sticky translation to zero. Its sticky containing block
                // is the table wrapper: the nearest block-level table
                // ancestor that owns the table's full scrollable extent.
                let table_wrapper = if css_box.display.internal_table.is_some_and(|role| {
                    role != InternalTableRole::Wrapper && supports_retained_sticky_table_part(role)
                }) {
                    let mut ancestor = css_box.parent();
                    loop {
                        let candidate = ancestor?;
                        if self.buckram.boxes()[candidate].display.internal_table
                            == Some(InternalTableRole::Wrapper)
                        {
                            break Some(candidate);
                        }
                        ancestor = self.buckram.boxes()[candidate].parent();
                    }
                } else {
                    None
                };
                let containing = match table_wrapper
                    .map(ContainingBlock::Box)
                    .unwrap_or(css_box.containing_block)
                {
                    ContainingBlock::Initial => PhysicalRect {
                        x: 0.0,
                        y: 0.0,
                        width: viewport_width,
                        height: viewport_height,
                    },
                    ContainingBlock::Box(containing) => self
                        .buckram
                        .fragments()
                        .fragment_ids_for_box(containing)
                        .first()
                        .and_then(|fragment_id| self.buckram.fragments().get(*fragment_id))
                        .map(TreeFragment::physical_rect)?,
                };
                let computed = styles.get(node)?;
                let computed = if css_box.display.internal_table == Some(InternalTableRole::Wrapper)
                {
                    wrapper_style(computed)
                } else {
                    computed.clone()
                };
                let font_size = font_size_px(&computed.font_size, LIVE_ROOT_FONT_SIZE);
                let style =
                    to_block_style(self.buckram.boxes(), styles, box_id, &computed, font_size);
                let percentage_basis = style
                    .containing_flow
                    .logical_size(PhysicalSize {
                        width: scrollport.rect.width,
                        height: scrollport.rect.height,
                    })
                    .inline;
                Some((
                    root,
                    current,
                    containing,
                    scrollport,
                    style.inset.left.resolve(percentage_basis),
                    style.inset.right.resolve(percentage_basis),
                    style.inset.top.resolve(percentage_basis),
                    style.inset.bottom.resolve(percentage_basis),
                ))
            })
            .collect::<Vec<_>>();

        for (root, current, containing, scrollport, left, right, top, bottom) in placements {
            let x = buckram::solve_sticky_axis(buckram::StickyAxisInput {
                normal_start: current.x,
                box_size: current.width,
                scrollport_start: scrollport.rect.x,
                scrollport_size: scrollport.rect.width,
                scroll_offset: scrollport.offset.x,
                containing_start: containing.x,
                containing_size: containing.width,
                start_inset: left,
                end_inset: right,
            });
            let y = buckram::solve_sticky_axis(buckram::StickyAxisInput {
                normal_start: current.y,
                box_size: current.height,
                scrollport_start: scrollport.rect.y,
                scrollport_size: scrollport.rect.height,
                scroll_offset: scrollport.offset.y,
                containing_start: containing.y,
                containing_size: containing.height,
                start_inset: top,
                end_inset: bottom,
            });
            self.buckram
                .fragments_mut()
                .translate_subtree(root, PhysicalOffset { x, y });
        }
    }

    /// Reposition one retained absolute or fixed fragment subtree when its
    /// computed insets are the only style change and Buckram proves its used
    /// border-box size is unchanged. General dirty-root formatting still
    /// rebuilds; this bounded K5h route owns only the final K5d translation.
    pub(crate) fn reposition_stable_positioned_subtree<D>(
        &mut self,
        dom: &D,
        styles: &StylePlane<D::NodeId>,
        image_sources: &ImageSources,
        node: D::NodeId,
        viewport_width: f32,
        viewport_height: f32,
    ) -> bool
    where
        D: LayoutDom<NodeId = Id>,
        D::NodeId: Copy + Eq + Hash,
    {
        let Some(placement) = self.positioned_placement_for_node(
            dom,
            styles,
            image_sources,
            node,
            viewport_width,
            viewport_height,
        ) else {
            return false;
        };
        let target = placement.target_rect();
        if (target.width - placement.current.width).abs() > 0.001
            || (target.height - placement.current.height).abs() > 0.001
        {
            return false;
        }
        let offset = PhysicalOffset {
            x: placement.containing_rect.x + target.x - placement.current.x,
            y: placement.containing_rect.y + target.y - placement.current.y,
        };
        if offset.x == 0.0 && offset.y == 0.0 {
            return false;
        }
        {
            let fragments = self.buckram.fragments_mut();
            fragments.translate_subtree(placement.root, offset);
            fragments.set_containing_fragment(placement.root, placement.containing_fragment);
        }
        if let Some(text_frame) = self.text_frame.as_mut() {
            text_frame.translate_subtree(dom, node, (offset.x, offset.y));
        }
        true
    }

    /// Resize and reposition one retained absolute or fixed leaf after its
    /// declared width or height changed. The leaf-only precondition prevents
    /// a stale child containing block: any subtree with descendants continues
    /// through the ordinary full-layout path.
    pub(crate) fn resize_positioned_leaf<D>(
        &mut self,
        dom: &D,
        styles: &StylePlane<D::NodeId>,
        image_sources: &ImageSources,
        node: D::NodeId,
        viewport_width: f32,
        viewport_height: f32,
    ) -> bool
    where
        D: LayoutDom<NodeId = Id>,
        D::NodeId: Copy + Eq + Hash,
    {
        let Some(placement) = self.positioned_placement_for_node(
            dom,
            styles,
            image_sources,
            node,
            viewport_width,
            viewport_height,
        ) else {
            return false;
        };
        let target = placement.target_rect();
        let offset = PhysicalOffset {
            x: placement.containing_rect.x + target.x - placement.current.x,
            y: placement.containing_rect.y + target.y - placement.current.y,
        };
        let size_changed = (target.width - placement.current.width).abs() > 0.001
            || (target.height - placement.current.height).abs() > 0.001;
        if size_changed
            && self
                .text_frame
                .as_ref()
                .is_some_and(|text_frame| text_frame.subtree_has_prepared_text(dom, node))
        {
            return false;
        }
        let fragments = self.buckram.fragments_mut();
        if !fragments.resize_leaf(
            placement.root,
            PhysicalSize {
                width: target.width,
                height: target.height,
            },
        ) {
            return false;
        }
        fragments.translate_subtree(placement.root, offset);
        fragments.set_containing_fragment(placement.root, placement.containing_fragment);
        if let Some(text_frame) = self.text_frame.as_mut() {
            text_frame.translate_subtree(dom, node, (offset.x, offset.y));
        }
        true
    }

    fn positioned_placement_for_node<D>(
        &self,
        dom: &D,
        styles: &StylePlane<D::NodeId>,
        image_sources: &ImageSources,
        node: D::NodeId,
        viewport_width: f32,
        viewport_height: f32,
    ) -> Option<PositionedPlacement>
    where
        D: LayoutDom<NodeId = Id>,
        D::NodeId: Copy + Eq + Hash,
    {
        let intrinsic_sizes = HashMap::new();
        let mut placements = positioned_placements(
            self.buckram.fragments(),
            self.buckram.boxes(),
            styles,
            dom,
            image_sources,
            &intrinsic_sizes,
            viewport_width,
            viewport_height,
        )
        .into_iter()
        .filter(|placement| self.buckram.boxes()[placement.box_id].origin.node() == Some(node));
        let placement = placements.next()?;
        (placements.next().is_none()
            && self.buckram.boxes()[placement.box_id]
                .display
                .internal_table
                .is_none()
            && self
                .buckram
                .fragments()
                .fragment_ids_for_box(placement.box_id)
                .len()
                == 1)
            .then_some(placement)
    }

    pub fn fragments_for_node(&self, node: Id) -> impl Iterator<Item = &TreeFragment> {
        self.buckram.fragments_for_node(node)
    }

    pub fn get(&self, node: Id) -> Option<&TreeFragment> {
        self.buckram.get(node)
    }

    /// Compatibility name for callers that only need a node's outermost
    /// retained fragment.
    pub fn rect_of(&self, node: Id) -> Option<&TreeFragment> {
        self.get(node)
    }

    /// A retained caret rectangle in document coordinates.
    pub fn caret_rect(&self, node: Id, byte: usize) -> Option<crate::TextRect> {
        self.text_frame()?
            .caret_rect(node, byte, |_, fragment| crate::TextRect {
                x: fragment.x,
                y: fragment.y,
                width: fragment.width,
                height: fragment.height,
            })
    }

    /// The shaped source position nearest a document point.
    pub fn text_position_at_point(&self, x: f32, y: f32) -> Option<(Id, usize)> {
        self.text_frame()?
            .text_position_at_point(x, y, |_, fragment| crate::TextRect {
                x: fragment.x,
                y: fragment.y,
                width: fragment.width,
                height: fragment.height,
            })
    }

    /// Retained geometry and text for a directed source range.
    pub fn text_selection(&self, range: crate::TextRange<Id>) -> Option<crate::TextSelection<Id>> {
        self.text_frame()?
            .text_selection(range, |_, fragment| crate::TextRect {
                x: fragment.x,
                y: fragment.y,
                width: fragment.width,
                height: fragment.height,
            })
    }

    /// Resolve the first shaped occurrence of `text` to its source range.
    ///
    /// Retained hosts use this to turn find results into ordinary pointer
    /// selection gestures without reaching into Livery's text-frame storage.
    pub fn text_range_for_text(&self, text: &str) -> Option<crate::TextRange<Id>> {
        self.text_frame()?.find_text_range(text)
    }

    /// The node's principal box's fragment: a table element's grid box, which
    /// owns background, borders, and used `width`/`height` under CSS 2.1
    /// section 17.4. Rectangle queries and paint-effect anchors use
    /// [`Self::get`], whose first box is the outermost - the wrapper.
    pub fn principal_fragment(&self, node: Id) -> Option<&TreeFragment> {
        self.buckram.principal_fragment(node)
    }

    pub fn len(&self) -> usize {
        self.buckram.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buckram.is_empty()
    }

    pub fn block_algorithm_counts(&self) -> BlockAlgorithmCounts {
        self.block_algorithms
    }

    /// K4f's retained table paint model. Structural table boxes are emitted by
    /// Buckram, but their background phase cannot be reconstructed from DOM
    /// traversal once row and column boxes have been flattened away.
    pub(crate) fn table_paint_for_node(&self, node: Id) -> Option<&TablePaintModel> {
        self.buckram
            .boxes()
            .boxes_for_node(node)
            .iter()
            .find_map(|box_id| self.table_paint.table(*box_id))
    }

    /// Whether a node's own decoration is painted by the separated-table
    /// phase, rather than the ordinary DOM walk.
    pub(crate) fn table_paint_manages_node(&self, node: Id) -> bool {
        self.buckram
            .boxes()
            .boxes_for_node(node)
            .iter()
            .copied()
            .any(|box_id| self.table_paint.manages(box_id))
    }

    /// A collapsed table's grid and cells retain their normal background
    /// phase, but its generic border command must yield to K4g5's one-winner
    /// segment model.
    pub(crate) fn table_paint_uses_collapsed_borders(&self, node: Id) -> bool {
        self.table_paint_for_node(node)
            .is_some_and(TablePaintModel::is_collapsed)
    }

    /// Whether the node's descendants must clip at the accepted edge of a
    /// cell spanning a collapsed track.
    pub(crate) fn table_cell_requires_clip(&self, node: Id) -> bool {
        self.buckram
            .boxes()
            .boxes_for_node(node)
            .iter()
            .copied()
            .any(|box_id| self.table_paint.clips_cell(box_id))
    }

    /// K4c5a's shadow comparison of Buckram's fixed sizing against the live
    /// path. K4c5b may only make Buckram authoritative once this is silent.
    pub fn table_shadow_ledger(&self) -> &TableShadowLedger {
        &self.table_shadow
    }

    pub(crate) fn text_frame(&self) -> Option<&TextFrame<Id>> {
        self.text_frame.as_ref()
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
        tree: AlgorithmTree::new(),
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
        tree: AlgorithmTree::new(),
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
            tree: AlgorithmTree::new(),
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
        // An admitted atomic inline root needs a containing block so its
        // shrink-to-fit query runs as a child formatting context. Keep the
        // established direct-root path for the deferred cases, whose inline
        // placement may depend on unsupported vertical alignment behavior.
        let root = if state.tree.uses_intrinsic_shrink_to_fit(atomic_root) {
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
        state.tree.compute_layout_with_measure(
            root,
            AlgorithmSize::new(
                AlgorithmAvailableSpace::Definite(viewport_width),
                AlgorithmAvailableSpace::Definite(viewport_height),
            ),
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
        tree: AlgorithmTree::new(),
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
                apply_replaced_intrinsic_size(
                    &mut taffy_style,
                    self.dom,
                    node,
                    &computed,
                    self.image_sources,
                    font_size,
                );
                let block_style =
                    to_block_style(self.boxes, self.styles, box_id, &computed, font_size);
                let kind = algorithm_kind(&self.boxes[box_id], children.is_empty());
                let dom_node = node;
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
                    &self.boxes[box_id],
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
                apply_replaced_intrinsic_size(
                    &mut taffy_style,
                    self.dom,
                    node,
                    &computed,
                    self.image_sources,
                    font_size,
                );
                let block_style =
                    to_block_style(self.boxes, self.styles, box_id, &computed, font_size);
                let kind = algorithm_kind(&self.boxes[box_id], children.is_empty());
                let dom_node = node;
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
                    &self.boxes[box_id],
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

/// Publish a static-position rectangle at the formatting boundary that
/// produced it. The selected absolute or fixed containing block comes from
/// Buckram's K5a box graph; the backend never chooses it here.
fn static_position_record<Id>(
    boxes: &GeneratedBoxTree<Id>,
    box_id: BoxId,
    source_fragment: Option<FragmentId>,
    logical_rect: LogicalRect,
    containing_block_area: Option<PhysicalRect>,
    fragments: &FragmentTree,
) -> Option<StaticPosition>
where
    Id: Copy + Eq + Hash,
{
    if !matches!(
        boxes[box_id].positioning,
        PositioningScheme::Absolute | PositioningScheme::Fixed
    ) {
        return None;
    }
    let containing_block_area = containing_block_area.and_then(|area| {
        let source = source_fragment?;
        let fragment = fragments.get(source)?;
        let rect = fragment.physical_rect();
        Some(fragment.flow().logical_rect(
            area,
            PhysicalSize {
                width: rect.width,
                height: rect.height,
            },
        ))
    });
    Some(StaticPosition {
        box_id,
        source: source_fragment.map_or(
            StaticPositionSource::InitialContainingBlock,
            StaticPositionSource::Fragment,
        ),
        containing_block: boxes[box_id].containing_block,
        logical_rect: if boxes[box_id]
            .display
            .internal_table
            .is_some_and(uses_zero_track_static_anchor)
        {
            LogicalRect::default()
        } else {
            logical_rect
        },
        containing_block_area,
    })
}

fn record_static_position<Id>(
    boxes: &GeneratedBoxTree<Id>,
    box_id: BoxId,
    source_fragment: Option<FragmentId>,
    logical_rect: LogicalRect,
    containing_block_area: Option<PhysicalRect>,
    output: &mut FragmentOutput<'_>,
) where
    Id: Copy + Eq + Hash,
{
    if let Some(position) = static_position_record(
        boxes,
        box_id,
        source_fragment,
        logical_rect,
        containing_block_area,
        output.fragments,
    ) {
        output.fragments.record_static_position(position);
    }
}

/// Apply relative positioning only after every normal-flow fragment exists.
///
/// Taffy receives auto insets for `position: relative`; it determines the
/// unshifted flow rectangle, while Buckram resolves the retained CSS inputs
/// and moves the emitted fragment subtree. Internal table parts keep the K4h
/// table traversal for now, because it owns their structural fragment draft
/// and cell-content offset together. The table wrapper itself is ordinary
/// flow geometry and uses this route.
fn apply_relative_positioning<D>(
    fragments: &mut FragmentTree,
    boxes: &GeneratedBoxTree<D::NodeId>,
    styles: &StylePlane<D::NodeId>,
    dom: &D,
    mut text_frame: Option<&mut TextFrame<D::NodeId>>,
    initial_containing_size: PhysicalSize,
) where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let placements = boxes
        .iter()
        .filter_map(|(box_id, css_box)| {
            if css_box.positioning != PositioningScheme::Relative
                || matches!(
                    css_box.display.internal_table,
                    Some(role)
                        if !matches!(
                            role,
                            InternalTableRole::Wrapper | InternalTableRole::Caption
                        )
                )
            {
                return None;
            }
            let node = css_box.origin.node()?;
            let computed = styles.get(node)?;
            let font_size = font_size_px(&computed.font_size, LIVE_ROOT_FONT_SIZE);
            let style = to_block_style(boxes, styles, box_id, computed, font_size);
            let roots = fragments
                .fragment_ids_for_box(box_id)
                .iter()
                .copied()
                .filter(|fragment_id| {
                    fragments
                        .get(*fragment_id)
                        .and_then(TreeFragment::parent)
                        .and_then(|parent| fragments.get(parent))
                        .is_none_or(|parent| parent.box_id() != box_id)
                })
                .collect::<Vec<_>>();
            (!roots.is_empty()).then_some((box_id, node, style, roots))
        })
        .collect::<Vec<_>>();

    for (_box_id, node, style, roots) in placements {
        // The retained text frame shaped this box's glyphs at its normal-flow
        // position. Move those glyphs in lockstep with the fragment subtree,
        // once per box, while nested relative descendants retain their own
        // additional offset.
        let mut text_offset: Option<PhysicalOffset> = None;
        for root in roots {
            let containing = fragments
                .get(root)
                .and_then(TreeFragment::containing_fragment)
                .and_then(|containing| fragments.get(containing));
            let (containing_inline_size, containing_block_size) = match containing {
                None => {
                    let size = style.containing_flow.logical_size(initial_containing_size);
                    (size.inline, Some(size.block))
                },
                Some(fragment) => {
                    let inline = style
                        .containing_flow
                        .logical_size(PhysicalSize {
                            width: fragment.width,
                            height: fragment.height,
                        })
                        .inline;
                    let block = definite_containing_block_size(boxes, styles, fragments, fragment)
                        .map(|size| style.containing_flow.logical_size(size).block);
                    (inline, block)
                },
            };
            let logical = style.relative_offset(containing_inline_size, containing_block_size);
            let physical: PhysicalOffset = style.containing_flow.physical_offset(logical);
            text_offset.get_or_insert(physical);
            fragments.translate_subtree(root, physical);
        }
        if let (Some(offset), Some(text)) = (text_offset, text_frame.as_deref_mut())
            && (offset.x != 0.0 || offset.y != 0.0)
        {
            text.translate_subtree(dom, node, (offset.x, offset.y));
        }
    }
}

/// The content-box size of a containing block whose block-axis size is
/// specified, so a block-axis percentage inset has a basis. A containing block
/// sized by its content has no such basis and CSS treats the percentage as
/// `auto`; this reports `None` for it.
fn definite_containing_block_size<Id>(
    boxes: &GeneratedBoxTree<Id>,
    styles: &StylePlane<Id>,
    fragments: &FragmentTree,
    fragment: &TreeFragment,
) -> Option<PhysicalSize>
where
    Id: Copy + Eq + Hash,
{
    let css_box = &boxes[fragment.box_id()];
    let computed = styles.get(css_box.origin.node()?)?;
    let font_size = font_size_px(&computed.font_size, LIVE_ROOT_FONT_SIZE);
    let block_axis = if css_box.flow.is_horizontal() {
        computed.height
    } else {
        computed.width
    };
    let definite = match block_size_value(block_axis, font_size) {
        BlockSizeValue::Length(length) if length.percentage == 0.0 => true,
        // A percentage block size is only as definite as the size it
        // resolves against (CSS 2.1 §10.5).
        BlockSizeValue::Length(_) => match css_box.parent() {
            None => true,
            Some(parent) => fragments
                .fragment_ids_for_box(parent)
                .first()
                .and_then(|id| fragments.get(*id))
                .and_then(|parent| definite_containing_block_size(boxes, styles, fragments, parent))
                .is_some(),
        },
        _ => stretched_item_block_size_is_definite(boxes, styles, css_box, computed),
    };
    if !definite {
        return None;
    }
    let (width, height) = content_box_size(computed, fragment);
    Some(PhysicalSize { width, height })
}

/// CSS Flexbox §9.8 and CSS Grid §6.6: once a stretched flex item's cross
/// size or a grid item's area is laid out, its descendants treat that size as
/// definite, so a percentage inset resolves against it even though the item's
/// own block size computes to `auto`.
fn stretched_item_block_size_is_definite<Id>(
    boxes: &GeneratedBoxTree<Id>,
    styles: &StylePlane<Id>,
    css_box: &buckram::CssBox<Id>,
    computed: &ComputedValues,
) -> bool
where
    Id: Copy + Eq + Hash,
{
    let Some(parent) = css_box.parent() else {
        return false;
    };
    let Some(container) = boxes[parent]
        .origin
        .node()
        .and_then(|node| styles.get(node))
    else {
        return false;
    };
    let stretch =
        |alignment: CssAlignment| matches!(alignment, CssAlignment::Auto | CssAlignment::Stretch);
    match container.display {
        CssDisplay::Grid => stretch(computed.align_self),
        CssDisplay::Flex => {
            let cross_axis_is_block = matches!(
                container.flex_direction,
                CssFlexDirection::Row | CssFlexDirection::RowReverse
            ) == css_box.flow.is_horizontal();
            cross_axis_is_block
                && match computed.align_self {
                    CssAlignment::Auto => stretch(container.align_items),
                    alignment => alignment == CssAlignment::Stretch,
                }
        },
        _ => false,
    }
}

#[derive(Clone, Copy)]
struct PositionedPlacement {
    box_id: BoxId,
    root: FragmentId,
    containing_fragment: Option<FragmentId>,
    current: PhysicalRect,
    containing_rect: PhysicalRect,
    containing_flow: FlowAxes,
    containing_size: buckram::LogicalSize,
    style: BlockStyle,
    geometry: buckram::PositionedBoxGeometry,
}

impl PositionedPlacement {
    fn target_rect(self) -> PhysicalRect {
        self.containing_flow.physical_rect(
            self.geometry.logical_rect,
            PhysicalSize {
                width: self.containing_rect.width,
                height: self.containing_rect.height,
            },
        )
    }

    /// Convert Buckram's border-box answer back into the formatter's CSS
    /// inline-size input. This is only admitted for same-flow roots whose
    /// intrinsic query was accepted above.
    fn formatter_inline_size(self) -> Option<f32> {
        if self.style.flow != self.style.containing_flow {
            return None;
        }
        let inline_size = if self.style.flow.is_horizontal() {
            self.style.size.width
        } else {
            self.style.size.height
        };
        if !matches!(
            inline_size,
            BlockSizeValue::Auto
                | BlockSizeValue::MinContent
                | BlockSizeValue::MaxContent
                | BlockSizeValue::FitContent(_)
        ) {
            return None;
        }
        let padding_border = self
            .style
            .logical_padding_border(self.containing_size.inline);
        let border_box = self.geometry.logical_rect.inline_size;
        Some(match self.style.box_sizing {
            BlockBoxSizing::ContentBox => {
                (border_box - padding_border.inline_start - padding_border.inline_end).max(0.0)
            },
            BlockBoxSizing::BorderBox => border_box,
        })
    }

    /// Convert a standards-resolved block size back into the formatter's CSS
    /// input for the constrained second pass.
    fn formatter_block_size(self) -> Option<f32> {
        if self.style.flow != self.style.containing_flow || !self.geometry.block_size_solved {
            return None;
        }
        let measured = self
            .containing_flow
            .logical_size(PhysicalSize {
                width: self.current.width,
                height: self.current.height,
            })
            .block;
        let border_box = self.geometry.logical_rect.block_size;
        if (border_box - measured).abs() <= 0.01 {
            return None;
        }
        let padding_border = self
            .style
            .logical_padding_border(self.containing_size.inline);
        Some(match self.style.box_sizing {
            BlockBoxSizing::ContentBox => {
                (border_box - padding_border.block_start - padding_border.block_end).max(0.0)
            },
            BlockBoxSizing::BorderBox => border_box,
        })
    }
}

/// Resolve absolute and fixed used geometry from K5a/K5b inputs after a
/// formatting pass has supplied static rectangles and admitted intrinsic
/// contributions. The returned record keeps positioning separate from the
/// later fragment translation and possible constrained reformat.
#[expect(
    clippy::too_many_arguments,
    reason = "the positioning boundary needs fragment, box, style, replaced-source, intrinsic, and viewport inputs"
)]
fn positioned_placements<D>(
    fragments: &FragmentTree,
    boxes: &buckram::CssBoxTree<D::NodeId>,
    styles: &StylePlane<D::NodeId>,
    dom: &D,
    image_sources: &ImageSources,
    intrinsic_sizes: &HashMap<BoxId, IntrinsicSizes>,
    viewport_width: f32,
    viewport_height: f32,
) -> Vec<PositionedPlacement>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let candidates = boxes
        .iter()
        .filter_map(|(box_id, css_box)| {
            if !matches!(
                css_box.positioning,
                PositioningScheme::Absolute | PositioningScheme::Fixed
            ) || matches!(
                css_box.display.internal_table,
                Some(role) if !supports_shared_positioned_table_part(role)
            ) {
                return None;
            }
            let node = css_box.origin.node()?;
            styles.get(node)?;
            let root = fragments.fragment_ids_for_box(box_id).first().copied()?;
            let static_position = *fragments.static_position_for_box(box_id)?;
            Some((box_id, node, root, static_position))
        })
        .collect::<Vec<_>>();

    candidates
        .into_iter()
        .filter_map(|(box_id, node, root, static_position)| {
            let current = fragments.get(root).map(TreeFragment::physical_rect)?;
            let (containing_fragment, containing_rect, containing_flow) =
                match static_position.containing_block {
                    ContainingBlock::Initial => (
                        None,
                        Fragment {
                            x: 0.0,
                            y: 0.0,
                            width: viewport_width,
                            height: viewport_height,
                        },
                        FlowAxes::HORIZONTAL_LTR,
                    ),
                    ContainingBlock::Box(containing_box) => {
                        let fragment_id = fragments
                            .fragment_ids_for_box(containing_box)
                            .first()
                            .copied()?;
                        let border_rect = fragments
                            .get(fragment_id)
                            .map(TreeFragment::physical_rect)?;
                        let rect = match (
                            static_position.source,
                            static_position.containing_block_area,
                        ) {
                            (StaticPositionSource::Fragment(source), Some(area))
                                if source == fragment_id =>
                            {
                                let area = boxes[containing_box].flow.physical_rect(
                                    area,
                                    PhysicalSize {
                                        width: border_rect.width,
                                        height: border_rect.height,
                                    },
                                );
                                PhysicalRect {
                                    x: border_rect.x + area.x,
                                    y: border_rect.y + area.y,
                                    width: area.width,
                                    height: area.height,
                                }
                            },
                            _ => positioned_containing_block_rect(
                                border_rect,
                                containing_box,
                                fragments,
                                boxes,
                                styles,
                            ),
                        };
                        (Some(fragment_id), rect, boxes[containing_box].flow)
                    },
                };
            let (source_origin, source_size) = match static_position.source {
                StaticPositionSource::InitialContainingBlock => (
                    (0.0, 0.0),
                    PhysicalSize {
                        width: viewport_width,
                        height: viewport_height,
                    },
                ),
                StaticPositionSource::Fragment(source) => fragments
                    .get(source)
                    .map(TreeFragment::physical_rect)
                    .map_or(
                        (
                            (0.0, 0.0),
                            PhysicalSize {
                                width: viewport_width,
                                height: viewport_height,
                            },
                        ),
                        |rect| {
                            (
                                (rect.x, rect.y),
                                PhysicalSize {
                                    width: rect.width,
                                    height: rect.height,
                                },
                            )
                        },
                    ),
            };
            let static_in_source = boxes[box_id]
                .flow
                .physical_rect(static_position.logical_rect, source_size);
            let static_in_containing = PhysicalRect {
                x: source_origin.0 + static_in_source.x - containing_rect.x,
                y: source_origin.1 + static_in_source.y - containing_rect.y,
                width: static_in_source.width,
                height: static_in_source.height,
            };
            let computed = styles
                .get(node)
                .expect("a generated positioned box keeps its computed style");
            let computed =
                if boxes[box_id].display.internal_table == Some(InternalTableRole::Wrapper) {
                    wrapper_style(computed)
                } else {
                    computed.clone()
                };
            let font_size = font_size_px(&computed.font_size, LIVE_ROOT_FONT_SIZE);
            let style = to_block_style(boxes, styles, box_id, &computed, font_size);
            let replaced = positioned_replaced_input(dom, node, image_sources, &style);
            let containing_size = containing_flow.logical_size(PhysicalSize {
                width: containing_rect.width,
                height: containing_rect.height,
            });
            let static_rect = containing_flow.logical_rect(
                static_in_containing,
                PhysicalSize {
                    width: containing_rect.width,
                    height: containing_rect.height,
                },
            );
            let intrinsic_inline =
                positioned_contain_intrinsic_inline(&computed, &style, font_size)
                    .or_else(|| intrinsic_sizes.get(&box_id).copied());
            let geometry = buckram::solve_positioned_box(
                style,
                buckram::PositionedBoxInput {
                    containing_size,
                    static_rect,
                    measured_size: containing_flow.logical_size(PhysicalSize {
                        width: current.width,
                        height: current.height,
                    }),
                    intrinsic_inline,
                    replaced,
                },
            );
            Some(PositionedPlacement {
                box_id,
                root,
                containing_fragment,
                current,
                containing_rect,
                containing_flow,
                containing_size,
                style,
                geometry,
            })
        })
        .collect()
}

/// Supply the explicit substitute intrinsic contribution for the positioned
/// inline axis only when that physical axis is size-contained.
fn positioned_contain_intrinsic_inline(
    computed: &ComputedValues,
    style: &BlockStyle,
    font_size: f32,
) -> Option<IntrinsicSizes> {
    let (width, height) = computed.contain_intrinsic_size.physical_lengths()?;
    let inline_is_contained = if style.containing_flow.is_horizontal() {
        style.size_containment.width
    } else {
        style.size_containment.height
    };
    if !inline_is_contained {
        return None;
    }
    let physical = PhysicalSize {
        width: absolute_length(width, font_size, LIVE_ROOT_FONT_SIZE),
        height: absolute_length(height, font_size, LIVE_ROOT_FONT_SIZE),
    };
    let inline = style.containing_flow.logical_size(physical).inline;
    IntrinsicSizes::new(inline, inline)
}

/// Resolve the ordinary absolute/fixed containing-block rectangle from an
/// established ancestor fragment. CSS Positioned Layout defines a non-inline
/// ancestor's containing block at its padding edge. An inline ancestor instead
/// combines the logical start content edges of its first fragment with the
/// logical end content edges of its last fragment.
fn positioned_containing_block_rect<Id>(
    border_rect: PhysicalRect,
    containing_box: BoxId,
    fragments: &FragmentTree,
    boxes: &buckram::CssBoxTree<Id>,
    styles: &StylePlane<Id>,
) -> PhysicalRect
where
    Id: Copy + Eq + Hash,
{
    let css_box = &boxes[containing_box];
    let Some(computed) = css_box.origin.node().and_then(|node| styles.get(node)) else {
        return border_rect;
    };
    let font_size = font_size_px(&computed.font_size, LIVE_ROOT_FONT_SIZE);
    if css_box.display.outside == Some(DisplayOutside::Inline)
        && css_box.display.inside == Some(DisplayInside::Flow)
        && css_box.display.internal_table.is_none()
    {
        return positioned_inline_containing_block_rect(
            containing_box,
            fragments,
            boxes,
            css_box.flow,
            computed,
            font_size,
        )
        .unwrap_or(border_rect);
    }
    let border = PhysicalSides {
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
    };
    PhysicalRect {
        x: border_rect.x + border.left,
        y: border_rect.y + border.top,
        width: (border_rect.width - border.left - border.right).max(0.0),
        height: (border_rect.height - border.top - border.bottom).max(0.0),
    }
}

/// CSS Positioned Layout's special containing-block rule for an inline
/// positioned ancestor. The fragment tree retains the in-order line fragments
/// emitted by the inline formatter, including generated continuation boxes
/// around an in-flow block. The CSS rectangle starts at the first fragment's
/// logical content starts and ends at the last fragment's logical content ends,
/// so it can span intervening lines without treating their union as a normal
/// block border box.
fn positioned_inline_containing_block_rect<Id>(
    containing_box: BoxId,
    fragments: &FragmentTree,
    boxes: &buckram::CssBoxTree<Id>,
    flow: FlowAxes,
    computed: &ComputedValues,
    font_size: f32,
) -> Option<PhysicalRect>
where
    Id: Copy + Eq + Hash,
{
    // One DOM inline can lower to several generated boxes when it is split by
    // an in-flow block. K5a names the continuation that structurally owns the
    // positioned descendant, but CSS Position defines one containing block
    // from every fragment of the original inline element.
    let fragment_ids = boxes[containing_box]
        .origin
        .node()
        .map(|node| {
            boxes
                .boxes_for_node(node)
                .iter()
                .copied()
                .filter(|box_id| {
                    let candidate = &boxes[*box_id];
                    candidate.display.outside == Some(DisplayOutside::Inline)
                        && candidate.display.inside == Some(DisplayInside::Flow)
                        && candidate.display.internal_table.is_none()
                        && candidate.flow == flow
                })
                .flat_map(|box_id| fragments.fragment_ids_for_box(box_id).iter().copied())
                .collect::<Vec<_>>()
        })
        .filter(|fragment_ids| !fragment_ids.is_empty())
        .unwrap_or_else(|| fragments.fragment_ids_for_box(containing_box).to_vec());
    let first = fragments.get(*fragment_ids.first()?)?;
    let last = fragments.get(*fragment_ids.last()?)?;
    let first = first.physical_rect();
    let last = last.physical_rect();

    // Inline padding percentages use the inline formatting context's resolved
    // width, which is also the basis supplied to the retained text formatter.
    // The structural containing fragment is that formatting-context fragment.
    let percentage_basis = fragments
        .get(*fragment_ids.first()?)
        .and_then(TreeFragment::containing_fragment)
        .and_then(|parent| fragments.get(parent))
        .map_or(first.width, |parent| parent.physical_rect().width);
    let decoration = PhysicalSides {
        top: length_percentage_px(computed.padding_top.0, font_size, percentage_basis)
            + border_width_px(
                computed.border_top_style,
                computed.border_top_width,
                font_size,
            ),
        right: length_percentage_px(computed.padding_right.0, font_size, percentage_basis)
            + border_width_px(
                computed.border_right_style,
                computed.border_right_width,
                font_size,
            ),
        bottom: length_percentage_px(computed.padding_bottom.0, font_size, percentage_basis)
            + border_width_px(
                computed.border_bottom_style,
                computed.border_bottom_width,
                font_size,
            ),
        left: length_percentage_px(computed.padding_left.0, font_size, percentage_basis)
            + border_width_px(
                computed.border_left_style,
                computed.border_left_width,
                font_size,
            ),
    };
    let content_rect = |rect: PhysicalRect| PhysicalRect {
        x: rect.x + decoration.left,
        y: rect.y + decoration.top,
        width: (rect.width - decoration.left - decoration.right).max(0.0),
        height: (rect.height - decoration.top - decoration.bottom).max(0.0),
    };
    let first = content_rect(first);
    let last = content_rect(last);
    let edge = |side: PhysicalSide| {
        let fragment = if side == flow.inline_start() || side == flow.block_start() {
            first
        } else {
            last
        };
        match side {
            PhysicalSide::Top => fragment.y,
            PhysicalSide::Right => fragment.x + fragment.width,
            PhysicalSide::Bottom => fragment.y + fragment.height,
            PhysicalSide::Left => fragment.x,
        }
    };
    let left = edge(PhysicalSide::Left);
    let right = edge(PhysicalSide::Right);
    let top = edge(PhysicalSide::Top);
    let bottom = edge(PhysicalSide::Bottom);
    Some(PhysicalRect {
        x: left,
        y: top,
        width: (right - left).max(0.0),
        height: (bottom - top).max(0.0),
    })
}

/// Reformat an admitted auto-sized positioned root at Buckram's resolved
/// inline size. Other positioned subtrees retain the formatter fallback
/// until their own K5d sizing route is implemented.
fn apply_admitted_positioned_inline_sizes<Context, Source>(
    tree: &mut AlgorithmTree<Style, Context, Source>,
    candidates: &[(BoxId, AlgorithmNodeId)],
    placements: &[PositionedPlacement],
    intrinsic_sizes: &HashMap<BoxId, IntrinsicSizes>,
) -> bool {
    let mut changed = false;
    for (box_id, node) in candidates {
        let Some(placement) = placements
            .iter()
            .find(|placement| placement.box_id == *box_id)
        else {
            continue;
        };
        if intrinsic_sizes.contains_key(box_id)
            && let Some(size) = placement.formatter_inline_size()
        {
            if placement.style.flow.is_horizontal() {
                tree.style_mut(*node).size.width = Dimension::length(size);
            } else {
                tree.style_mut(*node).size.height = Dimension::length(size);
            }
            tree.set_positioned_inline_size(*node, size);
            changed = true;
        }
        if let Some(size) = placement.formatter_block_size() {
            if placement.style.flow.is_horizontal() {
                tree.style_mut(*node).size.height = Dimension::length(size);
            } else {
                tree.style_mut(*node).size.width = Dimension::length(size);
            }
            tree.set_positioned_block_size(*node, size);
            changed = true;
        }
    }
    if changed {
        tree.clear_layout_cache();
    }
    changed
}

/// Apply final absolute and fixed offsets from Buckram's resolved used
/// geometry. The formatter supplies content fragments only; this bridge never
/// lets it select the CSS containing block or final inset origin.
#[expect(
    clippy::too_many_arguments,
    reason = "the final positioning bridge receives the same explicit CSS and replaced-source inputs as placement"
)]
fn apply_absolute_and_fixed_positioning<D>(
    fragments: &mut FragmentTree,
    boxes: &GeneratedBoxTree<D::NodeId>,
    styles: &StylePlane<D::NodeId>,
    dom: &D,
    mut text_frame: Option<&mut TextFrame<D::NodeId>>,
    image_sources: &ImageSources,
    intrinsic_sizes: &HashMap<BoxId, IntrinsicSizes>,
    viewport_width: f32,
    viewport_height: f32,
) where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    for placement in positioned_placements(
        fragments,
        boxes,
        styles,
        dom,
        image_sources,
        intrinsic_sizes,
        viewport_width,
        viewport_height,
    ) {
        let target = placement.target_rect();
        // The formatter owns positioned subtrees, but a fragment with no
        // descendants has no child containing block to invalidate. Publish
        // Buckram's used border box directly for that leaf.
        fragments.resize_leaf(
            placement.root,
            PhysicalSize {
                width: target.width,
                height: target.height,
            },
        );
        let offset = PhysicalOffset {
            x: placement.containing_rect.x + target.x - placement.current.x,
            y: placement.containing_rect.y + target.y - placement.current.y,
        };
        fragments.translate_subtree(placement.root, offset);
        if let Some(node) = boxes[placement.box_id].origin.node()
            && let Some(text) = text_frame.as_deref_mut()
        {
            text.translate_subtree(dom, node, (offset.x, offset.y));
        }
        fragments.set_containing_fragment(placement.root, placement.containing_fragment);
    }
}

fn fragment_baselines<Id, Context, Source>(
    tree: &AlgorithmTree<Style, Context, Source>,
    boxes: &GeneratedBoxTree<Id>,
    node: AlgorithmNodeId,
    box_id: BoxId,
    rect: Fragment,
) -> Baselines
where
    Id: Copy + Eq + Hash,
{
    let css_box = &boxes[box_id];
    if css_box.display.outside == Some(DisplayOutside::Inline)
        && css_box.display.inside == Some(DisplayInside::FlowRoot)
    {
        // The admitted atomic lane currently has no line-baseline provider of
        // its own. Its modeled fallback is therefore its block-end edge,
        // rather than a value inferred from the parent's line rectangle.
        Baselines::synthesized_from_block_end(rect.height)
    } else {
        tree.baselines(node)
    }
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
    css_box: &CssBox<Id>,
    computed: &ComputedValues,
    block_style: BlockStyle,
    kind: AlgorithmKind,
) -> bool {
    let float_root = css_box.display.outside == Some(DisplayOutside::Block)
        && block_style.float != FloatSide::None;
    let atomic_inline_root = css_box.display.outside == Some(DisplayOutside::Inline)
        && block_style.float == FloatSide::None;
    kind == AlgorithmKind::Block
        && matches!(
            css_box.display.inside,
            Some(DisplayInside::Flow | DisplayInside::FlowRoot)
        )
        && css_box.display.internal_table.is_none()
        && block_style.position == BuckramBlockPosition::Static
        && block_style.shrink_to_fit
        && !block_style.replaced
        && computed.vertical_align == VerticalAlign::Baseline
        && block_style.flow.is_horizontal()
        && block_style.containing_flow.is_horizontal()
        && (float_root || atomic_inline_root)
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

/// Retained paint data for one table whose geometry Buckram accepted.
///
/// The fragment vector is the paint-order authority for table-internal boxes;
/// the live fragment tree supplies the final physical coordinates. The clip
/// set records the CSS Tables 3 rendering rule for a cell that crosses a
/// collapsed track boundary.
#[derive(Clone, Debug)]
pub(crate) struct TablePaintModel {
    fragments: TableFragments,
    separated: bool,
    collapsed_geometry: Option<CollapsedBorderGeometry<ComputedColor>>,
    clipped_cells: HashSet<BoxId>,
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

    fn manages(&self, box_id: BoxId) -> bool {
        self.fragments.fragments().iter().any(|fragment| {
            fragment.box_id == Some(box_id)
                && (self.is_collapsed()
                    || (self.separated && fragment.role != TableFragmentRole::Grid))
        })
    }

    fn clips_cell(&self, box_id: BoxId) -> bool {
        self.clipped_cells.contains(&box_id)
    }

    fn remap_box_ids(&mut self, identities: &buckram::LayoutIdentityMap) {
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
    tables: HashMap<BoxId, TablePaintModel>,
}

impl TablePaintPlane {
    fn table(&self, grid: BoxId) -> Option<&TablePaintModel> {
        self.tables.get(&grid)
    }

    fn manages(&self, box_id: BoxId) -> bool {
        self.tables.values().any(|table| table.manages(box_id))
    }

    fn clips_cell(&self, box_id: BoxId) -> bool {
        self.tables.values().any(|table| table.clips_cell(box_id))
    }

    fn fragments(&self) -> TableFragmentPlane {
        self.tables
            .iter()
            .map(|(grid, table)| (*grid, table.fragments.clone()))
            .collect()
    }

    fn merge(&mut self, other: Self) {
        self.tables.extend(other.tables);
    }

    /// Replace only the table paint models rooted in one reconciled fragment
    /// subtree. Their BoxIds already agree with the fresh layout; unrelated
    /// table models keep their existing structural fragments and paint order.
    fn replace_subtree_from<Id>(
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

    fn remap_box_ids(&mut self, identities: &buckram::LayoutIdentityMap) {
        self.tables = std::mem::take(&mut self.tables)
            .into_iter()
            .map(|(grid, mut table)| {
                table.remap_box_ids(identities);
                (identities.box_id(grid), table)
            })
            .collect();
    }
}

fn table_cell_spans_collapsed_track(visibility: &TableTrackVisibility, cell: &TableCell) -> bool {
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
fn relative_table_part_offset(
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
fn specified_table_block_basis<Id>(
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
fn apply_relative_table_part_offsets<Id>(
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

fn table_part_relative_offset<Id>(
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

fn table_paint_plane<Id>(
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
fn commit_table_structure<Id>(
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

/// Return the topmost pointer-events-enabled element whose layout fragment
/// contains a scene point. The walk mirrors the lane's DOM paint order for the
/// bounded stacking subset: numeric z-index first, then source order within a
/// stacking context. Descendants remain inside their nearest positioned
/// context, so a child can paint above its context's background without
/// escaping an ancestor that is below a sibling context.
pub fn hit_test<D>(
    dom: &D,
    styles: &StylePlane<D::NodeId>,
    fragments: &LiveryLayout<D::NodeId>,
    x: f32,
    y: f32,
) -> Option<D::NodeId>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    hit_test_with_scroll(dom, styles, fragments, &HashMap::new(), x, y)
}

/// Return a numeric stacking level only where CSS lets `z-index` establish a
/// context: positioned boxes and direct flex/grid items. A static ordinary
/// block keeps normal paint order even when it carries a numeric declaration.
pub(crate) fn z_index_stacking_level<D>(
    dom: &D,
    styles: &StylePlane<D::NodeId>,
    id: D::NodeId,
) -> Option<i32>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let style = styles.get(id)?;
    let livery::values::ZIndex::Integer(level) = style.z_index else {
        return None;
    };
    let is_flex_or_grid_item = dom
        .parent(id)
        .and_then(|parent| styles.get(parent))
        .is_some_and(|parent| matches!(parent.display, CssDisplay::Flex | CssDisplay::Grid));
    (style.position != CssPosition::Static || is_flex_or_grid_item).then_some(level)
}

/// Return direct DOM children in CSS paint order for the admitted item
/// containers. Flex and grid order by their computed `order` value while the
/// stable sort preserves document order for equal values and anonymous text.
pub(crate) fn order_modified_children<D>(
    dom: &D,
    styles: &StylePlane<D::NodeId>,
    parent: D::NodeId,
) -> Vec<D::NodeId>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let mut children = dom.dom_children(parent).collect::<Vec<_>>();
    let is_flex_or_grid = styles
        .get(parent)
        .is_some_and(|style| matches!(style.display, CssDisplay::Flex | CssDisplay::Grid));
    if is_flex_or_grid {
        children.sort_by_key(|child| styles.get(*child).map_or(0, |style| style.order.value()));
    }
    children
}

/// Direct children in the admitted paint order. Flex/grid `order` remains the
/// first ordering step; positioned and flex/grid-item stacking levels then
/// divide that sequence, with equal levels retaining its stable source order.
///
/// This is local to one stacking context. A child of a positioned
/// `z-index: 4` box may have `z-index: 2`, but it still belongs to the outer
/// level 4 context rather than competing with an unrelated level 2 sibling.
fn stacking_paint_children<D>(
    dom: &D,
    styles: &StylePlane<D::NodeId>,
    parent: D::NodeId,
) -> Vec<D::NodeId>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let mut children = order_modified_children(dom, styles, parent);
    children.sort_by_key(|child| z_index_stacking_level(dom, styles, *child).unwrap_or_default());
    children
}

/// Hit-test a retained fragment plane after applying per-element scroll
/// offsets to descendants. The ordinary [`hit_test`] path keeps the map empty;
/// retained sessions use this variant for wheel-scrolled containers.
pub fn hit_test_with_scroll<D>(
    dom: &D,
    styles: &StylePlane<D::NodeId>,
    fragments: &LiveryLayout<D::NodeId>,
    scroll_offsets: &HashMap<D::NodeId, (f32, f32)>,
    x: f32,
    y: f32,
) -> Option<D::NodeId>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let mut state = HitTestState {
        dom,
        styles,
        fragments,
        scroll_offsets,
        x,
        y,
        clips: Vec::new(),
        order: 0,
        candidates: Vec::new(),
    };
    collect_hit_candidates(&mut state, dom.document(), (0.0, 0.0), None);
    state
        .candidates
        .into_iter()
        .max_by_key(|candidate| (candidate.level, candidate.order))
        .map(|candidate| candidate.id)
}

struct HitCandidate<Id> {
    id: Id,
    level: i32,
    order: u64,
}

struct HitTestState<'a, D>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    dom: &'a D,
    styles: &'a StylePlane<D::NodeId>,
    fragments: &'a LiveryLayout<D::NodeId>,
    scroll_offsets: &'a HashMap<D::NodeId, (f32, f32)>,
    x: f32,
    y: f32,
    clips: Vec<(f32, f32, f32, f32)>,
    order: u64,
    candidates: Vec<HitCandidate<D::NodeId>>,
}

fn collect_hit_candidates<D>(
    state: &mut HitTestState<'_, D>,
    id: D::NodeId,
    ancestor_scroll: (f32, f32),
    ancestor_stacking_level: Option<i32>,
) where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let style = state.styles.get(id);
    let stacking_level =
        ancestor_stacking_level.or_else(|| z_index_stacking_level(state.dom, state.styles, id));
    // K4e4: the hit target is the node's outermost box - a table element's
    // wrapper - so the caption area belongs to the table when nothing deeper
    // claims it, and the caption element wins inside its own rectangle by
    // paint order.
    let fragment = state.fragments.get(id);
    let visible_fragment = fragment.map(|fragment| Fragment {
        x: fragment.x - ancestor_scroll.0,
        y: fragment.y - ancestor_scroll.1,
        ..fragment.physical_rect()
    });
    let inside_clips = state.clips.iter().all(|(left, top, right, bottom)| {
        state.x >= *left && state.x <= *right && state.y >= *top && state.y <= *bottom
    });
    if state.dom.kind(id) == NodeKind::Element
        && let (Some(style), Some(fragment)) = (style, visible_fragment)
        && style.display != CssDisplay::None
        && style.visibility == livery::values::Visibility::Visible
        && style.pointer_events == livery::values::PointerEvents::Auto
        && inside_clips
        && state.x >= fragment.x
        && state.x <= fragment.x + fragment.width
        && state.y >= fragment.y
        && state.y <= fragment.y + fragment.height
    {
        let level = stacking_level.unwrap_or_default();
        state.candidates.push(HitCandidate {
            id,
            level,
            order: state.order,
        });
    }
    state.order = state.order.saturating_add(1);

    let pushed_clip = style
        .zip(visible_fragment)
        .filter(|(style, _)| {
            style.overflow_x != CssOverflow::Visible || style.overflow_y != CssOverflow::Visible
        })
        .map(|(_, fragment)| {
            (
                fragment.x,
                fragment.y,
                fragment.x + fragment.width,
                fragment.y + fragment.height,
            )
        });
    if let Some(clip) = pushed_clip.as_ref() {
        state.clips.push(*clip);
    }
    let children = stacking_paint_children(state.dom, state.styles, id);
    let next_scroll = state
        .scroll_offsets
        .get(&id)
        .copied()
        .map_or(ancestor_scroll, |offset| {
            (ancestor_scroll.0 + offset.0, ancestor_scroll.1 + offset.1)
        });
    for child in children {
        collect_hit_candidates(state, child, next_scroll, stacking_level);
    }
    if pushed_clip.is_some() {
        state.clips.pop();
    }
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

fn apply_replaced_intrinsic_size<D>(
    style: &mut Style,
    dom: &D,
    id: D::NodeId,
    computed: &ComputedValues,
    image_sources: &ImageSources,
    font_size: f32,
) where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    let intrinsic = replaced_intrinsic_size(dom, id, image_sources);
    let natural_ratio = intrinsic
        .filter(|(width, height)| *width > 0.0 && *height > 0.0)
        .map(|(width, height)| width / height);

    // Attribute-derived dimensions already reached `computed` through the
    // presentational-hint origin. Layout owns only natural-size resolution.
    let width_specified = !matches!(computed.width, CssSize::Auto);
    let height_specified = !matches!(computed.height, CssSize::Auto);
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
    match (
        width,
        height,
        width_specified,
        height_specified,
        style.aspect_ratio,
        intrinsic,
    ) {
        (Some(width), _, true, false, Some(ratio), _) => {
            style.size.width = Dimension::length(width);
            style.size.height = Dimension::length(width / ratio);
        },
        (_, Some(height), false, true, Some(ratio), _) => {
            style.size.width = Dimension::length(height * ratio);
            style.size.height = Dimension::length(height);
        },
        (None, None, false, false, _, Some((intrinsic_width, intrinsic_height))) => {
            style.size.width = Dimension::length(intrinsic_width);
            style.size.height = Dimension::length(intrinsic_height);
        },
        _ => {},
    }
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

fn to_taffy_style(computed: &ComputedValues, font_size: f32) -> Style {
    let display = match computed.display {
        CssDisplay::None => Display::None,
        CssDisplay::Flex => Display::Flex,
        CssDisplay::Grid => Display::Grid,
        // Buckram commits the table grid before backend dispatch. Its table
        // parts are structural fragments, while cell contents keep their
        // ordinary local formatting contexts.
        CssDisplay::Table | CssDisplay::InlineTable | CssDisplay::TableRow => Display::Block,
        _ => Display::Block,
    };
    let flex_direction = match computed.flex_direction {
        CssFlexDirection::Row => FlexDirection::Row,
        CssFlexDirection::RowReverse => FlexDirection::RowReverse,
        CssFlexDirection::Column => FlexDirection::Column,
        CssFlexDirection::ColumnReverse => FlexDirection::ColumnReverse,
    };
    let float = match computed.float {
        CssFloat::None => TaffyFloat::None,
        CssFloat::Left => TaffyFloat::Left,
        CssFloat::Right => TaffyFloat::Right,
    };
    Style {
        display,
        float,
        box_sizing: match computed.box_sizing {
            CssBoxSizing::ContentBox => BoxSizing::ContentBox,
            CssBoxSizing::BorderBox => BoxSizing::BorderBox,
        },
        overflow: Point {
            x: overflow(computed.overflow_x),
            y: overflow(computed.overflow_y),
        },
        // Buckram owns every CSS positioning category. The scratch formatter
        // starts in flow; Buckram's explicit flex/grid static-position
        // provider changes a child's private backend role after attachment,
        // and a Taffy block fallback does the same for its out-of-flow
        // children only while it runs, so they take no normal-flow space there.
        position: Position::Relative,
        // Sticky geometry is a retained Buckram scroll constraint. The
        // scratch formatter receives no inset so it produces only the normal
        // flow rectangle, rather than selecting a sticky offset itself.
        inset: Rect::auto(),
        size: Size {
            width: dimension(computed.width, font_size),
            height: dimension(computed.height, font_size),
        },
        min_size: Size {
            width: dimension(computed.min_width, font_size),
            height: dimension(computed.min_height, font_size),
        },
        max_size: Size {
            width: dimension(computed.max_width, font_size),
            height: dimension(computed.max_height, font_size),
        },
        aspect_ratio: computed.aspect_ratio.preferred_ratio(),
        size_containment: match computed.container_type {
            ContainerType::Normal => Size {
                width: false,
                height: false,
            },
            ContainerType::InlineSize if computed.writing_mode.is_vertical() => Size {
                width: false,
                height: true,
            },
            ContainerType::InlineSize => Size {
                width: true,
                height: false,
            },
            ContainerType::Size => Size {
                width: true,
                height: true,
            },
        },
        flex_direction,
        flex_wrap: match computed.flex_wrap {
            CssFlexWrap::NoWrap => FlexWrap::NoWrap,
            CssFlexWrap::Wrap => FlexWrap::Wrap,
            CssFlexWrap::WrapReverse => FlexWrap::WrapReverse,
        },
        flex_basis: dimension(computed.flex_basis, font_size),
        flex_grow: computed.flex_grow.value(),
        flex_shrink: computed.flex_shrink.value(),
        order: computed.order.value(),
        margin: Rect {
            left: margin(computed.margin_left, font_size),
            right: margin(computed.margin_right, font_size),
            top: margin(computed.margin_top, font_size),
            bottom: margin(computed.margin_bottom, font_size),
        },
        padding: Rect {
            left: length_percentage(computed.padding_left.0, font_size),
            right: length_percentage(computed.padding_right.0, font_size),
            top: length_percentage(computed.padding_top.0, font_size),
            bottom: length_percentage(computed.padding_bottom.0, font_size),
        },
        border: Rect {
            left: border(
                computed.border_left_style,
                computed.border_left_width,
                font_size,
            ),
            right: border(
                computed.border_right_style,
                computed.border_right_width,
                font_size,
            ),
            top: border(
                computed.border_top_style,
                computed.border_top_width,
                font_size,
            ),
            bottom: border(
                computed.border_bottom_style,
                computed.border_bottom_width,
                font_size,
            ),
        },
        gap: Size {
            width: gap(computed.column_gap, font_size),
            height: gap(computed.row_gap, font_size),
        },
        align_items: Some(align_items(computed.align_items)),
        // `auto` on the self properties defers to the parent's items value,
        // which is taffy's `None`. A content-keyword size in that axis
        // additionally suppresses stretch (see `suppresses_stretch`).
        align_self: self_alignment(computed.align_self, computed.height),
        justify_items: Some(align_items(computed.justify_items)),
        justify_self: self_alignment(computed.justify_self, computed.width),
        align_content: Some(align_content(computed.align_content)),
        justify_content: Some(justify_content(computed.justify_content)),
        grid_template_columns: grid_template(&computed.grid_template_columns, font_size),
        grid_template_rows: grid_template(&computed.grid_template_rows, font_size),
        grid_auto_flow: grid_auto_flow(computed.grid_auto_flow),
        grid_column: Line {
            start: grid_placement(computed.grid_column_start),
            end: grid_placement(computed.grid_column_end),
        },
        grid_row: Line {
            start: grid_placement(computed.grid_row_start),
            end: grid_placement(computed.grid_row_end),
        },
        ..Style::default()
    }
}

/// The K5b flex/grid provider is selected only after the direct child is
/// attached to the scratch parent. Livery supplies generated CSS ownership;
/// Buckram owns the narrow renderer-role transition and retains the static
/// rectangle it yields for the later K5d equation.
fn enable_flex_grid_static_position_provider<Id, Context, Source>(
    tree: &mut AlgorithmTree<Style, Context, Source>,
    styles: &StylePlane<Id>,
    boxes: &GeneratedBoxTree<Id>,
    container: BoxId,
    container_node: AlgorithmNodeId,
) where
    Id: Copy + Eq + Hash,
    Source: DirectBoxSource,
{
    let inside = boxes[container].display.inside;
    if !matches!(inside, Some(DisplayInside::Flex | DisplayInside::Grid)) {
        return;
    }
    let grid_flow = (inside == Some(DisplayInside::Grid)).then_some(boxes[container].flow);
    let children = tree.children(container_node).to_vec();
    for child in children {
        if matches!(
            tree.block_style(child).position,
            BuckramBlockPosition::Absolute | BuckramBlockPosition::Fixed
        ) {
            if let Some(container_flow) = grid_flow {
                let subject_alignment = tree.source(child).direct_box().and_then(|box_id| {
                    let css_box = &boxes[box_id];
                    css_box.origin.node().and_then(|node| {
                        styles.get(node).map(|computed| {
                            (css_box.flow, computed.align_self, computed.justify_self)
                        })
                    })
                });
                let style = tree.style_mut(child);
                if !container_flow.is_horizontal() {
                    map_vertical_grid_static_alignment(style, container_flow);
                }
                if let Some((subject_flow, align_self, justify_self)) = subject_alignment {
                    map_grid_static_self_alignment(
                        style,
                        container_flow,
                        subject_flow,
                        align_self,
                        justify_self,
                    );
                }
            }
            tree.enable_flex_grid_static_position_provider(child);
            // CSS Grid §9.2: the static position is aligned in the grid's
            // content box unless the grid container also generates the
            // child's containing block, in which case the §9.1 grid area
            // applies. K5a's box graph is the only authority for that
            // relationship; the backend never selects it.
            if grid_flow.is_some()
                && tree.source(child).direct_box().is_some_and(|box_id| {
                    boxes[box_id].containing_block == ContainingBlock::Box(container)
                })
            {
                tree.use_grid_area_for_static_position(child);
            }
        }
    }
}

/// The source forms used by the block and inline scratch trees. Only a direct
/// generated box has an unambiguous subject writing mode for CSS `self-*`.
trait DirectBoxSource {
    fn direct_box(&self) -> Option<BoxId>;
}

impl DirectBoxSource for Option<BoxId> {
    fn direct_box(&self) -> Option<BoxId> {
        *self
    }
}

impl DirectBoxSource for Vec<BoxId> {
    fn direct_box(&self) -> Option<BoxId> {
        match self.as_slice() {
            [box_id] => Some(*box_id),
            _ => None,
        }
    }
}

/// Taffy's grid static-position hook has physical horizontal/vertical axes.
/// A vertical CSS grid therefore has to trade its block-axis self-alignment
/// onto Taffy's horizontal self-alignment before it supplies the K5b rectangle.
/// This is deliberately limited to direct out-of-flow grid children. The
/// `self-*` repair below supplies the distinct subject-writing-mode rule.
/// Normal vertical grid layout remains its own formatting work.
fn map_vertical_grid_static_alignment(style: &mut Style, flow: FlowAxes) {
    let align_self = style.align_self.take();
    let justify_self = style.justify_self.take();
    style.align_self = if flow.inline_start() == PhysicalSide::Bottom {
        reverse_self_alignment(justify_self)
    } else {
        justify_self
    };
    style.justify_self = if flow.block_start() == PhysicalSide::Right {
        reverse_self_alignment(align_self)
    } else {
        align_self
    };
}

/// CSS Align gives `self-start` and `self-end` the subject's start and end
/// sides, while `start` and `end` use the containing block's writing mode.
/// Taffy's static-position hook has only physical axes, so repair the two
/// explicit `self-*` values after the ordinary vertical-grid axis mapping.
fn map_grid_static_self_alignment(
    style: &mut Style,
    container_flow: FlowAxes,
    subject_flow: FlowAxes,
    align_self: CssAlignment,
    justify_self: CssAlignment,
) {
    if let Some(alignment) =
        self_alignment_for_axis(align_self, subject_flow, container_flow.block_start())
    {
        set_physical_self_alignment(style, container_flow.block_start(), alignment);
    }
    if let Some(alignment) =
        self_alignment_for_axis(justify_self, subject_flow, container_flow.inline_start())
    {
        set_physical_self_alignment(style, container_flow.inline_start(), alignment);
    }
}

fn self_alignment_for_axis(
    alignment: CssAlignment,
    subject_flow: FlowAxes,
    axis_side: PhysicalSide,
) -> Option<AlignItems> {
    let subject_side = match alignment {
        CssAlignment::SelfStart => subject_side_on_axis(subject_flow, axis_side, true),
        CssAlignment::SelfEnd => subject_side_on_axis(subject_flow, axis_side, false),
        _ => return None,
    };
    Some(align_items(match subject_side {
        PhysicalSide::Top | PhysicalSide::Left => CssAlignment::Start,
        PhysicalSide::Right | PhysicalSide::Bottom => CssAlignment::End,
    }))
}

fn subject_side_on_axis(
    subject_flow: FlowAxes,
    axis_side: PhysicalSide,
    start: bool,
) -> PhysicalSide {
    let inline_start = subject_flow.inline_start();
    if same_physical_axis(inline_start, axis_side) {
        return if start {
            inline_start
        } else {
            subject_flow.inline_end()
        };
    }
    debug_assert!(same_physical_axis(subject_flow.block_start(), axis_side));
    if start {
        subject_flow.block_start()
    } else {
        subject_flow.block_end()
    }
}

fn same_physical_axis(first: PhysicalSide, second: PhysicalSide) -> bool {
    matches!(first, PhysicalSide::Left | PhysicalSide::Right)
        == matches!(second, PhysicalSide::Left | PhysicalSide::Right)
}

fn set_physical_self_alignment(style: &mut Style, axis_side: PhysicalSide, alignment: AlignItems) {
    match axis_side {
        PhysicalSide::Left | PhysicalSide::Right => style.justify_self = Some(alignment),
        PhysicalSide::Top | PhysicalSide::Bottom => style.align_self = Some(alignment),
    }
}

fn reverse_self_alignment(alignment: Option<AlignItems>) -> Option<AlignItems> {
    alignment.map(|mut alignment| {
        alignment.keyword = match alignment.keyword {
            AlignItemsKeyword::Start => AlignItemsKeyword::End,
            AlignItemsKeyword::End => AlignItemsKeyword::Start,
            AlignItemsKeyword::FlexStart => AlignItemsKeyword::FlexEnd,
            AlignItemsKeyword::FlexEnd => AlignItemsKeyword::FlexStart,
            // Reversing an axis swaps its self-relative ends too. These reach
            // here only when the subject's own flow has not already resolved
            // them to a physical side; taffy resolves the pair against an
            // Ltr/Rtl `direction` alone, so the flow-aware resolution above
            // stays responsible for vertical writing modes.
            AlignItemsKeyword::SelfStart => AlignItemsKeyword::SelfEnd,
            AlignItemsKeyword::SelfEnd => AlignItemsKeyword::SelfStart,
            AlignItemsKeyword::Center
            | AlignItemsKeyword::Baseline
            | AlignItemsKeyword::Stretch => alignment.keyword,
        };
        alignment
    })
}

fn grid_auto_flow(value: CssGridAutoFlow) -> GridAutoFlow {
    match value {
        CssGridAutoFlow::Row => GridAutoFlow::Row,
        CssGridAutoFlow::Column => GridAutoFlow::Column,
        CssGridAutoFlow::RowDense => GridAutoFlow::RowDense,
        CssGridAutoFlow::ColumnDense => GridAutoFlow::ColumnDense,
    }
}

fn grid_placement(value: CssGridPlacement) -> GridPlacement {
    match value {
        CssGridPlacement::Auto => GridPlacement::Auto,
        CssGridPlacement::Line(value) => line(value),
        CssGridPlacement::Span(value) => span(value),
    }
}

fn grid_template(value: &CssGridTemplate, em: f32) -> Vec<GridTemplateComponent<String>> {
    match value {
        CssGridTemplate::None => Vec::new(),
        CssGridTemplate::Tracks(tracks) => tracks
            .iter()
            .map(|track| match track {
                CssGridTrack::Auto => auto(),
                CssGridTrack::MinContent => min_content(),
                CssGridTrack::MaxContent => max_content(),
                CssGridTrack::Length(value) => length(value.unit.to_px(value.value, em, 16.0)),
                CssGridTrack::Percent(value) => percent(*value),
                CssGridTrack::Fr(value) => fr(*value),
            })
            .collect(),
    }
}

/// The taffy self-alignment for one axis.
///
/// `auto` normally defers to the parent's items value, which taffy spells
/// `None`. The exception is a size that suppresses stretch: css-align applies
/// `stretch` only when the item's size in that axis computes to `auto`, and
/// Livery maps the content keywords onto `Dimension::auto()` because taffy's
/// safe `Dimension` constructors cannot express them. Without this the item
/// would inherit the container's `stretch` and fill its grid area instead of
/// taking its content size. Resolving to `Start` here is the fallback
/// alignment stretch degrades to.
fn self_alignment(value: CssAlignment, size: CssSize) -> Option<AlignItems> {
    match value {
        CssAlignment::Auto if suppresses_stretch(size) => Some(align_items(CssAlignment::Start)),
        CssAlignment::Auto => None,
        value => Some(align_items(value)),
    }
}

/// Whether a size is not `auto` but reaches taffy as `auto`.
///
/// An explicit length or percentage already defeats stretch on its own, since
/// the definite size wins. Only the content keywords need saying out loud.
fn suppresses_stretch(size: CssSize) -> bool {
    matches!(
        size,
        CssSize::MinContent | CssSize::MaxContent | CssSize::FitContent(_)
    )
}

fn align_items(value: CssAlignment) -> AlignItems {
    AlignItems {
        keyword: match value {
            CssAlignment::Start => AlignItemsKeyword::Start,
            CssAlignment::End => AlignItemsKeyword::End,
            // Taffy has no subject-writing-mode self edge. The narrow direct
            // positioned-grid provider repairs it from the generated box.
            CssAlignment::SelfStart => AlignItemsKeyword::Start,
            CssAlignment::SelfEnd => AlignItemsKeyword::End,
            CssAlignment::FlexStart => AlignItemsKeyword::FlexStart,
            CssAlignment::FlexEnd => AlignItemsKeyword::FlexEnd,
            CssAlignment::Center => AlignItemsKeyword::Center,
            CssAlignment::Baseline => AlignItemsKeyword::Baseline,
            _ => AlignItemsKeyword::Stretch,
        },
        safety: taffy::style::AlignmentSafety::Unsafe,
    }
}

fn align_content(value: CssAlignment) -> AlignContent {
    AlignContent {
        keyword: match value {
            CssAlignment::Start => AlignContentKeyword::Start,
            CssAlignment::End => AlignContentKeyword::End,
            CssAlignment::FlexStart => AlignContentKeyword::FlexStart,
            CssAlignment::FlexEnd => AlignContentKeyword::FlexEnd,
            CssAlignment::Center => AlignContentKeyword::Center,
            CssAlignment::SpaceBetween => AlignContentKeyword::SpaceBetween,
            CssAlignment::SpaceAround => AlignContentKeyword::SpaceAround,
            CssAlignment::SpaceEvenly => AlignContentKeyword::SpaceEvenly,
            _ => AlignContentKeyword::Stretch,
        },
        safety: taffy::style::AlignmentSafety::Unsafe,
    }
}

fn justify_content(value: CssAlignment) -> JustifyContent {
    align_content(value)
}

fn font_size_px(size: &FontSize, parent: f32) -> f32 {
    size.absolute_px()
        .unwrap_or_else(|| match size {
            FontSize::Value(value) => absolute_length_percentage(*value, parent, 16.0, parent),
            _ => unreachable!("absolute font sizes returned a px value"),
        })
        .max(0.0)
}

pub(crate) fn line_height_px(height: &LineHeight, font_size: f32) -> f32 {
    match height {
        LineHeight::Normal => font_size * 1.2,
        LineHeight::Number(value) => font_size * value,
        LineHeight::Value(value) => absolute_length_percentage(*value, font_size, 16.0, font_size),
    }
}

fn dimension(size: CssSize, em: f32) -> Dimension {
    match size {
        CssSize::Value(value) => match value {
            CssLengthPercentage::Percentage(value) => Dimension::percent(value),
            _ => Dimension::length(absolute_length_percentage(value, em, 16.0, 0.0)),
        },
        _ => Dimension::auto(),
    }
}

fn dimension_with_basis(size: CssSize, em: f32, basis: Option<f32>) -> Dimension {
    match (size, basis) {
        (CssSize::Value(CssLengthPercentage::Calc(calc)), Some(basis))
            if calc.percentage != 0.0 =>
        {
            Dimension::length(absolute_length_percentage(
                CssLengthPercentage::Calc(calc),
                em,
                16.0,
                basis,
            ))
        },
        (size, _) => dimension(size, em),
    }
}

fn resolved_child_containing_size(
    computed: &ComputedValues,
    em: f32,
    containing_size: (Option<f32>, Option<f32>),
) -> (Option<f32>, Option<f32>) {
    let fills_available_width = !matches!(
        computed.display,
        CssDisplay::None | CssDisplay::Inline | CssDisplay::InlineBlock
    );
    (
        resolved_explicit_size(computed.width, em, containing_size.0).or(
            if fills_available_width {
                containing_size.0
            } else {
                None
            },
        ),
        resolved_explicit_size(computed.height, em, containing_size.1),
    )
}

fn resolved_explicit_size(size: CssSize, em: f32, basis: Option<f32>) -> Option<f32> {
    let CssSize::Value(value) = size else {
        return None;
    };
    if value.has_percentage() {
        basis.map(|basis| absolute_length_percentage(value, em, 16.0, basis))
    } else {
        Some(absolute_length_percentage(value, em, 16.0, 0.0))
    }
}

fn margin(value: Margin, em: f32) -> LengthPercentageAuto {
    match value {
        Margin::Auto => LengthPercentageAuto::auto(),
        Margin::Value(value) => length_percentage_auto(value, em),
    }
}

fn length_percentage_auto(value: CssLengthPercentage, em: f32) -> LengthPercentageAuto {
    match value {
        CssLengthPercentage::Percentage(value) => LengthPercentageAuto::percent(value),
        _ => LengthPercentageAuto::length(absolute_length_percentage(value, em, 16.0, 0.0)),
    }
}

fn length_percentage(value: CssLengthPercentage, em: f32) -> LengthPercentage {
    match value {
        CssLengthPercentage::Percentage(value) => LengthPercentage::percent(value),
        _ => LengthPercentage::length(absolute_length_percentage(value, em, 16.0, 0.0)),
    }
}

fn gap(value: CssGap, em: f32) -> LengthPercentage {
    length_percentage(value.0, em)
}

fn absolute_length_percentage(
    value: CssLengthPercentage,
    em: f32,
    rem: f32,
    percentage_basis: f32,
) -> f32 {
    match value {
        CssLengthPercentage::Zero => 0.0,
        CssLengthPercentage::Length(length) => absolute_length(length, em, rem),
        CssLengthPercentage::Percentage(value) => percentage_basis * value,
        CssLengthPercentage::Calc(calc) => {
            percentage_basis * calc.percentage + calc.px + calc.em * em + calc.rem * rem
        },
        CssLengthPercentage::Math(math) => {
            CssLengthPercentage::Math(math).to_px(em, rem, percentage_basis)
        },
    }
}

pub(crate) fn length_percentage_px(
    value: CssLengthPercentage,
    em: f32,
    percentage_basis: f32,
) -> f32 {
    absolute_length_percentage(value, em, 16.0, percentage_basis).max(0.0)
}

pub(crate) fn signed_length_percentage_px(
    value: CssLengthPercentage,
    em: f32,
    percentage_basis: f32,
) -> f32 {
    absolute_length_percentage(value, em, 16.0, percentage_basis)
}

fn absolute_length(length: Length, em: f32, rem: f32) -> f32 {
    length.unit.to_px(length.value, em, rem)
}

pub(crate) fn border_width_px(style: BorderStyle, width: BorderWidth, em: f32) -> f32 {
    if matches!(style, BorderStyle::None | BorderStyle::Hidden) {
        return 0.0;
    }
    match width {
        BorderWidth::Thin => 1.0,
        BorderWidth::Medium => 3.0,
        BorderWidth::Thick => 5.0,
        BorderWidth::Length(length) => absolute_length(length, em, 16.0),
    }
    .max(0.0)
}

fn border(style: BorderStyle, width: BorderWidth, em: f32) -> LengthPercentage {
    LengthPercentage::length(border_width_px(style, width, em))
}

fn overflow(value: CssOverflow) -> Overflow {
    match value {
        CssOverflow::Visible => Overflow::Visible,
        CssOverflow::Hidden => Overflow::Hidden,
        CssOverflow::Clip => Overflow::Clip,
        CssOverflow::Scroll | CssOverflow::Auto => Overflow::Scroll,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Device, InteractionStates, StyleSet, emit_paint_list_with_text_system, resolve_styles,
    };
    use genet_static_dom::StaticDocument;
    use paint_list_api::DeviceIntSize;

    fn node_by_id(
        dom: &StaticDocument,
        node: <StaticDocument as LayoutDom>::NodeId,
        id: &str,
    ) -> Option<<StaticDocument as LayoutDom>::NodeId> {
        if dom.attribute(node, &Namespace::from(""), &LocalName::from("id")) == Some(id) {
            return Some(node);
        }
        dom.dom_children(node)
            .find_map(|child| node_by_id(dom, child, id))
    }

    #[test]
    fn html_table_spans_are_normalized_before_buckram_receives_them() {
        let dom = StaticDocument::parse(
            "<table id=table><tbody><tr><td id=first colspan=9001 rowspan=0></td></tr><tr><td id=second></td></tr></tbody></table>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "table { display: table; } tbody { display: table-row-group; } tr { display: table-row; } td { display: table-cell; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let boxes = GeneratedBoxTree::from_dom(&dom, &styles);
        let table = node_by_id(&dom, dom.document(), "table").expect("table");
        let grid = boxes.principal_box(table).expect("table grid");
        let model = build_table_grid(&boxes, &dom, grid);

        assert_eq!(model.cells[0].column_span, 1_000);
        assert_eq!(model.cells[0].row_span, 2);
        assert_eq!(model.cells[1].column, 1_000);
    }

    #[test]
    fn css_display_tables_do_not_consume_html_span_attributes() {
        let dom = StaticDocument::parse(
            "<div id=table><div id=row><div id=cell colspan=9 rowspan=0></div></div></div>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "#table { display: table; } #row { display: table-row; } #cell { display: table-cell; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let boxes = GeneratedBoxTree::from_dom(&dom, &styles);
        let table = node_by_id(&dom, dom.document(), "table").expect("table");
        let grid = boxes.principal_box(table).expect("table grid");
        let model = build_table_grid(&boxes, &dom, grid);

        assert_eq!(model.cells[0].column_span, 1);
        assert_eq!(model.cells[0].row_span, 1);
    }

    #[test]
    fn tables_dispatch_through_buckram_without_a_grid_bridge() {
        let dom = StaticDocument::parse(
            "<table><tbody><tr><td>one</td><td>two</td></tr></tbody></table>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "table { display: table; } tbody { display: table-row-group; } tr { display: table-row; } td { display: table-cell; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );

        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
        let ledger = layout.table_shadow_ledger();
        assert_eq!(
            ledger.assigned, 1,
            "Buckram must assign the table: {ledger:?}"
        );
        assert_eq!(
            ledger.honored, 1,
            "the committed table must honor Buckram columns: {ledger:?}"
        );
        assert_eq!(
            ledger.block.laid_out, 1,
            "Buckram must commit the table block axis: {ledger:?}"
        );
        assert!(
            ledger.skipped.is_empty() && ledger.block.skipped.is_empty(),
            "the basic table may not fall back to a backend route: {ledger:?}"
        );
    }

    #[test]
    fn absolute_static_position_keeps_its_formatting_source_and_k5a_containing_block() {
        let dom = StaticDocument::parse(
            "<div id=containing><div id=source><div id=positioned>item</div></div></div>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "#containing { position: relative; width: 200px; } #source { width: 120px; } \
                 #positioned { position: absolute; left: 36px; top: 11px; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );

        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
        let by_id = |id| node_by_id(&dom, dom.document(), id).expect("node");
        let source = layout
            .boxes()
            .principal_box(by_id("source"))
            .expect("source box");
        let containing = layout
            .boxes()
            .principal_box(by_id("containing"))
            .expect("containing box");
        let positioned = layout
            .boxes()
            .principal_box(by_id("positioned"))
            .expect("positioned box");
        let source_fragment = layout
            .fragments()
            .fragment_ids_for_box(source)
            .first()
            .copied()
            .expect("source fragment");
        let static_position = layout
            .fragments()
            .static_position_for_box(positioned)
            .expect("static-position record");

        assert_eq!(
            static_position.source,
            StaticPositionSource::Fragment(source_fragment),
            "the record must keep the source formatting fragment",
        );
        assert_eq!(
            static_position.containing_block,
            buckram::ContainingBlock::Box(containing),
            "the absolute containing block comes from the K5a graph, not the source parent",
        );
        assert_eq!(
            (
                static_position.logical_rect.inline_start,
                static_position.logical_rect.block_start
            ),
            (0.0, 0.0),
            "the K5b record is the pre-inset static position, not the final absolute location",
        );
        let containing_fragment = layout
            .fragments()
            .fragment_ids_for_box(containing)
            .first()
            .copied()
            .expect("containing fragment");
        let containing_fragment_rect = layout
            .fragments()
            .get(containing_fragment)
            .map(TreeFragment::physical_rect)
            .expect("containing fragment geometry");
        let positioned_fragment = layout
            .fragments()
            .fragments_for_box(positioned)
            .next()
            .expect("positioned fragment");
        assert_eq!(
            (positioned_fragment.x, positioned_fragment.y),
            (
                containing_fragment_rect.x + 36.0,
                containing_fragment_rect.y + 11.0
            ),
            "K5d resolves final insets after K5b records the static rectangle",
        );
        assert_eq!(
            positioned_fragment.containing_fragment(),
            Some(containing_fragment),
            "the final fragment attaches to K5a's selected containing fragment",
        );
    }

    #[test]
    fn absolute_auto_inline_size_fills_between_definite_insets_from_buckram_inputs() {
        let dom = StaticDocument::parse(
            "<div id=containing><div id=positioned>unconstrained content</div></div>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; } \
                 #containing { position: relative; width: 200px; } \
                 #positioned { position: absolute; left: 10px; right: 20px; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );

        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
        let positioned = layout
            .boxes()
            .principal_box(node_by_id(&dom, dom.document(), "positioned").expect("positioned"))
            .expect("positioned box");
        let fragment = layout
            .fragments()
            .fragments_for_box(positioned)
            .next()
            .expect("positioned fragment");

        assert_eq!(fragment.width, 170.0);
        assert_eq!(fragment.x, 10.0);
    }

    #[test]
    fn absolute_nonleaf_reformats_at_buckrams_resolved_inline_size() {
        let dom = StaticDocument::parse(
            "<div id=containing><div id=positioned><div id=child></div></div></div>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; } \
                 #containing { position: relative; width: 200px; height: 100px; } \
                 #positioned { position: absolute; left: 10px; right: 20px; top: 7px; } \
                 #child { width: 40px; height: 20px; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );

        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
        let rect_for = |id| {
            let box_id = layout
                .boxes()
                .principal_box(node_by_id(&dom, dom.document(), id).expect(id))
                .expect("principal box");
            layout
                .fragments()
                .fragments_for_box(box_id)
                .next()
                .map(TreeFragment::physical_rect)
                .expect("fragment")
        };
        let positioned = rect_for("positioned");
        let child = rect_for("child");

        assert_eq!(
            positioned,
            PhysicalRect {
                x: 10.0,
                y: 7.0,
                width: 170.0,
                height: 20.0,
            },
            "the non-leaf root reformats at Buckram's final used width",
        );
        assert_eq!(
            child,
            PhysicalRect {
                x: 10.0,
                y: 7.0,
                width: 40.0,
                height: 20.0,
            },
            "the descendant belongs to the reformatted positioned root",
        );
    }

    #[test]
    fn vertical_absolute_nonleaf_reformats_at_buckrams_resolved_inline_size() {
        let dom = StaticDocument::parse(
            "<div id=containing><div id=positioned><div id=child></div></div></div>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; } \
                 #containing { position: relative; writing-mode: vertical-rl; width: 100px; height: 200px; } \
                 #positioned { position: absolute; writing-mode: vertical-rl; left: 7px; top: 10px; bottom: 20px; } \
                 #child { width: 40px; height: 20px; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );

        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
        let rect_for = |id| {
            let box_id = layout
                .boxes()
                .principal_box(node_by_id(&dom, dom.document(), id).expect(id))
                .expect("principal box");
            layout
                .fragments()
                .fragments_for_box(box_id)
                .next()
                .map(TreeFragment::physical_rect)
                .expect("fragment")
        };
        let positioned = rect_for("positioned");
        let child = rect_for("child");

        assert_eq!(
            positioned,
            PhysicalRect {
                x: 7.0,
                y: 10.0,
                width: 40.0,
                height: 170.0,
            },
            "the vertical non-leaf root reformats at Buckram's final used inline size",
        );
        assert_eq!(
            child,
            PhysicalRect {
                x: 7.0,
                y: 10.0,
                width: 40.0,
                height: 20.0,
            },
            "the descendant belongs to the reformatted vertical positioned root",
        );
    }

    #[test]
    fn absolute_empty_leaf_uses_buckrams_resolved_border_box() {
        let dom = StaticDocument::parse("<div id=containing><div id=positioned></div></div>");
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; } \
                 #containing { position: relative; width: 200px; height: 100px; } \
                 #positioned { position: absolute; left: 10px; right: 20px; top: 7px; height: 30px; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );

        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
        let positioned = layout
            .boxes()
            .principal_box(node_by_id(&dom, dom.document(), "positioned").expect("positioned"))
            .expect("positioned box");
        let fragment = layout
            .fragments()
            .fragments_for_box(positioned)
            .next()
            .expect("positioned fragment");

        assert_eq!(
            fragment.physical_rect(),
            PhysicalRect {
                x: 10.0,
                y: 7.0,
                width: 170.0,
                height: 30.0,
            }
        );
    }

    #[test]
    fn fixed_leaf_percentage_block_size_uses_the_initial_containing_block() {
        let dom = StaticDocument::parse("<div id=fixed></div>");
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; } \
                 #fixed { position: fixed; left: 50px; top: 50px; width: 50%; height: 50%; border: 10px solid; }"]),
            &Device::screen(800.0, 600.0),
            &InteractionStates::default(),
        );

        let layout = layout(&dom, &styles, 800.0, 600.0).expect("layout");
        let fixed = layout
            .get(node_by_id(&dom, dom.document(), "fixed").expect("fixed node"))
            .expect("fixed fragment")
            .physical_rect();

        assert_eq!(
            fixed,
            PhysicalRect {
                x: 50.0,
                y: 50.0,
                width: 420.0,
                height: 320.0,
            }
        );
    }

    #[test]
    fn absolute_non_leaf_percentage_block_size_uses_the_initial_containing_block() {
        let dom = StaticDocument::parse("<body id=positioned><div></div></body>");
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; } \
                 #positioned { position: absolute; left: 50px; top: 50px; width: 50%; height: 50%; border: 10px solid; }"]),
            &Device::screen(800.0, 600.0),
            &InteractionStates::default(),
        );

        let layout = layout(&dom, &styles, 800.0, 600.0).expect("layout");
        let positioned = layout
            .get(node_by_id(&dom, dom.document(), "positioned").expect("positioned node"))
            .expect("positioned fragment")
            .physical_rect();

        assert_eq!(
            positioned,
            PhysicalRect {
                x: 50.0,
                y: 50.0,
                width: 420.0,
                height: 320.0,
            }
        );
    }

    #[test]
    fn ordinary_block_flow_keeps_an_absolute_subtree_out_of_its_cursor() {
        let dom = StaticDocument::parse(
            "<div id=host><div id=before></div><div id=positioned><div id=inside></div></div><div id=after></div></div>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; } \
                 #host { position: relative; width: 200px; } \
                 #before { height: 20px; } \
                 #positioned { position: absolute; left: 25px; width: 80px; } \
                 #inside { height: 30px; } \
                 #after { height: 10px; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );

        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
        let rect = |id| {
            layout
                .get(node_by_id(&dom, dom.document(), id).expect("node"))
                .map(TreeFragment::physical_rect)
                .expect("fragment")
        };
        let host = rect("host");
        let positioned = rect("positioned");
        let after = rect("after");
        let algorithms = layout.block_algorithm_counts();

        assert_eq!(
            host.height, 30.0,
            "the absolute child does not size its block parent"
        );
        assert_eq!((after.x - host.x, after.y - host.y), (0.0, 20.0));
        assert_eq!(
            (
                positioned.x - host.x,
                positioned.y - host.y,
                positioned.width,
                positioned.height
            ),
            (25.0, 20.0, 80.0, 30.0),
        );
        assert_eq!(
            algorithms.taffy, 0,
            "an ordinary block parent and its positioned block subtree stay on Buckram's cursor",
        );
    }

    #[test]
    fn relative_position_moves_its_fragment_subtree_without_reflowing_siblings() {
        let dom = StaticDocument::parse(
            "<div id=relative><div id=child>child</div></div><div id=following>following</div>",
        );
        let static_styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "#relative { width: 120px; } #child { width: 40px; } #following { width: 80px; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let relative_styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "#relative { position: relative; left: 21px; top: 13px; width: 120px; } \
                 #child { width: 40px; } #following { width: 80px; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let static_layout = layout(&dom, &static_styles, 320.0, 240.0).expect("static layout");
        let relative_layout =
            layout(&dom, &relative_styles, 320.0, 240.0).expect("relative layout");
        let box_for = |layout: &LiveryLayout<_>, id| {
            layout
                .boxes()
                .principal_box(node_by_id(&dom, dom.document(), id).expect("node"))
                .expect("principal box")
        };
        let rect_for = |layout: &LiveryLayout<_>, id| {
            layout
                .fragments()
                .fragments_for_box(box_for(layout, id))
                .next()
                .map(TreeFragment::physical_rect)
                .expect("fragment")
        };

        let static_relative = rect_for(&static_layout, "relative");
        let static_child = rect_for(&static_layout, "child");
        let static_following = rect_for(&static_layout, "following");
        let positioned_relative = rect_for(&relative_layout, "relative");
        let positioned_child = rect_for(&relative_layout, "child");
        let positioned_following = rect_for(&relative_layout, "following");

        assert_eq!(
            (positioned_relative.x, positioned_relative.y),
            (static_relative.x + 21.0, static_relative.y + 13.0),
        );
        assert_eq!(
            (positioned_child.x, positioned_child.y),
            (static_child.x + 21.0, static_child.y + 13.0),
            "the containing-block subtree moves with the relative box",
        );
        assert_eq!(
            positioned_following, static_following,
            "relative positioning does not change following normal-flow geometry",
        );
    }

    #[test]
    fn inline_origin_absolute_position_uses_the_line_fragment_as_its_static_source() {
        let dom = StaticDocument::parse(
            "<div id=container>before <span id=source>source <span id=positioned>item</span></span> after</div>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "#container { position: relative; width: 160px; } #source { display: inline; } \
                 #positioned { position: absolute; left: 34px; top: 8px; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );

        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
        let source = layout
            .boxes()
            .principal_box(node_by_id(&dom, dom.document(), "source").expect("source"))
            .expect("source box");
        let positioned = layout
            .boxes()
            .principal_box(node_by_id(&dom, dom.document(), "positioned").expect("positioned"))
            .expect("positioned box");
        let source_fragment = layout
            .fragments()
            .fragment_ids_for_box(source)
            .first()
            .copied()
            .expect("source line fragment");
        let static_position = layout
            .fragments()
            .static_position_for_box(positioned)
            .expect("static position");
        let positioned_fragment = layout
            .fragments()
            .fragments_for_box(positioned)
            .next()
            .expect("positioned fragment");
        let container = layout
            .boxes()
            .principal_box(node_by_id(&dom, dom.document(), "container").expect("container"))
            .expect("container box");
        let container_fragment = layout
            .fragments()
            .fragments_for_box(container)
            .next()
            .expect("container fragment");

        assert_eq!(
            static_position.source,
            StaticPositionSource::Fragment(source_fragment),
            "an inline-origin positioned child uses its line fragment, not a leaf fallback",
        );
        assert_eq!(
            (positioned_fragment.x, positioned_fragment.y),
            (container_fragment.x + 34.0, container_fragment.y + 8.0),
            "the shared K5d route resolves the inline-origin child's final insets",
        );
    }

    #[test]
    fn inline_origin_absolute_auto_width_refits_to_the_k5d_inline_size() {
        let dom = StaticDocument::parse(
            "<div id=container>before <span id=source>source <span id=positioned>one two three four five six seven eight</span></span> after</div>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; } \
                 #container { position: relative; width: 160px; } #source { display: inline; } \
                 #positioned { position: absolute; left: 34px; right: 0; top: 8px; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );

        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let container = layout
            .boxes()
            .principal_box(node_by_id(&dom, dom.document(), "container").expect("container"))
            .expect("container box");
        let positioned = layout
            .boxes()
            .principal_box(node_by_id(&dom, dom.document(), "positioned").expect("positioned"))
            .expect("positioned box");
        let source = layout
            .boxes()
            .principal_box(node_by_id(&dom, dom.document(), "source").expect("source"))
            .expect("source box");
        let container_fragment = layout
            .fragments()
            .fragments_for_box(container)
            .next()
            .expect("container fragment");
        let source_fragment = layout
            .fragments()
            .fragment_ids_for_box(source)
            .first()
            .copied()
            .expect("source line fragment");
        let positioned_fragment = layout
            .fragments()
            .fragments_for_box(positioned)
            .next()
            .expect("positioned fragment");

        assert_eq!(
            layout
                .fragments()
                .static_position_for_box(positioned)
                .expect("static position")
                .source,
            StaticPositionSource::Fragment(source_fragment),
            "the separate formatting root retains its enclosing inline line as the static source",
        );
        assert_eq!(
            (positioned_fragment.x, positioned_fragment.y),
            (container_fragment.x + 34.0, container_fragment.y + 8.0),
        );
        assert_eq!(positioned_fragment.width, 126.0);
        assert!(
            positioned_fragment.height > 20.0,
            "the text reflows at Buckram's 126px used inline size: {positioned_fragment:?}",
        );
    }

    #[test]
    fn inline_origin_fixed_auto_width_refits_to_the_k5d_inline_size() {
        let dom = StaticDocument::parse(
            "<div id=container>before <span id=source>source <span id=positioned>one two three four five six seven eight</span></span> after</div>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; } \
                 #container { width: 160px; } #source { display: inline; } \
                 #positioned { position: fixed; left: 34px; right: 160px; top: 8px; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );

        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let source = layout
            .boxes()
            .principal_box(node_by_id(&dom, dom.document(), "source").expect("source"))
            .expect("source box");
        let positioned = layout
            .boxes()
            .principal_box(node_by_id(&dom, dom.document(), "positioned").expect("positioned"))
            .expect("positioned box");
        let source_fragment = layout
            .fragments()
            .fragment_ids_for_box(source)
            .first()
            .copied()
            .expect("source line fragment");
        let positioned_fragment = layout
            .fragments()
            .fragments_for_box(positioned)
            .next()
            .expect("positioned fragment");

        assert_eq!(
            layout
                .fragments()
                .static_position_for_box(positioned)
                .expect("static position")
                .source,
            StaticPositionSource::Fragment(source_fragment),
        );
        assert_eq!((positioned_fragment.x, positioned_fragment.y), (34.0, 8.0));
        assert_eq!(positioned_fragment.width, 126.0);
        assert!(
            positioned_fragment.height > 20.0,
            "the fixed text reflows at Buckram's 126px used inline size: {positioned_fragment:?}",
        );
    }

    #[test]
    fn absolute_flex_and_grid_children_keep_their_native_static_rectangles() {
        let dom = StaticDocument::parse(
            "<div id=flex><div id=flex-positioned>flex</div></div>\
             <div id=grid><div id=grid-positioned>grid</div></div>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; } \
                 #flex { position: relative; display: flex; width: 200px; height: 100px; \
                         justify-content: center; align-items: end; } \
                 #grid { position: relative; display: grid; width: 200px; height: 100px; } \
                 #flex-positioned, #grid-positioned { position: absolute; left: 18px; top: 9px; \
                                                       width: 30px; height: 20px; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );

        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
        let box_for = |id| {
            layout
                .boxes()
                .principal_box(node_by_id(&dom, dom.document(), id).expect(id))
                .expect("principal box")
        };
        let rect_for = |box_id| {
            layout
                .fragments()
                .fragments_for_box(box_id)
                .next()
                .map(TreeFragment::physical_rect)
                .expect("fragment")
        };

        let flex = box_for("flex");
        let flex_positioned = box_for("flex-positioned");
        let grid = box_for("grid");
        let grid_positioned = box_for("grid-positioned");
        let flex_static = layout
            .fragments()
            .static_position_for_box(flex_positioned)
            .expect("flex static rectangle");
        let grid_static = layout
            .fragments()
            .static_position_for_box(grid_positioned)
            .expect("grid static rectangle");

        assert_eq!(
            (
                flex_static.logical_rect.inline_start,
                flex_static.logical_rect.block_start
            ),
            (85.0, 80.0),
            "the flex formatter owns alignment, while Buckram keeps its pre-inset result"
        );
        assert_eq!(
            (
                grid_static.logical_rect.inline_start,
                grid_static.logical_rect.block_start
            ),
            (0.0, 0.0),
            "the grid formatter contributes its grid-area static rectangle"
        );
        assert_eq!(
            grid_static.containing_block_area,
            Some(LogicalRect {
                inline_start: 0.0,
                block_start: 0.0,
                inline_size: 200.0,
                block_size: 100.0,
            }),
            "the direct grid child retains its finalized containing area separately from its static rectangle"
        );

        let flex_rect = rect_for(flex);
        let flex_positioned_rect = rect_for(flex_positioned);
        let grid_rect = rect_for(grid);
        let grid_positioned_rect = rect_for(grid_positioned);
        assert_eq!(
            (flex_positioned_rect.x, flex_positioned_rect.y),
            (flex_rect.x + 18.0, flex_rect.y + 9.0),
        );
        assert_eq!(
            (grid_positioned_rect.x, grid_positioned_rect.y),
            (grid_rect.x + 18.0, grid_rect.y + 9.0),
        );
    }

    #[test]
    fn absolute_grid_self_end_uses_the_grid_content_end() {
        let dom = StaticDocument::parse("<div id=grid><div id=positioned></div></div>");
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; } \
                 #grid { display: grid; width: 100px; height: 100px; border: 1px solid; } \
                 #positioned { position: absolute; width: 50px; height: 50px; align-self: self-end; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        assert_eq!(
            styles
                .get(node_by_id(&dom, dom.document(), "positioned").expect("positioned node"))
                .map(|style| (style.position, style.align_self)),
            Some((CssPosition::Absolute, CssAlignment::SelfEnd)),
            "the style value must survive parsing before layout maps it to the formatter",
        );

        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
        let grid = layout
            .boxes()
            .principal_box(node_by_id(&dom, dom.document(), "grid").expect("grid node"))
            .expect("grid box");
        let positioned = layout
            .boxes()
            .principal_box(node_by_id(&dom, dom.document(), "positioned").expect("positioned node"))
            .expect("positioned box");
        let grid_rect = layout
            .fragments()
            .fragments_for_box(grid)
            .next()
            .map(TreeFragment::physical_rect)
            .expect("grid fragment");
        let positioned_rect = layout
            .fragments()
            .fragments_for_box(positioned)
            .next()
            .map(TreeFragment::physical_rect)
            .expect("positioned fragment");
        assert_eq!(
            (positioned_rect.x, positioned_rect.y),
            (grid_rect.x + 1.0, grid_rect.y + 51.0),
            "a same-flow self-end positioned grid item uses the grid content end",
        );
        assert!(
            layout
                .fragments()
                .static_position_for_box(positioned)
                .is_some(),
            "the grid static-position route retains its K5b record",
        );
    }

    #[test]
    fn absolute_grid_static_position_uses_the_placed_area_when_the_grid_is_its_containing_block() {
        let dom = StaticDocument::parse("<div id=grid><div id=positioned></div></div>");
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; } \
                 #grid { position: relative; display: grid; width: 100px; height: 100px; \
                         grid-template-columns: 20px 80px; grid-template-rows: 30px 70px; } \
                 #positioned { position: absolute; grid-area: 2 / 2 / 3 / 3; \
                               width: 20px; height: 10px; align-self: self-end; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );

        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
        let grid = layout
            .boxes()
            .principal_box(node_by_id(&dom, dom.document(), "grid").expect("grid node"))
            .expect("grid box");
        let positioned = layout
            .boxes()
            .principal_box(node_by_id(&dom, dom.document(), "positioned").expect("positioned node"))
            .expect("positioned box");
        let grid_rect = layout
            .fragments()
            .fragments_for_box(grid)
            .next()
            .map(TreeFragment::physical_rect)
            .expect("grid fragment");
        let positioned_rect = layout
            .fragments()
            .fragments_for_box(positioned)
            .next()
            .map(TreeFragment::physical_rect)
            .expect("positioned fragment");
        let static_position = layout
            .fragments()
            .static_position_for_box(positioned)
            .expect("grid static position");

        assert_eq!(
            (
                static_position.logical_rect.inline_start,
                static_position.logical_rect.block_start,
            ),
            (20.0, 90.0),
            "CSS Grid 9.2: a grid that generates the containing block aligns the static \
             rectangle in the placed grid area, not its content box"
        );
        assert_eq!(
            static_position.containing_block_area,
            Some(LogicalRect {
                inline_start: 20.0,
                block_start: 30.0,
                inline_size: 80.0,
                block_size: 70.0,
            }),
            "the placed grid area remains the containing block for positioned insets"
        );
        assert_eq!(
            (positioned_rect.x, positioned_rect.y),
            (grid_rect.x + 20.0, grid_rect.y + 90.0),
        );
    }

    #[test]
    fn absolute_grid_static_position_uses_content_edges_when_the_containing_block_is_elsewhere() {
        let dom = StaticDocument::parse(
            "<div id=outer><div id=grid><div id=positioned></div></div></div>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; } \
                 #outer { position: relative; width: 100px; height: 100px; } \
                 #grid { display: grid; width: 100px; height: 100px; \
                         grid-template-columns: 20px 80px; grid-template-rows: 30px 70px; } \
                 #positioned { position: absolute; grid-area: 2 / 2 / 3 / 3; \
                               width: 20px; height: 10px; align-self: self-end; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );

        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
        let grid = layout
            .boxes()
            .principal_box(node_by_id(&dom, dom.document(), "grid").expect("grid node"))
            .expect("grid box");
        let positioned = layout
            .boxes()
            .principal_box(node_by_id(&dom, dom.document(), "positioned").expect("positioned node"))
            .expect("positioned box");
        let grid_rect = layout
            .fragments()
            .fragments_for_box(grid)
            .next()
            .map(TreeFragment::physical_rect)
            .expect("grid fragment");
        let positioned_rect = layout
            .fragments()
            .fragments_for_box(positioned)
            .next()
            .map(TreeFragment::physical_rect)
            .expect("positioned fragment");
        let static_position = layout
            .fragments()
            .static_position_for_box(positioned)
            .expect("grid static position");

        assert_eq!(
            (
                static_position.logical_rect.inline_start,
                static_position.logical_rect.block_start,
            ),
            (0.0, 90.0),
            "CSS Grid 9.2: a grid that is only the static-position parent aligns the static \
             rectangle in its content box; its placement lines do not apply"
        );
        assert_eq!(
            (positioned_rect.x, positioned_rect.y),
            (grid_rect.x, grid_rect.y + 90.0),
        );
    }

    #[test]
    fn vertical_grid_static_alignment_uses_the_placed_area_block_end() {
        let dom = StaticDocument::parse("<div id=grid><div id=positioned></div></div>");
        for (writing_mode, expected_x) in [("vertical-rl", 0.0), ("vertical-lr", 80.0)] {
            let styles = resolve_styles(
                &dom,
                &StyleSet::cambium(&[&format!(
                    "html, body, div {{ margin: 0; padding: 0; }} \
                         #grid {{ position: relative; display: grid; writing-mode: {writing_mode}; \
                                 width: 100px; height: 80px; \
                                 grid-template-columns: 20px 60px; grid-template-rows: 30px 70px; }} \
                         #positioned {{ position: absolute; grid-area: 2 / 2 / 3 / 3; \
                                       width: 20px; height: 10px; align-self: end; }}"
                )]),
                &Device::screen(320.0, 240.0),
                &InteractionStates::default(),
            );

            let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
            let grid = layout
                .boxes()
                .principal_box(node_by_id(&dom, dom.document(), "grid").expect("grid node"))
                .expect("grid box");
            let positioned = layout
                .boxes()
                .principal_box(
                    node_by_id(&dom, dom.document(), "positioned").expect("positioned node"),
                )
                .expect("positioned box");
            let grid_rect = layout
                .fragments()
                .fragments_for_box(grid)
                .next()
                .map(TreeFragment::physical_rect)
                .expect("grid fragment");
            let positioned_rect = layout
                .fragments()
                .fragments_for_box(positioned)
                .next()
                .map(TreeFragment::physical_rect)
                .expect("positioned fragment");
            assert_eq!(
                (positioned_rect.x, positioned_rect.y),
                (grid_rect.x + expected_x, grid_rect.y + 20.0),
                "{writing_mode}: block-end alignment uses the placed row's physical end edge, \
                 and the inline start is the placed column's start"
            );
        }
    }

    #[test]
    fn grid_static_self_alignment_uses_the_subject_writing_mode() {
        let dom = StaticDocument::parse("<div id=grid><div id=positioned></div></div>");
        let scenarios = [
            (
                "vertical grid, horizontal subject self-start",
                "writing-mode: vertical-rl;",
                "writing-mode: horizontal-tb; align-self: self-start;",
                (0.0, 0.0),
            ),
            (
                "vertical grid, horizontal subject self-end",
                "writing-mode: vertical-rl;",
                "writing-mode: horizontal-tb; align-self: self-end;",
                (80.0, 0.0),
            ),
            (
                "horizontal grid, vertical rtl subject self-start",
                "writing-mode: horizontal-tb;",
                "writing-mode: vertical-rl; direction: rtl; align-self: self-start;",
                (0.0, 70.0),
            ),
            (
                "horizontal grid, vertical rtl subject self-end",
                "writing-mode: horizontal-tb;",
                "writing-mode: vertical-rl; direction: rtl; align-self: self-end;",
                (0.0, 0.0),
            ),
            (
                "horizontal grid, vertical rl subject justify self-start",
                "writing-mode: horizontal-tb;",
                "writing-mode: vertical-rl; justify-self: self-start;",
                (80.0, 0.0),
            ),
            (
                "horizontal grid, vertical rl subject justify self-end",
                "writing-mode: horizontal-tb;",
                "writing-mode: vertical-rl; justify-self: self-end;",
                (0.0, 0.0),
            ),
            (
                "vertical grid, vertical rtl subject justify self-start",
                "writing-mode: vertical-rl;",
                "writing-mode: vertical-rl; direction: rtl; justify-self: self-start;",
                (0.0, 70.0),
            ),
            (
                "vertical grid, vertical rtl subject justify self-end",
                "writing-mode: vertical-rl;",
                "writing-mode: vertical-rl; direction: rtl; justify-self: self-end;",
                (0.0, 0.0),
            ),
        ];

        for (description, grid_writing_mode, subject_writing_mode, expected) in scenarios {
            let styles = resolve_styles(
                &dom,
                &StyleSet::cambium(&[&format!(
                    "html, body, div {{ margin: 0; padding: 0; }} \
                     #grid {{ position: relative; display: grid; {grid_writing_mode} \
                             width: 100px; height: 80px; }} \
                     #positioned {{ position: absolute; {subject_writing_mode} \
                                   width: 20px; height: 10px; }}"
                )]),
                &Device::screen(320.0, 240.0),
                &InteractionStates::default(),
            );
            let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
            let grid = layout
                .boxes()
                .principal_box(node_by_id(&dom, dom.document(), "grid").expect("grid node"))
                .expect("grid box");
            let positioned = layout
                .boxes()
                .principal_box(
                    node_by_id(&dom, dom.document(), "positioned").expect("positioned node"),
                )
                .expect("positioned box");
            let grid_rect = layout
                .fragments()
                .fragments_for_box(grid)
                .next()
                .map(TreeFragment::physical_rect)
                .expect("grid fragment");
            let positioned_rect = layout
                .fragments()
                .fragments_for_box(positioned)
                .next()
                .map(TreeFragment::physical_rect)
                .expect("positioned fragment");

            assert_eq!(
                (
                    positioned_rect.x - grid_rect.x,
                    positioned_rect.y - grid_rect.y
                ),
                expected,
                "{description} aligns to the subject's corresponding start or end side",
            );
        }
    }

    #[test]
    fn positioned_grid_area_transforms_from_flow_relative_tracks_to_physical_insets() {
        let dom = StaticDocument::parse("<div id=grid><div id=positioned></div></div>");
        for (writing_mode, direction, expected) in [
            ("vertical-rl", "ltr", (10.0, 25.0)),
            ("vertical-lr", "ltr", (40.0, 25.0)),
            ("vertical-rl", "rtl", (10.0, 5.0)),
            ("vertical-lr", "rtl", (40.0, 5.0)),
        ] {
            let styles = resolve_styles(
                &dom,
                &StyleSet::cambium(&[&format!(
                    "html, body, div {{ margin: 0; padding: 0; }} \
                     #grid {{ position: relative; display: grid; writing-mode: {writing_mode}; \
                             direction: {direction}; width: 100px; height: 80px; \
                             grid-template-columns: 20px 60px; grid-template-rows: 30px 70px; }} \
                     #positioned {{ position: absolute; grid-area: 2 / 2 / 3 / 3; \
                                   left: 10px; right: 20px; top: 5px; bottom: 15px; \
                                   width: 40px; height: 40px; }}"
                )]),
                &Device::screen(320.0, 240.0),
                &InteractionStates::default(),
            );
            let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
            let grid = layout
                .boxes()
                .principal_box(node_by_id(&dom, dom.document(), "grid").expect("grid node"))
                .expect("grid box");
            let positioned = layout
                .boxes()
                .principal_box(
                    node_by_id(&dom, dom.document(), "positioned").expect("positioned node"),
                )
                .expect("positioned box");
            let grid_rect = layout
                .fragments()
                .fragments_for_box(grid)
                .next()
                .map(TreeFragment::physical_rect)
                .expect("grid fragment");
            let positioned_rect = layout
                .fragments()
                .fragments_for_box(positioned)
                .next()
                .map(TreeFragment::physical_rect)
                .expect("positioned fragment");
            let static_position = layout
                .fragments()
                .static_position_for_box(positioned)
                .expect("grid static position");

            assert_eq!(
                static_position.containing_block_area,
                Some(LogicalRect {
                    inline_start: 20.0,
                    block_start: 30.0,
                    inline_size: 60.0,
                    block_size: 70.0,
                }),
                "{writing_mode} {direction}: the finalized area is stored in the grid's logical coordinates",
            );
            assert_eq!(
                (
                    positioned_rect.x - grid_rect.x,
                    positioned_rect.y - grid_rect.y,
                    positioned_rect.width,
                    positioned_rect.height,
                ),
                (expected.0, expected.1, 40.0, 40.0),
                "{writing_mode} {direction}: physical insets resolve inside the transformed grid area",
            );
        }
    }

    #[test]
    fn positioned_child_uses_the_positioned_ancestor_padding_box() {
        let dom = StaticDocument::parse("<div id=containing><div id=positioned></div></div>");
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; } \
                 #containing { position: relative; width: 100px; height: 100px; \
                               border-style: solid; border-top-width: 5px; \
                               border-right-width: 10px; border-bottom-width: 15px; \
                               border-left-width: 20px; } \
                 #positioned { position: absolute; width: 100%; height: 100%; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );

        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
        let box_for = |id| {
            layout
                .boxes()
                .principal_box(node_by_id(&dom, dom.document(), id).expect(id))
                .expect("principal box")
        };
        let rect_for = |box_id| {
            layout
                .fragments()
                .fragments_for_box(box_id)
                .next()
                .map(TreeFragment::physical_rect)
                .expect("fragment")
        };

        let containing = rect_for(box_for("containing"));
        let positioned = rect_for(box_for("positioned"));
        assert_eq!((containing.width, containing.height), (130.0, 120.0));
        assert_eq!(
            (
                positioned.x,
                positioned.y,
                positioned.width,
                positioned.height
            ),
            (20.0, 5.0, 100.0, 100.0),
            "percentage sizes and auto insets resolve against the padding box"
        );
    }

    #[test]
    fn positioned_child_of_a_split_inline_uses_first_and_last_content_edges() {
        let dom = StaticDocument::parse(
            "<div id=host><span id=containing>one two three four five six <span id=start></span><span id=end></span></span></div>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; } \
                 #host { width: 70px; font-size: 10px; line-height: 10px; } \
                 #containing { display: inline; position: relative; padding: 3px 7px 11px 13px; border: 2px solid; } \
                 #start, #end { position: absolute; width: 1px; height: 1px; } \
                 #start { top: 0; left: 0; } #end { right: 0; bottom: 0; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );

        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let box_for = |id| {
            layout
                .boxes()
                .principal_box(node_by_id(&dom, dom.document(), id).expect(id))
                .expect("principal box")
        };
        let containing_fragments = layout
            .fragments()
            .fragments_for_box(box_for("containing"))
            .map(TreeFragment::physical_rect)
            .collect::<Vec<_>>();
        let positioned = |id| {
            layout
                .fragments()
                .fragments_for_box(box_for(id))
                .next()
                .map(TreeFragment::physical_rect)
                .expect("positioned fragment")
        };
        assert!(
            containing_fragments.len() >= 2,
            "the positioned inline must fragment across multiple lines: {containing_fragments:?}"
        );
        let first = containing_fragments.first().expect("first fragment");
        let last = containing_fragments.last().expect("last fragment");
        assert_eq!(
            positioned("start"),
            PhysicalRect {
                x: first.x + 15.0,
                y: first.y + 5.0,
                width: 1.0,
                height: 1.0,
            }
        );
        assert_eq!(
            positioned("end"),
            PhysicalRect {
                x: last.x + last.width - 10.0,
                y: last.y + last.height - 14.0,
                width: 1.0,
                height: 1.0,
            }
        );
    }

    #[test]
    fn positioned_child_of_inline_split_by_a_block_uses_all_continuations() {
        let dom = StaticDocument::parse(
            "<div id=container><div id=before></div>B<span id=containing><div id=split></div>AA<span id=positioned></span></span></div>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; } \
                 #container { font-size: 20px; line-height: 20px; width: 100px; height: 100px; } \
                 #before { height: 60px; } #split { height: 0; } \
                 #containing { display: inline; position: relative; } \
                 #positioned { position: absolute; left: 0; top: -60px; width: 100px; height: 100px; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let box_for = |id| {
            layout
                .boxes()
                .principal_box(node_by_id(&dom, dom.document(), id).expect(id))
                .expect("principal box")
        };
        let first_containing_fragment = layout
            .fragments()
            .fragments_for_box(box_for("containing"))
            .next()
            .map(TreeFragment::physical_rect)
            .expect("first containing fragment");
        let positioned = box_for("positioned");
        assert_eq!(
            layout
                .boxes()
                .boxes_for_node(node_by_id(&dom, dom.document(), "containing").expect("containing"))
                .len(),
            2,
            "the in-flow block produces a generated inline continuation"
        );
        assert_eq!(
            layout
                .fragments()
                .fragments_for_box(positioned)
                .next()
                .map(TreeFragment::physical_rect),
            Some(PhysicalRect {
                x: first_containing_fragment.x,
                y: first_containing_fragment.y - 60.0,
                width: 100.0,
                height: 100.0,
            }),
            "the -60px top inset resolves from the first continuation, not the child-owning continuation"
        );
    }

    #[test]
    fn absolute_static_position_in_a_block_split_from_inline_includes_margins() {
        let dom = StaticDocument::parse(
            "<div id=before></div><div id=wrapper><span><div id=block><div id=positioned></div></div></span></div>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body { margin: 0; padding: 0; } \
                 #before { height: 50px; } \
                 #wrapper { display: flow-root; margin-top: -100px; } \
                 #block { margin-top: 100px; } \
                 #positioned { position: absolute; width: 100px; height: 100px; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );

        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
        let rect = |id| {
            layout
                .get(node_by_id(&dom, dom.document(), id).expect("node"))
                .expect("fragment")
                .physical_rect()
        };
        let positioned = rect("positioned");

        assert_eq!(
            positioned.y,
            50.0,
            "before={:?}, wrapper={:?}, block={:?}, positioned={positioned:?}",
            rect("before"),
            rect("wrapper"),
            rect("block"),
        );
    }

    #[test]
    fn absolute_siblings_in_one_inline_keep_an_empty_first_fragment() {
        let dom = StaticDocument::parse(
            "<div id=container><span id=prefix>BBBBBB</span> <span id=containing><div id=first></div>AA A AA AAAA<div id=second></div></span></div>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; } \
                 #container { font-size: 20px; line-height: 20px; width: 100px; height: 100px; } \
                 #containing { display: inline; position: relative; } \
                 #first, #second { position: absolute; top: 0; width: 50px; height: 100px; } \
                 #first { left: -30px; } #second { left: -80px; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let box_for = |id| {
            layout
                .boxes()
                .principal_box(node_by_id(&dom, dom.document(), id).expect(id))
                .expect("principal box")
        };
        let rect_for = |id| {
            layout
                .fragments()
                .fragments_for_box(box_for(id))
                .next()
                .map(TreeFragment::physical_rect)
                .expect("positioned fragment")
        };
        let prefix = layout
            .fragments()
            .fragments_for_box(box_for("prefix"))
            .next()
            .map(TreeFragment::physical_rect)
            .expect("prefix fragment");
        let containing_fragments = layout
            .fragments()
            .fragments_for_box(box_for("containing"))
            .map(TreeFragment::physical_rect)
            .collect::<Vec<_>>();
        assert_eq!(
            rect_for("first"),
            PhysicalRect {
                x: prefix.x + prefix.width - 30.0,
                y: prefix.y,
                width: 50.0,
                height: 100.0,
            },
            "prefix={prefix:?}, containing={containing_fragments:?}"
        );
        assert_eq!(
            rect_for("second"),
            PhysicalRect {
                x: prefix.x + prefix.width - 80.0,
                y: prefix.y,
                width: 50.0,
                height: 100.0,
            }
        );
    }

    #[test]
    fn positioned_hit_test_respects_stacking_level_and_ancestor_clip() {
        let dom = StaticDocument::parse(
            "<div id=host><div id=behind></div><div id=normal></div><div id=front></div></div>\
             <div id=clip><div id=overlay></div></div>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; } \
                 #host { position: relative; width: 100px; height: 100px; } \
                 #behind, #front { position: absolute; left: 0; top: 0; width: 80px; height: 80px; } \
                 #behind { z-index: -1; } #normal { width: 80px; height: 80px; } \
                 #front { z-index: 1; } \
                 #clip { position: relative; width: 50px; height: 50px; overflow: hidden; } \
                 #overlay { position: absolute; left: 0; top: 0; width: 100px; height: 100px; z-index: 1; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
        let node = |id| node_by_id(&dom, dom.document(), id).expect(id);

        assert_eq!(
            hit_test(&dom, &styles, &layout, 10.0, 10.0),
            Some(node("front"))
        );
        assert_eq!(
            hit_test(&dom, &styles, &layout, 10.0, 110.0),
            Some(node("overlay"))
        );
        assert_ne!(
            hit_test(&dom, &styles, &layout, 75.0, 110.0),
            Some(node("overlay"))
        );
    }

    #[test]
    fn positioned_descendant_paints_above_its_stacking_context_background() {
        let dom = StaticDocument::parse(
            "<div id=card><div id=collapse></div><div id=editor></div></div>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; } \
                 #card { position: relative; width: 80px; height: 80px; z-index: 4; } \
                 #collapse, #editor { position: absolute; left: 0; top: 0; width: 80px; height: 80px; } \
                 #collapse { z-index: 2; } #editor { z-index: 0; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");

        assert_eq!(
            hit_test(&dom, &styles, &layout, 10.0, 10.0),
            Some(node_by_id(&dom, dom.document(), "collapse").expect("collapse node")),
            "a child z-index is ordered within its parent's context, above the parent and lower siblings",
        );
    }

    #[test]
    fn nested_absolute_card_keeps_its_editor_in_the_card_region() {
        let dom = StaticDocument::parse(
            "<div id=canvas><div id=card-root><div id=layer><div id=card><button id=collapse>Collapse</button><div id=editor>Card controls</div></div></div></div></div>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; } \
                 #canvas { position: relative; width: 520px; height: 260px; } \
                 #card-root { position: absolute; left: 0; top: 0; z-index: 4; } \
                 #layer { position: absolute; left: 100px; top: 60px; width: 150px; height: 160px; z-index: 4; } \
                 #card { position: absolute; left: 0; top: 0; width: 100%; height: 100%; \
                         box-sizing: border-box; overflow: hidden; padding: 10px; z-index: 4; } \
                 #collapse { position: absolute; right: 8px; top: 8px; z-index: 2; } \
                 #editor { display: flex; flex-wrap: wrap; }"]),
            &Device::screen(640.0, 480.0),
            &InteractionStates::default(),
        );
        let layout = layout(&dom, &styles, 640.0, 480.0).expect("layout");
        let rect = |id| {
            layout
                .get(node_by_id(&dom, dom.document(), id).expect(id))
                .expect("fragment")
        };
        let layer = rect("layer");
        let card = rect("card");
        let collapse = rect("collapse");
        let editor = rect("editor");

        assert!((card.x - layer.x).abs() < 0.01 && (card.y - layer.y).abs() < 0.01);
        assert!(
            (card.width - layer.width).abs() < 0.01 && (card.height - layer.height).abs() < 0.01
        );
        assert!(editor.x >= card.x && editor.y >= card.y);
        assert!(editor.x + editor.width <= card.x + card.width + 0.01);
        assert!(editor.y + editor.height <= card.y + card.height + 0.01);
        assert!(collapse.x >= card.x && collapse.y >= card.y);
        assert!(collapse.x + collapse.width <= card.x + card.width + 0.01);
        assert!(collapse.y + collapse.height <= card.y + card.height + 0.01);
    }

    #[test]
    fn static_block_z_index_keeps_normal_hit_order() {
        let dom =
            StaticDocument::parse("<div id=host><div id=front></div><div id=normal></div></div>");
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; } \
                 #host { width: 80px; height: 80px; } \
                 #front { width: 80px; height: 80px; margin-bottom: -80px; z-index: 1; } \
                 #normal { width: 80px; height: 80px; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");

        assert_eq!(
            hit_test(&dom, &styles, &layout, 10.0, 10.0),
            Some(node_by_id(&dom, dom.document(), "normal").expect("normal node")),
            "a static block's numeric z-index does not outrank later normal content",
        );
    }

    #[test]
    fn grid_item_order_changes_the_topmost_hit_target() {
        let dom =
            StaticDocument::parse("<div id=grid><div id=later></div><div id=earlier></div></div>");
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; } \
                 #grid { display: grid; width: 80px; height: 80px; \
                         grid-template-columns: 80px; grid-template-rows: 80px; } \
                 #later, #earlier { grid-area: 1 / 1 / 2 / 2; width: 80px; height: 80px; } \
                 #later { order: 1; } #earlier { order: -1; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");

        assert_eq!(
            hit_test(&dom, &styles, &layout, 10.0, 10.0),
            Some(node_by_id(&dom, dom.document(), "later").expect("later node")),
            "the item painted last in order-modified order receives the hit",
        );
    }

    #[test]
    fn fixed_position_uses_a_transform_fixed_containing_block() {
        let dom = StaticDocument::parse("<div id=trigger><div id=fixed>item</div></div>");
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "#trigger { transform: translateX(0px); margin-left: 40px; margin-top: 20px; \
                 width: 120px; height: 60px; } \
                 #fixed { position: fixed; left: 17px; top: 9px; width: 30px; height: 10px; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );

        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
        let trigger = layout
            .boxes()
            .principal_box(node_by_id(&dom, dom.document(), "trigger").expect("trigger"))
            .expect("trigger box");
        let fixed = layout
            .boxes()
            .principal_box(node_by_id(&dom, dom.document(), "fixed").expect("fixed"))
            .expect("fixed box");
        let trigger_fragment = layout
            .fragments()
            .fragments_for_box(trigger)
            .next()
            .expect("trigger fragment");
        let fixed_fragment = layout
            .fragments()
            .fragments_for_box(fixed)
            .next()
            .expect("fixed fragment");

        assert_eq!(
            (fixed_fragment.x, fixed_fragment.y),
            (trigger_fragment.x + 17.0, trigger_fragment.y + 9.0),
        );
        assert_eq!(
            fixed_fragment.containing_fragment(),
            layout
                .fragments()
                .fragment_ids_for_box(trigger)
                .first()
                .copied(),
        );
    }

    #[test]
    fn absolute_position_converts_between_vertical_static_and_containing_flows() {
        let dom = StaticDocument::parse("<div id=container><div id=positioned>item</div></div>");
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "#container { position: relative; writing-mode: vertical-rl; \
                 width: 120px; height: 100px; } \
                 #positioned { position: absolute; left: 13px; top: 8px; \
                 width: 20px; height: 30px; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );

        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
        let container = layout
            .boxes()
            .principal_box(node_by_id(&dom, dom.document(), "container").expect("container"))
            .expect("container box");
        let positioned = layout
            .boxes()
            .principal_box(node_by_id(&dom, dom.document(), "positioned").expect("positioned"))
            .expect("positioned box");
        let container_fragment = layout
            .fragments()
            .fragments_for_box(container)
            .next()
            .expect("container fragment");
        let positioned_fragment = layout
            .fragments()
            .fragments_for_box(positioned)
            .next()
            .expect("positioned fragment");

        assert_eq!(
            (positioned_fragment.x, positioned_fragment.y),
            (container_fragment.x + 13.0, container_fragment.y + 8.0),
            "physical insets retain their sides while K5d changes coordinate systems",
        );
        assert_eq!(
            positioned_fragment.containing_fragment(),
            layout
                .fragments()
                .fragment_ids_for_box(container)
                .first()
                .copied(),
        );
    }

    #[test]
    fn relative_table_parts_move_their_retained_fragment_subtree() {
        let dom = StaticDocument::parse(
            "<table id=table><tbody id=group><tr id=row><td id=cell>one</td></tr></tbody>\
             <tbody><tr><td>two</td></tr></tbody></table>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "table { display: table; border-collapse: collapse; border-spacing: 0; } \
                 tbody { display: table-row-group; } tr { display: table-row; } \
                 #group { position: relative; left: 12px; top: 8px; } \
                 #row { position: relative; left: 7px; top: 100px; } \
                 td { display: table-cell; width: 40px; height: 20px; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );

        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
        let by_id = |id| node_by_id(&dom, dom.document(), id).expect("node");
        let group = layout.get(by_id("group")).expect("row-group fragment");
        let row = layout.get(by_id("row")).expect("row fragment");
        let cell = layout.get(by_id("cell")).expect("cell fragment");

        assert_eq!((row.x - group.x, row.y - group.y), (7.0, 100.0));
        assert_eq!((cell.x, cell.y), (row.x, row.y));
        assert!(
            group.x > layout.principal_fragment(by_id("table")).expect("grid").x,
            "the row-group's offset must survive flattening"
        );
    }

    #[test]
    fn html_align_descendants_adjusts_used_margins_without_rewriting_computed_css() {
        let dom = StaticDocument::parse(
            r#"
                <div style="width: 300px">
                  <div align="right"><div id="right" style="width: 100px; margin: 10px">right</div></div>
                  <center><div id="center" style="width: 100px; margin: 10px">center</div></center>
                  <div align="left" style="direction: rtl"><div id="rtl-left" style="width: 100px; margin: 10px">rtl</div></div>
                  <div align="right"><div id="auto" style="margin: 10px">auto</div></div>
                </div>
            "#,
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body { margin: 0; }"]),
            &Device::screen(300.0, 240.0),
            &InteractionStates::default(),
        );
        let layout = layout(&dom, &styles, 300.0, 240.0).expect("layout");
        let by_id = |id| node_by_id(&dom, dom.document(), id).expect("node");

        assert_eq!(layout.get(by_id("right")).expect("right").x, 190.0);
        assert_eq!(layout.get(by_id("center")).expect("center").x, 100.0);
        assert_eq!(
            layout.get(by_id("rtl-left")).expect("rtl left").x,
            10.0,
            "line-left remains physical left in horizontal RTL"
        );
        assert_eq!(
            layout.get(by_id("auto")).expect("auto width").x,
            10.0,
            "width:auto is outside the legacy over-constrained rule"
        );
        assert_eq!(
            styles.get(by_id("right")).unwrap().margin_left,
            Margin::Value(CssLengthPercentage::Length(Length::px(10.0))),
            "the adjustment must remain a used value"
        );
    }

    #[test]
    fn absolute_table_root_uses_shared_k5d_wrapper_geometry() {
        let dom =
            StaticDocument::parse("<table id=table><tbody><tr><td>one</td></tr></tbody></table>");
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "table { display: table; position: absolute; left: 31px; top: 14px; border-spacing: 0; } \
                 tbody { display: table-row-group; } tr { display: table-row; } \
                 td { display: table-cell; width: 40px; height: 20px; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );

        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
        let table = node_by_id(&dom, dom.document(), "table").expect("table");
        let table_box = layout.boxes().principal_box(table).expect("table grid");
        let positioned_wrapper = layout
            .boxes()
            .boxes_for_node(table)
            .iter()
            .copied()
            .find(|box_id| layout.boxes()[*box_id].positioning == PositioningScheme::Absolute)
            .expect("the table wrapper carries the table root's positioning");
        let wrapper_fragment = layout
            .fragments()
            .fragments_for_box(positioned_wrapper)
            .next()
            .expect("positioned wrapper fragment");
        assert_eq!((wrapper_fragment.x, wrapper_fragment.y), (31.0, 14.0));
        let ledger = layout.table_shadow_ledger();
        assert!(
            !ledger
                .positioning_gaps
                .contains(&crate::table_shadow::TablePositioningGapRecord {
                    table: table_box,
                    part: table_box,
                    gap: crate::table_shadow::TablePositioningGap::Absolute,
                }),
            "the shared wrapper route replaces the root-only table positioning gap: {ledger:?}"
        );
        assert_eq!(
            ledger.block.laid_out, 1,
            "the table stays on Buckram: {ledger:?}"
        );
    }

    #[test]
    fn absolute_table_caption_uses_shared_k5d_wrapper_geometry() {
        let dom = StaticDocument::parse(
            "<table id=table><caption id=caption>caption</caption><tbody><tr><td>cell</td></tr></tbody></table>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                r#"table { display: table; position: relative; border-spacing: 0; }
                   caption { display: table-caption; position: absolute; left: 31px; top: 14px; width: 240px; height: 20px; }
                   tbody { display: table-row-group; } tr { display: table-row; }
                   td { display: table-cell; width: 80px; height: 20px; }"#,
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );

        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
        let table = node_by_id(&dom, dom.document(), "table").expect("table");
        let caption = node_by_id(&dom, dom.document(), "caption").expect("caption");
        let table_grid = layout.boxes().principal_box(table).expect("table grid");
        let wrapper = layout.boxes()[table_grid].parent().expect("table wrapper");
        let caption_box = layout.boxes().principal_box(caption).expect("caption box");
        assert_eq!(
            layout.boxes()[caption_box].positioning,
            PositioningScheme::Absolute,
        );
        let wrapper_fragment = layout
            .fragments()
            .fragments_for_box(wrapper)
            .next()
            .expect("wrapper fragment");
        let grid_fragment = layout
            .fragments()
            .fragments_for_box(table_grid)
            .next()
            .expect("grid fragment");
        let caption_fragment = layout
            .fragments()
            .fragments_for_box(caption_box)
            .next()
            .expect("caption fragment");
        let caption_static = layout
            .fragments()
            .static_position_for_box(caption_box)
            .expect("caption static-position record");

        assert_eq!(
            (caption_fragment.x, caption_fragment.y),
            (wrapper_fragment.x + 31.0, wrapper_fragment.y + 14.0),
            "the caption uses the wrapper containing block rather than table tracks",
        );
        assert_eq!(
            wrapper_fragment.width, grid_fragment.width,
            "the out-of-flow caption must not widen the table wrapper",
        );
        // The cell's 80px content width plus its initial 1px inline borders;
        // the 240px out-of-flow caption does not participate in this width.
        assert_eq!(grid_fragment.width, 82.0);
        assert_eq!(caption_fragment.width, 240.0);
        assert_eq!(
            caption_static.containing_block,
            ContainingBlock::Box(wrapper)
        );
        assert_eq!(
            caption_fragment.containing_fragment(),
            layout
                .fragments()
                .fragment_ids_for_box(wrapper)
                .first()
                .copied(),
        );
        assert_eq!(layout.table_shadow_ledger().block.laid_out, 1);
    }

    #[test]
    fn fixed_table_caption_uses_shared_k5d_initial_geometry() {
        let dom = StaticDocument::parse(
            "<table id=table><caption id=caption>caption</caption><tbody><tr><td>cell</td></tr></tbody></table>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                r#"table { display: table; position: relative; border-spacing: 0; }
                   caption { display: table-caption; position: fixed; left: 31px; top: 14px; width: 240px; height: 20px; }
                   tbody { display: table-row-group; } tr { display: table-row; }
                   td { display: table-cell; width: 80px; height: 20px; }"#,
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );

        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
        let table = node_by_id(&dom, dom.document(), "table").expect("table");
        let caption = node_by_id(&dom, dom.document(), "caption").expect("caption");
        let table_grid = layout.boxes().principal_box(table).expect("table grid");
        let wrapper = layout.boxes()[table_grid].parent().expect("table wrapper");
        let caption_box = layout.boxes().principal_box(caption).expect("caption box");
        let wrapper_fragment = layout
            .fragments()
            .fragments_for_box(wrapper)
            .next()
            .expect("wrapper fragment");
        let grid_fragment = layout
            .fragments()
            .fragments_for_box(table_grid)
            .next()
            .expect("grid fragment");
        let caption_fragment = layout
            .fragments()
            .fragments_for_box(caption_box)
            .next()
            .expect("caption fragment");
        let caption_static = layout
            .fragments()
            .static_position_for_box(caption_box)
            .expect("caption static-position record");

        assert_eq!(
            layout.boxes()[caption_box].positioning,
            PositioningScheme::Fixed,
        );
        assert_eq!((caption_fragment.x, caption_fragment.y), (31.0, 14.0));
        assert_eq!(
            wrapper_fragment.width, grid_fragment.width,
            "the out-of-flow caption must not widen the table wrapper",
        );
        // The cell's 80px content width plus its initial 1px inline borders;
        // the 240px out-of-flow caption does not participate in this width.
        assert_eq!(grid_fragment.width, 82.0);
        assert_eq!(caption_static.containing_block, ContainingBlock::Initial);
        assert_eq!(caption_fragment.containing_fragment(), None);
        assert_eq!(layout.table_shadow_ledger().block.laid_out, 1);
    }

    #[test]
    fn absolute_table_track_parts_use_zero_track_static_anchors() {
        let dom = StaticDocument::parse(
            "<table id=cell-table><tbody><tr><td>flow</td><td id=cell>cell</td></tr></tbody></table>\
             <table id=row-table><tbody><tr><td>flow</td></tr><tr id=row><td>row</td></tr></tbody></table>\
             <table id=group-table><tbody><tr><td>flow</td></tr></tbody><tbody id=group><tr><td>group</td></tr></tbody></table>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                r#"table { display: table; position: relative; border-spacing: 0; }
                   tbody { display: table-row-group; } tr { display: table-row; }
                   td { display: table-cell; width: 80px; height: 20px; }
                   #cell, #row, #group { position: absolute; left: 31px; top: 14px; width: 240px; height: 20px; }"#,
            ]),
            &Device::screen(640.0, 240.0),
            &InteractionStates::default(),
        );

        let layout = layout(&dom, &styles, 640.0, 240.0).expect("layout");
        for (table_id, part_id) in [
            ("cell-table", "cell"),
            ("row-table", "row"),
            ("group-table", "group"),
        ] {
            let table = node_by_id(&dom, dom.document(), table_id).expect("table");
            let part = node_by_id(&dom, dom.document(), part_id).expect("detached table part");
            let grid = layout.boxes().principal_box(table).expect("table grid");
            let wrapper = layout.boxes()[grid].parent().expect("table wrapper");
            let part_box = layout.boxes().principal_box(part).expect("part box");
            let wrapper_fragment = layout
                .fragments()
                .fragments_for_box(wrapper)
                .next()
                .expect("wrapper fragment");
            let grid_fragment = layout
                .fragments()
                .fragments_for_box(grid)
                .next()
                .expect("grid fragment");
            let part_fragment = layout
                .fragments()
                .fragments_for_box(part_box)
                .next()
                .expect("detached part fragment");
            let static_position = layout
                .fragments()
                .static_position_for_box(part_box)
                .expect("zero-track static position");

            assert_eq!(
                (part_fragment.x, part_fragment.y),
                (wrapper_fragment.x + 31.0, wrapper_fragment.y + 14.0),
                "{part_id} resolves through the wrapper containing block",
            );
            assert_eq!(
                grid_fragment.width, 82.0,
                "{part_id} does not widen the grid"
            );
            assert_eq!(static_position.logical_rect, LogicalRect::default());
        }
        assert!(
            layout.table_shadow_ledger().positioning_gaps.is_empty(),
            "post-track parts must not retain a K5 positioning gap: {:?}",
            layout.table_shadow_ledger(),
        );
    }

    #[test]
    fn fixed_table_track_parts_use_zero_track_static_anchors() {
        let dom = StaticDocument::parse(
            "<table id=cell-table><tbody><tr><td>flow</td><td id=cell>cell</td></tr></tbody></table>\
             <table id=row-table><tbody><tr><td>flow</td></tr><tr id=row><td>row</td></tr></tbody></table>\
             <table id=group-table><tbody><tr><td>flow</td></tr></tbody><tbody id=group><tr><td>group</td></tr></tbody></table>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                r#"table { display: table; position: relative; border-spacing: 0; }
                   tbody { display: table-row-group; } tr { display: table-row; }
                   td { display: table-cell; width: 80px; height: 20px; }
                   #cell, #row, #group { position: fixed; left: 31px; top: 14px; width: 240px; height: 20px; }"#,
            ]),
            &Device::screen(640.0, 240.0),
            &InteractionStates::default(),
        );

        let layout = layout(&dom, &styles, 640.0, 240.0).expect("layout");
        for (table_id, part_id) in [
            ("cell-table", "cell"),
            ("row-table", "row"),
            ("group-table", "group"),
        ] {
            let table = node_by_id(&dom, dom.document(), table_id).expect("table");
            let part = node_by_id(&dom, dom.document(), part_id).expect("detached table part");
            let grid = layout.boxes().principal_box(table).expect("table grid");
            let part_box = layout.boxes().principal_box(part).expect("part box");
            let grid_fragment = layout
                .fragments()
                .fragments_for_box(grid)
                .next()
                .expect("grid fragment");
            let part_fragment = layout
                .fragments()
                .fragments_for_box(part_box)
                .next()
                .expect("detached part fragment");
            let static_position = layout
                .fragments()
                .static_position_for_box(part_box)
                .expect("zero-track static position");

            assert_eq!(
                (part_fragment.x, part_fragment.y),
                (31.0, 14.0),
                "{part_id} resolves against the initial containing block",
            );
            assert_eq!(
                grid_fragment.width, 82.0,
                "{part_id} does not widen the grid"
            );
            assert_eq!(static_position.logical_rect, LogicalRect::default());
            assert_eq!(static_position.containing_block, ContainingBlock::Initial);
            assert_eq!(part_fragment.containing_fragment(), None);
        }
        assert!(
            layout.table_shadow_ledger().positioning_gaps.is_empty(),
            "post-track parts must not retain a K5 positioning gap: {:?}",
            layout.table_shadow_ledger(),
        );
    }

    #[test]
    fn k4g4_consumes_projected_metrics_on_both_table_axes() {
        let dom = StaticDocument::parse(
            "<table id=table><tbody><tr><td>one</td><td>two</td></tr></tbody></table>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["table { display: table; border-collapse: collapse; } \
                 tbody { display: table-row-group; } tr { display: table-row; } \
                 td { display: table-cell; border: 5px solid; padding: 0; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );

        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
        let ledger = layout.table_shadow_ledger();
        assert_eq!(ledger.collapsed_metrics, 1, "{ledger:?}");
        assert!(
            ledger.skipped.is_empty(),
            "K4g4 must consume B2's projected metrics without a fallback: {ledger:?}"
        );
        assert_eq!(ledger.assigned, 1, "{ledger:?}");
        assert_eq!(ledger.honored, 1, "{ledger:?}");
        assert_eq!(ledger.block.laid_out, 1, "{ledger:?}");
        assert_eq!(ledger.block.agreed, 1, "{ledger:?}");
        let table = node_by_id(&dom, dom.document(), "table").expect("table node");
        let fragment = layout.principal_fragment(table).expect("table fragment");
        assert!(
            (fragment.logical_rect.inline_start - fragment.overflow.inline_start - 2.5).abs()
                < 0.01,
            "the first outer winner spills beyond the table border box: {fragment:?}"
        );
        assert!(
            (fragment.logical_rect.block_start - fragment.overflow.block_start - 2.5).abs() < 0.01,
            "the block-start winner also propagates into table overflow: {fragment:?}"
        );
    }

    #[test]
    fn ph3_rules_attribute_reaches_k4g_collapsed_border_resolution() {
        let dom = StaticDocument::parse(
            r#"<table id="table" rules="all" bordercolor="red"><tbody><tr><td id="cell">one</td><td>two</td></tr></tbody></table>"#,
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let table = node_by_id(&dom, dom.document(), "table").expect("table");
        let cell = node_by_id(&dom, dom.document(), "cell").expect("cell");

        assert_eq!(
            styles.get(table).unwrap().border_collapse,
            BorderCollapse::Collapse,
            "the HTML attribute must first become an ordinary computed declaration"
        );
        assert_eq!(
            styles.get(cell).unwrap().border_top_style,
            BorderStyle::Solid
        );
        assert_eq!(
            styles.get(cell).unwrap().border_top_color.to_srgb8(),
            Some((0, 0, 0, 255)),
            "the attribute-sensitive UA rule supplies the cell candidate color"
        );

        let layout = layout(&dom, &styles, 320.0, 240.0).expect("layout");
        let ledger = layout.table_shadow_ledger();
        assert_eq!(ledger.collapsed_metrics, 1, "{ledger:?}");
        assert_eq!(ledger.assigned, 1, "{ledger:?}");
        assert_eq!(ledger.honored, 1, "{ledger:?}");
        assert!(ledger.skipped.is_empty(), "{ledger:?}");
    }

    fn fixed_table_ledger(spacing: &str) -> crate::table_shadow::TableShadowLedger {
        let dom = StaticDocument::parse(
            "<table><tbody><tr><td id=first>one</td><td>two</td><td>three</td></tr></tbody></table>",
        );
        let css = format!(
            "table {{ display: table; table-layout: fixed; width: 300px; border-spacing: {spacing}; }} tbody {{ display: table-row-group; }} tr {{ display: table-row; }} td {{ display: table-cell; }} #first {{ width: 120px; }}"
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[&css]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        layout(&dom, &styles, 320.0, 240.0)
            .expect("layout")
            .table_shadow_ledger()
            .clone()
    }

    fn assert_assigned_and_honored(ledger: &crate::table_shadow::TableShadowLedger) {
        assert_eq!(
            ledger.assigned, 1,
            "Buckram did not size the table: {ledger:?}"
        );
        assert_eq!(
            ledger.verified, 1,
            "the assignment was not verified: {ledger:?}"
        );
        assert!(
            ledger.is_silent(),
            "the bridge did not honor Buckram's tracks: {ledger:?}"
        );
        assert_eq!(ledger.honored, 1, "{ledger:?}");
    }

    /// K4c5b: Buckram owns the fixed algorithm and the painted fragments
    /// honor its columns exactly.
    #[test]
    fn k4c5b_fixed_table_columns_are_buckram_owned() {
        assert_assigned_and_honored(&fixed_table_ledger("0"));
    }

    /// The first K4c5a divergence, resolved by authority. The deleted live
    /// helper omitted `border-spacing` from CSS 2.1 17.5.2.1's distribution
    /// and painted 89px columns; Buckram's 85px columns now paint, and the
    /// fragment verification proves it.
    #[test]
    fn k4c5b_fixed_border_spacing_distribution_is_painted() {
        assert_assigned_and_honored(&fixed_table_ledger("2px"));
    }

    /// K4c5b on the production text path: a fixed table routed through
    /// `InlineBuildState` receives Buckram columns before the main pass. This
    /// route previously had no fixed sizing at all.
    #[test]
    fn k4c5b_text_path_fixed_tables_are_buckram_owned() {
        let dom = StaticDocument::parse(
            "<p>before the table</p><table><tbody><tr><td id=first>one</td><td>two</td><td>three</td></tr></tbody></table>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "table { display: table; table-layout: fixed; width: 300px; border-spacing: 0; } tbody { display: table-row-group; } tr { display: table-row; } td { display: table-cell; } #first { width: 120px; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        assert_assigned_and_honored(layout.table_shadow_ledger());
    }

    /// A fixed table inside an inline-block builds under an atomic subtree's
    /// own `BuildState`; its assignment and verification survive into
    /// `LiveryLayout` through the accumulated plane ledger.
    ///
    /// Span-based markup because a `<div>` start tag inside `<p>` closes the
    /// paragraph at the HTML parser, before box generation runs.
    #[test]
    fn k4c5b_tables_inside_atomic_inline_subtrees_are_buckram_owned() {
        let dom = StaticDocument::parse(
            "<p>before <span id=atom><span class=t><span class=tb><span class=row><span class=cell id=first>one</span><span class=cell>two</span><span class=cell>three</span></span></span></span></span> after</p>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "#atom { display: inline-block; } .t { display: table; table-layout: fixed; width: 300px; border-spacing: 0; } .tb { display: table-row-group; } .row { display: table-row; } .cell { display: table-cell; } #first { width: 120px; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let ledger = layout.table_shadow_ledger();
        assert!(
            ledger.assigned >= 1,
            "the atomic subtree's table was not sized by Buckram: {ledger:?}"
        );
        assert!(ledger.is_silent(), "{ledger:?}");
    }

    /// K4d6b: a `height` on a `<tr>` reaches the painted rows.
    ///
    /// It is a row minimum under CSS 2.1 section 17.5.3. Buckram computes 40
    /// and 60 and writes the retained row fragments at those sizes.
    ///
    /// The painted rectangles are asserted directly rather than through the
    /// ledger. A ledger that agreed with itself would prove nothing.
    #[test]
    fn k4d6b_row_heights_reach_the_painted_rows() {
        let dom = StaticDocument::parse(
            "<table><tbody><tr id=a><td>one</td><td>two</td></tr><tr id=b><td>three</td><td>four</td></tr></tbody></table>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "table { display: table; table-layout: fixed; width: 200px; border-spacing: 0; }                  tbody { display: table-row-group; } tr { display: table-row; }                  td { display: table-cell; padding: 0; } #a { height: 40px; } #b { height: 60px; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let ledger = layout.table_shadow_ledger();
        assert_eq!(
            ledger.block.laid_out, 1,
            "Buckram did not lay out the table's block axis: {:?}",
            ledger.block
        );
        assert_eq!(
            ledger.block.agreed, 1,
            "the painted cells must now be the ones Buckram wrote: {:?}",
            ledger.block.divergences
        );

        fn cell_rect(
            dom: &StaticDocument,
            layout: &LiveryLayout<<StaticDocument as LayoutDom>::NodeId>,
            index: usize,
        ) -> PhysicalRect {
            fn cells(
                dom: &StaticDocument,
                node: <StaticDocument as LayoutDom>::NodeId,
                found: &mut Vec<<StaticDocument as LayoutDom>::NodeId>,
            ) {
                if dom
                    .element_name(node)
                    .is_some_and(|name| name.local.as_ref() == "td")
                {
                    found.push(node);
                }
                for child in dom.dom_children(node) {
                    cells(dom, child, found);
                }
            }
            let mut found = Vec::new();
            cells(dom, dom.document(), &mut found);
            let box_id = layout.boxes().boxes_for_node(found[index])[0];
            layout
                .fragments()
                .fragments_for_box(box_id)
                .next()
                .expect("cell fragment")
                .physical_rect()
        }
        // The first cell of each row, in document order.
        let first = cell_rect(&dom, &layout, 0);
        let second = cell_rect(&dom, &layout, 2);
        assert!(
            (first.height - 40.0).abs() < 0.5,
            "the first row's cell must be its row's 40px, not its content              height: {first:?}"
        );
        assert!(
            (second.height - 60.0).abs() < 0.5,
            "the second row's cell must be its row's 60px: {second:?}"
        );
        assert!(
            (second.y - first.y - 40.0).abs() < 0.5,
            "the second row must start just below a 40px first row:              {first:?} {second:?}"
        );
    }

    /// B3: the accepted K4d3 row-group rule must reach live geometry rather
    /// than remain a pure constraint. A definite `tbody` height is a minimum
    /// shared proportionally by only that group's 20px and 40px rows.
    #[test]
    fn b3_row_group_height_reaches_the_buckram_table_route() {
        let dom = StaticDocument::parse(
            "<table><tbody><tr><td><i class=small></i></td></tr><tr><td><i class=large></i></td></tr></tbody></table>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "table { display: table; table-layout: fixed; width: 200px; border-spacing: 0; } \
                 tbody { display: table-row-group; height: 200px; } \
                 tr { display: table-row; } td { display: table-cell; padding: 0; } \
                 i { display: block; } .small { height: 20px; } .large { height: 40px; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let ledger = layout.table_shadow_ledger();
        assert_eq!(ledger.block.laid_out, 1, "block ledger: {:?}", ledger.block);
        assert_eq!(ledger.block.agreed, 1, "block ledger: {:?}", ledger.block);

        let rows = layout
            .boxes()
            .iter()
            .filter(|(_, css_box)| css_box.display.internal_table == Some(InternalTableRole::Row))
            .filter_map(|(box_id, _)| {
                layout
                    .fragments()
                    .fragments_for_box(box_id)
                    .next()
                    .map(|fragment| fragment.physical_rect())
            })
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 2, "rows: {rows:?}");
        assert!((rows[0].height - 66.67).abs() < 0.5, "rows: {rows:?}");
        assert!((rows[1].height - 133.33).abs() < 0.5, "rows: {rows:?}");
        assert!(
            (rows[0].height + rows[1].height - 200.0).abs() < 0.5,
            "rows: {rows:?}"
        );
    }

    /// B3: a table's own definite height distributes across its rows once;
    /// an auto row group must not receive that height as a second constraint.
    /// This is the table geometry used by CSS2 containing-block-029's
    /// reference, kept here as a direct layout receipt.
    #[test]
    fn b3_auto_row_group_does_not_repeat_the_table_height() {
        let html = "<table><col id=first><col id=second><tbody><tr><td></td><td></td></tr>\
                    <tr id=last><td></td><td id=orange>.</td></tr></tbody></table>";
        let css = "table { border-spacing: 0; height: 96px; table-layout: fixed; width: 96px; }\
                   col#first { width: 72px; } col#second { width: 24px; }\
                   td { background-color: blue; padding: 0; }\
                   td#orange { background-color: orange; vertical-align: top; }\
                   tr { height: 72px; } tr#last { height: 24px; }";
        let grid = table_role_rects(html, css, InternalTableRole::Grid)[0];
        let rows = table_role_rects(html, css, InternalTableRole::Row);
        let cells = table_role_rects(html, css, InternalTableRole::Cell);
        assert_eq!(rows.len(), 2, "rows: {rows:?}");
        assert_eq!(cells.len(), 4, "cells: {cells:?}");
        assert!((grid.height - 96.0).abs() < 0.5, "grid: {grid:?}");
        assert!((rows[0].height - 72.0).abs() < 0.5, "rows: {rows:?}");
        assert!((rows[1].height - 24.0).abs() < 0.5, "rows: {rows:?}");
        assert!((cells[3].height - 24.0).abs() < 0.5, "cells: {cells:?}");
        assert!(
            (rows[0].height + rows[1].height - grid.height).abs() < 0.5,
            "grid: {grid:?}; rows: {rows:?}"
        );
    }

    /// Lay out one document and return every table-role box's rectangle.
    fn table_boxes(html: &str, css: &str) -> Vec<(InternalTableRole, PhysicalRect)> {
        let dom = StaticDocument::parse(html);
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; border: 0; }", css]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        layout
            .boxes()
            .iter()
            .filter_map(|(box_id, css_box)| {
                Some((
                    css_box.display.internal_table?,
                    layout
                        .fragments()
                        .fragments_for_box(box_id)
                        .next()?
                        .physical_rect(),
                ))
            })
            .collect()
    }

    /// Every rectangle laid out for one table role, in tree order.
    fn table_role_rects(html: &str, css: &str, role: InternalTableRole) -> Vec<PhysicalRect> {
        table_boxes(html, css)
            .into_iter()
            .filter(|(each, _)| *each == role)
            .map(|(_, rect)| rect)
            .collect()
    }

    /// The table wrapper box and the table grid box, in that order.
    fn table_wrapper_and_grid(html: &str, css: &str) -> (PhysicalRect, PhysicalRect) {
        let one = |role| {
            *table_role_rects(html, css, role)
                .first()
                .unwrap_or_else(|| panic!("no fragment for {role:?}"))
        };
        (
            one(InternalTableRole::Wrapper),
            one(InternalTableRole::Grid),
        )
    }

    #[test]
    fn ph2_table_width_hint_reaches_buckram_through_computed_css() {
        let (wrapper, grid) = table_wrapper_and_grid(
            "<div id=host><table width=50%><tr><td></td></tr></table></div>",
            "#host { width: 200px; }\
             table { display: table; table-layout: fixed; border-spacing: 0; }\
             tr { display: table-row; } td { display: table-cell; padding: 0; }",
        );

        assert!((wrapper.width - 100.0).abs() < 0.5, "wrapper: {wrapper:?}");
        assert!((grid.width - 100.0).abs() < 0.5, "grid: {grid:?}");
    }

    /// K4e1: the wrapper and the grid are two boxes that split one element.
    ///
    /// CSS 2.1 section 17.4 uses `margin-*` on the wrapper and leaves `width`,
    /// `border`, and `padding` on the grid. Both are observable at once: the
    /// margin has to move the wrapper, and the grid's own border box has to
    /// still contain the border and padding the wrapper does not have. CSS
    /// Tables 3 section 2.2.1 then makes the two the same width, because the
    /// wrapper's width *is* the grid's border-edge width.
    #[test]
    fn k4e1_the_wrapper_takes_the_margin_and_the_grid_keeps_its_own_box() {
        let (wrapper, grid) = table_wrapper_and_grid(
            "<div id=host><table><tr><td></td></tr></table></div>",
            "#host { width: 300px; }\
             table { display: table; box-sizing: content-box; width: 100px;\
                     margin-left: 20px; border: 5px solid; padding: 3px;\
                     border-spacing: 0; }\
             tr { display: table-row; } td { display: table-cell; padding: 0; }",
        );

        // The margin is the wrapper's, so both boxes start past it.
        assert!((wrapper.x - 20.0).abs() < 0.5, "wrapper: {wrapper:?}");
        assert!((grid.x - 20.0).abs() < 0.5, "grid: {grid:?}");
        // The grid's border edge is 100 of content plus its own padding and
        // border, and the wrapper is exactly that wide - no wider, which is
        // what would happen if it had kept the border and padding too.
        assert!((grid.width - 116.0).abs() < 0.5, "grid: {grid:?}");
        assert!(
            (wrapper.width - grid.width).abs() < 0.5,
            "the wrapper is the grid's border-edge width: {wrapper:?} {grid:?}"
        );
    }

    /// K4e1: `position` is the wrapper's, and a percentage size skips it.
    ///
    /// CSS 2.1 section 17.4 again: "Percentages on 'width' and 'height' on the
    /// table are relative to the table wrapper box's containing block, not the
    /// table wrapper box itself." Without that rule the grid's `50%` resolves
    /// against a wrapper that is itself waiting on the grid, and the pair
    /// collapses to zero - which is what `absolute-tables-012` measures.
    #[test]
    fn k4e1_a_percentage_table_skips_the_wrapper_it_would_otherwise_wait_on() {
        let (wrapper, grid) = table_wrapper_and_grid(
            "<div id=host><table></table></div>",
            "#host { position: relative; width: 200px; }\
             table { display: table; position: absolute; width: 50%; height: 100px;\
                     table-layout: fixed; border-spacing: 0; }",
        );

        for (name, rect) in [("wrapper", wrapper), ("grid", grid)] {
            assert!((rect.width - 100.0).abs() < 0.5, "{name}: {rect:?}");
            assert!((rect.height - 100.0).abs() < 0.5, "{name}: {rect:?}");
        }
    }

    /// K4e1: a table flex item is the wrapper, and it does not stretch.
    ///
    /// Inserting the wrapper makes it, not the grid, the flex item. CSS
    /// Tables 3 section 2.2.1 keeps its width the grid's, so a column flex
    /// container's default `align-items: stretch` has nothing to stretch -
    /// the width is not `auto`. `table-as-item-cell-percentage-002` fails the
    /// moment the wrapper widens to the container instead.
    #[test]
    fn k4e1_a_table_flex_item_is_the_wrapper_and_keeps_the_grids_width() {
        let (wrapper, grid) = table_wrapper_and_grid(
            "<div id=host><table><tr><td></td></tr></table></div>",
            "#host { display: flex; flex-direction: column; width: 300px; }\
             table { display: table; width: 100px; height: 100px; border-spacing: 0; }\
             tr { display: table-row; } td { display: table-cell; padding: 0; }",
        );

        assert!((wrapper.width - 100.0).abs() < 0.5, "wrapper: {wrapper:?}");
        assert!((grid.width - 100.0).abs() < 0.5, "grid: {grid:?}");
    }

    /// K4e2: an `auto`-width wrapper measures the grid rather than filling.
    ///
    /// This is the half of CSS Tables 3 section 2.2.1 that cannot be computed
    /// before layout. An ordinary block with `width: auto` would take all 300
    /// of the container; the wrapper takes the 80 its two columns come to,
    /// through Buckram's intrinsic shrink-to-fit lane rather than through the
    /// `float: left` that used to stand in for it.
    #[test]
    fn k4e2_an_auto_width_wrapper_measures_the_grid_instead_of_filling() {
        let (wrapper, grid) = table_wrapper_and_grid(
            "<div id=host><table><tr><td></td><td></td></tr></table></div>",
            "#host { width: 300px; }\
             table { display: table; border-spacing: 0; }\
             tr { display: table-row; }\
             td { display: table-cell; padding: 0; width: 40px; height: 10px; }",
        );

        assert!((grid.width - 80.0).abs() < 0.5, "grid: {grid:?}");
        assert!((wrapper.width - 80.0).abs() < 0.5, "wrapper: {wrapper:?}");
    }

    /// K4e2: auto margins centre a table, which a float cannot do.
    ///
    /// The margins are the wrapper's under CSS 2.1 section 17.4, and a float
    /// resolves an `auto` margin to zero. Once the wrapper is an in-flow
    /// shrink-to-fit block on K3's equations, `margin: 0 auto` on a table
    /// centres it the way it does on any other block.
    #[test]
    fn k4e2_auto_margins_centre_a_table() {
        let (wrapper, grid) = table_wrapper_and_grid(
            "<div id=host><table><tr><td></td></tr></table></div>",
            "#host { width: 300px; }\
             table { display: table; border-spacing: 0; width: 100px;\
                     margin-left: auto; margin-right: auto; }\
             tr { display: table-row; }\
             td { display: table-cell; padding: 0; width: 100px; height: 10px; }",
        );

        assert!((wrapper.width - 100.0).abs() < 0.5, "wrapper: {wrapper:?}");
        assert!((wrapper.x - 100.0).abs() < 0.5, "wrapper: {wrapper:?}");
        assert!((grid.x - 100.0).abs() < 0.5, "grid: {grid:?}");
    }

    /// K4e3: a captioned table stops deferring.
    ///
    /// The point of the gate. `CaptionMinContribution::PendingK4e` was a named
    /// gap rather than a defect, and the 2026-08-03 census counted it firing
    /// 17 times; a measured caption closes it, and Buckram sizes the table
    /// instead of declining it.
    #[test]
    fn k4e3_a_captioned_table_no_longer_defers() {
        let dom = StaticDocument::parse(
            "<div id=host><table><caption>a caption</caption>\
             <tr><td>one</td><td>two</td></tr></table></div>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["#host { width: 400px; } \
                 table { display: table; border-spacing: 0; } \
                 caption { display: table-caption; margin: 0; padding: 0; } \
                 tr { display: table-row; } td { display: table-cell; padding: 0; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");

        let ledger = layout.table_shadow_ledger();
        assert_assigned_and_honored(ledger);
    }

    /// K4f: `visibility: collapse` removes a row's space, not just its ink.
    ///
    /// CSS 2.1 section 17.5.5 reduces the table's height by exactly what the
    /// collapsed row occupied and leaves the other rows the heights they were
    /// given. Three 20px rows come to 60; collapsing the middle one leaves 40,
    /// with the third row moved up into the gap rather than a hole left where
    /// the second used to be.
    #[test]
    fn k4f_a_collapsed_row_gives_its_space_back_to_the_table() {
        let html = "<div id=host><table><tr id=a><td></td></tr>\
                    <tr id=b><td></td></tr><tr id=c><td></td></tr></table></div>";
        let visible = "#host { width: 300px; }\
                       table { display: table; border-spacing: 0; }\
                       tr { display: table-row; }\
                       td { display: table-cell; padding: 0; width: 40px; height: 20px; }";
        let collapsed = format!("{visible} #b {{ visibility: collapse; }}");

        let (_, before) = table_wrapper_and_grid(html, visible);
        let (_, after) = table_wrapper_and_grid(html, &collapsed);
        let rows = table_role_rects(html, &collapsed, InternalTableRole::Row);

        assert!((before.height - 60.0).abs() < 0.5, "before: {before:?}");
        assert!((after.height - 40.0).abs() < 0.5, "after: {after:?}");
        assert!(
            (rows[1].height - 0.0).abs() < 0.5,
            "collapsed: {:?}",
            rows[1]
        );
        assert!(
            (rows[2].y - rows[0].y - 20.0).abs() < 0.5,
            "the third row closes the gap: {:?} {:?}",
            rows[0],
            rows[2]
        );
    }

    /// K4f: a collapsed column gives its width back the same way.
    ///
    /// Applied through the column group here, which section 17.5.5 also
    /// allows: a collapsed `<colgroup>` collapses every column in its range.
    #[test]
    fn k4f_a_collapsed_column_group_gives_its_width_back() {
        let html = "<div id=host><table><colgroup id=g><col></colgroup>\
                    <colgroup><col></colgroup>\
                    <tr><td></td><td></td></tr></table></div>";
        let visible = "#host { width: 300px; }\
                       table { display: table; border-spacing: 0; }\
                       colgroup { display: table-column-group; }\
                       col { display: table-column; }\
                       tr { display: table-row; }\
                       td { display: table-cell; padding: 0; width: 40px; height: 20px; }";
        let collapsed = format!("{visible} #g {{ visibility: collapse; }}");

        let (_, before) = table_wrapper_and_grid(html, visible);
        let (wrapper, after) = table_wrapper_and_grid(html, &collapsed);

        assert!((before.width - 80.0).abs() < 0.5, "before: {before:?}");
        assert!((after.width - 40.0).abs() < 0.5, "after: {after:?}");
        // K4e2's rule still holds through the collapse.
        assert!((wrapper.width - after.width).abs() < 0.5, "{wrapper:?}");
    }

    /// K4e4: used `width` and `height` answer from the grid, not the wrapper.
    ///
    /// The `height` property stayed on the grid under CSS 2.1 section 17.4,
    /// so `getComputedStyle(table).height` reports the grid's border box - the
    /// 40px of rows, not the 70px wrapper that also contains the caption.
    #[test]
    fn k4e4_used_height_of_a_captioned_table_is_the_grids() {
        let dom =
            StaticDocument::parse("<table><caption>above</caption><tr><td>one</td></tr></table>");
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "table { display: table; table-layout: fixed; width: 200px; border-spacing: 0; } \
                 caption { display: table-caption; height: 30px; margin: 0; padding: 0; } \
                 tr { display: table-row; height: 40px; } \
                 td { display: table-cell; padding: 0; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let table = {
            fn find(
                dom: &StaticDocument,
                node: <StaticDocument as LayoutDom>::NodeId,
            ) -> Option<<StaticDocument as LayoutDom>::NodeId> {
                if dom
                    .element_name(node)
                    .is_some_and(|name| name.local.as_ref() == "table")
                {
                    return Some(node);
                }
                dom.dom_children(node)
                    .into_iter()
                    .find_map(|child| find(dom, child))
            }
            find(&dom, dom.document()).expect("the table exists")
        };
        let used = used_value_context(&dom, &styles, 320.0, 240.0, table)
            .expect("layout")
            .expect("the table has a fragment");

        assert!((used.border_box.0 - 200.0).abs() < 0.5, "{used:?}");
        assert!(
            (used.border_box.1 - 40.0).abs() < 0.5,
            "the used height is the grid's, without the caption: {used:?}"
        );
    }

    /// K4e4: an inline-table occupies line space as an atom.
    ///
    /// Deleting K4a's wrapper/grid exclusion from `box_is_inline` lets the
    /// wrapper join the inline group, and the atomic-inline lane lays its
    /// subtree out separately - the same route an inline-block rides. The
    /// receipt is placement: the table sits to the right of the text that
    /// precedes it in the same line, at the grid's own width, instead of
    /// dropping to a line of its own as the block it used to be built as.
    #[test]
    fn k4e4_an_inline_table_sits_in_the_text_line() {
        let (wrapper, grid) = table_wrapper_and_grid(
            "<div id=host>before<span class=t><span class=r>\
             <span class=c>cell</span></span></span></div>",
            "#host { width: 300px; font-family: monospace; font-size: 10px;\
                     line-height: 20px; }\
             .t { display: inline-table; border-spacing: 0; }\
             .r { display: table-row; }\
             .c { display: table-cell; padding: 0; width: 50px; height: 12px; }",
        );

        assert!((grid.width - 50.0).abs() < 0.5, "grid: {grid:?}");
        assert!(
            (wrapper.width - grid.width).abs() < 0.5,
            "{wrapper:?} {grid:?}"
        );
        // In the line, after the word, not below it.
        assert!(
            wrapper.x > 30.0,
            "the atom must sit after 'before': {wrapper:?}"
        );
        assert!(
            wrapper.y < 20.0,
            "the atom must sit in the first line: {wrapper:?}"
        );
    }

    /// B3: K4d5's first table baseline positions a baseline-aligned
    /// inline-table. The second row makes the table much taller than its first
    /// row, so the old wrapper block-end fallback would put the first cell's
    /// text far above its inline peer.
    #[test]
    fn b3_inline_table_uses_its_first_table_baseline() {
        fn by_id(
            dom: &StaticDocument,
            node: <StaticDocument as LayoutDom>::NodeId,
            expected: &str,
        ) -> Option<<StaticDocument as LayoutDom>::NodeId> {
            if dom.attributes(node).any(|attribute| {
                attribute.name.ns.as_ref().is_empty()
                    && attribute.name.local.as_ref() == "id"
                    && attribute.value == expected
            }) {
                return Some(node);
            }
            dom.dom_children(node)
                .find_map(|child| by_id(dom, child, expected))
        }

        let dom = StaticDocument::parse(
            "<div id=host><span id=peer>peer</span><span id=table class=t><span class=r>\
             <span id=first class=c>table</span></span>\
             <span class=r><span class=c id=second>lower</span></span></span></div>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "html, body, div, span { margin: 0; padding: 0; border: 0; }\
                 #host { width: 320px; font-family: monospace; font-size: 10px; line-height: 20px; }\
                 .t { display: inline-table; border-spacing: 0; vertical-align: baseline; }\
                 .r { display: table-row; } .c { display: table-cell; padding: 0; }\
                 #first { height: 40px; } #second { height: 60px; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let peer_node = by_id(&dom, dom.document(), "peer").expect("peer");
        let peer = layout
            .text_frame()
            .and_then(|frame| frame.first_inline_baseline(peer_node))
            .expect("peer shaped-line baseline");
        let first_cell = layout
            .get(by_id(&dom, dom.document(), "first").expect("first cell"))
            .expect("first cell fragment");
        // This table's default cell baseline is its first row's cell block
        // end. Its row is 40px while the full table is 100px, so the receipt
        // rejects the old 100px wrapper block-end fallback.
        let cell = first_cell.physical_rect().y + first_cell.physical_rect().height;
        let rect = |id| {
            layout
                .get(by_id(&dom, dom.document(), id).expect(id))
                .expect(id)
                .physical_rect()
        };

        assert!(
            (peer - cell).abs() < 0.5,
            "the inline peer and first table-row baseline must agree: peer={peer}, cell={cell}, peer_rect={:?}, cell_rect={:?}, table_rect={:?}",
            rect("peer"),
            rect("first"),
            rect("table"),
        );
    }

    /// K4e3: a caption wider than the table widens the *grid*.
    ///
    /// The two engines break CSS Tables 3 section 2.2.1 apart here, measured
    /// in the K4e1 interop matrix: Chrome grows the grid and its columns to
    /// the caption, Firefox leaves the grid at its own content width and lets
    /// only the wrapper be caption-wide. Section 2.2.1 says the wrapper's
    /// width *is* the grid's border-edge width, which Firefox's answer
    /// contradicts and Chrome's keeps, so this keeps the rule.
    ///
    /// C7 of that matrix is the sharpest case to assert, because a specified
    /// caption width fixes the expected number without depending on font
    /// metrics: the caption's 300 reaches the single column.
    #[test]
    fn k4e3_a_caption_widens_the_grid_and_its_columns() {
        let html = "<div id=host><table><caption>x</caption>\
                    <tr><td></td></tr></table></div>";
        let css = "#host { width: 400px; }\
                   table { display: table; border-spacing: 0; }\
                   caption { display: table-caption; width: 300px; margin: 0; padding: 0; }\
                   tr { display: table-row; }\
                   td { display: table-cell; padding: 0; height: 10px; }";
        let (wrapper, grid) = table_wrapper_and_grid(html, css);
        let cells = table_role_rects(html, css, InternalTableRole::Cell);

        assert!((grid.width - 300.0).abs() < 0.5, "grid: {grid:?}");
        assert!((cells[0].width - 300.0).abs() < 0.5, "cell: {:?}", cells[0]);
        // Section 2.2.1 still holds: the wrapper is the grid's width.
        assert!(
            (wrapper.width - grid.width).abs() < 0.5,
            "{wrapper:?} {grid:?}"
        );
    }

    /// K4e3: a caption's own margins are part of the floor it puts down.
    ///
    /// C5 of the interop matrix, where both engines agree: a 176-wide caption
    /// with `margin-left: 30px` contributes 206. Asserted with a specified
    /// width so the number does not depend on font metrics.
    #[test]
    fn k4e3_a_captions_margins_count_toward_what_it_contributes() {
        let html = "<div id=host><table><caption>x</caption>\
                    <tr><td></td></tr></table></div>";
        let css = "#host { width: 400px; }\
                   table { display: table; border-spacing: 0; }\
                   caption { display: table-caption; width: 200px; margin-left: 30px;\
                             padding: 0; }\
                   tr { display: table-row; }\
                   td { display: table-cell; padding: 0; height: 10px; }";
        let (_, grid) = table_wrapper_and_grid(html, css);

        assert!((grid.width - 230.0).abs() < 0.5, "grid: {grid:?}");
    }

    /// K4e3: `caption-side` decides which side of the grid a caption lands on.
    ///
    /// CSS 2.1 section 17.4.1 lays a caption above or below the grid inside
    /// the wrapper's margins. Buckram's box tree keeps every caption before
    /// the grid, so a bottom caption has to be reordered on the way into
    /// layout - and C4 of the matrix pins that the side does not change what
    /// the caption contributes to sizing.
    #[test]
    fn k4e3_caption_side_moves_the_caption_without_changing_the_table() {
        let html = "<div id=host><table><caption>x</caption>\
                    <tr><td></td></tr></table></div>";
        let above = "#host { width: 400px; }\
                     table { display: table; border-spacing: 0; }\
                     caption { display: table-caption; width: 300px; height: 20px;\
                               margin: 0; padding: 0; }\
                     tr { display: table-row; }\
                     td { display: table-cell; padding: 0; height: 10px; }";
        let below = format!("{above} caption {{ caption-side: bottom; }}");

        let (top_wrapper, top_grid) = table_wrapper_and_grid(html, above);
        let top_caption = table_role_rects(html, above, InternalTableRole::Caption)[0];
        let (bottom_wrapper, bottom_grid) = table_wrapper_and_grid(html, &below);
        let bottom_caption = table_role_rects(html, &below, InternalTableRole::Caption)[0];

        assert!(
            top_caption.y < top_grid.y,
            "a top caption sits above the grid: {top_caption:?} {top_grid:?}"
        );
        assert!(
            bottom_caption.y > bottom_grid.y,
            "a bottom caption sits below the grid: {bottom_caption:?} {bottom_grid:?}"
        );
        // The side is placement only; the sizing it forces is the same.
        assert!((top_grid.width - bottom_grid.width).abs() < 0.5);
        assert!((top_wrapper.height - bottom_wrapper.height).abs() < 0.5);
    }

    /// B3: `caption-side` is the table wrapper's logical block-axis order.
    /// In vertical-rl, block-start is physical right and block-end is left;
    /// assigning caption placement to fragmentation would lose that wrapper
    /// relationship before any fragmentainer exists.
    #[test]
    fn b3_vertical_caption_side_uses_the_wrappers_logical_block_axis() {
        let html = "<div id=host><table><caption>x</caption>\
                    <tr><td></td></tr></table></div>";
        for (writing_mode, top_at_right) in [("vertical-rl", true), ("vertical-lr", false)] {
            let top = format!(
                "#host {{ width: 300px; height: 200px; }}\
                 table {{ display: table; writing-mode: {writing_mode}; height: 100px; border-spacing: 0; }}\
                 caption {{ display: table-caption; width: 40px; height: 20px; margin: 0; padding: 0; }}\
                 tr {{ display: table-row; }} td {{ display: table-cell; padding: 0; width: 60px; height: 30px; }}"
            );
            let bottom = format!("{top} caption {{ caption-side: bottom; }}");
            let top_caption = table_role_rects(html, &top, InternalTableRole::Caption)[0];
            let top_grid = table_role_rects(html, &top, InternalTableRole::Grid)[0];
            let bottom_caption = table_role_rects(html, &bottom, InternalTableRole::Caption)[0];
            let bottom_grid = table_role_rects(html, &bottom, InternalTableRole::Grid)[0];

            assert_eq!(
                top_caption.x > top_grid.x,
                top_at_right,
                "{writing_mode} top caption must occupy its block-start: {top_caption:?} {top_grid:?}"
            );
            assert_eq!(
                bottom_caption.x < bottom_grid.x,
                top_at_right,
                "{writing_mode} bottom caption must occupy its block-end: {bottom_caption:?} {bottom_grid:?}"
            );
        }
    }

    /// K4d6: a table row now has a fragment of its own.
    ///
    /// Buckram emits the whole structural subtree from the track model, so
    /// each part gets its exact rectangle whether or not a cell covers it.
    #[test]
    fn k4d6_rows_groups_and_columns_have_their_own_fragments() {
        let dom = StaticDocument::parse(
            "<table><colgroup><col></colgroup><tbody><tr id=a><td>one</td></tr><tr id=b><td>two</td></tr></tbody></table>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "table { display: table; table-layout: fixed; width: 200px; border-spacing: 0; }                  tbody { display: table-row-group; } tr { display: table-row; }                  td { display: table-cell; padding: 0; } #a { height: 40px; } #b { height: 60px; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");

        let rect_of = |local: &str, nth: usize| {
            fn walk(
                dom: &StaticDocument,
                node: <StaticDocument as LayoutDom>::NodeId,
                local: &str,
                out: &mut Vec<<StaticDocument as LayoutDom>::NodeId>,
            ) {
                if dom
                    .element_name(node)
                    .is_some_and(|n| n.local.as_ref() == local)
                {
                    out.push(node);
                }
                for child in dom.dom_children(node) {
                    walk(dom, child, local, out);
                }
            }
            let mut found = Vec::new();
            walk(&dom, dom.document(), local, &mut found);
            // A table element generates a wrapper and a grid; only one of
            // them carries the fragment.
            layout
                .boxes()
                .boxes_for_node(found[nth])
                .iter()
                .find_map(|box_id| layout.fragments().fragments_for_box(*box_id).next())
                .unwrap_or_else(|| panic!("no fragment for {local}[{nth}]"))
                .physical_rect()
        };

        let table = rect_of("table", 0);
        let first = rect_of("tr", 0);
        let second = rect_of("tr", 1);
        let group = rect_of("tbody", 0);
        let column = rect_of("col", 0);

        // Each row spans the grid and holds exactly its own track.
        assert!((first.height - 40.0).abs() < 0.5, "first row: {first:?}");
        assert!((second.height - 60.0).abs() < 0.5, "second row: {second:?}");
        assert!(
            (second.y - first.y - 40.0).abs() < 0.5,
            "rows must tile: {first:?} {second:?}"
        );
        assert!((first.width - 200.0).abs() < 0.5, "{first:?}");

        // A group's rectangle is the exact union of its track range, not a
        // box reconstructed from the cells inside it.
        assert!((group.y - first.y).abs() < 0.5, "{group:?}");
        assert!((group.height - 100.0).abs() < 0.5, "{group:?}");

        // A column runs the table's whole block extent.
        assert!((column.height - table.height).abs() < 0.5, "{column:?}");
        assert!((column.width - 200.0).abs() < 0.5, "{column:?}");
    }

    /// K4d4c: `min-height` and `max-height` do not reach a table cell.
    ///
    /// CSS 2.1 section 10.7 leaves their effect on table cells, rows, and row
    /// groups undefined, and Chrome 150 and Firefox 153 both ignore them
    /// outright in all eight measured cases. So a cell carrying them is
    /// ordinary work rather than a deferral, and a 100px child keeps its
    /// 100px row against a `max-height: 20px` cell.
    #[test]
    fn k4d4c_cell_min_and_max_height_are_ignored() {
        let dom = StaticDocument::parse(
            "<table><tbody><tr><td><div class=tall></div></td></tr></tbody></table>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "table { display: table; table-layout: fixed; width: 100px; border-spacing: 0; }                  tbody { display: table-row-group; } tr { display: table-row; }                  td { display: table-cell; padding: 0; height: 20px; max-height: 20px;                  min-height: 5px; } .tall { height: 100px; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let ledger = layout.table_shadow_ledger();
        assert_eq!(
            ledger.block.laid_out, 1,
            "a cell max-height must not defer the table: {:?}",
            ledger.block
        );
        assert!(
            ledger.block.skipped.is_empty(),
            "{:?}",
            ledger.block.skipped
        );
        assert_eq!(
            ledger.block.agreed, 1,
            "the painted cell must be the one Buckram wrote: {:?}",
            ledger.block.divergences
        );
    }

    fn automatic_table_ledger(table_css: &str) -> crate::table_shadow::TableShadowLedger {
        let dom = StaticDocument::parse(
            "<table><tbody><tr><td>one</td><td>one</td><td>one</td></tr></tbody></table>",
        );
        let css = format!(
            "table {{ display: table; border-spacing: 0; {table_css} }} tbody {{ display: table-row-group; }} tr {{ display: table-row; }} td {{ display: table-cell; padding: 0; }}"
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[&css]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        layout(&dom, &styles, 320.0, 240.0)
            .expect("layout")
            .table_shadow_ledger()
            .clone()
    }

    /// K4c5b: a shrink-to-fit automatic table is sized by Buckram from cell
    /// intrinsics measured through the live machinery before the main pass.
    #[test]
    fn k4c5b_automatic_shrink_to_fit_is_buckram_owned() {
        assert_assigned_and_honored(&automatic_table_ledger(""));
    }

    /// The second K4c5a divergence, resolved by authority: an automatic table
    /// explicitly wider than its max-content distributes the extra space over
    /// its columns (CSS 2.1 17.5.2.2). Buckram's 100px columns paint,
    /// verified against the fragments.
    #[test]
    fn k4c5b_automatic_explicit_width_is_distributed_and_painted() {
        assert_assigned_and_honored(&automatic_table_ledger("width: 300px;"));
    }

    /// K4c5b on the production text path: automatic tables there previously
    /// could not even be shadowed; they are now sized by Buckram.
    #[test]
    fn k4c5b_text_path_automatic_tables_are_buckram_owned() {
        let dom = StaticDocument::parse(
            "<p>before</p><table><tbody><tr><td>one</td><td>two</td></tr></tbody></table>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "table { display: table; border-spacing: 0; } tbody { display: table-row-group; } tr { display: table-row; } td { display: table-cell; padding: 0; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let ledger = layout.table_shadow_ledger();
        assert!(
            ledger.assigned >= 1,
            "the text path's automatic table was not sized by Buckram: {ledger:?}"
        );
        assert!(ledger.is_silent(), "{ledger:?}");
    }

    /// Regression for the K4a/K4b-window crash in WPT
    /// `css/CSS2/css21-errata/s-11-1-1b-005.html`, run byte-exact from the
    /// in-repo corpus: the root element styled `display: table` with a bare
    /// `table-cell` `<body>` whose `margin-top: -15px` places its baseline
    /// above the parent's block-start edge. Baseline propagation asserted
    /// offsets were non-negative and panicked on the legitimate negative one.
    #[test]
    fn a_root_element_table_with_a_negative_margin_cell_does_not_panic() {
        let source = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/wpt/tests/css/CSS2/css21-errata/s-11-1-1b-005.html"
        ))
        .expect("in-repo WPT corpus file");
        let style_start = source.find("<style>").expect("style open") + "<style>".len();
        let style_end = source.find("</style>").expect("style close");
        let css = source[style_start..style_end].to_owned();
        let dom = StaticDocument::parse(&source);
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[&css]),
            &Device::screen(800.0, 600.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        layout_with_text_system(
            &dom,
            &styles,
            800.0,
            600.0,
            ViewportSizes::uniform(800.0, 600.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout must not panic");
    }

    /// K4d1 adapter fixture: a `TableCellFormatter` over the live algorithm
    /// tree formats block and flex cell contents at the exact inline size the
    /// table algorithm supplies. The scratch tree contains only cell
    /// subtrees, so the table and its row structurally record no backend
    /// call, while the flex cell dispatches through its own algorithm; the
    /// old bridge's table-as-Grid node does not exist here at all.
    #[test]
    fn k4d1_cell_formatter_formats_contents_at_exact_inline_sizes() {
        use buckram::{
            FragmentDraft, FragmentDraftTree, TableCellFormatter, TableCellLayoutInput,
            TableCellLayoutOutput, TableRowLayoutError,
        };

        struct TreeFormatter<'a> {
            tree: &'a mut AlgorithmTree<Style, TextMeasure, Option<BoxId>>,
            nodes: HashMap<BoxId, AlgorithmNodeId>,
            formatted: Vec<(BoxId, f32)>,
        }

        impl TableCellFormatter for TreeFormatter<'_> {
            fn format_cell(
                &mut self,
                input: TableCellLayoutInput,
            ) -> Result<TableCellLayoutOutput, TableRowLayoutError> {
                let node = *self.nodes.get(&input.box_id).ok_or(
                    TableRowLayoutError::InvalidCellOutput {
                        box_id: input.box_id,
                    },
                )?;
                self.tree.compute_layout_with_measure(
                    node,
                    AlgorithmSize::new(
                        AlgorithmAvailableSpace::Definite(input.content_inline_size),
                        AlgorithmAvailableSpace::MaxContent,
                    ),
                    |known, _, _, context, _| {
                        let Some(context) = context else {
                            return AlgorithmSize::new(0.0, 0.0);
                        };
                        AlgorithmSize::new(
                            known.width.unwrap_or(context.max_width),
                            known.height.unwrap_or(context.height),
                        )
                    },
                );
                let layout = self.tree.layout(node);
                self.formatted
                    .push((input.box_id, input.content_inline_size));
                let mut fragments = FragmentDraftTree::default();
                fragments.push(FragmentDraft {
                    box_id: input.box_id,
                    logical_rect: buckram::LogicalRect::default(),
                    overflow: buckram::LogicalRect::default(),
                    parent: None,
                });
                Ok(TableCellLayoutOutput {
                    content_block_size: layout.height,
                    border_box_min_block_size: layout.height,
                    // K4d5 owns real cell baselines; the contract placeholder
                    // synthesizes from the block end.
                    baselines: buckram::Baselines::synthesized_from_block_end(layout.height),
                    overflow: buckram::LogicalRect::default(),
                    fragments,
                })
            }
        }

        // A real grid from a two-cell table document; the algorithm tree gets
        // one block cell and one flex cell, and nothing else.
        let dom = StaticDocument::parse(
            "<table><tbody><tr><td id=blocky>x</td><td id=flexy><i>a</i><i>b</i></td></tr></tbody></table>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "table { display: table; border-spacing: 0; } tbody { display: table-row-group; } tr { display: table-row; } td { display: table-cell; padding: 0; } #flexy { display: table-cell; } #flexy i { display: block; height: 7px; }",
            ]),
            &Device::screen(800.0, 600.0),
            &InteractionStates::default(),
        );
        let boxes = GeneratedBoxTree::from_dom(&dom, &styles);
        let table = boxes
            .iter()
            .find_map(|(box_id, css_box)| {
                (css_box.display.internal_table == Some(buckram::InternalTableRole::Grid))
                    .then_some(box_id)
            })
            .expect("table grid box");
        let grid = build_table_grid(&boxes, &dom, table);
        assert_eq!(grid.cells.len(), 2);

        let mut tree: AlgorithmTree<Style, TextMeasure, Option<BoxId>> = AlgorithmTree::new();
        let mut nodes = HashMap::new();
        for cell in &grid.cells {
            let kind = if nodes.is_empty() {
                AlgorithmKind::Block
            } else {
                AlgorithmKind::Flex
            };
            let children = if kind == AlgorithmKind::Flex {
                vec![tree.new_with_children(
                    AlgorithmKind::Block,
                    Style {
                        size: Size {
                            width: Dimension::auto(),
                            height: Dimension::length(7.0),
                        },
                        ..Style::default()
                    },
                    &[],
                    None,
                )]
            } else {
                Vec::new()
            };
            let node = tree.new_with_children(kind, Style::default(), &children, None);
            nodes.insert(cell.source, node);
        }
        let inline = {
            let sizing = buckram::TableInlineSizingInput {
                grid: &grid,
                available_inline_size: Some(150.0),
                table_constraints: buckram::TableInlineConstraints::default(),
                border_metrics: buckram::TableInlineBorderMetrics::Separated(
                    buckram::TableSeparatedBorderMetrics::default(),
                ),
                caption_min: buckram::CaptionMinContribution::NoCaption,
                track_visibility: buckram::TableTrackVisibility::all_visible(&grid),
            };
            buckram::TableInlineSizingResult::new(
                &sizing,
                buckram::IntrinsicSizes::new(150.0, 150.0).expect("intrinsic pair"),
                150.0,
                150.0,
                vec![90.0, 60.0],
            )
            .expect("reconciled inline result")
        };
        let input = buckram::TableBlockSizingInput {
            grid: &grid,
            inline: &inline,
            table_constraint: buckram::TableBlockConstraint::Auto,
            table_box_sizing: buckram::TableBoxSizing::BorderBox,
            row_group_constraints: &[],
            border_metrics: buckram::TableBlockBorderMetrics::Separated(
                buckram::TableSeparatedBlockMetrics::default(),
            ),
            available_block_size: None,
            track_visibility: buckram::TableTrackVisibility::all_visible(&grid),
        };
        let mut formatter = TreeFormatter {
            tree: &mut tree,
            nodes,
            formatted: Vec::new(),
        };
        let outputs = buckram::format_table_cells(&input, 0.0, |_, _| 0.0, &mut formatter)
            .expect("formatted cells");

        assert_eq!(outputs.len(), 2);
        // Exact K4c column sizes reached the formatter.
        let widths = formatter
            .formatted
            .iter()
            .map(|(_, width)| *width)
            .collect::<Vec<_>>();
        assert_eq!(widths, vec![90.0, 60.0]);
        // The tree holds only cell subtrees: no node represents the table or
        // the row, so neither can have recorded a backend call.
        assert_eq!(tree.node_ids().count(), 3);
        assert!(
            tree.node_ids()
                .all(|id| tree.kind(id) != AlgorithmKind::Grid),
            "no table-as-Grid node may exist in the K4d dispatch shape"
        );
    }

    /// K4d2 adapter fixture: real cell contents are formatted at exact K4c
    /// inline sizes with an indefinite first-pass block size, and the row
    /// minima that follow come from those measured contents. The taller row
    /// is taller because its content is, not because anything was assumed.
    #[test]
    fn k4d2_row_minima_follow_from_formatted_cell_contents() {
        use buckram::{
            FragmentDraftTree, TableCellBlockStyle, TableCellFormatter, TableCellLayoutInput,
            TableCellLayoutOutput, TableCellLayoutPass, TableRowLayoutError,
        };

        struct TreeFormatter<'a> {
            tree: &'a mut AlgorithmTree<Style, TextMeasure, Option<BoxId>>,
            nodes: HashMap<BoxId, AlgorithmNodeId>,
            requests: Vec<TableCellLayoutInput>,
        }

        impl TableCellFormatter for TreeFormatter<'_> {
            fn format_cell(
                &mut self,
                input: TableCellLayoutInput,
            ) -> Result<TableCellLayoutOutput, TableRowLayoutError> {
                self.requests.push(input);
                let node = *self.nodes.get(&input.box_id).ok_or(
                    TableRowLayoutError::InvalidCellOutput {
                        box_id: input.box_id,
                    },
                )?;
                self.tree.compute_layout_with_measure(
                    node,
                    AlgorithmSize::new(
                        AlgorithmAvailableSpace::Definite(input.content_inline_size),
                        // The first pass is deliberately indefinite in the
                        // block axis: a cell height must not stretch its
                        // content formatting context.
                        AlgorithmAvailableSpace::MaxContent,
                    ),
                    |known, _, _, context, _| {
                        let Some(context) = context else {
                            return AlgorithmSize::new(0.0, 0.0);
                        };
                        AlgorithmSize::new(
                            known.width.unwrap_or(context.max_width),
                            known.height.unwrap_or(context.height),
                        )
                    },
                );
                let layout = self.tree.layout(node);
                Ok(TableCellLayoutOutput {
                    content_block_size: layout.height,
                    border_box_min_block_size: 0.0,
                    baselines: buckram::Baselines::synthesized_from_block_end(layout.height),
                    overflow: buckram::LogicalRect::default(),
                    fragments: FragmentDraftTree::default(),
                })
            }
        }

        // Row 0's cell is three stacked 9px blocks; row 1's is one.
        let dom = StaticDocument::parse(
            "<table><tbody><tr><td id=tall><i></i><i></i><i></i></td></tr><tr><td id=short><i></i></td></tr></tbody></table>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "table { display: table; border-spacing: 0; } tbody { display: table-row-group; } tr { display: table-row; } td { display: table-cell; padding: 0; } i { display: block; height: 9px; }",
            ]),
            &Device::screen(800.0, 600.0),
            &InteractionStates::default(),
        );
        let boxes = GeneratedBoxTree::from_dom(&dom, &styles);
        let table = boxes
            .iter()
            .find_map(|(box_id, css_box)| {
                (css_box.display.internal_table == Some(buckram::InternalTableRole::Grid))
                    .then_some(box_id)
            })
            .expect("table grid box");
        let grid = build_table_grid(&boxes, &dom, table);
        assert_eq!(grid.rows.len(), 2);
        assert_eq!(grid.cells.len(), 2);

        let mut tree: AlgorithmTree<Style, TextMeasure, Option<BoxId>> = AlgorithmTree::new();
        let mut nodes = HashMap::new();
        for (index, cell) in grid.cells.iter().enumerate() {
            let blocks = (0..if index == 0 { 3 } else { 1 })
                .map(|_| {
                    tree.new_with_children_and_block_style(
                        AlgorithmKind::Block,
                        BlockStyle {
                            size: BlockDimensions::new(
                                BlockSizeValue::Auto,
                                BlockSizeValue::Length(FlowLength::px(9.0)),
                            ),
                            ..BlockStyle::default()
                        },
                        Style {
                            size: Size {
                                width: Dimension::auto(),
                                height: Dimension::length(9.0),
                            },
                            ..Style::default()
                        },
                        &[],
                        None,
                    )
                })
                .collect::<Vec<_>>();
            nodes.insert(
                cell.source,
                tree.new_with_children_and_block_style(
                    AlgorithmKind::Block,
                    BlockStyle::default(),
                    Style::default(),
                    &blocks,
                    None,
                ),
            );
        }

        let inline = {
            let sizing = buckram::TableInlineSizingInput {
                grid: &grid,
                available_inline_size: Some(80.0),
                table_constraints: buckram::TableInlineConstraints::default(),
                border_metrics: buckram::TableInlineBorderMetrics::Separated(
                    buckram::TableSeparatedBorderMetrics::default(),
                ),
                caption_min: buckram::CaptionMinContribution::NoCaption,
                track_visibility: buckram::TableTrackVisibility::all_visible(&grid),
            };
            buckram::TableInlineSizingResult::new(
                &sizing,
                buckram::IntrinsicSizes::new(80.0, 80.0).expect("intrinsic pair"),
                80.0,
                80.0,
                vec![80.0],
            )
            .expect("reconciled inline result")
        };
        let input = buckram::TableBlockSizingInput {
            grid: &grid,
            inline: &inline,
            table_constraint: buckram::TableBlockConstraint::Auto,
            table_box_sizing: buckram::TableBoxSizing::BorderBox,
            row_group_constraints: &[],
            border_metrics: buckram::TableBlockBorderMetrics::Separated(
                buckram::TableSeparatedBlockMetrics::default(),
            ),
            available_block_size: None,
            track_visibility: buckram::TableTrackVisibility::all_visible(&grid),
        };
        let mut formatter = TreeFormatter {
            tree: &mut tree,
            nodes,
            requests: Vec::new(),
        };
        let outputs = buckram::format_table_cells(&input, 0.0, |_, _| 0.0, &mut formatter)
            .expect("formatted cells");

        // Every first-pass request carried the exact column size and an
        // indefinite block size.
        assert!(formatter.requests.iter().all(|request| {
            request.content_inline_size == 80.0
                && request.available_block_size.is_none()
                && request.percentage_basis.is_none()
                && request.pass == TableCellLayoutPass::Measure
        }));

        let rows = buckram::measure_single_span_rows(
            &input,
            &[TableCellBlockStyle::default(); 2],
            &outputs,
            &[buckram::TableBlockConstraint::Auto; 2],
        )
        .expect("row measures");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].min_block_size, 27.0);
        assert_eq!(rows[1].min_block_size, 9.0);
        assert!(rows.iter().all(|row| !row.constrained && row.row.is_some()));
    }

    /// K4d5 adapter fixture: block and flex cell contents return their
    /// baselines through the formatter's own output, and the table algorithm
    /// aligns from those values alone. Nothing walks a backend descendant to
    /// rediscover a baseline, and no physical coordinate is stored as one.
    #[test]
    fn k4d5_cell_contents_return_baselines_directly() {
        use buckram::{
            FragmentDraftTree, TableCellAlignment, TableCellBlockStyle, TableCellFormatter,
            TableCellLayoutInput, TableCellLayoutOutput, TableRowLayoutError,
        };

        struct BaselineFormatter<'a> {
            tree: &'a mut AlgorithmTree<Style, TextMeasure, Option<BoxId>>,
            nodes: HashMap<BoxId, AlgorithmNodeId>,
        }

        impl TableCellFormatter for BaselineFormatter<'_> {
            fn format_cell(
                &mut self,
                input: TableCellLayoutInput,
            ) -> Result<TableCellLayoutOutput, TableRowLayoutError> {
                let node = *self.nodes.get(&input.box_id).ok_or(
                    TableRowLayoutError::InvalidCellOutput {
                        box_id: input.box_id,
                    },
                )?;
                self.tree.compute_layout_with_measure(
                    node,
                    AlgorithmSize::new(
                        AlgorithmAvailableSpace::Definite(input.content_inline_size),
                        AlgorithmAvailableSpace::MaxContent,
                    ),
                    |known, _, _, context, _| {
                        let Some(context) = context else {
                            return AlgorithmSize::new(0.0, 0.0);
                        };
                        AlgorithmSize::new(
                            known.width.unwrap_or(context.max_width),
                            known.height.unwrap_or(context.height),
                        )
                    },
                );
                self.tree.propagate_baselines();
                let layout = self.tree.layout(node);
                // The formatting context hands its baselines back directly.
                let baselines = self.tree.baselines(node);
                Ok(TableCellLayoutOutput {
                    content_block_size: layout.height,
                    border_box_min_block_size: 0.0,
                    baselines,
                    overflow: buckram::LogicalRect::default(),
                    fragments: FragmentDraftTree::default(),
                })
            }
        }

        let dom = StaticDocument::parse(
            "<table><tbody><tr><td id=a><i></i></td><td id=b><i></i><i></i></td></tr></tbody></table>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "table { display: table; border-spacing: 0; } tbody { display: table-row-group; } tr { display: table-row; } td { display: table-cell; padding: 0; } i { display: block; height: 12px; }",
            ]),
            &Device::screen(800.0, 600.0),
            &InteractionStates::default(),
        );
        let boxes = GeneratedBoxTree::from_dom(&dom, &styles);
        let table = boxes
            .iter()
            .find_map(|(box_id, css_box)| {
                (css_box.display.internal_table == Some(buckram::InternalTableRole::Grid))
                    .then_some(box_id)
            })
            .expect("table grid box");
        let grid = build_table_grid(&boxes, &dom, table);

        let mut tree: AlgorithmTree<Style, TextMeasure, Option<BoxId>> = AlgorithmTree::new();
        let mut nodes = HashMap::new();
        for (index, cell) in grid.cells.iter().enumerate() {
            // A block container's first baseline is its first child's, so the
            // cells differ in their first block rather than their count.
            let height = if index == 0 { 20.0 } else { 12.0 };
            let blocks = vec![tree.new_with_children_and_block_style(
                AlgorithmKind::Block,
                BlockStyle {
                    size: BlockDimensions::new(
                        BlockSizeValue::Auto,
                        BlockSizeValue::Length(FlowLength::px(height)),
                    ),
                    ..BlockStyle::default()
                },
                Style {
                    size: Size {
                        width: Dimension::auto(),
                        height: Dimension::length(height),
                    },
                    ..Style::default()
                },
                &[],
                None,
            )];
            nodes.insert(
                cell.source,
                tree.new_with_children_and_block_style(
                    AlgorithmKind::Block,
                    BlockStyle::default(),
                    Style::default(),
                    &blocks,
                    None,
                ),
            );
        }

        let inline = {
            let sizing = buckram::TableInlineSizingInput {
                grid: &grid,
                available_inline_size: Some(80.0),
                table_constraints: buckram::TableInlineConstraints::default(),
                border_metrics: buckram::TableInlineBorderMetrics::Separated(
                    buckram::TableSeparatedBorderMetrics::default(),
                ),
                caption_min: buckram::CaptionMinContribution::NoCaption,
                track_visibility: buckram::TableTrackVisibility::all_visible(&grid),
            };
            buckram::TableInlineSizingResult::new(
                &sizing,
                buckram::IntrinsicSizes::new(80.0, 80.0).expect("intrinsic pair"),
                80.0,
                80.0,
                vec![40.0, 40.0],
            )
            .expect("reconciled inline result")
        };
        let input = buckram::TableBlockSizingInput {
            grid: &grid,
            inline: &inline,
            table_constraint: buckram::TableBlockConstraint::Auto,
            table_box_sizing: buckram::TableBoxSizing::BorderBox,
            row_group_constraints: &[],
            border_metrics: buckram::TableBlockBorderMetrics::Separated(
                buckram::TableSeparatedBlockMetrics::default(),
            ),
            available_block_size: None,
            track_visibility: buckram::TableTrackVisibility::all_visible(&grid),
        };
        let mut formatter = BaselineFormatter {
            tree: &mut tree,
            nodes,
        };
        let outputs = buckram::format_table_cells(&input, 0.0, |_, _| 0.0, &mut formatter)
            .expect("formatted cells");
        // Each cell reported a real baseline from its own formatting context.
        assert!(
            outputs
                .iter()
                .all(|(_, output)| output.baselines.first.is_some())
        );

        let styles = vec![TableCellBlockStyle::default(); grid.cells.len()];
        let mut measures = buckram::measure_single_span_rows(
            &input,
            &styles,
            &outputs,
            &[buckram::TableBlockConstraint::Auto],
        )
        .expect("measures");
        buckram::apply_baseline_row_minima(&input, &styles, &outputs, &mut measures)
            .expect("baseline minima");
        let sizing =
            buckram::size_table_rows(&input, &measures, &styles, &outputs).expect("sizing");
        let alignment =
            buckram::align_table_cells(&input, &sizing, &styles, &outputs, 0.0).expect("alignment");

        // Cell a's baseline is 20 and cell b's is 12, so the row takes 20 and
        // shifts b down by 8 to meet it.
        assert!(alignment.rows[0].from_aligned_cell);
        assert!(
            (alignment.rows[0].baseline - 20.0).abs() < 0.05,
            "{alignment:?}"
        );
        assert!(
            (alignment.cells[0].content_block_offset).abs() < 0.05,
            "{alignment:?}"
        );
        assert!(
            (alignment.cells[1].content_block_offset - 8.0).abs() < 0.05,
            "{alignment:?}"
        );
        assert_eq!(
            alignment.baselines.first,
            Some(alignment.rows[0].baseline),
            "the table's first baseline is its first row's"
        );
        // Alignment never touches K4c's columns.
        assert_eq!(inline.column_sizes, vec![40.0, 40.0]);
        assert!(
            alignment
                .cells
                .iter()
                .all(|cell| (cell.rect.inline_size - 40.0).abs() < 0.05)
        );
        assert_eq!(
            buckram::TableCellAlignment::default(),
            TableCellAlignment::Baseline
        );
    }

    #[test]
    fn html_column_and_column_group_spans_are_bounded_at_the_adapter() {
        let dom = StaticDocument::parse(
            "<table id=table><colgroup span=9001></colgroup><col span=9001><tbody><tr><td></td></tr></tbody></table>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "table { display: table; } colgroup { display: table-column-group; } col { display: table-column; } tbody { display: table-row-group; } tr { display: table-row; } td { display: table-cell; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let boxes = GeneratedBoxTree::from_dom(&dom, &styles);
        let table = node_by_id(&dom, dom.document(), "table").expect("table");
        let grid = boxes.principal_box(table).expect("table grid");
        let model = build_table_grid(&boxes, &dom, grid);

        assert_eq!(model.column_groups[0].span, 1_000);
        assert_eq!(model.columns.len(), 2_000);
    }

    #[test]
    fn retained_inline_format_is_not_shaped_again_for_paint() {
        let dom = StaticDocument::parse(
            "<html><body><div class=\"label\"><span id=\"split\">one two three four</span></div></body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[".label { width: 80px; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (styles, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let after_layout = text.shape_count();
        let split = {
            fn find(
                dom: &StaticDocument,
                node: <StaticDocument as LayoutDom>::NodeId,
            ) -> Option<<StaticDocument as LayoutDom>::NodeId> {
                if dom
                    .element_name(node)
                    .is_some_and(|name| name.local.as_ref() == "span")
                {
                    return Some(node);
                }
                dom.dom_children(node).find_map(|child| find(dom, child))
            }
            find(&dom, dom.document()).expect("split span")
        };

        assert!(after_layout > 0);
        assert!(
            layout.fragments_for_node(split).count() >= 2,
            "one inline box must own one fragment per wrapped line"
        );
        let _ = emit_paint_list_with_text_system(
            &dom,
            &styles,
            &layout,
            DeviceIntSize::new(320, 240),
            1,
            &mut text,
        );
        assert_eq!(
            text.shape_count(),
            after_layout,
            "paint must consume the retained inline result"
        );
    }

    #[test]
    fn split_inline_continuations_format_their_own_box_children() {
        let dom = StaticDocument::parse(
            "<html><body><div class=\"host\"><span>before<div class=\"block\">block</div>after</span></div></body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                ".host { width: 120px; } .block { display: block; height: 20px; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let split = {
            fn find(
                dom: &StaticDocument,
                node: <StaticDocument as LayoutDom>::NodeId,
            ) -> Option<<StaticDocument as LayoutDom>::NodeId> {
                if dom
                    .element_name(node)
                    .is_some_and(|name| name.local.as_ref() == "span")
                {
                    return Some(node);
                }
                dom.dom_children(node).find_map(|child| find(dom, child))
            }
            find(&dom, dom.document()).expect("split span")
        };
        let boxes = layout.boxes().boxes_for_node(split);
        let first = layout
            .fragments()
            .fragments_for_box(boxes[0])
            .next()
            .expect("first continuation")
            .physical_rect();
        let second = layout
            .fragments()
            .fragments_for_box(boxes[1])
            .next()
            .expect("second continuation")
            .physical_rect();

        assert_eq!(boxes.len(), 2);
        assert!(
            second.y > first.y,
            "the block between continuation boxes must advance block flow"
        );
    }

    #[test]
    fn partial_inline_groups_do_not_share_one_box_intrinsic_cache_entry() {
        fn find(
            dom: &StaticDocument,
            node: <StaticDocument as LayoutDom>::NodeId,
        ) -> Option<<StaticDocument as LayoutDom>::NodeId> {
            if dom
                .element_name(node)
                .is_some_and(|name| name.local.as_ref() == "div")
            {
                return Some(node);
            }
            dom.dom_children(node).find_map(|child| find(dom, child))
        }

        let dom = StaticDocument::parse(
            "<html><body><div>before<span class=\"out\">out</span>after</div></body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[".out { position: absolute; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let boxes = GeneratedBoxTree::from_dom(&dom, &styles);
        let host = find(&dom, dom.document()).expect("host");
        let host_box = boxes.principal_box(host).expect("host box");

        assert_eq!(
            intrinsic_owner_for_flow_children(&boxes, host_box, boxes[host_box].children()),
            None,
            "two partial inline groups must not alias the parent box query"
        );
    }

    #[test]
    fn ordinary_live_block_flow_uses_buckram_without_backend_dispatch() {
        fn collect_divs(
            dom: &StaticDocument,
            node: <StaticDocument as LayoutDom>::NodeId,
            output: &mut Vec<<StaticDocument as LayoutDom>::NodeId>,
        ) {
            if dom
                .element_name(node)
                .is_some_and(|name| name.local.as_ref() == "div")
            {
                output.push(node);
            }
            for child in dom.dom_children(node) {
                collect_divs(dom, child, output);
            }
        }

        let dom = StaticDocument::parse(
            "<html><body><div class=\"host\"><div></div><div></div></div></body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "html, body, div { margin: 0; padding: 0; border: 0; } .host > div { height: 20px; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let mut divs = Vec::new();
        collect_divs(&dom, dom.document(), &mut divs);
        let first = layout.get(divs[1]).expect("first child").physical_rect();
        let second = layout.get(divs[2]).expect("second child").physical_rect();
        let algorithms = layout.block_algorithm_counts();

        assert!(
            algorithms.buckram >= 4,
            "the root, html, body, and host block contexts should use Buckram"
        );
        assert_eq!(algorithms.taffy, 0);
        assert_eq!(second.y, first.y + 20.0);
    }

    #[test]
    fn live_orthogonal_normal_flow_preserves_logical_fragment_geometry_and_baseline() {
        use buckram::{Direction, WritingMode};

        let document = StaticDocument::parse(
            "<html><body><div class=\"vertical\"><div>orthogonal text</div></div></body></html>",
        );
        let styles = resolve_styles(
            &document,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; border: 0; } \
                 .vertical { writing-mode: vertical-rl; height: 100px; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let vertical = document
            .first_with_class(document.document(), "vertical")
            .expect("vertical host");
        assert!(
            styles
                .get(vertical)
                .is_some_and(|style| style.writing_mode.is_vertical()),
            "the cascade must retain vertical-rl for the principal box"
        );
        let mut text = TextSystem::new();
        let (resolved, layout) = layout_with_text_system(
            &document,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("orthogonal layout");

        assert!(
            resolved
                .get(vertical)
                .is_some_and(|style| style.writing_mode.is_vertical()),
            "relative-unit resolution must preserve vertical-rl"
        );

        let vertical_box = layout
            .boxes()
            .principal_box(vertical)
            .expect("vertical principal box");
        assert_eq!(
            layout.boxes()[vertical_box].flow,
            FlowAxes::new(WritingMode::VerticalRl, Direction::Ltr),
            "the generated principal box must preserve the computed flow"
        );
        let fragment = layout
            .fragments()
            .fragments_for_box(vertical_box)
            .next()
            .expect("vertical fragment");
        let physical = fragment.physical_rect();
        assert_eq!(
            fragment.flow(),
            FlowAxes::new(WritingMode::VerticalRl, Direction::Ltr)
        );
        assert_eq!(fragment.logical_rect.inline_size, physical.height);
        assert_eq!(fragment.logical_rect.block_size, physical.width);
        assert!(
            fragment.baselines.first.is_some() && fragment.baselines.last.is_some(),
            "the host must retain its modeled BFC baseline output"
        );
        assert!(
            fragment
                .containing_fragment()
                .and_then(|id| layout.fragments().get(id))
                .is_some(),
            "the orthogonal host must retain its containing fragment"
        );
        assert!(layout.block_algorithm_counts().buckram >= 4);
    }

    #[test]
    fn live_two_level_orthogonal_flow_uses_the_inner_line_block_contribution() {
        let document = StaticDocument::parse(
            "<html><body><div class=\"vertical\"><div class=\"line\">A B C D E F G</div></div><div class=\"after\"></div></body></html>",
        );
        let styles = resolve_styles(
            &document,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; border: 0; } \
                 .vertical { writing-mode: vertical-rl; background: yellow; } \
                 .line { writing-mode: horizontal-tb; } .after { height: 10px; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let vertical = document
            .first_with_class(document.document(), "vertical")
            .expect("vertical host");
        let line = document
            .first_with_class(document.document(), "line")
            .expect("horizontal line");
        let after = document
            .first_with_class(document.document(), "after")
            .expect("following block");
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &document,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("two-level orthogonal layout");
        let vertical = layout
            .get(vertical)
            .expect("vertical fragment")
            .physical_rect();
        let line = layout.get(line).expect("line fragment").physical_rect();
        let after = layout
            .get(after)
            .expect("following fragment")
            .physical_rect();

        assert!(vertical.height > 0.0 && vertical.height < 40.0);
        assert_eq!(vertical.height, line.height);
        assert_eq!(vertical.width, line.width);
        assert_eq!(after.y, vertical.height);
    }

    #[test]
    fn contained_root_keeps_body_writing_mode_local_to_body_content() {
        fn first_text(
            dom: &StaticDocument,
            node: <StaticDocument as LayoutDom>::NodeId,
        ) -> Option<<StaticDocument as LayoutDom>::NodeId> {
            if dom.kind(node) == NodeKind::Text
                && dom.text(node).is_some_and(|text| !text.is_empty())
            {
                return Some(node);
            }
            dom.dom_children(node)
                .find_map(|child| first_text(dom, child))
        }

        fn text_fragment_x(
            document: &StaticDocument,
            css: &str,
        ) -> (
            f32,
            Fragment,
            FlowAxes,
            Option<FlowAxes>,
            (CssSize, CssSize),
        ) {
            let styles = resolve_styles(
                document,
                &StyleSet::cambium(&[css]),
                &Device::screen(800.0, 600.0),
                &InteractionStates::default(),
            );
            let mut text = TextSystem::new();
            let (_, layout) = layout_with_text_system(
                document,
                &styles,
                800.0,
                600.0,
                ViewportSizes::uniform(800.0, 600.0),
                &mut text,
                &HashMap::new(),
            )
            .expect("writing-mode layout");
            let source = first_text(document, document.document()).expect("text node");
            let text_x = layout
                .fragments_for_node(source)
                .next()
                .expect("text fragment")
                .physical_rect()
                .x;
            let parent = document.parent(source).expect("text parent");
            let parent = layout.get(parent).expect("parent fragment").physical_rect();
            let parent_box = layout
                .boxes()
                .principal_box(document.parent(source).expect("text parent"))
                .expect("parent box");
            let flow = layout.boxes()[parent_box].flow;
            let containing_flow = layout.boxes()[parent_box]
                .parent()
                .map(|containing| layout.boxes()[containing].flow);
            let style = styles
                .get(document.parent(source).expect("text parent"))
                .expect("parent style");
            (
                text_x,
                parent,
                flow,
                containing_flow,
                (style.width, style.height),
            )
        }

        let target = StaticDocument::parse(
            "<html><body>This text should run vertically on the left side</body></html>",
        );
        let reference = StaticDocument::parse(
            "<html><body><div>This text should run vertically on the left side</div></body></html>",
        );
        let target_x = text_fragment_x(
            &target,
            "html { contain: paint; } body { writing-mode: vertical-rl; }",
        );
        let reference_x = text_fragment_x(&reference, "div { writing-mode: vertical-rl; }");

        assert_eq!(
            target_x.0, reference_x.0,
            "target={target_x:?} reference={reference_x:?}"
        );
    }

    #[test]
    fn replaced_html_dimensions_use_computed_css_and_canvas_intrinsics() {
        fn find_by_name(
            dom: &StaticDocument,
            node: <StaticDocument as LayoutDom>::NodeId,
            name: &str,
        ) -> Option<<StaticDocument as LayoutDom>::NodeId> {
            if dom
                .element_name(node)
                .is_some_and(|element| element.local.as_ref() == name)
            {
                return Some(node);
            }
            dom.dom_children(node)
                .find_map(|child| find_by_name(dom, child, name))
        }

        let dom = StaticDocument::parse(
            "<html><body><div><img width=\"100%\" height=\"3\">\
             <canvas width=\"100\" height=\"100\"></canvas></div></body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "html, body { margin: 0; } div { position: relative; width: 200px; }\
                 img { position: absolute; left: 0; top: 0; } canvas { display: block; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let image = find_by_name(&dom, dom.document(), "img").expect("img");
        assert_eq!(
            styles.get(image).unwrap().width,
            CssSize::Value(CssLengthPercentage::Percentage(1.0))
        );
        assert_eq!(
            styles.get(image).unwrap().height,
            CssSize::Value(CssLengthPercentage::Length(Length::px(3.0)))
        );
        let canvas = find_by_name(&dom, dom.document(), "canvas").expect("canvas");
        assert_eq!(styles.get(canvas).unwrap().width, CssSize::Auto);
        assert_eq!(styles.get(canvas).unwrap().height, CssSize::Auto);
        assert_eq!(
            styles.get(canvas).unwrap().aspect_ratio,
            livery::values::AspectRatio::AutoRatio {
                width: 100.0,
                height: 100.0,
            },
            "canvas dimensions remain natural-size inputs rather than CSS dimensions"
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");

        let image = layout.get(image).expect("image fragment").physical_rect();
        assert_eq!(
            (image.width, image.height),
            (200.0, 3.0),
            "the percentage hint resolves against the positioned containing block"
        );

        let canvas = layout.get(canvas).expect("canvas fragment").physical_rect();
        assert_eq!((canvas.width, canvas.height), (100.0, 100.0));
    }

    #[test]
    fn positioned_replaced_leaf_keeps_its_hint_size_between_definite_insets() {
        fn find_by_name(
            dom: &StaticDocument,
            node: <StaticDocument as LayoutDom>::NodeId,
            name: &str,
        ) -> Option<<StaticDocument as LayoutDom>::NodeId> {
            if dom
                .element_name(node)
                .is_some_and(|element| element.local.as_ref() == name)
            {
                return Some(node);
            }
            dom.dom_children(node)
                .find_map(|child| find_by_name(dom, child, name))
        }

        let dom = StaticDocument::parse(
            "<html><body><div><canvas width=\"80\" height=\"40\"></canvas></div></body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "html, body { margin: 0; } div { position: relative; width: 200px; } \
                 canvas { position: absolute; left: 10px; right: 20px; top: 5px; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");

        let canvas = find_by_name(&dom, dom.document(), "canvas").expect("canvas");
        let canvas = layout.get(canvas).expect("canvas fragment").physical_rect();
        assert_eq!(
            (canvas.x, canvas.y, canvas.width, canvas.height),
            (10.0, 5.0, 80.0, 40.0)
        );
    }

    #[test]
    fn percentage_height_chain_uses_initial_containing_block_height() {
        fn find_by_name(
            dom: &StaticDocument,
            node: <StaticDocument as LayoutDom>::NodeId,
            name: &str,
        ) -> Option<<StaticDocument as LayoutDom>::NodeId> {
            if dom
                .element_name(node)
                .is_some_and(|element| element.local.as_ref() == name)
            {
                return Some(node);
            }
            dom.dom_children(node)
                .find_map(|child| find_by_name(dom, child, name))
        }

        let dom = StaticDocument::parse("<html><body><p>viewport</p></body></html>");
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "html, body, p { height: 100%; margin: 0; padding: 0; border: 0; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");

        for name in ["html", "body", "p"] {
            let node = find_by_name(&dom, dom.document(), name).expect(name);
            assert_eq!(
                layout.get(node).expect(name).physical_rect().height,
                240.0,
                "{name} should resolve 100% against a definite containing block"
            );
        }
        assert_eq!(layout.block_algorithm_counts().taffy, 0);
    }

    #[test]
    fn live_block_flow_keeps_collapsed_margin_chains_in_buckram() {
        fn collect_divs(
            dom: &StaticDocument,
            node: <StaticDocument as LayoutDom>::NodeId,
            output: &mut Vec<<StaticDocument as LayoutDom>::NodeId>,
        ) {
            if dom
                .element_name(node)
                .is_some_and(|name| name.local.as_ref() == "div")
            {
                output.push(node);
            }
            for child in dom.dom_children(node) {
                collect_divs(dom, child, output);
            }
        }

        let dom = StaticDocument::parse(
            "<html><body><div class=\"host\">\
             <div class=\"parent\"><div class=\"child\"></div></div>\
             <div class=\"after\"></div>\
             <div class=\"chain\"><div class=\"first\"></div><div class=\"empty\"></div>\
             <div class=\"last\"></div></div>\
             </div></body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "html, body, .host, .chain { margin: 0; padding: 0; border: 0; }\
                 .parent { margin: 10px 0 15px; }\
                 .child { height: 20px; margin: 30px 0 40px; }\
                 .after { height: 10px; margin: 12px 0 0; }\
                 .first { height: 10px; margin: 0 0 20px; }\
                 .empty { margin: -7px 0 12px; }\
                 .last { height: 10px; margin: -15px 0 0; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let mut divs = Vec::new();
        collect_divs(&dom, dom.document(), &mut divs);
        let parent = layout.get(divs[1]).expect("parent").physical_rect();
        let child = layout.get(divs[2]).expect("child").physical_rect();
        let after = layout.get(divs[3]).expect("after").physical_rect();
        let first = layout.get(divs[5]).expect("first").physical_rect();
        let empty = layout.get(divs[6]).expect("empty").physical_rect();
        let last = layout.get(divs[7]).expect("last").physical_rect();
        let algorithms = layout.block_algorithm_counts();

        assert_eq!(child.y, parent.y);
        assert_eq!(after.y, parent.y + 60.0);
        assert_eq!(empty.y, first.y + 23.0);
        assert_eq!(last.y, first.y + 15.0);
        assert!(algorithms.buckram >= 6);
        assert_eq!(algorithms.taffy, 0);
    }

    #[test]
    fn live_bfc_places_blockified_floats_and_direct_clearance_in_buckram() {
        fn by_class(
            dom: &StaticDocument,
            node: <StaticDocument as LayoutDom>::NodeId,
            expected: &str,
        ) -> Option<<StaticDocument as LayoutDom>::NodeId> {
            if dom.attributes(node).any(|attribute| {
                attribute.name.ns.as_ref().is_empty()
                    && attribute.name.local.as_ref() == "class"
                    && attribute.value == expected
            }) {
                return Some(node);
            }
            dom.dom_children(node)
                .find_map(|child| by_class(dom, child, expected))
        }

        let dom = StaticDocument::parse(
            "<html><body><div class=\"host\">\
             <span class=\"left\"></span><div class=\"right\"></div>\
             <div class=\"clear\"></div></div></body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "html, body, div, span { margin: 0; padding: 0; border: 0; }\
                 .host { width: 200px; overflow-x: hidden; overflow-y: hidden; }\
                 .left { float: left; width: 80px; height: 40px; }\
                 .right { float: right; width: 60px; height: 70px; }\
                 .clear { clear: both; height: 10px; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let rect = |class| {
            let node = by_class(&dom, dom.document(), class).expect(class);
            layout.get(node).expect(class).physical_rect()
        };

        let host = rect("host");
        let left = rect("left");
        let right = rect("right");
        let clear = rect("clear");
        let algorithms = layout.block_algorithm_counts();

        assert_eq!((left.x, left.y), (host.x, host.y));
        assert_eq!((right.x, right.y), (host.x + 140.0, host.y));
        assert_eq!((clear.x, clear.y), (host.x, host.y + 70.0));
        assert_eq!(host.height, 80.0);
        assert!(algorithms.buckram >= 4);
        assert_eq!(algorithms.taffy, 0);
    }

    #[test]
    fn live_empty_clearance_keeps_its_following_margin_chain_in_buckram() {
        fn by_id(
            dom: &StaticDocument,
            node: <StaticDocument as LayoutDom>::NodeId,
            expected: &str,
        ) -> Option<<StaticDocument as LayoutDom>::NodeId> {
            if dom.attributes(node).any(|attribute| {
                attribute.name.ns.as_ref().is_empty()
                    && attribute.name.local.as_ref() == "id"
                    && attribute.value == expected
            }) {
                return Some(node);
            }
            dom.dom_children(node)
                .find_map(|child| by_id(dom, child, expected))
        }

        let dom = StaticDocument::parse(
            "<html><body><div id=\"host\">\
             <div id=\"float\"></div><div id=\"empty\"></div><div id=\"after\"></div>\
             </div></body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; border: 0; }\
                 #host { width: 200px; overflow-x: hidden; overflow-y: hidden; }\
                 #float { float: left; width: 80px; height: 40px; }\
                 #empty { clear: left; margin-top: 10px; margin-bottom: 20px; }\
                 #after { height: 10px; margin-top: 30px; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let rect = |id| {
            let node = by_id(&dom, dom.document(), id).expect(id);
            layout.get(node).expect(id).physical_rect()
        };

        let host = rect("host");
        let float = rect("float");
        let empty = rect("empty");
        let after = rect("after");
        let algorithms = layout.block_algorithm_counts();

        assert_eq!((float.x - host.x, float.y - host.y), (0.0, 0.0));
        assert_eq!(
            (empty.y - host.y, empty.height),
            (40.0, 0.0),
            "host={host:?}, float={float:?}, empty={empty:?}, after={after:?}, algorithms={algorithms:?}"
        );
        assert_eq!((after.y - host.y, after.height), (70.0, 10.0));
        assert_eq!(host.height, 80.0);
        assert_eq!(algorithms.taffy, 0);
    }

    #[test]
    fn live_inline_lines_in_an_ordinary_wrapper_share_outer_float_exclusions() {
        fn by_id(
            dom: &StaticDocument,
            node: <StaticDocument as LayoutDom>::NodeId,
            expected: &str,
        ) -> Option<<StaticDocument as LayoutDom>::NodeId> {
            if dom.attributes(node).any(|attribute| {
                attribute.name.ns.as_ref().is_empty()
                    && attribute.name.local.as_ref() == "id"
                    && attribute.value == expected
            }) {
                return Some(node);
            }
            dom.dom_children(node)
                .find_map(|child| by_id(dom, child, expected))
        }

        let dom = StaticDocument::parse(
            "<html><body><div id=\"host\"><div id=\"float\"></div>\
             <div id=\"wrapper\"><span id=\"copy\">aa aa aa aa aa aa aa aa aa aa aa aa \
             aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa \
             aa aa</span></div>\
             </div></body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "html, body, div, span { margin: 0; padding: 0; border: 0; }\
                 #host { width: 200px; overflow-x: hidden; overflow-y: hidden;\
                         font-family: monospace; font-size: 10px; line-height: 20px; }\
                 #float { float: left; width: 80px; height: 40px; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let host = by_id(&dom, dom.document(), "host").expect("host");
        let copy = by_id(&dom, dom.document(), "copy").expect("copy");
        let host = layout.get(host).expect("host fragment").physical_rect();
        let algorithms = layout.block_algorithm_counts();
        let mut lines = layout
            .fragments_for_node(copy)
            .map(|fragment| fragment.physical_rect())
            .collect::<Vec<_>>();
        lines.sort_by(|left, right| left.y.total_cmp(&right.y));

        assert!(
            lines.len() >= 4,
            "fixture must produce several line fragments"
        );
        assert!(
            (lines[0].x - (host.x + 80.0)).abs() <= 0.5,
            "host={host:?}, lines={lines:?}, algorithms={algorithms:?}"
        );
        assert!(
            (lines[1].x - (host.x + 80.0)).abs() <= 0.5,
            "host={host:?}, lines={lines:?}, algorithms={algorithms:?}"
        );
        assert!(
            lines
                .iter()
                .filter(|line| line.y >= host.y + 40.0)
                .all(|line| (line.x - host.x).abs() <= 0.5),
            "lines below the float must use the full content column"
        );
        assert_eq!(algorithms.taffy, 0);
    }

    #[test]
    fn live_shape_outside_reference_boxes_change_lines_but_not_float_placement() {
        fn by_id(
            dom: &StaticDocument,
            node: <StaticDocument as LayoutDom>::NodeId,
            expected: &str,
        ) -> Option<<StaticDocument as LayoutDom>::NodeId> {
            if dom.attributes(node).any(|attribute| {
                attribute.name.ns.as_ref().is_empty()
                    && attribute.name.local.as_ref() == "id"
                    && attribute.value == expected
            }) {
                return Some(node);
            }
            dom.dom_children(node)
                .find_map(|child| by_id(dom, child, expected))
        }

        let hosts = ["none", "margin", "border", "padding", "content", "curved"];
        let markup = hosts
            .iter()
            .map(|name| {
                format!(
                    "<div id=\"host-{name}\" class=\"host\"><div class=\"float {name}\"></div>\
                     <div><span id=\"copy-{name}\">aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa \
                     aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa</span></div></div>"
                )
            })
            .collect::<String>();
        let dom = StaticDocument::parse(&format!("<html><body>{markup}</body></html>"));
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "html, body, div, span { margin: 0; padding: 0; border: 0; }\
                 .host { width: 200px; overflow-x: hidden; overflow-y: hidden;\
                         font-family: monospace; font-size: 10px; line-height: 20px; }\
                 .float { float: left; width: 50px; height: 80px; margin-right: 20px;\
                          padding-right: 10px; border-right: 20px solid; }\
                 .margin { shape-outside: margin-box; }\
                 .border { shape-outside: border-box; }\
                 .padding { shape-outside: padding-box; }\
                 .content { shape-outside: content-box; }\
                 .curved { shape-outside: content-box; border-radius: 10px; }",
            ]),
            &Device::screen(320.0, 600.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            600.0,
            ViewportSizes::uniform(320.0, 600.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let algorithms = layout.block_algorithm_counts();

        for (name, expected_line_start) in [
            ("none", 100.0),
            ("margin", 100.0),
            ("border", 80.0),
            ("padding", 60.0),
            ("content", 50.0),
            ("curved", 50.0),
        ] {
            let host_node = by_id(&dom, dom.document(), &format!("host-{name}"))
                .unwrap_or_else(|| panic!("host-{name}"));
            let copy_node = by_id(&dom, dom.document(), &format!("copy-{name}"))
                .unwrap_or_else(|| panic!("copy-{name}"));
            let host = layout
                .get(host_node)
                .unwrap_or_else(|| panic!("host-{name} fragment"))
                .physical_rect();
            let float_node = dom
                .dom_children(host_node)
                .next()
                .unwrap_or_else(|| panic!("float-{name}"));
            let float = layout
                .get(float_node)
                .unwrap_or_else(|| panic!("float-{name} fragment"))
                .physical_rect();
            let first_line = layout
                .fragments_for_node(copy_node)
                .map(|fragment| fragment.physical_rect())
                .min_by(|left, right| left.y.total_cmp(&right.y))
                .unwrap_or_else(|| panic!("copy-{name} line"));

            assert_eq!((float.x - host.x, float.y - host.y), (0.0, 0.0));
            assert!(
                (first_line.x - host.x - expected_line_start).abs() <= 0.5,
                "name={name}, host={host:?}, float={float:?}, line={first_line:?}"
            );
            assert!(host.height >= 80.0);
        }
        assert_eq!(algorithms.taffy, 0);
    }

    #[test]
    fn live_shape_outside_keeps_forced_break_lines_in_the_selected_float_band() {
        let dom = StaticDocument::parse(
            "<html><body><div id=\"container\"><div id=\"host\"><div id=\"shape\"></div>\
             <br><br>\n            X\n</div></div></body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["body { margin: 0; }\
                 #container { position: relative; }\
                 #host { width: 300px; height: 200px; font-family: monospace;\
                         font-size: 40px; line-height: 40px; }\
                 #shape { float: left; width: 150px; height: 150px; margin: 10px;\
                          padding: 10px; border: 10px solid transparent;\
                          shape-outside: border-box; }"]),
            &Device::screen(400.0, 300.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            400.0,
            300.0,
            ViewportSizes::uniform(400.0, 300.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let host = node_by_id(&dom, dom.document(), "host").expect("host");
        let copy = dom
            .dom_children(host)
            .find(|node| dom.text(*node).is_some_and(|text| text.contains('X')))
            .expect("direct text");
        let host = layout.get(host).expect("host fragment").physical_rect();
        let copy = layout
            .fragments_for_node(copy)
            .map(|fragment| fragment.physical_rect())
            .max_by(|left, right| left.width.total_cmp(&right.width))
            .expect("copy line");

        assert!(
            (copy.x - host.x - 200.0).abs() <= 0.5,
            "forced breaks must retain the border-box line origin: host={host:?}, copy={copy:?}"
        );
        assert!(
            (copy.y - host.y - 80.0).abs() <= 0.5,
            "host={host:?}, copy={copy:?}"
        );
        assert_eq!(layout.block_algorithm_counts().taffy, 0);
    }

    #[test]
    fn relative_zero_height_wrapper_retains_its_floated_descendant() {
        let dom = StaticDocument::parse(
            "<html><body><div id=\"outer\"><div id=\"float\"><div></div></div>\
             <div id=\"absolute\"></div></div></body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["body { margin: 0; }\
                 #outer { position: relative; }\
                 #float { float: left; }\
                 #float > div, #absolute { width: 96px; height: 96px; }\
                 #absolute { position: absolute; left: 96px; top: 0; }"]),
            &Device::screen(400.0, 300.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            400.0,
            300.0,
            ViewportSizes::uniform(400.0, 300.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let outer = node_by_id(&dom, dom.document(), "outer").expect("outer");
        let float = node_by_id(&dom, dom.document(), "float").expect("float");
        let absolute = node_by_id(&dom, dom.document(), "absolute").expect("absolute");
        let outer = layout.get(outer).expect("outer fragment").physical_rect();
        let float = layout.get(float).expect("float fragment").physical_rect();
        let absolute = layout
            .get(absolute)
            .expect("absolute fragment")
            .physical_rect();

        assert_eq!(
            (float.x, float.y, float.width, float.height),
            (outer.x, outer.y, 96.0, 96.0)
        );
        assert_eq!((absolute.x, absolute.y), (outer.x + 96.0, outer.y));
    }

    #[test]
    fn live_rounded_shape_boxes_shift_left_and_right_line_edges() {
        fn by_id(
            dom: &StaticDocument,
            node: <StaticDocument as LayoutDom>::NodeId,
            expected: &str,
        ) -> Option<<StaticDocument as LayoutDom>::NodeId> {
            if dom.attributes(node).any(|attribute| {
                attribute.name.ns.as_ref().is_empty()
                    && attribute.name.local.as_ref() == "id"
                    && attribute.value == expected
            }) {
                return Some(node);
            }
            dom.dom_children(node)
                .find_map(|child| by_id(dom, child, expected))
        }

        let dom = StaticDocument::parse(
            "<html><body>\
             <div id=\"left\" class=\"host\"><div id=\"left-float\" class=\"shape left\"></div>\
             <div><span id=\"left-copy\">aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa</span></div></div>\
             <div id=\"right\" class=\"host right-host\"><div id=\"right-float\" class=\"shape right\"></div>\
             <div><span id=\"right-copy\">aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa</span></div></div>\
             </body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "html, body, div, span { margin: 0; padding: 0; border: 0; }\
                 .host { width: 200px; overflow-x: hidden; overflow-y: hidden;\
                         font-family: monospace; font-size: 10px; line-height: 20px; }\
                 .shape { width: 80px; height: 80px; shape-outside: border-box; border-radius: 50%; }\
                 .left { float: left; }\
                 .right { float: right; }\
                 .right-host { direction: rtl; text-align: right; }",
            ]),
            &Device::screen(320.0, 300.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            300.0,
            ViewportSizes::uniform(320.0, 300.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let rect = |id| {
            let node = by_id(&dom, dom.document(), id).expect(id);
            layout.get(node).expect(id).physical_rect()
        };
        let first_line = |id| {
            let node = by_id(&dom, dom.document(), id).expect(id);
            layout
                .fragments_for_node(node)
                .map(|fragment| fragment.physical_rect())
                .min_by(|left, right| left.y.total_cmp(&right.y))
                .expect(id)
        };

        let left = rect("left-float");
        let left_line = first_line("left-copy");
        assert!(
            left_line.x > left.x + 50.0 && left_line.x < left.x + 80.0,
            "a rounded left float releases its top-corner interval: host={left:?}, line={left_line:?}"
        );

        let right = rect("right-float");
        let right_line = first_line("right-copy");
        assert!(
            right_line.x + right_line.width > right.x + 5.0
                && right_line.x + right_line.width < right.x + 15.0,
            "a rounded right float releases its top-corner interval: float={right:?}, line={right_line:?}"
        );
        assert_eq!(layout.block_algorithm_counts().taffy, 0);
    }

    #[test]
    fn horizontal_direction_changes_keep_shape_constraints_for_atomic_lines() {
        let dom = StaticDocument::parse(
            r#"<html><body><div id=host><div id=shape></div>
             <div id=a class=box></div> <div id=b class=box></div>
             <div id=c class='box tall'></div> <div id=d class='box tall'></div>
             <div id=e class=box></div> <div id=f class=box></div></div></body></html>"#,
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body { margin: 0; }\
                 #host { direction: rtl; width: 200px; line-height: 0; }\
                 #shape { float: right; shape-outside: margin-box; border-radius: 50%;\
                          width: 20px; height: 20px; padding: 20px; border: 20px solid;\
                          margin: 10px; }\
                 .box { display: inline-block; width: 60px; height: 12px; }\
                 .tall { height: 36px; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let host = node_by_id(&dom, dom.document(), "host").expect("host");
        let host = layout.get(host).expect("host fragment").physical_rect();
        let actual = ["a", "b", "c", "d", "e", "f"].map(|id| {
            let node = node_by_id(&dom, dom.document(), id).unwrap_or_else(|| panic!("{id}"));
            let rect = layout
                .get(node)
                .unwrap_or_else(|| panic!("{id} fragment"))
                .physical_rect();
            (rect.x - host.x, rect.y - host.y, rect.height)
        });

        assert_eq!(
            actual,
            [
                (44.0, 0.0, 12.0),
                (32.0, 12.0, 12.0),
                (20.0, 24.0, 36.0),
                (20.0, 60.0, 36.0),
                (32.0, 96.0, 12.0),
                (44.0, 108.0, 12.0),
            ]
        );
        assert_eq!(layout.block_algorithm_counts().taffy, 0);
    }

    #[test]
    fn nonlinear_corner_radius_falls_back_only_when_shape_outside_consumes_it() {
        let dom = StaticDocument::parse(
            "<html><body><div id=\"paint\"></div><div id=\"shape\"></div></body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["#paint, #shape { width: 100px; height: 100px;\
                                  border-radius: min(10px, 50%); }\
                 #shape { float: left; shape-outside: border-box; }"]),
            &Device::screen(320.0, 300.0),
            &InteractionStates::default(),
        );
        let paint = node_by_id(&dom, dom.document(), "paint").expect("paint");
        let shape = node_by_id(&dom, dom.document(), "shape").expect("shape");
        let paint = styles.get(paint).expect("paint style");
        let shape = styles.get(shape).expect("shape style");

        assert!(length_has_math(shape.border_top_left_radius.0));
        assert!(!shape_outside_has_nonlinear_radius(paint));
        assert!(shape_outside_has_nonlinear_radius(shape));
        assert!(!block_style_has_nonlinear_lengths(paint));
        assert!(!block_style_has_nonlinear_lengths(shape));
    }

    #[test]
    fn live_nonlinear_shape_radius_retains_buckram_and_the_default_margin_area() {
        let dom = StaticDocument::parse(
            "<html><body><div id=\"host\"><div id=\"shape\"></div>\
             <div><span id=\"copy\">aa aa aa aa aa aa aa aa</span></div></div></body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "html, body, div, span { margin: 0; padding: 0; border: 0; }\
                 #host { width: 200px; overflow-x: hidden; overflow-y: hidden;\
                         font-family: monospace; font-size: 10px; line-height: 20px; }\
                 #shape { float: left; width: 80px; height: 80px; margin: 10px;\
                          shape-outside: border-box; border-radius: min(10px, 50%); }",
            ]),
            &Device::screen(320.0, 300.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            300.0,
            ViewportSizes::uniform(320.0, 300.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let shape = node_by_id(&dom, dom.document(), "shape").expect("shape");
        let copy = node_by_id(&dom, dom.document(), "copy").expect("copy");
        let shape = layout.get(shape).expect("shape layout").physical_rect();
        let line = layout
            .fragments_for_node(copy)
            .map(|fragment| fragment.physical_rect())
            .min_by(|left, right| left.y.total_cmp(&right.y))
            .expect("copy line");

        assert!((line.x - (shape.x + shape.width + 10.0)).abs() <= 0.01);
        assert_eq!(layout.block_algorithm_counts().taffy, 0);
    }

    #[test]
    fn live_unbreakable_line_retries_inside_a_rounded_bottom_contour() {
        let dom = StaticDocument::parse(
            "<html><body><div id=\"host\"><div id=\"shape\"></div>\
             <div><span id=\"copy\">aaaaaaaaaaaaaaaaaaaa</span></div></div></body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "html, body, div, span { margin: 0; padding: 0; border: 0; }\
                 #host { width: 200px; overflow-x: hidden; overflow-y: hidden;\
                         font-family: monospace; font-size: 10px; line-height: 20px; }\
                 #shape { float: left; width: 100px; height: 100px;\
                          shape-outside: border-box; border-radius: 0 0 50% 50%; }\
                 #copy { white-space: nowrap; }",
            ]),
            &Device::screen(320.0, 300.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            300.0,
            ViewportSizes::uniform(320.0, 300.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let shape = node_by_id(&dom, dom.document(), "shape").expect("shape");
        let copy = node_by_id(&dom, dom.document(), "copy").expect("copy");
        let shape = layout.get(shape).expect("shape layout").physical_rect();
        let line = layout
            .fragments_for_node(copy)
            .map(|fragment| fragment.physical_rect())
            .min_by(|left, right| left.y.total_cmp(&right.y))
            .expect("copy line");

        assert!(line.width > 100.0 && line.width < 150.0, "line={line:?}");
        assert!(
            line.y > shape.y + 50.0 && line.y < shape.y + shape.height,
            "the line should fit within the widening bottom contour: shape={shape:?}, line={line:?}"
        );
        assert_eq!(layout.block_algorithm_counts().taffy, 0);
    }

    #[test]
    fn live_nowrap_nested_inline_content_uses_float_bands_in_both_directions() {
        fn by_id(
            dom: &StaticDocument,
            node: <StaticDocument as LayoutDom>::NodeId,
            expected: &str,
        ) -> Option<<StaticDocument as LayoutDom>::NodeId> {
            if dom.attributes(node).any(|attribute| {
                attribute.name.ns.as_ref().is_empty()
                    && attribute.name.local.as_ref() == "id"
                    && attribute.value == expected
            }) {
                return Some(node);
            }
            dom.dom_children(node)
                .find_map(|child| by_id(dom, child, expected))
        }

        let dom = StaticDocument::parse(
            "<html><body>\
             <div id=\"ltr\" class=\"host\"><div class=\"float\"></div>\
             <span id=\"ltr-copy\"><span><span>aa aa aa aa</span></span></span></div>\
             <div id=\"rtl\" class=\"host\"><div class=\"float\"></div>\
             <span id=\"rtl-copy\"><span><span>aa aa aa aa</span></span></span></div>\
             </body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "html, body, div, span { margin: 0; padding: 0; border: 0; }\
                 .host { width: 200px; overflow-x: hidden; overflow-y: hidden;\
                         white-space: nowrap; font-family: monospace; font-size: 10px;\
                         line-height: 20px; }\
                 .float { float: left; width: 80px; height: 40px; }\
                 #rtl { direction: rtl; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let rect = |id| {
            let node = by_id(&dom, dom.document(), id).expect(id);
            layout.get(node).expect(id).physical_rect()
        };
        let copy_lines = |id| {
            let node = by_id(&dom, dom.document(), id).expect(id);
            layout
                .fragments_for_node(node)
                .map(|fragment| fragment.physical_rect())
                .collect::<Vec<_>>()
        };

        let ltr = rect("ltr");
        let rtl = rect("rtl");
        let ltr_lines = copy_lines("ltr-copy");
        let rtl_lines = copy_lines("rtl-copy");
        let algorithms = layout.block_algorithm_counts();

        assert_eq!(ltr_lines.len(), 1, "nowrap must remain one line");
        assert_eq!(rtl_lines.len(), 1, "nowrap must remain one line");
        assert_eq!((ltr_lines[0].x, ltr_lines[0].y), (ltr.x + 80.0, ltr.y));
        assert!(
            rtl_lines[0].x >= rtl.x + 80.0 - 0.5
                && rtl_lines[0].x + rtl_lines[0].width <= rtl.x + rtl.width + 0.5
                && (rtl_lines[0].y - rtl.y).abs() <= 0.5,
            "rtl host={rtl:?}, lines={rtl_lines:?}"
        );
        assert_eq!(algorithms.taffy, 0);
    }

    #[test]
    fn live_nested_float_state_crosses_ordinary_wrappers_but_stops_at_bfcs() {
        fn by_id(
            dom: &StaticDocument,
            node: <StaticDocument as LayoutDom>::NodeId,
            expected: &str,
        ) -> Option<<StaticDocument as LayoutDom>::NodeId> {
            if dom.attributes(node).any(|attribute| {
                attribute.name.ns.as_ref().is_empty()
                    && attribute.name.local.as_ref() == "id"
                    && attribute.value == expected
            }) {
                return Some(node);
            }
            dom.dom_children(node)
                .find_map(|child| by_id(dom, child, expected))
        }

        let dom = StaticDocument::parse(
            "<html><body>\
             <div id=\"shared\" class=\"host\"><div id=\"wrapper\"><div class=\"float\"></div></div>\
             <div id=\"shared-clear\" class=\"clear\"></div></div>\
             <div id=\"isolated\" class=\"host\"><div id=\"boundary\"><div class=\"float\"></div></div>\
             <div id=\"isolated-clear\" class=\"clear\"></div></div>\
             </body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; border: 0; }\
                 .host { width: 200px; overflow-x: hidden; overflow-y: hidden; }\
                 .float { float: left; width: 80px; height: 40px; }\
                 .clear { clear: left; height: 10px; }\
                 #boundary { display: flow-root; height: 0; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let rect = |id| {
            let node = by_id(&dom, dom.document(), id).expect(id);
            layout.get(node).expect(id).physical_rect()
        };

        let shared = rect("shared");
        let wrapper = rect("wrapper");
        let shared_clear = rect("shared-clear");
        let isolated = rect("isolated");
        let boundary = rect("boundary");
        let isolated_clear = rect("isolated-clear");
        let algorithms = layout.block_algorithm_counts();

        assert_eq!(wrapper.height, 0.0);
        assert_eq!(shared_clear.y - shared.y, 40.0);
        assert_eq!(shared.height, 50.0);
        assert_eq!(boundary.height, 0.0);
        assert_eq!(isolated_clear.y, isolated.y);
        assert_eq!(isolated.height, 10.0);
        assert_eq!(algorithms.taffy, 0);
    }

    #[test]
    fn live_generated_block_roots_translate_nested_float_state() {
        fn by_id(
            dom: &StaticDocument,
            node: <StaticDocument as LayoutDom>::NodeId,
            expected: &str,
        ) -> Option<<StaticDocument as LayoutDom>::NodeId> {
            if dom.attributes(node).any(|attribute| {
                attribute.name.ns.as_ref().is_empty()
                    && attribute.name.local.as_ref() == "id"
                    && attribute.value == expected
            }) {
                return Some(node);
            }
            dom.dom_children(node)
                .find_map(|child| by_id(dom, child, expected))
        }

        let dom = StaticDocument::parse(
            "<html><body><div id=\"host\"><div id=\"outer\"><div id=\"middle\">\
             <div id=\"float\"></div></div></div><div id=\"clear\"></div></div></body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; border: 0; }\
                 #host { width: 200px; overflow-x: hidden; overflow-y: hidden; }\
                 #outer { margin-top: 10px; }\
                 #middle { margin-top: 20px; padding-top: 5px; border-top: 3px solid; }\
                 #float { float: left; width: 80px; height: 40px; }\
                 #clear { clear: left; height: 10px; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let rect = |id| {
            let node = by_id(&dom, dom.document(), id).expect(id);
            layout.get(node).expect(id).physical_rect()
        };

        let host = rect("host");
        let outer = rect("outer");
        let middle = rect("middle");
        let float = rect("float");
        let clear = rect("clear");
        let algorithms = layout.block_algorithm_counts();

        assert_eq!((outer.y - host.y, middle.y - host.y), (20.0, 20.0));
        assert_eq!(float.y - host.y, 28.0);
        assert_eq!(clear.y - host.y, 68.0);
        assert_eq!(host.height, 78.0);
        assert_eq!(algorithms.taffy, 0);
    }

    #[test]
    fn livery_box_tree_preserves_split_inline_float_provenance() {
        fn by_id(
            dom: &StaticDocument,
            node: <StaticDocument as LayoutDom>::NodeId,
            expected: &str,
        ) -> Option<<StaticDocument as LayoutDom>::NodeId> {
            if dom.attributes(node).any(|attribute| {
                attribute.name.ns.as_ref().is_empty()
                    && attribute.name.local.as_ref() == "id"
                    && attribute.value == expected
            }) {
                return Some(node);
            }
            dom.dom_children(node)
                .find_map(|child| by_id(dom, child, expected))
        }

        let dom = StaticDocument::parse(
            "<html><body><div id=\"host\"><div id=\"wrapper\"><span id=\"split\">before\
             <span id=\"inline-float\"></span><span id=\"block\"></span>after</span></div>\
             <div id=\"clear\"></div></div></body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "html, body, div, span { margin: 0; padding: 0; border: 0; }\
                 #host { width: 200px; overflow-x: hidden; overflow-y: hidden; }\
                 #inline-float { float: left; width: 80px; height: 40px; }\
                 #block { display: block; height: 0; }\
                 #clear { clear: left; height: 10px; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let split = by_id(&dom, dom.document(), "split").expect("split");
        let inline_float = by_id(&dom, dom.document(), "inline-float").expect("inline float");
        let boxes = GeneratedBoxTree::from_dom(&dom, &styles);
        let float_box = boxes.principal_box(inline_float).expect("float box");

        assert_eq!(boxes.boxes_for_node(split).len(), 2);
        assert_eq!(
            boxes[float_box].float_context,
            FloatContextProvenance::Inline
        );
    }

    #[test]
    fn live_block_bfcs_narrow_beside_a_float_or_move_below_it() {
        fn by_id(
            dom: &StaticDocument,
            node: <StaticDocument as LayoutDom>::NodeId,
            expected: &str,
        ) -> Option<<StaticDocument as LayoutDom>::NodeId> {
            if dom.attributes(node).any(|attribute| {
                attribute.name.ns.as_ref().is_empty()
                    && attribute.name.local.as_ref() == "id"
                    && attribute.value == expected
            }) {
                return Some(node);
            }
            dom.dom_children(node)
                .find_map(|child| by_id(dom, child, expected))
        }

        let dom = StaticDocument::parse(
            "<html><body><div id=\"host\"><div id=\"float\"></div>\
             <div id=\"adjacent\"></div><div id=\"lowered\"></div>\
             </div></body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; border: 0; }\
                 #host { width: 200px; overflow-x: hidden; overflow-y: hidden; }\
                 #float { float: left; width: 80px; height: 40px; }\
                 #adjacent { height: 20px; overflow-x: hidden; overflow-y: hidden; }\
                 #lowered { width: 150px; height: 20px;\
                            overflow-x: hidden; overflow-y: hidden; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let rect = |id| {
            let node = by_id(&dom, dom.document(), id).expect(id);
            layout.get(node).expect(id).physical_rect()
        };

        let host = rect("host");
        let adjacent = rect("adjacent");
        let lowered = rect("lowered");
        let algorithms = layout.block_algorithm_counts();

        assert_eq!(
            (adjacent.x, adjacent.y, adjacent.width, adjacent.height),
            (host.x + 80.0, host.y, 120.0, 20.0)
        );
        assert_eq!(
            (lowered.x, lowered.y, lowered.width, lowered.height),
            (host.x, host.y + 40.0, 150.0, 20.0)
        );
        assert_eq!(host.height, 60.0);
        assert_eq!(algorithms.taffy, 0);
    }

    #[test]
    fn live_bfc_auto_margins_fit_or_move_below_floats_in_both_directions() {
        fn by_id(
            dom: &StaticDocument,
            node: <StaticDocument as LayoutDom>::NodeId,
            expected: &str,
        ) -> Option<<StaticDocument as LayoutDom>::NodeId> {
            if dom.attributes(node).any(|attribute| {
                attribute.name.ns.as_ref().is_empty()
                    && attribute.name.local.as_ref() == "id"
                    && attribute.value == expected
            }) {
                return Some(node);
            }
            dom.dom_children(node)
                .find_map(|child| by_id(dom, child, expected))
        }

        let dom = StaticDocument::parse(
            "<html><body>\
             <div id=\"ltr\" class=\"host\"><div class=\"right-float\"></div>\
             <div id=\"ltr-bfc\" class=\"bfc\"></div></div>\
             <div id=\"rtl\" class=\"host\"><div class=\"left-float\"></div>\
             <div id=\"rtl-bfc\" class=\"bfc\"></div></div>\
             <div id=\"lowered\" class=\"host\"><div class=\"right-float\"></div>\
             <div id=\"lowered-bfc\" class=\"bfc\"></div></div>\
             </body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; border: 0; }\
                 .host { width: 100px; overflow-x: hidden; overflow-y: hidden; }\
                 .right-float { float: right; width: 50px; height: 40px; }\
                 .left-float { float: left; width: 50px; height: 40px; }\
                 .bfc { display: flow-root; width: 30px; height: 20px; }\
                 #ltr-bfc { margin-left: auto; }\
                 #rtl { direction: rtl; } #rtl-bfc { margin-right: auto; }\
                 #lowered-bfc { width: 60px; margin-left: auto; margin-right: 10px; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let rect = |id| {
            let node = by_id(&dom, dom.document(), id).expect(id);
            layout.get(node).expect(id).physical_rect()
        };

        let ltr = rect("ltr");
        let ltr_bfc = rect("ltr-bfc");
        let rtl = rect("rtl");
        let rtl_bfc = rect("rtl-bfc");
        let lowered = rect("lowered");
        let lowered_bfc = rect("lowered-bfc");
        let algorithms = layout.block_algorithm_counts();

        assert_eq!(
            (
                ltr_bfc.x - ltr.x,
                ltr_bfc.y - ltr.y,
                ltr_bfc.width,
                ltr_bfc.height,
            ),
            (20.0, 0.0, 30.0, 20.0),
            "ltr={ltr:?}, ltr_bfc={ltr_bfc:?}, rtl={rtl:?}, rtl_bfc={rtl_bfc:?}, lowered={lowered:?}, lowered_bfc={lowered_bfc:?}, algorithms={algorithms:?}"
        );
        assert_eq!(
            (
                rtl_bfc.x - rtl.x,
                rtl_bfc.y - rtl.y,
                rtl_bfc.width,
                rtl_bfc.height,
            ),
            (50.0, 0.0, 30.0, 20.0)
        );
        assert_eq!(
            (
                lowered_bfc.x - lowered.x,
                lowered_bfc.y - lowered.y,
                lowered_bfc.width,
                lowered_bfc.height,
            ),
            (30.0, 40.0, 60.0, 20.0)
        );
        assert_eq!((ltr.height, rtl.height, lowered.height), (40.0, 40.0, 60.0));
        assert_eq!(algorithms.taffy, 0);
    }

    #[test]
    fn live_flex_and_grid_bfcs_use_buckram_float_placement() {
        fn by_id(
            dom: &StaticDocument,
            node: <StaticDocument as LayoutDom>::NodeId,
            expected: &str,
        ) -> Option<<StaticDocument as LayoutDom>::NodeId> {
            if dom.attributes(node).any(|attribute| {
                attribute.name.ns.as_ref().is_empty()
                    && attribute.name.local.as_ref() == "id"
                    && attribute.value == expected
            }) {
                return Some(node);
            }
            dom.dom_children(node)
                .find_map(|child| by_id(dom, child, expected))
        }

        let dom = StaticDocument::parse(
            "<html><body><div id=\"host\"><div id=\"float\"></div>\
             <div id=\"flex\"><div id=\"flex-child\"></div></div>\
             <div id=\"grid\"><div id=\"grid-child\"></div></div>\
             </div></body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; border: 0; }\
                 #host { width: 100px; overflow-x: hidden; overflow-y: hidden; }\
                 #float { float: left; width: 40px; height: 40px; }\
                 #flex { display: flex; height: 20px; }\
                 #flex-child { width: 20px; height: 10px; }\
                 #grid { display: grid; grid-template-columns: 20px; width: 70px; height: 20px; }\
                 #grid-child { width: 20px; height: 10px; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let rect = |id| {
            let node = by_id(&dom, dom.document(), id).expect(id);
            layout.get(node).expect(id).physical_rect()
        };

        let host = rect("host");
        let flex = rect("flex");
        let flex_child = rect("flex-child");
        let grid = rect("grid");
        let grid_child = rect("grid-child");
        let algorithms = layout.block_algorithm_counts();

        assert_eq!(
            (
                flex.x - host.x,
                flex.y - host.y,
                flex.width,
                flex.height,
                flex_child.x - flex.x,
                flex_child.y - flex.y,
            ),
            (40.0, 0.0, 60.0, 20.0, 0.0, 0.0),
            "host={host:?}, flex={flex:?}, flex_child={flex_child:?}, grid={grid:?}, grid_child={grid_child:?}, algorithms={algorithms:?}"
        );
        assert_eq!(
            (
                grid.x - host.x,
                grid.y - host.y,
                grid.width,
                grid.height,
                grid_child.x - grid.x,
                grid_child.y - grid.y,
            ),
            (0.0, 40.0, 70.0, 20.0, 0.0, 0.0)
        );
        assert_eq!(host.height, 60.0);
        assert!(algorithms.buckram > 0);
        assert_eq!(algorithms.taffy, 0);
    }

    #[test]
    fn live_auto_float_width_clamps_retained_inline_intrinsics() {
        fn by_id(
            dom: &StaticDocument,
            node: <StaticDocument as LayoutDom>::NodeId,
            expected: &str,
        ) -> Option<<StaticDocument as LayoutDom>::NodeId> {
            if dom.attributes(node).any(|attribute| {
                attribute.name.ns.as_ref().is_empty()
                    && attribute.name.local.as_ref() == "id"
                    && attribute.value == expected
            }) {
                return Some(node);
            }
            dom.dom_children(node)
                .find_map(|child| by_id(dom, child, expected))
        }

        let dom = StaticDocument::parse(
            "<html><body>\
             <div id=\"narrow\" class=\"host\"><span id=\"narrow-float\" class=\"float\">\
             aaaa aaaa aaaa aaaa</span><div class=\"clear\"></div></div>\
             <div id=\"wide\" class=\"host\"><span id=\"wide-float\" class=\"float\">\
             aaaa aaaa aaaa aaaa</span><div class=\"clear\"></div></div>\
             </body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "html, body, div, span { margin: 0; padding: 0; border: 0; }\
                 .host { overflow-x: hidden; overflow-y: hidden; }\
                 #narrow { width: 80px; } #wide { width: 200px; }\
                 .float { float: left; font-family: monospace; font-size: 10px;\
                          line-height: 20px; }\
                 .clear { clear: both; height: 1px; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let rect = |id| {
            let node = by_id(&dom, dom.document(), id).expect(id);
            layout.get(node).expect(id).physical_rect()
        };

        let narrow_host = rect("narrow");
        let narrow_float = rect("narrow-float");
        let wide_host = rect("wide");
        let wide_float = rect("wide-float");
        let algorithms = layout.block_algorithm_counts();

        assert!((narrow_float.width - narrow_host.width).abs() <= 0.5);
        assert!(
            wide_float.width > narrow_float.width + 10.0
                && wide_float.width < wide_host.width - 10.0,
            "narrow={narrow_float:?}, wide={wide_float:?}"
        );
        assert!(narrow_float.height > wide_float.height);
        assert_eq!(algorithms.taffy, 0);
    }

    #[test]
    fn live_multi_child_float_and_atomic_inline_use_intrinsic_subtrees() {
        fn by_id(
            dom: &StaticDocument,
            node: <StaticDocument as LayoutDom>::NodeId,
            expected: &str,
        ) -> Option<<StaticDocument as LayoutDom>::NodeId> {
            if dom.attributes(node).any(|attribute| {
                attribute.name.ns.as_ref().is_empty()
                    && attribute.name.local.as_ref() == "id"
                    && attribute.value == expected
            }) {
                return Some(node);
            }
            dom.dom_children(node)
                .find_map(|child| by_id(dom, child, expected))
        }

        let dom = StaticDocument::parse(
            "<html><body>\
             <div id=\"narrow\" class=\"host\"><div id=\"narrow-float\" class=\"float\">\
             <div>aaaa aaaa aaaa aaaa</div><div>aaaa aaaa aaaa aaaa</div></div>\
             <div class=\"clear\"></div></div>\
             <div id=\"wide\" class=\"host\"><div id=\"wide-float\" class=\"float\">\
             <div>aaaa aaaa aaaa aaaa</div><div>aaaa aaaa aaaa aaaa</div></div>\
             <div class=\"clear\"></div></div>\
             </body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; border: 0; }\
                 .host { overflow-x: hidden; overflow-y: hidden; }\
                 #narrow { width: 80px; } #wide { width: 200px; }\
                 .float { float: left; font-family: monospace; font-size: 10px;\
                          line-height: 20px; }\
                 .clear { clear: both; height: 1px; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let rect = |id| {
            let node = by_id(&dom, dom.document(), id).expect(id);
            layout.get(node).expect(id).physical_rect()
        };

        let narrow_host = rect("narrow");
        let narrow_float = rect("narrow-float");
        let wide_host = rect("wide");
        let wide_float = rect("wide-float");

        assert!((narrow_float.width - narrow_host.width).abs() <= 0.5);
        assert!(
            wide_float.width > narrow_float.width + 10.0
                && wide_float.width < wide_host.width - 10.0,
            "narrow={narrow_float:?}, wide={wide_float:?}"
        );
        assert!(narrow_float.height > wide_float.height);
        assert_eq!(layout.block_algorithm_counts().taffy, 0);

        fn atomic_inline_width(viewport_width: f32) -> f32 {
            let dom = StaticDocument::parse(
                "<html><body><span id=\"atomic\">aaaa aaaa aaaa aaaa</span></body></html>",
            );
            let styles = resolve_styles(
                &dom,
                &StyleSet::cambium(&["html, body, span { margin: 0; padding: 0; border: 0; }\
                     span { display: inline-block; font-family: monospace; font-size: 10px;\
                            line-height: 20px; }"]),
                &Device::screen(viewport_width, 240.0),
                &InteractionStates::default(),
            );
            let mut text = TextSystem::new();
            let (_, layout) = layout_with_text_system(
                &dom,
                &styles,
                viewport_width,
                240.0,
                ViewportSizes::uniform(viewport_width, 240.0),
                &mut text,
                &HashMap::new(),
            )
            .expect("atomic inline layout");
            let atomic = by_id(&dom, dom.document(), "atomic").expect("atomic node");
            layout
                .get(atomic)
                .expect("atomic fragment")
                .physical_rect()
                .width
        }

        assert_eq!(atomic_inline_width(30.0), 30.0);
        assert_eq!(atomic_inline_width(80.0), 80.0);
        assert!((atomic_inline_width(200.0) - 104.462_89).abs() <= 0.01);
    }

    #[test]
    fn live_bfc_fragments_expose_text_flex_grid_and_atomic_baselines() {
        fn by_id(
            dom: &StaticDocument,
            node: <StaticDocument as LayoutDom>::NodeId,
            expected: &str,
        ) -> Option<<StaticDocument as LayoutDom>::NodeId> {
            if dom.attributes(node).any(|attribute| {
                attribute.name.ns.as_ref().is_empty()
                    && attribute.name.local.as_ref() == "id"
                    && attribute.value == expected
            }) {
                return Some(node);
            }
            dom.dom_children(node)
                .find_map(|child| by_id(dom, child, expected))
        }

        let dom = StaticDocument::parse(
            "<html><body><div id=\"host\"><span id=\"text\">text</span>\
             <span id=\"atomic\"></span><div id=\"flex\"><span id=\"flex-text\">flex</span></div>\
             <div id=\"grid\"><span id=\"grid-text\">grid</span></div></div></body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&[
                "html, body, div, span { margin: 0; padding: 0; border: 0; }\
                 #host { width: 160px; font-family: monospace; font-size: 10px; line-height: 20px; }\
                 #atomic { display: inline-block; width: 20px; height: 12px; }\
                 #flex { display: flex; width: 80px; }\
                 #grid { display: grid; width: 80px; grid-template-columns: 1fr; }",
            ]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("layout");
        let fragment = |id| {
            layout
                .get(by_id(&dom, dom.document(), id).expect(id))
                .expect(id)
        };
        let host = fragment("host");
        let text = fragment("text");
        let atomic = fragment("atomic");
        let flex = fragment("flex");
        let grid = fragment("grid");

        for (name, fragment) in [
            ("host", host),
            ("text", text),
            ("atomic", atomic),
            ("flex", flex),
            ("grid", grid),
        ] {
            assert!(
                fragment.baselines.first.is_some() && fragment.baselines.last.is_some(),
                "{name} must expose modeled first and last baselines"
            );
        }
        assert_eq!(
            atomic.baselines,
            Baselines::synthesized_from_block_end(atomic.physical_rect().height),
            "an admitted atomic context keeps its own block-end fallback"
        );
        assert!(
            host.baselines.first.expect("host first baseline") < host.logical_rect.block_size,
            "the independent host keeps its IFC first baseline instead of its block-end fallback"
        );
        assert!(
            host.baselines.first.expect("host first baseline")
                >= text.baselines.first.expect("text baseline"),
            "the host baseline must retain the text IFC contribution"
        );
        assert_eq!(
            flex.baselines,
            Baselines::synthesized_from_block_end(flex.physical_rect().height),
            "the admitted flex BFC returns its own empty-line fallback"
        );
        assert_eq!(
            grid.baselines,
            Baselines::synthesized_from_block_end(grid.physical_rect().height),
            "the admitted grid BFC returns its own empty-line fallback"
        );
        assert_eq!(
            host.baselines.last,
            grid.baselines
                .last
                .map(|baseline| { grid.physical_rect().y - host.physical_rect().y + baseline }),
            "the independent host consumes its admitted grid BFC output"
        );
    }

    #[test]
    fn live_flex_shorthands_reach_taffy_and_change_wrap_geometry() {
        let dom = StaticDocument::parse(
            "<html><body>\
             <div id=short><div id=s1 class=item></div><div id=s2 class=item></div><div id=s3 class=item></div></div>\
             <div id=explicit><div id=e1 class=item></div><div id=e2 class=item></div><div id=e3 class=item></div></div>\
             <div id=nowrap><div id=n1 class=item></div><div id=n2 class=item></div><div id=n3 class=item></div></div>\
             </body></html>",
        );
        let styles = resolve_styles(
            &dom,
            &StyleSet::cambium(&["html, body, div { margin: 0; padding: 0; border: 0; }\
                 #short, #explicit, #nowrap { display: flex; width: 100px; }\
                 #short { flex-flow: row wrap; }\
                 #explicit { flex-direction: row; flex-wrap: wrap; }\
                 #nowrap { flex-flow: row nowrap; }\
                 .item { flex: 1 1 40px; height: 20px; }"]),
            &Device::screen(320.0, 240.0),
            &InteractionStates::default(),
        );
        let short = node_by_id(&dom, dom.document(), "short").expect("short flex host");
        let explicit = node_by_id(&dom, dom.document(), "explicit").expect("explicit host");
        assert_eq!(
            styles.get(short).expect("short style").flex_direction,
            styles.get(explicit).expect("explicit style").flex_direction
        );
        assert_eq!(
            styles.get(short).expect("short style").flex_wrap,
            styles.get(explicit).expect("explicit style").flex_wrap
        );

        let mut text = TextSystem::new();
        let (_, layout) = layout_with_text_system(
            &dom,
            &styles,
            320.0,
            240.0,
            ViewportSizes::uniform(320.0, 240.0),
            &mut text,
            &HashMap::new(),
        )
        .expect("flex shorthand layout");
        let rect = |id| {
            layout
                .get(node_by_id(&dom, dom.document(), id).expect(id))
                .expect(id)
                .physical_rect()
        };
        let short_host = rect("short");
        let explicit_host = rect("explicit");
        let s1 = rect("s1");
        let s2 = rect("s2");
        let s3 = rect("s3");
        let e1 = rect("e1");
        let e2 = rect("e2");
        let e3 = rect("e3");
        let n1 = rect("n1");
        let n2 = rect("n2");
        let n3 = rect("n3");

        assert!((s1.width - 50.0).abs() <= 0.01, "s1={s1:?}");
        assert!((s2.x - s1.x - s1.width).abs() <= 0.01, "s1={s1:?}, s2={s2:?}");
        assert!((s3.y - s1.y - 20.0).abs() <= 0.01, "s1={s1:?}, s3={s3:?}");
        assert!((s3.width - 100.0).abs() <= 0.01, "s3={s3:?}");
        assert_eq!((s1.width, s1.height), (e1.width, e1.height));
        assert!((s2.width - e2.width).abs() <= 0.01);
        assert!((s2.y - short_host.y - (e2.y - explicit_host.y)).abs() <= 0.01);
        assert!((s3.width - e3.width).abs() <= 0.01);
        assert!((s3.y - short_host.y - (e3.y - explicit_host.y)).abs() <= 0.01);
        assert_eq!(n1.y, n2.y);
        assert_eq!(n2.y, n3.y);
        assert!(
            n3.x > n2.x,
            "nowrap geometry did not stay on one row: {n1:?}, {n2:?}, {n3:?}"
        );
    }
}
