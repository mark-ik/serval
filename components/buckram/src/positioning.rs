//! CSS positioned-layout used geometry.
//!
//! This module consumes Buckram style inputs, a selected containing block,
//! and the K5b static rectangle. It deliberately has no dependency on the
//! scratch layout adapter: browser positioning is not a backend-node query.

use crate::{
    BlockBoxSizing, BlockDimensions, BlockSizeValue, BlockStyle, IntrinsicSizes, LogicalRect,
    LogicalSides, LogicalSize,
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
    /// The positioned box's admitted content-based inline contributions.
    ///
    /// A caller supplies this only when it has a formatting-context query
    /// rather than a completed normal-flow rectangle. The measured fallback
    /// stays available for unsupported descendants and the block axis.
    pub intrinsic_inline: Option<IntrinsicSizes>,
    /// Replaced-content contributions for the positioned leaf subset.
    ///
    /// The physical element metadata has already been converted into the
    /// positioned box's logical axes. Missing intrinsic metadata is valid:
    /// a definite CSS dimension or `aspect-ratio` may still determine one
    /// axis, while the other continues to use the measured fallback.
    pub replaced: Option<ReplacedSize>,
}

/// Intrinsic replaced-content inputs kept separate from CSS style.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReplacedSize {
    /// The intrinsic content-box size when the replaced source supplied one.
    pub intrinsic_size: Option<LogicalSize>,
}

/// The resolved border-box rectangle and margins for a positioned box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PositionedBoxGeometry {
    pub logical_rect: LogicalRect,
    pub margin: LogicalSides<f32>,
    /// A CSS rule, rather than the formatter's measured fallback, determined
    /// the used block size and the formatter must receive it before reflow.
    pub block_size_solved: bool,
}

/// Resolve the implemented absolute/fixed block box subset.
///
/// The caller selects the absolute or fixed containing block before calling
/// this function. Automatic dimensions use admitted intrinsic or replaced
/// contributions where available, with the formatter's measured border box
/// retained as the unsupported fallback.
pub fn solve_positioned_box(style: BlockStyle, input: PositionedBoxInput) -> PositionedBoxGeometry {
    let containing_inline = input.containing_size.inline;
    let insets = style
        .containing_flow
        .logical_sides(style.inset.map(|value| value.resolve(containing_inline)));
    let margins = style.logical_margin(containing_inline);
    let padding_border = style.logical_padding_border(containing_inline);
    let dimensions = logical_dimensions(style.containing_flow, style.size);
    let minimums = logical_dimensions(style.containing_flow, style.min_size);
    let maximums = logical_dimensions(style.containing_flow, style.max_size);
    let replaced = input
        .replaced
        .map(|replaced| resolve_replaced_size(style, replaced, input.containing_size))
        .unwrap_or_default();

    let inline_padding = padding_border.inline_start + padding_border.inline_end;
    let block_padding = padding_border.block_start + padding_border.block_end;
    let mut inline = solve_axis(
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
        inline_padding,
        style.box_sizing,
        input.intrinsic_inline,
        replaced.inline,
        None,
    );
    let mut block = solve_axis(
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
        block_padding,
        style.box_sizing,
        None,
        replaced.block,
        None,
    );
    let mut block_size_solved = false;

    if let Some(ratio) = logical_aspect_ratio(style).filter(|_| !style.replaced) {
        let inline_auto = matches!(dimensions.inline, BlockSizeValue::Auto);
        let block_auto = matches!(dimensions.block, BlockSizeValue::Auto);
        if block_auto {
            let ratio_block = transfer_border_box_size(
                inline.size,
                inline_padding,
                block_padding,
                style.box_sizing,
                ratio.recip(),
            );
            let constrained_block = clamp_border_box(
                ratio_block,
                minimums.block,
                maximums.block,
                input.containing_size.block,
                block_padding,
                style.box_sizing,
            );
            if inline_auto && (constrained_block - ratio_block).abs() > 0.01 {
                let transferred_inline = transfer_border_box_size(
                    constrained_block,
                    block_padding,
                    inline_padding,
                    style.box_sizing,
                    ratio,
                );
                inline = solve_axis(
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
                    inline_padding,
                    style.box_sizing,
                    input.intrinsic_inline,
                    replaced.inline,
                    Some(transferred_inline),
                );
            }
            let ratio_block = transfer_border_box_size(
                inline.size,
                inline_padding,
                block_padding,
                style.box_sizing,
                ratio.recip(),
            );
            block = solve_axis(
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
                block_padding,
                style.box_sizing,
                None,
                replaced.block,
                Some(ratio_block),
            );
            block_size_solved = true;
        } else if inline_auto {
            let ratio_inline = transfer_border_box_size(
                block.size,
                block_padding,
                inline_padding,
                style.box_sizing,
                ratio,
            );
            inline = solve_axis(
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
                inline_padding,
                style.box_sizing,
                input.intrinsic_inline,
                replaced.inline,
                Some(ratio_inline),
            );
        }
    }
    block_size_solved |= (block.size - input.measured_size.block).abs() > 0.01;

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
        block_size_solved,
    }
}

