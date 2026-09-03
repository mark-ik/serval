// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! `background-*` values: image, position, repeat, origin/clip box, and
//! the size components, plus the shorthand that assembles them.

use super::*;

#[derive(Clone, Debug, PartialEq)]
pub enum BackgroundImage {
    None,
    LinearGradient {
        from: ComputedColor,
        to: ComputedColor,
    },
    Url(Box<str>),
}

impl BackgroundImage {
    /// Interpolate the bounded single-image forms consumed by the retained
    /// paint lane. Two-stop linear gradients interpolate each stop; URLs and
    /// mixed image shapes remain discrete until the image-list ratchet.
    pub fn interpolate(&self, other: &Self, progress: f32) -> Self {
        let progress = progress.clamp(0.0, 1.0);
        let value = match (self, other) {
            (
                Self::LinearGradient { from, to },
                Self::LinearGradient {
                    from: other_from,
                    to: other_to,
                },
            ) => Some(Self::LinearGradient {
                from: from.interpolate(other_from, progress),
                to: to.interpolate(other_to, progress),
            }),
            _ => None,
        };
        value.unwrap_or_else(|| {
            if progress < 0.5 {
                self.clone()
            } else {
                other.clone()
            }
        })
    }

    /// Interpolate gradient stops after resolving each endpoint under its
    /// element context. URL and mixed image shapes remain discrete, as in the
    /// ordinary bounded interpolation path.
    pub fn interpolate_used(
        &self,
        other: &Self,
        from_context: UsedColorContext,
        to_context: UsedColorContext,
        progress: f32,
    ) -> Self {
        let progress = progress.clamp(0.0, 1.0);
        let value = match (self, other) {
            (
                Self::LinearGradient { from, to },
                Self::LinearGradient {
                    from: other_from,
                    to: other_to,
                },
            ) => Some(Self::LinearGradient {
                from: from.interpolate_used(other_from, from_context, to_context, progress),
                to: to.interpolate_used(other_to, from_context, to_context, progress),
            }),
            _ => None,
        };
        value.unwrap_or_else(|| {
            if progress < 0.5 {
                self.clone()
            } else {
                other.clone()
            }
        })
    }
}

/// The bounded background-position pair consumed by the paint lane. Lengths,
/// percentages, and the five physical position keywords are accepted; the
/// full four-value grammar remains outside this ratchet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BackgroundPosition {
    pub x: LengthPercentage,
    pub y: LengthPercentage,
}

impl BackgroundPosition {
    pub const ZERO: Self = Self {
        x: LengthPercentage::ZERO,
        y: LengthPercentage::ZERO,
    };

    /// Interpolate the bounded two-component position used by the retained
    /// background image lane. Each component keeps the shared
    /// length/percentage unit boundary.
    pub fn interpolate(self, other: Self, progress: f32) -> Self {
        Self {
            x: self.x.interpolate(other.x, progress),
            y: self.y.interpolate(other.y, progress),
        }
    }
}

