//! Caller-owned scratch tree for Taffy's low-level layout algorithms.
//!
//! Taffy supplies block, flex, and grid algorithms. Buckram owns the tree,
//! source identity, contexts, and returned placements. The public surface in
//! this module deliberately uses Buckram types rather than Taffy types.

use std::{marker::PhantomData, slice};

use taffy::{
    BlockContext, Cache, CacheTree, DetailedGridInfo, Layout, LayoutBlockContainer,
    LayoutFlexboxContainer, LayoutGridContainer, LayoutInput, LayoutOutput, LayoutPartialTree,
    NodeId, RoundTree, RunMode, SizingMode, Style, TraversePartialTree, TraverseTree,
    compute_block_layout, compute_cached_layout, compute_flexbox_layout, compute_grid_layout,
    compute_hidden_layout, compute_leaf_layout, compute_root_layout, round_layout,
};

use crate::block::FloatContextState;
use crate::block::solve_in_flow_inline_size_for_border_box;
use crate::{
    Baselines, BlockBoxSizing, BlockContainingBlock, BlockDeferral, BlockFormattingContext,
    BlockMarginState, BlockSizeValue, BlockStyle, ClearSide, CollapsedMargin, FloatLineConstraints,
    FloatSide, FlowAxes, FlowLength, FlowLengthAuto, IntrinsicSizeKind, IntrinsicSizes,
    LogicalRect, LogicalSides, LogicalSize, PhysicalRect, PhysicalSides, PhysicalSize,
    solve_float_inline_size, solve_in_flow_inline_size, solve_shrink_to_fit_inline_size,
};

/// Formatting role selected by Buckram before entering a backend algorithm.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlgorithmKind {
    Hidden,
    Leaf,
    Block,
    Flex,
    Grid,
    /// A Buckram table formatting context. K4h selects it for every table
    /// grid before layout; K4d6 supplies final geometry when the table
    /// pipeline accepts its sizing input.
    Table,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockAlgorithm {
    Buckram,
    Taffy,
}

/// Stable identity within one scratch algorithm tree.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AlgorithmNodeId(u32);

impl AlgorithmNodeId {
    pub fn index(self) -> usize {
        self.0 as usize
    }

    fn from_taffy(id: NodeId) -> Self {
        Self(
            usize::from(id)
                .try_into()
                .expect("a Taffy node id exceeded u32::MAX"),
        )
    }

    fn into_taffy(self) -> NodeId {
        NodeId::from(self.index())
    }
}

/// Width and height pair at the algorithm boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AlgorithmSize<T> {
    pub width: T,
    pub height: T,
}

impl<T> AlgorithmSize<T> {
    pub const fn new(width: T, height: T) -> Self {
        Self { width, height }
    }
}

/// Available-space constraint without exposing the backend enum.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AlgorithmAvailableSpace {
    Definite(f32),
    MinContent,
    MaxContent,
}

/// Parent-relative placement returned by the algorithm backend.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AlgorithmLayout {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

mod sealed {
    pub trait AlgorithmStyle {
        fn as_taffy_style(&self) -> &taffy::Style;
        fn as_taffy_style_mut(&mut self) -> &mut taffy::Style;
    }

    impl AlgorithmStyle for taffy::Style {
        fn as_taffy_style(&self) -> &taffy::Style {
            self
        }

        fn as_taffy_style_mut(&mut self) -> &mut taffy::Style {
            self
        }
    }
}

/// A style accepted by the private Taffy algorithm adapter.
///
/// This sealed trait keeps Taffy out of Buckram's method signatures while the
/// K1 caller still constructs the backend style privately. K3 replaces that
/// caller-side lowering with Buckram-owned logical and intrinsic inputs.
pub trait AlgorithmStyle: sealed::AlgorithmStyle {}

impl AlgorithmStyle for Style {}

struct AlgorithmNode<S, Context, Source> {
    kind: AlgorithmKind,
    block_style: BlockStyle,
    block_algorithm: Option<BlockAlgorithm>,
    block_deferral: Option<BlockDeferral>,
    block_margins: Option<BlockMarginState>,
    exported_float_state: Option<FloatContextState>,
    nested_float_state_enabled: bool,
    inline_context_float: bool,
    float_line_constraints_enabled: bool,
    float_avoidance_enabled: bool,
    intrinsic_shrink_to_fit_enabled: bool,
    intrinsic_inline_sizes: Option<IntrinsicSizes>,
    table_wrapper_inline_size: Option<f32>,
    style: S,
    context: Option<Context>,
    source: Source,
    parent: Option<AlgorithmNodeId>,
    children: Vec<AlgorithmNodeId>,
    cache: Cache,
    unrounded_layout: Layout,
    final_layout: Layout,
    grid_info: Option<DetailedGridInfo>,
    /// CSS Grid §9.2: a direct absolute or fixed child's static position is
    /// aligned in the grid container's content box unless that grid also
    /// generates the child's containing block, in which case the §9.1 grid
    /// area applies. The renderer sets this from the K5a box graph; the
    /// adapter never derives it from backend positioning.
    grid_static_position_uses_grid_area: bool,
    baselines: Baselines,
}

/// A caller-owned arena used only while running layout algorithms.
///
/// Source identity lives on each node, so callers need no parallel
/// `NodeId -> source` map. The style parameter is generic and the public
/// methods expose only Buckram identifiers and geometry.
pub struct AlgorithmTree<S, Context, Source> {
    nodes: Vec<AlgorithmNode<S, Context, Source>>,
}

impl<S, Context, Source> Default for AlgorithmTree<S, Context, Source> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S, Context, Source> AlgorithmTree<S, Context, Source> {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    pub fn new_with_children(
        &mut self,
        kind: AlgorithmKind,
        style: S,
        children: &[AlgorithmNodeId],
        source: Source,
    ) -> AlgorithmNodeId {
        self.new_with_children_and_block_style(kind, BlockStyle::default(), style, children, source)
    }

    pub fn new_with_children_and_block_style(
        &mut self,
        kind: AlgorithmKind,
        block_style: BlockStyle,
        style: S,
        children: &[AlgorithmNodeId],
        source: Source,
    ) -> AlgorithmNodeId {
        self.push(kind, block_style, style, children, None, source)
    }

    pub fn new_leaf_with_context(
        &mut self,
        style: S,
        context: Context,
        source: Source,
    ) -> AlgorithmNodeId {
        self.new_leaf_with_context_and_block_style(BlockStyle::default(), style, context, source)
    }

    pub fn new_leaf_with_context_and_block_style(
        &mut self,
        block_style: BlockStyle,
        style: S,
        context: Context,
        source: Source,
    ) -> AlgorithmNodeId {
        self.push(
            AlgorithmKind::Leaf,
            block_style,
            style,
            &[],
            Some(context),
            source,
        )
    }

    fn push(
        &mut self,
        kind: AlgorithmKind,
        block_style: BlockStyle,
        style: S,
        children: &[AlgorithmNodeId],
        context: Option<Context>,
        source: Source,
    ) -> AlgorithmNodeId {
        let id = AlgorithmNodeId(
            self.nodes
                .len()
                .try_into()
                .expect("an algorithm tree exceeded u32::MAX nodes"),
        );
        self.nodes.push(AlgorithmNode {
            kind,
            block_style,
            block_algorithm: None,
            block_deferral: None,
            block_margins: None,
            exported_float_state: None,
            nested_float_state_enabled: false,
            inline_context_float: false,
            float_line_constraints_enabled: false,
            float_avoidance_enabled: false,
            intrinsic_shrink_to_fit_enabled: false,
            intrinsic_inline_sizes: None,
            table_wrapper_inline_size: None,
            style,
            context,
            source,
            parent: None,
            children: children.to_vec(),
            cache: Cache::new(),
            unrounded_layout: Layout::new(),
            final_layout: Layout::new(),
            grid_info: None,
            grid_static_position_uses_grid_area: false,
            baselines: Baselines::default(),
        });
        for child in children {
            let previous = self.nodes[child.index()].parent.replace(id);
            assert!(
                previous.is_none(),
                "an algorithm scratch node cannot have two parents"
            );
        }
        id
    }

    pub fn source(&self, id: AlgorithmNodeId) -> &Source {
        &self.nodes[id.index()].source
    }

    pub fn kind(&self, id: AlgorithmNodeId) -> AlgorithmKind {
        self.nodes[id.index()].kind
    }

    pub fn style(&self, id: AlgorithmNodeId) -> &S {
        &self.nodes[id.index()].style
    }

    pub fn block_style(&self, id: AlgorithmNodeId) -> BlockStyle {
        self.nodes[id.index()].block_style
    }

    pub fn block_algorithm(&self, id: AlgorithmNodeId) -> Option<BlockAlgorithm> {
        self.nodes[id.index()].block_algorithm
    }

    /// The named Buckram boundary that selected Taffy's block algorithm.
    ///
    /// `None` means this node either used Buckram's block algorithm or is not
    /// a block-algorithm node. Taffy descendants reached while an ancestor's
    /// fallback is already active report `BackendSizingMode`; their ancestor
    /// retains the CSS-facing reason.
    pub fn block_deferral(&self, id: AlgorithmNodeId) -> Option<BlockDeferral> {
        self.nodes[id.index()].block_deferral
    }

    pub fn block_margins(&self, id: AlgorithmNodeId) -> Option<BlockMarginState> {
        self.nodes[id.index()].block_margins
    }

    pub fn block_algorithm_counts(&self) -> (usize, usize) {
        self.nodes.iter().fold((0, 0), |(buckram, taffy), node| {
            match node.block_algorithm {
                Some(BlockAlgorithm::Buckram) => (buckram + 1, taffy),
                Some(BlockAlgorithm::Taffy) => (buckram, taffy + 1),
                None => (buckram, taffy),
            }
        })
    }

    /// Count Taffy block runs by their named Buckram deferral.
    ///
    /// This keeps CSS-facing fallback distinct from scratch measurement in
    /// [`BlockDeferral::BackendSizingMode`]. A table cell's intrinsic pass may
    /// use the latter without causing any block ancestor to abandon Buckram.
    pub fn block_deferral_count(&self, deferral: BlockDeferral) -> usize {
        self.nodes
            .iter()
            .filter(|node| {
                node.block_algorithm == Some(BlockAlgorithm::Taffy)
                    && node.block_deferral == Some(deferral)
            })
            .count()
    }

    pub fn style_mut(&mut self, id: AlgorithmNodeId) -> &mut S {
        &mut self.nodes[id.index()].style
    }

    /// Publish a table algorithm's used grid width to its generated wrapper.
    ///
    /// Backend root setup may offer the containing width as a provisional
    /// known dimension. The table grid is the standards-owned authority for
    /// its wrapper's border-edge width, so this explicit carrier outranks that
    /// provisional value when Buckram admits the wrapper as a block child.
    pub fn set_table_wrapper_inline_size(&mut self, id: AlgorithmNodeId, size: f32) {
        assert!(
            size.is_finite() && size >= 0.0,
            "a table wrapper inline size must be finite and non-negative"
        );
        let node = &mut self.nodes[id.index()];
        node.table_wrapper_inline_size = Some(size);
        let size = BlockSizeValue::Length(FlowLength::px(size));
        if node.block_style.flow.is_horizontal() {
            node.block_style.size.width = size;
        } else {
            node.block_style.size.height = size;
        }
        node.cache.clear();
    }

    /// Admit this direct flex/grid child to the renderer's static-position
    /// provider. Buckram selects the narrow participation boundary and keeps
    /// the resulting rectangle as the K5b formatter output; callers never
    /// lower a CSS positioning role into the backend style themselves.
    ///
    /// The provider is deliberately unavailable outside a flex or grid
    /// parent. Ordinary block, inline, and table-internal out-of-flow routes
    /// have separate Buckram formatting boundaries.
    pub fn enable_flex_grid_static_position_provider(&mut self, id: AlgorithmNodeId)
    where
        S: AlgorithmStyle,
    {
        let parent = self.nodes[id.index()]
            .parent
            .expect("a flex/grid static-position provider requires an attached direct child");
        assert!(
            matches!(
                self.nodes[parent.index()].kind,
                AlgorithmKind::Flex | AlgorithmKind::Grid
            ),
            "the static-position provider belongs only to a flex or grid parent"
        );
        assert!(
            matches!(
                self.nodes[id.index()].block_style.position,
                crate::BlockPosition::Absolute | crate::BlockPosition::Fixed
            ),
            "the static-position provider requires an absolute or fixed child"
        );
        sealed::AlgorithmStyle::as_taffy_style_mut(&mut self.nodes[id.index()].style).position =
            taffy::Position::Absolute;
    }

    /// Select this direct grid child's finalized grid area as its
    /// static-position alignment container (CSS Grid §9.2, second sentence).
    /// Callers make this selection only when the K5a containing-block graph
    /// chose this same grid as the child's containing block; otherwise the
    /// provider keeps the grid's content box. With `auto` grid lines the area
    /// is bounded by the grid's padding edges, as §9.1 requires.
    pub fn use_grid_area_for_static_position(&mut self, id: AlgorithmNodeId) {
        let parent = self.nodes[id.index()]
            .parent
            .expect("a grid static-position area requires an attached direct child");
        assert_eq!(
            self.nodes[parent.index()].kind,
            AlgorithmKind::Grid,
            "only a direct grid child can use a grid-area static-position rectangle"
        );
        assert!(
            matches!(
                self.nodes[id.index()].block_style.position,
                crate::BlockPosition::Absolute | crate::BlockPosition::Fixed
            ),
            "the grid-area static-position rectangle requires an absolute or fixed child"
        );
        self.nodes[id.index()].grid_static_position_uses_grid_area = true;
    }

    /// Supply the resolved CSS inline size to a detached absolute/fixed
    /// formatting root before its second formatting pass. The positioned
    /// solver owns the value; this scratch-tree setter merely gives the local
    /// formatter the same constraint without asking Taffy to select a
    /// containing block or out-of-flow participation.
    pub fn set_positioned_inline_size(&mut self, id: AlgorithmNodeId, size: f32) {
        assert!(
            size.is_finite() && size >= 0.0,
            "a positioned formatting inline size must be finite and non-negative"
        );
        let style = &mut self.nodes[id.index()].block_style;
        let size = BlockSizeValue::Length(FlowLength::px(size));
        if style.flow.is_horizontal() {
            style.size.width = size;
        } else {
            style.size.height = size;
        }
    }

    /// Supply a standards-resolved block size to a detached absolute/fixed
    /// formatting root before its constrained second formatting pass.
    pub fn set_positioned_block_size(&mut self, id: AlgorithmNodeId, size: f32) {
        assert!(
            size.is_finite() && size >= 0.0,
            "a positioned formatting block size must be finite and non-negative"
        );
        let style = &mut self.nodes[id.index()].block_style;
        let size = BlockSizeValue::Length(FlowLength::px(size));
        if style.flow.is_horizontal() {
            style.size.height = size;
        } else {
            style.size.width = size;
        }
    }

    /// Clear formatter state after a standards-owned used size changes a
    /// detached formatting root. The next tree walk must visit that root and
    /// its ancestors rather than reuse the pre-positioning layout cache.
    pub fn clear_layout_cache(&mut self) {
        for node in &mut self.nodes {
            node.cache.clear();
            node.intrinsic_inline_sizes = None;
        }
    }

    /// Admit this direct measured leaf to Buckram's float-aware line lane.
    ///
    /// Measured contexts opt in explicitly because a generic callback is not
    /// necessarily an inline formatter and may ignore float constraints.
    pub fn enable_float_line_constraints(&mut self, id: AlgorithmNodeId) {
        assert!(
            self.nodes[id.index()].context.is_some(),
            "only a measured leaf can consume float line constraints"
        );
        self.nodes[id.index()].float_line_constraints_enabled = true;
    }

    /// Admit this ordinary block as a continuation of its parent's BFC.
    ///
    /// Callers decide from the generated CSS box role, keeping backend display
    /// lookalikes and unresolved formatting-context boundaries outside this
    /// lane.
    pub fn enable_nested_float_state(&mut self, id: AlgorithmNodeId) {
        let node = &self.nodes[id.index()];
        assert_eq!(
            node.kind,
            AlgorithmKind::Block,
            "nested float state requires an ordinary block algorithm node"
        );
        assert!(
            !node.block_style.establishes_bfc,
            "an explicit BFC must not inherit its parent's float state"
        );
        self.nodes[id.index()].nested_float_state_enabled = true;
    }

    /// Preserve the generated-box fact that this blockified float originated
    /// inside an inline formatting context.
    pub fn mark_inline_context_float(&mut self, id: AlgorithmNodeId) {
        assert_ne!(
            self.nodes[id.index()].block_style.float,
            FloatSide::None,
            "only a floated box can carry inline-context float provenance"
        );
        self.nodes[id.index()].inline_context_float = true;
    }

    /// Admit this block-level independent formatting context to Buckram's
    /// float-avoidance policy.
    ///
    /// The caller opts in because a generic flex/grid backend or atomic inline
    /// box can establish a BFC while still requiring sizing and baseline work
    /// outside this lane.
    pub fn enable_float_avoidance(&mut self, id: AlgorithmNodeId) {
        let node = &mut self.nodes[id.index()];
        assert!(
            matches!(
                node.kind,
                AlgorithmKind::Leaf
                    | AlgorithmKind::Block
                    | AlgorithmKind::Flex
                    | AlgorithmKind::Grid
            ),
            "float avoidance accepts block, leaf, flex, and grid algorithms"
        );
        assert!(
            node.block_style.establishes_bfc && node.block_style.float == FloatSide::None,
            "float avoidance requires an in-flow BFC"
        );
        node.float_avoidance_enabled = true;
    }

    /// Whether this auto-width box has a complete intrinsic inline provider.
    ///
    /// The provider keeps block, inline, flex, and grid roles distinct. It
    /// rejects percentage and cyclic shapes instead of recovering a width from
    /// a completed backend layout.
    pub fn supports_intrinsic_shrink_to_fit(&self, id: AlgorithmNodeId) -> bool {
        let node = &self.nodes[id.index()];
        node.kind == AlgorithmKind::Block
            && node.block_style.shrink_to_fit
            && self.intrinsic_inline_subtree_is_admitted(id, true)
    }

    /// Admit an auto-width float or atomic inline box to Buckram's intrinsic
    /// shrink-to-fit lane.
    pub fn enable_intrinsic_shrink_to_fit(&mut self, id: AlgorithmNodeId) {
        let node = &self.nodes[id.index()];
        assert_eq!(
            node.kind,
            AlgorithmKind::Block,
            "intrinsic shrink-to-fit requires a block formatting context"
        );
        assert!(
            node.block_style.shrink_to_fit,
            "intrinsic shrink-to-fit requires an auto-width shrink-to-fit box"
        );
        assert!(
            self.supports_intrinsic_shrink_to_fit(id),
            "intrinsic shrink-to-fit requires an admitted in-flow subtree"
        );
        self.nodes[id.index()].intrinsic_shrink_to_fit_enabled = true;
    }

    /// Whether this node is using Buckram's admitted intrinsic query lane.
    pub fn uses_intrinsic_shrink_to_fit(&self, id: AlgorithmNodeId) -> bool {
        self.nodes[id.index()].intrinsic_shrink_to_fit_enabled
    }

    /// Query admitted min-content and max-content widths for an out-of-flow
    /// formatting root.
    ///
    /// The caller remains responsible for selecting the containing block and
    /// resolving the positioned inset equation. This temporarily removes the
    /// position deferral only for the intrinsic query, so a completed
    /// normal-flow rectangle never becomes the browser-facing auto width.
    pub fn positioned_intrinsic_inline_sizes<Measure>(
        &mut self,
        id: AlgorithmNodeId,
        measure: Measure,
    ) -> Option<IntrinsicSizes>
    where
        S: AlgorithmStyle,
        Measure: FnMut(
            AlgorithmSize<Option<f32>>,
            AlgorithmSize<AlgorithmAvailableSpace>,
            AlgorithmNodeId,
            Option<&mut Context>,
            Option<&FloatLineConstraints>,
        ) -> AlgorithmSize<f32>,
    {
        if !matches!(
            self.nodes[id.index()].kind,
            AlgorithmKind::Block | AlgorithmKind::Leaf
        ) || !matches!(
            self.nodes[id.index()].block_style.position,
            crate::BlockPosition::Absolute | crate::BlockPosition::Fixed
        ) {
            return None;
        }

        let original_style = self.nodes[id.index()].block_style;
        {
            let root_style = &mut self.nodes[id.index()].block_style;
            root_style.position = crate::BlockPosition::Static;
            // The intrinsic query measures the root's contents. K5d applies
            // the root's preferred ratio to those contributions afterwards.
            root_style.aspect_ratio = None;
        }
        if !self.intrinsic_inline_subtree_is_admitted(id, true) {
            self.nodes[id.index()].block_style = original_style;
            return None;
        }

        let mut run = AlgorithmRun {
            tree: self,
            measure,
            line_constraints: None,
            nested_float_state: None,
            resolved_shrink_to_fit: None,
            fixed_leaf_intrinsics_enabled: false,
            marker: PhantomData,
        };
        let result = run.measure_intrinsic_inline_subtree(id, None).ok();
        run.tree.nodes[id.index()].block_style = original_style;
        result
    }

    fn intrinsic_inline_subtree_is_admitted(&self, id: AlgorithmNodeId, is_root: bool) -> bool {
        let node = &self.nodes[id.index()];
        if !intrinsic_inline_style_is_admitted(node.block_style, is_root) {
            return false;
        }
        // The generic leaf measurement callback and the flex/grid intrinsic
        // query are still physical-width routes. Same-flow vertical block
        // trees without measured leaves have complete logical inline inputs,
        // so admit that narrower route without treating text or algorithms as
        // vertically intrinsic-capable.
        if !node.block_style.flow.is_horizontal()
            && (matches!(node.kind, AlgorithmKind::Flex | AlgorithmKind::Grid)
                || (matches!(node.kind, AlgorithmKind::Leaf) && node.context.is_some()))
        {
            return false;
        }
        match node.kind {
            AlgorithmKind::Hidden => true,
            // A context-free leaf represents an empty formatting root. Its
            // min-content and max-content contributions are both zero, so it
            // is safe to admit without asking a formatter callback to infer a
            // normal-flow fallback rectangle.
            AlgorithmKind::Leaf => true,
            AlgorithmKind::Block | AlgorithmKind::Flex | AlgorithmKind::Grid => node
                .children
                .iter()
                .copied()
                .all(|child| self.intrinsic_inline_subtree_is_admitted(child, false)),
            // K4d owns table intrinsic admission when live dispatch lands.
            AlgorithmKind::Table => false,
        }
    }

    pub fn children(&self, id: AlgorithmNodeId) -> &[AlgorithmNodeId] {
        &self.nodes[id.index()].children
    }

    pub fn context(&self, id: AlgorithmNodeId) -> Option<&Context> {
        self.nodes[id.index()].context.as_ref()
    }

    pub fn layout(&self, id: AlgorithmNodeId) -> AlgorithmLayout {
        let layout = self.nodes[id.index()].final_layout;
        AlgorithmLayout {
            x: layout.location.x,
            y: layout.location.y,
            width: layout.size.width,
            height: layout.size.height,
        }
    }

