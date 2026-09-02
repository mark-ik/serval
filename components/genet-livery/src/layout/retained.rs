//! Retained-root formatting: one bounded pass over a changed subtree when
//! the surrounding layout can be kept, and the predicates that decide
//! whether it can.

use super::*;

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

pub(in crate::layout) fn supports_retained_root_formatting<Id>(
    boxes: &GeneratedBoxTree<Id>,
    root: BoxId,
) -> bool
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

pub(in crate::layout) fn retained_root_box<Id>(
    boxes: &buckram::CssBoxTree<Id>,
    node: Id,
) -> Option<BoxId>
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

pub(in crate::layout) fn box_is_descendant_of<Id>(
    boxes: &buckram::CssBoxTree<Id>,
    box_id: BoxId,
    root: BoxId,
) -> bool
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

pub(in crate::layout) fn supports_retained_table_root_formatting<Id>(
    boxes: &GeneratedBoxTree<Id>,
    root: BoxId,
) -> bool
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

pub(in crate::layout) fn retained_ancestor_styles_unchanged<Id>(
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

pub(in crate::layout) fn inherited_font_size<Id>(
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

pub(in crate::layout) fn same_retained_root_size(left: PhysicalRect, right: PhysicalRect) -> bool {
    (left.width - right.width).abs() <= 0.01 && (left.height - right.height).abs() <= 0.01
}