fn position_component(input: &str, horizontal: bool) -> Result<LengthPercentage, ParseError> {
    let value = input.trim();
    let keyword = if value.eq_ignore_ascii_case("center") {
        Some(LengthPercentage::Percentage(0.5))
    } else if horizontal && value.eq_ignore_ascii_case("left") {
        Some(LengthPercentage::ZERO)
    } else if horizontal && value.eq_ignore_ascii_case("right") {
        Some(LengthPercentage::Percentage(1.0))
    } else if !horizontal && value.eq_ignore_ascii_case("top") {
        Some(LengthPercentage::ZERO)
    } else if !horizontal && value.eq_ignore_ascii_case("bottom") {
        Some(LengthPercentage::Percentage(1.0))
    } else {
        None
    };
    keyword.map_or_else(|| value.parse(), Ok)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PositionKeyword {
    Left,
    Right,
    Top,
    Bottom,
    Center,
}

fn position_keyword(value: &str) -> Option<PositionKeyword> {
    match value.trim().to_ascii_lowercase().as_str() {
        "left" => Some(PositionKeyword::Left),
        "right" => Some(PositionKeyword::Right),
        "top" => Some(PositionKeyword::Top),
        "bottom" => Some(PositionKeyword::Bottom),
        "center" => Some(PositionKeyword::Center),
        _ => None,
    }
}

fn from_far_edge(offset: LengthPercentage) -> Result<LengthPercentage, ParseError> {
    match offset {
        LengthPercentage::Zero => Ok(LengthPercentage::Percentage(1.0)),
        LengthPercentage::Percentage(value) => Ok(LengthPercentage::Percentage(1.0 - value)),
        other => format!("calc(100% - {other})").parse(),
    }
}

fn position_from_edge_offsets(values: &[&str]) -> Result<BackgroundPosition, ParseError> {
    let error = || ParseError::expected("edge keywords with at most one offset each");
    let mut x = None;
    let mut y = None;
    let mut centers = 0_u8;
    let mut index = 0;
    while index < values.len() {
        let keyword = position_keyword(values[index]).ok_or_else(error)?;
        index += 1;
        let offset = if keyword != PositionKeyword::Center
            && index < values.len()
            && position_keyword(values[index]).is_none()
        {
            let offset = values[index].parse::<LengthPercentage>()?;
            index += 1;
            Some(offset)
        } else {
            None
        };
        let resolved = match (keyword, offset) {
            (PositionKeyword::Center, _) => {
                centers += 1;
                continue;
            },
            (PositionKeyword::Left | PositionKeyword::Top, Some(offset)) => offset,
            (PositionKeyword::Left | PositionKeyword::Top, None) => LengthPercentage::ZERO,
            (PositionKeyword::Right | PositionKeyword::Bottom, Some(offset)) => {
                from_far_edge(offset)?
            },
            (PositionKeyword::Right | PositionKeyword::Bottom, None) => {
                LengthPercentage::Percentage(1.0)
            },
        };
        let slot = if matches!(keyword, PositionKeyword::Left | PositionKeyword::Right) {
            &mut x
        } else {
            &mut y
        };
        if slot.replace(resolved).is_some() {
            return Err(error());
        }
    }
    match (x, y, centers) {
        (Some(x), Some(y), 0) => Ok(BackgroundPosition { x, y }),
        (Some(x), None, 1) => Ok(BackgroundPosition {
            x,
            y: LengthPercentage::Percentage(0.5),
        }),
        (None, Some(y), 1) => Ok(BackgroundPosition {
            x: LengthPercentage::Percentage(0.5),
            y,
        }),
        _ => Err(error()),
    }
}

impl FromStr for BackgroundPosition {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let values = shadow_components(input.trim());
        match values.as_slice() {
            [value] => {
                if value.eq_ignore_ascii_case("top") || value.eq_ignore_ascii_case("bottom") {
                    Ok(Self {
                        x: LengthPercentage::Percentage(0.5),
                        y: position_component(value, false)?,
                    })
                } else {
                    Ok(Self {
                        x: position_component(value, true)?,
                        y: LengthPercentage::Percentage(0.5),
                    })
                }
            },
            [first, second]
                if matches!(
                    position_keyword(first),
                    Some(PositionKeyword::Top | PositionKeyword::Bottom)
                ) || matches!(
                    position_keyword(second),
                    Some(PositionKeyword::Left | PositionKeyword::Right)
                ) =>
            {
                if position_keyword(first).is_none() || position_keyword(second).is_none() {
                    return Err(ParseError::expected(
                        "a horizontal value before a vertical value",
                    ));
                }
                Ok(Self {
                    x: position_component(second, true)?,
                    y: position_component(first, false)?,
                })
            },
            [first, second] => Ok(Self {
                x: position_component(first, true)?,
                y: position_component(second, false)?,
            }),
            [_, _, _] | [_, _, _, _] => position_from_edge_offsets(&values),
            _ => Err(ParseError::expected(
                "one to four background-position values",
            )),
        }
    }
}

impl fmt::Display for BackgroundPosition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.x, self.y)
    }
}

keyword_value! {
    /// One axis of `background-repeat`.
    pub enum RepeatStyle {
        Repeat => "repeat",
        NoRepeat => "no-repeat",
        Space => "space",
        Round => "round",
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BackgroundRepeat {
    pub x: RepeatStyle,
    pub y: RepeatStyle,
}

impl BackgroundRepeat {
    pub const REPEAT: Self = Self::new(RepeatStyle::Repeat, RepeatStyle::Repeat);
    pub const NO_REPEAT: Self = Self::new(RepeatStyle::NoRepeat, RepeatStyle::NoRepeat);

    pub const fn new(x: RepeatStyle, y: RepeatStyle) -> Self {
        Self { x, y }
    }

    /// Repeat modes are discrete in CSS. The retained transition switches to
    /// the target mode at the midpoint of the clock.
    pub fn interpolate(self, other: Self, progress: f32) -> Self {
        if progress.clamp(0.0, 1.0) < 0.5 {
            self
        } else {
            other
        }
    }
}

impl FromStr for BackgroundRepeat {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let values = input.split_ascii_whitespace().collect::<Vec<_>>();
        match values.as_slice() {
            [value] if value.eq_ignore_ascii_case("repeat-x") => {
                Ok(Self::new(RepeatStyle::Repeat, RepeatStyle::NoRepeat))
            },
            [value] if value.eq_ignore_ascii_case("repeat-y") => {
                Ok(Self::new(RepeatStyle::NoRepeat, RepeatStyle::Repeat))
            },
            [value] => {
                let style = value.parse::<RepeatStyle>()?;
                Ok(Self::new(style, style))
            },
            [x, y] => Ok(Self::new(x.parse()?, y.parse()?)),
            _ => Err(ParseError::expected(
                "one or two background-repeat keywords",
            )),
        }
    }
}

impl fmt::Display for BackgroundRepeat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.x, self.y) {
            (RepeatStyle::Repeat, RepeatStyle::NoRepeat) => formatter.write_str("repeat-x"),
            (RepeatStyle::NoRepeat, RepeatStyle::Repeat) => formatter.write_str("repeat-y"),
            (x, y) if x == y => x.fmt(formatter),
            (x, y) => write!(formatter, "{x} {y}"),
        }
    }
}