    /// The formatting coordinate assigned before CSS positioning applies
    /// insets. This carries a formatter output into Buckram's K5 positioning
    /// pass; it does not expose backend node identity or parent selection.
    pub fn static_layout(&self, id: AlgorithmNodeId) -> AlgorithmLayout {
        let layout = self.nodes[id.index()].final_layout;
        AlgorithmLayout {
            x: layout.static_location.x,
            y: layout.static_location.y,
            width: layout.size.width,
            height: layout.size.height,
        }
    }

    /// The grid area Taffy finalized for this direct absolutely positioned
    /// child before it applied the child's insets and self-alignment. Taffy
    /// reports its grid-column and grid-row axes as horizontal X/Y; project
    /// those track coordinates through the container flow before exposing the
    /// physical rectangle that the K5 route consumes.
    pub fn grid_positioned_area(&self, id: AlgorithmNodeId) -> Option<PhysicalRect> {
        let parent = self.nodes[id.index()].parent?;
        if self.nodes[parent.index()].kind != AlgorithmKind::Grid {
            return None;
        }
        let area = self.nodes[parent.index()]
            .grid_info
            .as_ref()?
            .positioned_items
            .iter()
            .find(|item| item.node == id.into_taffy())?
            .grid_area;
        let track_area = LogicalRect::from_horizontal_physical(PhysicalRect {
            x: area.left,
            y: area.top,
            width: (area.right - area.left).max(0.0),
            height: (area.bottom - area.top).max(0.0),
        });
        let container = &self.nodes[parent.index()];
        Some(container.block_style.flow.physical_rect(
            track_area,
            PhysicalSize {
                width: container.final_layout.size.width,
                height: container.final_layout.size.height,
            },
        ))
    }

    /// The rectangle an algorithm wrote, before the backend's rounding pass.
    ///
    /// An owned formatting context that adjusts its own subtree after writing
    /// it must read back the same store it wrote, or it would fold one
    /// rounding into the unrounded values and round twice.
    pub fn unrounded_layout(&self, id: AlgorithmNodeId) -> AlgorithmLayout {
        let layout = self.nodes[id.index()].unrounded_layout;
        AlgorithmLayout {
            x: layout.location.x,
            y: layout.location.y,
            width: layout.size.width,
            height: layout.size.height,
        }
    }

    /// Hand one node's dispatch to a Buckram algorithm after the table's
    /// block pipeline has written its final geometry. Table grids begin on
    /// the Buckram dispatcher too, so a named sizing deferral never revives a
    /// backend table route.
    pub fn set_kind(&mut self, id: AlgorithmNodeId, kind: AlgorithmKind) {
        self.nodes[id.index()].kind = kind;
    }

    /// Write a rectangle a Buckram algorithm already decided.
    ///
    /// A formatting context Buckram owns outright computes its own geometry
    /// and its children's before the backend walk begins; its node then
    /// reports that size and does not recurse, so nothing overwrites what was
    /// written here. Positions are relative to the node's parent, matching
    /// [`AlgorithmTree::layout`].
    ///
    /// The rectangle is written unrounded. The backend's rounding pass walks
    /// the whole tree from the root and derives every final layout from the
    /// unrounded one, so writing only the final layout would be discarded,
    /// and writing only a pre-rounded value would round this subtree on a
    /// different grid from its siblings.
    pub fn set_layout(&mut self, id: AlgorithmNodeId, layout: AlgorithmLayout) {
        assert!(
            [layout.x, layout.y, layout.width, layout.height]
                .into_iter()
                .all(f32::is_finite),
            "an owned formatting context must write a finite rectangle"
        );
        let node = &mut self.nodes[id.index()];
        node.unrounded_layout.location.x = layout.x;
        node.unrounded_layout.location.y = layout.y;
        node.unrounded_layout.size.width = layout.width;
        node.unrounded_layout.size.height = layout.height;
        node.final_layout = node.unrounded_layout;
    }

    /// First and last baseline outputs produced by this formatting context.
    /// They are logical offsets from the node's block-start edge.
    pub fn baselines(&self, id: AlgorithmNodeId) -> Baselines {
        self.nodes[id.index()].baselines
    }

    /// Replace a direct formatting-context baseline result before parent
    /// contexts consume it. The adapter stores the modeled output and never
    /// asks Taffy to rediscover a descendant baseline later.
    pub fn set_baselines(&mut self, id: AlgorithmNodeId, baselines: Baselines) {
        assert!(
            Baselines::new(baselines.first, baselines.last).is_some(),
            "formatting-context baselines must be finite logical offsets"
        );
        self.nodes[id.index()].baselines = baselines;
    }

    /// Propagate child formatting-context baseline outputs through the
    /// standards-owned scratch tree. This consumes each child's declared
    /// output and parent-relative placement, never Taffy's child traversal.
    pub fn propagate_baselines(&mut self) {
        for node in &mut self.nodes {
            node.baselines = Baselines::synthesized_from_block_end(node.final_layout.size.height);
        }
        self.propagate_declared_baselines();
    }

    /// Re-run parent baseline selection after a caller has supplied direct
    /// line-formatting baselines for admitted measured contexts.
    pub fn propagate_declared_baselines(&mut self) {
        for index in (0..self.nodes.len()).rev() {
            let children = self.nodes[index].children.clone();
            let first = children.iter().copied().find_map(|child| {
                let child_node = &self.nodes[child.index()];
                (child_node.block_style.float == FloatSide::None
                    && !child_node.block_style.is_out_of_flow())
                .then(|| {
                    child_node
                        .baselines
                        .first
                        .map(|baseline| child_node.final_layout.location.y + baseline)
                })
                .flatten()
            });
            let last = children.iter().rev().copied().find_map(|child| {
                let child_node = &self.nodes[child.index()];
                (child_node.block_style.float == FloatSide::None
                    && !child_node.block_style.is_out_of_flow())
                .then(|| {
                    child_node
                        .baselines
                        .last
                        .map(|baseline| child_node.final_layout.location.y + baseline)
                })
                .flatten()
            });
            if first.is_some() || last.is_some() {
                self.nodes[index].baselines = Baselines::new(first, last)
                    .expect("child baseline outputs remain finite logical offsets");
            }
        }
    }

    pub fn node_ids(&self) -> impl Iterator<Item = AlgorithmNodeId> + '_ {
        (0..self.nodes.len()).map(|index| {
            AlgorithmNodeId(
                index
                    .try_into()
                    .expect("an algorithm scratch tree exceeded u32::MAX nodes"),
            )
        })
    }
}

impl<S, Context, Source> AlgorithmTree<S, Context, Source>
where
    S: AlgorithmStyle,
{
    /// Run scratch layout with the root's renderer-owned positioned children
    /// presented to Taffy as out of flow.
    ///
    /// Buckram retains their CSS position and final geometry separately. This
    /// adapter role only prevents a direct absolute or fixed child from
    /// contributing to an intrinsic or table-cell measurement while preserving
    /// the backend node for later formatting and fragment collection.
    pub fn compute_layout_with_measure_excluding_out_of_flow_children<Measure>(
        &mut self,
        root: AlgorithmNodeId,
        available: AlgorithmSize<AlgorithmAvailableSpace>,
        measure: Measure,
    ) where
        Measure: FnMut(
            AlgorithmSize<Option<f32>>,
            AlgorithmSize<AlgorithmAvailableSpace>,
            AlgorithmNodeId,
            Option<&mut Context>,
            Option<&FloatLineConstraints>,
        ) -> AlgorithmSize<f32>,
    {
        let children = self.nodes[root.index()].children.clone();
        let mut flipped = Vec::new();
        for child in children {
            if !self.nodes[child.index()].block_style.is_out_of_flow() {
                continue;
            }
            let style =
                sealed::AlgorithmStyle::as_taffy_style_mut(&mut self.nodes[child.index()].style);
            flipped.push((child, style.position));
            style.position = taffy::Position::Absolute;
        }
        self.clear_layout_cache();
        self.compute_layout_with_measure(root, available, measure);
        for (child, previous) in flipped {
            sealed::AlgorithmStyle::as_taffy_style_mut(&mut self.nodes[child.index()].style)
                .position = previous;
        }
        // The next ordinary run must not reuse a cache entry made while those
        // private backend roles were temporarily different.
        self.clear_layout_cache();
    }

    pub fn compute_layout_with_measure<Measure>(
        &mut self,
        root: AlgorithmNodeId,
        available: AlgorithmSize<AlgorithmAvailableSpace>,
        measure: Measure,
    ) where
        Measure: FnMut(
            AlgorithmSize<Option<f32>>,
            AlgorithmSize<AlgorithmAvailableSpace>,
            AlgorithmNodeId,
            Option<&mut Context>,
            Option<&FloatLineConstraints>,
        ) -> AlgorithmSize<f32>,
    {
        let available = taffy::Size {
            width: to_taffy_available(available.width),
            height: to_taffy_available(available.height),
        };
        let mut run = AlgorithmRun {
            tree: self,
            measure,
            line_constraints: None,
            nested_float_state: None,
            resolved_shrink_to_fit: None,
            fixed_leaf_intrinsics_enabled: false,
            marker: PhantomData,
        };
        compute_root_layout(&mut run, root.into_taffy(), available);
        round_layout(&mut run, root.into_taffy());
        run.tree.propagate_baselines();
    }
}

fn to_taffy_available(value: AlgorithmAvailableSpace) -> taffy::AvailableSpace {
    match value {
        AlgorithmAvailableSpace::Definite(value) => taffy::AvailableSpace::Definite(value),
        AlgorithmAvailableSpace::MinContent => taffy::AvailableSpace::MinContent,
        AlgorithmAvailableSpace::MaxContent => taffy::AvailableSpace::MaxContent,
    }
}

fn from_taffy_available(value: taffy::AvailableSpace) -> AlgorithmAvailableSpace {
    match value {
        taffy::AvailableSpace::Definite(value) => AlgorithmAvailableSpace::Definite(value),
        taffy::AvailableSpace::MinContent => AlgorithmAvailableSpace::MinContent,
        taffy::AvailableSpace::MaxContent => AlgorithmAvailableSpace::MaxContent,
    }
}

#[derive(Clone, Copy)]
struct PhysicalOptionalSize {
    width: Option<f32>,
    height: Option<f32>,
}

impl PhysicalOptionalSize {
    fn from_taffy(size: taffy::Size<Option<f32>>) -> Self {
        Self {
            width: size.width,
            height: size.height,
        }
    }

    fn from_available(size: taffy::Size<taffy::AvailableSpace>) -> Self {
        Self {
            width: available_option(size.width),
            height: available_option(size.height),
        }
    }

    fn into_taffy(self) -> taffy::Size<Option<f32>> {
        taffy::Size {
            width: self.width,
            height: self.height,
        }
    }
}

fn available_option(value: taffy::AvailableSpace) -> Option<f32> {
    match value {
        taffy::AvailableSpace::Definite(value) => Some(value),
        taffy::AvailableSpace::MinContent | taffy::AvailableSpace::MaxContent => None,
    }
}

fn logical_optional_size(axes: FlowAxes, physical: PhysicalOptionalSize) -> LogicalOptionalSize {
    if axes.is_horizontal() {
        LogicalOptionalSize {
            inline: physical.width,
            block: physical.height,
        }
    } else {
        LogicalOptionalSize {
            inline: physical.height,
            block: physical.width,
        }
    }
}

fn physical_optional_size(axes: FlowAxes, logical: LogicalOptionalSize) -> PhysicalOptionalSize {
    if axes.is_horizontal() {
        PhysicalOptionalSize {
            width: logical.inline,
            height: logical.block,
        }
    } else {
        PhysicalOptionalSize {
            width: logical.block,
            height: logical.inline,
        }
    }
}

fn physical_available_size(
    axes: FlowAxes,
    logical: AlgorithmSize<AlgorithmAvailableSpace>,
) -> taffy::Size<taffy::AvailableSpace> {
    if axes.is_horizontal() {
        taffy::Size {
            width: to_taffy_available(logical.width),
            height: to_taffy_available(logical.height),
        }
    } else {
        taffy::Size {
            width: to_taffy_available(logical.height),
            height: to_taffy_available(logical.width),
        }
    }
}

fn logical_available_size(
    axes: FlowAxes,
    physical: taffy::Size<taffy::AvailableSpace>,
) -> AlgorithmSize<AlgorithmAvailableSpace> {
    if axes.is_horizontal() {
        AlgorithmSize::new(
            from_taffy_available(physical.width),
            from_taffy_available(physical.height),
        )
    } else {
        AlgorithmSize::new(
            from_taffy_available(physical.height),
            from_taffy_available(physical.width),
        )
    }
}

#[derive(Clone, Copy)]
struct LogicalOptionalSize {
    inline: Option<f32>,
    block: Option<f32>,
}

fn resolve_outer_dimension(
    preferred: BlockSizeValue,
    minimum: BlockSizeValue,
    maximum: BlockSizeValue,
    containing_size: Option<f32>,
    padding_border: f32,
    box_sizing: BlockBoxSizing,
) -> Option<f32> {
    preferred
        .resolve_definite(containing_size)
        .map(|preferred| specified_outer_size(preferred, padding_border, box_sizing))
        .map(|preferred| {
            clamp_outer_dimension(
                preferred,
                minimum,
                maximum,
                containing_size,
                padding_border,
                box_sizing,
            )
        })
}

fn clamp_outer_dimension(
    value: f32,
    minimum: BlockSizeValue,
    maximum: BlockSizeValue,
    containing_size: Option<f32>,
    padding_border: f32,
    box_sizing: BlockBoxSizing,
) -> f32 {
    let minimum = minimum
        .resolve_definite(containing_size)
        .map(|minimum| specified_outer_size(minimum, padding_border, box_sizing))
        .unwrap_or(padding_border);
    let maximum = maximum
        .resolve_definite(containing_size)
        .map(|maximum| specified_outer_size(maximum, padding_border, box_sizing));
    let value = value.max(minimum);
    maximum.map_or(value, |maximum| value.min(maximum.max(padding_border)))
}

fn specified_outer_size(specified: f32, padding_border: f32, box_sizing: BlockBoxSizing) -> f32 {
    match box_sizing {
        BlockBoxSizing::ContentBox => specified.max(0.0) + padding_border,
        BlockBoxSizing::BorderBox => specified.max(padding_border),
    }
}

fn to_taffy_rect(sides: PhysicalSides<f32>) -> taffy::Rect<f32> {
    taffy::Rect {
        left: sides.left,
        right: sides.right,
        top: sides.top,
        bottom: sides.bottom,
    }
}

/// Whether a box is opaque to the block formatting context it participates
/// in: it establishes an independent formatting context and is not a float,
/// so CSS 2.1 section 9.4.1 gives it its own algorithm and nothing inside it
/// interacts with the outside. Floats keep their existing walk because their
/// placement is the parent's decision (section 9.5).
fn is_opaque_formatting_root(style: BlockStyle) -> bool {
    style.establishes_bfc && style.float == FloatSide::None
}

struct ChildIter<'a>(slice::Iter<'a, AlgorithmNodeId>);

impl Iterator for ChildIter<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().copied().map(AlgorithmNodeId::into_taffy)
    }
}

struct AlgorithmRun<'a, S, Context, Source, Measure> {
    tree: &'a mut AlgorithmTree<S, Context, Source>,
    measure: Measure,
    line_constraints: Option<FloatLineConstraints>,
    nested_float_state: Option<FloatContextState>,
    resolved_shrink_to_fit: Option<AlgorithmNodeId>,
    fixed_leaf_intrinsics_enabled: bool,
    marker: PhantomData<&'a mut Context>,
}

#[derive(Clone, Copy)]
struct BlockChildInput {
    /// The containing formatting context's logical axes. A child can have a
    /// different own flow, but its containing-size inputs remain in this
    /// context's coordinate space until the adapter boundary.
    containing_flow: FlowAxes,
    /// A same-flow parent has already solved this physical dimension through
    /// its inline-size equation. An orthogonal child must size that physical
    /// axis in its own formatting context instead.
    border_box_inline_size: Option<f32>,
    containing_size: LogicalOptionalSize,
    available_size: AlgorithmSize<AlgorithmAvailableSpace>,
}

struct PendingBlockChildLayout {
    child: AlgorithmNodeId,
    order: u32,
    output: LayoutOutput,
    padding: PhysicalSides<f32>,
    border: PhysicalSides<f32>,
    margin: PhysicalSides<f32>,
    logical_rect: LogicalRect,
    static_position: bool,
}

