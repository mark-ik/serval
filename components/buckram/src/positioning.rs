//! CSS positioned-layout used geometry.
//!
//! This module consumes Buckram style inputs, a selected containing block,
//! and the K5b static rectangle. It deliberately has no dependency on the
//! scratch layout adapter: browser positioning is not a backend-node query.

use crate::{
    BlockBoxSizing, BlockDimensions, BlockSizeValue, BlockStyle, LogicalRect, LogicalSize,
    LogicalSides,
};

/// Inputs that remain after a formatting context has supplied a static
/// rectangle and the positioned box has measured its own contents.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PositionedBoxInput {
    /// The selected K5a containing block in the positioned box's logical
    /// coordinate system.
    pub containing_size: LogicalSize,
    /// The K5b static rectangle, expressed in that same coordinate system.
    pub static_rect: LogicalRect,
    /// The measured border-box fallback for automatic dimensions.
    pub measured_size: LogicalSize,
}

/// The resolved border-box rectangle and margins for a positioned box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PositionedBoxGeometry {
    pub logical_rect: LogicalRect,
    pub margin: LogicalSides<f32>,
}

/// Resolve the implemented non-replaced absolute/fixed block box subset.
///
/// The caller selects the absolute or fixed containing block before calling
/// this function. Auto dimensions use the supplied measured border box; the
/// later K5d shrink-to-fit and replaced branches replace that fallback with
/// their own contributions without changing the inset equation.
pub fn solve_positioned_box(style: BlockStyle, input: PositionedBoxInput) -> PositionedBoxGeometry {
    let containing_inline = input.containing_size.inline;
    let insets = style.containing_flow.logical_sides(
        style
            .inset
            .map(|value| value.resolve(containing_inline)),
    );
    let margins = style.logical_margin(containing_inline);
    let padding_border = style.logical_padding_border(containing_inline);
    let dimensions = logical_dimensions(style.containing_flow, style.size);
    let minimums = logical_dimensions(style.containing_flow, style.min_size);
    let maximums = logical_dimensions(style.containing_flow, style.max_size);

    let inline = solve_axis(
        input.containing_size.inline,
        input.static_rect.inline_start,
        input.measured_size.inline,
        insets.inline_start,
        insets.inline_end,
        margins.inline_start,
        margins.inline_end,
        dimensions.inline,
        minimums.inline,
        maximums.inline,
        padding_border.inline_start + padding_border.inline_end,
        style.box_sizing,
    );
    let block = solve_axis(
        input.containing_size.block,
        input.static_rect.block_start,
        input.measured_size.block,
        insets.block_start,
        insets.block_end,
        margins.block_start,
        margins.block_end,
        dimensions.block,
        minimums.block,
        maximums.block,
        padding_border.block_start + padding_border.block_end,
        style.box_sizing,
    );

    PositionedBoxGeometry {
        logical_rect: LogicalRect {
            inline_start: inline.start + inline.margin_start,
            block_start: block.start + block.margin_start,
            inline_size: inline.size,
            block_size: block.size,
        },
        margin: LogicalSides {
            inline_start: inline.margin_start,
            inline_end: inline.margin_end,
            block_start: block.margin_start,
            block_end: block.margin_end,
        },
    }
}

#[derive(Clone, Copy)]
struct AxisGeometry {
    start: f32,
    size: f32,
    margin_start: f32,
    margin_end: f32,
}

#[expect(
    clippy::too_many_arguments,
    reason = "one CSS inset equation has the two insets, two margins, three sizes, and box-sizing inputs"
)]
fn solve_axis(
    containing: f32,
    static_start: f32,
    measured_size: f32,
    inset_start: Option<f32>,
    inset_end: Option<f32>,
    margin_start: Option<f32>,
    margin_end: Option<f32>,
    preferred: BlockSizeValue,
    minimum: BlockSizeValue,
    maximum: BlockSizeValue,
    padding_border: f32,
    box_sizing: BlockBoxSizing,
) -> AxisGeometry {
    let mut size = specified_border_box(preferred, containing, padding_border, box_sizing)
        .unwrap_or(measured_size.max(padding_border));
    size = clamp_border_box(size, minimum, maximum, containing, padding_border, box_sizing);

    let start_auto = margin_start.is_none();
    let end_auto = margin_end.is_none();
    let mut margin_start = margin_start.unwrap_or(0.0);
    let mut margin_end = margin_end.unwrap_or(0.0);
    let resolved_start = inset_start;
    let resolved_end = inset_end;

    let remaining = containing
        - resolved_start.unwrap_or(0.0)
        - resolved_end.unwrap_or(0.0)
        - size
        - margin_start
        - margin_end;
    // Auto margins are zero while the width itself is auto, then consume
    // positive remaining space once the used border box is known.
    if remaining > 0.0 && !matches!(preferred, BlockSizeValue::Auto) {
        match (start_auto, end_auto) {
            (true, true) => {
                margin_start = remaining / 2.0;
                margin_end = remaining / 2.0;
            },
            (true, false) => margin_start = remaining,
            (false, true) => margin_end = remaining,
            (false, false) => {},
        }
    }

    let start = match (resolved_start, resolved_end) {
        (Some(start), _) => start,
        (None, Some(end)) => containing - end - margin_start - size - margin_end,
        (None, None) => static_start - margin_start,
    };
    AxisGeometry {
        start,
        size,
        margin_start,
        margin_end,
    }
}