/// A retained background positioning or painting box. `Text` is carried so
/// paint can conservatively suppress an unsupported glyph clip instead of
/// leaking the background through the full element box.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BackgroundBox {
    BorderBox,
    PaddingBox,
    ContentBox,
    Text,
}

impl FromStr for BackgroundBox {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let parse_one = |value: &str| match value.trim().to_ascii_lowercase().as_str() {
            "border-box" => Ok(Self::BorderBox),
            "padding-box" => Ok(Self::PaddingBox),
            "content-box" => Ok(Self::ContentBox),
            "text" => Ok(Self::Text),
            _ => Err(ParseError::expected(
                "border-box, padding-box, content-box, or text",
            )),
        };
        let values = split_top_level_commas(input)
            .into_iter()
            .map(parse_one)
            .collect::<Result<Vec<_>, _>>()?;
        if values.is_empty() {
            return Err(ParseError::expected("a background box"));
        }
        // Image-layer lists remain outside the retained single-image model.
        // Choosing the innermost authored clip is conservative for the canvas
        // color: it cannot leak through a border merely because extra layers
        // were collapsed by this bounded value representation.
        Ok(values
            .iter()
            .copied()
            .find(|value| matches!(value, Self::Text | Self::ContentBox))
            .unwrap_or(values[0]))
    }
}

impl fmt::Display for BackgroundBox {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::BorderBox => "border-box",
            Self::PaddingBox => "padding-box",
            Self::ContentBox => "content-box",
            Self::Text => "text",
        })
    }
}

keyword_value! {
    /// `background-attachment`.
    pub enum BackgroundAttachment {
        Scroll => "scroll",
        Fixed => "fixed",
        Local => "local",
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BackgroundSizeComponent {
    Auto,
    Value(LengthPercentage),
}

impl BackgroundSizeComponent {
    fn interpolate(self, other: Self, progress: f32) -> Option<Self> {
        match (self, other) {
            (Self::Auto, Self::Auto) => Some(Self::Auto),
            (Self::Value(from), Self::Value(to)) => {
                Some(Self::Value(from.interpolate(to, progress)))
            },
            _ => None,
        }
    }

    fn resolve_relative(self, environment: RelativeLengthEnvironment) -> Self {
        match self {
            Self::Auto => Self::Auto,
            Self::Value(value) => Self::Value(value.resolve_relative(environment)),
        }
    }
}

impl FromStr for BackgroundSizeComponent {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("auto") {
            return Ok(Self::Auto);
        }
        let value = input.parse::<LengthPercentage>()?;
        let negative = match value {
            LengthPercentage::Length(length) => length.value < 0.0,
            LengthPercentage::Percentage(percentage) => percentage < 0.0,
            _ => false,
        };
        if negative {
            return Err(ParseError::expected("a non-negative background-size"));
        }
        Ok(Self::Value(value))
    }
}

impl fmt::Display for BackgroundSizeComponent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => formatter.write_str("auto"),
            Self::Value(value) => value.fmt(formatter),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BackgroundSize {
    Cover,
    Contain,
    Explicit {
        width: BackgroundSizeComponent,
        height: BackgroundSizeComponent,
    },
}

impl BackgroundSize {
    pub const AUTO: Self = Self::Explicit {
        width: BackgroundSizeComponent::Auto,
        height: BackgroundSizeComponent::Auto,
    };

    pub fn interpolate(self, other: Self, progress: f32) -> Self {
        let progress = progress.clamp(0.0, 1.0);
        if let (
            Self::Explicit {
                width: from_width,
                height: from_height,
            },
            Self::Explicit {
                width: to_width,
                height: to_height,
            },
        ) = (self, other)
            && let Some(width) = from_width.interpolate(to_width, progress)
            && let Some(height) = from_height.interpolate(to_height, progress)
        {
            return Self::Explicit { width, height };
        }
        if progress < 0.5 { self } else { other }
    }

    pub fn resolve_relative(self, environment: RelativeLengthEnvironment) -> Self {
        match self {
            Self::Explicit { width, height } => Self::Explicit {
                width: width.resolve_relative(environment),
                height: height.resolve_relative(environment),
            },
            keyword => keyword,
        }
    }
}

impl FromStr for BackgroundSize {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("cover") {
            return Ok(Self::Cover);
        }
        if input.eq_ignore_ascii_case("contain") {
            return Ok(Self::Contain);
        }
        match shadow_components(input).as_slice() {
            [width] => Ok(Self::Explicit {
                width: width.parse()?,
                height: BackgroundSizeComponent::Auto,
            }),
            [width, height] => Ok(Self::Explicit {
                width: width.parse()?,
                height: height.parse()?,
            }),
            _ => Err(ParseError::expected(
                "cover, contain, or one or two background-size values",
            )),
        }
    }
}