impl<S, Context, Source, Measure> AlgorithmRun<'_, S, Context, Source, Measure>
where
    S: AlgorithmStyle,
    Measure: FnMut(
        AlgorithmSize<Option<f32>>,
        AlgorithmSize<AlgorithmAvailableSpace>,
        AlgorithmNodeId,
        Option<&mut Context>,
        Option<&FloatLineConstraints>,
    ) -> AlgorithmSize<f32>,
{
    fn style(&self, id: NodeId) -> &Style {
        sealed::AlgorithmStyle::as_taffy_style(
            &self.tree.nodes[AlgorithmNodeId::from_taffy(id).index()].style,
        )
    }

    fn block_subtree_deferral(&self, node: AlgorithmNodeId) -> Option<BlockDeferral> {
        if self.contains_inline_context_float(node) {
            return Some(BlockDeferral::NestedFloatState);
        }
        let mut active_left_float = self
            .nested_float_state
            .as_ref()
            .is_some_and(|state| state.has_side(FloatSide::Left));
        let mut active_right_float = self
            .nested_float_state
            .as_ref()
            .is_some_and(|state| state.has_side(FloatSide::Right));
        self.block_subtree_deferral_with_float_state(
            node,
            &mut active_left_float,
            &mut active_right_float,
        )
    }

    fn contains_inline_context_float(&self, node: AlgorithmNodeId) -> bool {
        self.tree.nodes[node.index()]
            .children
            .iter()
            .copied()
            .any(|child| {
                let child_node = &self.tree.nodes[child.index()];
                child_node.inline_context_float
                    || (!is_opaque_formatting_root(child_node.block_style)
                        && self.contains_inline_context_float(child))
            })
    }

    fn block_subtree_deferral_with_float_state(
        &self,
        node: AlgorithmNodeId,
        active_left_float: &mut bool,
        active_right_float: &mut bool,
    ) -> Option<BlockDeferral> {
        for child in self.tree.nodes[node.index()].children.iter().copied() {
            let child_node = &self.tree.nodes[child.index()];
            let child_style = child_node.block_style;
            if child_style.is_out_of_flow() {
                // The parent preserves this child's static rectangle and
                // formats it locally below. It does not participate in the
                // normal-flow cursor or make the whole parent fall back.
                continue;
            }
            if let Some(deferral) = child_style.deferral() {
                let admitted_shrink_to_fit = child_node.intrinsic_shrink_to_fit_enabled
                    && matches!(
                        deferral,
                        BlockDeferral::ShrinkToFit | BlockDeferral::FloatShrinkToFit
                    );
                if !admitted_shrink_to_fit {
                    return Some(deferral);
                }
            }
            // Float exclusions live in their owning BFC's logical axes. Do
            // not copy physical left/right state into an orthogonal child;
            // only same-flow continuation is admitted until that transform
            // has its own modeled contract.
            if child_style.flow.is_horizontal() != child_style.containing_flow.is_horizontal()
                && (child_style.float != FloatSide::None
                    || child_style.clear != ClearSide::None
                    || *active_left_float
                    || *active_right_float
                    || self.exports_float_state(child))
            {
                return Some(BlockDeferral::NestedFloatState);
            }

            match child_style.clear {
                ClearSide::None => {},
                ClearSide::Left => *active_left_float = false,
                ClearSide::Right => *active_right_float = false,
                ClearSide::Both => {
                    *active_left_float = false;
                    *active_right_float = false;
                },
            }

            if child_style.float != FloatSide::None {
                if child_node.inline_context_float {
                    return Some(BlockDeferral::NestedFloatState);
                }
                match child_style.float {
                    FloatSide::None => {},
                    FloatSide::Left => *active_left_float = true,
                    FloatSide::Right => *active_right_float = true,
                }
                if child_node.kind == AlgorithmKind::Block {
                    let mut isolated_left = false;
                    let mut isolated_right = false;
                    if let Some(deferral) = self.block_subtree_deferral_with_float_state(
                        child,
                        &mut isolated_left,
                        &mut isolated_right,
                    ) {
                        return Some(deferral);
                    }
                }
                continue;
            }

            let floats_are_active = *active_left_float || *active_right_float;
            if is_opaque_formatting_root(child_style) {
                // CSS 2.1 section 9.4.1: a box that establishes an independent
                // formatting context is opaque to this one. Its own algorithm
                // lays it out (Buckram's block path, the table pipeline, flex,
                // grid, or Taffy's block algorithm when its own root defers),
                // and only its border box and margins take part in this flow,
                // so nothing inside it is walked here. The one parent-level
                // rule is section 9.5: beside an active float its border box
                // may not overlap the float. That needs Livery's avoidance
                // opt-in (K3g) and otherwise stays a named deferral.
                if floats_are_active && !child_node.float_avoidance_enabled {
                    return Some(BlockDeferral::FloatFormattingContextAvoidance);
                }
                // Flex and grid keep their established dispatch until their
                // own lane measures the change: without a float that needs
                // the K3g lane, an ordinary parent still defers for them.
                if !floats_are_active
                    && matches!(child_node.kind, AlgorithmKind::Flex | AlgorithmKind::Grid)
                {
                    return Some(BlockDeferral::IndependentFormattingContext);
                }
                continue;
            }

            // From here on the child participates in this block formatting
            // context: floats, clearance, and line boxes inside it are this
            // context's concern.
            let exports_float_state =
                child_node.kind == AlgorithmKind::Block && self.exports_float_state(child);
            let contains_clearance =
                child_node.kind == AlgorithmKind::Block && self.contains_clearance(child);
            if exports_float_state && let Some(deferral) = self.nested_float_state_deferral(child) {
                return Some(deferral);
            }
            if child_node.kind == AlgorithmKind::Block
                && !self.shares_parent_float_context(child, child_style)
                && (floats_are_active || exports_float_state || contains_clearance)
            {
                return Some(BlockDeferral::NestedFloatState);
            }
            if floats_are_active
                && self.has_line_boxes_in_same_bfc(child)
                && !self.accepts_float_line_constraints_in_same_bfc(child)
            {
                return Some(BlockDeferral::FloatLineExclusion);
            }

            if child_node.kind == AlgorithmKind::Block
                && let Some(deferral) = self.block_subtree_deferral_with_float_state(
                    child,
                    active_left_float,
                    active_right_float,
                )
            {
                return Some(deferral);
            }
        }
        None
    }

    fn nested_float_state_deferral(&self, node: AlgorithmNodeId) -> Option<BlockDeferral> {
        for child in self.tree.nodes[node.index()].children.iter().copied() {
            let child_node = &self.tree.nodes[child.index()];
            let style = child_node.block_style;
            if style.float != FloatSide::None {
                continue;
            }
            if child_node.kind == AlgorithmKind::Block
                && !style.establishes_bfc
                && let Some(deferral) = self.nested_float_state_deferral(child)
            {
                return Some(deferral);
            }
        }
        None
    }

    fn exports_float_state(&self, node: AlgorithmNodeId) -> bool {
        self.tree.nodes[node.index()]
            .children
            .iter()
            .copied()
            .any(|child| {
                let child_node = &self.tree.nodes[child.index()];
                let style = child_node.block_style;
                style.float != FloatSide::None
                    || (child_node.kind == AlgorithmKind::Block
                        && !style.establishes_bfc
                        && self.exports_float_state(child))
            })
    }

    fn contains_clearance(&self, node: AlgorithmNodeId) -> bool {
        self.tree.nodes[node.index()]
            .children
            .iter()
            .copied()
            .any(|child| {
                let child_node = &self.tree.nodes[child.index()];
                let style = child_node.block_style;
                style.clear != ClearSide::None
                    || (child_node.kind == AlgorithmKind::Block
                        && !style.establishes_bfc
                        && self.contains_clearance(child))
            })
    }

    fn table_wrapper_inline_size(&self, node: AlgorithmNodeId) -> Option<f32> {
        self.tree.nodes[node.index()].table_wrapper_inline_size
    }

    fn has_line_boxes_in_same_bfc(&self, node: AlgorithmNodeId) -> bool {
        let node = &self.tree.nodes[node.index()];
        node.context.is_some()
            || node.children.iter().copied().any(|child| {
                let child_node = &self.tree.nodes[child.index()];
                let style = child_node.block_style;
                style.float == FloatSide::None
                    && !style.establishes_bfc
                    && self.has_line_boxes_in_same_bfc(child)
            })
    }

    fn accepts_float_line_constraints_in_same_bfc(&self, node: AlgorithmNodeId) -> bool {
        let node = &self.tree.nodes[node.index()];
        if node.context.is_some() && !node.float_line_constraints_enabled {
            return false;
        }
        node.children.iter().copied().all(|child| {
            let child_node = &self.tree.nodes[child.index()];
            let style = child_node.block_style;
            style.float != FloatSide::None
                || style.establishes_bfc
                || self.accepts_float_line_constraints_in_same_bfc(child)
        })
    }

    fn compute_block_child(
        &mut self,
        child: AlgorithmNodeId,
        input: BlockChildInput,
        line_constraints: Option<FloatLineConstraints>,
        nested_float_state: Option<FloatContextState>,
    ) -> LayoutOutput {
        if line_constraints.is_some() || nested_float_state.is_some() {
            // Float geometry is not represented in Taffy's cache key. Force
            // the nested subtree through the caller on the final pass.
            self.tree.nodes[child.index()].cache.clear();
        }
        let previous_line_constraints =
            std::mem::replace(&mut self.line_constraints, line_constraints);
        let previous_nested_float_state =
            std::mem::replace(&mut self.nested_float_state, nested_float_state);
        let resolved_shrink_to_fit = self.tree.nodes[child.index()].intrinsic_shrink_to_fit_enabled;
        let previous_resolved_shrink_to_fit = std::mem::replace(
            &mut self.resolved_shrink_to_fit,
            resolved_shrink_to_fit.then_some(child),
        );
        let known_dimensions = physical_optional_size(
            input.containing_flow,
            LogicalOptionalSize {
                inline: input.border_box_inline_size,
                block: None,
            },
        )
        .into_taffy();
        let parent_size =
            physical_optional_size(input.containing_flow, input.containing_size).into_taffy();
        let output = self.compute_node(
            child.into_taffy(),
            LayoutInput {
                run_mode: RunMode::PerformLayout,
                sizing_mode: SizingMode::InherentSize,
                axis: taffy::RequestedAxis::Both,
                known_dimensions,
                parent_size,
                available_space: physical_available_size(
                    input.containing_flow,
                    input.available_size,
                ),
                vertical_margins_are_collapsible: taffy::Line::FALSE,
            },
            None,
        );
        self.line_constraints = previous_line_constraints;
        self.nested_float_state = previous_nested_float_state;
        self.resolved_shrink_to_fit = previous_resolved_shrink_to_fit;
        output
    }

    /// Format one out-of-flow child in its own local coordinate space. The
    /// caller owns its participation and static position, so its root may use
    /// Buckram's ordinary block algorithm after temporarily removing only the
    /// root's position deferral.
    /// Run one Taffy block fallback with this node's absolute and fixed
    /// children presented to Taffy as out of flow. Buckram owns their final
    /// geometry; the fallback only needs them to take no normal-flow space
    /// and to report their hypothetical in-flow position as
    /// `static_location`. Each child's private backend role is restored
    /// afterwards so its own leaf or block layout is unchanged.
    fn with_out_of_flow_children_excluded<R>(
        &mut self,
        node_id: NodeId,
        layout: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let node_index = AlgorithmNodeId::from_taffy(node_id).index();
        let children = self.tree.nodes[node_index].children.clone();
        let mut flipped = Vec::new();
        for child in children {
            if !self.tree.nodes[child.index()].block_style.is_out_of_flow() {
                continue;
            }
            let style = sealed::AlgorithmStyle::as_taffy_style_mut(
                &mut self.tree.nodes[child.index()].style,
            );
            flipped.push((child, style.position));
            style.position = taffy::Position::Absolute;
        }
        let output = layout(self);
        for (child, previous) in flipped {
            sealed::AlgorithmStyle::as_taffy_style_mut(&mut self.tree.nodes[child.index()].style)
                .position = previous;
        }
        output
    }

    fn compute_out_of_flow_block_child(
        &mut self,
        child: AlgorithmNodeId,
        input: BlockChildInput,
    ) -> LayoutOutput {
        let original_position = self.tree.nodes[child.index()].block_style.position;
        self.tree.nodes[child.index()].block_style.position = crate::BlockPosition::Static;
        self.clear_subtree_cache(child);
        let output = self.compute_block_child(child, input, None, None);
        self.tree.nodes[child.index()].block_style.position = original_position;
        output
    }

    fn measure_inline_intrinsic(
        &mut self,
        node: AlgorithmNodeId,
        kind: IntrinsicSizeKind,
    ) -> Option<f32> {
        let available_width = match kind {
            IntrinsicSizeKind::MinContent => AlgorithmAvailableSpace::MinContent,
            IntrinsicSizeKind::MaxContent => AlgorithmAvailableSpace::MaxContent,
        };
        let context = self.tree.nodes[node.index()].context.as_mut()?;
        let measured = (self.measure)(
            AlgorithmSize::new(None, None),
            AlgorithmSize::new(available_width, AlgorithmAvailableSpace::MaxContent),
            node,
            Some(context),
            None,
        );
        Some(measured.width)
    }

    fn measure_intrinsic_inline_subtree(
        &mut self,
        node: AlgorithmNodeId,
        orthogonal_inline_constraint: Option<f32>,
    ) -> Result<IntrinsicSizes, BlockDeferral> {
        if orthogonal_inline_constraint.is_none()
            && let Some(sizes) = self.tree.nodes[node.index()].intrinsic_inline_sizes
        {
            return Ok(sizes);
        }

        let kind = self.tree.nodes[node.index()].kind;
        let style = self.tree.nodes[node.index()].block_style;
        let definite_content_size = if kind == AlgorithmKind::Block
            || (kind == AlgorithmKind::Leaf && self.fixed_leaf_intrinsics_enabled)
        {
            intrinsic_definite_inline_content_size(style)?
        } else {
            None
        };
        let sizes = if let Some(content_size) = definite_content_size {
            IntrinsicSizes::new(content_size, content_size).ok_or(BlockDeferral::IntrinsicSize)?
        } else {
            match kind {
                // K4d6 supplies table intrinsics from the accepted K4c query
                // contract; nothing constructs the tag before then.
                AlgorithmKind::Hidden | AlgorithmKind::Table => {
                    IntrinsicSizes::new(0.0, 0.0).expect("zero intrinsic sizes are valid")
                },
                AlgorithmKind::Leaf if self.tree.nodes[node.index()].context.is_none() => {
                    IntrinsicSizes::new(0.0, 0.0).expect("zero intrinsic sizes are valid")
                },
                AlgorithmKind::Leaf => {
                    if orthogonal_inline_constraint.is_some() && !style.flow.is_horizontal() {
                        return Err(BlockDeferral::IntrinsicSize);
                    }
                    let min_content = self
                        .measure_inline_intrinsic(node, IntrinsicSizeKind::MinContent)
                        .ok_or(BlockDeferral::IntrinsicSize)?;
                    let max_content = self
                        .measure_inline_intrinsic(node, IntrinsicSizeKind::MaxContent)
                        .ok_or(BlockDeferral::IntrinsicSize)?;
                    IntrinsicSizes::new(min_content, max_content)
                        .ok_or(BlockDeferral::IntrinsicSize)?
                },
                AlgorithmKind::Block => {
                    let children = self.tree.nodes[node.index()].children.clone();
                    let mut min_content = 0.0_f32;
                    let mut max_content = 0.0_f32;
                    for child in children {
                        let child_style = self.tree.nodes[child.index()].block_style;
                        let child_sizes =
                            if style.flow.is_horizontal() == child_style.flow.is_horizontal() {
                                self.intrinsic_inline_outer_contribution(
                                    child,
                                    orthogonal_inline_constraint,
                                )?
                            } else {
                                self.measure_orthogonal_block_outer_contribution(
                                    child,
                                    orthogonal_inline_constraint,
                                )?
                            };
                        min_content = min_content.max(child_sizes.min_content);
                        max_content = max_content.max(child_sizes.max_content);
                    }
                    IntrinsicSizes::new(min_content, max_content)
                        .ok_or(BlockDeferral::IntrinsicSize)?
                },
                // Flex and grid retain their formatting roles. Taffy is queried
                // in its intrinsic mode for these admitted algorithm subtrees;
                // Buckram never reads a completed normal-flow layout back as the
                // browser-facing intrinsic answer.
                AlgorithmKind::Flex | AlgorithmKind::Grid => {
                    self.measure_admitted_algorithm_inline_intrinsic(node, style)?
                },
            }
        };
        if orthogonal_inline_constraint.is_none() {
            self.tree.nodes[node.index()].intrinsic_inline_sizes = Some(sizes);
        }
        Ok(sizes)
    }

    fn intrinsic_inline_outer_contribution(
        &mut self,
        node: AlgorithmNodeId,
        orthogonal_inline_constraint: Option<f32>,
    ) -> Result<IntrinsicSizes, BlockDeferral> {
        let style = self.tree.nodes[node.index()].block_style;
        let sizes = self.measure_intrinsic_inline_subtree(node, orthogonal_inline_constraint)?;
        let (padding_border_start, padding_border_end) = intrinsic_inline_padding_border(style)?;
        let (margin_start, margin_end) = intrinsic_inline_margins(style)?;
        let outer = padding_border_start + padding_border_end + margin_start + margin_end;
        IntrinsicSizes::new(sizes.min_content + outer, sizes.max_content + outer)
            .ok_or(BlockDeferral::IntrinsicSize)
    }

    /// Resolve the contribution of a perpendicular child to this formatting
    /// root's intrinsic inline size. Writing Modes requires the child's sizing
    /// phase to run first: its auto inline size becomes fit-content, then its
    /// used block size contributes to the parent's min/max-content query.
    ///
    /// The query temporarily treats the child as the root of its own flow and
    /// accepts only a Buckram block result (or a formatter leaf). A Taffy block
    /// fallback therefore cannot become an intrinsic answer.
    fn measure_orthogonal_block_outer_contribution(
        &mut self,
        node: AlgorithmNodeId,
        orthogonal_inline_constraint: Option<f32>,
    ) -> Result<IntrinsicSizes, BlockDeferral> {
        let constraint = orthogonal_inline_constraint
            .filter(|size| size.is_finite() && *size >= 0.0)
            .ok_or(BlockDeferral::IndefiniteInlineSize)?;
        let original_style = self.tree.nodes[node.index()].block_style;
        let kind = self.tree.nodes[node.index()].kind;
        if !matches!(kind, AlgorithmKind::Block | AlgorithmKind::Leaf)
            || original_style.position != crate::BlockPosition::Static
            || original_style.float != FloatSide::None
            || original_style.replaced
            || original_style.aspect_ratio.is_some()
            || original_style.size_containment.width
            || original_style.size_containment.height
            || original_style.has_nonlinear_lengths
        {
            return Err(BlockDeferral::IntrinsicSize);
        }

        let mut query_style = original_style;
        query_style.containing_flow = query_style.flow;
        self.tree.nodes[node.index()].block_style = query_style;
        let result = (|| {
            let intrinsic = self.measure_intrinsic_inline_subtree(node, Some(constraint))?;
            let mut sizing_style = query_style;
            sizing_style.shrink_to_fit = true;
            let used_inline = solve_shrink_to_fit_inline_size(sizing_style, constraint, intrinsic);
            self.clear_subtree_backend_cache(node);
            let output = self.compute_block_child(
                node,
                BlockChildInput {
                    containing_flow: query_style.flow,
                    border_box_inline_size: Some(used_inline.border_box),
                    containing_size: LogicalOptionalSize {
                        inline: Some(constraint),
                        block: None,
                    },
                    available_size: AlgorithmSize::new(
                        AlgorithmAvailableSpace::Definite(constraint),
                        AlgorithmAvailableSpace::MaxContent,
                    ),
                },
                None,
                None,
            );
            if kind == AlgorithmKind::Block
                && self.tree.nodes[node.index()].block_algorithm != Some(BlockAlgorithm::Buckram)
            {
                return Err(BlockDeferral::IntrinsicSize);
            }
            let block_size = query_style
                .flow
                .logical_size(PhysicalSize {
                    width: output.size.width,
                    height: output.size.height,
                })
                .block;
            let (margin_start, margin_end) = intrinsic_inline_margins(original_style)?;
            let outer = block_size + margin_start + margin_end;
            IntrinsicSizes::new(outer, outer).ok_or(BlockDeferral::IntrinsicSize)
        })();
        self.tree.nodes[node.index()].block_style = original_style;
        self.clear_subtree_cache(node);
        result
    }

    fn measure_admitted_algorithm_inline_intrinsic(
        &mut self,
        node: AlgorithmNodeId,
        style: BlockStyle,
    ) -> Result<IntrinsicSizes, BlockDeferral> {
        let min_border_box =
            self.measure_algorithm_inline_intrinsic(node, IntrinsicSizeKind::MinContent);
        let max_border_box =
            self.measure_algorithm_inline_intrinsic(node, IntrinsicSizeKind::MaxContent);
        let (padding_border_start, padding_border_end) = intrinsic_inline_padding_border(style)?;
        let padding_border = padding_border_start + padding_border_end;
        IntrinsicSizes::new(
            (min_border_box - padding_border).max(0.0),
            (max_border_box - padding_border).max(0.0),
        )
        .ok_or(BlockDeferral::IntrinsicSize)
    }

    fn measure_algorithm_inline_intrinsic(
        &mut self,
        node: AlgorithmNodeId,
        kind: IntrinsicSizeKind,
    ) -> f32 {
        let available_width = match kind {
            IntrinsicSizeKind::MinContent => taffy::AvailableSpace::MinContent,
            IntrinsicSizeKind::MaxContent => taffy::AvailableSpace::MaxContent,
        };
        self.clear_subtree_backend_cache(node);
        self.compute_node(
            node.into_taffy(),
            LayoutInput {
                run_mode: RunMode::ComputeSize,
                sizing_mode: SizingMode::ContentSize,
                axis: taffy::RequestedAxis::Horizontal,
                known_dimensions: taffy::Size {
                    width: None,
                    height: None,
                },
                parent_size: taffy::Size {
                    width: None,
                    height: None,
                },
                available_space: taffy::Size {
                    width: available_width,
                    height: taffy::AvailableSpace::MaxContent,
                },
                vertical_margins_are_collapsible: taffy::Line::FALSE,
            },
            None,
        )
        .size
        .width
    }

    fn resolve_intrinsic_shrink_to_fit(
        &mut self,
        node: AlgorithmNodeId,
        style: BlockStyle,
        containing_inline_size: f32,
    ) -> Result<crate::UsedInlineSize, BlockDeferral> {
        // Auto-width floats and atomic inline roots need fixed-size leaf
        // descendants in their intrinsic contribution. Positioned queries
        // keep their separately admitted empty-leaf contract.
        let previous = std::mem::replace(&mut self.fixed_leaf_intrinsics_enabled, true);
        let intrinsic = self.measure_intrinsic_inline_subtree(node, None);
        self.fixed_leaf_intrinsics_enabled = previous;
        let intrinsic = intrinsic?;
        Ok(solve_shrink_to_fit_inline_size(
            style,
            containing_inline_size,
            intrinsic,
        ))
    }

    fn resolve_orthogonal_auto_inline_size(
        &mut self,
        node: AlgorithmNodeId,
        style: BlockStyle,
        constraint: f32,
    ) -> Result<f32, BlockDeferral> {
        if !constraint.is_finite()
            || constraint < 0.0
            || style.flow.is_horizontal() == style.containing_flow.is_horizontal()
            // The vertical-host half of this first slice sizes a direct
            // perpendicular block contribution. Walking through same-flow
            // wrappers can reach a nested horizontal atomic inline while an
            // adjacent vertical text run remains unsupported, producing two
            // different answers for otherwise equivalent subtrees.
            || (!style.flow.is_horizontal()
                && !self.tree.nodes[node.index()].children.iter().any(|child| {
                    let child_kind = self.tree.nodes[child.index()].kind;
                    let child_style = self.tree.nodes[child.index()].block_style;
                    matches!(child_kind, AlgorithmKind::Block)
                        && child_style.position == crate::BlockPosition::Static
                        && child_style.float == FloatSide::None
                        && child_style.flow.is_horizontal()
                            != style.flow.is_horizontal()
                }))
            || !matches!(
                intrinsic_inline_dimension(style.size, style.flow),
                BlockSizeValue::Auto
            )
        {
            return Err(BlockDeferral::IntrinsicSize);
        }

        let original_style = self.tree.nodes[node.index()].block_style;
        let mut query_style = style;
        query_style.containing_flow = query_style.flow;
        // This first slice admits only absolute padding and margins. Their
        // percentage basis differs across the sizing and positioning phases;
        // accepting percentages here would silently choose the wrong phase.
        intrinsic_inline_padding_border(query_style)?;
        intrinsic_inline_margins(query_style)?;
        self.tree.nodes[node.index()].block_style = query_style;
        let result = self
            .measure_intrinsic_inline_subtree(node, Some(constraint))
            .map(|intrinsic| {
                let mut sizing_style = query_style;
                sizing_style.shrink_to_fit = true;
                solve_shrink_to_fit_inline_size(sizing_style, constraint, intrinsic).border_box
            });
        self.tree.nodes[node.index()].block_style = original_style;
        result
    }

    fn clear_subtree_cache(&mut self, node: AlgorithmNodeId) {
        let children = self.tree.nodes[node.index()].children.clone();
        self.tree.nodes[node.index()].cache.clear();
        self.tree.nodes[node.index()].intrinsic_inline_sizes = None;
        for child in children {
            self.clear_subtree_cache(child);
        }
    }

    fn clear_subtree_backend_cache(&mut self, node: AlgorithmNodeId) {
        let children = self.tree.nodes[node.index()].children.clone();
        self.tree.nodes[node.index()].cache.clear();
        for child in children {
            self.clear_subtree_backend_cache(child);
        }
    }

    fn child_margin_state(
        &self,
        child: AlgorithmNodeId,
        child_style: BlockStyle,
        child_size: PhysicalSize,
        containing_inline_size: f32,
        containing_block_size: Option<f32>,
    ) -> BlockMarginState {
        let child_node = &self.tree.nodes[child.index()];
        if child_node.kind == AlgorithmKind::Block
            && child_node.block_deferral.is_none()
            && let Some(margins) = child_node.block_margins
        {
            return margins;
        }

        // A leaf, an algorithm-owned context (table, flex, grid), or a block
        // that Taffy laid out for its own reasons contributes margins modeled
        // from its style and used size. Taffy receives such a block with
        // `vertical_margins_are_collapsible: FALSE`, so the margins of its
        // own children lie inside the border box it returned; only an empty
        // box with no block-axis separator collapses through.
        let child_size_logical = child_style.containing_flow.logical_size(child_size);
        let child_has_line_boxes = child_node.context.is_some();
        let child_collapses_through = child_style.can_collapse_through(
            containing_inline_size,
            containing_block_size,
            false,
            child_has_line_boxes,
            true,
        ) && child_size_logical.block == 0.0;
        BlockMarginState::from_box(
            child_style,
            containing_inline_size,
            CollapsedMargin::ZERO,
            CollapsedMargin::ZERO,
            child_style.child_margin_collapse(
                containing_inline_size,
                containing_block_size,
                false,
                true,
            ),
            child_collapses_through,
        )
    }

    fn shares_parent_float_context(&self, child: AlgorithmNodeId, child_style: BlockStyle) -> bool {
        // Relative positioning preserves the box's normal-flow BFC geometry;
        // its retained visual offset is applied after this layout.
        self.tree.nodes[child.index()].nested_float_state_enabled
            && self.tree.nodes[child.index()].kind == AlgorithmKind::Block
            && !child_style.establishes_bfc
            && matches!(
                child_style.position,
                crate::BlockPosition::Static | crate::BlockPosition::Relative
            )
            && child_style.float == FloatSide::None
            && !child_style.replaced
            && child_style.flow == child_style.containing_flow
    }

    fn predicted_child_content_origin(
        formatting_context: &BlockFormattingContext,
        child_style: BlockStyle,
        margin_state: BlockMarginState,
        inline_size: crate::UsedInlineSize,
        containing_inline_size: f32,
    ) -> (f32, f32) {
        let padding_border = child_style.logical_padding_border(containing_inline_size);
        (
            inline_size.margin_start + padding_border.inline_start,
            formatting_context.hypothetical_in_flow_block_start(child_style, margin_state)
                + padding_border.block_start,
        )
    }

    fn compute_owned_block_layout(
        &mut self,
        node_id: NodeId,
        inputs: LayoutInput,
    ) -> Result<LayoutOutput, BlockDeferral> {
        if inputs.run_mode != RunMode::PerformLayout
            || inputs.sizing_mode != SizingMode::InherentSize
            || inputs.axis != taffy::RequestedAxis::Both
        {
            return Err(BlockDeferral::BackendSizingMode);
        }

        let node = AlgorithmNodeId::from_taffy(node_id);
        let node_index = node.index();
        self.tree.nodes[node_index].exported_float_state = None;
        let parent_is_block = self.tree.nodes[node_index]
            .parent
            .is_none_or(|parent| self.tree.nodes[parent.index()].kind == AlgorithmKind::Block);
        if !parent_is_block {
            return Err(BlockDeferral::BackendSizingMode);
        }

        let style = self.tree.nodes[node_index].block_style;
        let is_layout_root = self.tree.nodes[node_index].parent.is_none();
        if let Some(deferral) = style.deferral() {
            let resolved_shrink_to_fit = self.resolved_shrink_to_fit == Some(node)
                && matches!(
                    deferral,
                    BlockDeferral::ShrinkToFit | BlockDeferral::FloatShrinkToFit
                );
            if !resolved_shrink_to_fit {
                return Err(deferral);
            }
        }
        let parent_physical = PhysicalOptionalSize::from_taffy(inputs.parent_size);
        let available_physical = PhysicalOptionalSize::from_available(inputs.available_space);
        let containing_parent = logical_optional_size(style.containing_flow, parent_physical);
        let containing_available = logical_optional_size(style.containing_flow, available_physical);
        let containing_inline = containing_parent
            .inline
            .or(containing_available.inline)
            .ok_or(BlockDeferral::IndefiniteInlineSize)?;
        // Physical CSS width/height resolve against the containing block's
        // physical axes. Once resolved, keep the box's own logical axes until
        // its auto block size is known.
        let own_parent = logical_optional_size(style.flow, parent_physical);
        let own_available_optional = logical_optional_size(style.flow, available_physical);
        let own_available = logical_available_size(style.flow, inputs.available_space);
        let containing_block_size = own_parent.block.or(own_available_optional.block);
        let padding = style.resolved_padding(containing_inline);
        let border = style.border;
        let padding_border = padding.zip_map(border, |padding, border| padding + border);

        let mut outer = PhysicalOptionalSize::from_taffy(inputs.known_dimensions);
        // Taffy's root setup offers the available physical width as a known
        // dimension. For an orthogonal root that axis is its own auto block
        // axis, not a completed used size. Retain the logical query until
        // children establish the block contribution below.
        if is_layout_root
            && !style.flow.is_horizontal()
            && matches!(style.size.width, BlockSizeValue::Auto)
        {
            outer.width = None;
        }
        // A horizontal child in an auto-sized vertical containing block (or
        // the converse) has an indefinite physical width, but orthogonal-flow
        // sizing supplies the containing context's available physical width
        // as the percentage fallback. Keep ordinary percentage resolution
        // tied to the actual containing block; this fallback exists only at a
        // cross-flow boundary.
        let width_percentage_basis = parent_physical.width.or_else(|| {
            (style.flow.is_horizontal() != style.containing_flow.is_horizontal())
                .then_some(available_physical.width)
                .flatten()
        });
        outer.width = outer.width.or_else(|| {
            resolve_outer_dimension(
                style.size.width,
                style.min_size.width,
                style.max_size.width,
                width_percentage_basis,
                padding_border.left + padding_border.right,
                style.box_sizing,
            )
        });
        outer.height = outer.height.or_else(|| {
            resolve_outer_dimension(
                style.size.height,
                style.min_size.height,
                style.max_size.height,
                parent_physical.height,
                padding_border.top + padding_border.bottom,
                style.box_sizing,
            )
        });
        let mut outer_logical = logical_optional_size(style.flow, outer);
        // A table wrapper box that nobody else sized (a layout root or an
        // out-of-flow root) takes its grid's border-edge inline size rather
        // than filling the available space.
        let table_wrapper_inline = self.table_wrapper_inline_size(node);
        let orthogonal_auto_inline = if table_wrapper_inline.is_none()
            && outer_logical.inline.is_none()
            && style.flow.is_horizontal() != style.containing_flow.is_horizontal()
            && matches!(
                intrinsic_inline_dimension(style.size, style.flow),
                BlockSizeValue::Auto
            ) {
            let constraint = own_parent.inline.or(match own_available.width {
                AlgorithmAvailableSpace::Definite(size) => Some(size),
                AlgorithmAvailableSpace::MinContent | AlgorithmAvailableSpace::MaxContent => None,
            });
            constraint.and_then(|constraint| {
                self.resolve_orthogonal_auto_inline_size(node, style, constraint)
                    .ok()
            })
        } else {
            None
        };
        outer_logical.inline = table_wrapper_inline
            .or(outer_logical.inline)
            .or(orthogonal_auto_inline)
            .or(match own_available.width {
                AlgorithmAvailableSpace::Definite(size) => Some(size),
                AlgorithmAvailableSpace::MinContent | AlgorithmAvailableSpace::MaxContent => None,
            });
        let content_padding_border = style.content_logical_padding_border(containing_inline);
        let content_inline = outer_logical
            .inline
            .map(|size| {
                (size - content_padding_border.inline_start - content_padding_border.inline_end)
                    .max(0.0)
            })
            .ok_or(BlockDeferral::IndefiniteInlineSize)?;
        let content_block = outer_logical.block.map(|size| {
            (size - content_padding_border.block_start - content_padding_border.block_end).max(0.0)
        });
        let content_physical = physical_optional_size(
            style.flow,
            LogicalOptionalSize {
                inline: Some(content_inline),
                block: content_block,
            },
        );

        if let Some(deferral) = self.block_subtree_deferral(node) {
            return Err(deferral);
        }

        let collapse_parent_start = style
            .child_margin_collapse(
                containing_inline,
                containing_block_size,
                is_layout_root,
                false,
            )
            .block_start;
        let content_box = PhysicalSize {
            width: content_physical.width.unwrap_or_default(),
            height: content_physical.height.unwrap_or_default(),
        };
        let mut formatting_context = BlockFormattingContext::with_float_state(
            BlockContainingBlock {
                flow: style.flow,
                content_box,
            },
            collapse_parent_start,
            self.nested_float_state.clone().unwrap_or_default(),
        );
        let children = self.tree.nodes[node_index].children.clone();
        let mut placed_children = Vec::with_capacity(children.len());
        for (order, child) in children.into_iter().enumerate() {
            let child_style = self.tree.nodes[child.index()].block_style;
            let child_containing_block_size =
                logical_optional_size(child_style.flow, content_physical).block;
            let inline = if self.tree.nodes[child.index()].intrinsic_shrink_to_fit_enabled {
                self.resolve_intrinsic_shrink_to_fit(child, child_style, content_inline)?
            } else if child_style.float != FloatSide::None {
                solve_float_inline_size(child_style, content_inline)
            } else if let Some(grid_inline_size) = self.table_wrapper_inline_size(child) {
                solve_in_flow_inline_size_for_border_box(
                    child_style,
                    content_inline,
                    grid_inline_size,
                )
            } else {
                solve_in_flow_inline_size(child_style, content_inline)
            };
            let provisional_margin_state = BlockMarginState::from_box(
                child_style,
                content_inline,
                CollapsedMargin::ZERO,
                CollapsedMargin::ZERO,
                child_style.child_margin_collapse(
                    content_inline,
                    child_containing_block_size,
                    false,
                    true,
                ),
                false,
            );
            let avoids_floats = child_style.float == FloatSide::None
                && self.tree.nodes[child.index()].float_avoidance_enabled
                && formatting_context.float_exclusion_count() != 0;
            let mut float_avoiding_placement = None;
            let default_child_input = BlockChildInput {
                containing_flow: style.flow,
                border_box_inline_size: (style.flow.is_horizontal()
                    == child_style.flow.is_horizontal())
                .then_some(inline.border_box),
                containing_size: LogicalOptionalSize {
                    inline: Some(content_inline),
                    block: content_block,
                },
                available_size: AlgorithmSize::new(
                    AlgorithmAvailableSpace::Definite(content_inline),
                    content_block.map_or(own_available.height, AlgorithmAvailableSpace::Definite),
                ),
            };
            if child_style.is_out_of_flow() {
                let child_output = self.compute_out_of_flow_block_child(
                    child,
                    BlockChildInput {
                        border_box_inline_size: None,
                        ..default_child_input
                    },
                );
                let child_size = PhysicalSize {
                    width: child_output.size.width,
                    height: child_output.size.height,
                };
                let child_margin_state = self.child_margin_state(
                    child,
                    child_style,
                    child_size,
                    content_inline,
                    child_containing_block_size,
                );
                let static_logical_rect = LogicalRect {
                    inline_start: inline.margin_start,
                    block_start: formatting_context
                        .hypothetical_in_flow_block_start(child_style, child_margin_state),
                    inline_size: style.flow.logical_size(child_size).inline,
                    block_size: style.flow.logical_size(child_size).block,
                };
                let child_padding = child_style.resolved_padding(content_inline);
                let child_border = child_style.border;
                let logical_margin = LogicalSides {
                    inline_start: inline.margin_start,
                    inline_end: inline.margin_end,
                    block_start: 0.0,
                    block_end: 0.0,
                };
                placed_children.push(PendingBlockChildLayout {
                    child,
                    order: order
                        .try_into()
                        .expect("block child order exceeded u32::MAX"),
                    output: child_output,
                    padding: child_padding,
                    border: child_border,
                    margin: style.flow.physical_sides(logical_margin),
                    logical_rect: static_logical_rect,
                    static_position: true,
                });
                continue;
            }
            let mut child_output = if avoids_floats {
                let mut measured_block_size = 0.0;
                let attempts = formatting_context.float_exclusion_count() * 2 + 3;
                let mut measured = None;
                for _ in 0..attempts {
                    let candidate = formatting_context.float_avoiding_placement(
                        child_style,
                        provisional_margin_state,
                        measured_block_size,
                    );
                    // The float band is not represented in Taffy's cache
                    // key, and earlier intrinsic/root probes may have cached
                    // this isolated subtree at the full containing width.
                    self.clear_subtree_cache(child);
                    let output = self.compute_block_child(
                        child,
                        BlockChildInput {
                            border_box_inline_size: Some(candidate.inline_size.border_box),
                            available_size: AlgorithmSize::new(
                                AlgorithmAvailableSpace::Definite(candidate.inline_size.border_box),
                                default_child_input.available_size.height,
                            ),
                            ..default_child_input
                        },
                        None,
                        None,
                    );
                    let child_size = PhysicalSize {
                        width: output.size.width,
                        height: output.size.height,
                    };
                    let actual_block_size = style.flow.logical_size(child_size).block;
                    let final_candidate = formatting_context.float_avoiding_placement(
                        child_style,
                        provisional_margin_state,
                        actual_block_size,
                    );
                    if (final_candidate.inline_size.border_box - candidate.inline_size.border_box)
                        .abs()
                        <= 0.01
                    {
                        float_avoiding_placement = Some(final_candidate);
                        measured = Some(output);
                        break;
                    }
                    measured_block_size = actual_block_size;
                }
                measured.ok_or(BlockDeferral::FloatFormattingContextAvoidance)?
            } else {
                let line_constraints = if child_style.float == FloatSide::None
                    && self.tree.nodes[child.index()].float_line_constraints_enabled
                {
                    let block_start = formatting_context
                        .hypothetical_in_flow_block_start(child_style, provisional_margin_state);
                    formatting_context.float_line_constraints(block_start)
                } else {
                    None
                };
                self.compute_block_child(child, default_child_input, line_constraints, None)
            };
            let shares_float_context = self.shares_parent_float_context(child, child_style);
            if shares_float_context && formatting_context.float_exclusion_count() != 0 {
                let attempts = formatting_context.float_exclusion_count() * 2 + 3;
                let mut converged = false;
                for _ in 0..attempts {
                    let child_size = PhysicalSize {
                        width: child_output.size.width,
                        height: child_output.size.height,
                    };
                    let margin_state = self.child_margin_state(
                        child,
                        child_style,
                        child_size,
                        content_inline,
                        child_containing_block_size,
                    );
                    let origin = Self::predicted_child_content_origin(
                        &formatting_context,
                        child_style,
                        margin_state,
                        inline,
                        content_inline,
                    );
                    let float_state =
                        formatting_context.float_state_for_descendant(origin.0, origin.1);
                    self.clear_subtree_cache(child);
                    let next_output = self.compute_block_child(
                        child,
                        default_child_input,
                        None,
                        Some(float_state),
                    );
                    let next_size = PhysicalSize {
                        width: next_output.size.width,
                        height: next_output.size.height,
                    };
                    let next_margin_state = self.child_margin_state(
                        child,
                        child_style,
                        next_size,
                        content_inline,
                        child_containing_block_size,
                    );
                    let next_origin = Self::predicted_child_content_origin(
                        &formatting_context,
                        child_style,
                        next_margin_state,
                        inline,
                        content_inline,
                    );
                    child_output = next_output;
                    if (next_origin.0 - origin.0).abs() <= 0.01
                        && (next_origin.1 - origin.1).abs() <= 0.01
                    {
                        converged = true;
                        break;
                    }
                }
                if !converged {
                    return Err(BlockDeferral::NestedFloatState);
                }
            }
            let child_size = PhysicalSize {
                width: child_output.size.width,
                height: child_output.size.height,
            };
            let child_margin_state = self.child_margin_state(
                child,
                child_style,
                child_size,
                content_inline,
                child_containing_block_size,
            );
            let placement = if child_style.float != FloatSide::None {
                formatting_context.place_float(child_style, child_size)
            } else {
                if let Some(float_avoiding_placement) = float_avoiding_placement {
                    formatting_context.place_float_avoiding_in_flow(
                        child_style,
                        child_size,
                        child_margin_state,
                        float_avoiding_placement,
                    )
                } else {
                    formatting_context.place_in_flow_with_used_inline(
                        child_style,
                        child_size,
                        child_margin_state,
                        inline,
                    )
                }
            };
            if shares_float_context
                && let Some(float_state) =
                    self.tree.nodes[child.index()].exported_float_state.take()
            {
                let padding_border = child_style.logical_padding_border(content_inline);
                formatting_context.import_descendant_float_state(
                    float_state,
                    placement.logical_rect.inline_start + padding_border.inline_start,
                    placement.logical_rect.block_start + padding_border.block_start,
                );
            }
            let child_padding = child_style.resolved_padding(content_inline);
            let child_border = child_style.border;
            let logical_margin = LogicalSides {
                inline_start: placement.margin_inline_start,
                inline_end: placement.margin_inline_end,
                block_start: 0.0,
                block_end: 0.0,
            };
            let child_margin = style.flow.physical_sides(logical_margin);
            placed_children.push(PendingBlockChildLayout {
                child,
                order: order
                    .try_into()
                    .expect("block child order exceeded u32::MAX"),
                output: child_output,
                padding: child_padding,
                border: child_border,
                margin: child_margin,
                logical_rect: placement.logical_rect,
                static_position: false,
            });
        }

        let all_children_collapse_through = formatting_context.all_children_collapse_through();
        let mut margin_collapse = style.child_margin_collapse(
            containing_inline,
            containing_block_size,
            is_layout_root,
            all_children_collapse_through,
        );
        if !formatting_context.active_margin_may_collapse_with_parent_end() {
            margin_collapse.block_end = false;
        }
        let collapses_through = style.can_collapse_through(
            containing_inline,
            containing_block_size,
            is_layout_root,
            false,
            all_children_collapse_through,
        );
        let collapse_parent_end = margin_collapse.block_end || collapses_through;
        let used_content_block = if is_layout_root || style.establishes_bfc {
            formatting_context.used_block_size_containing_floats(collapse_parent_end)
        } else {
            formatting_context.used_block_size_with_margin_collapse(collapse_parent_end)
        };
        let auto_outer_block = used_content_block
            + content_padding_border.block_start
            + content_padding_border.block_end;
        let (minimum_block, maximum_block) = if style.flow.is_horizontal() {
            (style.min_size.height, style.max_size.height)
        } else {
            (style.min_size.width, style.max_size.width)
        };
        let final_block = outer_logical.block.unwrap_or_else(|| {
            clamp_outer_dimension(
                auto_outer_block,
                minimum_block,
                maximum_block,
                own_parent.block,
                content_padding_border.block_start + content_padding_border.block_end,
                style.box_sizing,
            )
        });
        let final_size = style.flow.physical_size(LogicalSize {
            inline: outer_logical
                .inline
                .expect("an owned block has a definite used inline size"),
            block: final_block,
        });
        let final_content_box = style.flow.physical_size(LogicalSize {
            inline: content_inline,
            block: (final_block
                - content_padding_border.block_start
                - content_padding_border.block_end)
                .max(0.0),
        });
        for child in placed_children {
            let rect = style
                .flow
                .physical_rect(child.logical_rect, final_content_box);
            let mut child_layout = Layout::with_order(child.order);
            child_layout.location = taffy::Point {
                x: padding_border.left + rect.x,
                y: padding_border.top + rect.y,
            };
            if child.static_position {
                child_layout.static_location = child_layout.location;
            }
            child_layout.size = child.output.size;
            child_layout.scrollbar_size = taffy::Size::ZERO;
            child_layout.padding = to_taffy_rect(child.padding);
            child_layout.border = to_taffy_rect(child.border);
            child_layout.margin = to_taffy_rect(child.margin);
            self.set_unrounded_layout(child.child.into_taffy(), &child_layout);
        }
        let final_size = taffy::Size {
            width: final_size.width,
            height: final_size.height,
        };
        self.tree.nodes[node_index].block_margins = Some(BlockMarginState::from_box(
            style,
            containing_inline,
            formatting_context.first_child_margin(),
            formatting_context.last_child_margin(),
            margin_collapse,
            collapses_through,
        ));
        if !style.establishes_bfc {
            self.tree.nodes[node_index].exported_float_state =
                Some(formatting_context.exported_float_state());
        }
        Ok(LayoutOutput::from_outer_size(final_size))
    }

    fn compute_node(
        &mut self,
        node_id: NodeId,
        inputs: LayoutInput,
        block_context: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        if inputs.run_mode == RunMode::PerformHiddenLayout {
            return compute_hidden_layout(self, node_id);
        }

        compute_cached_layout(self, node_id, inputs, |tree, node_id, inputs| {
            let node_index = AlgorithmNodeId::from_taffy(node_id).index();
            let kind = tree.tree.nodes[node_index].kind;

            match kind {
                AlgorithmKind::Hidden => compute_hidden_layout(tree, node_id),
                AlgorithmKind::Block if block_context.is_none() => {
                    match tree.compute_owned_block_layout(node_id, inputs) {
                        Ok(output) => {
                            tree.tree.nodes[node_index].block_algorithm =
                                Some(BlockAlgorithm::Buckram);
                            tree.tree.nodes[node_index].block_deferral = None;
                            output
                        },
                        Err(deferral) => {
                            tree.tree.nodes[node_index].block_algorithm =
                                Some(BlockAlgorithm::Taffy);
                            tree.tree.nodes[node_index].block_deferral = Some(deferral);
                            tree.tree.nodes[node_index].block_margins = None;
                            tree.with_out_of_flow_children_excluded(node_id, |tree| {
                                compute_block_layout(tree, node_id, inputs, None)
                            })
                        },
                    }
                },
                AlgorithmKind::Block => {
                    tree.tree.nodes[node_index].block_algorithm = Some(BlockAlgorithm::Taffy);
                    tree.tree.nodes[node_index].block_deferral =
                        Some(BlockDeferral::BackendSizingMode);
                    tree.tree.nodes[node_index].block_margins = None;
                    tree.with_out_of_flow_children_excluded(node_id, |tree| {
                        compute_block_layout(tree, node_id, inputs, block_context)
                    })
                },
                AlgorithmKind::Flex => compute_flexbox_layout(tree, node_id, inputs),
                AlgorithmKind::Grid => compute_grid_layout(tree, node_id, inputs),
                // Buckram owns table layout outright. The caller computed the
                // grid and every cell rectangle before this walk began and
                // wrote them through `set_layout`, so this arm reports the
                // size it was given and deliberately does not lay out
                // children: recursing would overwrite the table algorithm's
                // own result with a backend guess.
                AlgorithmKind::Table => {
                    let size = tree.tree.nodes[node_index].final_layout.size;
                    LayoutOutput::from_outer_size(size)
                },
                AlgorithmKind::Leaf => {
                    let node = &mut tree.tree.nodes[node_index];
                    let style = sealed::AlgorithmStyle::as_taffy_style(&node.style);
                    let context = node.context.as_mut();
                    let measure = &mut tree.measure;
                    let line_constraints = tree.line_constraints.as_ref();
                    compute_leaf_layout(
                        inputs,
                        style,
                        |_, _| 0.0,
                        |known, available| {
                            let measured = measure(
                                AlgorithmSize::new(known.width, known.height),
                                AlgorithmSize::new(
                                    from_taffy_available(available.width),
                                    from_taffy_available(available.height),
                                ),
                                AlgorithmNodeId::from_taffy(node_id),
                                context,
                                line_constraints,
                            );
                            taffy::Size {
                                width: measured.width,
                                height: measured.height,
                            }
                        },
                    )
                },
            }
        })
    }
}

