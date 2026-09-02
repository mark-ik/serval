//! The inline builder: projects an inline group's boxes and atomics into
//! the inline algorithm tree, tables included.

use super::*;

pub(in crate::layout) struct InlineBuildState<'a, D: LayoutDom> {
    pub(in crate::layout) dom: &'a D,
    pub(in crate::layout) styles: &'a StylePlane<D::NodeId>,
    pub(in crate::layout) boxes: &'a GeneratedBoxTree<D::NodeId>,
    pub(in crate::layout) atomic: &'a AtomicLayoutPlane,
    pub(in crate::layout) tree: AlgorithmTree<Style, InlineMeasure, Vec<BoxId>>,
    pub(in crate::layout) image_sources: &'a ImageSources,
    pub(in crate::layout) table_shadow: TableShadowLedger,
    pub(in crate::layout) pending_tables: Vec<PendingTable<D::NodeId>>,
    /// The grid, in-flow cell nodes, and detached table-part nodes for the
    /// table `build_children` just processed, consumed by `build_box` when it
    /// creates the table's algorithm node.
    pub(in crate::layout) pending_table_handoff: Option<(
        TableGrid,
        Vec<Option<AlgorithmNodeId>>,
        Vec<DetachedTablePart>,
    )>,
}

impl<D> InlineBuildState<'_, D>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    pub(in crate::layout) fn build_box(
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
    pub(in crate::layout) fn measure_cell_intrinsics(
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
    pub(in crate::layout) fn measure_caption_min(
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
    pub(in crate::layout) fn apply_buckram_table_layout(&mut self, text: &mut TextSystem) {
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
    pub(in crate::layout) fn size_wrapper_from_grid(&mut self, pending: &PendingTable<D::NodeId>) {
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
    pub(in crate::layout) fn apply_buckram_table_rows(
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
    pub(in crate::layout) fn table_paint_plane(&self) -> TablePaintPlane {
        table_paint_plane(&self.pending_tables, self.boxes, self.styles)
    }

    /// Assert the painted fragments honored every assigned column vector, and
    /// record how far the painted cells sit from Buckram's block rectangles.
    pub(in crate::layout) fn verify_table_layout(
        &mut self,
        live_rect_of: impl Fn(BoxId) -> Option<Fragment>,
    ) {
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
    pub(in crate::layout) fn collect_out_of_flow_table_parts(
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

    pub(in crate::layout) fn build_anonymous_table_grid(
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

    pub(in crate::layout) fn build_children(
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

    pub(in crate::layout) fn build_flow_children(
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
    pub(in crate::layout) fn build_positioned_inline_descendants(
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
    pub(in crate::layout) fn inline_group_is_blank(
        &self,
        roots: &[BoxId],
        _parent_style: &ComputedValues,
    ) -> bool {
        roots.is_empty()
    }

    pub(in crate::layout) fn build_inline_group(
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
