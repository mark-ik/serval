/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Flow-relative geometry used by generated logical-property mappings.
//!
//! This is a bounded adaptation of `genet-stylo` 0.19.1's
//! `logical_geometry.rs`. Livery keeps its own value enums and retains only
//! the axis/side projection required by its generated property catalog.

use super::{Direction, WritingMode};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum LogicalAxis {
    Inline,
    Block,
}

impl LogicalAxis {
    pub const fn to_physical(self, writing_mode: WritingMode) -> PhysicalAxis {
        match (self, writing_mode.is_vertical()) {
            (Self::Inline, false) | (Self::Block, true) => PhysicalAxis::Horizontal,
            (Self::Inline, true) | (Self::Block, false) => PhysicalAxis::Vertical,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PhysicalAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum LogicalSide {
    BlockStart,
    BlockEnd,
    InlineStart,
    InlineEnd,
}

impl LogicalSide {
    pub const fn to_physical(
        self,
        writing_mode: WritingMode,
        direction: Direction,
    ) -> PhysicalSide {
        match self {
            Self::BlockStart => match writing_mode {
                WritingMode::HorizontalTb => PhysicalSide::Top,
                WritingMode::VerticalRl | WritingMode::SidewaysRl => PhysicalSide::Right,
                WritingMode::VerticalLr | WritingMode::SidewaysLr => PhysicalSide::Left,
            },
            Self::BlockEnd => match writing_mode {
                WritingMode::HorizontalTb => PhysicalSide::Bottom,
                WritingMode::VerticalRl | WritingMode::SidewaysRl => PhysicalSide::Left,
                WritingMode::VerticalLr | WritingMode::SidewaysLr => PhysicalSide::Right,
            },
            Self::InlineStart | Self::InlineEnd => {
                let reversed = match writing_mode {
                    WritingMode::SidewaysLr => matches!(direction, Direction::Ltr),
                    _ => matches!(direction, Direction::Rtl),
                };
                let start = match (writing_mode.is_vertical(), reversed) {
                    (false, false) => PhysicalSide::Left,
                    (false, true) => PhysicalSide::Right,
                    (true, false) => PhysicalSide::Top,
                    (true, true) => PhysicalSide::Bottom,
                };
                if matches!(self, Self::InlineStart) {
                    start
                } else {
                    start.opposite()
                }
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PhysicalSide {
    Top,
    Right,
    Bottom,
    Left,
}

impl PhysicalSide {
    const fn opposite(self) -> Self {
        match self {
            Self::Top => Self::Bottom,
            Self::Right => Self::Left,
            Self::Bottom => Self::Top,
            Self::Left => Self::Right,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_sides_follow_stylo_writing_mode_and_direction_rules() {
        let cases = [
            (
                WritingMode::HorizontalTb,
                Direction::Ltr,
                PhysicalSide::Left,
                PhysicalSide::Right,
            ),
            (
                WritingMode::HorizontalTb,
                Direction::Rtl,
                PhysicalSide::Right,
                PhysicalSide::Left,
            ),
            (
                WritingMode::VerticalRl,
                Direction::Ltr,
                PhysicalSide::Top,
                PhysicalSide::Bottom,
            ),
            (
                WritingMode::VerticalLr,
                Direction::Rtl,
                PhysicalSide::Bottom,
                PhysicalSide::Top,
            ),
            (
                WritingMode::SidewaysLr,
                Direction::Ltr,
                PhysicalSide::Bottom,
                PhysicalSide::Top,
            ),
            (
                WritingMode::SidewaysLr,
                Direction::Rtl,
                PhysicalSide::Top,
                PhysicalSide::Bottom,
            ),
        ];
        for (writing_mode, direction, start, end) in cases {
            assert_eq!(
                LogicalSide::InlineStart.to_physical(writing_mode, direction),
                start
            );
            assert_eq!(
                LogicalSide::InlineEnd.to_physical(writing_mode, direction),
                end
            );
        }
    }
}
