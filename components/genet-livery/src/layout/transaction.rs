//! The layout transaction: the public entries, the pass that builds the
//! block and inline algorithm trees, lays out the atomic subtrees, and
//! folds the inline groups back into one retained result.

use super::*;

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

pub(in crate::layout) fn layout_impl<D>(
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

pub(in crate::layout) fn layout_atomic_subtrees<D>(
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

pub(in crate::layout) fn collect_atomic_fragments(
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

pub(in crate::layout) fn merge_atomic_subtrees<Id>(
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

pub(in crate::layout) fn append_atomic_fragment<Id>(
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

pub(in crate::layout) fn layout_inline_groups<D>(
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