fn specified_border_box(
    value: BlockSizeValue,
    containing: f32,
    padding_border: f32,
    box_sizing: BlockBoxSizing,
) -> Option<f32> {
    value
        .resolve_definite(Some(containing))
        .map(|value| match box_sizing {
            BlockBoxSizing::ContentBox => value + padding_border,
            BlockBoxSizing::BorderBox => value,
        })
}

fn clamp_border_box(
    size: f32,
    minimum: BlockSizeValue,
    maximum: BlockSizeValue,
    containing: f32,
    padding_border: f32,
    box_sizing: BlockBoxSizing,
) -> f32 {
    let minimum = specified_border_box(minimum, containing, padding_border, box_sizing)
        .unwrap_or(padding_border);
    let maximum = specified_border_box(maximum, containing, padding_border, box_sizing);
    maximum.map_or(size.max(minimum), |maximum| size.max(minimum).min(maximum.max(minimum)))
}

fn logical_dimensions<T: Copy>(flow: crate::FlowAxes, dimensions: BlockDimensions<T>) -> LogicalSizeOf<T> {
    if flow.is_horizontal() {
        LogicalSizeOf {
            inline: dimensions.width,
            block: dimensions.height,
        }
    } else {
        LogicalSizeOf {
            inline: dimensions.height,
            block: dimensions.width,
        }
    }
}

#[derive(Clone, Copy)]
struct LogicalSizeOf<T> {
    inline: T,
    block: T,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockPosition, FlowLength, FlowLengthAuto};

    fn input() -> PositionedBoxInput {
        PositionedBoxInput {
            containing_size: LogicalSize {
                inline: 200.0,
                block: 100.0,
            },
            static_rect: LogicalRect {
                inline_start: 24.0,
                block_start: 16.0,
                inline_size: 0.0,
                block_size: 0.0,
            },
            measured_size: LogicalSize {
                inline: 30.0,
                block: 20.0,
            },
        }
    }

    fn positioned() -> BlockStyle {
        BlockStyle {
            position: BlockPosition::Absolute,
            ..BlockStyle::default()
        }
    }

    #[test]
    fn explicit_insets_place_the_measured_border_box() {
        let mut style = positioned();
        style.inset.left = FlowLengthAuto::Value(FlowLength::px(40.0));
        style.inset.top = FlowLengthAuto::Value(FlowLength::px(12.0));

        assert_eq!(
            solve_positioned_box(style, input()).logical_rect,
            LogicalRect {
                inline_start: 40.0,
                block_start: 12.0,
                inline_size: 30.0,
                block_size: 20.0,
            }
        );
    }

    #[test]
    fn auto_insets_keep_the_static_position() {
        assert_eq!(
            solve_positioned_box(positioned(), input()).logical_rect,
            LogicalRect {
                inline_start: 24.0,
                block_start: 16.0,
                inline_size: 30.0,
                block_size: 20.0,
            }
        );
    }

    #[test]
    fn end_insets_solve_against_the_selected_containing_block() {
        let mut style = positioned();
        style.inset.right = FlowLengthAuto::Value(FlowLength::px(20.0));
        style.inset.bottom = FlowLengthAuto::Value(FlowLength::px(8.0));

        assert_eq!(
            solve_positioned_box(style, input()).logical_rect,
            LogicalRect {
                inline_start: 150.0,
                block_start: 72.0,
                inline_size: 30.0,
                block_size: 20.0,
            }
        );
    }

    #[test]
    fn definite_size_distributes_automatic_margins_after_insets() {
        let mut style = positioned();
        style.size.width = BlockSizeValue::Length(FlowLength::px(100.0));
        style.inset.left = FlowLengthAuto::Value(FlowLength::ZERO);
        style.inset.right = FlowLengthAuto::Value(FlowLength::ZERO);
        style.margin.left = FlowLengthAuto::Auto;
        style.margin.right = FlowLengthAuto::Auto;

        let geometry = solve_positioned_box(style, input());
        assert_eq!(geometry.logical_rect.inline_start, 50.0);
        assert_eq!(geometry.logical_rect.inline_size, 100.0);
        assert_eq!(geometry.margin.inline_start, 50.0);
        assert_eq!(geometry.margin.inline_end, 50.0);
    }

    #[test]
    fn automatic_size_keeps_automatic_margins_at_zero_for_static_fallback() {
        let mut style = positioned();
        style.margin.left = FlowLengthAuto::Auto;
        style.margin.right = FlowLengthAuto::Auto;

        let geometry = solve_positioned_box(style, input());
        assert_eq!(geometry.logical_rect.inline_start, 24.0);
        assert_eq!(geometry.margin.inline_start, 0.0);
        assert_eq!(geometry.margin.inline_end, 0.0);
    }
}
