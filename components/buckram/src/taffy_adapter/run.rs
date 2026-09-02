//! One run of the layout algorithms over an [`AlgorithmTree`].
//!
//! `AlgorithmRun` borrows the tree for the duration of a single
//! layout pass and carries the per-run state Taffy's traits need: the
//! float line constraints, nested float state, and the intrinsic
//! inline-size computation the block path queries.

use super::*;

/// The leaf measure callback, type-erased.
///
/// Erased on purpose (2026-09-02). Taken by value as a type parameter, every
/// call site's closure made its own `AlgorithmRun` type, and every one of
/// Taffy's tree traits — the whole algorithm — was instantiated once per
/// closure: fourteen copies in graphshell-web alone, ~44k functions, and the
/// DWARF that crashed the wasm linker. Behind `dyn` there is one copy per
/// `(Context, Source)` pair, for an indirect call per leaf measure.
pub(in crate::taffy_adapter) type MeasureFn<'m, Context> = dyn FnMut(
        AlgorithmSize<Option<f32>>,
        AlgorithmSize<AlgorithmAvailableSpace>,
        AlgorithmNodeId,
        Option<&mut Context>,
        Option<&FloatLineConstraints>,
    ) -> AlgorithmSize<f32>
    + 'm;

pub(in crate::taffy_adapter) struct AlgorithmRun<'a, S, Context, Source> {
    pub(in crate::taffy_adapter) tree: &'a mut AlgorithmTree<S, Context, Source>,
    pub(in crate::taffy_adapter) measure: &'a mut MeasureFn<'a, Context>,
    pub(in crate::taffy_adapter) line_constraints: Option<FloatLineConstraints>,
    pub(in crate::taffy_adapter) nested_float_state: Option<FloatContextState>,
    pub(in crate::taffy_adapter) resolved_shrink_to_fit: Option<AlgorithmNodeId>,
    pub(in crate::taffy_adapter) fixed_leaf_intrinsics_enabled: bool,
    pub(in crate::taffy_adapter) marker: PhantomData<&'a mut Context>,
}

#[derive(Clone, Copy)]
pub(in crate::taffy_adapter) struct BlockChildInput {
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

pub(in crate::taffy_adapter) struct PendingBlockChildLayout {
    child: AlgorithmNodeId,
    order: u32,
    output: LayoutOutput,
    padding: PhysicalSides<f32>,
    border: PhysicalSides<f32>,
    margin: PhysicalSides<f32>,
    logical_rect: LogicalRect,
    static_position: bool,
}

impl<S, Context, Source> AlgorithmRun<'_, S, Context, Source>
where
    S: AlgorithmStyle,
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
            let admitted_orthogonal_flex_auto_block = admits_orthogonal_auto_block_flex_child(
                self.tree.nodes[node.index()].block_style,
                child_node.kind,
                child_style,
                self.style(child.into_taffy()).flex_direction,
            );
            // Float exclusions live in their owning BFC's logical axes. Do
            // not copy physical left/right state into an orthogonal child;
            // only same-flow continuation is admitted until that transform
            // has its own modeled contract.
            if child_style.flow.is_horizontal() != child_style.containing_flow.is_horizontal()
                && (child_style.float != FloatSide::None
                    || child_style.clear != ClearSide::None
                    || *active_left_float
                    || *active_right_float
                    // A flex item's float is ignored by flex layout. The
                    // admitted vertical row shape therefore cannot export
                    // that descendant float into its horizontal BFC.
                    || (!admitted_orthogonal_flex_auto_block && self.exports_float_state(child)))
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
                // The one admitted exception is a horizontal BFC's
                // orthogonal, vertical flex child with a definite logical
                // inline size and an automatic logical block size. Its width
                // is Taffy's flex cross size, so do not leak the parent's
                // solved horizontal inline size into that axis.
                if !floats_are_active
                    && matches!(child_node.kind, AlgorithmKind::Flex | AlgorithmKind::Grid)
                    && !admitted_orthogonal_flex_auto_block
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