fn intrinsic_inline_style_is_admitted(style: BlockStyle, is_root: bool) -> bool {
    if style.flow != style.containing_flow
        || style.position != crate::BlockPosition::Static
        || style.replaced
        || style.aspect_ratio.is_some()
        || style.size_containment.width
        || style.size_containment.height
        || style.has_nonlinear_lengths
    {
        return false;
    }
    if !is_root && (style.float != FloatSide::None || style.shrink_to_fit) {
        return false;
    }

    let inline_size = intrinsic_inline_dimension(style.size, style.containing_flow);
    let inline_min_size = intrinsic_inline_dimension(style.min_size, style.containing_flow);
    let inline_max_size = intrinsic_inline_dimension(style.max_size, style.containing_flow);
    let child_size_is_supported = matches!(inline_size, BlockSizeValue::Auto)
        || intrinsic_absolute_size(inline_size).is_some();
    if !child_size_is_supported {
        return false;
    }
    if !is_root
        && (!matches!(inline_min_size, BlockSizeValue::Auto)
            || !matches!(inline_max_size, BlockSizeValue::None))
    {
        return false;
    }
    if is_root
        && (!intrinsic_size_constraint_is_supported(inline_min_size, true)
            || !intrinsic_size_constraint_is_supported(inline_max_size, false))
    {
        return false;
    }

    let (padding_start, padding_end) = intrinsic_inline_padding(style);
    let (margin_start, margin_end) = intrinsic_inline_margin_values(style);
    intrinsic_absolute_length(padding_start).is_some()
        && intrinsic_absolute_length(padding_end).is_some()
        && intrinsic_auto_length(margin_start).is_some()
        && intrinsic_auto_length(margin_end).is_some()
}

fn intrinsic_inline_dimension<T: Copy>(dimensions: crate::BlockDimensions<T>, flow: FlowAxes) -> T {
    if flow.is_horizontal() {
        dimensions.width
    } else {
        dimensions.height
    }
}

fn intrinsic_size_constraint_is_supported(value: BlockSizeValue, minimum: bool) -> bool {
    match value {
        BlockSizeValue::Auto if minimum => true,
        BlockSizeValue::None if !minimum => true,
        BlockSizeValue::Length(_) => true,
        _ => false,
    }
}

fn intrinsic_absolute_size(value: BlockSizeValue) -> Option<FlowLength> {
    match value {
        BlockSizeValue::Length(value) if value.percentage == 0.0 && value.px.is_finite() => {
            Some(value)
        },
        _ => None,
    }
}

fn intrinsic_absolute_length(value: FlowLength) -> Option<f32> {
    (value.percentage == 0.0 && value.px.is_finite()).then_some(value.px)
}

fn intrinsic_auto_length(value: FlowLengthAuto) -> Option<f32> {
    match value {
        FlowLengthAuto::Auto => Some(0.0),
        FlowLengthAuto::Value(value) => intrinsic_absolute_length(value),
    }
}

