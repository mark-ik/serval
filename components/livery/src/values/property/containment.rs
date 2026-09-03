// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Containment values: the `contain` keyword set, intrinsic sizing under
//! containment, and container names.

use super::*;

/// The computed containment types activated by `contain`.
///
/// `strict` and `content` are normalized to their computed keyword sets, and
/// the component-keyword form serializes in grammar order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Contain(u8);

impl Contain {
    const SIZE: u8 = 1 << 0;
    const INLINE_SIZE: u8 = 1 << 1;
    const LAYOUT: u8 = 1 << 2;
    const STYLE: u8 = 1 << 3;
    const PAINT: u8 = 1 << 4;

    pub const NONE: Self = Self(0);
    pub const CONTENT: Self = Self(Self::LAYOUT | Self::STYLE | Self::PAINT);
    pub const STRICT: Self = Self(Self::SIZE | Self::LAYOUT | Self::STYLE | Self::PAINT);

    /// Whether this value activates at least one containment type.
    pub const fn is_active(self) -> bool {
        self.0 != 0
    }

    pub const fn has_size(self) -> bool {
        self.0 & Self::SIZE != 0
    }

    pub const fn has_inline_size(self) -> bool {
        self.0 & Self::INLINE_SIZE != 0
    }

    pub const fn has_layout(self) -> bool {
        self.0 & Self::LAYOUT != 0
    }

    pub const fn has_paint(self) -> bool {
        self.0 & Self::PAINT != 0
    }
}

impl FromStr for Contain {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("none") {
            return Ok(Self::NONE);
        }
        if input.eq_ignore_ascii_case("strict") {
            return Ok(Self::STRICT);
        }
        if input.eq_ignore_ascii_case("content") {
            return Ok(Self::CONTENT);
        }

        let mut flags = 0_u8;
        for keyword in input.split_ascii_whitespace() {
            let bit = if keyword.eq_ignore_ascii_case("size") {
                if flags & Self::INLINE_SIZE != 0 {
                    return Err(ParseError::expected("at most one of size and inline-size"));
                }
                Self::SIZE
            } else if keyword.eq_ignore_ascii_case("inline-size") {
                if flags & Self::SIZE != 0 {
                    return Err(ParseError::expected("at most one of size and inline-size"));
                }
                Self::INLINE_SIZE
            } else if keyword.eq_ignore_ascii_case("layout") {
                Self::LAYOUT
            } else if keyword.eq_ignore_ascii_case("style") {
                Self::STYLE
            } else if keyword.eq_ignore_ascii_case("paint") {
                Self::PAINT
            } else {
                return Err(ParseError::expected(
                    "none, strict, content, or containment keywords",
                ));
            };
            if flags & bit != 0 {
                return Err(ParseError::expected(
                    "each containment keyword at most once",
                ));
            }
            flags |= bit;
        }

        (flags != 0)
            .then_some(Self(flags))
            .ok_or_else(|| ParseError::expected("a contain value"))
    }
}

impl fmt::Display for Contain {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.is_active() {
            return formatter.write_str("none");
        }

        let mut first = true;
        for (bit, keyword) in [
            (Self::SIZE, "size"),
            (Self::INLINE_SIZE, "inline-size"),
            (Self::LAYOUT, "layout"),
            (Self::STYLE, "style"),
            (Self::PAINT, "paint"),
        ] {
            if self.0 & bit == 0 {
                continue;
            }
            if !first {
                formatter.write_str(" ")?;
            }
            formatter.write_str(keyword)?;
            first = false;
        }
        Ok(())
    }
}

/// The bounded physical substitute-size pair consumed by positioned size
/// containment. CSS's remembered-size `auto` form remains outside this lane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ContainIntrinsicSize {
    None,
    Lengths { width: Length, height: Length },
}

impl ContainIntrinsicSize {
    pub const NONE: Self = Self::None;

    pub const fn physical_lengths(self) -> Option<(Length, Length)> {
        match self {
            Self::None => None,
            Self::Lengths { width, height } => Some((width, height)),
        }
    }
}

impl FromStr for ContainIntrinsicSize {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("none") {
            return Ok(Self::None);
        }
        let values = input.split_ascii_whitespace().collect::<Vec<_>>();
        let (width, height) = match values.as_slice() {
            [width] => {
                let width = width.parse::<Length>()?;
                (width, width)
            },
            [width, height] => (width.parse::<Length>()?, height.parse::<Length>()?),
            _ => {
                return Err(ParseError::expected(
                    "none or one or two non-negative lengths",
                ));
            },
        };
        if width.value < 0.0 || height.value < 0.0 {
            return Err(ParseError::expected(
                "none or one or two non-negative lengths",
            ));
        }
        Ok(Self::Lengths { width, height })
    }
}