    pub(in crate::taffy_adapter) fn measure_intrinsic_inline_subtree(
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
                    // Floats participate in a max-content query as an
                    // unwrapped float line. Keep adjacent floats together,
                    // but end the line before an in-flow block or `clear`.
                    let mut float_min_content = 0.0_f32;
                    let mut float_max_content = 0.0_f32;
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
                        if child_style.float != FloatSide::None {
                            if child_style.clear != ClearSide::None {
                                min_content = min_content.max(float_min_content);
                                max_content = max_content.max(float_max_content);
                                float_min_content = 0.0;
                                float_max_content = 0.0;
                            }
                            float_min_content = float_min_content.max(child_sizes.min_content);
                            float_max_content += child_sizes.max_content;
                            continue;
                        }
                        min_content = min_content.max(float_min_content);
                        max_content = max_content.max(float_max_content);
                        float_min_content = 0.0;
                        float_max_content = 0.0;
                        min_content = min_content.max(child_sizes.min_content);
                        max_content = max_content.max(child_sizes.max_content);
                    }
                    min_content = min_content.max(float_min_content);
                    max_content = max_content.max(float_max_content);
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
            // Capability admission decides whether a horizontal direction
            // boundary may use this transform. Once admitted, the block axis
            // is unchanged and Buckram mirrors the inline geometry.
            && (child_style.flow == child_style.containing_flow
                || child_style.flow.is_horizontal()
                    && child_style.containing_flow.is_horizontal())
    }

    fn child_content_inline_size(
        child_style: BlockStyle,
        child_size: PhysicalSize,
        containing_inline_size: f32,
    ) -> f32 {
        let padding_border = child_style.content_logical_padding_border(containing_inline_size);
        (child_style.flow.logical_size(child_size).inline
            - padding_border.inline_start
            - padding_border.inline_end)
            .max(0.0)
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

    /// Whether a row flex item's used basis falls through to its content.
    ///
    /// `auto` consults the preferred main size first, while `content` skips
    /// it. The narrow bridge below only covers the shared content case where
    /// `auto` has no preferred width to use.
    fn row_flex_basis_uses_content(&self, node: AlgorithmNodeId) -> bool {
        let flex_basis = self.style(node.into_taffy()).flex_basis;
        flex_basis.is_content()
            || (flex_basis.is_auto()
                && matches!(
                    self.tree.nodes[node.index()].block_style.size.width,
                    BlockSizeValue::Auto
                ))
    }

    /// The column counterpart of [`Self::row_flex_basis_uses_content`].
    ///
    /// In a column, `auto` first consults the preferred height. It reaches
    /// the content-sizing path only when that preferred height is itself
    /// automatic.
    fn column_flex_basis_uses_content(&self, node: AlgorithmNodeId) -> bool {
        let flex_basis = self.style(node.into_taffy()).flex_basis;
        flex_basis.is_content()
            || (flex_basis.is_auto()
                && matches!(
                    self.tree.nodes[node.index()].block_style.size.height,
                    BlockSizeValue::Auto
                ))
    }

    fn flex_basis_uses_content(&self, node: AlgorithmNodeId) -> bool {
        let Some(parent) = self.tree.nodes[node.index()].parent else {
            return false;
        };
        if self.tree.nodes[parent.index()].kind != AlgorithmKind::Flex {
            return false;
        }
        match self.style(parent.into_taffy()).flex_direction {
            taffy::FlexDirection::Row | taffy::FlexDirection::RowReverse => {
                self.row_flex_basis_uses_content(node)
            },
            taffy::FlexDirection::Column | taffy::FlexDirection::ColumnReverse => {
                self.column_flex_basis_uses_content(node)
            },
        }
    }

    /// Derive the fit-content inline border box Flexbox §9.2 Step E needs
    /// while it measures a column item's content-based main size.
    fn column_flex_content_inline_size(
        &mut self,
        node: AlgorithmNodeId,
        style: BlockStyle,
        inputs: LayoutInput,
    ) -> Result<f32, BlockDeferral> {
        let constraint = inputs
            .parent_size
            .width
            .or(inputs.available_space.width.into_option())
            .filter(|width| width.is_finite() && *width >= 0.0)
            .ok_or(BlockDeferral::IndefiniteInlineSize)?;
        // Flexbox substitutes fit-content for an indefinite automatic cross
        // size during this main-axis query. Reuse the admitted intrinsic
        // provider, but keep the stored style unchanged for the subsequent
        // owned-block format at the resolved width.
        let original_style = style;
        let mut query_style = style;
        query_style.shrink_to_fit = true;
        self.clear_subtree_cache(node);
        self.tree.nodes[node.index()].block_style = query_style;
        let result = self.resolve_intrinsic_shrink_to_fit(node, query_style, constraint);
        self.tree.nodes[node.index()].block_style = original_style;
        self.clear_subtree_cache(node);
        result.map(|used_inline| used_inline.border_box)
    }

    /// Answer Flexbox §9.2 Step E's max-content query for an ordinary block
    /// subtree. The general owned-block route deliberately accepts only a
    /// full normal-flow layout; using Taffy's generic block fallback here
    /// loses Buckram's intrinsic inline-subtree measurement before the flex
    /// algorithm can establish the base size.
    fn compute_flex_base_intrinsic_block(
        &mut self,
        node: AlgorithmNodeId,
        inputs: LayoutInput,
    ) -> Result<Option<LayoutOutput>, BlockDeferral> {
        let parent_direction = self.tree.nodes[node.index()].parent.and_then(|parent| {
            (self.tree.nodes[parent.index()].kind == AlgorithmKind::Flex)
                .then(|| self.style(parent.into_taffy()).flex_direction)
        });
        let parent_is_row_flex = matches!(
            parent_direction,
            Some(taffy::FlexDirection::Row | taffy::FlexDirection::RowReverse)
        );
        let parent_is_column_flex = matches!(
            parent_direction,
            Some(taffy::FlexDirection::Column | taffy::FlexDirection::ColumnReverse)
        );
        let exact_row_flex_base_query = parent_is_row_flex
            && inputs.run_mode == RunMode::ComputeSize
            && is_content_sizing_mode(inputs.sizing_mode)
            && inputs.axis == taffy::RequestedAxis::Horizontal
            && inputs.available_space.width == taffy::AvailableSpace::MaxContent
            && self.row_flex_basis_uses_content(node);
        let exact_column_flex_base_query = parent_is_column_flex
            && !self.tree.nodes[node.index()].flex_content_base_probe_active
            && inputs.run_mode == RunMode::ComputeSize
            && is_content_sizing_mode(inputs.sizing_mode)
            && inputs.axis == taffy::RequestedAxis::Vertical
            && inputs.known_dimensions.height.is_none()
            && inputs.available_space.height == taffy::AvailableSpace::MaxContent
            && self.column_flex_basis_uses_content(node);
        let exact_column_cross_query = parent_is_column_flex
            && self.tree.nodes[node.index()].flex_content_base_query
            && inputs.run_mode == RunMode::ComputeSize
            && is_content_sizing_mode(inputs.sizing_mode)
            && inputs.axis == taffy::RequestedAxis::Horizontal
            && inputs.known_dimensions.width.is_none()
            && inputs.known_dimensions.height.is_some()
            && self.column_flex_basis_uses_content(node);
        if !exact_row_flex_base_query && !exact_column_flex_base_query && !exact_column_cross_query
        {
            return Ok(None);
        }

        let style = self.tree.nodes[node.index()].block_style;
        if !style.flow.is_horizontal()
            || style.flow != style.containing_flow
            || style.deferral().is_some()
        {
            return Err(BlockDeferral::BackendSizingMode);
        }
        if exact_column_cross_query {
            let width = self.column_flex_content_inline_size(node, style, inputs)?;
            return Ok(Some(LayoutOutput::from_outer_size(taffy::Size {
                width,
                height: inputs
                    .known_dimensions
                    .height
                    .expect("column cross query requires a resolved main size"),
            })));
        }

        if exact_column_flex_base_query {
            let width = match inputs.known_dimensions.width {
                Some(width) => width,
                None => self.column_flex_content_inline_size(node, style, inputs)?,
            };
            // The fitting-width query establishes only the cross size. The
            // existing owned-block formatter still supplies the block-axis
            // contribution that Step E adopts as the flex base size.
            let previous_marker = self.tree.nodes[node.index()].flex_content_base_query;
            let previous_probe_active =
                self.tree.nodes[node.index()].flex_content_base_probe_active;
            self.tree.nodes[node.index()].flex_content_base_query = true;
            self.tree.nodes[node.index()].flex_content_base_probe_active = true;
            // `content` replaces the preferred main size for this query.
            // `auto` reaches the same branch only when its preferred height
            // is already automatic, so it needs no substitution.
            let ignores_preferred_height = self.style(node.into_taffy()).flex_basis.is_content();
            if ignores_preferred_height {
                let mut probe_style = style;
                probe_style.size.height = BlockSizeValue::Auto;
                self.clear_subtree_cache(node);
                self.tree.nodes[node.index()].block_style = probe_style;
            }
            let result = self.compute_owned_block_layout(
                node.into_taffy(),
                LayoutInput {
                    run_mode: RunMode::ComputeSize,
                    sizing_mode: SizingMode::ContentSize,
                    axis: taffy::RequestedAxis::Vertical,
                    known_dimensions: taffy::Size {
                        width: Some(width),
                        height: None,
                    },
                    parent_size: inputs.parent_size,
                    available_space: inputs.available_space,
                    vertical_margins_are_collapsible: taffy::Line::FALSE,
                },
            );
            if ignores_preferred_height {
                self.tree.nodes[node.index()].block_style = style;
                self.clear_subtree_cache(node);
            }
            self.tree.nodes[node.index()].flex_content_base_probe_active = previous_probe_active;
            if result.is_err() {
                self.tree.nodes[node.index()].flex_content_base_query = previous_marker;
            }
            return result.map(Some);
        }

        // Step E substitutes the used flex basis for the item's main size.
        // A definite preferred inline size therefore cannot participate in
        // this query, even though it remains authored for the final layout.
        let original_style = style;
        let mut intrinsic_style = style;
        intrinsic_style.size.width = BlockSizeValue::Auto;
        self.clear_subtree_cache(node);
        self.tree.nodes[node.index()].block_style = intrinsic_style;
        let previous_fixed_leaf_intrinsics =
            std::mem::replace(&mut self.fixed_leaf_intrinsics_enabled, true);
        let intrinsic = self.measure_intrinsic_inline_subtree(node, None);
        self.fixed_leaf_intrinsics_enabled = previous_fixed_leaf_intrinsics;
        self.tree.nodes[node.index()].block_style = original_style;
        self.clear_subtree_cache(node);
        let intrinsic = intrinsic?;
        // This exact query succeeded, so the final layout may take Buckram's
        // normal block-formatting route even though its direct parent is a
        // flex container. Other flex children keep Taffy's fallback.
        self.tree.nodes[node.index()].flex_content_base_query = true;
        let (padding_border_start, padding_border_end) = intrinsic_inline_padding_border(style)?;
        let width = intrinsic.max_content + padding_border_start + padding_border_end;
        Ok(Some(LayoutOutput::from_outer_size(taffy::Size {
            width,
            height: inputs.known_dimensions.height.unwrap_or(0.0),
        })))
    }

    fn compute_owned_block_layout(
        &mut self,
        node_id: NodeId,
        inputs: LayoutInput,
    ) -> Result<LayoutOutput, BlockDeferral> {
        let node = AlgorithmNodeId::from_taffy(node_id);
        if let Some(output) = self.compute_flex_base_intrinsic_block(node, inputs)? {
            return Ok(output);
        }
        let node_index = node.index();
        let measured_content_flex_item = self.tree.nodes[node_index].flex_content_base_query
            && self.flex_basis_uses_content(node)
            && self.tree.nodes[node_index]
                .parent
                .is_some_and(|parent| self.tree.nodes[parent.index()].kind == AlgorithmKind::Flex);
        let parent_is_row_flex = self.tree.nodes[node_index].parent.is_some_and(|parent| {
            self.tree.nodes[parent.index()].kind == AlgorithmKind::Flex
                && matches!(
                    self.style(parent.into_taffy()).flex_direction,
                    taffy::FlexDirection::Row | taffy::FlexDirection::RowReverse
                )
        });
        let parent_is_column_flex = self.tree.nodes[node_index].parent.is_some_and(|parent| {
            self.tree.nodes[parent.index()].kind == AlgorithmKind::Flex
                && matches!(
                    self.style(parent.into_taffy()).flex_direction,
                    taffy::FlexDirection::Column | taffy::FlexDirection::ColumnReverse
                )
        });
        let content_row_height_query = measured_content_flex_item
            && parent_is_row_flex
            && inputs.run_mode == RunMode::ComputeSize
            && is_content_sizing_mode(inputs.sizing_mode)
            && inputs.axis == taffy::RequestedAxis::Vertical
            && inputs.known_dimensions.width.is_some()
            && inputs.known_dimensions.height.is_none()
            && inputs.available_space.height == taffy::AvailableSpace::MaxContent;
        // A column Step E query first supplies the fit-content inline size
        // itself, then asks Buckram for the item's auto block contribution.
        let content_column_base_probe = measured_content_flex_item
            && parent_is_column_flex
            && inputs.run_mode == RunMode::ComputeSize
            && is_content_sizing_mode(inputs.sizing_mode)
            && inputs.axis == taffy::RequestedAxis::Vertical
            && inputs.known_dimensions.width.is_some()
            && inputs.known_dimensions.height.is_none()
            && inputs.available_space.height == taffy::AvailableSpace::MaxContent;
        // Taffy enters the final flex-item pass with ContentSize after the
        // Step E query. Admit that one pass only after the exact Buckram
        // content measurement above succeeded.
        let final_content_layout = inputs.run_mode == RunMode::PerformLayout
            && inputs.axis == taffy::RequestedAxis::Both
            && (inputs.sizing_mode == SizingMode::InherentSize
                || (measured_content_flex_item && inputs.sizing_mode == SizingMode::ContentSize));
        // Flexbox asks for the auto cross size after Step E fixes the main
        // size. Let the existing owned-block formatter answer that exact
        // query at its supplied width, which preserves a flex item's BFC.
        if !final_content_layout && !content_row_height_query && !content_column_base_probe {
            return Err(BlockDeferral::BackendSizingMode);
        }

        self.tree.nodes[node_index].exported_float_state = None;
        let parent_is_block = self.tree.nodes[node_index]
            .parent
            .is_none_or(|parent| self.tree.nodes[parent.index()].kind == AlgorithmKind::Block);
        if !parent_is_block && !measured_content_flex_item {
            return Err(BlockDeferral::BackendSizingMode);
        }

        let mut style = self.tree.nodes[node_index].block_style;
        // Flex items establish an independent formatting context for their
        // contents. The regular Livery block style records only properties
        // authored on the item itself, so retain that parent-derived fact for
        // the narrow Buckram route above.
        style.establishes_bfc |= measured_content_flex_item;
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
                    let child_content_inline_size =
                        Self::child_content_inline_size(child_style, child_size, content_inline);
                    let float_state = formatting_context.float_state_for_descendant(
                        origin.0,
                        origin.1,
                        child_style.flow,
                        child_content_inline_size,
                    );
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
                    child_style.flow,
                    Self::child_content_inline_size(child_style, child_size, content_inline),
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

pub(in crate::taffy_adapter) fn intrinsic_inline_style_is_admitted(
    style: BlockStyle,
    is_root: bool,
) -> bool {
    if style.flow != style.containing_flow
        || style.position != crate::BlockPosition::Static
        || style.size_containment.width
        || style.size_containment.height
        || style.has_nonlinear_lengths
    {
        return false;
    }
    if !is_root && style.shrink_to_fit {
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
    // A fixed-size float contributes a definite outer box to the parent's
    // max-content float line. Auto-width floats retain the dedicated
    // shrink-to-fit admission instead of being guessed from this traversal.
    if !is_root && style.float != FloatSide::None && intrinsic_absolute_size(inline_size).is_none()
    {
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

pub(in crate::taffy_adapter) fn intrinsic_inline_dimension<T: Copy>(
    dimensions: crate::BlockDimensions<T>,
    flow: FlowAxes,
) -> T {
    if flow.is_horizontal() {
        dimensions.width
    } else {
        dimensions.height
    }
}

pub(in crate::taffy_adapter) fn intrinsic_size_constraint_is_supported(
    value: BlockSizeValue,
    minimum: bool,
) -> bool {
    match value {
        BlockSizeValue::Auto if minimum => true,
        BlockSizeValue::None if !minimum => true,
        BlockSizeValue::Length(_) => true,
        _ => false,
    }
}

pub(in crate::taffy_adapter) fn intrinsic_absolute_size(
    value: BlockSizeValue,
) -> Option<FlowLength> {
    match value {
        BlockSizeValue::Length(value) if value.percentage == 0.0 && value.px.is_finite() => {
            Some(value)
        },
        _ => None,
    }
}

pub(in crate::taffy_adapter) fn intrinsic_absolute_length(value: FlowLength) -> Option<f32> {
    (value.percentage == 0.0 && value.px.is_finite()).then_some(value.px)
}

pub(in crate::taffy_adapter) fn intrinsic_auto_length(value: FlowLengthAuto) -> Option<f32> {
    match value {
        FlowLengthAuto::Auto => Some(0.0),
        FlowLengthAuto::Value(value) => intrinsic_absolute_length(value),
    }
}

pub(in crate::taffy_adapter) fn intrinsic_inline_padding(
    style: BlockStyle,
) -> (FlowLength, FlowLength) {
    if style.containing_flow.is_horizontal() {
        (style.padding.left, style.padding.right)
    } else {
        (style.padding.top, style.padding.bottom)
    }
}

pub(in crate::taffy_adapter) fn intrinsic_inline_margin_values(
    style: BlockStyle,
) -> (FlowLengthAuto, FlowLengthAuto) {
    if style.containing_flow.is_horizontal() {
        (style.margin.left, style.margin.right)
    } else {
        (style.margin.top, style.margin.bottom)
    }
}

pub(in crate::taffy_adapter) fn intrinsic_inline_padding_border(
    style: BlockStyle,
) -> Result<(f32, f32), BlockDeferral> {
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

pub(in crate::taffy_adapter) fn intrinsic_inline_margins(
    style: BlockStyle,
) -> Result<(f32, f32), BlockDeferral> {
    let (margin_start, margin_end) = intrinsic_inline_margin_values(style);
    Ok((
        intrinsic_auto_length(margin_start).ok_or(BlockDeferral::IntrinsicSize)?,
        intrinsic_auto_length(margin_end).ok_or(BlockDeferral::IntrinsicSize)?,
    ))
}

pub(in crate::taffy_adapter) fn intrinsic_definite_inline_content_size(
    style: BlockStyle,
) -> Result<Option<f32>, BlockDeferral> {
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

impl<S, Context, Source> TraversePartialTree for AlgorithmRun<'_, S, Context, Source>
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

impl<S, Context, Source> TraverseTree for AlgorithmRun<'_, S, Context, Source> where
    S: AlgorithmStyle
{
}

impl<S, Context, Source> LayoutPartialTree for AlgorithmRun<'_, S, Context, Source>
where
    S: AlgorithmStyle,
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

    fn resolve_calc_value(&self, value: *const (), basis: f32) -> f32 {
        self.tree
            .calc_resolver
            .map_or(0.0, |resolver| resolver(value, basis))
    }

    fn compute_child_layout(&mut self, node_id: NodeId, inputs: LayoutInput) -> LayoutOutput {
        self.compute_node(node_id, inputs, None)
    }
}

impl<S, Context, Source> CacheTree for AlgorithmRun<'_, S, Context, Source>
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

impl<S, Context, Source> LayoutBlockContainer for AlgorithmRun<'_, S, Context, Source>
where
    S: AlgorithmStyle,
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

impl<S, Context, Source> LayoutFlexboxContainer for AlgorithmRun<'_, S, Context, Source>
where
    S: AlgorithmStyle,
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

impl<S, Context, Source> LayoutGridContainer for AlgorithmRun<'_, S, Context, Source>
where
    S: AlgorithmStyle,
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

impl<S, Context, Source> RoundTree for AlgorithmRun<'_, S, Context, Source>
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