fn intrinsic_inline_padding(style: BlockStyle) -> (FlowLength, FlowLength) {
    if style.containing_flow.is_horizontal() {
        (style.padding.left, style.padding.right)
    } else {
        (style.padding.top, style.padding.bottom)
    }
}

fn intrinsic_inline_margin_values(style: BlockStyle) -> (FlowLengthAuto, FlowLengthAuto) {
    if style.containing_flow.is_horizontal() {
        (style.margin.left, style.margin.right)
    } else {
        (style.margin.top, style.margin.bottom)
    }
}

fn intrinsic_inline_padding_border(style: BlockStyle) -> Result<(f32, f32), BlockDeferral> {
    let (padding_start, padding_end) = intrinsic_inline_padding(style);
    let padding_start =
        intrinsic_absolute_length(padding_start).ok_or(BlockDeferral::IntrinsicSize)?;
    let padding_end = intrinsic_absolute_length(padding_end).ok_or(BlockDeferral::IntrinsicSize)?;
    let (border_start, border_end) = if style.containing_flow.is_horizontal() {
        (style.border.left, style.border.right)
    } else {
        (style.border.top, style.border.bottom)
    };
    Ok((padding_start + border_start, padding_end + border_end))
}

fn intrinsic_inline_margins(style: BlockStyle) -> Result<(f32, f32), BlockDeferral> {
    let (margin_start, margin_end) = intrinsic_inline_margin_values(style);
    Ok((
        intrinsic_auto_length(margin_start).ok_or(BlockDeferral::IntrinsicSize)?,
        intrinsic_auto_length(margin_end).ok_or(BlockDeferral::IntrinsicSize)?,
    ))
}

fn intrinsic_definite_inline_content_size(style: BlockStyle) -> Result<Option<f32>, BlockDeferral> {
    let size = intrinsic_inline_dimension(style.size, style.containing_flow);
    let Some(size) = intrinsic_absolute_size(size) else {
        return match size {
            BlockSizeValue::Auto => Ok(None),
            _ => Err(BlockDeferral::IntrinsicSize),
        };
    };
    let (padding_border_start, padding_border_end) = intrinsic_inline_padding_border(style)?;
    let padding_border = padding_border_start + padding_border_end;
    let content_size = match style.box_sizing {
        BlockBoxSizing::ContentBox => size.px,
        BlockBoxSizing::BorderBox => (size.px - padding_border).max(0.0),
    };
    Ok(Some(content_size))
}

impl<S, Context, Source, Measure> TraversePartialTree
    for AlgorithmRun<'_, S, Context, Source, Measure>
where
    S: AlgorithmStyle,
{
    type ChildIter<'a>
        = ChildIter<'a>
    where
        Self: 'a;

    fn child_ids(&self, parent_node_id: NodeId) -> Self::ChildIter<'_> {
        ChildIter(
            self.tree.nodes[AlgorithmNodeId::from_taffy(parent_node_id).index()]
                .children
                .iter(),
        )
    }

    fn child_count(&self, parent_node_id: NodeId) -> usize {
        self.tree.nodes[AlgorithmNodeId::from_taffy(parent_node_id).index()]
            .children
            .len()
    }

    fn get_child_id(&self, parent_node_id: NodeId, child_index: usize) -> NodeId {
        self.tree.nodes[AlgorithmNodeId::from_taffy(parent_node_id).index()].children[child_index]
            .into_taffy()
    }
}

impl<S, Context, Source, Measure> TraverseTree for AlgorithmRun<'_, S, Context, Source, Measure> where
    S: AlgorithmStyle
{
}

impl<S, Context, Source, Measure> LayoutPartialTree
    for AlgorithmRun<'_, S, Context, Source, Measure>
where
    S: AlgorithmStyle,
    Measure: FnMut(
        AlgorithmSize<Option<f32>>,
        AlgorithmSize<AlgorithmAvailableSpace>,
        AlgorithmNodeId,
        Option<&mut Context>,
        Option<&FloatLineConstraints>,
    ) -> AlgorithmSize<f32>,
{
    type CoreContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type CustomIdent = String;

    fn get_core_container_style(&self, node_id: NodeId) -> Self::CoreContainerStyle<'_> {
        self.style(node_id)
    }

    fn set_unrounded_layout(&mut self, node_id: NodeId, layout: &Layout) {
        self.tree.nodes[AlgorithmNodeId::from_taffy(node_id).index()].unrounded_layout = *layout;
    }

    fn resolve_calc_value(&self, _value: *const (), _basis: f32) -> f32 {
        0.0
    }

    fn compute_child_layout(&mut self, node_id: NodeId, inputs: LayoutInput) -> LayoutOutput {
        self.compute_node(node_id, inputs, None)
    }
}

impl<S, Context, Source, Measure> CacheTree for AlgorithmRun<'_, S, Context, Source, Measure>
where
    S: AlgorithmStyle,
{
    fn cache_get(&self, node_id: NodeId, input: &LayoutInput) -> Option<LayoutOutput> {
        self.tree.nodes[AlgorithmNodeId::from_taffy(node_id).index()]
            .cache
            .get(input)
    }

    fn cache_store(&mut self, node_id: NodeId, input: &LayoutInput, output: LayoutOutput) {
        self.tree.nodes[AlgorithmNodeId::from_taffy(node_id).index()]
            .cache
            .store(input, output);
    }

    fn cache_clear(&mut self, node_id: NodeId) {
        self.tree.nodes[AlgorithmNodeId::from_taffy(node_id).index()]
            .cache
            .clear();
    }
}

impl<S, Context, Source, Measure> LayoutBlockContainer
    for AlgorithmRun<'_, S, Context, Source, Measure>
where
    S: AlgorithmStyle,
    Measure: FnMut(
        AlgorithmSize<Option<f32>>,
        AlgorithmSize<AlgorithmAvailableSpace>,
        AlgorithmNodeId,
        Option<&mut Context>,
        Option<&FloatLineConstraints>,
    ) -> AlgorithmSize<f32>,
{
    type BlockContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type BlockItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_block_container_style(&self, node_id: NodeId) -> Self::BlockContainerStyle<'_> {
        self.style(node_id)
    }

    fn get_block_child_style(&self, child_node_id: NodeId) -> Self::BlockItemStyle<'_> {
        self.style(child_node_id)
    }

    fn compute_block_child_layout(
        &mut self,
        node_id: NodeId,
        inputs: LayoutInput,
        block_context: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        self.compute_node(node_id, inputs, block_context)
    }
}

impl<S, Context, Source, Measure> LayoutFlexboxContainer
    for AlgorithmRun<'_, S, Context, Source, Measure>
where
    S: AlgorithmStyle,
    Measure: FnMut(
        AlgorithmSize<Option<f32>>,
        AlgorithmSize<AlgorithmAvailableSpace>,
        AlgorithmNodeId,
        Option<&mut Context>,
        Option<&FloatLineConstraints>,
    ) -> AlgorithmSize<f32>,
{
    type FlexboxContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type FlexboxItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_flexbox_container_style(&self, node_id: NodeId) -> Self::FlexboxContainerStyle<'_> {
        self.style(node_id)
    }

    fn get_flexbox_child_style(&self, child_node_id: NodeId) -> Self::FlexboxItemStyle<'_> {
        self.style(child_node_id)
    }
}

impl<S, Context, Source, Measure> LayoutGridContainer
    for AlgorithmRun<'_, S, Context, Source, Measure>
where
    S: AlgorithmStyle,
    Measure: FnMut(
        AlgorithmSize<Option<f32>>,
        AlgorithmSize<AlgorithmAvailableSpace>,
        AlgorithmNodeId,
        Option<&mut Context>,
        Option<&FloatLineConstraints>,
    ) -> AlgorithmSize<f32>,
{
    type GridContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type GridItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_grid_container_style(&self, node_id: NodeId) -> Self::GridContainerStyle<'_> {
        self.style(node_id)
    }

    fn get_grid_child_style(&self, child_node_id: NodeId) -> Self::GridItemStyle<'_> {
        self.style(child_node_id)
    }

    fn grid_child_static_position_area(
        &self,
        container_node_id: NodeId,
        child_node_id: NodeId,
        grid_area: taffy::geometry::Rect<f32>,
        content_box: taffy::geometry::Rect<f32>,
        grid_area_auto: taffy::geometry::Rect<bool>,
        container_border: taffy::geometry::Rect<f32>,
        container_border_box: taffy::Size<f32>,
    ) -> taffy::geometry::Rect<f32> {
        // CSS Grid §9.2 aligns the direct absolute child as the sole grid item
        // in an area formed by the grid container's content edges, unless the
        // grid container also generates the child's containing block; then
        // the §9.1 grid area applies, an `auto` line being the padding edge.
        // The renderer records that K5a relationship through
        // `use_grid_area_for_static_position`. Either way the placement area
        // stays available to the containing-block route in
        // `grid_positioned_area`.
        if self.tree.nodes[AlgorithmNodeId::from_taffy(child_node_id).index()]
            .grid_static_position_uses_grid_area
        {
            let container =
                &self.tree.nodes[AlgorithmNodeId::from_taffy(container_node_id).index()];
            let container_size = PhysicalSize {
                width: container_border_box.width,
                height: container_border_box.height,
            };
            let logical_padding_box = container.block_style.flow.logical_rect(
                PhysicalRect {
                    x: container_border.left,
                    y: container_border.top,
                    width: (container_border_box.width
                        - container_border.left
                        - container_border.right)
                        .max(0.0),
                    height: (container_border_box.height
                        - container_border.top
                        - container_border.bottom)
                        .max(0.0),
                },
                container_size,
            );
            let inline_start = if grid_area_auto.left {
                logical_padding_box.inline_start
            } else {
                grid_area.left
            };
            let inline_end = if grid_area_auto.right {
                logical_padding_box.inline_start + logical_padding_box.inline_size
            } else {
                grid_area.right
            };
            let block_start = if grid_area_auto.top {
                logical_padding_box.block_start
            } else {
                grid_area.top
            };
            let block_end = if grid_area_auto.bottom {
                logical_padding_box.block_start + logical_padding_box.block_size
            } else {
                grid_area.bottom
            };
            let track_area = LogicalRect {
                inline_start,
                block_start,
                inline_size: (inline_end - inline_start).max(0.0),
                block_size: (block_end - block_start).max(0.0),
            };
            let physical = container
                .block_style
                .flow
                .physical_rect(track_area, container_size);
            taffy::geometry::Rect {
                left: physical.x,
                right: physical.x + physical.width,
                top: physical.y,
                bottom: physical.y + physical.height,
            }
        } else {
            content_box
        }
    }

    fn set_detailed_grid_info(&mut self, node_id: NodeId, detailed_grid_info: DetailedGridInfo) {
        self.tree.nodes[AlgorithmNodeId::from_taffy(node_id).index()].grid_info =
            Some(detailed_grid_info);
    }
}

impl<S, Context, Source, Measure> RoundTree for AlgorithmRun<'_, S, Context, Source, Measure>
where
    S: AlgorithmStyle,
{
    fn get_unrounded_layout(&self, node_id: NodeId) -> Layout {
        self.tree.nodes[AlgorithmNodeId::from_taffy(node_id).index()].unrounded_layout
    }

    fn set_final_layout(&mut self, node_id: NodeId, layout: &Layout) {
        self.tree.nodes[AlgorithmNodeId::from_taffy(node_id).index()].final_layout = *layout;
    }
}

#[cfg(test)]
mod tests {
    use taffy::{
        geometry::Rect,
        prelude::{Dimension, Display, Position, Style, fr, length, line},
        style::{AlignItems, JustifyContent},
    };

    use super::*;

    fn available(width: f32, height: f32) -> AlgorithmSize<AlgorithmAvailableSpace> {
        AlgorithmSize::new(
            AlgorithmAvailableSpace::Definite(width),
            AlgorithmAvailableSpace::Definite(height),
        )
    }

    fn zero_measure(
        _known: AlgorithmSize<Option<f32>>,
        _available: AlgorithmSize<AlgorithmAvailableSpace>,
        _node: AlgorithmNodeId,
        _context: Option<&mut ()>,
        _line_constraints: Option<&FloatLineConstraints>,
    ) -> AlgorithmSize<f32> {
        AlgorithmSize::new(0.0, 0.0)
    }

    #[test]
    fn flex_dispatch_preserves_exact_placements_and_sources() {
        let mut tree = AlgorithmTree::<Style, (), &str>::new();
        let child_style = Style {
            size: taffy::Size {
                width: Dimension::length(50.0),
                height: Dimension::length(20.0),
            },
            ..Style::default()
        };
        let first = tree.new_with_children(AlgorithmKind::Leaf, child_style.clone(), &[], "first");
        let second = tree.new_with_children(AlgorithmKind::Leaf, child_style, &[], "second");
        let root = tree.new_with_children(
            AlgorithmKind::Flex,
            Style {
                display: Display::Flex,
                size: taffy::Size {
                    width: Dimension::length(200.0),
                    height: Dimension::length(40.0),
                },
                ..Style::default()
            },
            &[first, second],
            "root",
        );

        tree.compute_layout_with_measure(root, available(200.0, 40.0), zero_measure);

        assert_eq!(tree.source(second), &"second");
        assert_eq!(
            tree.layout(first),
            AlgorithmLayout {
                width: 50.0,
                height: 20.0,
                ..AlgorithmLayout::default()
            }
        );
        assert_eq!(
            tree.layout(second),
            AlgorithmLayout {
                x: 50.0,
                width: 50.0,
                height: 20.0,
                ..AlgorithmLayout::default()
            }
        );
    }

