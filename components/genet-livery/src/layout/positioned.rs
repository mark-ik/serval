//! Out-of-flow placement: static positions, relative offsets, and the
//! absolute/fixed containing-block and inset resolution.

use super::*;

/// Publish a static-position rectangle at the formatting boundary that
/// produced it. The selected absolute or fixed containing block comes from
/// Buckram's K5a box graph; the backend never chooses it here.
pub(in crate::layout) fn static_position_record<Id>(
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

pub(in crate::layout) fn record_static_position<Id>(
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
pub(in crate::layout) fn apply_relative_positioning<D>(
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
pub(in crate::layout) fn definite_containing_block_size<Id>(
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
pub(in crate::layout) fn stretched_item_block_size_is_definite<Id>(
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
    // `normal` is the initial value of the items properties and behaves as
    // `stretch` in flex, so a flex item under a default container still gets a
    // definite cross size. Grid is where `normal` and `stretch` diverge, and
    // only for a replaced item with a natural size; that divergence is resolved
    // inside the grid algorithm rather than here.
    let stretch = |alignment: CssAlignment| {
        matches!(
            alignment,
            CssAlignment::Auto | CssAlignment::Normal | CssAlignment::Stretch
        )
    };
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
                    alignment => stretch(alignment),
                }
        },
        _ => false,
    }
}

#[derive(Clone, Copy)]
pub(in crate::layout) struct PositionedPlacement {
    pub(in crate::layout) box_id: BoxId,
    pub(in crate::layout) root: FragmentId,
    pub(in crate::layout) containing_fragment: Option<FragmentId>,
    pub(in crate::layout) current: PhysicalRect,
    pub(in crate::layout) containing_rect: PhysicalRect,
    pub(in crate::layout) containing_flow: FlowAxes,
    pub(in crate::layout) containing_size: buckram::LogicalSize,
    pub(in crate::layout) style: BlockStyle,
    pub(in crate::layout) geometry: buckram::PositionedBoxGeometry,
}

impl PositionedPlacement {
    pub(in crate::layout) fn target_rect(self) -> PhysicalRect {
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
    pub(in crate::layout) fn formatter_inline_size(self) -> Option<f32> {
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
    pub(in crate::layout) fn formatter_block_size(self) -> Option<f32> {
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
pub(in crate::layout) fn positioned_placements<D>(
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
pub(in crate::layout) fn positioned_contain_intrinsic_inline(
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
pub(in crate::layout) fn positioned_containing_block_rect<Id>(
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
pub(in crate::layout) fn positioned_inline_containing_block_rect<Id>(
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
pub(in crate::layout) fn apply_admitted_positioned_inline_sizes<Context, Source>(
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
pub(in crate::layout) fn apply_absolute_and_fixed_positioning<D>(
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

pub(in crate::layout) fn fragment_baselines<Id, Context, Source>(
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
