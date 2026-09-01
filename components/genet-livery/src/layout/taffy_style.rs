//! Projection of computed CSS into Taffy's style vocabulary.
//!
//! The flex and grid axis mappings that turn logical CSS into physical
//! Taffy axes, plus the value converters for dimensions, lengths,
//! margins, borders and overflow — including the calc() scratch that
//! lets a percentage-bearing math length resolve against a real basis.

use super::*;

pub(in crate::layout) fn to_taffy_style(computed: &ComputedValues, font_size: f32) -> Style {
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
    let flex_flow = flex_flow_axes(computed);
    let flex_direction = physical_flex_direction(computed.flex_direction, flex_flow);
    let direction = if computed.display == CssDisplay::Flex {
        physical_flex_cross_axis_direction(computed.flex_direction, flex_direction, flex_flow)
    } else {
        TaffyDirection::Ltr
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
        direction,
        flex_wrap: physical_flex_wrap(computed, flex_direction, flex_flow),
        flex_basis: flex_basis(computed.flex_basis, font_size),
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
        gap: physical_flex_gap(computed, font_size, flex_flow),
        align_items: Some(align_items(physical_flex_cross_alignment(
            computed,
            computed.align_items,
            computed.flex_direction,
            flex_direction,
            flex_flow,
        ))),
        // `auto` on the self properties defers to the parent's items value,
        // which is taffy's `None`. A content-keyword size in that axis
        // additionally suppresses stretch (see `suppresses_stretch`).
        align_self: self_alignment(computed.align_self, computed.height),
        justify_items: Some(align_items(computed.justify_items)),
        justify_self: self_alignment(computed.justify_self, computed.width),
        align_content: Some(align_content(physical_flex_cross_alignment(
            computed,
            computed.align_content,
            computed.flex_direction,
            flex_direction,
            flex_flow,
        ))),
        justify_content: Some(physical_flex_justify_content(computed, flex_flow)),
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

pub(in crate::layout) fn flex_flow_axes(computed: &ComputedValues) -> FlowAxes {
    let writing_mode = match computed.writing_mode {
        CssWritingMode::HorizontalTb => buckram::WritingMode::HorizontalTb,
        CssWritingMode::VerticalRl => buckram::WritingMode::VerticalRl,
        CssWritingMode::VerticalLr => buckram::WritingMode::VerticalLr,
        CssWritingMode::SidewaysRl => buckram::WritingMode::SidewaysRl,
        CssWritingMode::SidewaysLr => buckram::WritingMode::SidewaysLr,
    };
    let direction = match computed.direction {
        CssDirection::Ltr => buckram::Direction::Ltr,
        CssDirection::Rtl => buckram::Direction::Rtl,
    };
    FlowAxes::new(writing_mode, direction)
}

/// Taffy's flex axes are physical. CSS flex axes are logical, so convert the
/// authored direction through the container's writing mode and direction
/// before the backend sizes or places any children.
pub(in crate::layout) fn physical_flex_direction(
    direction: CssFlexDirection,
    flow: FlowAxes,
) -> FlexDirection {
    let start = physical_flex_main_axis_start(direction, flow);
    let reverse = matches!(
        direction,
        CssFlexDirection::RowReverse | CssFlexDirection::ColumnReverse
    );
    match (start, reverse) {
        (PhysicalSide::Left, false) | (PhysicalSide::Right, true) => FlexDirection::Row,
        (PhysicalSide::Right, false) | (PhysicalSide::Left, true) => FlexDirection::RowReverse,
        (PhysicalSide::Top, false) | (PhysicalSide::Bottom, true) => FlexDirection::Column,
        (PhysicalSide::Bottom, false) | (PhysicalSide::Top, true) => FlexDirection::ColumnReverse,
    }
}

pub(in crate::layout) fn physical_flex_main_axis_start(
    direction: CssFlexDirection,
    flow: FlowAxes,
) -> PhysicalSide {
    match direction {
        CssFlexDirection::Row | CssFlexDirection::RowReverse => flow.inline_start(),
        CssFlexDirection::Column | CssFlexDirection::ColumnReverse => flow.block_start(),
    }
}

/// Taffy uses `direction` only for a physical column's horizontal cross axis.
/// Keep it Ltr unless the CSS flex cross-start is the physical right edge: the
/// one targeted Rtl case preserves `start`/`end` and lets Taffy's existing
/// wrap-reverse XOR keep flex-relative alignment and line order in sync.
/// Physical-row containers with a vertical cross axis, and mixed-writing-mode
/// baseline alignment, need their own lowering rather than this direction bit.
pub(in crate::layout) fn physical_flex_cross_axis_direction(
    direction: CssFlexDirection,
    physical_direction: FlexDirection,
    flow: FlowAxes,
) -> TaffyDirection {
    let cross_start = match direction {
        CssFlexDirection::Row | CssFlexDirection::RowReverse => flow.block_start(),
        CssFlexDirection::Column | CssFlexDirection::ColumnReverse => flow.inline_start(),
    };
    if matches!(
        physical_direction,
        FlexDirection::Column | FlexDirection::ColumnReverse
    ) && cross_start == PhysicalSide::Right
    {
        TaffyDirection::Rtl
    } else {
        TaffyDirection::Ltr
    }
}

pub(in crate::layout) fn physical_flex_wrap(
    computed: &ComputedValues,
    physical_direction: FlexDirection,
    flow: FlowAxes,
) -> FlexWrap {
    let reverse = computed.display == CssDisplay::Flex
        && matches!(
            physical_direction,
            FlexDirection::Row | FlexDirection::RowReverse
        )
        && matches!(
            computed.flex_direction,
            CssFlexDirection::Column | CssFlexDirection::ColumnReverse
        )
        && flow.inline_start() == PhysicalSide::Bottom;
    match (computed.flex_wrap, reverse) {
        (CssFlexWrap::NoWrap, _) => FlexWrap::NoWrap,
        (CssFlexWrap::Wrap, false) | (CssFlexWrap::WrapReverse, true) => FlexWrap::Wrap,
        (CssFlexWrap::WrapReverse, false) | (CssFlexWrap::Wrap, true) => FlexWrap::WrapReverse,
    }
}

pub(in crate::layout) fn physical_flex_cross_alignment(
    computed: &ComputedValues,
    value: CssAlignment,
    direction: CssFlexDirection,
    physical_direction: FlexDirection,
    flow: FlowAxes,
) -> CssAlignment {
    if computed.display == CssDisplay::Flex
        && matches!(
            physical_direction,
            FlexDirection::Row | FlexDirection::RowReverse
        )
        && matches!(
            direction,
            CssFlexDirection::Column | CssFlexDirection::ColumnReverse
        )
        && flow.inline_start() == PhysicalSide::Bottom
    {
        match (value, computed.flex_wrap) {
            (CssAlignment::Start, _) => CssAlignment::End,
            (CssAlignment::End, _) => CssAlignment::Start,
            (CssAlignment::FlexStart, CssFlexWrap::NoWrap) => CssAlignment::FlexEnd,
            (CssAlignment::FlexEnd, CssFlexWrap::NoWrap) => CssAlignment::FlexStart,
            (value, _) => value,
        }
    } else {
        value
    }
}

/// `start` and `end` are flow-relative, unlike `flex-start` and `flex-end`.
/// Once the flex main axis is physical, lower those two keywords to the
/// corresponding low or high physical edge as well. Other display modes keep
/// their existing alignment lowering.
pub(in crate::layout) fn physical_flex_justify_content(
    computed: &ComputedValues,
    flow: FlowAxes,
) -> JustifyContent {
    let value = if computed.display == CssDisplay::Flex {
        match (
            computed.justify_content,
            physical_flex_main_axis_start(computed.flex_direction, flow),
        ) {
            (CssAlignment::Normal, _) => CssAlignment::FlexStart,
            (CssAlignment::Start, PhysicalSide::Right | PhysicalSide::Bottom) => CssAlignment::End,
            (CssAlignment::End, PhysicalSide::Right | PhysicalSide::Bottom) => CssAlignment::Start,
            (value, _) => value,
        }
    } else {
        computed.justify_content
    };
    justify_content(value)
}

/// CSS `column-gap` follows the logical inline axis, and `row-gap` follows the
/// block axis. Taffy names its gap components after physical width and height.
/// The grid lane owns its separate logical lowering, so only flex containers
/// transpose these components here.
pub(in crate::layout) fn physical_flex_gap(
    computed: &ComputedValues,
    font_size: f32,
    flow: FlowAxes,
) -> Size<LengthPercentage> {
    let row_gap = gap(computed.row_gap, font_size);
    let column_gap = gap(computed.column_gap, font_size);
    if computed.display == CssDisplay::Flex && !flow.is_horizontal() {
        Size {
            width: row_gap,
            height: column_gap,
        }
    } else {
        Size {
            width: column_gap,
            height: row_gap,
        }
    }
}

/// The K5b flex/grid provider is selected only after the direct child is
/// attached to the scratch parent. This is also the narrow point where a
/// flex item's `align-self` can see the parent's physical flex axes. Livery
/// supplies generated CSS ownership; Buckram owns the renderer-role
/// transition and retains the static rectangle it yields for the later K5d
/// equation.
pub(in crate::layout) fn enable_flex_grid_static_position_provider<Id, Context, Source>(
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
    let flex_parent = (inside == Some(DisplayInside::Flex))
        .then(|| element_origin_node(boxes[container].origin).and_then(|node| styles.get(node)))
        .flatten();
    let grid_flow = (inside == Some(DisplayInside::Grid)).then_some(boxes[container].flow);
    let children = tree.children(container_node).to_vec();
    for child in children {
        let direct_child = tree.source(child).direct_box();
        if let (Some(parent), Some(child_box)) = (flex_parent, direct_child) {
            let css_box = &boxes[child_box];
            if let Some(child_computed) =
                element_origin_node(css_box.origin).and_then(|node| styles.get(node))
            {
                map_flex_child_self_alignment(
                    tree.style_mut(child),
                    parent,
                    css_box.flow,
                    child_computed,
                );
            }
        }
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

/// Only an element-originated generated box owns the corresponding computed
/// style. Text, pseudo, and anonymous boxes may name an owner for provenance,
/// but that owner's `align-self`, dimensions, and writing mode are not the
/// generated item's own values.
pub(in crate::layout) fn element_origin_node<Id: Copy>(origin: BoxOrigin<Id>) -> Option<Id> {
    match origin {
        BoxOrigin::Element(node) => Some(node),
        BoxOrigin::Text(_) | BoxOrigin::Pseudo { .. } | BoxOrigin::Anonymous { .. } => None,
    }
}

/// Project a direct flex child's logical `align-self` after its parent's
/// physical flex direction is known. Taffy can represent a physical column's
/// right cross start with `Direction::Rtl`, but cannot resolve `self-start`
/// and `self-end` against a differently written subject.
pub(in crate::layout) fn map_flex_child_self_alignment(
    style: &mut Style,
    parent: &ComputedValues,
    subject_flow: FlowAxes,
    subject: &ComputedValues,
) {
    debug_assert_eq!(parent.display, CssDisplay::Flex);
    let parent_flow = flex_flow_axes(parent);
    let physical_direction = physical_flex_direction(parent.flex_direction, parent_flow);
    let backend_direction =
        physical_flex_cross_axis_direction(parent.flex_direction, physical_direction, parent_flow);
    let cross_size = match physical_direction {
        FlexDirection::Row | FlexDirection::RowReverse => subject.height,
        FlexDirection::Column | FlexDirection::ColumnReverse => subject.width,
    };
    let Some(value) = effective_flex_child_alignment(parent, subject, cross_size) else {
        // Preserve taffy's native inheritance for ordinary auto/stretch.
        style.align_self = None;
        return;
    };
    let lowered = match value {
        CssAlignment::SelfStart | CssAlignment::SelfEnd => {
            let backend_cross_start =
                backend_flex_cross_start(physical_direction, backend_direction);
            let desired = subject_side_on_axis(
                subject_flow,
                backend_cross_start,
                value == CssAlignment::SelfStart,
            );
            if desired == backend_cross_start {
                CssAlignment::Start
            } else {
                CssAlignment::End
            }
        },
        value => physical_flex_cross_alignment(
            parent,
            value,
            parent.flex_direction,
            physical_direction,
            parent_flow,
        ),
    };
    style.align_self = Some(align_items(lowered));
}

/// Choose the alignment that a direct flex item contributes after CSS `auto`
/// inherits the container's `align-items`. Content-keyword cross sizes only
/// suppress the effective stretch case: an inherited center, end, or
/// subject-relative self edge still has to align at that edge.
pub(in crate::layout) fn effective_flex_child_alignment(
    parent: &ComputedValues,
    subject: &ComputedValues,
    cross_size: CssSize,
) -> Option<CssAlignment> {
    let (value, inherited) = match subject.align_self {
        CssAlignment::Auto => (parent.align_items, true),
        value => (value, false),
    };
    // `normal` resolves to stretch for flex items. Content-keyword sizes
    // suppress that stretch regardless of whether it came from `auto` or an
    // explicit `normal`/`stretch` child value.
    let value = match value {
        CssAlignment::Auto | CssAlignment::Normal => CssAlignment::Stretch,
        value => value,
    };
    if value == CssAlignment::Stretch && suppresses_stretch(cross_size) {
        Some(CssAlignment::FlexStart)
    } else if inherited && value == CssAlignment::Stretch {
        // Preserve taffy's native inheritance for ordinary auto/stretch.
        None
    } else {
        Some(value)
    }
}

/// Taffy's cross-axis `Start` is top for physical rows and follows the
/// container direction for physical columns.
pub(in crate::layout) fn backend_flex_cross_start(
    physical_direction: FlexDirection,
    direction: TaffyDirection,
) -> PhysicalSide {
    match physical_direction {
        FlexDirection::Row | FlexDirection::RowReverse => PhysicalSide::Top,
        FlexDirection::Column | FlexDirection::ColumnReverse => match direction {
            TaffyDirection::Ltr => PhysicalSide::Left,
            TaffyDirection::Rtl => PhysicalSide::Right,
        },
    }
}

/// The source forms used by the block and inline scratch trees. Only a direct
/// generated box has an unambiguous subject writing mode for CSS `self-*`.
pub(in crate::layout) trait DirectBoxSource {
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
pub(in crate::layout) fn map_vertical_grid_static_alignment(style: &mut Style, flow: FlowAxes) {
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
pub(in crate::layout) fn map_grid_static_self_alignment(
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

pub(in crate::layout) fn self_alignment_for_axis(
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

pub(in crate::layout) fn subject_side_on_axis(
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

pub(in crate::layout) fn same_physical_axis(first: PhysicalSide, second: PhysicalSide) -> bool {
    matches!(first, PhysicalSide::Left | PhysicalSide::Right)
        == matches!(second, PhysicalSide::Left | PhysicalSide::Right)
}

pub(in crate::layout) fn set_physical_self_alignment(
    style: &mut Style,
    axis_side: PhysicalSide,
    alignment: AlignItems,
) {
    match axis_side {
        PhysicalSide::Left | PhysicalSide::Right => style.justify_self = Some(alignment),
        PhysicalSide::Top | PhysicalSide::Bottom => style.align_self = Some(alignment),
    }
}

pub(in crate::layout) fn reverse_self_alignment(
    alignment: Option<AlignItems>,
) -> Option<AlignItems> {
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

pub(in crate::layout) fn grid_auto_flow(value: CssGridAutoFlow) -> GridAutoFlow {
    match value {
        CssGridAutoFlow::Row => GridAutoFlow::Row,
        CssGridAutoFlow::Column => GridAutoFlow::Column,
        CssGridAutoFlow::RowDense => GridAutoFlow::RowDense,
        CssGridAutoFlow::ColumnDense => GridAutoFlow::ColumnDense,
    }
}

pub(in crate::layout) fn grid_placement(value: CssGridPlacement) -> GridPlacement {
    match value {
        CssGridPlacement::Auto => GridPlacement::Auto,
        CssGridPlacement::Line(value) => line(value),
        CssGridPlacement::Span(value) => span(value),
    }
}

pub(in crate::layout) fn grid_template(
    value: &CssGridTemplate,
    em: f32,
) -> Vec<GridTemplateComponent<String>> {
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
pub(in crate::layout) fn self_alignment(value: CssAlignment, size: CssSize) -> Option<AlignItems> {
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
pub(in crate::layout) fn suppresses_stretch(size: CssSize) -> bool {
    matches!(
        size,
        CssSize::MinContent | CssSize::MaxContent | CssSize::FitContent(_)
    )
}

pub(in crate::layout) fn align_items(value: CssAlignment) -> AlignItems {
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

pub(in crate::layout) fn align_content(value: CssAlignment) -> AlignContent {
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

pub(in crate::layout) fn justify_content(value: CssAlignment) -> JustifyContent {
    align_content(value)
}

pub(in crate::layout) fn font_size_px(size: &FontSize, parent: f32) -> f32 {
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

thread_local! {
    /// The calc()/math lengths this thread's current layout pass tagged into
    /// Taffy `Dimension::calc` slots, addressed by the index encoded in each
    /// tag. Cleared at the public layout entries; sound because a layout pass
    /// builds and resolves its algorithm trees on one thread.
    static CALC_SCRATCH: std::cell::RefCell<Vec<(CssLengthPercentage, f32)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Encode a [`CALC_SCRATCH`] index as the non-null, 8-aligned opaque pointer
/// `Dimension::calc` requires. Taffy never dereferences it; it comes back
/// verbatim to [`resolve_taffy_calc`].
pub(in crate::layout) fn calc_tag(index: usize) -> *const () {
    ((index + 1) << 3) as *const ()
}

pub(in crate::layout) fn resolve_taffy_calc(ptr: *const (), basis: f32) -> f32 {
    let index = ((ptr as usize) >> 3).wrapping_sub(1);
    CALC_SCRATCH.with(|scratch| {
        scratch.borrow().get(index).map_or(0.0, |&(value, em)| {
            absolute_length_percentage(value, em, 16.0, basis)
        })
    })
}

/// Park a percentage-bearing `calc()` in the scratch and hand back the tag the
/// tree's resolver interprets against the real basis. Every box property that
/// accepts a percentage goes through here; resolving one against zero silently
/// drops the percentage, which is how `calc(50% - 10px)` became `-10px`.
pub(in crate::layout) fn calc_slot(value: CssLengthPercentage, em: f32) -> *const () {
    let index = CALC_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        scratch.push((value, em));
        scratch.len() - 1
    });
    calc_tag(index)
}

pub(in crate::layout) fn dimension(size: CssSize, em: f32) -> Dimension {
    match size {
        CssSize::Value(value) => match value {
            CssLengthPercentage::Percentage(value) => Dimension::percent(value),
            // calc()/min()/max()/clamp() mixing a percentage with lengths has
            // no linear Taffy form; tag the value into a calc slot the tree's
            // resolver interprets against the real basis. Resolving against
            // zero here is what turned min(100% - 48px, 960px) into a
            // negative used width.
            CssLengthPercentage::Calc(_) | CssLengthPercentage::Math(_)
                if value.has_percentage() =>
            {
                Dimension::calc(calc_slot(value, em))
            },
            _ => Dimension::length(absolute_length_percentage(value, em, 16.0, 0.0)),
        },
        _ => Dimension::auto(),
    }
}

/// Taffy preserves CSS `auto` and `content` as separate used-value paths.
/// Intrinsic keyword bases remain outside this bounded adapter and continue to
/// use its `auto` compatibility lowering.
pub(in crate::layout) fn flex_basis(basis: CssFlexBasis, em: f32) -> TaffyFlexBasis {
    match basis {
        CssFlexBasis::Value(value) => match value {
            CssLengthPercentage::Percentage(value) => {
                TaffyFlexBasis::from(Dimension::percent(value))
            },
            CssLengthPercentage::Calc(_) | CssLengthPercentage::Math(_)
                if value.has_percentage() =>
            {
                TaffyFlexBasis::from(Dimension::calc(calc_slot(value, em)))
            },
            _ => TaffyFlexBasis::from(Dimension::length(absolute_length_percentage(
                value, em, 16.0, 0.0,
            ))),
        },
        CssFlexBasis::Auto => TaffyFlexBasis::auto(),
        CssFlexBasis::Content => TaffyFlexBasis::content(),
        CssFlexBasis::MinContent | CssFlexBasis::MaxContent | CssFlexBasis::FitContent => {
            TaffyFlexBasis::auto()
        },
    }
}

pub(in crate::layout) fn dimension_with_basis(
    size: CssSize,
    em: f32,
    basis: Option<f32>,
) -> Dimension {
    match (size, basis) {
        // A calc() or math function mixing a percentage with lengths cannot
        // ride Taffy's plain percent dimension; resolve it against the
        // definite basis here. Plain percentages stay native so Taffy keeps
        // its own dynamic resolution.
        (CssSize::Value(value), Some(basis))
            if value.has_percentage() && !matches!(value, CssLengthPercentage::Percentage(_)) =>
        {
            Dimension::length(absolute_length_percentage(value, em, 16.0, basis))
        },
        // A math length with a percentage but no definite basis cannot be
        // linearized; auto is honest, resolving against zero is not — it
        // turned min(100% - 48px, 960px) into a negative used width.
        (CssSize::Value(value), None)
            if matches!(value, CssLengthPercentage::Math(_)) && value.has_percentage() =>
        {
            Dimension::auto()
        },
        (size, _) => dimension(size, em),
    }
}

pub(in crate::layout) fn resolved_child_containing_size(
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

pub(in crate::layout) fn resolved_explicit_size(
    size: CssSize,
    em: f32,
    basis: Option<f32>,
) -> Option<f32> {
    let CssSize::Value(value) = size else {
        return None;
    };
    if value.has_percentage() {
        basis.map(|basis| absolute_length_percentage(value, em, 16.0, basis))
    } else {
        Some(absolute_length_percentage(value, em, 16.0, 0.0))
    }
}

pub(in crate::layout) fn margin(value: Margin, em: f32) -> LengthPercentageAuto {
    match value {
        Margin::Auto => LengthPercentageAuto::auto(),
        Margin::Value(value) => length_percentage_auto(value, em),
    }
}

pub(in crate::layout) fn length_percentage_auto(
    value: CssLengthPercentage,
    em: f32,
) -> LengthPercentageAuto {
    match value {
        CssLengthPercentage::Percentage(value) => LengthPercentageAuto::percent(value),
        CssLengthPercentage::Calc(_) | CssLengthPercentage::Math(_) if value.has_percentage() => {
            LengthPercentageAuto::calc(calc_slot(value, em))
        },
        _ => LengthPercentageAuto::length(absolute_length_percentage(value, em, 16.0, 0.0)),
    }
}

pub(in crate::layout) fn length_percentage(
    value: CssLengthPercentage,
    em: f32,
) -> LengthPercentage {
    match value {
        CssLengthPercentage::Percentage(value) => LengthPercentage::percent(value),
        CssLengthPercentage::Calc(_) | CssLengthPercentage::Math(_) if value.has_percentage() => {
            LengthPercentage::calc(calc_slot(value, em))
        },
        _ => LengthPercentage::length(absolute_length_percentage(value, em, 16.0, 0.0)),
    }
}

pub(in crate::layout) fn gap(value: CssGap, em: f32) -> LengthPercentage {
    length_percentage(value.0, em)
}

pub(in crate::layout) fn absolute_length_percentage(
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

pub(in crate::layout) fn absolute_length(length: Length, em: f32, rem: f32) -> f32 {
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

pub(in crate::layout) fn border(
    style: BorderStyle,
    width: BorderWidth,
    em: f32,
) -> LengthPercentage {
    LengthPercentage::length(border_width_px(style, width, em))
}

pub(in crate::layout) fn overflow(value: CssOverflow) -> Overflow {
    match value {
        CssOverflow::Visible => Overflow::Visible,
        CssOverflow::Hidden => Overflow::Hidden,
        CssOverflow::Clip => Overflow::Clip,
        CssOverflow::Scroll | CssOverflow::Auto => Overflow::Scroll,
    }
}

/// Drop every calc() slot this thread's previous layout pass tagged.
///
/// The public layout entries call this before building any style, so a tag
/// never outlives the pass that made it.
pub(in crate::layout) fn reset_calc_scratch() {
    CALC_SCRATCH.with(|scratch| scratch.borrow_mut().clear());
}