    #[test]
    fn buckram_block_parent_keeps_absolute_child_at_its_static_position() {
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let positioned = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                position: crate::BlockPosition::Absolute,
                ..BlockStyle::default()
            },
            Style {
                position: Position::Absolute,
                inset: Rect {
                    left: length(40.0_f32),
                    top: length(12.0_f32),
                    ..Rect::auto()
                },
                size: taffy::Size {
                    width: Dimension::length(30.0),
                    height: Dimension::length(20.0),
                },
                ..Style::default()
            },
            &[],
            1,
        );
        let root = tree.new_with_children(
            AlgorithmKind::Block,
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::length(200.0),
                    height: Dimension::length(100.0),
                },
                ..Style::default()
            },
            &[positioned],
            0,
        );

        tree.compute_layout_with_measure(root, available(200.0, 100.0), zero_measure);

        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Buckram));
        assert_eq!(tree.layout(positioned).x, 0.0);
        assert_eq!(tree.layout(positioned).y, 0.0);
        assert_eq!(tree.static_layout(positioned).x, 0.0);
        assert_eq!(tree.static_layout(positioned).y, 0.0);
    }

    #[test]
    fn positioned_empty_leaf_has_zero_intrinsic_inline_contributions() {
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let positioned = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                position: crate::BlockPosition::Absolute,
                ..BlockStyle::default()
            },
            Style {
                position: Position::Absolute,
                ..Style::default()
            },
            &[],
            1,
        );

        assert_eq!(
            tree.positioned_intrinsic_inline_sizes(positioned, zero_measure),
            IntrinsicSizes::new(0.0, 0.0),
        );
    }

    #[test]
    fn vertical_positioned_block_intrinsic_and_inline_size_follow_height() {
        use crate::{Direction, WritingMode};

        let vertical = FlowAxes::new(WritingMode::VerticalRl, Direction::Ltr);
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let child = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                flow: vertical,
                containing_flow: vertical,
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Auto,
                    BlockSizeValue::Length(FlowLength::px(20.0)),
                ),
                ..BlockStyle::default()
            },
            Style::default(),
            &[],
            2,
        );
        let positioned = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                flow: vertical,
                containing_flow: vertical,
                position: crate::BlockPosition::Absolute,
                ..BlockStyle::default()
            },
            Style::default(),
            &[child],
            1,
        );

        assert_eq!(
            tree.positioned_intrinsic_inline_sizes(positioned, zero_measure),
            IntrinsicSizes::new(20.0, 20.0),
        );

        tree.set_positioned_inline_size(positioned, 170.0);
        assert_eq!(
            tree.block_style(positioned).size.height,
            BlockSizeValue::Length(FlowLength::px(170.0)),
        );
    }

    #[test]
    fn grid_static_layout_uses_content_box_before_insets() {
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let positioned = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                position: crate::BlockPosition::Absolute,
                ..BlockStyle::default()
            },
            Style {
                inset: Rect {
                    left: length(18.0_f32),
                    top: length(9.0_f32),
                    ..Rect::auto()
                },
                size: taffy::Size {
                    width: Dimension::length(30.0),
                    height: Dimension::length(20.0),
                },
                grid_column: taffy::Line {
                    start: line(2),
                    end: line(3),
                },
                grid_row: taffy::Line {
                    start: line(2),
                    end: line(3),
                },
                ..Style::default()
            },
            &[],
            1,
        );
        let root = tree.new_with_children(
            AlgorithmKind::Grid,
            Style {
                display: Display::Grid,
                size: taffy::Size {
                    width: Dimension::length(200.0),
                    height: Dimension::length(100.0),
                },
                grid_template_columns: vec![length(80.0_f32), length(120.0_f32)],
                grid_template_rows: vec![length(40.0_f32), length(60.0_f32)],
                ..Style::default()
            },
            &[positioned],
            0,
        );
        tree.enable_flex_grid_static_position_provider(positioned);

        tree.compute_layout_with_measure(root, available(200.0, 100.0), zero_measure);

        assert_eq!(tree.layout(positioned).x, 98.0);
        assert_eq!(tree.layout(positioned).y, 49.0);
        assert_eq!(tree.static_layout(positioned).x, 0.0);
        assert_eq!(tree.static_layout(positioned).y, 0.0);
        assert_eq!(
            tree.grid_positioned_area(positioned),
            Some(PhysicalRect {
                x: 80.0,
                y: 40.0,
                width: 120.0,
                height: 60.0,
            }),
            "the grid area remains available to the positioned containing-block route"
        );
    }

    #[test]
    fn grid_static_layout_uses_the_grid_area_when_the_grid_is_the_containing_block() {
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let positioned = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                position: crate::BlockPosition::Absolute,
                ..BlockStyle::default()
            },
            Style {
                size: taffy::Size {
                    width: Dimension::length(30.0),
                    height: Dimension::length(20.0),
                },
                grid_column: taffy::Line {
                    start: line(2),
                    end: line(3),
                },
                grid_row: taffy::Line {
                    start: line(2),
                    end: line(3),
                },
                ..Style::default()
            },
            &[],
            1,
        );
        let root = tree.new_with_children(
            AlgorithmKind::Grid,
            Style {
                display: Display::Grid,
                size: taffy::Size {
                    width: Dimension::length(200.0),
                    height: Dimension::length(100.0),
                },
                padding: Rect {
                    left: length(10.0_f32),
                    right: length(30.0_f32),
                    top: length(5.0_f32),
                    bottom: length(25.0_f32),
                },
                grid_template_columns: vec![length(80.0_f32), length(80.0_f32)],
                grid_template_rows: vec![length(40.0_f32), length(30.0_f32)],
                ..Style::default()
            },
            &[positioned],
            0,
        );
        tree.enable_flex_grid_static_position_provider(positioned);
        tree.use_grid_area_for_static_position(positioned);

        tree.compute_layout_with_measure(root, available(200.0, 100.0), zero_measure);

        // Column 2 starts at padding 10 + 80 and row 2 at padding 5 + 40: the
        // static location is the start of the placed area, not the content
        // origin (10, 5) the unselected provider would report.
        assert_eq!(
            (
                tree.static_layout(positioned).x,
                tree.static_layout(positioned).y
            ),
            (90.0, 45.0)
        );
        assert_eq!(
            tree.grid_positioned_area(positioned),
            Some(PhysicalRect {
                x: 90.0,
                y: 45.0,
                width: 80.0,
                height: 30.0,
            })
        );
    }

    #[test]
    fn grid_static_layout_auto_lines_align_in_the_padding_box_when_selected() {
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let positioned = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                position: crate::BlockPosition::Absolute,
                ..BlockStyle::default()
            },
            Style {
                size: taffy::Size {
                    width: Dimension::length(30.0),
                    height: Dimension::length(20.0),
                },
                align_self: Some(AlignItems::CENTER),
                justify_self: Some(AlignItems::CENTER),
                ..Style::default()
            },
            &[],
            1,
        );
        let root = tree.new_with_children(
            AlgorithmKind::Grid,
            Style {
                display: Display::Grid,
                size: taffy::Size {
                    width: Dimension::length(200.0),
                    height: Dimension::length(100.0),
                },
                padding: Rect {
                    left: length(10.0_f32),
                    right: length(30.0_f32),
                    top: length(5.0_f32),
                    bottom: length(25.0_f32),
                },
                ..Style::default()
            },
            &[positioned],
            0,
        );
        tree.enable_flex_grid_static_position_provider(positioned);
        tree.use_grid_area_for_static_position(positioned);

        tree.compute_layout_with_measure(root, available(200.0, 100.0), zero_measure);

        // `auto` lines bound the area at the padding edges, so centering uses
        // the 200 x 100 padding box: (85, 40). Centering in the asymmetric
        // 160 x 70 content box would give (75, 30).
        assert_eq!(
            (
                tree.static_layout(positioned).x,
                tree.static_layout(positioned).y
            ),
            (85.0, 40.0)
        );
    }

    #[test]
    fn grid_positioned_area_keeps_the_final_explicit_track_rectangle() {
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let positioned = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                position: crate::BlockPosition::Absolute,
                ..BlockStyle::default()
            },
            Style {
                position: Position::Absolute,
                grid_column: taffy::Line {
                    start: line(2),
                    end: line(3),
                },
                grid_row: taffy::Line {
                    start: line(2),
                    end: line(3),
                },
                ..Style::default()
            },
            &[],
            1,
        );
        let root = tree.new_with_children(
            AlgorithmKind::Grid,
            Style {
                display: Display::Grid,
                size: taffy::Size {
                    width: Dimension::length(500.0),
                    height: Dimension::length(250.0),
                },
                grid_template_columns: vec![length(200.0_f32), length(300.0_f32)],
                grid_template_rows: vec![length(150.0_f32), length(100.0_f32)],
                ..Style::default()
            },
            &[positioned],
            0,
        );
        tree.enable_flex_grid_static_position_provider(positioned);

        tree.compute_layout_with_measure(root, available(500.0, 250.0), zero_measure);

        assert_eq!(
            tree.grid_positioned_area(positioned),
            Some(PhysicalRect {
                x: 200.0,
                y: 150.0,
                width: 300.0,
                height: 100.0,
            })
        );
    }

    #[test]
    fn grid_positioned_area_projects_track_axes_through_container_flow() {
        use crate::{Direction, WritingMode};

        for (flow, expected) in [
            (
                FlowAxes::new(WritingMode::VerticalRl, Direction::Ltr),
                PhysicalRect {
                    x: 0.0,
                    y: 20.0,
                    width: 70.0,
                    height: 60.0,
                },
            ),
            (
                FlowAxes::new(WritingMode::VerticalLr, Direction::Ltr),
                PhysicalRect {
                    x: 30.0,
                    y: 20.0,
                    width: 70.0,
                    height: 60.0,
                },
            ),
            (
                FlowAxes::new(WritingMode::VerticalRl, Direction::Rtl),
                PhysicalRect {
                    x: 0.0,
                    y: 0.0,
                    width: 70.0,
                    height: 60.0,
                },
            ),
            (
                FlowAxes::new(WritingMode::VerticalLr, Direction::Rtl),
                PhysicalRect {
                    x: 30.0,
                    y: 0.0,
                    width: 70.0,
                    height: 60.0,
                },
            ),
        ] {
            let mut tree = AlgorithmTree::<Style, (), u8>::new();
            let positioned = tree.new_with_children_and_block_style(
                AlgorithmKind::Leaf,
                BlockStyle {
                    position: crate::BlockPosition::Absolute,
                    ..BlockStyle::default()
                },
                Style {
                    position: Position::Absolute,
                    grid_column: taffy::Line {
                        start: line(2),
                        end: line(3),
                    },
                    grid_row: taffy::Line {
                        start: line(2),
                        end: line(3),
                    },
                    ..Style::default()
                },
                &[],
                1,
            );
            let root = tree.new_with_children_and_block_style(
                AlgorithmKind::Grid,
                BlockStyle {
                    flow,
                    containing_flow: flow,
                    establishes_bfc: true,
                    ..BlockStyle::default()
                },
                Style {
                    display: Display::Grid,
                    size: taffy::Size {
                        width: Dimension::length(100.0),
                        height: Dimension::length(80.0),
                    },
                    grid_template_columns: vec![length(20.0_f32), length(60.0_f32)],
                    grid_template_rows: vec![length(30.0_f32), length(70.0_f32)],
                    ..Style::default()
                },
                &[positioned],
                0,
            );
            tree.enable_flex_grid_static_position_provider(positioned);
            tree.compute_layout_with_measure(root, available(100.0, 80.0), zero_measure);

            assert_eq!(
                tree.grid_positioned_area(positioned),
                Some(expected),
                "{flow:?}: track coordinates project through the grid flow"
            );
        }
    }

    #[test]
    fn flex_static_layout_keeps_alignment_when_insets_place_the_item() {
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let positioned = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                position: crate::BlockPosition::Absolute,
                ..BlockStyle::default()
            },
            Style {
                inset: Rect {
                    left: length(18.0_f32),
                    top: length(9.0_f32),
                    ..Rect::auto()
                },
                size: taffy::Size {
                    width: Dimension::length(30.0),
                    height: Dimension::length(20.0),
                },
                ..Style::default()
            },
            &[],
            1,
        );
        let root = tree.new_with_children(
            AlgorithmKind::Flex,
            Style {
                display: Display::Flex,
                size: taffy::Size {
                    width: Dimension::length(200.0),
                    height: Dimension::length(100.0),
                },
                justify_content: Some(JustifyContent::CENTER),
                align_items: Some(AlignItems::END),
                ..Style::default()
            },
            &[positioned],
            0,
        );
        tree.enable_flex_grid_static_position_provider(positioned);

        tree.compute_layout_with_measure(root, available(200.0, 100.0), zero_measure);

        assert_eq!(
            (tree.layout(positioned).x, tree.layout(positioned).y),
            (18.0, 9.0)
        );
        assert_eq!(
            (
                tree.static_layout(positioned).x,
                tree.static_layout(positioned).y
            ),
            (85.0, 80.0)
        );
    }

    #[test]
    fn grid_dispatch_preserves_exact_track_geometry() {
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let children = (0..4)
            .map(|source| {
                tree.new_with_children(AlgorithmKind::Leaf, Style::default(), &[], source)
            })
            .collect::<Vec<_>>();
        let root = tree.new_with_children(
            AlgorithmKind::Grid,
            Style {
                display: Display::Grid,
                size: taffy::Size {
                    width: Dimension::length(200.0),
                    height: Dimension::length(100.0),
                },
                grid_template_columns: vec![length(100.0_f32), fr(1.0_f32)],
                grid_template_rows: vec![length(50.0_f32), length(50.0_f32)],
                ..Style::default()
            },
            &children,
            9,
        );

        tree.compute_layout_with_measure(root, available(200.0, 100.0), zero_measure);

        assert_eq!(
            (tree.layout(children[0]).x, tree.layout(children[0]).y),
            (0.0, 0.0)
        );
        assert_eq!(
            (tree.layout(children[1]).x, tree.layout(children[1]).y),
            (100.0, 0.0)
        );
        assert_eq!(
            (tree.layout(children[2]).x, tree.layout(children[2]).y),
            (0.0, 50.0)
        );
        assert_eq!(
            (tree.layout(children[3]).x, tree.layout(children[3]).y),
            (100.0, 50.0)
        );
    }

    #[test]
    fn buckram_kind_selects_the_algorithm_independently_of_backend_display() {
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let child_style = Style {
            size: taffy::Size {
                width: Dimension::length(50.0),
                height: Dimension::length(20.0),
            },
            ..Style::default()
        };
        let first = tree.new_with_children(AlgorithmKind::Leaf, child_style.clone(), &[], 1);
        let second = tree.new_with_children(AlgorithmKind::Leaf, child_style, &[], 2);
        let root = tree.new_with_children(
            AlgorithmKind::Flex,
            Style {
                // Deliberately contradictory. Dispatch must follow Buckram's
                // formatting role, not this private backend field.
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::length(200.0),
                    height: Dimension::length(40.0),
                },
                ..Style::default()
            },
            &[first, second],
            0,
        );

        tree.compute_layout_with_measure(root, available(200.0, 40.0), zero_measure);

        assert_eq!(tree.kind(root), AlgorithmKind::Flex);
        assert_eq!(tree.layout(first).x, 0.0);
        assert_eq!(tree.layout(second).x, 50.0);
        assert_eq!(tree.layout(second).y, 0.0);
    }

    #[test]
    fn buckram_block_flow_uses_css_inputs_instead_of_backend_sizes() {
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let backend_child_style = Style {
            size: taffy::Size {
                // Deliberately contradictory: Buckram's CSS input says 80px.
                width: Dimension::length(10.0),
                height: Dimension::length(20.0),
            },
            ..Style::default()
        };
        let block_child_style = BlockStyle {
            size: crate::BlockDimensions::new(
                BlockSizeValue::Length(crate::FlowLength::px(80.0)),
                BlockSizeValue::Length(crate::FlowLength::px(20.0)),
            ),
            ..BlockStyle::default()
        };
        let first = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            block_child_style,
            backend_child_style.clone(),
            &[],
            1,
        );
        let second = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            block_child_style,
            backend_child_style,
            &[],
            2,
        );
        let root = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle::default(),
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::length(200.0),
                    height: Dimension::auto(),
                },
                ..Style::default()
            },
            &[first, second],
            0,
        );

        tree.compute_layout_with_measure(root, available(200.0, 100.0), zero_measure);

        assert_eq!(tree.layout(first).width, 80.0);
        assert_eq!(tree.layout(second).width, 80.0);
        assert_eq!(tree.layout(first).y, 0.0);
        assert_eq!(tree.layout(second).y, 20.0);
        assert_eq!(tree.layout(root).height, 40.0);
        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Buckram));
    }

    #[test]
    fn buckram_block_flow_propagates_parent_child_and_empty_margin_chains() {
        fn block_style(height: f32, margin_top: f32, margin_bottom: f32) -> BlockStyle {
            BlockStyle {
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Auto,
                    BlockSizeValue::Length(crate::FlowLength::px(height)),
                ),
                margin: PhysicalSides {
                    top: crate::FlowLengthAuto::Value(crate::FlowLength::px(margin_top)),
                    right: crate::FlowLengthAuto::ZERO,
                    bottom: crate::FlowLengthAuto::Value(crate::FlowLength::px(margin_bottom)),
                    left: crate::FlowLengthAuto::ZERO,
                },
                ..BlockStyle::default()
            }
        }

        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let child = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            block_style(20.0, 30.0, 40.0),
            Style {
                size: taffy::Size {
                    width: Dimension::auto(),
                    height: Dimension::length(20.0),
                },
                ..Style::default()
            },
            &[],
            2,
        );
        let parent = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                margin: PhysicalSides {
                    top: crate::FlowLengthAuto::Value(crate::FlowLength::px(10.0)),
                    right: crate::FlowLengthAuto::ZERO,
                    bottom: crate::FlowLengthAuto::Value(crate::FlowLength::px(15.0)),
                    left: crate::FlowLengthAuto::ZERO,
                },
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                ..Style::default()
            },
            &[child],
            1,
        );
        let after = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            block_style(10.0, 12.0, 0.0),
            Style {
                size: taffy::Size {
                    width: Dimension::auto(),
                    height: Dimension::length(10.0),
                },
                ..Style::default()
            },
            &[],
            3,
        );
        let root = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle::default(),
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::length(200.0),
                    height: Dimension::auto(),
                },
                ..Style::default()
            },
            &[parent, after],
            0,
        );

        tree.compute_layout_with_measure(root, available(200.0, 200.0), zero_measure);

        assert_eq!(tree.layout(parent).y, 30.0);
        assert_eq!(tree.layout(child).y, 0.0);
        assert_eq!(tree.layout(after).y, 90.0);
        assert_eq!(tree.layout(root).height, 100.0);
        assert_eq!(tree.block_algorithm(parent), Some(BlockAlgorithm::Buckram));
        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Buckram));
        assert_eq!(
            tree.block_margins(parent),
            Some(BlockMarginState {
                block_start: CollapsedMargin::from_margin(30.0),
                block_end: CollapsedMargin::from_margin(40.0),
                collapses_through: false,
            })
        );
    }

    #[test]
    fn buckram_places_direct_floats_and_clearance_inside_an_independent_bfc() {
        fn float_style(side: FloatSide, width: f32, height: f32) -> BlockStyle {
            BlockStyle {
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Length(crate::FlowLength::px(width)),
                    BlockSizeValue::Length(crate::FlowLength::px(height)),
                ),
                float: side,
                establishes_bfc: true,
                ..BlockStyle::default()
            }
        }
        fn backend_size(width: f32, height: f32) -> Style {
            Style {
                size: taffy::Size {
                    width: Dimension::length(width),
                    height: Dimension::length(height),
                },
                ..Style::default()
            }
        }

        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let left = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            float_style(FloatSide::Left, 80.0, 40.0),
            backend_size(80.0, 40.0),
            &[],
            1,
        );
        let right = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            float_style(FloatSide::Right, 60.0, 70.0),
            backend_size(60.0, 70.0),
            &[],
            2,
        );
        let clear = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Auto,
                    BlockSizeValue::Length(crate::FlowLength::px(10.0)),
                ),
                clear: ClearSide::Both,
                ..BlockStyle::default()
            },
            backend_size(200.0, 10.0),
            &[],
            3,
        );
        let root = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::length(200.0),
                    height: Dimension::auto(),
                },
                ..Style::default()
            },
            &[left, right, clear],
            0,
        );

        tree.compute_layout_with_measure(root, available(200.0, 200.0), zero_measure);

        assert_eq!((tree.layout(left).x, tree.layout(left).y), (0.0, 0.0));
        assert_eq!((tree.layout(right).x, tree.layout(right).y), (140.0, 0.0));
        assert_eq!((tree.layout(clear).x, tree.layout(clear).y), (0.0, 70.0));
        assert_eq!(tree.layout(root).height, 80.0);
        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Buckram));
    }

    #[test]
    fn buckram_carries_clearance_through_an_empty_collapsing_block() {
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let float = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Length(crate::FlowLength::px(80.0)),
                    BlockSizeValue::Length(crate::FlowLength::px(40.0)),
                ),
                float: FloatSide::Left,
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                size: taffy::Size {
                    width: Dimension::length(80.0),
                    height: Dimension::length(40.0),
                },
                ..Style::default()
            },
            &[],
            1,
        );
        let mut empty_style = BlockStyle {
            clear: ClearSide::Left,
            ..BlockStyle::default()
        };
        empty_style.margin.top = crate::FlowLengthAuto::Value(crate::FlowLength::px(10.0));
        empty_style.margin.bottom = crate::FlowLengthAuto::Value(crate::FlowLength::px(20.0));
        let empty = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            empty_style,
            Style {
                display: Display::Block,
                ..Style::default()
            },
            &[],
            2,
        );
        let mut following_style = BlockStyle {
            size: crate::BlockDimensions::new(
                BlockSizeValue::Auto,
                BlockSizeValue::Length(crate::FlowLength::px(10.0)),
            ),
            ..BlockStyle::default()
        };
        following_style.margin.top = crate::FlowLengthAuto::Value(crate::FlowLength::px(30.0));
        let following = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            following_style,
            Style {
                size: taffy::Size {
                    width: Dimension::auto(),
                    height: Dimension::length(10.0),
                },
                ..Style::default()
            },
            &[],
            3,
        );
        let root = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::length(200.0),
                    height: Dimension::auto(),
                },
                ..Style::default()
            },
            &[float, empty, following],
            0,
        );

        tree.compute_layout_with_measure(root, available(200.0, 200.0), zero_measure);

        assert_eq!((tree.layout(float).x, tree.layout(float).y), (0.0, 0.0));
        assert_eq!(
            (tree.layout(empty).y, tree.layout(empty).height),
            (40.0, 0.0)
        );
        assert_eq!(
            (tree.layout(following).y, tree.layout(following).height),
            (70.0, 10.0)
        );
        assert_eq!(tree.layout(root).height, 80.0);
        assert_eq!(tree.block_algorithm(empty), Some(BlockAlgorithm::Buckram));
        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Buckram));
    }

    #[test]
    fn buckram_sizes_an_auto_float_from_inline_intrinsic_queries() {
        let mut tree = AlgorithmTree::<Style, Vec<AlgorithmAvailableSpace>, u8>::new();
        let lines = tree.new_leaf_with_context_and_block_style(
            BlockStyle::anonymous(FlowAxes::HORIZONTAL_LTR, FlowAxes::HORIZONTAL_LTR),
            Style::default(),
            Vec::new(),
            2,
        );
        let float = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                float: FloatSide::Left,
                establishes_bfc: true,
                shrink_to_fit: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                ..Style::default()
            },
            &[lines],
            1,
        );
        tree.enable_intrinsic_shrink_to_fit(float);
        let root = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::length(100.0),
                    height: Dimension::auto(),
                },
                ..Style::default()
            },
            &[float],
            0,
        );

        tree.compute_layout_with_measure(
            root,
            available(100.0, 200.0),
            |known, available, _, context, _| {
                if let Some(context) = context {
                    context.push(available.width);
                }
                let measured_width = match available.width {
                    AlgorithmAvailableSpace::Definite(width) => width,
                    AlgorithmAvailableSpace::MinContent => 40.0,
                    AlgorithmAvailableSpace::MaxContent => 120.0,
                };
                AlgorithmSize::new(
                    known.width.unwrap_or(measured_width),
                    known.height.unwrap_or(20.0),
                )
            },
        );

        assert_eq!(
            tree.layout(float),
            AlgorithmLayout {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 20.0,
            }
        );
        let queries = tree.context(lines).expect("inline queries");
        assert!(queries.contains(&AlgorithmAvailableSpace::MinContent));
        assert!(queries.contains(&AlgorithmAvailableSpace::MaxContent));
        assert!(queries.contains(&AlgorithmAvailableSpace::Definite(100.0)));
        assert_eq!(tree.layout(root).height, 20.0);
        assert_eq!(tree.block_algorithm(float), Some(BlockAlgorithm::Buckram));
        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Buckram));
    }

    #[test]
    fn buckram_queries_multi_child_and_block_content_intrinsics_for_shrink_to_fit() {
        fn layout_float(containing_width: f32, wrap_children: bool) -> (f32, usize) {
            let mut tree = AlgorithmTree::<Style, Vec<AlgorithmAvailableSpace>, u8>::new();
            let first = tree.new_leaf_with_context_and_block_style(
                BlockStyle::anonymous(FlowAxes::HORIZONTAL_LTR, FlowAxes::HORIZONTAL_LTR),
                Style::default(),
                Vec::new(),
                1,
            );
            let second = tree.new_leaf_with_context_and_block_style(
                BlockStyle::anonymous(FlowAxes::HORIZONTAL_LTR, FlowAxes::HORIZONTAL_LTR),
                Style::default(),
                Vec::new(),
                2,
            );
            let children = if wrap_children {
                let block = tree.new_with_children_and_block_style(
                    AlgorithmKind::Block,
                    BlockStyle::default(),
                    Style::default(),
                    &[first, second],
                    3,
                );
                vec![block]
            } else {
                vec![first, second]
            };
            let float = tree.new_with_children_and_block_style(
                AlgorithmKind::Block,
                BlockStyle {
                    float: FloatSide::Left,
                    establishes_bfc: true,
                    shrink_to_fit: true,
                    ..BlockStyle::default()
                },
                Style::default(),
                &children,
                4,
            );
            tree.enable_intrinsic_shrink_to_fit(float);
            let root = tree.new_with_children_and_block_style(
                AlgorithmKind::Block,
                BlockStyle {
                    establishes_bfc: true,
                    ..BlockStyle::default()
                },
                Style {
                    display: Display::Block,
                    size: taffy::Size {
                        width: Dimension::length(containing_width),
                        height: Dimension::auto(),
                    },
                    ..Style::default()
                },
                &[float],
                0,
            );

            tree.compute_layout_with_measure(
                root,
                available(containing_width, 200.0),
                |known, available, _, context, _| {
                    if let Some(context) = context {
                        context.push(available.width);
                    }
                    let width = match available.width {
                        AlgorithmAvailableSpace::Definite(width) => width,
                        AlgorithmAvailableSpace::MinContent => 40.0,
                        AlgorithmAvailableSpace::MaxContent => 120.0,
                    };
                    AlgorithmSize::new(known.width.unwrap_or(width), known.height.unwrap_or(10.0))
                },
            );

            assert!(
                tree.context(first)
                    .expect("first inline context")
                    .contains(&AlgorithmAvailableSpace::MinContent)
            );
            assert!(
                tree.context(second)
                    .expect("second inline context")
                    .contains(&AlgorithmAvailableSpace::MaxContent)
            );
            assert_eq!(tree.block_algorithm_counts().1, 0);
            (tree.layout(float).width, tree.block_algorithm_counts().0)
        }

        for wrap_children in [false, true] {
            assert_eq!(layout_float(30.0, wrap_children).0, 40.0);
            assert_eq!(layout_float(80.0, wrap_children).0, 80.0);
            let (width, buckram_blocks) = layout_float(200.0, wrap_children);
            assert_eq!(width, 120.0);
            assert!(buckram_blocks >= if wrap_children { 3 } else { 2 });
        }
    }

    #[test]
    fn buckram_queries_atomic_inline_intrinsics_without_float_placement() {
        fn layout_atomic_inline(containing_width: f32) -> f32 {
            let mut tree = AlgorithmTree::<Style, Vec<AlgorithmAvailableSpace>, u8>::new();
            let lines = tree.new_leaf_with_context_and_block_style(
                BlockStyle::anonymous(FlowAxes::HORIZONTAL_LTR, FlowAxes::HORIZONTAL_LTR),
                Style::default(),
                Vec::new(),
                1,
            );
            let inline_block = tree.new_with_children_and_block_style(
                AlgorithmKind::Block,
                BlockStyle {
                    establishes_bfc: true,
                    shrink_to_fit: true,
                    ..BlockStyle::default()
                },
                Style::default(),
                &[lines],
                2,
            );
            tree.enable_float_avoidance(inline_block);
            tree.enable_intrinsic_shrink_to_fit(inline_block);
            let root = tree.new_with_children_and_block_style(
                AlgorithmKind::Block,
                BlockStyle {
                    establishes_bfc: true,
                    ..BlockStyle::default()
                },
                Style {
                    display: Display::Block,
                    size: taffy::Size {
                        width: Dimension::length(containing_width),
                        height: Dimension::auto(),
                    },
                    ..Style::default()
                },
                &[inline_block],
                0,
            );

            tree.compute_layout_with_measure(
                root,
                available(containing_width, 200.0),
                |known, available, _, context, _| {
                    if let Some(context) = context {
                        context.push(available.width);
                    }
                    let width = match available.width {
                        AlgorithmAvailableSpace::Definite(width) => width,
                        AlgorithmAvailableSpace::MinContent => 40.0,
                        AlgorithmAvailableSpace::MaxContent => 120.0,
                    };
                    AlgorithmSize::new(known.width.unwrap_or(width), known.height.unwrap_or(10.0))
                },
            );

            assert_eq!(tree.block_algorithm_counts().1, 0);
            tree.layout(inline_block).width
        }

        assert_eq!(layout_atomic_inline(30.0), 40.0);
        assert_eq!(layout_atomic_inline(80.0), 80.0);
        assert_eq!(layout_atomic_inline(200.0), 120.0);
    }

    #[test]
    fn buckram_remeasures_opted_in_bfcs_inside_the_float_band() {
        let mut tree = AlgorithmTree::<Style, Vec<f32>, u8>::new();
        let float = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Length(crate::FlowLength::px(80.0)),
                    BlockSizeValue::Length(crate::FlowLength::px(40.0)),
                ),
                float: FloatSide::Left,
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                size: taffy::Size {
                    width: Dimension::length(80.0),
                    height: Dimension::length(40.0),
                },
                ..Style::default()
            },
            &[],
            1,
        );
        let auto_bfc = tree.new_leaf_with_context_and_block_style(
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style::default(),
            Vec::new(),
            2,
        );
        tree.enable_float_avoidance(auto_bfc);
        let definite_bfc = tree.new_leaf_with_context_and_block_style(
            BlockStyle {
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Length(crate::FlowLength::px(150.0)),
                    BlockSizeValue::Auto,
                ),
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style::default(),
            Vec::new(),
            3,
        );
        tree.enable_float_avoidance(definite_bfc);
        let root = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::length(200.0),
                    height: Dimension::auto(),
                },
                ..Style::default()
            },
            &[float, auto_bfc, definite_bfc],
            0,
        );

        tree.compute_layout_with_measure(
            root,
            available(200.0, 200.0),
            |known, available, _, context, _| {
                let width = known.width.unwrap_or(match available.width {
                    AlgorithmAvailableSpace::Definite(width) => width,
                    AlgorithmAvailableSpace::MinContent => 0.0,
                    AlgorithmAvailableSpace::MaxContent => f32::INFINITY,
                });
                if let Some(context) = context {
                    context.push(width);
                }
                AlgorithmSize::new(width, 20.0)
            },
        );

        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Buckram));
        assert_eq!(tree.context(auto_bfc), Some(&vec![120.0]));
        assert_eq!(tree.context(definite_bfc), Some(&vec![150.0]));
        assert_eq!(
            tree.layout(auto_bfc),
            AlgorithmLayout {
                x: 80.0,
                y: 0.0,
                width: 120.0,
                height: 20.0,
            }
        );
        assert_eq!(
            tree.layout(definite_bfc),
            AlgorithmLayout {
                x: 0.0,
                y: 40.0,
                width: 150.0,
                height: 20.0,
            }
        );
        assert_eq!(tree.layout(root).height, 60.0);
    }

    #[test]
    fn buckram_applies_bfc_inline_margins_before_avoiding_a_float() {
        let mut tree = AlgorithmTree::<Style, Vec<f32>, u8>::new();
        let float = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Length(crate::FlowLength::px(50.0)),
                    BlockSizeValue::Length(crate::FlowLength::px(40.0)),
                ),
                float: FloatSide::Right,
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                size: taffy::Size {
                    width: Dimension::length(50.0),
                    height: Dimension::length(40.0),
                },
                ..Style::default()
            },
            &[],
            1,
        );
        let mut bfc_style = BlockStyle {
            establishes_bfc: true,
            ..BlockStyle::default()
        };
        bfc_style.margin.left = crate::FlowLengthAuto::Value(crate::FlowLength::px(51.0));
        let bfc =
            tree.new_leaf_with_context_and_block_style(bfc_style, Style::default(), Vec::new(), 2);
        tree.enable_float_avoidance(bfc);
        let root = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::length(100.0),
                    height: Dimension::auto(),
                },
                ..Style::default()
            },
            &[float, bfc],
            0,
        );

        tree.compute_layout_with_measure(
            root,
            available(100.0, 200.0),
            |known, available, _, context, _| {
                let width = known.width.unwrap_or(match available.width {
                    AlgorithmAvailableSpace::Definite(width) => width,
                    AlgorithmAvailableSpace::MinContent => 0.0,
                    AlgorithmAvailableSpace::MaxContent => f32::INFINITY,
                });
                if let Some(context) = context {
                    context.push(width);
                }
                AlgorithmSize::new(width, 60.0)
            },
        );

        assert_eq!(
            tree.layout(bfc),
            AlgorithmLayout {
                x: 51.0,
                y: 40.0,
                width: 49.0,
                height: 60.0,
            }
        );
        assert_eq!(tree.context(bfc), Some(&vec![49.0]));
        assert_eq!(tree.layout(root).height, 100.0);
        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Buckram));
    }

    #[test]
    fn buckram_places_flex_and_grid_algorithms_around_a_float() {
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let float = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Length(crate::FlowLength::px(40.0)),
                    BlockSizeValue::Length(crate::FlowLength::px(40.0)),
                ),
                float: FloatSide::Left,
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                size: taffy::Size {
                    width: Dimension::length(40.0),
                    height: Dimension::length(40.0),
                },
                ..Style::default()
            },
            &[],
            1,
        );
        let flex_child = tree.new_with_children(
            AlgorithmKind::Leaf,
            Style {
                size: taffy::Size {
                    width: Dimension::length(20.0),
                    height: Dimension::length(10.0),
                },
                ..Style::default()
            },
            &[],
            2,
        );
        let flex = tree.new_with_children_and_block_style(
            AlgorithmKind::Flex,
            BlockStyle {
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Auto,
                    BlockSizeValue::Length(crate::FlowLength::px(20.0)),
                ),
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Flex,
                size: taffy::Size {
                    width: Dimension::auto(),
                    height: Dimension::length(20.0),
                },
                ..Style::default()
            },
            &[flex_child],
            3,
        );
        tree.enable_float_avoidance(flex);
        let grid_child = tree.new_with_children(
            AlgorithmKind::Leaf,
            Style {
                size: taffy::Size {
                    width: Dimension::length(20.0),
                    height: Dimension::length(10.0),
                },
                ..Style::default()
            },
            &[],
            4,
        );
        let grid = tree.new_with_children_and_block_style(
            AlgorithmKind::Grid,
            BlockStyle {
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Length(crate::FlowLength::px(70.0)),
                    BlockSizeValue::Length(crate::FlowLength::px(20.0)),
                ),
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Grid,
                size: taffy::Size {
                    width: Dimension::length(70.0),
                    height: Dimension::length(20.0),
                },
                grid_template_columns: vec![length(20.0_f32)],
                ..Style::default()
            },
            &[grid_child],
            5,
        );
        tree.enable_float_avoidance(grid);
        let root = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::length(100.0),
                    height: Dimension::auto(),
                },
                ..Style::default()
            },
            &[float, flex, grid],
            0,
        );

        tree.compute_layout_with_measure(root, available(100.0, 200.0), zero_measure);

        assert_eq!(
            tree.layout(flex),
            AlgorithmLayout {
                x: 40.0,
                y: 0.0,
                width: 60.0,
                height: 20.0,
            }
        );
        assert_eq!(
            tree.layout(flex_child),
            AlgorithmLayout {
                width: 20.0,
                height: 10.0,
                ..AlgorithmLayout::default()
            }
        );
        assert_eq!(
            tree.layout(grid),
            AlgorithmLayout {
                x: 0.0,
                y: 40.0,
                width: 70.0,
                height: 20.0,
            }
        );
        assert_eq!(
            tree.layout(grid_child),
            AlgorithmLayout {
                width: 20.0,
                height: 10.0,
                ..AlgorithmLayout::default()
            }
        );
        assert_eq!(tree.layout(root).height, 60.0);
        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Buckram));
        assert_eq!(tree.block_algorithm(flex), None);
        assert_eq!(tree.block_algorithm(grid), None);
    }

    #[test]
    fn flex_without_an_active_float_does_not_widen_the_buckram_block_lane() {
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let flex = tree.new_with_children_and_block_style(
            AlgorithmKind::Flex,
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Flex,
                size: taffy::Size {
                    width: Dimension::auto(),
                    height: Dimension::length(20.0),
                },
                ..Style::default()
            },
            &[],
            1,
        );
        tree.enable_float_avoidance(flex);
        let root = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::length(100.0),
                    height: Dimension::auto(),
                },
                ..Style::default()
            },
            &[flex],
            0,
        );

        tree.compute_layout_with_measure(root, available(100.0, 200.0), zero_measure);

        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Taffy));
    }

    #[test]
    fn buckram_delivers_float_constraints_to_a_direct_inline_leaf() {
        let mut tree = AlgorithmTree::<Style, Vec<crate::FloatAvailableSpace>, u8>::new();
        let float = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Length(crate::FlowLength::px(80.0)),
                    BlockSizeValue::Length(crate::FlowLength::px(40.0)),
                ),
                float: FloatSide::Left,
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                size: taffy::Size {
                    width: Dimension::length(80.0),
                    height: Dimension::length(40.0),
                },
                ..Style::default()
            },
            &[],
            1,
        );
        let lines = tree.new_leaf_with_context_and_block_style(
            BlockStyle::anonymous(FlowAxes::HORIZONTAL_LTR, FlowAxes::HORIZONTAL_LTR),
            Style {
                display: Display::Block,
                ..Style::default()
            },
            Vec::new(),
            2,
        );
        tree.enable_float_line_constraints(lines);
        let root = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::length(200.0),
                    height: Dimension::auto(),
                },
                ..Style::default()
            },
            &[float, lines],
            0,
        );

        tree.compute_layout_with_measure(
            root,
            available(200.0, 200.0),
            |known, _, _, context, constraints| {
                let Some(context) = context else {
                    return AlgorithmSize::new(0.0, 0.0);
                };
                if let Some(constraints) = constraints {
                    *context = [0.0, 20.0, 40.0]
                        .map(|line_top| constraints.available_space(line_top, 18.0))
                        .to_vec();
                }
                AlgorithmSize::new(known.width.unwrap_or(200.0), known.height.unwrap_or(60.0))
            },
        );

        assert_eq!(
            tree.context(lines).expect("line context"),
            &[
                crate::FloatAvailableSpace {
                    inline_start: 80.0,
                    inline_size: 120.0,
                },
                crate::FloatAvailableSpace {
                    inline_start: 80.0,
                    inline_size: 120.0,
                },
                crate::FloatAvailableSpace {
                    inline_start: 0.0,
                    inline_size: 200.0,
                },
            ]
        );
        assert_eq!(tree.layout(lines).height, 60.0);
        assert_eq!(tree.layout(root).height, 60.0);
        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Buckram));
    }

    #[test]
    fn buckram_exports_nested_negative_margin_floats_through_a_relative_wrapper() {
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let nested_float = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Length(crate::FlowLength::px(80.0)),
                    BlockSizeValue::Length(crate::FlowLength::px(40.0)),
                ),
                float: FloatSide::Left,
                establishes_bfc: true,
                margin: PhysicalSides {
                    top: FlowLengthAuto::Value(FlowLength::px(-10.0)),
                    right: FlowLengthAuto::ZERO,
                    bottom: FlowLengthAuto::ZERO,
                    left: FlowLengthAuto::ZERO,
                },
                ..BlockStyle::default()
            },
            Style {
                size: taffy::Size {
                    width: Dimension::length(80.0),
                    height: Dimension::length(40.0),
                },
                ..Style::default()
            },
            &[],
            1,
        );
        let inner = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle::default(),
            Style {
                display: Display::Block,
                ..Style::default()
            },
            &[nested_float],
            2,
        );
        tree.enable_nested_float_state(inner);
        let outer = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                position: crate::BlockPosition::Relative,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                ..Style::default()
            },
            &[inner],
            3,
        );
        tree.enable_nested_float_state(outer);
        let clear = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Auto,
                    BlockSizeValue::Length(crate::FlowLength::px(10.0)),
                ),
                clear: ClearSide::Left,
                ..BlockStyle::default()
            },
            Style {
                size: taffy::Size {
                    width: Dimension::auto(),
                    height: Dimension::length(10.0),
                },
                ..Style::default()
            },
            &[],
            4,
        );
        let root = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::length(200.0),
                    height: Dimension::auto(),
                },
                ..Style::default()
            },
            &[outer, clear],
            0,
        );

        tree.compute_layout_with_measure(root, available(200.0, 200.0), zero_measure);

        assert_eq!(
            (tree.layout(nested_float).x, tree.layout(nested_float).y),
            (0.0, -10.0)
        );
        assert_eq!(tree.layout(inner).height, 0.0);
        assert_eq!(tree.layout(outer).height, 0.0);
        assert_eq!(tree.layout(clear).y, 30.0);
        assert_eq!(tree.layout(root).height, 40.0);
        assert_eq!(tree.block_algorithm(inner), Some(BlockAlgorithm::Buckram));
        assert_eq!(tree.block_algorithm(outer), Some(BlockAlgorithm::Buckram));
        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Buckram));
    }

    #[test]
    fn buckram_delivers_outer_float_constraints_through_an_ordinary_wrapper() {
        let mut tree = AlgorithmTree::<Style, Vec<crate::FloatAvailableSpace>, u8>::new();
        let float = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Length(crate::FlowLength::px(80.0)),
                    BlockSizeValue::Length(crate::FlowLength::px(40.0)),
                ),
                float: FloatSide::Left,
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                size: taffy::Size {
                    width: Dimension::length(80.0),
                    height: Dimension::length(40.0),
                },
                ..Style::default()
            },
            &[],
            1,
        );
        let lines = tree.new_leaf_with_context_and_block_style(
            BlockStyle::anonymous(FlowAxes::HORIZONTAL_LTR, FlowAxes::HORIZONTAL_LTR),
            Style {
                display: Display::Block,
                ..Style::default()
            },
            Vec::new(),
            2,
        );
        tree.enable_float_line_constraints(lines);
        let wrapper = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle::default(),
            Style {
                display: Display::Block,
                ..Style::default()
            },
            &[lines],
            3,
        );
        tree.enable_nested_float_state(wrapper);
        let root = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::length(200.0),
                    height: Dimension::auto(),
                },
                ..Style::default()
            },
            &[float, wrapper],
            0,
        );

        tree.compute_layout_with_measure(
            root,
            available(200.0, 200.0),
            |known, _, _, context, constraints| {
                let Some(context) = context else {
                    return AlgorithmSize::new(0.0, 0.0);
                };
                if let Some(constraints) = constraints {
                    *context = [0.0, 20.0, 40.0]
                        .map(|line_top| constraints.available_space(line_top, 18.0))
                        .to_vec();
                }
                AlgorithmSize::new(known.width.unwrap_or(200.0), known.height.unwrap_or(60.0))
            },
        );

        assert_eq!(
            tree.context(lines).expect("line context"),
            &[
                crate::FloatAvailableSpace {
                    inline_start: 80.0,
                    inline_size: 120.0,
                },
                crate::FloatAvailableSpace {
                    inline_start: 80.0,
                    inline_size: 120.0,
                },
                crate::FloatAvailableSpace {
                    inline_start: 0.0,
                    inline_size: 200.0,
                },
            ]
        );
        assert_eq!(tree.layout(wrapper).height, 60.0);
        assert_eq!(tree.layout(root).height, 60.0);
        assert_eq!(tree.block_algorithm(wrapper), Some(BlockAlgorithm::Buckram));
        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Buckram));
    }

    #[test]
    fn buckram_stops_nested_float_state_at_an_explicit_bfc() {
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let nested_float = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Length(crate::FlowLength::px(80.0)),
                    BlockSizeValue::Length(crate::FlowLength::px(40.0)),
                ),
                float: FloatSide::Left,
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                size: taffy::Size {
                    width: Dimension::length(80.0),
                    height: Dimension::length(40.0),
                },
                ..Style::default()
            },
            &[],
            1,
        );
        let boundary = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Auto,
                    BlockSizeValue::Length(crate::FlowLength::ZERO),
                ),
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::auto(),
                    height: Dimension::length(0.0),
                },
                ..Style::default()
            },
            &[nested_float],
            2,
        );
        let clear = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Auto,
                    BlockSizeValue::Length(crate::FlowLength::px(10.0)),
                ),
                clear: ClearSide::Left,
                ..BlockStyle::default()
            },
            Style {
                size: taffy::Size {
                    width: Dimension::auto(),
                    height: Dimension::length(10.0),
                },
                ..Style::default()
            },
            &[],
            3,
        );
        let root = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::length(200.0),
                    height: Dimension::auto(),
                },
                ..Style::default()
            },
            &[boundary, clear],
            0,
        );

        tree.compute_layout_with_measure(root, available(200.0, 200.0), zero_measure);

        assert_eq!(tree.layout(boundary).height, 0.0);
        assert_eq!(tree.layout(clear).y, 0.0);
        assert_eq!(tree.layout(root).height, 10.0);
        assert_eq!(
            tree.block_algorithm(boundary),
            Some(BlockAlgorithm::Buckram)
        );
        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Buckram));
    }

    #[test]
    fn buckram_delivers_outer_clearance_through_an_ordinary_wrapper() {
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let float = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Length(crate::FlowLength::px(80.0)),
                    BlockSizeValue::Length(crate::FlowLength::px(40.0)),
                ),
                float: FloatSide::Left,
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                size: taffy::Size {
                    width: Dimension::length(80.0),
                    height: Dimension::length(40.0),
                },
                ..Style::default()
            },
            &[],
            1,
        );
        let clear = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Auto,
                    BlockSizeValue::Length(crate::FlowLength::px(10.0)),
                ),
                clear: ClearSide::Left,
                ..BlockStyle::default()
            },
            Style {
                size: taffy::Size {
                    width: Dimension::auto(),
                    height: Dimension::length(10.0),
                },
                ..Style::default()
            },
            &[],
            2,
        );
        let wrapper = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle::default(),
            Style {
                display: Display::Block,
                ..Style::default()
            },
            &[clear],
            3,
        );
        tree.enable_nested_float_state(wrapper);
        let root = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::length(200.0),
                    height: Dimension::auto(),
                },
                ..Style::default()
            },
            &[float, wrapper],
            0,
        );

        tree.compute_layout_with_measure(root, available(200.0, 200.0), zero_measure);

        assert_eq!(tree.layout(clear).y, 40.0);
        assert_eq!(tree.layout(wrapper).height, 50.0);
        assert_eq!(tree.layout(root).height, 50.0);
        assert_eq!(tree.block_algorithm(wrapper), Some(BlockAlgorithm::Buckram));
        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Buckram));
    }

    #[test]
    fn nested_clear_without_a_shared_float_role_remains_deferred() {
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let clear = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Auto,
                    BlockSizeValue::Length(crate::FlowLength::px(10.0)),
                ),
                clear: ClearSide::Both,
                ..BlockStyle::default()
            },
            Style {
                size: taffy::Size {
                    width: Dimension::auto(),
                    height: Dimension::length(10.0),
                },
                ..Style::default()
            },
            &[],
            1,
        );
        let wrapper = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle::default(),
            Style {
                display: Display::Block,
                ..Style::default()
            },
            &[clear],
            2,
        );
        let root = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::length(200.0),
                    height: Dimension::auto(),
                },
                ..Style::default()
            },
            &[wrapper],
            0,
        );

        tree.compute_layout_with_measure(root, available(200.0, 200.0), zero_measure);

        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Taffy));
    }

    #[test]
    fn nested_float_state_nonconvergence_remains_deferred() {
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let float = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Length(crate::FlowLength::px(80.0)),
                    BlockSizeValue::Length(crate::FlowLength::px(40.0)),
                ),
                float: FloatSide::Left,
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                size: taffy::Size {
                    width: Dimension::length(80.0),
                    height: Dimension::length(40.0),
                },
                ..Style::default()
            },
            &[],
            1,
        );
        let oscillating_leaf = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                margin: PhysicalSides {
                    top: FlowLengthAuto::ZERO,
                    right: FlowLengthAuto::ZERO,
                    bottom: FlowLengthAuto::Value(FlowLength::px(20.0)),
                    left: FlowLengthAuto::ZERO,
                },
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                ..Style::default()
            },
            &[],
            2,
        );
        let wrapper = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle::default(),
            Style {
                display: Display::Block,
                ..Style::default()
            },
            &[oscillating_leaf],
            3,
        );
        tree.enable_nested_float_state(wrapper);
        let root = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::length(200.0),
                    height: Dimension::auto(),
                },
                ..Style::default()
            },
            &[float, wrapper],
            0,
        );
        let calls = std::cell::Cell::new(0_u32);

        tree.compute_layout_with_measure(root, available(200.0, 200.0), |known, _, _, _, _| {
            let call = calls.get();
            calls.set(call + 1);
            AlgorithmSize::new(
                known.width.unwrap_or(200.0),
                if call.is_multiple_of(2) { 0.0 } else { 10.0 },
            )
        });

        assert!(
            calls.get() >= 6,
            "fixture must exhaust the bounded retry loop"
        );
        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Taffy));
        assert_eq!(
            tree.block_deferral(root),
            Some(BlockDeferral::NestedFloatState)
        );
    }

    #[test]
    fn buckram_keeps_inline_context_float_state_deferred() {
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let nested_float = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Length(crate::FlowLength::px(80.0)),
                    BlockSizeValue::Length(crate::FlowLength::px(40.0)),
                ),
                float: FloatSide::Left,
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                size: taffy::Size {
                    width: Dimension::length(80.0),
                    height: Dimension::length(40.0),
                },
                ..Style::default()
            },
            &[],
            1,
        );
        tree.mark_inline_context_float(nested_float);
        let wrapper = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle::default(),
            Style {
                display: Display::Block,
                ..Style::default()
            },
            &[nested_float],
            2,
        );
        tree.enable_nested_float_state(wrapper);
        let root = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::length(200.0),
                    height: Dimension::auto(),
                },
                ..Style::default()
            },
            &[wrapper],
            0,
        );

        tree.compute_layout_with_measure(root, available(200.0, 200.0), zero_measure);

        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Taffy));
    }

    #[test]
    fn adapter_propagates_declared_bfc_baselines_without_backend_child_walks() {
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let absolute = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                position: crate::BlockPosition::Absolute,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                position: Position::Absolute,
                size: taffy::Size {
                    width: Dimension::length(80.0),
                    height: Dimension::length(5.0),
                },
                ..Style::default()
            },
            &[],
            3,
        );
        let flex = tree.new_with_children_and_block_style(
            AlgorithmKind::Flex,
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Flex,
                size: taffy::Size {
                    width: Dimension::length(80.0),
                    height: Dimension::length(20.0),
                },
                ..Style::default()
            },
            &[],
            1,
        );
        let grid = tree.new_with_children_and_block_style(
            AlgorithmKind::Grid,
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Grid,
                size: taffy::Size {
                    width: Dimension::length(80.0),
                    height: Dimension::length(30.0),
                },
                ..Style::default()
            },
            &[],
            2,
        );
        let fixed = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                position: crate::BlockPosition::Fixed,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                position: Position::Absolute,
                size: taffy::Size {
                    width: Dimension::length(80.0),
                    height: Dimension::length(5.0),
                },
                ..Style::default()
            },
            &[],
            4,
        );
        let root = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::length(80.0),
                    height: Dimension::auto(),
                },
                ..Style::default()
            },
            &[absolute, flex, grid, fixed],
            0,
        );

        tree.compute_layout_with_measure(root, available(80.0, 200.0), zero_measure);
        tree.set_baselines(
            flex,
            Baselines::new(Some(7.0), Some(9.0)).expect("flex baselines"),
        );
        tree.set_baselines(
            grid,
            Baselines::new(Some(11.0), Some(13.0)).expect("grid baselines"),
        );
        tree.set_baselines(
            absolute,
            Baselines::new(Some(1.0), Some(2.0)).expect("absolute baselines"),
        );
        tree.set_baselines(
            fixed,
            Baselines::new(Some(40.0), Some(50.0)).expect("fixed baselines"),
        );
        tree.propagate_declared_baselines();

        assert_eq!(
            tree.baselines(flex),
            Baselines::new(Some(7.0), Some(9.0)).unwrap()
        );
        assert_eq!(
            tree.baselines(grid),
            Baselines::new(Some(11.0), Some(13.0)).unwrap()
        );
        assert_eq!(
            tree.baselines(root),
            Baselines::new(
                Some(tree.layout(flex).y + 7.0),
                Some(tree.layout(grid).y + 13.0),
            )
            .unwrap()
        );
    }

    /// K4d6b's seam: a formatting context Buckram owns writes its own and its
    /// children's rectangles before the backend walk, and the walk must not
    /// overwrite them. A `Table` node reports the size it was given and never
    /// lays out a child.
    #[test]
    fn an_owned_table_context_keeps_the_geometry_it_was_given() {
        let mut tree: AlgorithmTree<Style, (), u8> = AlgorithmTree::new();
        let cells = (0..2)
            .map(|index| {
                tree.new_with_children_and_block_style(
                    AlgorithmKind::Block,
                    BlockStyle::default(),
                    Style::default(),
                    &[],
                    index,
                )
            })
            .collect::<Vec<_>>();
        let table = tree.new_with_children_and_block_style(
            AlgorithmKind::Table,
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style::default(),
            &cells,
            9,
        );
        let root = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::length(200.0),
                    height: Dimension::length(200.0),
                },
                ..Style::default()
            },
            &[table],
            10,
        );

        // The table algorithm's decisions, written before the walk.
        let decided = [
            AlgorithmLayout {
                x: 0.0,
                y: 0.0,
                width: 60.0,
                height: 25.0,
            },
            AlgorithmLayout {
                x: 60.0,
                y: 0.0,
                width: 40.0,
                height: 25.0,
            },
        ];
        for (cell, layout) in cells.iter().zip(decided) {
            tree.set_layout(*cell, layout);
        }
        tree.set_layout(
            table,
            AlgorithmLayout {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 25.0,
            },
        );

        tree.compute_layout_with_measure(root, available(200.0, 200.0), zero_measure);

        // The table reported Buckram's size, and every cell rectangle
        // survived the walk untouched.
        let table_layout = tree.layout(table);
        assert_eq!((table_layout.width, table_layout.height), (100.0, 25.0));
        for (cell, expected) in cells.iter().zip(decided) {
            let layout = tree.layout(*cell);
            assert_eq!(
                (layout.x, layout.y, layout.width, layout.height),
                (expected.x, expected.y, expected.width, expected.height),
                "the backend overwrote a rectangle the table algorithm owns"
            );
        }
    }

    #[test]
    fn orthogonal_auto_inline_uses_intrinsic_child_block_contribution() {
        use crate::{Direction, WritingMode};

        let horizontal = FlowAxes::HORIZONTAL_LTR;
        let vertical = FlowAxes::new(WritingMode::VerticalRl, Direction::Ltr);
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let text = tree.new_leaf_with_context_and_block_style(
            BlockStyle::anonymous(horizontal, horizontal),
            Style::default(),
            (),
            2,
        );
        let line = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle::anonymous(horizontal, vertical),
            Style {
                display: Display::Block,
                ..Style::default()
            },
            &[text],
            1,
        );
        let root = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                flow: vertical,
                containing_flow: horizontal,
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                ..Style::default()
            },
            &[line],
            0,
        );

        tree.compute_layout_with_measure(
            root,
            available(300.0, 200.0),
            |known, available, node, _context, _line_constraints| {
                assert_eq!(node, text);
                let width = known.width.unwrap_or_else(|| match available.width {
                    AlgorithmAvailableSpace::Definite(width) => 120.0_f32.min(width),
                    AlgorithmAvailableSpace::MinContent => 40.0,
                    AlgorithmAvailableSpace::MaxContent => 120.0,
                });
                AlgorithmSize::new(width, known.height.unwrap_or(20.0))
            },
        );

        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Buckram));
        assert_eq!(tree.block_algorithm(line), Some(BlockAlgorithm::Buckram));
        assert_eq!(
            tree.layout(root),
            AlgorithmLayout {
                width: 120.0,
                height: 20.0,
                ..AlgorithmLayout::default()
            }
        );
        assert_eq!(
            tree.layout(line),
            AlgorithmLayout {
                width: 120.0,
                height: 20.0,
                ..AlgorithmLayout::default()
            }
        );
    }

    #[test]
    fn orthogonal_auto_inline_rejects_a_nested_flow_boundary() {
        use crate::{Direction, WritingMode};

        let horizontal = FlowAxes::HORIZONTAL_LTR;
        let vertical = FlowAxes::new(WritingMode::VerticalRl, Direction::Ltr);
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let text = tree.new_leaf_with_context_and_block_style(
            BlockStyle::anonymous(horizontal, horizontal),
            Style::default(),
            (),
            3,
        );
        let line = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle::anonymous(horizontal, vertical),
            Style {
                display: Display::Block,
                ..Style::default()
            },
            &[text],
            2,
        );
        let same_flow_wrapper = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle::anonymous(vertical, vertical),
            Style {
                display: Display::Block,
                ..Style::default()
            },
            &[line],
            1,
        );
        let root = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                flow: vertical,
                containing_flow: horizontal,
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                ..Style::default()
            },
            &[same_flow_wrapper],
            0,
        );

        tree.compute_layout_with_measure(
            root,
            available(300.0, 200.0),
            |known, available, node, _context, _line_constraints| {
                assert_eq!(node, text);
                let width = known.width.unwrap_or_else(|| match available.width {
                    AlgorithmAvailableSpace::Definite(width) => 120.0_f32.min(width),
                    AlgorithmAvailableSpace::MinContent => 40.0,
                    AlgorithmAvailableSpace::MaxContent => 120.0,
                });
                AlgorithmSize::new(width, known.height.unwrap_or(20.0))
            },
        );

        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Buckram));
        assert_eq!(tree.layout(root).height, 200.0);
    }

    #[test]
    fn orthogonal_auto_block_sizes_finalize_before_physical_child_placement() {
        use crate::{Direction, WritingMode};

        for (writing_mode, first_x, second_x) in [
            (WritingMode::VerticalRl, 30.0, 0.0),
            (WritingMode::VerticalLr, 0.0, 20.0),
            (WritingMode::SidewaysRl, 30.0, 0.0),
            (WritingMode::SidewaysLr, 0.0, 20.0),
        ] {
            let flow = FlowAxes::new(writing_mode, Direction::Ltr);
            let mut tree = AlgorithmTree::<Style, (), u8>::new();
            let first = tree.new_with_children_and_block_style(
                AlgorithmKind::Leaf,
                BlockStyle::anonymous(flow, flow),
                Style::default(),
                &[],
                1,
            );
            let second = tree.new_with_children_and_block_style(
                AlgorithmKind::Leaf,
                BlockStyle::anonymous(flow, flow),
                Style::default(),
                &[],
                2,
            );
            let root = tree.new_with_children_and_block_style(
                AlgorithmKind::Block,
                BlockStyle {
                    flow,
                    containing_flow: flow,
                    establishes_bfc: true,
                    ..BlockStyle::default()
                },
                Style {
                    display: Display::Block,
                    ..Style::default()
                },
                &[first, second],
                0,
            );

            tree.compute_layout_with_measure(
                root,
                available(300.0, 200.0),
                |known, _available, node, _context, _line_constraints| {
                    let block = if node == first { 20.0 } else { 30.0 };
                    AlgorithmSize::new(block, known.height.unwrap_or(200.0))
                },
            );

            assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Buckram));
            assert_eq!(
                tree.layout(root),
                AlgorithmLayout {
                    width: 50.0,
                    height: 200.0,
                    ..AlgorithmLayout::default()
                },
                "{writing_mode:?} root size"
            );
            assert_eq!(
                tree.layout(first),
                AlgorithmLayout {
                    x: first_x,
                    width: 20.0,
                    height: 200.0,
                    ..AlgorithmLayout::default()
                },
                "{writing_mode:?} first child"
            );
            assert_eq!(
                tree.layout(second),
                AlgorithmLayout {
                    x: second_x,
                    width: 30.0,
                    height: 200.0,
                    ..AlgorithmLayout::default()
                },
                "{writing_mode:?} second child"
            );
        }
    }

    #[test]
    fn adapter_nests_horizontal_and_vertical_normal_flow_in_both_directions() {
        use crate::{Direction, WritingMode};

        fn layout_nested(parent_flow: FlowAxes, child_flow: FlowAxes) -> AlgorithmLayout {
            let mut tree = AlgorithmTree::<Style, (), u8>::new();
            let leaf = tree.new_with_children_and_block_style(
                AlgorithmKind::Leaf,
                BlockStyle::anonymous(child_flow, child_flow),
                Style::default(),
                &[],
                2,
            );
            let child = tree.new_with_children_and_block_style(
                AlgorithmKind::Block,
                BlockStyle {
                    flow: child_flow,
                    containing_flow: parent_flow,
                    size: if child_flow.is_horizontal() {
                        crate::BlockDimensions::new(
                            BlockSizeValue::Length(FlowLength::px(80.0)),
                            BlockSizeValue::Auto,
                        )
                    } else {
                        crate::BlockDimensions::new(
                            BlockSizeValue::Auto,
                            BlockSizeValue::Length(FlowLength::px(80.0)),
                        )
                    },
                    ..BlockStyle::default()
                },
                Style {
                    display: Display::Block,
                    ..Style::default()
                },
                &[leaf],
                1,
            );
            let root = tree.new_with_children_and_block_style(
                AlgorithmKind::Block,
                BlockStyle {
                    flow: parent_flow,
                    containing_flow: parent_flow,
                    size: if parent_flow.is_horizontal() {
                        crate::BlockDimensions::new(
                            BlockSizeValue::Length(FlowLength::px(200.0)),
                            BlockSizeValue::Auto,
                        )
                    } else {
                        crate::BlockDimensions::new(
                            BlockSizeValue::Auto,
                            BlockSizeValue::Length(FlowLength::px(200.0)),
                        )
                    },
                    establishes_bfc: true,
                    ..BlockStyle::default()
                },
                Style {
                    display: Display::Block,
                    ..Style::default()
                },
                &[child],
                0,
            );

            tree.compute_layout_with_measure(
                root,
                available(200.0, 200.0),
                |known, _available, _node, _context, _line_constraints| {
                    AlgorithmSize::new(known.width.unwrap_or(20.0), known.height.unwrap_or(20.0))
                },
            );

            assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Buckram));
            assert_eq!(tree.block_algorithm(child), Some(BlockAlgorithm::Buckram));
            tree.layout(child)
        }

        let horizontal_parent = FlowAxes::HORIZONTAL_LTR;
        let vertical_child = FlowAxes::new(WritingMode::VerticalRl, Direction::Ltr);
        assert_eq!(
            layout_nested(horizontal_parent, vertical_child),
            AlgorithmLayout {
                width: 20.0,
                height: 80.0,
                ..AlgorithmLayout::default()
            }
        );

        let vertical_parent = FlowAxes::new(WritingMode::VerticalLr, Direction::Ltr);
        let horizontal_child = FlowAxes::HORIZONTAL_LTR;
        assert_eq!(
            layout_nested(vertical_parent, horizontal_child),
            AlgorithmLayout {
                width: 80.0,
                height: 20.0,
                ..AlgorithmLayout::default()
            }
        );
    }

    #[test]
    fn orthogonal_percentage_width_uses_available_physical_fallback() {
        use crate::{Direction, WritingMode};

        let vertical = FlowAxes::new(WritingMode::VerticalLr, Direction::Ltr);
        let horizontal = FlowAxes::HORIZONTAL_LTR;
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let text = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle::anonymous(horizontal, horizontal),
            Style::default(),
            &[],
            2,
        );
        let child = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                flow: horizontal,
                containing_flow: vertical,
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Length(FlowLength::percent(0.5)),
                    BlockSizeValue::Auto,
                ),
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                ..Style::default()
            },
            &[text],
            1,
        );
        let root = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                flow: vertical,
                containing_flow: vertical,
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                ..Style::default()
            },
            &[child],
            0,
        );

        tree.compute_layout_with_measure(
            root,
            available(300.0, 200.0),
            |known, _available, _node, _context, _line_constraints| {
                AlgorithmSize::new(
                    known.width.unwrap_or_default(),
                    known.height.unwrap_or(40.0),
                )
            },
        );

        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Buckram));
        assert_eq!(tree.block_algorithm(child), Some(BlockAlgorithm::Buckram));
        assert_eq!(
            tree.layout(child),
            AlgorithmLayout {
                width: 150.0,
                height: 40.0,
                ..AlgorithmLayout::default()
            }
        );
    }

    #[test]
    fn orthogonal_float_continuation_stays_deferred_without_a_logical_transform() {
        use crate::{Direction, WritingMode};

        let horizontal = FlowAxes::HORIZONTAL_LTR;
        let vertical = FlowAxes::new(WritingMode::VerticalRl, Direction::Ltr);
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let orthogonal_float = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                flow: vertical,
                containing_flow: horizontal,
                float: FloatSide::Left,
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Length(FlowLength::px(40.0)),
                    BlockSizeValue::Length(FlowLength::px(80.0)),
                ),
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::length(40.0),
                    height: Dimension::length(80.0),
                },
                ..Style::default()
            },
            &[],
            1,
        );
        let root = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::length(200.0),
                    height: Dimension::auto(),
                },
                ..Style::default()
            },
            &[orthogonal_float],
            0,
        );

        tree.compute_layout_with_measure(root, available(200.0, 200.0), zero_measure);

        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Taffy));
        assert_eq!(
            tree.block_deferral(root),
            Some(BlockDeferral::NestedFloatState)
        );
    }

    #[test]
    fn taffy_block_fallback_retains_the_css_facing_deferral() {
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let child = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle::default(),
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::auto(),
                    height: Dimension::length(20.0),
                },
                ..Style::default()
            },
            &[],
            1,
        );
        let root = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                replaced: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::length(200.0),
                    height: Dimension::auto(),
                },
                ..Style::default()
            },
            &[child],
            0,
        );

        tree.compute_layout_with_measure(root, available(200.0, 200.0), zero_measure);

        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Taffy));
        assert_eq!(tree.block_deferral(root), Some(BlockDeferral::Replaced));
        assert_eq!(tree.block_algorithm(child), Some(BlockAlgorithm::Taffy));
        assert_eq!(
            tree.block_deferral(child),
            Some(BlockDeferral::BackendSizingMode)
        );
    }

    fn leaf_with_height(
        tree: &mut AlgorithmTree<Style, (), u8>,
        height: f32,
        source: u8,
    ) -> AlgorithmNodeId {
        tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Auto,
                    BlockSizeValue::Length(crate::FlowLength::px(height)),
                ),
                ..BlockStyle::default()
            },
            Style {
                size: taffy::Size {
                    width: Dimension::auto(),
                    height: Dimension::length(height),
                },
                ..Style::default()
            },
            &[],
            source,
        )
    }

    fn bfc_root_with_children(
        tree: &mut AlgorithmTree<Style, (), u8>,
        width: f32,
        children: &[AlgorithmNodeId],
    ) -> AlgorithmNodeId {
        tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                size: taffy::Size {
                    width: Dimension::length(width),
                    height: Dimension::auto(),
                },
                ..Style::default()
            },
            children,
            0,
        )
    }

    /// Lane 8 (2026-08-21): a table wrapper box is an opaque block child of
    /// the owned block formatter. Its `auto` inline size is the grid's
    /// border-edge inline size (CSS Tables 3 section 2.2.1), so `margin: auto`
    /// centers it by that width, and neither it nor its parent falls back to
    /// Taffy because the grid establishes its own formatting context.
    #[test]
    fn buckram_admits_a_table_wrapper_as_an_opaque_block_sized_by_its_grid() {
        let mut tree: AlgorithmTree<Style, (), u8> = AlgorithmTree::new();
        let cells = (0..2)
            .map(|index| {
                tree.new_with_children_and_block_style(
                    AlgorithmKind::Block,
                    BlockStyle::default(),
                    Style::default(),
                    &[],
                    index,
                )
            })
            .collect::<Vec<_>>();
        let grid = tree.new_with_children_and_block_style(
            AlgorithmKind::Table,
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style::default(),
            &cells,
            9,
        );
        // The wrapper carries the table's margins (CSS 2.1 section 17.4).
        let wrapper = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                establishes_bfc: true,
                margin: PhysicalSides {
                    top: FlowLengthAuto::ZERO,
                    right: FlowLengthAuto::Auto,
                    bottom: FlowLengthAuto::ZERO,
                    left: FlowLengthAuto::Auto,
                },
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                margin: Rect {
                    left: taffy::LengthPercentageAuto::auto(),
                    right: taffy::LengthPercentageAuto::auto(),
                    top: taffy::LengthPercentageAuto::length(0.0),
                    bottom: taffy::LengthPercentageAuto::length(0.0),
                },
                ..Style::default()
            },
            &[grid],
            10,
        );
        let after = leaf_with_height(&mut tree, 10.0, 11);
        let root = bfc_root_with_children(&mut tree, 200.0, &[wrapper, after]);
        let decided = [
            AlgorithmLayout {
                x: 0.0,
                y: 0.0,
                width: 60.0,
                height: 25.0,
            },
            AlgorithmLayout {
                x: 60.0,
                y: 0.0,
                width: 40.0,
                height: 25.0,
            },
        ];
        for (cell, layout) in cells.iter().zip(decided) {
            tree.set_layout(*cell, layout);
        }
        tree.set_layout(
            grid,
            AlgorithmLayout {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 25.0,
            },
        );
        tree.set_table_wrapper_inline_size(wrapper, 100.0);

        tree.compute_layout_with_measure(root, available(200.0, 200.0), zero_measure);

        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Buckram));
        assert_eq!(tree.block_algorithm(wrapper), Some(BlockAlgorithm::Buckram));
        assert_eq!(
            tree.layout(wrapper),
            AlgorithmLayout {
                x: 50.0,
                y: 0.0,
                width: 100.0,
                height: 25.0,
            },
            "the wrapper is as wide as its grid and centered by its auto margins"
        );
        assert_eq!(
            tree.layout(grid),
            AlgorithmLayout {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 25.0,
            }
        );
        for (cell, expected) in cells.iter().zip(decided) {
            assert_eq!(tree.layout(*cell), expected);
        }
        assert_eq!(tree.layout(after).y, 25.0);
        assert_eq!(tree.layout(root).height, 35.0);
    }

    /// A flow-root whose own subtree defers (a replaced child) is laid out by
    /// Taffy on its own, and its parent places it with margins modeled from
    /// its style: 30px below the previous sibling's margin, not deferred.
    #[test]
    fn a_deferred_flow_root_child_does_not_defer_its_buckram_parent() {
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let replaced = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                replaced: true,
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Length(crate::FlowLength::px(50.0)),
                    BlockSizeValue::Length(crate::FlowLength::px(30.0)),
                ),
                ..BlockStyle::default()
            },
            Style {
                size: taffy::Size {
                    width: Dimension::length(50.0),
                    height: Dimension::length(30.0),
                },
                ..Style::default()
            },
            &[],
            1,
        );
        let flow_root = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                establishes_bfc: true,
                margin: PhysicalSides {
                    top: FlowLengthAuto::Value(crate::FlowLength::px(10.0)),
                    right: FlowLengthAuto::ZERO,
                    bottom: FlowLengthAuto::Value(crate::FlowLength::px(20.0)),
                    left: FlowLengthAuto::ZERO,
                },
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                margin: Rect {
                    left: taffy::LengthPercentageAuto::length(0.0),
                    right: taffy::LengthPercentageAuto::length(0.0),
                    top: taffy::LengthPercentageAuto::length(10.0),
                    bottom: taffy::LengthPercentageAuto::length(20.0),
                },
                ..Style::default()
            },
            &[replaced],
            2,
        );
        let before = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Auto,
                    BlockSizeValue::Length(crate::FlowLength::px(40.0)),
                ),
                margin: PhysicalSides {
                    top: FlowLengthAuto::ZERO,
                    right: FlowLengthAuto::ZERO,
                    bottom: FlowLengthAuto::Value(crate::FlowLength::px(30.0)),
                    left: FlowLengthAuto::ZERO,
                },
                ..BlockStyle::default()
            },
            Style {
                size: taffy::Size {
                    width: Dimension::auto(),
                    height: Dimension::length(40.0),
                },
                margin: Rect {
                    left: taffy::LengthPercentageAuto::length(0.0),
                    right: taffy::LengthPercentageAuto::length(0.0),
                    top: taffy::LengthPercentageAuto::length(0.0),
                    bottom: taffy::LengthPercentageAuto::length(30.0),
                },
                ..Style::default()
            },
            &[],
            3,
        );
        let after = leaf_with_height(&mut tree, 10.0, 4);
        let root = bfc_root_with_children(&mut tree, 200.0, &[before, flow_root, after]);

        tree.compute_layout_with_measure(root, available(200.0, 200.0), zero_measure);

        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Buckram));
        assert_eq!(tree.block_algorithm(flow_root), Some(BlockAlgorithm::Taffy));
        assert_eq!(
            tree.block_deferral(flow_root),
            Some(BlockDeferral::Replaced)
        );
        assert_eq!(
            tree.layout(flow_root),
            AlgorithmLayout {
                x: 0.0,
                y: 70.0,
                width: 200.0,
                height: 30.0,
            },
            "30px bottom margin and 10px top margin collapse to 30px below a 40px sibling"
        );
        assert_eq!(tree.layout(after).y, 120.0);
        assert_eq!(tree.layout(root).height, 130.0);
    }

    /// CSS 2.1 section 9.5 still applies: beside an active float, a BFC child
    /// without the avoidance opt-in defers with the float deferral, not with a
    /// formatting-context deferral, and the parent falls back as before.
    #[test]
    fn a_table_beside_an_active_float_still_defers_to_float_avoidance() {
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let float = tree.new_with_children_and_block_style(
            AlgorithmKind::Leaf,
            BlockStyle {
                size: crate::BlockDimensions::new(
                    BlockSizeValue::Length(crate::FlowLength::px(80.0)),
                    BlockSizeValue::Length(crate::FlowLength::px(40.0)),
                ),
                float: FloatSide::Left,
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style {
                size: taffy::Size {
                    width: Dimension::length(80.0),
                    height: Dimension::length(40.0),
                },
                ..Style::default()
            },
            &[],
            1,
        );
        let grid = tree.new_with_children_and_block_style(
            AlgorithmKind::Table,
            BlockStyle {
                establishes_bfc: true,
                ..BlockStyle::default()
            },
            Style::default(),
            &[],
            2,
        );
        tree.set_layout(
            grid,
            AlgorithmLayout {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 25.0,
            },
        );
        let root = bfc_root_with_children(&mut tree, 200.0, &[float, grid]);

        tree.compute_layout_with_measure(root, available(200.0, 200.0), zero_measure);

        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Taffy));
        assert_eq!(
            tree.block_deferral(root),
            Some(BlockDeferral::FloatFormattingContextAvoidance)
        );
    }

    /// A relatively positioned scroll container establishes a BFC but is not
    /// a float-avoidance candidate in Livery; without an active float it is an
    /// ordinary opaque block child.
    #[test]
    fn a_relatively_positioned_scroll_container_is_an_opaque_block_child() {
        let mut tree = AlgorithmTree::<Style, (), u8>::new();
        let inner = leaf_with_height(&mut tree, 10.0, 1);
        let scroll = tree.new_with_children_and_block_style(
            AlgorithmKind::Block,
            BlockStyle {
                establishes_bfc: true,
                position: crate::BlockPosition::Relative,
                margin: PhysicalSides {
                    top: FlowLengthAuto::Value(crate::FlowLength::px(10.0)),
                    right: FlowLengthAuto::ZERO,
                    bottom: FlowLengthAuto::ZERO,
                    left: FlowLengthAuto::ZERO,
                },
                ..BlockStyle::default()
            },
            Style {
                display: Display::Block,
                margin: Rect {
                    left: taffy::LengthPercentageAuto::length(0.0),
                    right: taffy::LengthPercentageAuto::length(0.0),
                    top: taffy::LengthPercentageAuto::length(10.0),
                    bottom: taffy::LengthPercentageAuto::length(0.0),
                },
                ..Style::default()
            },
            &[inner],
            2,
        );
        let root = bfc_root_with_children(&mut tree, 200.0, &[scroll]);

        tree.compute_layout_with_measure(root, available(200.0, 200.0), zero_measure);

        assert_eq!(tree.block_algorithm(root), Some(BlockAlgorithm::Buckram));
        assert_eq!(tree.block_algorithm(scroll), Some(BlockAlgorithm::Buckram));
        assert_eq!(
            tree.layout(scroll),
            AlgorithmLayout {
                x: 0.0,
                y: 10.0,
                width: 200.0,
                height: 10.0,
            }
        );
        assert_eq!(tree.layout(root).height, 20.0);
    }
}