/// The preferred ratio expressed as logical inline size over logical block
/// size in the containing flow.
fn logical_aspect_ratio(style: BlockStyle) -> Option<f32> {
    style.aspect_ratio.and_then(|ratio| {
        (ratio.is_finite() && ratio > 0.0).then_some(if style.containing_flow.is_horizontal() {
            ratio
        } else {
            ratio.recip()
        })
    })
}

fn transfer_border_box_size(
    origin: f32,
    origin_padding_border: f32,
    destination_padding_border: f32,
    box_sizing: BlockBoxSizing,
    ratio: f32,
) -> f32 {
    let content = content_from_border_box(origin, origin_padding_border, box_sizing);
    border_box_from_content(content * ratio, destination_padding_border, box_sizing)
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
    intrinsic: Option<IntrinsicSizes>,
    replaced_size: Option<f32>,
    forced_size: Option<f32>,
) -> AxisGeometry {
    let start_auto = margin_start.is_none();
    let end_auto = margin_end.is_none();
    let mut margin_start = margin_start.unwrap_or(0.0);
    let mut margin_end = margin_end.unwrap_or(0.0);
    let resolved_start = inset_start;
    let resolved_end = inset_end;

    let mut size = forced_size.unwrap_or_else(|| {
        specified_border_box(preferred, containing, padding_border, box_sizing)
            .or_else(|| {
                intrinsic.and_then(|intrinsic| {
                    intrinsic_border_box(
                        preferred,
                        containing,
                        padding_border,
                        box_sizing,
                        intrinsic,
                    )
                })
            })
            .unwrap_or_else(|| {
                if let Some(size) = replaced_size {
                    size
                } else if let Some(intrinsic) = intrinsic {
                    let available = (containing
                        - resolved_start.unwrap_or(0.0)
                        - resolved_end.unwrap_or(0.0)
                        - margin_start
                        - margin_end
                        - padding_border)
                        .max(0.0);
                    if resolved_start.is_some() && resolved_end.is_some() {
                        // CSS2 10.3.7 gives the auto inline size the remaining
                        // space when both inline insets are definite.
                        available + padding_border
                    } else {
                        intrinsic
                            .min_content
                            .max(available)
                            .min(intrinsic.max_content)
                            + padding_border
                    }
                } else if matches!(preferred, BlockSizeValue::Auto)
                    && resolved_start.is_some()
                    && resolved_end.is_some()
                {
                    // CSS2 10.6.4 gives a non-replaced absolutely positioned
                    // box with an automatic block size and two definite block
                    // insets the remaining containing-block space. The inline
                    // axis reaches the same equation through its intrinsic
                    // branch; the block axis has no intrinsic contribution.
                    (containing
                        - resolved_start.unwrap_or(0.0)
                        - resolved_end.unwrap_or(0.0)
                        - margin_start
                        - margin_end
                        - padding_border)
                        .max(0.0)
                        + padding_border
                } else {
                    measured_size.max(padding_border)
                }
            })
    });
    size = clamp_border_box(
        size,
        minimum,
        maximum,
        containing,
        padding_border,
        box_sizing,
    );

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

#[derive(Clone, Copy, Default)]
struct ReplacedAxisSize {
    inline: Option<f32>,
    block: Option<f32>,
}

/// Resolve the small replaced-content subset K5d owns: a replaced leaf with
/// definite CSS dimensions, an intrinsic size, or one usable aspect ratio.
/// It returns border-box dimensions, leaving min/max clamping and the inset
/// equation to `solve_axis`.
fn resolve_replaced_size(
    style: BlockStyle,
    input: ReplacedSize,
    containing_size: LogicalSize,
) -> ReplacedAxisSize {
    let dimensions = logical_dimensions(style.containing_flow, style.size);
    let padding_border = style.logical_padding_border(containing_size.inline);
    let inline_padding = padding_border.inline_start + padding_border.inline_end;
    let block_padding = padding_border.block_start + padding_border.block_end;
    let specified_inline = specified_border_box(
        dimensions.inline,
        containing_size.inline,
        inline_padding,
        style.box_sizing,
    );
    let specified_block = specified_border_box(
        dimensions.block,
        containing_size.block,
        block_padding,
        style.box_sizing,
    );
    let intrinsic = input.intrinsic_size;
    let intrinsic_ratio = intrinsic.and_then(|size| {
        (size.inline > 0.0 && size.block > 0.0).then_some(size.inline / size.block)
    });
    // CSS `aspect-ratio` is physical width/height. Flip it into the logical
    // axes when the containing flow is vertical.
    let style_ratio = style.aspect_ratio.and_then(|ratio| {
        (ratio.is_finite() && ratio > 0.0).then_some(if style.containing_flow.is_horizontal() {
            ratio
        } else {
            ratio.recip()
        })
    });
    let ratio = style_ratio.or(intrinsic_ratio);

    let inline = specified_inline.or_else(|| {
        specified_block
            .and_then(|block| {
                ratio.map(|ratio| {
                    let block_content =
                        content_from_border_box(block, block_padding, style.box_sizing);
                    border_box_from_content(block_content * ratio, inline_padding, style.box_sizing)
                })
            })
            .or_else(|| {
                intrinsic.map(|size| {
                    border_box_from_content(size.inline, inline_padding, style.box_sizing)
                })
            })
    });
    let block = specified_block.or_else(|| {
        specified_inline
            .and_then(|inline| {
                ratio.map(|ratio| {
                    let inline_content =
                        content_from_border_box(inline, inline_padding, style.box_sizing);
                    border_box_from_content(inline_content / ratio, block_padding, style.box_sizing)
                })
            })
            .or_else(|| {
                intrinsic.map(|size| {
                    let content = style_ratio.map_or(size.block, |ratio| size.inline / ratio);
                    border_box_from_content(content, block_padding, style.box_sizing)
                })
            })
    });
    ReplacedAxisSize { inline, block }
}

fn border_box_from_content(content: f32, padding_border: f32, box_sizing: BlockBoxSizing) -> f32 {
    match box_sizing {
        BlockBoxSizing::ContentBox => content + padding_border,
        BlockBoxSizing::BorderBox => content.max(padding_border),
    }
}

fn content_from_border_box(
    border_box: f32,
    padding_border: f32,
    box_sizing: BlockBoxSizing,
) -> f32 {
    match box_sizing {
        BlockBoxSizing::ContentBox => (border_box - padding_border).max(0.0),
        BlockBoxSizing::BorderBox => border_box,
    }
}

fn intrinsic_border_box(
    preferred: BlockSizeValue,
    containing: f32,
    padding_border: f32,
    box_sizing: BlockBoxSizing,
    intrinsic: IntrinsicSizes,
) -> Option<f32> {
    let content = match preferred {
        BlockSizeValue::MinContent => intrinsic.min_content,
        BlockSizeValue::MaxContent => intrinsic.max_content,
        BlockSizeValue::FitContent(limit) => intrinsic
            .min_content
            .max(limit.resolve(containing))
            .min(intrinsic.max_content),
        BlockSizeValue::Auto | BlockSizeValue::None | BlockSizeValue::Length(_) => return None,
    };
    Some(match box_sizing {
        BlockBoxSizing::ContentBox => content + padding_border,
        BlockBoxSizing::BorderBox => content.max(padding_border),
    })
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
    maximum.map_or(size.max(minimum), |maximum| {
        size.max(minimum).min(maximum.max(minimum))
    })
}

fn logical_dimensions<T: Copy>(
    flow: crate::FlowAxes,
    dimensions: BlockDimensions<T>,
) -> LogicalSizeOf<T> {
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
            intrinsic_inline: None,
            replaced: None,
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
    fn definite_block_size_marks_a_changed_formatter_size_as_solved() {
        let mut style = positioned();
        style.size.height = BlockSizeValue::Length(FlowLength::percent(0.5));

        let geometry = solve_positioned_box(style, input());

        assert_eq!(geometry.logical_rect.block_size, 50.0);
        assert!(geometry.block_size_solved);
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

    #[test]
    fn automatic_inline_size_uses_admitted_intrinsics_not_the_measured_fallback() {
        let mut style = positioned();
        style.inset.left = FlowLengthAuto::Value(FlowLength::px(10.0));
        let mut input = input();
        input.intrinsic_inline = IntrinsicSizes::new(40.0, 120.0);

        let geometry = solve_positioned_box(style, input);

        assert_eq!(geometry.logical_rect.inline_size, 120.0);
        assert_eq!(geometry.logical_rect.inline_start, 10.0);
    }

    #[test]
    fn automatic_inline_size_fills_between_definite_insets() {
        let mut style = positioned();
        style.inset.left = FlowLengthAuto::Value(FlowLength::px(10.0));
        style.inset.right = FlowLengthAuto::Value(FlowLength::px(20.0));
        let mut input = input();
        input.intrinsic_inline = IntrinsicSizes::new(40.0, 120.0);

        let geometry = solve_positioned_box(style, input);

        assert_eq!(geometry.logical_rect.inline_size, 170.0);
        assert_eq!(geometry.logical_rect.inline_start, 10.0);
    }

    #[test]
    fn automatic_block_size_fills_between_definite_insets() {
        let mut style = positioned();
        style.inset.top = FlowLengthAuto::Value(FlowLength::px(10.0));
        style.inset.bottom = FlowLengthAuto::Value(FlowLength::px(20.0));

        let geometry = solve_positioned_box(style, input());

        assert_eq!(geometry.logical_rect.block_size, 70.0);
        assert_eq!(geometry.logical_rect.block_start, 10.0);
        assert!(geometry.block_size_solved);
    }

    #[test]
    fn replaced_intrinsic_size_does_not_fill_between_definite_insets() {
        let mut style = positioned();
        style.replaced = true;
        style.inset.left = FlowLengthAuto::Value(FlowLength::px(10.0));
        style.inset.right = FlowLengthAuto::Value(FlowLength::px(20.0));
        let mut input = input();
        input.replaced = Some(ReplacedSize {
            intrinsic_size: Some(LogicalSize {
                inline: 50.0,
                block: 20.0,
            }),
        });

        let geometry = solve_positioned_box(style, input);

        assert_eq!(geometry.logical_rect.inline_size, 50.0);
        assert_eq!(geometry.logical_rect.block_size, 20.0);
        assert_eq!(geometry.logical_rect.inline_start, 10.0);
    }

    #[test]
    fn replaced_auto_block_size_follows_its_intrinsic_ratio() {
        let mut style = positioned();
        style.replaced = true;
        style.size.width = BlockSizeValue::Length(FlowLength::px(80.0));
        let mut input = input();
        input.replaced = Some(ReplacedSize {
            intrinsic_size: Some(LogicalSize {
                inline: 50.0,
                block: 20.0,
            }),
        });

        let geometry = solve_positioned_box(style, input);

        assert_eq!(geometry.logical_rect.inline_size, 80.0);
        assert_eq!(geometry.logical_rect.block_size, 32.0);
    }

    #[test]
    fn aspect_ratio_derives_the_block_size_from_the_shrink_to_fit_inline_size() {
        let mut style = positioned();
        style.aspect_ratio = Some(2.0);
        let mut input = input();
        input.intrinsic_inline = IntrinsicSizes::new(20.0, 60.0);

        let geometry = solve_positioned_box(style, input);

        assert_eq!(geometry.logical_rect.inline_size, 60.0);
        assert_eq!(geometry.logical_rect.block_size, 30.0);
    }

    #[test]
    fn aspect_ratio_transfers_the_block_maximum_to_an_automatic_inline_size() {
        let mut style = positioned();
        style.aspect_ratio = Some(1.0);
        style.max_size.height = BlockSizeValue::Length(FlowLength::percent(1.0));
        let mut input = input();
        input.intrinsic_inline = IntrinsicSizes::new(20.0, 200.0);

        let geometry = solve_positioned_box(style, input);

        assert_eq!(geometry.logical_rect.inline_size, 100.0);
        assert_eq!(geometry.logical_rect.block_size, 100.0);
    }
}
