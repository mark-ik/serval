// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! CSS block formatting inputs and normal-flow placement.
//!
//! This module owns the CSS-facing model. Backend styles are lowered into
//! these values at an adapter edge; they do not choose or define the block
//! algorithm.

use crate::{
    FlowAxes, IntrinsicSizes, LogicalOffset, LogicalRect, PhysicalRect, PhysicalSide,
    PhysicalSides, PhysicalSize, WritingMode,
};

/// A linear used-value expression: an absolute component plus a percentage
/// of the containing block's inline size.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FlowLength {
    pub px: f32,
    pub percentage: f32,
}

impl FlowLength {
    pub const ZERO: Self = Self {
        px: 0.0,
        percentage: 0.0,
    };

    pub const fn px(px: f32) -> Self {
        Self {
            px,
            percentage: 0.0,
        }
    }

    pub const fn percent(percentage: f32) -> Self {
        Self {
            px: 0.0,
            percentage,
        }
    }

    pub fn resolve(self, containing_inline_size: f32) -> f32 {
        self.px + self.percentage * containing_inline_size
    }
}

/// A margin or inset value which may remain automatic until layout.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum FlowLengthAuto {
    #[default]
    Auto,
    Value(FlowLength),
}

impl FlowLengthAuto {
    pub const ZERO: Self = Self::Value(FlowLength::ZERO);

    pub fn resolve(self, containing_inline_size: f32) -> Option<f32> {
        match self {
            Self::Auto => None,
            Self::Value(value) => Some(value.resolve(containing_inline_size)),
        }
    }

    /// Resolve against a block-axis basis that may be indefinite. A
    /// percentage with no definite basis behaves as `auto`, which is how CSS
    /// treats a block-axis inset whose containing block has no specified
    /// block size.
    pub fn resolve_block(self, containing_block_size: Option<f32>) -> Option<f32> {
        match self {
            Self::Auto => None,
            Self::Value(value) if value.percentage == 0.0 => Some(value.px),
            Self::Value(value) => containing_block_size.map(|basis| value.resolve(basis)),
        }
    }
}

/// A CSS preferred, minimum, or maximum size before intrinsic querying.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum BlockSizeValue {
    #[default]
    Auto,
    None,
    Length(FlowLength),
    MinContent,
    MaxContent,
    FitContent(FlowLength),
}

impl BlockSizeValue {
    pub fn resolve_definite(self, containing_size: Option<f32>) -> Option<f32> {
        match (self, containing_size) {
            (Self::Length(value), Some(containing_size)) => Some(value.resolve(containing_size)),
            (Self::Length(value), None) if value.percentage == 0.0 => Some(value.px),
            _ => None,
        }
    }