impl fmt::Display for BackgroundSize {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cover => formatter.write_str("cover"),
            Self::Contain => formatter.write_str("contain"),
            Self::Explicit { width, height } => write!(formatter, "{width} {height}"),
        }
    }
}

impl FromStr for BackgroundImage {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("none") {
            return Ok(Self::None);
        }
        if input.len() > 5 && input[..4].eq_ignore_ascii_case("url(") && input.ends_with(')') {
            let raw = input[4..input.len() - 1].trim();
            let url = raw
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .or_else(|| {
                    raw.strip_prefix('\'')
                        .and_then(|value| value.strip_suffix('\''))
                })
                .unwrap_or(raw)
                .trim();
            if !url.is_empty() {
                return Ok(Self::Url(url.into()));
            }
        }
        if let Some(arguments) = input
            .strip_prefix("image-set(")
            .and_then(|value| value.strip_suffix(')'))
        {
            let candidates = split_top_level_commas(arguments);
            if let [candidate] = candidates.as_slice() {
                let components = split_top_level(candidate.trim());
                if let [image, resolution] = components.as_slice()
                    && resolution
                        .strip_suffix('x')
                        .and_then(|density| density.parse::<f32>().ok())
                        .is_some_and(|density| density.is_finite() && density > 0.0)
                {
                    let selected = image.parse::<Self>()?;
                    if matches!(selected, Self::LinearGradient { .. }) {
                        return Ok(selected);
                    }
                }
            }
            return Err(ParseError::expected(
                "a single gradient image-set candidate with a positive pixel density",
            ));
        }
        let Some(arguments) = input
            .strip_prefix("linear-gradient(")
            .and_then(|value| value.strip_suffix(')'))
        else {
            return Err(ParseError::expected(
                "none, url(<image>), or a two-stop linear-gradient",
            ));
        };
        let stops = split_top_level_commas(arguments);
        let mut colors = stops.iter().map(|stop| stop.trim());
        let from = colors
            .next()
            .ok_or_else(|| ParseError::expected("two gradient colors"))?
            .parse::<ComputedColor>()?;
        let to = colors
            .next()
            .ok_or_else(|| ParseError::expected("two gradient colors"))?
            .parse::<ComputedColor>()?;
        if colors.next().is_some() {
            return Err(ParseError::expected("two gradient colors"));
        }
        Ok(Self::LinearGradient { from, to })
    }
}

/// Split on commas that are not inside parentheses.
///
/// A gradient stop list cannot be split with `str::split(',')`: a functional
/// color carries its own commas, so `linear-gradient(rgb(255, 0, 0), blue)`
/// would read as four stops. This surfaced when colors started serializing in
/// the `rgb()` form CSS Color 4 requires, but the bug predates that: an
/// authored comma-form color inside a gradient never parsed.
pub(crate) fn split_top_level_commas(input: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, ch) in input.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&input[start..index]);
                start = index + 1;
            },
            _ => {},
        }
    }
    parts.push(&input[start..]);
    parts
}

impl fmt::Display for BackgroundImage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => formatter.write_str("none"),
            Self::LinearGradient { from, to } => {
                write!(formatter, "linear-gradient({from}, {to})")
            },
            Self::Url(url) => write!(formatter, "url({url})"),
        }
    }
}