impl fmt::Display for ContainIntrinsicSize {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => formatter.write_str("none"),
            Self::Lengths { width, height } => {
                width.fmt(formatter)?;
                if height != width {
                    write!(formatter, " {height}")?;
                }
                Ok(())
            },
        }
    }
}

/// Names by which descendant `@container` rules can select this query
/// container.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContainerName {
    None,
    Names(Vec<String>),
}

impl ContainerName {
    pub fn contains(&self, name: &str) -> bool {
        match self {
            Self::None => false,
            Self::Names(names) => names.iter().any(|candidate| candidate == name),
        }
    }

    pub fn names(&self) -> &[String] {
        match self {
            Self::None => &[],
            Self::Names(names) => names,
        }
    }
}

impl FromStr for ContainerName {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("none") {
            return Ok(Self::None);
        }
        let names = input
            .split_ascii_whitespace()
            .map(|name| {
                let lower = name.to_ascii_lowercase();
                let reserved = [
                    "none",
                    "default",
                    "initial",
                    "inherit",
                    "unset",
                    "revert",
                    "revert-layer",
                    "and",
                    "or",
                    "not",
                ];
                let valid = !reserved.contains(&lower.as_str())
                    && name.chars().next().is_some_and(|character| {
                        character.is_ascii_alphabetic() || character == '_'
                    })
                    && name.chars().all(|character| {
                        character.is_ascii_alphanumeric() || "-_".contains(character)
                    });
                valid
                    .then(|| name.to_owned())
                    .ok_or_else(|| ParseError::expected("none or one or more custom identifiers"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        (!names.is_empty())
            .then_some(Self::Names(names))
            .ok_or_else(|| ParseError::expected("none or one or more custom identifiers"))
    }
}

impl fmt::Display for ContainerName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => formatter.write_str("none"),
            Self::Names(names) => formatter.write_str(&names.join(" ")),
        }
    }
}

keyword_value! {
    /// Inline base direction used by logical layout and bidi.
    pub enum Direction {
        Ltr => "ltr",
        Rtl => "rtl",
    }
}

keyword_value! {
    /// Block-flow direction used to map logical viewport and container axes.
    pub enum WritingMode {
        HorizontalTb => "horizontal-tb",
        VerticalRl => "vertical-rl",
        VerticalLr => "vertical-lr",
        SidewaysRl => "sideways-rl",
        SidewaysLr => "sideways-lr",
    }
}

impl WritingMode {
    pub const fn is_vertical(self) -> bool {
        !matches!(self, Self::HorizontalTb)
    }
}

keyword_value! {
    /// Whether a box participates in hit testing and event dispatch.
    pub enum PointerEvents {
        Auto => "auto",
        None => "none",
    }
}

keyword_value! {
    /// Visibility state. Hidden boxes retain layout space but are not painted.
    pub enum Visibility {
        Visible => "visible",
        Hidden => "hidden",
        Collapse => "collapse",
    }
}

keyword_value! {
    /// Inline-axis text alignment keywords used by Parley.
    pub enum TextAlign {
        Start => "start",
        End => "end",
        Left => "left",
        Right => "right",
        Center => "center",
        Justify => "justify",
        JustifyAll => "justify-all",
    }
}

keyword_value! {
    /// `text-align-last` keywords.
    pub enum TextAlignLast {
        Auto => "auto",
        Start => "start",
        End => "end",
        Left => "left",
        Right => "right",
        Center => "center",
        Justify => "justify",
    }
}

keyword_value! {
    /// `text-justify` keywords.
    pub enum TextJustify {
        Auto => "auto",
        None => "none",
        InterWord => "inter-word",
        InterCharacter => "inter-character",
        Distribute => "distribute",
    }
}

keyword_value! {
    /// `overflow-wrap` keywords.
    pub enum OverflowWrap {
        Normal => "normal",
        BreakWord => "break-word",
        Anywhere => "anywhere",
    }
}

keyword_value! {
    /// `word-break` keywords.
    pub enum WordBreak {
        Normal => "normal",
        KeepAll => "keep-all",
        BreakAll => "break-all",
        BreakWord => "break-word",
    }
}

keyword_value! {
    /// `line-break` keywords.
    pub enum LineBreak {
        Auto => "auto",
        Loose => "loose",
        Normal => "normal",
        Strict => "strict",
        Anywhere => "anywhere",
    }
}

keyword_value! {
    /// `hyphens` keywords.
    pub enum Hyphens {
        None => "none",
        Manual => "manual",
        Auto => "auto",
    }
}

keyword_value! {
    /// The case-mapping component of `text-transform`.
    pub enum TextTransformCase {
        None => "none",
        Capitalize => "capitalize",
        Uppercase => "uppercase",
        Lowercase => "lowercase",
        MathAuto => "math-auto",
    }
}