    fn requires_intrinsic_query(self) -> bool {
        matches!(
            self,
            Self::MinContent | Self::MaxContent | Self::FitContent(_)
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BlockDimensions<T> {
    pub width: T,
    pub height: T,
}

impl<T> BlockDimensions<T> {
    pub const fn new(width: T, height: T) -> Self {
        Self { width, height }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BlockBoxSizing {
    #[default]
    ContentBox,
    BorderBox,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BlockPosition {
    #[default]
    Static,
    Relative,
    Absolute,
    Fixed,
    Sticky,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FloatSide {
    #[default]
    None,
    Left,
    Right,
}

/// Rectangular float area used only for inline line exclusions.
///
/// Float placement, clearance, independent formatting-context avoidance, and
/// containing-block height always continue to use the float margin box.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FloatReferenceBox {
    #[default]
    MarginBox,
    BorderBox,
    PaddingBox,
    ContentBox,
}

/// One physical corner radius retained until the float's used border box is
/// known. Percentages resolve against the corresponding border-box dimension,
/// not the containing block.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BlockCornerRadius {
    pub horizontal: FlowLength,
    pub vertical: FlowLength,
}

/// Physical CSS border radii. The block algorithm converts these to its
/// logical axes only for horizontal `shape-outside` reference boxes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BlockCornerRadii {
    pub top_left: BlockCornerRadius,
    pub top_right: BlockCornerRadius,
    pub bottom_right: BlockCornerRadius,
    pub bottom_left: BlockCornerRadius,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ClearSide {
    #[default]
    None,
    Left,
    Right,
    Both,
}

/// Selects which line-relative side receives the remaining space when an
/// otherwise ordinary block-width equation has two non-auto inline margins.
///
/// This is a used-value policy rather than a computed CSS value. Consumers
/// such as HTML presentational hints can request it without rewriting either
/// computed margin.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverconstrainedInlineAlignment {
    LineLeft,
    Center,
    LineRight,
}

/// Standards-owned outer-box inputs used by block flow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockStyle {
    /// Axes used by this box to lay out its own contents.
    pub flow: FlowAxes,
    /// Axes of the containing block in which this box participates.
    pub containing_flow: FlowAxes,
    pub size: BlockDimensions<BlockSizeValue>,
    pub min_size: BlockDimensions<BlockSizeValue>,
    pub max_size: BlockDimensions<BlockSizeValue>,
    pub margin: PhysicalSides<FlowLengthAuto>,
    /// Physical inset inputs retained until the relevant positioning phase.
    pub inset: PhysicalSides<FlowLengthAuto>,
    pub padding: PhysicalSides<FlowLength>,
    pub border: PhysicalSides<f32>,
    pub box_sizing: BlockBoxSizing,
    pub position: BlockPosition,
    pub float: FloatSide,
    pub float_reference_box: FloatReferenceBox,
    /// Whether `float_reference_box` came from an authored box-valued
    /// `shape-outside`, rather than the initial `shape-outside: none`.
    pub has_shape_outside_box: bool,
    pub corner_radii: BlockCornerRadii,
    pub clear: ClearSide,
    pub establishes_bfc: bool,
    /// The used inline size must be obtained from CSS shrink-to-fit sizing.
    pub shrink_to_fit: bool,
    pub replaced: bool,
    pub aspect_ratio: Option<f32>,
    pub size_containment: BlockDimensions<bool>,
    pub has_nonlinear_lengths: bool,
    /// Optional used-margin adjustment for a definite-width block whose two
    /// inline margins are non-auto and leave positive remaining space.
    pub overconstrained_inline_alignment: Option<OverconstrainedInlineAlignment>,
    /// The principal box of the document element. Its margins never collapse.
    pub is_root_element: bool,
}

impl Default for BlockStyle {
    fn default() -> Self {
        Self {
            flow: FlowAxes::HORIZONTAL_LTR,
            containing_flow: FlowAxes::HORIZONTAL_LTR,
            size: BlockDimensions::new(BlockSizeValue::Auto, BlockSizeValue::Auto),
            min_size: BlockDimensions::new(BlockSizeValue::Auto, BlockSizeValue::Auto),
            max_size: BlockDimensions::new(BlockSizeValue::None, BlockSizeValue::None),
            margin: PhysicalSides::splat(FlowLengthAuto::ZERO),
            inset: PhysicalSides::splat(FlowLengthAuto::Auto),
            padding: PhysicalSides::splat(FlowLength::ZERO),
            border: PhysicalSides::splat(0.0),
            box_sizing: BlockBoxSizing::ContentBox,
            position: BlockPosition::Static,
            float: FloatSide::None,
            float_reference_box: FloatReferenceBox::MarginBox,
            has_shape_outside_box: false,
            corner_radii: BlockCornerRadii::default(),
            clear: ClearSide::None,
            establishes_bfc: false,
            shrink_to_fit: false,
            replaced: false,
            aspect_ratio: None,
            size_containment: BlockDimensions::new(false, false),
            has_nonlinear_lengths: false,
            overconstrained_inline_alignment: None,
            is_root_element: false,
        }
    }
}

impl BlockStyle {
    pub fn anonymous(flow: FlowAxes, containing_flow: FlowAxes) -> Self {
        Self {
            flow,
            containing_flow,
            ..Self::default()
        }
    }

    /// Whether this box is excluded from its parent's normal-flow cursor.
    /// Its contents still need a local formatting pass, but the parent must
    /// retain the static-position rectangle rather than make a backend
    /// `Position` value decide participation.
    pub fn is_out_of_flow(self) -> bool {
        matches!(
            self.position,
            BlockPosition::Absolute | BlockPosition::Fixed
        )
    }

    /// Name the first feature that requires a later owned algorithm.
    pub fn deferral(self) -> Option<BlockDeferral> {
        if matches!(
            self.position,
            BlockPosition::Absolute | BlockPosition::Fixed | BlockPosition::Sticky
        ) {
            return Some(BlockDeferral::Positioning);
        }
        if self.shrink_to_fit {
            return Some(BlockDeferral::ShrinkToFit);
        }
        if self.float != FloatSide::None
            && !matches!(
                logical_dimension(self.containing_flow, self.size).inline,
                BlockSizeValue::Length(_)
            )
        {
            return Some(BlockDeferral::FloatShrinkToFit);
        }
        if self.replaced {
            return Some(BlockDeferral::Replaced);
        }
        if self.aspect_ratio.is_some() {
            return Some(BlockDeferral::AspectRatio);
        }
        if self.size_containment.width || self.size_containment.height {
            return Some(BlockDeferral::SizeContainment);
        }
        if self.has_nonlinear_lengths {
            return Some(BlockDeferral::NonlinearLength);
        }
        if [
            self.size.width,
            self.size.height,
            self.min_size.width,
            self.min_size.height,
            self.max_size.width,
            self.max_size.height,
        ]
        .into_iter()
        .any(BlockSizeValue::requires_intrinsic_query)
        {
            return Some(BlockDeferral::IntrinsicSize);
        }
        None
    }

    pub fn resolved_padding(self, containing_inline_size: f32) -> PhysicalSides<f32> {
        self.padding
            .map(|value| value.resolve(containing_inline_size))
    }

    pub fn logical_margin(self, containing_inline_size: f32) -> crate::LogicalSides<Option<f32>> {
        self.containing_flow.logical_sides(
            self.margin
                .map(|value| value.resolve(containing_inline_size)),
        )
    }

    /// Resolve relative positioning after normal-flow placement.
    ///
    /// The static flow rectangle remains unchanged; a specified start inset
    /// wins over the opposing end inset, whose sign is reversed. Inline-axis
    /// percentages resolve against the containing block's inline size.
    /// Block-axis percentages resolve against its block size, and behave as
    /// `auto` when that size is indefinite, as CSS 2.1 §9.3.2 and CSS
    /// Positioned Layout require for a containing block with no specified
    /// block size.
    pub fn relative_offset(
        self,
        containing_inline_size: f32,
        containing_block_size: Option<f32>,
    ) -> LogicalOffset {
        if self.position != BlockPosition::Relative {
            return LogicalOffset::default();
        }
        let inline = self.containing_flow.logical_sides(
            self.inset
                .map(|value| value.resolve(containing_inline_size)),
        );
        let block = self.containing_flow.logical_sides(
            self.inset
                .map(|value| value.resolve_block(containing_block_size)),
        );
        LogicalOffset {
            inline: inline
                .inline_start
                .or_else(|| inline.inline_end.map(|value| -value))
                .unwrap_or(0.0),
            block: block
                .block_start
                .or_else(|| block.block_end.map(|value| -value))
                .unwrap_or(0.0),
        }
    }

    pub fn logical_padding_border(self, containing_inline_size: f32) -> crate::LogicalSides<f32> {
        self.containing_flow.logical_sides(
            self.resolved_padding(containing_inline_size)
                .zip_map(self.border, |padding, border| padding + border),
        )
    }

    pub fn content_logical_padding_border(
        self,
        containing_inline_size: f32,
    ) -> crate::LogicalSides<f32> {
        self.flow.logical_sides(
            self.resolved_padding(containing_inline_size)
                .zip_map(self.border, |padding, border| padding + border),
        )
    }

    /// Which of this box's block edges may adjoin an in-flow child's margin.
    pub fn child_margin_collapse(
        self,
        containing_inline_size: f32,
        containing_block_size: Option<f32>,
        is_layout_root: bool,
        first_and_last_child_margins_adjoin: bool,
    ) -> BlockMarginCollapse {
        if is_layout_root || self.is_root_element || self.establishes_bfc {
            return BlockMarginCollapse::NONE;
        }

        let padding_border = self.content_logical_padding_border(containing_inline_size);
        let block_size = logical_dimension(self.flow, self.size).block;
        let min_block_size = logical_dimension(self.flow, self.min_size).block;
        BlockMarginCollapse {
            block_start: padding_border.block_start == 0.0,
            block_end: padding_border.block_end == 0.0
                && matches!(block_size, BlockSizeValue::Auto)
                && (!first_and_last_child_margins_adjoin
                    || min_block_size
                        .resolve_definite(containing_block_size)
                        .is_none_or(|minimum| minimum <= 0.0)),
        }
    }

    /// Whether this box has no block-axis separator, so adjoining margins may
    /// collapse through it.
    pub fn can_collapse_through(
        self,
        containing_inline_size: f32,
        containing_block_size: Option<f32>,
        is_layout_root: bool,
        has_line_boxes: bool,
        all_children_collapse_through: bool,
    ) -> bool {
        if is_layout_root
            || self.is_root_element
            || self.establishes_bfc
            || has_line_boxes
            || !all_children_collapse_through
        {
            return false;
        }

        let padding_border = self.content_logical_padding_border(containing_inline_size);
        let block_size = logical_dimension(self.flow, self.size).block;
        let min_block_size = logical_dimension(self.flow, self.min_size).block;
        let block_size_is_zero_or_auto = match block_size {
            BlockSizeValue::Auto => true,
            value => value
                .resolve_definite(containing_block_size)
                .is_some_and(|size| size == 0.0),
        };
        let min_block_size_is_zero = min_block_size
            .resolve_definite(containing_block_size)
            .is_none_or(|minimum| minimum <= 0.0);

        padding_border.block_start == 0.0
            && padding_border.block_end == 0.0
            && block_size_is_zero_or_auto
            && min_block_size_is_zero
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockDeferral {
    Positioning,
    ShrinkToFit,
    FloatShrinkToFit,
    FloatLineExclusion,
    FloatFormattingContextAvoidance,
    NestedFloatState,
    IntrinsicSize,
    IndependentFormattingContext,
    Replaced,
    AspectRatio,
    SizeContainment,
    NonlinearLength,
    IndefiniteInlineSize,
    BackendSizingMode,
}

/// Positive and negative adjoining margins collapse independently.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CollapsedMargin {
    positive: f32,
    negative: f32,
}

impl CollapsedMargin {
    pub const ZERO: Self = Self {
        positive: 0.0,
        negative: 0.0,
    };

    pub fn from_margin(margin: f32) -> Self {
        if margin >= 0.0 {
            Self {
                positive: margin,
                negative: 0.0,
            }
        } else {
            Self {
                positive: 0.0,
                negative: margin,
            }
        }
    }

    pub fn collapse(mut self, margin: f32) -> Self {
        if margin >= 0.0 {
            self.positive = self.positive.max(margin);
        } else {
            self.negative = self.negative.min(margin);
        }
        self
    }

    pub fn collapse_with(self, other: Self) -> Self {
        Self {
            positive: self.positive.max(other.positive),
            negative: self.negative.min(other.negative),
        }
    }

    pub fn resolve(self) -> f32 {
        self.positive + self.negative
    }
}

/// The two adjoining margin sets produced by a block box.
///
/// These are algorithm outputs, not pre-resolved spacing. A parent may need to
/// combine either set with an ancestor or sibling before its used width is
/// known.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BlockMarginState {
    pub block_start: CollapsedMargin,
    pub block_end: CollapsedMargin,
    pub collapses_through: bool,
}

impl BlockMarginState {
    pub fn from_box(
        style: BlockStyle,
        containing_inline_size: f32,
        child_block_start: CollapsedMargin,
        child_block_end: CollapsedMargin,
        collapse: BlockMarginCollapse,
        collapses_through: bool,
    ) -> Self {
        let margin = style.logical_margin(containing_inline_size);
        let own_start = CollapsedMargin::from_margin(margin.block_start.unwrap_or(0.0));
        let own_end = CollapsedMargin::from_margin(margin.block_end.unwrap_or(0.0));
        Self {
            block_start: if collapse.block_start || collapses_through {
                own_start.collapse_with(child_block_start)
            } else {
                own_start
            },
            block_end: if collapse.block_end || collapses_through {
                own_end.collapse_with(child_block_end)
            } else {
                own_end
            },
            collapses_through,
        }
    }
}

/// Whether this box's start and end margins adjoin its first and last in-flow
/// children's margins.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BlockMarginCollapse {
    pub block_start: bool,
    pub block_end: bool,
}

impl BlockMarginCollapse {
    pub const NONE: Self = Self {
        block_start: false,
        block_end: false,
    };
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UsedInlineSize {
    pub margin_start: f32,
    pub border_box: f32,
    pub margin_end: f32,
}

/// Solve CSS 2.2 section 10.3.3's block-width equation in logical terms.
pub fn solve_in_flow_inline_size(style: BlockStyle, containing_inline_size: f32) -> UsedInlineSize {
    solve_in_flow_inline_size_for_available(style, containing_inline_size, containing_inline_size)
}

/// Solve the block-width equation inside a float-constrained interval.
///
/// Percentages still resolve against the actual containing block. Only the
/// space distributed by `width: auto` and automatic margins is narrowed.
pub fn solve_in_flow_inline_size_for_available(
    style: BlockStyle,
    containing_inline_size: f32,
    available_inline_size: f32,
) -> UsedInlineSize {
    solve_in_flow_inline_size_inner(style, containing_inline_size, available_inline_size, None)
}

/// Solve the block-width equation for a box whose border-box inline size is
/// an algorithm output rather than a `width` value.
///
/// A table wrapper box takes the grid's border-edge inline size (CSS Tables 3
/// section 2.2.1); only its margins remain to be distributed, exactly as for
/// a definite-width block, and its own minimum and maximum constraints still
/// apply.
pub(crate) fn solve_in_flow_inline_size_for_border_box(
    style: BlockStyle,
    containing_inline_size: f32,
    border_box: f32,
) -> UsedInlineSize {
    solve_in_flow_inline_size_inner(
        style,
        containing_inline_size,
        containing_inline_size,
        Some(border_box),
    )
}

fn solve_in_flow_inline_size_inner(
    style: BlockStyle,
    containing_inline_size: f32,
    available_inline_size: f32,
    algorithm_border_box: Option<f32>,
) -> UsedInlineSize {
    let margins = style.logical_margin(containing_inline_size);
    let padding_border = style.logical_padding_border(containing_inline_size);
    let padding_border_sum = padding_border.inline_start + padding_border.inline_end;
    let preferred = algorithm_border_box.or_else(|| {
        logical_dimension(style.containing_flow, style.size)
            .inline
            .resolve_definite(Some(containing_inline_size))
            .map(|size| border_box_size(style.box_sizing, size, padding_border_sum))
    });
    let minimum = logical_dimension(style.containing_flow, style.min_size)
        .inline
        .resolve_definite(Some(containing_inline_size))
        .map_or(padding_border_sum, |size| {
            border_box_size(style.box_sizing, size, padding_border_sum)
        });
    let maximum = logical_dimension(style.containing_flow, style.max_size)
        .inline
        .resolve_definite(Some(containing_inline_size))
        .map(|size| border_box_size(style.box_sizing, size, padding_border_sum));

    let start = margins.inline_start.unwrap_or(0.0);
    let end = margins.inline_end.unwrap_or(0.0);
    let mut border_box = preferred
        .unwrap_or_else(|| (available_inline_size - start - end).max(padding_border_sum))
        .max(minimum);
    if let Some(maximum) = maximum {
        border_box = border_box.min(maximum.max(padding_border_sum));
    }

    let remaining = available_inline_size - border_box - start - end;
    let (mut margin_start, mut margin_end) =
        match (margins.inline_start.is_none(), margins.inline_end.is_none()) {
            (true, true) if remaining >= 0.0 => (remaining / 2.0, remaining / 2.0),
            (true, true) => (0.0, remaining),
            (true, false) if remaining >= 0.0 => (remaining, end),
            // CSS 2.2 section 10.3.3 first resolves an auto margin to zero
            // when the equation is over-constrained, then ignores inline-end
            // in the containing block's direction.
            (true, false) => (0.0, end + remaining),
            (false, true) => (start, remaining),
            // In an over-constrained equation, direction determines the ignored
            // physical side. In logical coordinates that is always inline-end.
            (false, false) => (start, end + remaining),
        };

    if preferred.is_some()
        && margins.inline_start.is_some()
        && margins.inline_end.is_some()
        && remaining > 0.0
        && let Some(alignment) = style.overconstrained_inline_alignment
    {
        let line_left = match style.flow.writing_mode {
            WritingMode::HorizontalTb => PhysicalSide::Left,
            WritingMode::VerticalRl | WritingMode::SidewaysRl => PhysicalSide::Top,
            WritingMode::VerticalLr | WritingMode::SidewaysLr => PhysicalSide::Bottom,
        };
        let line_left_is_inline_start = line_left == style.containing_flow.inline_start();
        (margin_start, margin_end) = match (alignment, line_left_is_inline_start) {
            (OverconstrainedInlineAlignment::Center, _) => {
                (start + remaining / 2.0, end + remaining / 2.0)
            },
            (OverconstrainedInlineAlignment::LineLeft, true)
            | (OverconstrainedInlineAlignment::LineRight, false) => (start, end + remaining),
            (OverconstrainedInlineAlignment::LineLeft, false)
            | (OverconstrainedInlineAlignment::LineRight, true) => (start + remaining, end),
        };
    }

    UsedInlineSize {
        margin_start,
        border_box,
        margin_end,
    }
}

/// Resolve the outer inline inputs of a definite-width float.
///
/// CSS 2.2 section 10.3.5 resolves automatic float margins to zero and does
/// not distribute the remaining width as an ordinary in-flow block does.
pub fn solve_float_inline_size(style: BlockStyle, containing_inline_size: f32) -> UsedInlineSize {
    let margins = style.logical_margin(containing_inline_size);
    let padding_border = style.logical_padding_border(containing_inline_size);
    let padding_border_sum = padding_border.inline_start + padding_border.inline_end;
    let preferred = logical_dimension(style.containing_flow, style.size)
        .inline
        .resolve_definite(Some(containing_inline_size))
        .map(|size| border_box_size(style.box_sizing, size, padding_border_sum))
        .unwrap_or(padding_border_sum);
    let minimum = logical_dimension(style.containing_flow, style.min_size)
        .inline
        .resolve_definite(Some(containing_inline_size))
        .map_or(padding_border_sum, |size| {
            border_box_size(style.box_sizing, size, padding_border_sum)
        });
    let maximum = logical_dimension(style.containing_flow, style.max_size)
        .inline
        .resolve_definite(Some(containing_inline_size))
        .map(|size| border_box_size(style.box_sizing, size, padding_border_sum));
    let border_box = maximum.map_or(preferred.max(minimum), |maximum| {
        preferred.max(minimum).min(maximum.max(padding_border_sum))
    });

    UsedInlineSize {
        margin_start: margins.inline_start.unwrap_or(0.0),
        border_box,
        margin_end: margins.inline_end.unwrap_or(0.0),
    }
}

/// Resolve an auto-width shrink-to-fit box using CSS2's shrink-to-fit equation.
///
/// The intrinsic pair describes the box's content box. Padding and border
/// are added after `min(max(min-content, available), max-content)`, then the
/// box's definite minimum and maximum constraints are applied.
pub fn solve_shrink_to_fit_inline_size(
    style: BlockStyle,
    containing_inline_size: f32,
    intrinsic: IntrinsicSizes,
) -> UsedInlineSize {
    debug_assert!(style.shrink_to_fit);
    let margins = style.logical_margin(containing_inline_size);
    let padding_border = style.logical_padding_border(containing_inline_size);
    let padding_border_sum = padding_border.inline_start + padding_border.inline_end;
    let margin_start = margins.inline_start.unwrap_or(0.0);
    let margin_end = margins.inline_end.unwrap_or(0.0);
    let available_content =
        (containing_inline_size - margin_start - margin_end - padding_border_sum).max(0.0);
    let shrink_to_fit = intrinsic
        .min_content
        .max(available_content)
        .min(intrinsic.max_content);
    let minimum = logical_dimension(style.containing_flow, style.min_size)
        .inline
        .resolve_definite(Some(containing_inline_size))
        .map_or(padding_border_sum, |size| {
            border_box_size(style.box_sizing, size, padding_border_sum)
        });
    let maximum = logical_dimension(style.containing_flow, style.max_size)
        .inline
        .resolve_definite(Some(containing_inline_size))
        .map(|size| border_box_size(style.box_sizing, size, padding_border_sum));
    let tentative = shrink_to_fit + padding_border_sum;
    let maximum_constrained = maximum.map_or(tentative, |maximum| {
        tentative.min(maximum.max(padding_border_sum))
    });
    let border_box = maximum_constrained.max(minimum);

    UsedInlineSize {
        margin_start,
        border_box,
        margin_end,
    }
}

fn border_box_size(sizing: BlockBoxSizing, specified: f32, padding_border: f32) -> f32 {
    match sizing {
        BlockBoxSizing::ContentBox => specified.max(0.0) + padding_border,
        BlockBoxSizing::BorderBox => specified.max(padding_border),
    }
}

fn logical_dimension<T: Copy>(
    axes: FlowAxes,
    physical: BlockDimensions<T>,
) -> LogicalDimensions<T> {
    if axes.is_horizontal() {
        LogicalDimensions {
            inline: physical.width,
            block: physical.height,
        }
    } else {
        LogicalDimensions {
            inline: physical.height,
            block: physical.width,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct LogicalDimensions<T> {
    inline: T,
    block: T,
}

/// Dynamic containing-block geometry for one BFC run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockContainingBlock {
    pub flow: FlowAxes,
    pub content_box: PhysicalSize,
}

/// One in-flow child placement relative to the containing content box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockPlacement {
    /// Parent-flow geometry retained until the caller has finalized the
    /// containing block's own block size.
    pub logical_rect: LogicalRect,
    pub rect: PhysicalRect,
    pub margin_inline_start: f32,
    pub margin_inline_end: f32,
}

/// Candidate placement for an in-flow BFC that must avoid active floats.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatAvoidingPlacement {
    /// Border-box block start in the containing BFC's logical coordinates.
    pub block_start: f32,
    /// Border-box inline start in the containing BFC's logical coordinates.
    pub inline_start: f32,
    /// Used inline dimensions solved inside the selected float band.
    pub inline_size: UsedInlineSize,
}

/// Inline interval left by floats for a line or atomic formatting context.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatAvailableSpace {
    pub inline_start: f32,
    pub inline_size: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct FloatExclusion {
    side: FloatSide,
    at_inline_start: bool,
    margin_box: FloatMarginBox,
    /// The float area consulted by inline line breaking. For the default
    /// `shape-outside: none`, this is identical to `margin_box`.
    line_area: FloatLineArea,
}

/// The block axis of a float margin box retains its signed used size. A
/// negative used size still locates the float's border box, but cannot exclude
/// line boxes or participate in clearance.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SignedBlockExtent {
    start: f32,
    used_size: f32,
}

impl SignedBlockExtent {
    fn end(self) -> f32 {
        self.start + self.used_size
    }

    fn is_valid_exclusion(self) -> bool {
        self.used_size >= 0.0
    }
}

/// A float's margin box is deliberately not a [`LogicalRect`]: CSS margins
/// can make its used block size negative.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FloatMarginBox {
    inline_start: f32,
    inline_size: f32,
    block: SignedBlockExtent,
}

impl FloatMarginBox {
    fn mirror_inline(&mut self, containing_inline_size: f32) {
        self.inline_start = containing_inline_size - self.inline_start - self.inline_size;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct LogicalCornerRadius {
    inline: f32,
    block: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct LogicalCornerRadii {
    block_start_inline_start: LogicalCornerRadius,
    block_start_inline_end: LogicalCornerRadius,
    block_end_inline_end: LogicalCornerRadius,
    block_end_inline_start: LogicalCornerRadius,
}

/// A float area used only by inline line breaking. Its rectangular bounds are
/// kept separate from the per-line interval so the rest of float placement
/// remains governed by the margin box.
#[derive(Clone, Copy, Debug, PartialEq)]
enum FloatLineArea {
    Rect(FloatMarginBox),
    Rounded {
        rect: FloatMarginBox,
        clip: FloatMarginBox,
        radii: LogicalCornerRadii,
    },
}

impl FloatLineArea {
    fn bounds(self) -> FloatMarginBox {
        match self {
            Self::Rect(rect) => rect,
            Self::Rounded { clip, .. } => clip,
        }
    }

    fn is_valid_exclusion(self) -> bool {
        self.bounds().block.is_valid_exclusion()
    }

    fn overlaps_block(self, block_start: f32, block_size: f32) -> bool {
        let rect = self.bounds();
        self.is_valid_exclusion()
            && spans_overlap(
                rect.block.start,
                rect.block.used_size,
                block_start,
                block_size,
            )
    }

    fn block_end(self) -> f32 {
        self.bounds().block.end()
    }

    fn retry_breakpoints(self, line_block_size: f32) -> Vec<f32> {
        match self {
            Self::Rect(rect) => vec![
                rect.block.start - line_block_size.max(0.0),
                rect.block.end(),
            ],
            Self::Rounded { rect, clip, radii } => {
                let line_block_size = line_block_size.max(0.0);
                let lower = clip.block.start - line_block_size;
                let upper = clip.block.end();
                let mut points = vec![
                    lower,
                    clip.block.start,
                    clip.block.end() - line_block_size,
                    upper,
                ];
                for point in [
                    rect.block.start + radii.block_start_inline_start.block - line_block_size,
                    rect.block.start + radii.block_start_inline_end.block - line_block_size,
                    rect.block.end() - radii.block_end_inline_start.block,
                    rect.block.end() - radii.block_end_inline_end.block,
                ] {
                    if point >= lower && point <= upper {
                        points.push(point);
                    }
                }
                points
            },
        }
    }

    fn translate(&mut self, inline_offset: f32, block_offset: f32) {
        match self {
            Self::Rect(rect) => {
                rect.inline_start += inline_offset;
                rect.block.start += block_offset;
            },
            Self::Rounded { rect, clip, .. } => {
                rect.inline_start += inline_offset;
                rect.block.start += block_offset;
                clip.inline_start += inline_offset;
                clip.block.start += block_offset;
            },
        }
    }

    fn mirror_inline(&mut self, containing_inline_size: f32) {
        match self {
            Self::Rect(rect) => rect.mirror_inline(containing_inline_size),
            Self::Rounded { rect, clip, radii } => {
                rect.mirror_inline(containing_inline_size);
                clip.mirror_inline(containing_inline_size);
                std::mem::swap(
                    &mut radii.block_start_inline_start,
                    &mut radii.block_start_inline_end,
                );
                std::mem::swap(
                    &mut radii.block_end_inline_start,
                    &mut radii.block_end_inline_end,
                );
            },
        }
    }

    /// Return the conservative inline bounds occupied anywhere in this line's
    /// full block-axis span. A line that crosses a rounded corner therefore
    /// cannot overlap the curve after its first break attempt.
    fn inline_bounds_for_span(self, block_start: f32, block_size: f32) -> Option<(f32, f32)> {
        if !self.overlaps_block(block_start, block_size) {
            return None;
        }
        match self {
            Self::Rect(rect) => Some((rect.inline_start, rect.inline_start + rect.inline_size)),
            Self::Rounded { rect, clip, radii } => {
                let span_start = block_start.max(clip.block.start);
                let span_end = (block_start + block_size).min(clip.block.end());
                // A zero-height provisional line still needs the contour at
                // its top edge before Parley reports its eventual line height.
                // For a non-zero line we also inspect the points where either
                // edge reaches the straight middle of the rounded rectangle.
                let sample = [
                    span_start,
                    span_end,
                    rect.block.start + radii.block_start_inline_start.block,
                    rect.block.start + radii.block_start_inline_end.block,
                    rect.block.end() - radii.block_end_inline_start.block,
                    rect.block.end() - radii.block_end_inline_end.block,
                ];
                let mut inline_start = f32::INFINITY;
                let mut inline_end = f32::NEG_INFINITY;
                let clip_start = clip.inline_start;
                let clip_end = clip.inline_start + clip.inline_size;
                for block in sample {
                    if block < span_start || block > span_end {
                        continue;
                    }
                    let (start, end) = rounded_inline_bounds_at(rect, radii, block);
                    let start = start.max(clip_start);
                    let end = end.min(clip_end);
                    if start <= end {
                        inline_start = inline_start.min(start);
                        inline_end = inline_end.max(end);
                    }
                }
                (inline_start <= inline_end).then_some((inline_start, inline_end))
            },
        }
    }
}

fn rounded_inline_bounds_at(
    rect: FloatMarginBox,
    radii: LogicalCornerRadii,
    block: f32,
) -> (f32, f32) {
    let block_offset = (block - rect.block.start).clamp(0.0, rect.block.used_size);
    let block_end = rect.block.used_size;
    let edge_inset = |block_start_radius: LogicalCornerRadius,
                      block_end_radius: LogicalCornerRadius| {
        if block_offset < block_start_radius.block {
            rounded_corner_inset(block_start_radius, block_offset)
        } else if block_offset >= block_end - block_end_radius.block {
            rounded_corner_inset(block_end_radius, block_end - block_offset)
        } else {
            0.0
        }
    };
    let start_inset = edge_inset(radii.block_start_inline_start, radii.block_end_inline_start);
    let end_inset = edge_inset(radii.block_start_inline_end, radii.block_end_inline_end);
    (
        rect.inline_start + start_inset,
        rect.inline_start + rect.inline_size - end_inset,
    )
}

fn rounded_corner_inset(radius: LogicalCornerRadius, edge_distance: f32) -> f32 {
    if radius.inline <= 0.0 || radius.block <= 0.0 {
        return 0.0;
    }
    let center_distance = (radius.block - edge_distance).clamp(0.0, radius.block);
    let normalized = 1.0 - (center_distance / radius.block).powi(2);
    radius.inline * (1.0 - normalized.max(0.0).sqrt())
}

impl FloatExclusion {
    fn block_start(self) -> f32 {
        self.margin_box.block.start
    }

    fn block_end(self) -> f32 {
        self.margin_box.block.end()
    }

    fn is_valid_exclusion(self) -> bool {
        self.margin_box.block.is_valid_exclusion()
    }

    fn margin_box_overlaps_block(self, block_start: f32, block_size: f32) -> bool {
        self.is_valid_exclusion()
            && spans_overlap(
                self.block_start(),
                self.margin_box.block.used_size,
                block_start,
                block_size,
            )
    }

    fn line_area_overlaps_block(self, block_start: f32, block_size: f32) -> bool {
        self.is_valid_exclusion() && self.line_area.overlaps_block(block_start, block_size)
    }
}

/// Float exclusions translated into one ordinary descendant block's content
/// coordinate space.
///
/// This state is crate-private because it is an algorithm continuation, not a
/// fragment-tree result. Explicit BFC boundaries never receive or export it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct FloatContextState {
    exclusions: Vec<FloatExclusion>,
}

impl FloatContextState {
    pub(crate) fn has_side(&self, side: FloatSide) -> bool {
        self.exclusions.iter().any(|exclusion| {
            exclusion.side == side && exclusion.is_valid_exclusion() && exclusion.block_end() > 0.0
        })
    }

    fn mirror_inline(&mut self, containing_inline_size: f32) {
        // Exclusions are stored in the current formatting context's logical
        // coordinates. A direction change mirrors those coordinates while the
        // float side itself remains the physical left or right side.
        for exclusion in &mut self.exclusions {
            exclusion.margin_box.mirror_inline(containing_inline_size);
            exclusion.line_area.mirror_inline(containing_inline_size);
            exclusion.at_inline_start = !exclusion.at_inline_start;
        }
    }
}

/// Immutable float geometry supplied to one inline formatting context.
///
/// Line positions are local to the inline context. Buckram retains the
/// context's block-axis offset into the owning BFC so callers do not have to
/// translate float rectangles or infer their coordinate space.
#[derive(Clone, Debug, PartialEq)]
pub struct FloatLineConstraints {
    flow: FlowAxes,
    containing_inline_size: f32,
    block_offset: f32,
    exclusions: Vec<FloatExclusion>,
}

impl FloatLineConstraints {
    /// Return the inline interval available across this line's block-axis
    /// span. Using the whole span, rather than only its top edge, keeps a tall
    /// line from intersecting a float boundary partway through the line box.
    pub fn available_space(
        &self,
        line_block_start: f32,
        line_block_size: f32,
    ) -> FloatAvailableSpace {
        available_line_space_for(
            self.containing_inline_size,
            &self.exclusions,
            self.block_offset + line_block_start,
            line_block_size,
        )
    }

    /// Convert the flow-relative interval to the horizontal physical
    /// coordinate consumed by a text shaper. Vertical flow remains outside
    /// the admitted Buckram block lane.
    pub fn horizontal_physical_space(
        &self,
        line_block_start: f32,
        line_block_size: f32,
    ) -> FloatAvailableSpace {
        let logical = self.available_space(line_block_start, line_block_size);
        let inline_start = match self.flow.inline_start() {
            crate::PhysicalSide::Left => logical.inline_start,
            crate::PhysicalSide::Right => {
                self.containing_inline_size - logical.inline_start - logical.inline_size
            },
            crate::PhysicalSide::Top | crate::PhysicalSide::Bottom => logical.inline_start,
        };
        FloatAvailableSpace {
            inline_start: inline_start.max(0.0),
            inline_size: logical.inline_size,
        }
    }

    /// Find the first lower float band that can contain the requested inline
    /// advance. CSS moves a line down when the interval beside a float cannot
    /// contain any of its content.
    pub fn next_wider_block_start(
        &self,
        line_block_start: f32,
        line_block_size: f32,
        required_inline_size: f32,
    ) -> Option<f32> {
        let absolute_start = self.block_offset + line_block_start;
        // An unbreakable line wider than its containing block still moves to
        // the first band that restores the containing block's full width,
        // then overflows there. Requiring its impossible full advance would
        // leave it beside a float that occurs later in the inline sequence.
        let required_inline_size = required_inline_size.min(self.containing_inline_size);
        let active = self
            .exclusions
            .iter()
            .filter(|exclusion| {
                exclusion.is_valid_exclusion()
                    && exclusion.line_area.is_valid_exclusion()
                    && exclusion.line_area.block_end() > absolute_start
            })
            .collect::<Vec<_>>();
        let rounded_count = active
            .iter()
            .filter(|exclusion| matches!(exclusion.line_area, FloatLineArea::Rounded { .. }))
            .count();
        let fits = |candidate| {
            available_line_space_for(
                self.containing_inline_size,
                &self.exclusions,
                candidate,
                line_block_size,
            )
            .inline_size
                + 0.01
                >= required_inline_size
        };

        // Rectangles change only at exact boundaries. Intersections between
        // several moving contours need their own global envelope solver. Use
        // contour bisection only for exactly one rounded area; every other
        // case retries at exact complete-area boundaries.
        if rounded_count != 1 {
            let mut block_ends = active
                .into_iter()
                .map(|exclusion| exclusion.line_area.block_end())
                .filter(|block_end| *block_end > absolute_start)
                .collect::<Vec<_>>();
            block_ends.sort_by(f32::total_cmp);
            block_ends.dedup_by(|left, right| (*left - *right).abs() <= f32::EPSILON);
            return block_ends.into_iter().find_map(|candidate| {
                fits(candidate).then_some((candidate - self.block_offset).max(line_block_start))
            });
        }

        let mut candidates = Vec::new();
        for exclusion in active {
            candidates.extend(
                exclusion
                    .line_area
                    .retry_breakpoints(line_block_size)
                    .into_iter()
                    .filter(|point| *point > absolute_start),
            );
        }
        candidates.sort_by(f32::total_cmp);
        candidates.dedup_by(|left, right| (*left - *right).abs() <= f32::EPSILON);
        let mut lower = absolute_start;
        for mut upper in candidates {
            if !fits(upper) {
                lower = upper;
                continue;
            }
            // Rounded contours widen continuously through their block-end
            // corner. Rectangular boundaries are discontinuous; the same
            // search converges on their exact edge because only `upper` fits.
            for _ in 0..24 {
                let middle = lower + (upper - lower) * 0.5;
                if fits(middle) {
                    upper = middle;
                } else {
                    lower = middle;
                }
            }
            return Some((upper - self.block_offset).max(line_block_start));
        }
        None
    }
}

/// Normal-flow cursor for a block formatting context.
pub struct BlockFormattingContext {
    containing_block: BlockContainingBlock,
    block_cursor: f32,
    active_margin: CollapsedMargin,
    first_margin: CollapsedMargin,
    has_child: bool,
    adjoining_parent_start: bool,
    all_children_collapse_through: bool,
    collapse_parent_start: bool,
    float_exclusions: Vec<FloatExclusion>,
    inherited_float_count: usize,
    latest_float_block_start: f32,
    active_margin_has_clearance: bool,
}

impl BlockFormattingContext {
    pub fn new(containing_block: BlockContainingBlock) -> Self {
        Self::with_margin_collapse(containing_block, false)
    }

    pub fn with_margin_collapse(
        containing_block: BlockContainingBlock,
        collapse_parent_start: bool,
    ) -> Self {
        Self::with_float_state(
            containing_block,
            collapse_parent_start,
            FloatContextState::default(),
        )
    }

    pub(crate) fn with_float_state(
        containing_block: BlockContainingBlock,
        collapse_parent_start: bool,
        float_state: FloatContextState,
    ) -> Self {
        let inherited_float_count = float_state.exclusions.len();
        let latest_float_block_start = float_state
            .exclusions
            .iter()
            .filter(|exclusion| exclusion.is_valid_exclusion())
            .map(|exclusion| exclusion.block_start())
            .fold(0.0, f32::max);
        Self {
            containing_block,
            block_cursor: 0.0,
            active_margin: CollapsedMargin::ZERO,
            first_margin: CollapsedMargin::ZERO,
            has_child: false,
            adjoining_parent_start: true,
            all_children_collapse_through: true,
            collapse_parent_start,
            float_exclusions: float_state.exclusions,
            inherited_float_count,
            latest_float_block_start,
            active_margin_has_clearance: false,
        }
    }

    /// Translate all active exclusions into a descendant content box.
    pub(crate) fn float_state_for_descendant(
        &self,
        inline_offset: f32,
        block_offset: f32,
        descendant_flow: FlowAxes,
        descendant_inline_size: f32,
    ) -> FloatContextState {
        let mut state = FloatContextState {
            exclusions: self
                .float_exclusions
                .iter()
                .cloned()
                .map(|mut exclusion| {
                    exclusion.margin_box.inline_start -= inline_offset;
                    exclusion.margin_box.block.start -= block_offset;
                    exclusion.line_area.translate(-inline_offset, -block_offset);
                    exclusion
                })
                .collect(),
        };
        if self.containing_block.flow.inline_start() != descendant_flow.inline_start() {
            // Translation put the exclusion in the descendant's physical
            // content box; mirror it into the descendant's logical direction.
            state.mirror_inline(descendant_inline_size);
        }
        state
    }

    /// Return only floats created by this ordinary block, excluding inherited
    /// ancestor exclusions.
    pub(crate) fn exported_float_state(&self) -> FloatContextState {
        FloatContextState {
            exclusions: self.float_exclusions[self.inherited_float_count..].to_vec(),
        }
    }

    /// Merge a descendant's newly created floats back into this BFC.
    pub(crate) fn import_descendant_float_state(
        &mut self,
        mut state: FloatContextState,
        inline_offset: f32,
        block_offset: f32,
        descendant_flow: FlowAxes,
        descendant_inline_size: f32,
    ) {
        if self.containing_block.flow.inline_start() != descendant_flow.inline_start() {
            // Undo the descendant's logical direction before translating its
            // newly created floats back into this context.
            state.mirror_inline(descendant_inline_size);
        }
        for mut exclusion in state.exclusions {
            exclusion.margin_box.inline_start += inline_offset;
            exclusion.margin_box.block.start += block_offset;
            exclusion.line_area.translate(inline_offset, block_offset);
            if exclusion.is_valid_exclusion() {
                self.latest_float_block_start =
                    self.latest_float_block_start.max(exclusion.block_start());
            }
            self.float_exclusions.push(exclusion);
        }
    }

    /// Place a definite-size float using CSS's high-as-possible, then
    /// side-most placement rules. The stored exclusion is the float margin
    /// box; the returned rectangle is its border box.
    pub fn place_float(
        &mut self,
        style: BlockStyle,
        border_box_size: PhysicalSize,
    ) -> BlockPlacement {
        debug_assert_ne!(style.float, FloatSide::None);
        let containing_size = self
            .containing_block
            .flow
            .logical_size(self.containing_block.content_box);
        let margin = style.logical_margin(containing_size.inline);
        let margin_inline_start = margin.inline_start.unwrap_or(0.0);
        let margin_inline_end = margin.inline_end.unwrap_or(0.0);
        let margin_block_start = margin.block_start.unwrap_or(0.0);
        let margin_block_end = margin.block_end.unwrap_or(0.0);
        let border_size = self.containing_block.flow.logical_size(border_box_size);
        let outer_inline_size = margin_inline_start + border_size.inline + margin_inline_end;
        let outer_block_size = margin_block_start + border_size.block + margin_block_end;
        let hypothetical_block_start =
            self.block_cursor + self.active_margin.resolve() + margin_block_start;
        let mut margin_box_block_start = hypothetical_block_start - margin_block_start;
        if let Some(clear_block_end) = self.clearance_block_end(style.clear) {
            margin_box_block_start = margin_box_block_start.max(clear_block_end);
        }
        margin_box_block_start = margin_box_block_start.max(self.latest_float_block_start);
        let at_inline_start = self.float_is_at_inline_start(style.float);

        let margin_box_inline_start = loop {
            let available = self.available_inline_space(margin_box_block_start, outer_block_size);
            if outer_inline_size <= available.inline_size
                || !self.float_exclusions_overlap(margin_box_block_start, outer_block_size)
            {
                break if at_inline_start {
                    available.inline_start
                } else {
                    available.inline_start + available.inline_size - outer_inline_size
                };
            }
            let Some(next_block_start) =
                self.next_float_block_end(margin_box_block_start, outer_block_size)
            else {
                break if at_inline_start {
                    available.inline_start
                } else {
                    available.inline_start + available.inline_size - outer_inline_size
                };
            };
            margin_box_block_start = next_block_start;
        };
        let margin_box = FloatMarginBox {
            inline_start: margin_box_inline_start,
            inline_size: outer_inline_size,
            block: SignedBlockExtent {
                start: margin_box_block_start,
                used_size: outer_block_size,
            },
        };
        let border_box = LogicalRect {
            inline_start: margin_box.inline_start + margin_inline_start,
            block_start: margin_box.block.start + margin_block_start,
            inline_size: border_size.inline,
            block_size: border_size.block,
        };
        let line_area = float_line_area(style, containing_size.inline, margin_box, border_box);
        self.float_exclusions.push(FloatExclusion {
            side: style.float,
            at_inline_start,
            margin_box,
            line_area,
        });
        if margin_box.block.is_valid_exclusion() {
            self.latest_float_block_start =
                self.latest_float_block_start.max(margin_box.block.start);
        }

        BlockPlacement {
            logical_rect: border_box,
            rect: self
                .containing_block
                .flow
                .physical_rect(border_box, self.containing_block.content_box),
            margin_inline_start,
            margin_inline_end,
        }
    }

    /// Return the flow-relative interval not intersected by a float margin
    /// box across the supplied block-axis span.
    pub fn available_inline_space(&self, block_start: f32, block_size: f32) -> FloatAvailableSpace {
        let containing_inline = self
            .containing_block
            .flow
            .logical_size(self.containing_block.content_box)
            .inline;
        available_margin_box_space_for(
            containing_inline,
            &self.float_exclusions,
            block_start,
            block_size,
        )
    }

    /// Choose the highest float band in which an independent BFC's border
    /// box fits without intersecting a float margin box.
    ///
    /// `width: auto` is resolved against the selected band. A definite or
    /// minimum width that cannot fit moves to the next overlapping float
    /// boundary. The caller may repeat this query with the measured block
    /// size when layout at the candidate width changes the box's height.
    pub fn float_avoiding_placement(
        &self,
        style: BlockStyle,
        margin_state: BlockMarginState,
        border_box_block_size: f32,
    ) -> FloatAvoidingPlacement {
        debug_assert!(style.establishes_bfc);
        debug_assert_eq!(style.float, FloatSide::None);
        let containing_inline = self
            .containing_block
            .flow
            .logical_size(self.containing_block.content_box)
            .inline;
        let mut block_start = self.hypothetical_in_flow_block_start(style, margin_state);

        loop {
            if !self.float_exclusions_overlap(block_start, border_box_block_size) {
                let inline_size = solve_in_flow_inline_size(style, containing_inline);
                return FloatAvoidingPlacement {
                    block_start,
                    inline_start: inline_size.margin_start,
                    inline_size,
                };
            }

            let available = self.available_inline_space(block_start, border_box_block_size);
            let inline_size = solve_in_flow_inline_size_for_available(
                style,
                containing_inline,
                available.inline_size,
            );
            let inline_start = available.inline_start + inline_size.margin_start;
            let available_end = available.inline_start + available.inline_size;
            let border_end = inline_start + inline_size.border_box;
            if inline_start + 0.01 >= available.inline_start && border_end <= available_end + 0.01 {
                return FloatAvoidingPlacement {
                    block_start,
                    inline_start,
                    inline_size,
                };
            }

            // A negative inline-end margin may let this BFC's border box
            // extend into an inline-end float while its contracted margin box
            // still fits the available band. This covers a physical-left
            // float in RTL and a physical-right float in LTR.
            // Use the authored margin rather than the solved one: the width
            // equation can make an otherwise zero inline-end margin negative
            // when an inline-start margin already over-constrains the band.
            let authored_negative_inline_end = style
                .logical_margin(containing_inline)
                .inline_end
                .unwrap_or(0.0)
                .min(0.0);
            let has_inline_end_float_at_this_band = self.float_exclusions.iter().any(|exclusion| {
                !exclusion.at_inline_start
                    && exclusion.margin_box_overlaps_block(block_start, border_box_block_size)
            });
            let margin_box_fits_inline_end_band = has_inline_end_float_at_this_band
                // A nonzero available start means an inline-start float also
                // constrains this band, so there is no opposite edge to use.
                && available.inline_start <= 0.01
                && authored_negative_inline_end < 0.0
                && border_end - available_end <= -authored_negative_inline_end + 0.01;
            if margin_box_fits_inline_end_band {
                return FloatAvoidingPlacement {
                    block_start,
                    inline_start,
                    inline_size,
                };
            }

            let Some(next_block_start) =
                self.next_float_block_end(block_start, border_box_block_size)
            else {
                let inline_size = solve_in_flow_inline_size(style, containing_inline);
                return FloatAvoidingPlacement {
                    block_start,
                    inline_start: inline_size.margin_start,
                    inline_size,
                };
            };
            block_start = next_block_start;
        }
    }

    pub fn float_exclusion_count(&self) -> usize {
        self.float_exclusions.len()
    }

    /// Snapshot active floats for line breaking whose local block-axis origin
    /// starts at `block_offset` in this BFC.
    pub fn float_line_constraints(&self, block_offset: f32) -> Option<FloatLineConstraints> {
        self.float_exclusions
            .iter()
            .any(|exclusion| exclusion.is_valid_exclusion())
            .then(|| FloatLineConstraints {
                flow: self.containing_block.flow,
                containing_inline_size: self
                    .containing_block
                    .flow
                    .logical_size(self.containing_block.content_box)
                    .inline,
                block_offset,
                exclusions: self.float_exclusions.clone(),
            })
    }

    pub(crate) fn hypothetical_in_flow_block_start(
        &self,
        style: BlockStyle,
        margin_state: BlockMarginState,
    ) -> f32 {
        let normal = self.normal_in_flow_block_start(margin_state);
        self.clearance_block_end(style.clear)
            .map_or(normal, |clearance| normal.max(clearance))
    }

    fn normal_in_flow_block_start(&self, margin_state: BlockMarginState) -> f32 {
        let adjoining = self.active_margin.collapse_with(margin_state.block_start);
        let participates_in_parent_start =
            self.adjoining_parent_start && self.collapse_parent_start;
        if participates_in_parent_start {
            self.block_cursor
        } else {
            self.block_cursor + adjoining.resolve()
        }
    }

    pub fn place_in_flow(
        &mut self,
        style: BlockStyle,
        border_box_size: PhysicalSize,
    ) -> BlockPlacement {
        let containing_inline = self
            .containing_block
            .flow
            .logical_size(self.containing_block.content_box)
            .inline;
        let margin = style.logical_margin(containing_inline);
        self.place_in_flow_with_margins(
            style,
            border_box_size,
            BlockMarginState {
                block_start: CollapsedMargin::from_margin(margin.block_start.unwrap_or(0.0)),
                block_end: CollapsedMargin::from_margin(margin.block_end.unwrap_or(0.0)),
                collapses_through: false,
            },
        )
    }

    pub fn place_in_flow_with_margins(
        &mut self,
        style: BlockStyle,
        border_box_size: PhysicalSize,
        margin_state: BlockMarginState,
    ) -> BlockPlacement {
        let containing_inline = self
            .containing_block
            .flow
            .logical_size(self.containing_block.content_box)
            .inline;
        let used_inline = solve_in_flow_inline_size(style, containing_inline);
        self.place_in_flow_with_used_inline(style, border_box_size, margin_state, used_inline)
    }

    /// Place an in-flow child whose inline equation the caller has already
    /// solved, for example a table wrapper box whose border box came from its
    /// grid rather than from `width`.
    pub fn place_in_flow_with_used_inline(
        &mut self,
        style: BlockStyle,
        border_box_size: PhysicalSize,
        margin_state: BlockMarginState,
        used_inline: UsedInlineSize,
    ) -> BlockPlacement {
        self.place_in_flow_at(
            style,
            border_box_size,
            margin_state,
            used_inline.margin_start,
            used_inline,
            None,
        )
    }

    /// Commit a previously measured float-avoiding BFC placement to normal
    /// flow. The candidate is produced by [`Self::float_avoiding_placement`].
    pub fn place_float_avoiding_in_flow(
        &mut self,
        style: BlockStyle,
        border_box_size: PhysicalSize,
        margin_state: BlockMarginState,
        placement: FloatAvoidingPlacement,
    ) -> BlockPlacement {
        self.place_in_flow_at(
            style,
            border_box_size,
            margin_state,
            placement.inline_start,
            placement.inline_size,
            Some(placement.block_start),
        )
    }

    fn place_in_flow_at(
        &mut self,
        style: BlockStyle,
        border_box_size: PhysicalSize,
        margin_state: BlockMarginState,
        inline_start: f32,
        used_inline: UsedInlineSize,
        minimum_block_start: Option<f32>,
    ) -> BlockPlacement {
        let adjoining = self.active_margin.collapse_with(margin_state.block_start);
        let normal_block_start = self.normal_in_flow_block_start(margin_state);
        let mut block_start =
            normal_block_start.max(minimum_block_start.unwrap_or(normal_block_start));
        if let Some(clear_block_end) = self.clearance_block_end(style.clear) {
            block_start = block_start.max(clear_block_end);
        }
        let has_clearance = block_start > normal_block_start;
        let child_size = self.containing_block.flow.logical_size(border_box_size);
        let logical = LogicalRect {
            inline_start,
            block_start,
            inline_size: child_size.inline,
            block_size: child_size.block,
        };

        if has_clearance {
            if self.adjoining_parent_start {
                self.first_margin = CollapsedMargin::ZERO;
            }
            self.adjoining_parent_start = false;
        } else if self.adjoining_parent_start {
            self.first_margin = adjoining;
            if margin_state.collapses_through {
                self.first_margin = self.first_margin.collapse_with(margin_state.block_end);
            } else {
                self.adjoining_parent_start = false;
            }
        }
        if margin_state.collapses_through {
            if has_clearance {
                // Clearance separates this empty box's adjoining margins
                // from the preceding chain. Its own start and end margins
                // may still collapse through into following siblings, but
                // that resulting chain may not collapse with the parent's
                // block-end margin.
                self.block_cursor = logical.block_start;
                self.active_margin = margin_state
                    .block_start
                    .collapse_with(margin_state.block_end);
                self.active_margin_has_clearance = true;
                self.all_children_collapse_through = false;
            } else {
                self.active_margin = adjoining.collapse_with(margin_state.block_end);
            }
        } else {
            self.block_cursor = logical.block_start + logical.block_size;
            self.active_margin = margin_state.block_end;
            self.active_margin_has_clearance = false;
            self.all_children_collapse_through = false;
        }
        self.has_child = true;

        BlockPlacement {
            logical_rect: logical,
            rect: self
                .containing_block
                .flow
                .physical_rect(logical, self.containing_block.content_box),
            margin_inline_start: used_inline.margin_start,
            margin_inline_end: used_inline.margin_end,
        }
    }

    pub fn all_children_collapse_through(&self) -> bool {
        self.all_children_collapse_through
    }

    pub fn first_child_margin(&self) -> CollapsedMargin {
        if self.has_child {
            self.first_margin
        } else {
            CollapsedMargin::ZERO
        }
    }

    pub fn last_child_margin(&self) -> CollapsedMargin {
        if self.has_child {
            self.active_margin
        } else {
            CollapsedMargin::ZERO
        }
    }

    /// Whether the active trailing margin chain may adjoin the parent's
    /// block-end margin.
    pub(crate) fn active_margin_may_collapse_with_parent_end(&self) -> bool {
        !self.active_margin_has_clearance
    }

    pub fn used_block_size_with_margin_collapse(&self, collapse_parent_end: bool) -> f32 {
        self.block_cursor
            + if self.has_child && !collapse_parent_end {
                self.active_margin.resolve()
            } else {
                0.0
            }
    }

    pub fn used_block_size(&self) -> f32 {
        self.used_block_size_with_margin_collapse(false)
    }

    /// Auto block size of a box that establishes a BFC includes the bottom
    /// margin edge of its in-flow floats.
    pub fn used_block_size_containing_floats(&self, collapse_parent_end: bool) -> f32 {
        self.used_block_size_with_margin_collapse(collapse_parent_end)
            .max(self.lowest_float_block_end())
    }

    fn float_is_at_inline_start(&self, side: FloatSide) -> bool {
        matches!(
            (side, self.containing_block.flow.inline_start()),
            (FloatSide::Left, crate::PhysicalSide::Left)
                | (FloatSide::Right, crate::PhysicalSide::Right)
        )
    }

    fn clearance_block_end(&self, clear: ClearSide) -> Option<f32> {
        self.float_exclusions
            .iter()
            .filter(|exclusion| match clear {
                ClearSide::None => false,
                ClearSide::Left => exclusion.side == FloatSide::Left,
                ClearSide::Right => exclusion.side == FloatSide::Right,
                ClearSide::Both => true,
            })
            .filter(|exclusion| exclusion.is_valid_exclusion())
            .map(|exclusion| exclusion.block_end())
            .reduce(f32::max)
    }

    fn lowest_float_block_end(&self) -> f32 {
        self.float_exclusions
            .iter()
            .filter(|exclusion| exclusion.is_valid_exclusion())
            .map(|exclusion| exclusion.block_end())
            .fold(0.0, f32::max)
    }

    fn float_exclusions_overlap(&self, block_start: f32, block_size: f32) -> bool {
        self.float_exclusions
            .iter()
            .any(|exclusion| exclusion.margin_box_overlaps_block(block_start, block_size))
    }

    fn next_float_block_end(&self, block_start: f32, block_size: f32) -> Option<f32> {
        self.float_exclusions
            .iter()
            .filter(|exclusion| exclusion.margin_box_overlaps_block(block_start, block_size))
            .map(|exclusion| exclusion.block_end())
            .filter(|block_end| *block_end > block_start)
            .min_by(f32::total_cmp)
    }
}

fn float_line_area(
    style: BlockStyle,
    containing_inline_size: f32,
    margin_box: FloatMarginBox,
    border_box: LogicalRect,
) -> FloatLineArea {
    // A float without an authored box-valued shape keeps the CSS 2.1 margin
    // rectangle, even if it is painted with rounded borders.
    if !style.has_shape_outside_box
        || !matches!(
            style.containing_flow.inline_start(),
            crate::PhysicalSide::Left | crate::PhysicalSide::Right
        )
    {
        return FloatLineArea::Rect(margin_box);
    }

    if style.float_reference_box == FloatReferenceBox::MarginBox {
        let radii = reference_box_radii(style, border_box, margin_box);
        return if radii == LogicalCornerRadii::default() {
            FloatLineArea::Rect(margin_box)
        } else {
            FloatLineArea::Rounded {
                rect: margin_box,
                clip: margin_box,
                radii,
            }
        };
    }

    let border = style.containing_flow.logical_sides(style.border);
    let padding = style
        .containing_flow
        .logical_sides(style.resolved_padding(containing_inline_size));
    let inset = match style.float_reference_box {
        FloatReferenceBox::MarginBox => unreachable!("handled above"),
        FloatReferenceBox::BorderBox => crate::LogicalSides {
            inline_start: 0.0,
            inline_end: 0.0,
            block_start: 0.0,
            block_end: 0.0,
        },
        FloatReferenceBox::PaddingBox => border,
        FloatReferenceBox::ContentBox => crate::LogicalSides {
            inline_start: border.inline_start + padding.inline_start,
            inline_end: border.inline_end + padding.inline_end,
            block_start: border.block_start + padding.block_start,
            block_end: border.block_end + padding.block_end,
        },
    };
    let reference_box = FloatMarginBox {
        inline_start: border_box.inline_start + inset.inline_start,
        inline_size: (border_box.inline_size - inset.inline_start - inset.inline_end).max(0.0),
        block: SignedBlockExtent {
            start: border_box.block_start + inset.block_start,
            used_size: (border_box.block_size - inset.block_start - inset.block_end).max(0.0),
        },
    };
    let clipped_reference_box = clip_float_area_to_margin_box(reference_box, margin_box);
    let radii = reference_box_radii(style, border_box, reference_box);
    if radii == LogicalCornerRadii::default() {
        FloatLineArea::Rect(clipped_reference_box)
    } else {
        FloatLineArea::Rounded {
            rect: reference_box,
            clip: clipped_reference_box,
            radii,
        }
    }
}

fn reference_box_radii(
    style: BlockStyle,
    border_box: LogicalRect,
    reference_box: FloatMarginBox,
) -> LogicalCornerRadii {
    if reference_box.inline_size <= 0.0 || reference_box.block.used_size <= 0.0 {
        return LogicalCornerRadii::default();
    }
    let physical = style.corner_radii;
    let resolve = |radius: BlockCornerRadius| LogicalCornerRadius {
        inline: radius.horizontal.resolve(border_box.inline_size).max(0.0),
        block: radius.vertical.resolve(border_box.block_size).max(0.0),
    };
    let mut radii = match style.containing_flow.inline_start() {
        crate::PhysicalSide::Left => LogicalCornerRadii {
            block_start_inline_start: resolve(physical.top_left),
            block_start_inline_end: resolve(physical.top_right),
            block_end_inline_end: resolve(physical.bottom_right),
            block_end_inline_start: resolve(physical.bottom_left),
        },
        crate::PhysicalSide::Right => LogicalCornerRadii {
            block_start_inline_start: resolve(physical.top_right),
            block_start_inline_end: resolve(physical.top_left),
            block_end_inline_end: resolve(physical.bottom_left),
            block_end_inline_start: resolve(physical.bottom_right),
        },
        crate::PhysicalSide::Top | crate::PhysicalSide::Bottom => {
            return LogicalCornerRadii::default();
        },
    };
    normalize_corner_radii(&mut radii, border_box.inline_size, border_box.block_size);

    let insets = crate::LogicalSides {
        inline_start: reference_box.inline_start - border_box.inline_start,
        inline_end: border_box.inline_start + border_box.inline_size
            - reference_box.inline_start
            - reference_box.inline_size,
        block_start: reference_box.block.start - border_box.block_start,
        block_end: border_box.block_start + border_box.block_size
            - reference_box.block.start
            - reference_box.block.used_size,
    };
    if style.float_reference_box == FloatReferenceBox::MarginBox {
        let margin_inline_start = -insets.inline_start;
        let margin_inline_end = -insets.inline_end;
        let margin_block_start = -insets.block_start;
        let margin_block_end = -insets.block_end;
        radii.block_start_inline_start.inline =
            margin_box_corner_radius(radii.block_start_inline_start.inline, margin_inline_start);
        radii.block_start_inline_start.block =
            margin_box_corner_radius(radii.block_start_inline_start.block, margin_block_start);
        radii.block_start_inline_end.inline =
            margin_box_corner_radius(radii.block_start_inline_end.inline, margin_inline_end);
        radii.block_start_inline_end.block =
            margin_box_corner_radius(radii.block_start_inline_end.block, margin_block_start);
        radii.block_end_inline_end.inline =
            margin_box_corner_radius(radii.block_end_inline_end.inline, margin_inline_end);
        radii.block_end_inline_end.block =
            margin_box_corner_radius(radii.block_end_inline_end.block, margin_block_end);
        radii.block_end_inline_start.inline =
            margin_box_corner_radius(radii.block_end_inline_start.inline, margin_inline_start);
        radii.block_end_inline_start.block =
            margin_box_corner_radius(radii.block_end_inline_start.block, margin_block_end);
        normalize_corner_radii(
            &mut radii,
            reference_box.inline_size,
            reference_box.block.used_size,
        );
        return radii;
    }

    // Inner reference boxes contract the corresponding used border radius by
    // their distance from the outside border edge.
    for radius in [
        &mut radii.block_start_inline_start,
        &mut radii.block_start_inline_end,
    ] {
        radius.block = (radius.block - insets.block_start).max(0.0);
    }
    for radius in [
        &mut radii.block_end_inline_start,
        &mut radii.block_end_inline_end,
    ] {
        radius.block = (radius.block - insets.block_end).max(0.0);
    }
    for radius in [
        &mut radii.block_start_inline_start,
        &mut radii.block_end_inline_start,
    ] {
        radius.inline = (radius.inline - insets.inline_start).max(0.0);
    }
    for radius in [
        &mut radii.block_start_inline_end,
        &mut radii.block_end_inline_end,
    ] {
        radius.inline = (radius.inline - insets.inline_end).max(0.0);
    }
    normalize_corner_radii(
        &mut radii,
        reference_box.inline_size,
        reference_box.block.used_size,
    );
    radii
}

fn margin_box_corner_radius(border_radius: f32, margin: f32) -> f32 {
    if margin <= 0.0 {
        return (border_radius + margin).max(0.0);
    }
    let ratio = border_radius / margin;
    if ratio >= 1.0 {
        border_radius + margin
    } else {
        border_radius + margin * (1.0 + (ratio - 1.0).powi(3))
    }
}

fn normalize_corner_radii(radii: &mut LogicalCornerRadii, inline_size: f32, block_size: f32) {
    let inline_size = inline_size.max(0.0);
    let block_size = block_size.max(0.0);
    let scale = [
        inline_size / (radii.block_start_inline_start.inline + radii.block_start_inline_end.inline),
        inline_size / (radii.block_end_inline_start.inline + radii.block_end_inline_end.inline),
        block_size / (radii.block_start_inline_start.block + radii.block_end_inline_start.block),
        block_size / (radii.block_start_inline_end.block + radii.block_end_inline_end.block),
    ]
    .into_iter()
    .filter(|value| value.is_finite())
    .fold(1.0_f32, f32::min)
    .min(1.0);
    if scale >= 1.0 {
        return;
    }
    for radius in [
        &mut radii.block_start_inline_start,
        &mut radii.block_start_inline_end,
        &mut radii.block_end_inline_end,
        &mut radii.block_end_inline_start,
    ] {
        radius.inline *= scale;
        radius.block *= scale;
    }
}

fn clip_float_area_to_margin_box(
    float_area: FloatMarginBox,
    margin_box: FloatMarginBox,
) -> FloatMarginBox {
    if !margin_box.block.is_valid_exclusion() {
        return margin_box;
    }
    let inline_start = float_area.inline_start.max(margin_box.inline_start);
    let inline_end = (float_area.inline_start + float_area.inline_size)
        .min(margin_box.inline_start + margin_box.inline_size);
    let block_start = float_area.block.start.max(margin_box.block.start);
    let block_end = float_area.block.end().min(margin_box.block.end());
    FloatMarginBox {
        inline_start,
        inline_size: (inline_end - inline_start).max(0.0),
        block: SignedBlockExtent {
            start: block_start,
            used_size: (block_end - block_start).max(0.0),
        },
    }
}

fn available_margin_box_space_for(
    containing_inline: f32,
    exclusions: &[FloatExclusion],
    block_start: f32,
    block_size: f32,
) -> FloatAvailableSpace {
    let mut inline_start: f32 = 0.0;
    let mut inline_end = containing_inline;
    for exclusion in exclusions
        .iter()
        .filter(|exclusion| exclusion.margin_box_overlaps_block(block_start, block_size))
    {
        if exclusion.at_inline_start {
            inline_start = inline_start
                .max(exclusion.margin_box.inline_start + exclusion.margin_box.inline_size);
        } else {
            inline_end = inline_end.min(exclusion.margin_box.inline_start);
        }
    }
    FloatAvailableSpace {
        inline_start,
        inline_size: (inline_end - inline_start).max(0.0),
    }
}

fn available_line_space_for(
    containing_inline: f32,
    exclusions: &[FloatExclusion],
    block_start: f32,
    block_size: f32,
) -> FloatAvailableSpace {
    let mut inline_start: f32 = 0.0;
    let mut inline_end = containing_inline;
    for exclusion in exclusions
        .iter()
        .filter(|exclusion| exclusion.line_area_overlaps_block(block_start, block_size))
    {
        let Some((area_start, area_end)) = exclusion
            .line_area
            .inline_bounds_for_span(block_start, block_size)
        else {
            continue;
        };
        if exclusion.at_inline_start {
            inline_start = inline_start.max(area_end);
        } else {
            inline_end = inline_end.min(area_start);
        }
    }
    FloatAvailableSpace {
        inline_start,
        inline_size: (inline_end - inline_start).max(0.0),
    }
}

fn spans_overlap(first_start: f32, first_size: f32, second_start: f32, second_size: f32) -> bool {
    if first_size < 0.0 || second_size < 0.0 {
        return false;
    }
    // A zero-height float excludes a line only when the line strictly
    // straddles its block position. A line beginning at the same position is
    // not narrowed. A provisional zero-height line still samples a nonzero
    // float area at its top edge.
    if first_size == 0.0 {
        return second_size > 0.0
            && second_start < first_start
            && first_start < second_start + second_size;
    }
    let first_end = first_start + first_size;
    let second_end = second_start + second_size;
    if second_size == 0.0 {
        first_start <= second_start && second_start < first_end
    } else {
        first_start < second_end && second_start < first_end
    }
}

#[cfg(test)]
mod tests {
    use crate::{Direction, WritingMode};

    use super::*;

    fn fixed_width(width: f32) -> BlockStyle {
        BlockStyle {
            size: BlockDimensions::new(
                BlockSizeValue::Length(FlowLength::px(width)),
                BlockSizeValue::Auto,
            ),
            ..BlockStyle::default()
        }
    }

    #[test]
    fn block_width_equation_centres_two_auto_margins() {
        let mut style = fixed_width(100.0);
        style.margin.left = FlowLengthAuto::Auto;
        style.margin.right = FlowLengthAuto::Auto;

        assert_eq!(
            solve_in_flow_inline_size(style, 300.0),
            UsedInlineSize {
                margin_start: 100.0,
                border_box: 100.0,
                margin_end: 100.0,
            }
        );
    }

    #[test]
    fn one_auto_inline_margin_uses_the_logical_start_in_both_directions() {
        for (flow, auto_on_left) in [
            (FlowAxes::HORIZONTAL_LTR, true),
            (
                FlowAxes::new(WritingMode::HorizontalTb, Direction::Rtl),
                false,
            ),
        ] {
            let mut style = fixed_width(100.0);
            style.flow = flow;
            style.containing_flow = flow;
            if auto_on_left {
                style.margin.left = FlowLengthAuto::Auto;
                style.margin.right = FlowLengthAuto::Value(FlowLength::px(20.0));
            } else {
                style.margin.left = FlowLengthAuto::Value(FlowLength::px(20.0));
                style.margin.right = FlowLengthAuto::Auto;
            }

            assert_eq!(
                solve_in_flow_inline_size(style, 300.0),
                UsedInlineSize {
                    margin_start: 180.0,
                    border_box: 100.0,
                    margin_end: 20.0,
                },
                "flow={flow:?}"
            );
        }
    }

    #[test]
    fn overconstrained_auto_inline_start_resolves_to_zero() {
        for (flow, auto_on_left) in [
            (FlowAxes::HORIZONTAL_LTR, true),
            (
                FlowAxes::new(WritingMode::HorizontalTb, Direction::Rtl),
                false,
            ),
        ] {
            let mut style = fixed_width(200.0);
            style.flow = flow;
            style.containing_flow = flow;
            if auto_on_left {
                style.margin.left = FlowLengthAuto::Auto;
                style.margin.right = FlowLengthAuto::Value(FlowLength::px(25.0));
            } else {
                style.margin.left = FlowLengthAuto::Value(FlowLength::px(25.0));
                style.margin.right = FlowLengthAuto::Auto;
            }

            assert_eq!(
                solve_in_flow_inline_size(style, 100.0),
                UsedInlineSize {
                    margin_start: 0.0,
                    border_box: 200.0,
                    margin_end: -100.0,
                },
                "flow={flow:?}"
            );
        }
    }

    #[test]
    fn legacy_alignment_adjusts_only_positive_overconstrained_remaining_space() {
        let cases = [
            (
                OverconstrainedInlineAlignment::LineLeft,
                FlowAxes::HORIZONTAL_LTR,
                (10.0, 190.0),
            ),
            (
                OverconstrainedInlineAlignment::Center,
                FlowAxes::HORIZONTAL_LTR,
                (100.0, 100.0),
            ),
            (
                OverconstrainedInlineAlignment::LineRight,
                FlowAxes::HORIZONTAL_LTR,
                (190.0, 10.0),
            ),
            (
                OverconstrainedInlineAlignment::LineLeft,
                FlowAxes::new(WritingMode::HorizontalTb, Direction::Rtl),
                (190.0, 10.0),
            ),
        ];
        for (alignment, flow, (margin_start, margin_end)) in cases {
            let mut style = fixed_width(100.0);
            style.flow = flow;
            style.containing_flow = flow;
            style.margin.left = FlowLengthAuto::Value(FlowLength::px(10.0));
            style.margin.right = FlowLengthAuto::Value(FlowLength::px(10.0));
            style.overconstrained_inline_alignment = Some(alignment);

            assert_eq!(
                solve_in_flow_inline_size(style, 300.0),
                UsedInlineSize {
                    margin_start,
                    border_box: 100.0,
                    margin_end,
                },
                "alignment={alignment:?}, flow={flow:?}"
            );
        }

        let mut overflowing = fixed_width(320.0);
        overflowing.overconstrained_inline_alignment = Some(OverconstrainedInlineAlignment::Center);
        overflowing.margin.left = FlowLengthAuto::Value(FlowLength::px(10.0));
        overflowing.margin.right = FlowLengthAuto::Value(FlowLength::px(10.0));
        assert_eq!(
            solve_in_flow_inline_size(overflowing, 300.0),
            UsedInlineSize {
                margin_start: 10.0,
                border_box: 320.0,
                margin_end: -30.0,
            }
        );
    }

    #[test]
    fn adjoining_positive_and_negative_margins_collapse_separately() {
        assert_eq!(
            CollapsedMargin::from_margin(20.0)
                .collapse(12.0)
                .collapse(-7.0)
                .collapse(-4.0)
                .resolve(),
            13.0
        );
    }

    #[test]
    fn negative_sibling_margin_can_move_a_block_before_the_parent_start() {
        let containing = BlockContainingBlock {
            flow: FlowAxes::HORIZONTAL_LTR,
            content_box: PhysicalSize {
                width: 200.0,
                height: 0.0,
            },
        };
        let child = fixed_width(200.0);
        let mut context = BlockFormattingContext::new(containing);
        context.place_in_flow_with_margins(
            child,
            PhysicalSize {
                width: 200.0,
                height: 50.0,
            },
            margin_state(0.0, 0.0, false),
        );
        let pulled_up = context.place_in_flow_with_margins(
            child,
            PhysicalSize {
                width: 200.0,
                height: 100.0,
            },
            margin_state(-100.0, 0.0, false),
        );

        assert_eq!(pulled_up.rect.y, -50.0);
    }

    fn margin_state(start: f32, end: f32, collapses_through: bool) -> BlockMarginState {
        BlockMarginState {
            block_start: CollapsedMargin::from_margin(start),
            block_end: CollapsedMargin::from_margin(end),
            collapses_through,
        }
    }

    #[test]
    fn parent_start_and_end_edges_keep_child_margins_outside_used_height() {
        let containing = BlockContainingBlock {
            flow: FlowAxes::HORIZONTAL_LTR,
            content_box: PhysicalSize {
                width: 200.0,
                height: 0.0,
            },
        };
        let child = fixed_width(200.0);
        let mut context = BlockFormattingContext::with_margin_collapse(containing, true);
        let placement = context.place_in_flow_with_margins(
            child,
            PhysicalSize {
                width: 200.0,
                height: 20.0,
            },
            margin_state(30.0, 40.0, false),
        );

        assert_eq!(placement.rect.y, 0.0);
        assert_eq!(context.first_child_margin().resolve(), 30.0);
        assert_eq!(context.last_child_margin().resolve(), 40.0);
        assert_eq!(context.used_block_size_with_margin_collapse(true), 20.0);
    }

    #[test]
    fn empty_block_carries_one_margin_chain_between_siblings() {
        let containing = BlockContainingBlock {
            flow: FlowAxes::HORIZONTAL_LTR,
            content_box: PhysicalSize {
                width: 200.0,
                height: 0.0,
            },
        };
        let child = fixed_width(200.0);
        let mut context = BlockFormattingContext::new(containing);
        let first = context.place_in_flow_with_margins(
            child,
            PhysicalSize {
                width: 200.0,
                height: 10.0,
            },
            margin_state(0.0, 20.0, false),
        );
        let empty = context.place_in_flow_with_margins(
            child,
            PhysicalSize {
                width: 200.0,
                height: 0.0,
            },
            margin_state(-7.0, 12.0, true),
        );
        let last = context.place_in_flow_with_margins(
            child,
            PhysicalSize {
                width: 200.0,
                height: 10.0,
            },
            margin_state(-15.0, 0.0, false),
        );

        assert_eq!(first.rect.y, 0.0);
        assert_eq!(empty.rect.y, 23.0);
        assert_eq!(last.rect.y, 15.0);
        assert_eq!(context.used_block_size(), 25.0);
    }

    #[test]
    fn empty_auto_block_exposes_collapsing_start_and_end_sets() {
        let mut style = BlockStyle::default();
        style.margin.top = FlowLengthAuto::Value(FlowLength::px(20.0));
        style.margin.bottom = FlowLengthAuto::Value(FlowLength::px(30.0));
        let collapse = style.child_margin_collapse(200.0, None, false, true);
        let through = style.can_collapse_through(200.0, None, false, false, true);
        let margins = BlockMarginState::from_box(
            style,
            200.0,
            CollapsedMargin::from_margin(-8.0),
            CollapsedMargin::from_margin(12.0),
            collapse,
            through,
        );

        assert!(through);
        assert_eq!(margins.block_start.resolve(), 12.0);
        assert_eq!(margins.block_end.resolve(), 30.0);
        assert!(margins.collapses_through);
    }

    #[test]
    fn nonzero_minimum_only_blocks_end_collapse_when_the_chain_reaches_start() {
        let style = BlockStyle {
            min_size: BlockDimensions::new(
                BlockSizeValue::Auto,
                BlockSizeValue::Length(FlowLength::px(10.0)),
            ),
            ..BlockStyle::default()
        };

        assert!(
            style
                .child_margin_collapse(200.0, None, false, false)
                .block_end
        );
        assert!(
            !style
                .child_margin_collapse(200.0, None, false, true)
                .block_end
        );
    }

    #[test]
    fn vertical_rl_stacks_children_from_the_right_edge() {
        let flow = FlowAxes::new(WritingMode::VerticalRl, Direction::Ltr);
        let containing = BlockContainingBlock {
            flow,
            content_box: PhysicalSize {
                width: 300.0,
                height: 200.0,
            },
        };
        let mut context = BlockFormattingContext::new(containing);
        let mut child = fixed_width(40.0);
        child.containing_flow = flow;
        child.size = BlockDimensions::new(
            BlockSizeValue::Length(FlowLength::px(40.0)),
            BlockSizeValue::Length(FlowLength::px(200.0)),
        );

        let first = context.place_in_flow(
            child,
            PhysicalSize {
                width: 40.0,
                height: 200.0,
            },
        );
        let second = context.place_in_flow(
            child,
            PhysicalSize {
                width: 40.0,
                height: 200.0,
            },
        );

        assert_eq!(first.rect.x, 260.0);
        assert_eq!(second.rect.x, 220.0);
        assert_eq!(first.rect.y, 0.0);
        assert_eq!(context.used_block_size(), 80.0);
    }

    fn fixed_float(side: FloatSide, width: f32) -> BlockStyle {
        BlockStyle {
            size: BlockDimensions::new(
                BlockSizeValue::Length(FlowLength::px(width)),
                BlockSizeValue::Auto,
            ),
            float: side,
            establishes_bfc: true,
            ..BlockStyle::default()
        }
    }

    fn horizontal_context(width: f32) -> BlockFormattingContext {
        BlockFormattingContext::new(BlockContainingBlock {
            flow: FlowAxes::HORIZONTAL_LTR,
            content_box: PhysicalSize { width, height: 0.0 },
        })
    }

    #[test]
    fn definite_float_margins_resolve_auto_to_zero() {
        let mut style = fixed_float(FloatSide::Left, 80.0);
        style.margin.left = FlowLengthAuto::Auto;
        style.margin.right = FlowLengthAuto::Auto;

        assert_eq!(
            solve_float_inline_size(style, 200.0),
            UsedInlineSize {
                margin_start: 0.0,
                border_box: 80.0,
                margin_end: 0.0,
            }
        );
    }

    #[test]
    fn auto_shrink_to_fit_width_clamps_available_space_between_intrinsic_sizes() {
        let style = BlockStyle {
            padding: PhysicalSides {
                top: FlowLength::ZERO,
                right: FlowLength::px(5.0),
                bottom: FlowLength::ZERO,
                left: FlowLength::px(5.0),
            },
            float: FloatSide::Left,
            establishes_bfc: true,
            shrink_to_fit: true,
            ..BlockStyle::default()
        };
        let intrinsic = IntrinsicSizes::new(40.0, 120.0).expect("valid intrinsic pair");

        assert_eq!(
            solve_shrink_to_fit_inline_size(style, 100.0, intrinsic).border_box,
            100.0
        );
        assert_eq!(
            solve_shrink_to_fit_inline_size(style, 200.0, intrinsic).border_box,
            130.0
        );
        assert_eq!(
            solve_shrink_to_fit_inline_size(style, 30.0, intrinsic).border_box,
            50.0
        );

        let conflicting_constraints = BlockStyle {
            min_size: BlockDimensions::new(
                BlockSizeValue::Length(FlowLength::px(90.0)),
                BlockSizeValue::Auto,
            ),
            max_size: BlockDimensions::new(
                BlockSizeValue::Length(FlowLength::px(60.0)),
                BlockSizeValue::None,
            ),
            ..style
        };
        assert_eq!(
            solve_shrink_to_fit_inline_size(conflicting_constraints, 200.0, intrinsic).border_box,
            100.0
        );
    }

    #[test]
    fn floats_use_the_highest_side_most_available_interval() {
        let mut context = horizontal_context(200.0);
        let left = context.place_float(
            fixed_float(FloatSide::Left, 80.0),
            PhysicalSize {
                width: 80.0,
                height: 40.0,
            },
        );
        let right = context.place_float(
            fixed_float(FloatSide::Right, 70.0),
            PhysicalSize {
                width: 70.0,
                height: 30.0,
            },
        );
        let lowered = context.place_float(
            fixed_float(FloatSide::Left, 100.0),
            PhysicalSize {
                width: 100.0,
                height: 20.0,
            },
        );

        assert_eq!((left.rect.x, left.rect.y), (0.0, 0.0));
        assert_eq!((right.rect.x, right.rect.y), (130.0, 0.0));
        assert_eq!((lowered.rect.x, lowered.rect.y), (80.0, 30.0));
        assert_eq!(
            context.available_inline_space(10.0, 1.0),
            FloatAvailableSpace {
                inline_start: 80.0,
                inline_size: 50.0,
            }
        );
    }

    #[test]
    fn ordinary_descendants_translate_and_return_float_exclusions() {
        let mut parent = horizontal_context(200.0);
        parent.place_float(
            fixed_float(FloatSide::Left, 80.0),
            PhysicalSize {
                width: 80.0,
                height: 40.0,
            },
        );

        let inherited =
            parent.float_state_for_descendant(20.0, 10.0, FlowAxes::HORIZONTAL_LTR, 160.0);
        let mut descendant = BlockFormattingContext::with_float_state(
            BlockContainingBlock {
                flow: FlowAxes::HORIZONTAL_LTR,
                content_box: PhysicalSize {
                    width: 160.0,
                    height: 0.0,
                },
            },
            false,
            inherited,
        );
        assert_eq!(
            descendant.available_inline_space(0.0, 10.0),
            FloatAvailableSpace {
                inline_start: 60.0,
                inline_size: 100.0,
            }
        );

        let nested = descendant.place_float(
            fixed_float(FloatSide::Right, 50.0),
            PhysicalSize {
                width: 50.0,
                height: 20.0,
            },
        );
        assert_eq!((nested.rect.x, nested.rect.y), (110.0, 0.0));

        parent.import_descendant_float_state(
            descendant.exported_float_state(),
            20.0,
            10.0,
            FlowAxes::HORIZONTAL_LTR,
            160.0,
        );
        assert_eq!(parent.float_exclusion_count(), 2);
        assert_eq!(
            parent.available_inline_space(15.0, 1.0),
            FloatAvailableSpace {
                inline_start: 80.0,
                inline_size: 50.0,
            }
        );
    }

    #[test]
    fn direction_flipped_descendants_mirror_and_return_float_exclusions() {
        let rtl = FlowAxes::new(WritingMode::HorizontalTb, Direction::Rtl);
        let mut parent = horizontal_context(200.0);
        parent.place_float(
            fixed_float(FloatSide::Left, 80.0),
            PhysicalSize {
                width: 80.0,
                height: 40.0,
            },
        );

        let inherited = parent.float_state_for_descendant(20.0, 10.0, rtl, 160.0);
        let mut descendant = BlockFormattingContext::with_float_state(
            BlockContainingBlock {
                flow: rtl,
                content_box: PhysicalSize {
                    width: 160.0,
                    height: 0.0,
                },
            },
            false,
            inherited,
        );
        assert_eq!(
            descendant.available_inline_space(0.0, 10.0),
            FloatAvailableSpace {
                inline_start: 0.0,
                inline_size: 100.0,
            }
        );
        assert_eq!(
            descendant
                .float_line_constraints(0.0)
                .expect("mirrored line constraints")
                .horizontal_physical_space(0.0, 10.0),
            FloatAvailableSpace {
                inline_start: 60.0,
                inline_size: 100.0,
            }
        );

        let nested = descendant.place_float(
            fixed_float(FloatSide::Right, 50.0),
            PhysicalSize {
                width: 50.0,
                height: 20.0,
            },
        );
        assert_eq!((nested.rect.x, nested.rect.y), (110.0, 0.0));

        parent.import_descendant_float_state(
            descendant.exported_float_state(),
            20.0,
            10.0,
            rtl,
            160.0,
        );
        assert_eq!(
            parent.available_inline_space(15.0, 1.0),
            FloatAvailableSpace {
                inline_start: 80.0,
                inline_size: 50.0,
            }
        );

        let mut rtl_parent = BlockFormattingContext::new(BlockContainingBlock {
            flow: rtl,
            content_box: PhysicalSize {
                width: 200.0,
                height: 0.0,
            },
        });
        rtl_parent.place_float(
            fixed_float(FloatSide::Right, 80.0),
            PhysicalSize {
                width: 80.0,
                height: 40.0,
            },
        );
        let inherited =
            rtl_parent.float_state_for_descendant(20.0, 10.0, FlowAxes::HORIZONTAL_LTR, 160.0);
        let mut ltr_descendant = BlockFormattingContext::with_float_state(
            BlockContainingBlock {
                flow: FlowAxes::HORIZONTAL_LTR,
                content_box: PhysicalSize {
                    width: 160.0,
                    height: 0.0,
                },
            },
            false,
            inherited,
        );
        assert_eq!(
            ltr_descendant.available_inline_space(0.0, 10.0),
            FloatAvailableSpace {
                inline_start: 0.0,
                inline_size: 100.0,
            }
        );
        ltr_descendant.place_float(
            fixed_float(FloatSide::Left, 50.0),
            PhysicalSize {
                width: 50.0,
                height: 20.0,
            },
        );
        rtl_parent.import_descendant_float_state(
            ltr_descendant.exported_float_state(),
            20.0,
            10.0,
            FlowAxes::HORIZONTAL_LTR,
            160.0,
        );
        assert_eq!(rtl_parent.float_exclusion_count(), 2);
        assert_eq!(
            rtl_parent.float_exclusions[1].margin_box.inline_start,
            130.0
        );

        let mut rounded_parent = horizontal_context(200.0);
        let mut rounded_style = fixed_float(FloatSide::Left, 100.0);
        rounded_style.float_reference_box = FloatReferenceBox::BorderBox;
        rounded_style.has_shape_outside_box = true;
        rounded_style.corner_radii.top_right = BlockCornerRadius {
            horizontal: FlowLength::px(40.0),
            vertical: FlowLength::px(80.0),
        };
        rounded_parent.place_float(
            rounded_style,
            PhysicalSize {
                width: 100.0,
                height: 100.0,
            },
        );
        let rounded_descendant = BlockFormattingContext::with_float_state(
            BlockContainingBlock {
                flow: rtl,
                content_box: PhysicalSize {
                    width: 200.0,
                    height: 0.0,
                },
            },
            false,
            rounded_parent.float_state_for_descendant(0.0, 0.0, rtl, 200.0),
        );
        let rounded_top = rounded_descendant
            .float_line_constraints(0.0)
            .expect("mirrored rounded constraints")
            .horizontal_physical_space(0.0, 0.0);
        assert!((rounded_top.inline_start - 60.0).abs() <= 0.01);
        assert!((rounded_top.inline_size - 140.0).abs() <= 0.01);
    }

    #[test]
    fn translated_float_exclusions_remain_active_above_a_descendant_origin() {
        let mut parent = horizontal_context(200.0);
        parent.place_float(
            fixed_float(FloatSide::Left, 80.0),
            PhysicalSize {
                width: 80.0,
                height: 40.0,
            },
        );

        let mut descendant = BlockFormattingContext::with_float_state(
            BlockContainingBlock {
                flow: FlowAxes::HORIZONTAL_LTR,
                content_box: PhysicalSize {
                    width: 180.0,
                    height: 0.0,
                },
            },
            false,
            parent.float_state_for_descendant(20.0, 30.0, FlowAxes::HORIZONTAL_LTR, 180.0),
        );

        assert_eq!(
            descendant.available_inline_space(0.0, 5.0),
            FloatAvailableSpace {
                inline_start: 60.0,
                inline_size: 120.0,
            }
        );
        let clear = BlockStyle {
            clear: ClearSide::Left,
            ..fixed_width(180.0)
        };
        let placed = descendant.place_in_flow(
            clear,
            PhysicalSize {
                width: 180.0,
                height: 10.0,
            },
        );
        assert_eq!(placed.rect.y, 10.0);
    }

    #[test]
    fn negative_float_margin_box_sizes_do_not_create_exclusions_on_either_side() {
        for (side, clear) in [
            (FloatSide::Left, ClearSide::Left),
            (FloatSide::Right, ClearSide::Right),
        ] {
            let mut context = horizontal_context(200.0);
            let mut float = fixed_float(side, 80.0);
            float.margin.top = FlowLengthAuto::Value(FlowLength::px(-30.0));
            float.margin.bottom = FlowLengthAuto::Value(FlowLength::px(-20.0));
            let placement = context.place_float(
                float,
                PhysicalSize {
                    width: 80.0,
                    height: 40.0,
                },
            );

            assert_eq!(placement.rect.y, -30.0, "side={side:?}");
            assert_eq!(
                context.available_inline_space(0.0, 1.0),
                FloatAvailableSpace {
                    inline_start: 0.0,
                    inline_size: 200.0,
                },
                "side={side:?}"
            );
            assert!(
                context.float_line_constraints(0.0).is_none(),
                "side={side:?}"
            );
            let in_flow = context.place_in_flow(
                BlockStyle {
                    clear,
                    ..fixed_width(200.0)
                },
                PhysicalSize {
                    width: 200.0,
                    height: 10.0,
                },
            );
            assert_eq!(in_flow.rect.y, 0.0, "side={side:?}");
            assert_eq!(context.used_block_size_containing_floats(false), 10.0);
        }
    }

    #[test]
    fn zero_height_float_excludes_only_a_straddling_nonzero_line() {
        let mut context = horizontal_context(200.0);
        context.place_float(
            fixed_float(FloatSide::Left, 80.0),
            PhysicalSize {
                width: 80.0,
                height: 0.0,
            },
        );

        assert_eq!(
            context.available_inline_space(0.0, 40.0),
            FloatAvailableSpace {
                inline_start: 0.0,
                inline_size: 200.0,
            }
        );
        assert_eq!(
            context
                .float_line_constraints(0.0)
                .expect("retained zero-height float")
                .available_space(0.0, 40.0),
            FloatAvailableSpace {
                inline_start: 0.0,
                inline_size: 200.0,
            }
        );
        assert_eq!(
            context.available_inline_space(-10.0, 20.0),
            FloatAvailableSpace {
                inline_start: 80.0,
                inline_size: 120.0,
            }
        );
        assert_eq!(
            context
                .float_line_constraints(0.0)
                .expect("retained zero-height float")
                .available_space(-10.0, 20.0),
            FloatAvailableSpace {
                inline_start: 80.0,
                inline_size: 120.0,
            }
        );
    }

    #[test]
    fn floats_do_not_advance_normal_flow_but_bfc_height_contains_them() {
        let mut context = horizontal_context(200.0);
        context.place_float(
            fixed_float(FloatSide::Left, 80.0),
            PhysicalSize {
                width: 80.0,
                height: 40.0,
            },
        );
        let in_flow = context.place_in_flow(
            fixed_width(200.0),
            PhysicalSize {
                width: 200.0,
                height: 10.0,
            },
        );

        assert_eq!(in_flow.rect.y, 0.0);
        assert_eq!(context.used_block_size(), 10.0);
        assert_eq!(context.used_block_size_containing_floats(false), 40.0);
    }

    #[test]
    fn independent_bfc_narrows_beside_a_float_or_moves_below_when_it_cannot_fit() {
        let margin_state = BlockMarginState {
            block_start: CollapsedMargin::ZERO,
            block_end: CollapsedMargin::ZERO,
            collapses_through: false,
        };

        let mut adjacent_context = horizontal_context(200.0);
        adjacent_context.place_float(
            fixed_float(FloatSide::Left, 80.0),
            PhysicalSize {
                width: 80.0,
                height: 40.0,
            },
        );
        let auto_bfc = BlockStyle {
            establishes_bfc: true,
            ..BlockStyle::default()
        };
        let adjacent = adjacent_context.float_avoiding_placement(auto_bfc, margin_state, 20.0);
        assert_eq!(
            adjacent,
            FloatAvoidingPlacement {
                block_start: 0.0,
                inline_start: 80.0,
                inline_size: UsedInlineSize {
                    margin_start: 0.0,
                    border_box: 120.0,
                    margin_end: 0.0,
                },
            }
        );
        let committed = adjacent_context.place_float_avoiding_in_flow(
            auto_bfc,
            PhysicalSize {
                width: 120.0,
                height: 20.0,
            },
            margin_state,
            adjacent,
        );
        assert_eq!((committed.rect.x, committed.rect.y), (80.0, 0.0));

        let mut lowered_context = horizontal_context(200.0);
        lowered_context.place_float(
            fixed_float(FloatSide::Left, 80.0),
            PhysicalSize {
                width: 80.0,
                height: 40.0,
            },
        );
        let definite_bfc = BlockStyle {
            size: BlockDimensions::new(
                BlockSizeValue::Length(FlowLength::px(150.0)),
                BlockSizeValue::Auto,
            ),
            establishes_bfc: true,
            ..BlockStyle::default()
        };
        let lowered = lowered_context.float_avoiding_placement(definite_bfc, margin_state, 20.0);
        assert_eq!(lowered.block_start, 40.0);
        assert_eq!(lowered.inline_start, 0.0);
        assert_eq!(lowered.inline_size.border_box, 150.0);
    }

    #[test]
    fn bfc_margin_can_force_its_border_box_below_a_float() {
        let margin_state = BlockMarginState {
            block_start: CollapsedMargin::ZERO,
            block_end: CollapsedMargin::ZERO,
            collapses_through: false,
        };
        let mut context = horizontal_context(100.0);
        context.place_float(
            fixed_float(FloatSide::Right, 50.0),
            PhysicalSize {
                width: 50.0,
                height: 40.0,
            },
        );
        let mut bfc = BlockStyle {
            establishes_bfc: true,
            ..BlockStyle::default()
        };
        bfc.margin.left = FlowLengthAuto::Value(FlowLength::px(51.0));

        let placement = context.float_avoiding_placement(bfc, margin_state, 60.0);

        assert_eq!(
            placement,
            FloatAvoidingPlacement {
                block_start: 40.0,
                inline_start: 51.0,
                inline_size: UsedInlineSize {
                    margin_start: 51.0,
                    border_box: 49.0,
                    margin_end: 0.0,
                },
            }
        );
    }

    #[test]
    fn rtl_bfc_with_negative_left_margin_stays_beside_a_left_float() {
        let rtl = FlowAxes::new(WritingMode::HorizontalTb, Direction::Rtl);
        let margin_state = BlockMarginState {
            block_start: CollapsedMargin::ZERO,
            block_end: CollapsedMargin::ZERO,
            collapses_through: false,
        };
        let mut context = BlockFormattingContext::new(BlockContainingBlock {
            flow: rtl,
            content_box: PhysicalSize {
                width: 100.0,
                height: 0.0,
            },
        });
        context.place_float(
            fixed_float(FloatSide::Left, 50.0),
            PhysicalSize {
                width: 50.0,
                height: 100.0,
            },
        );
        let mut bfc = BlockStyle {
            flow: rtl,
            containing_flow: rtl,
            establishes_bfc: true,
            ..BlockStyle::default()
        };
        bfc.margin.left = FlowLengthAuto::Value(FlowLength::px(-20.0));

        let placement = context.float_avoiding_placement(bfc, margin_state, 100.0);

        assert_eq!(
            placement,
            FloatAvoidingPlacement {
                block_start: 0.0,
                inline_start: 0.0,
                inline_size: UsedInlineSize {
                    margin_start: 0.0,
                    border_box: 70.0,
                    margin_end: -20.0,
                },
            }
        );
        let committed = context.place_float_avoiding_in_flow(
            bfc,
            PhysicalSize {
                width: 70.0,
                height: 100.0,
            },
            margin_state,
            placement,
        );
        assert_eq!((committed.rect.x, committed.rect.y), (30.0, 0.0));
    }

    #[test]
    fn ltr_bfc_with_negative_right_margin_stays_beside_a_right_float() {
        let margin_state = BlockMarginState {
            block_start: CollapsedMargin::ZERO,
            block_end: CollapsedMargin::ZERO,
            collapses_through: false,
        };
        let mut context = horizontal_context(100.0);
        context.place_float(
            fixed_float(FloatSide::Right, 50.0),
            PhysicalSize {
                width: 50.0,
                height: 100.0,
            },
        );
        let mut bfc = BlockStyle {
            establishes_bfc: true,
            ..BlockStyle::default()
        };
        bfc.margin.right = FlowLengthAuto::Value(FlowLength::px(-20.0));

        let placement = context.float_avoiding_placement(bfc, margin_state, 100.0);

        assert_eq!(
            placement,
            FloatAvoidingPlacement {
                block_start: 0.0,
                inline_start: 0.0,
                inline_size: UsedInlineSize {
                    margin_start: 0.0,
                    border_box: 70.0,
                    margin_end: -20.0,
                },
            }
        );
        let committed = context.place_float_avoiding_in_flow(
            bfc,
            PhysicalSize {
                width: 70.0,
                height: 100.0,
            },
            margin_state,
            placement,
        );
        assert_eq!((committed.rect.x, committed.rect.y), (0.0, 0.0));
    }

    #[test]
    fn negative_inline_end_margin_does_not_cross_an_inline_start_float() {
        let rtl = FlowAxes::new(WritingMode::HorizontalTb, Direction::Rtl);
        let margin_state = BlockMarginState {
            block_start: CollapsedMargin::ZERO,
            block_end: CollapsedMargin::ZERO,
            collapses_through: false,
        };
        let mut context = BlockFormattingContext::new(BlockContainingBlock {
            flow: rtl,
            content_box: PhysicalSize {
                width: 100.0,
                height: 0.0,
            },
        });
        context.place_float(
            fixed_float(FloatSide::Left, 50.0),
            PhysicalSize {
                width: 50.0,
                height: 100.0,
            },
        );
        context.place_float(
            fixed_float(FloatSide::Right, 20.0),
            PhysicalSize {
                width: 20.0,
                height: 100.0,
            },
        );
        let mut bfc = BlockStyle {
            flow: rtl,
            containing_flow: rtl,
            establishes_bfc: true,
            ..BlockStyle::default()
        };
        bfc.margin.left = FlowLengthAuto::Value(FlowLength::px(-20.0));

        let placement = context.float_avoiding_placement(bfc, margin_state, 100.0);

        assert_eq!(placement.block_start, 100.0);
    }

    #[test]
    fn float_line_constraints_follow_each_line_span_and_reclaim_the_column() {
        let mut context = horizontal_context(200.0);
        context.place_float(
            fixed_float(FloatSide::Left, 80.0),
            PhysicalSize {
                width: 80.0,
                height: 40.0,
            },
        );
        context.place_float(
            fixed_float(FloatSide::Right, 60.0),
            PhysicalSize {
                width: 60.0,
                height: 20.0,
            },
        );
        let constraints = context.float_line_constraints(0.0).expect("floats");

        assert_eq!(
            constraints.available_space(0.0, 18.0),
            FloatAvailableSpace {
                inline_start: 80.0,
                inline_size: 60.0,
            }
        );
        assert_eq!(
            constraints.available_space(24.0, 12.0),
            FloatAvailableSpace {
                inline_start: 80.0,
                inline_size: 120.0,
            }
        );
        assert_eq!(
            constraints.available_space(40.0, 18.0),
            FloatAvailableSpace {
                inline_start: 0.0,
                inline_size: 200.0,
            }
        );
        assert_eq!(
            constraints.next_wider_block_start(0.0, 18.0, 100.0),
            Some(20.0)
        );
        assert_eq!(
            constraints.next_wider_block_start(0.0, 18.0, 300.0),
            Some(40.0)
        );
    }

    #[test]
    fn rectangular_shape_boxes_change_only_line_exclusions() {
        let cases = [
            (FloatReferenceBox::MarginBox, 100.0),
            (FloatReferenceBox::BorderBox, 90.0),
            (FloatReferenceBox::PaddingBox, 85.0),
            (FloatReferenceBox::ContentBox, 75.0),
        ];

        for (reference_box, expected_line_start) in cases {
            let mut context = horizontal_context(200.0);
            let mut style = fixed_float(FloatSide::Left, 80.0);
            style.float_reference_box = reference_box;
            style.has_shape_outside_box = true;
            style.margin = PhysicalSides::splat(FlowLengthAuto::Value(FlowLength::px(10.0)));
            style.padding = PhysicalSides::splat(FlowLength::px(10.0));
            style.border = PhysicalSides::splat(5.0);

            let placement = context.place_float(
                style,
                PhysicalSize {
                    width: 80.0,
                    height: 80.0,
                },
            );
            assert_eq!((placement.rect.x, placement.rect.y), (10.0, 10.0));
            assert_eq!(
                context.available_inline_space(30.0, 10.0),
                FloatAvailableSpace {
                    inline_start: 100.0,
                    inline_size: 100.0,
                },
                "placement geometry must remain the margin box for {reference_box:?}"
            );
            let constraints = context.float_line_constraints(0.0).expect("float area");
            assert_eq!(
                constraints.available_space(30.0, 10.0),
                FloatAvailableSpace {
                    inline_start: expected_line_start,
                    inline_size: 200.0 - expected_line_start,
                },
                "line geometry for {reference_box:?}"
            );
            assert_eq!(context.used_block_size_containing_floats(false), 100.0);

            let cleared = context.place_in_flow(
                BlockStyle {
                    clear: ClearSide::Left,
                    ..fixed_width(200.0)
                },
                PhysicalSize {
                    width: 200.0,
                    height: 10.0,
                },
            );
            assert_eq!(cleared.rect.y, 100.0);
        }
    }

    fn circular_radii(radius: f32) -> BlockCornerRadii {
        let corner = BlockCornerRadius {
            horizontal: FlowLength::px(radius),
            vertical: FlowLength::px(radius),
        };
        BlockCornerRadii {
            top_left: corner,
            top_right: corner,
            bottom_right: corner,
            bottom_left: corner,
        }
    }

    #[test]
    fn rounded_shape_boxes_use_conservative_full_line_span_intervals_on_both_sides() {
        let mut left = horizontal_context(200.0);
        let mut left_style = fixed_float(FloatSide::Left, 100.0);
        left_style.float_reference_box = FloatReferenceBox::BorderBox;
        left_style.has_shape_outside_box = true;
        left_style.corner_radii = circular_radii(50.0);
        left.place_float(
            left_style,
            PhysicalSize {
                width: 100.0,
                height: 100.0,
            },
        );
        let left_constraints = left
            .float_line_constraints(0.0)
            .expect("left rounded float");
        let top = left_constraints.available_space(0.0, 0.0);
        let top_span = left_constraints.available_space(0.0, 25.0);
        let middle = left_constraints.available_space(50.0, 10.0);
        let bottom = left_constraints.available_space(75.0, 25.0);
        assert!((top.inline_start - 50.0).abs() <= 0.01, "top={top:?}");
        assert!(
            (top_span.inline_start - 93.30127).abs() <= 0.01,
            "top_span={top_span:?}"
        );
        assert!(
            (middle.inline_start - 100.0).abs() <= 0.01,
            "middle={middle:?}"
        );
        assert!(
            (bottom.inline_start - 93.30127).abs() <= 0.01,
            "bottom={bottom:?}"
        );

        let mut right = horizontal_context(200.0);
        let mut right_style = fixed_float(FloatSide::Right, 100.0);
        right_style.float_reference_box = FloatReferenceBox::BorderBox;
        right_style.has_shape_outside_box = true;
        right_style.corner_radii = circular_radii(50.0);
        right.place_float(
            right_style,
            PhysicalSize {
                width: 100.0,
                height: 100.0,
            },
        );
        let right_constraints = right
            .float_line_constraints(0.0)
            .expect("right rounded float");
        let right_top = right_constraints.available_space(0.0, 0.0);
        let right_middle = right_constraints.available_space(50.0, 10.0);
        assert!(
            (right_top.inline_size - 150.0).abs() <= 0.01,
            "top={right_top:?}"
        );
        assert!(
            (right_middle.inline_size - 100.0).abs() <= 0.01,
            "middle={right_middle:?}"
        );
    }

    #[test]
    fn asymmetric_opposite_corner_bands_shape_each_inline_edge_independently() {
        let rect = FloatMarginBox {
            inline_start: 0.0,
            inline_size: 100.0,
            block: SignedBlockExtent {
                start: 0.0,
                used_size: 100.0,
            },
        };
        let radii = LogicalCornerRadii {
            block_start_inline_start: LogicalCornerRadius {
                inline: 40.0,
                block: 80.0,
            },
            block_end_inline_end: LogicalCornerRadius {
                inline: 30.0,
                block: 80.0,
            },
            ..LogicalCornerRadii::default()
        };

        let (inline_start, inline_end) = rounded_inline_bounds_at(rect, radii, 50.0);
        assert!(
            (inline_start - 2.919).abs() <= 0.01,
            "inline_start={inline_start}"
        );
        assert!(
            (inline_end - 97.811).abs() <= 0.01,
            "inline_end={inline_end}"
        );
    }

    #[test]
    fn rounded_retry_finds_the_first_lower_contour_band_that_fits() {
        let mut context = horizontal_context(200.0);
        let mut style = fixed_float(FloatSide::Left, 100.0);
        style.float_reference_box = FloatReferenceBox::BorderBox;
        style.has_shape_outside_box = true;
        style.corner_radii = BlockCornerRadii {
            bottom_right: BlockCornerRadius {
                horizontal: FlowLength::px(50.0),
                vertical: FlowLength::px(50.0),
            },
            bottom_left: BlockCornerRadius {
                horizontal: FlowLength::px(50.0),
                vertical: FlowLength::px(50.0),
            },
            ..BlockCornerRadii::default()
        };
        context.place_float(
            style,
            PhysicalSize {
                width: 100.0,
                height: 100.0,
            },
        );
        let constraints = context.float_line_constraints(0.0).expect("rounded float");

        assert_eq!(constraints.available_space(50.0, 10.0).inline_size, 100.0);
        let retry = constraints
            .next_wider_block_start(50.0, 10.0, 120.0)
            .expect("lower contour band");
        assert!((retry - 90.0).abs() <= 0.02, "retry={retry}");
        assert!(constraints.available_space(retry, 10.0).inline_size + 0.01 >= 120.0);
        assert!(constraints.available_space(retry - 0.05, 10.0).inline_size + 0.01 < 120.0);
    }

    #[test]
    fn rounded_retry_finds_a_fitting_window_before_a_lower_rectangle() {
        let rounded_box = FloatMarginBox {
            inline_start: 0.0,
            inline_size: 100.0,
            block: SignedBlockExtent {
                start: 0.0,
                used_size: 100.0,
            },
        };
        let lower_rectangle = FloatMarginBox {
            inline_start: 150.0,
            inline_size: 50.0,
            block: SignedBlockExtent {
                start: 102.0,
                used_size: 48.0,
            },
        };
        let bottom_corner = LogicalCornerRadius {
            inline: 50.0,
            block: 50.0,
        };
        let constraints = FloatLineConstraints {
            flow: FlowAxes::HORIZONTAL_LTR,
            containing_inline_size: 200.0,
            block_offset: 0.0,
            exclusions: vec![
                FloatExclusion {
                    side: FloatSide::Left,
                    at_inline_start: true,
                    margin_box: rounded_box,
                    line_area: FloatLineArea::Rounded {
                        rect: rounded_box,
                        clip: rounded_box,
                        radii: LogicalCornerRadii {
                            block_end_inline_start: bottom_corner,
                            block_end_inline_end: bottom_corner,
                            ..LogicalCornerRadii::default()
                        },
                    },
                },
                FloatExclusion {
                    side: FloatSide::Right,
                    at_inline_start: false,
                    margin_box: lower_rectangle,
                    line_area: FloatLineArea::Rect(lower_rectangle),
                },
            ],
        };

        let retry = constraints
            .next_wider_block_start(50.0, 10.0, 120.0)
            .expect("fitting window before the lower rectangle");
        assert!((retry - 90.0).abs() <= 0.02, "retry={retry}");
        assert!(retry < lower_rectangle.block.start - 10.0);
    }

    #[test]
    fn reference_box_radii_expand_and_contract_without_changing_margin_authority() {
        let border_box = LogicalRect {
            inline_start: 10.0,
            block_start: 10.0,
            inline_size: 100.0,
            block_size: 100.0,
        };
        let mut style = fixed_float(FloatSide::Left, 100.0);
        style.has_shape_outside_box = true;
        style.corner_radii = circular_radii(40.0);
        let reference = |inline_start, inline_size, block_start, block_size| FloatMarginBox {
            inline_start,
            inline_size,
            block: SignedBlockExtent {
                start: block_start,
                used_size: block_size,
            },
        };
        let margin = reference_box_radii(style, border_box, reference(0.0, 120.0, 0.0, 120.0));
        let padding = reference_box_radii(style, border_box, reference(20.0, 80.0, 20.0, 80.0));
        let content = reference_box_radii(style, border_box, reference(40.0, 40.0, 40.0, 40.0));
        let mut softened_style = style;
        softened_style.corner_radii = circular_radii(10.0);
        let softened_margin = reference_box_radii(
            softened_style,
            border_box,
            reference(-10.0, 140.0, -10.0, 140.0),
        );
        assert_eq!(
            margin.block_start_inline_start,
            LogicalCornerRadius {
                inline: 50.0,
                block: 50.0
            }
        );
        assert_eq!(
            padding.block_start_inline_start,
            LogicalCornerRadius {
                inline: 30.0,
                block: 30.0
            }
        );
        assert_eq!(
            content.block_start_inline_start,
            LogicalCornerRadius {
                inline: 10.0,
                block: 10.0
            }
        );
        assert_eq!(
            softened_margin.block_start_inline_start,
            LogicalCornerRadius {
                inline: 27.5,
                block: 27.5
            },
            "a positive margin larger than the radius uses the CSS Shapes cubic adjustment"
        );
        assert_eq!(margin_box_corner_radius(0.0, 20.0), 0.0);
        assert_eq!(margin_box_corner_radius(40.0, -10.0), 30.0);

        let mut context = horizontal_context(140.0);
        style.float_reference_box = FloatReferenceBox::ContentBox;
        style.margin = PhysicalSides::splat(FlowLengthAuto::Value(FlowLength::px(10.0)));
        style.padding = PhysicalSides::splat(FlowLength::px(10.0));
        style.border = PhysicalSides::splat(5.0);
        context.place_float(
            style,
            PhysicalSize {
                width: 100.0,
                height: 100.0,
            },
        );
        assert_eq!(
            context.available_inline_space(30.0, 10.0),
            FloatAvailableSpace {
                inline_start: 120.0,
                inline_size: 20.0,
            },
            "float placement continues to consult the margin box"
        );
        assert_eq!(
            context.used_block_size_containing_floats(false),
            120.0,
            "float containment continues to consult the margin box"
        );
    }

    #[test]
    fn rounded_reference_box_is_shaped_before_negative_margin_clipping() {
        let margin_box = FloatMarginBox {
            inline_start: 25.0,
            inline_size: 50.0,
            block: SignedBlockExtent {
                start: 0.0,
                used_size: 100.0,
            },
        };
        let border_box = LogicalRect {
            inline_start: 0.0,
            block_start: 0.0,
            inline_size: 100.0,
            block_size: 100.0,
        };
        let style = BlockStyle {
            float_reference_box: FloatReferenceBox::BorderBox,
            has_shape_outside_box: true,
            corner_radii: circular_radii(50.0),
            ..BlockStyle::default()
        };
        let area = float_line_area(style, 100.0, margin_box, border_box);

        assert_eq!(area.bounds(), margin_box);
        let (inline_start, inline_end) = area
            .inline_bounds_for_span(10.0, 0.0)
            .expect("clipped rounded contour");
        assert!((inline_start - 25.0).abs() <= 0.01);
        assert!((inline_end - 75.0).abs() <= 0.01);
    }

    #[test]
    fn descendant_float_state_translates_margin_and_line_areas_together() {
        let mut parent = horizontal_context(200.0);
        let mut style = fixed_float(FloatSide::Left, 80.0);
        style.float_reference_box = FloatReferenceBox::ContentBox;
        style.has_shape_outside_box = true;
        style.margin = PhysicalSides::splat(FlowLengthAuto::Value(FlowLength::px(10.0)));
        style.padding = PhysicalSides::splat(FlowLength::px(10.0));
        style.border = PhysicalSides::splat(5.0);
        parent.place_float(
            style,
            PhysicalSize {
                width: 80.0,
                height: 80.0,
            },
        );

        let descendant = BlockFormattingContext::with_float_state(
            BlockContainingBlock {
                flow: FlowAxes::HORIZONTAL_LTR,
                content_box: PhysicalSize {
                    width: 180.0,
                    height: 0.0,
                },
            },
            false,
            parent.float_state_for_descendant(20.0, 10.0, FlowAxes::HORIZONTAL_LTR, 180.0),
        );
        assert_eq!(
            descendant.available_inline_space(20.0, 10.0),
            FloatAvailableSpace {
                inline_start: 80.0,
                inline_size: 100.0,
            }
        );
        assert_eq!(
            descendant
                .float_line_constraints(0.0)
                .expect("translated float area")
                .available_space(20.0, 10.0),
            FloatAvailableSpace {
                inline_start: 55.0,
                inline_size: 125.0,
            }
        );
    }

    #[test]
    fn right_float_reference_box_uses_its_line_areas_start_edge() {
        let mut context = horizontal_context(200.0);
        let mut style = fixed_float(FloatSide::Right, 80.0);
        style.float_reference_box = FloatReferenceBox::ContentBox;
        style.has_shape_outside_box = true;
        style.margin = PhysicalSides::splat(FlowLengthAuto::Value(FlowLength::px(10.0)));
        style.padding = PhysicalSides::splat(FlowLength::px(10.0));
        style.border = PhysicalSides::splat(5.0);
        let placement = context.place_float(
            style,
            PhysicalSize {
                width: 80.0,
                height: 80.0,
            },
        );

        assert_eq!((placement.rect.x, placement.rect.y), (110.0, 10.0));
        assert_eq!(
            context.available_inline_space(30.0, 10.0),
            FloatAvailableSpace {
                inline_start: 0.0,
                inline_size: 100.0,
            }
        );
        assert_eq!(
            context
                .float_line_constraints(0.0)
                .expect("right float area")
                .available_space(30.0, 10.0),
            FloatAvailableSpace {
                inline_start: 0.0,
                inline_size: 125.0,
            }
        );
    }

    #[test]
    fn vertical_reference_boxes_retain_the_margin_box_line_area() {
        let margin_box = FloatMarginBox {
            inline_start: 0.0,
            inline_size: 100.0,
            block: SignedBlockExtent {
                start: 0.0,
                used_size: 100.0,
            },
        };
        let border_box = LogicalRect {
            inline_start: 10.0,
            block_start: 10.0,
            inline_size: 80.0,
            block_size: 80.0,
        };
        let style = BlockStyle {
            containing_flow: FlowAxes::new(WritingMode::VerticalRl, Direction::Ltr),
            float_reference_box: FloatReferenceBox::ContentBox,
            has_shape_outside_box: true,
            padding: PhysicalSides::splat(FlowLength::px(10.0)),
            border: PhysicalSides::splat(5.0),
            ..BlockStyle::default()
        };

        assert_eq!(
            float_line_area(style, 200.0, margin_box, border_box),
            FloatLineArea::Rect(margin_box)
        );
    }

    #[test]
    fn clearance_uses_only_the_requested_physical_float_sides() {
        let build = || {
            let mut context = horizontal_context(200.0);
            context.place_float(
                fixed_float(FloatSide::Left, 80.0),
                PhysicalSize {
                    width: 80.0,
                    height: 40.0,
                },
            );
            context.place_float(
                fixed_float(FloatSide::Right, 70.0),
                PhysicalSize {
                    width: 70.0,
                    height: 70.0,
                },
            );
            context
        };
        let clear = |side| BlockStyle {
            clear: side,
            ..fixed_width(200.0)
        };
        let size = PhysicalSize {
            width: 200.0,
            height: 10.0,
        };

        assert_eq!(
            build().place_in_flow(clear(ClearSide::Left), size).rect.y,
            40.0
        );
        assert_eq!(
            build().place_in_flow(clear(ClearSide::Right), size).rect.y,
            70.0
        );
        assert_eq!(
            build().place_in_flow(clear(ClearSide::Both), size).rect.y,
            70.0
        );
    }

    #[test]
    fn clearance_through_an_empty_box_starts_a_new_following_margin_chain() {
        let mut context = horizontal_context(200.0);
        context.place_float(
            fixed_float(FloatSide::Left, 80.0),
            PhysicalSize {
                width: 80.0,
                height: 40.0,
            },
        );
        let clear = BlockStyle {
            clear: ClearSide::Left,
            ..fixed_width(200.0)
        };
        let empty = context.place_in_flow_with_margins(
            clear,
            PhysicalSize {
                width: 200.0,
                height: 0.0,
            },
            margin_state(10.0, 20.0, true),
        );
        let following = context.place_in_flow_with_margins(
            fixed_width(200.0),
            PhysicalSize {
                width: 200.0,
                height: 10.0,
            },
            margin_state(30.0, 0.0, false),
        );

        assert_eq!(empty.rect.y, 40.0);
        assert_eq!(following.rect.y, 70.0);
        assert_eq!(context.used_block_size(), 80.0);
        assert!(context.active_margin_may_collapse_with_parent_end());

        let mut trailing_empty = horizontal_context(200.0);
        trailing_empty.place_float(
            fixed_float(FloatSide::Left, 80.0),
            PhysicalSize {
                width: 80.0,
                height: 40.0,
            },
        );
        trailing_empty.place_in_flow_with_margins(
            clear,
            PhysicalSize {
                width: 200.0,
                height: 0.0,
            },
            margin_state(10.0, 20.0, true),
        );

        assert_eq!(trailing_empty.used_block_size(), 60.0);
        assert!(!trailing_empty.active_margin_may_collapse_with_parent_end());
    }

    #[test]
    fn relative_offsets_follow_logical_start_sides() {
        let mut horizontal = BlockStyle {
            position: BlockPosition::Relative,
            ..BlockStyle::default()
        };
        horizontal.inset.left = FlowLengthAuto::Value(FlowLength::px(12.0));
        horizontal.inset.bottom = FlowLengthAuto::Value(FlowLength::px(7.0));
        assert_eq!(
            horizontal.relative_offset(200.0, None),
            LogicalOffset {
                inline: 12.0,
                block: -7.0,
            }
        );

        let mut vertical = horizontal;
        vertical.containing_flow =
            FlowAxes::new(crate::WritingMode::VerticalRl, crate::Direction::Ltr);
        vertical.inset = PhysicalSides::splat(FlowLengthAuto::Auto);
        vertical.inset.top = FlowLengthAuto::Value(FlowLength::px(9.0));
        vertical.inset.right = FlowLengthAuto::Value(FlowLength::px(5.0));
        assert_eq!(
            vertical.relative_offset(200.0, None),
            LogicalOffset {
                inline: 9.0,
                block: 5.0,
            }
        );
    }

    #[test]
    fn relative_block_percentages_need_a_definite_containing_block_size() {
        let mut style = BlockStyle {
            position: BlockPosition::Relative,
            ..BlockStyle::default()
        };
        style.inset.left = FlowLengthAuto::Value(FlowLength {
            px: 0.0,
            percentage: 0.25,
        });
        style.inset.top = FlowLengthAuto::Value(FlowLength {
            px: 10.0,
            percentage: -100.0,
        });

        // An indefinite block basis turns the percentage block inset into
        // `auto`; the inline percentage still resolves against the width.
        assert_eq!(
            style.relative_offset(200.0, None),
            LogicalOffset {
                inline: 50.0,
                block: 0.0,
            }
        );
        assert_eq!(
            style.relative_offset(200.0, Some(100.0)),
            LogicalOffset {
                inline: 50.0,
                block: -9990.0,
            }
        );

        // With `top` auto, `bottom` supplies the reversed offset under the
        // same basis rule.
        style.inset.top = FlowLengthAuto::Auto;
        style.inset.bottom = FlowLengthAuto::Value(FlowLength {
            px: 0.0,
            percentage: 0.5,
        });
        assert_eq!(style.relative_offset(200.0, None).block, 0.0);
        assert_eq!(style.relative_offset(200.0, Some(40.0)).block, -20.0);
    }
}
