use std::{fmt, str::FromStr};

use super::{
    ComputedColor, Length, LengthPercentage, MathLengthPercentage, Matrix2D, ParseError,
    RelativeLengthEnvironment, UsedColorContext, format_number, keyword_value,
};

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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Duration(f32);

impl Duration {
    pub const ZERO: Self = Self(0.0);

    pub const fn milliseconds(self) -> f32 {
        self.0
    }
}

impl FromStr for Duration {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let trimmed = input.trim();
        let lowered = trimmed.to_ascii_lowercase();
        let atomic = if let Some(value) = lowered.strip_suffix("ms") {
            Some((value, 1.0))
        } else if let Some(value) = lowered.strip_suffix('s') {
            Some((value, 1_000.0))
        } else if lowered == "0" {
            Some(("0", 1.0))
        } else {
            None
        };
        let value = atomic
            .and_then(|(number, multiplier)| {
                number
                    .trim()
                    .parse::<f32>()
                    .ok()
                    .filter(|value| value.is_finite() && *value >= 0.0)
                    .map(|value| value * multiplier)
            })
            // A `calc()`/comparison time expression (e.g. `round(10s, 6s)`)
            // folds to milliseconds through the shared math lane. The bound is
            // non-negative, matching the animation/transition duration lane; a
            // negative computed delay stays out of scope.
            .or_else(|| {
                super::calc::parse_time(trimmed)
                    .ok()
                    .filter(|value| *value >= 0.0)
            })
            .ok_or_else(|| ParseError::expected("a non-negative CSS duration"))?;
        Ok(Self(value))
    }
}

impl fmt::Display for Duration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}ms", format_number(self.0))
    }
}

/// A single signed `animation-delay`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimationDelay(f32);

impl AnimationDelay {
    pub const ZERO: Self = Self(0.0);

    pub const fn milliseconds(self) -> f32 {
        self.0
    }
}

impl FromStr for AnimationDelay {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let trimmed = input.trim();
        let lowered = trimmed.to_ascii_lowercase();
        let atomic = if let Some(value) = lowered.strip_suffix("ms") {
            Some((value, 1.0))
        } else if let Some(value) = lowered.strip_suffix('s') {
            Some((value, 1_000.0))
        } else if lowered == "0" {
            Some(("0", 1.0))
        } else {
            None
        };
        let value = atomic
            .and_then(|(number, multiplier)| {
                number
                    .trim()
                    .parse::<f32>()
                    .ok()
                    .filter(|value| value.is_finite())
                    .map(|value| value * multiplier)
            })
            .or_else(|| super::calc::parse_time(trimmed).ok())
            .ok_or_else(|| ParseError::expected("a CSS time"))?;
        Ok(Self(value))
    }
}

impl fmt::Display for AnimationDelay {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}ms", format_number(self.0))
    }
}

/// A bounded CSS animation name. The first animation gate accepts one custom
/// identifier or `none`; comma-separated animation lists remain outside the
/// lane.
#[derive(Clone, Debug, PartialEq)]
pub enum AnimationName {
    None,
    Name(Box<str>),
}

impl AnimationName {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::None => None,
            Self::Name(name) => Some(name),
        }
    }
}

impl FromStr for AnimationName {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("none") {
            return Ok(Self::None);
        }
        let valid = !input.is_empty()
            && input.chars().enumerate().all(|(index, ch)| {
                ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') && index > 0
            })
            && input
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_alphabetic() || matches!(ch, '_' | '-'));
        if valid {
            Ok(Self::Name(input.into()))
        } else {
            Err(ParseError::expected("none or a custom animation name"))
        }
    }
}

impl fmt::Display for AnimationName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => formatter.write_str("none"),
            Self::Name(name) => formatter.write_str(name),
        }
    }
}

/// The supported transition-property set consumed by the retained paint clock.
/// Explicit lists retain their property bitset so new combinations do not
/// silently widen to `all`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TransitionProperty {
    All,
    None,
    Opacity,
    BackgroundColor,
    Color,
    BorderTopColor,
    BorderBottomColor,
    BorderLeftColor,
    BorderRightColor,
    BorderTopWidth,
    BorderBottomWidth,
    BorderLeftWidth,
    BorderRightWidth,
    BorderRadius,
    Transform,
    BackgroundPosition,
    BoxShadow,
    BackgroundImage,
    BorderTopStyle,
    BorderBottomStyle,
    BorderLeftStyle,
    BorderRightStyle,
    BackgroundRepeat,
    List(u32),
    OpacityAndBackgroundColor,
    OpacityAndColor,
    BackgroundColorAndColor,
    OpacityAndBackgroundColorAndColor,
}

impl FromStr for TransitionProperty {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("all") {
            return Ok(Self::All);
        }
        if input.eq_ignore_ascii_case("none") {
            return Ok(Self::None);
        }
        let mut flags = 0_u32;
        let mut saw_item = false;
        for item in input.split(',') {
            saw_item = true;
            let bit = match item.trim().to_ascii_lowercase().as_str() {
                "opacity" => 1,
                "background-color" => 2,
                "color" => 4,
                "border-top-color" => 8,
                "border-bottom-color" => 16,
                "border-left-color" => 32,
                "border-right-color" => 64,
                "border-top-width" => 2048,
                "border-bottom-width" => 4096,
                "border-left-width" => 8192,
                "border-right-width" => 16384,
                "border-radius" => 128,
                "transform" => 256,
                "background-position" => 512,
                "box-shadow" => 1024,
                "background-image" => 32768,
                "border-top-style" => 65536,
                "border-bottom-style" => 131072,
                "border-left-style" => 262144,
                "border-right-style" => 524288,
                "background-repeat" => 1048576,
                _ => return Err(ParseError::expected("a bounded transition-property list")),
            };
            if flags & bit != 0 {
                return Err(ParseError::expected("a bounded transition-property list"));
            }
            flags |= bit;
        }
        if !saw_item {
            return Err(ParseError::expected("a bounded transition-property list"));
        }
        Self::from_flags(flags)
            .ok_or_else(|| ParseError::expected("a supported transition-property list"))
    }
}

impl fmt::Display for TransitionProperty {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = match self {
            Self::All => return formatter.write_str("all"),
            Self::None => return formatter.write_str("none"),
            Self::Opacity => "opacity",
            Self::BackgroundColor => "background-color",
            Self::Color => "color",
            Self::BorderTopColor => "border-top-color",
            Self::BorderBottomColor => "border-bottom-color",
            Self::BorderLeftColor => "border-left-color",
            Self::BorderRightColor => "border-right-color",
            Self::BorderTopWidth => "border-top-width",
            Self::BorderBottomWidth => "border-bottom-width",
            Self::BorderLeftWidth => "border-left-width",
            Self::BorderRightWidth => "border-right-width",
            Self::BorderRadius => "border-radius",
            Self::Transform => "transform",
            Self::BackgroundPosition => "background-position",
            Self::BoxShadow => "box-shadow",
            Self::BackgroundImage => "background-image",
            Self::BorderTopStyle => "border-top-style",
            Self::BorderBottomStyle => "border-bottom-style",
            Self::BorderLeftStyle => "border-left-style",
            Self::BorderRightStyle => "border-right-style",
            Self::BackgroundRepeat => "background-repeat",
            Self::OpacityAndBackgroundColor => "opacity, background-color",
            Self::OpacityAndColor => "opacity, color",
            Self::BackgroundColorAndColor => "background-color, color",
            Self::OpacityAndBackgroundColorAndColor => "opacity, background-color, color",
            Self::List(flags) => {
                let mut first = true;
                for (bit, name) in [
                    (1, "opacity"),
                    (2, "background-color"),
                    (4, "color"),
                    (8, "border-top-color"),
                    (16, "border-bottom-color"),
                    (32, "border-left-color"),
                    (64, "border-right-color"),
                    (2048, "border-top-width"),
                    (4096, "border-bottom-width"),
                    (8192, "border-left-width"),
                    (16384, "border-right-width"),
                    (128, "border-radius"),
                    (256, "transform"),
                    (512, "background-position"),
                    (1024, "box-shadow"),
                    (32768, "background-image"),
                    (65536, "border-top-style"),
                    (131072, "border-bottom-style"),
                    (262144, "border-left-style"),
                    (524288, "border-right-style"),
                    (1048576, "background-repeat"),
                ] {
                    if flags & bit == 0 {
                        continue;
                    }
                    if !first {
                        formatter.write_str(", ")?;
                    }
                    formatter.write_str(name)?;
                    first = false;
                }
                return Ok(());
            },
        };
        formatter.write_str(names)
    }
}

impl TransitionProperty {
    fn from_flags(flags: u32) -> Option<Self> {
        Some(match flags {
            1 => Self::Opacity,
            2 => Self::BackgroundColor,
            4 => Self::Color,
            8 => Self::BorderTopColor,
            16 => Self::BorderBottomColor,
            32 => Self::BorderLeftColor,
            64 => Self::BorderRightColor,
            2048 => Self::BorderTopWidth,
            4096 => Self::BorderBottomWidth,
            8192 => Self::BorderLeftWidth,
            16384 => Self::BorderRightWidth,
            128 => Self::BorderRadius,
            256 => Self::Transform,
            512 => Self::BackgroundPosition,
            1024 => Self::BoxShadow,
            32768 => Self::BackgroundImage,
            65536 => Self::BorderTopStyle,
            131072 => Self::BorderBottomStyle,
            262144 => Self::BorderLeftStyle,
            524288 => Self::BorderRightStyle,
            1048576 => Self::BackgroundRepeat,
            3 => Self::OpacityAndBackgroundColor,
            5 => Self::OpacityAndColor,
            6 => Self::BackgroundColorAndColor,
            7 => Self::OpacityAndBackgroundColorAndColor,
            _ if flags != 0 => Self::List(flags),
            _ => return None,
        })
    }

    fn includes_flag(self, bit: u32) -> bool {
        matches!(self, Self::All) || self.flags() & bit != 0
    }

    pub fn includes_opacity(self) -> bool {
        self.includes_flag(1)
    }

    pub fn includes_background_color(self) -> bool {
        self.includes_flag(2)
    }

    pub fn includes_color(self) -> bool {
        self.includes_flag(4)
    }

    pub fn includes_border_top_color(self) -> bool {
        self.includes_flag(8)
    }

    pub fn includes_border_bottom_color(self) -> bool {
        self.includes_flag(16)
    }

    pub fn includes_border_left_color(self) -> bool {
        self.includes_flag(32)
    }

    pub fn includes_border_right_color(self) -> bool {
        self.includes_flag(64)
    }

    pub fn includes_border_top_width(self) -> bool {
        self.includes_flag(2048)
    }

    pub fn includes_border_bottom_width(self) -> bool {
        self.includes_flag(4096)
    }

    pub fn includes_border_left_width(self) -> bool {
        self.includes_flag(8192)
    }

    pub fn includes_border_right_width(self) -> bool {
        self.includes_flag(16384)
    }

    pub fn includes_border_radius(self) -> bool {
        self.includes_flag(128)
    }

    pub fn includes_transform(self) -> bool {
        self.includes_flag(256)
    }

    pub fn includes_background_position(self) -> bool {
        self.includes_flag(512)
    }

    pub fn includes_box_shadow(self) -> bool {
        self.includes_flag(1024)
    }

    pub fn includes_background_image(self) -> bool {
        self.includes_flag(32768)
    }

    pub fn includes_border_top_style(self) -> bool {
        self.includes_flag(65536)
    }

    pub fn includes_border_bottom_style(self) -> bool {
        self.includes_flag(131072)
    }

    pub fn includes_border_left_style(self) -> bool {
        self.includes_flag(262144)
    }

    pub fn includes_border_right_style(self) -> bool {
        self.includes_flag(524288)
    }

    /// Every longhand the retained transition clock may drive (harvest H2).
    /// The `border-radius` flag covers its four corner longhands.
    pub const TRANSITIONABLE: &'static [crate::PropertyId] = &[
        crate::PropertyId::Opacity,
        crate::PropertyId::BackgroundColor,
        crate::PropertyId::Color,
        crate::PropertyId::BorderTopColor,
        crate::PropertyId::BorderBottomColor,
        crate::PropertyId::BorderLeftColor,
        crate::PropertyId::BorderRightColor,
        crate::PropertyId::BorderTopWidth,
        crate::PropertyId::BorderBottomWidth,
        crate::PropertyId::BorderLeftWidth,
        crate::PropertyId::BorderRightWidth,
        crate::PropertyId::BorderTopStyle,
        crate::PropertyId::BorderBottomStyle,
        crate::PropertyId::BorderLeftStyle,
        crate::PropertyId::BorderRightStyle,
        crate::PropertyId::BorderTopLeftRadius,
        crate::PropertyId::BorderTopRightRadius,
        crate::PropertyId::BorderBottomRightRadius,
        crate::PropertyId::BorderBottomLeftRadius,
        crate::PropertyId::Transform,
        crate::PropertyId::BackgroundPosition,
        crate::PropertyId::BoxShadow,
        crate::PropertyId::BackgroundImage,
        crate::PropertyId::BackgroundRepeat,
    ];

    /// Whether this transition-property value accepts one longhand
    /// (harvest H2: the generic form of the `includes_*` family).
    pub fn includes_property(self, property: crate::PropertyId) -> bool {
        use crate::PropertyId as P;
        match property {
            P::Opacity => self.includes_opacity(),
            P::BackgroundColor => self.includes_background_color(),
            P::Color => self.includes_color(),
            P::BorderTopColor => self.includes_border_top_color(),
            P::BorderBottomColor => self.includes_border_bottom_color(),
            P::BorderLeftColor => self.includes_border_left_color(),
            P::BorderRightColor => self.includes_border_right_color(),
            P::BorderTopWidth => self.includes_border_top_width(),
            P::BorderBottomWidth => self.includes_border_bottom_width(),
            P::BorderLeftWidth => self.includes_border_left_width(),
            P::BorderRightWidth => self.includes_border_right_width(),
            P::BorderTopStyle => self.includes_border_top_style(),
            P::BorderBottomStyle => self.includes_border_bottom_style(),
            P::BorderLeftStyle => self.includes_border_left_style(),
            P::BorderRightStyle => self.includes_border_right_style(),
            P::BorderTopLeftRadius
            | P::BorderTopRightRadius
            | P::BorderBottomRightRadius
            | P::BorderBottomLeftRadius => self.includes_border_radius(),
            P::Transform => self.includes_transform(),
            P::BackgroundPosition => self.includes_background_position(),
            P::BoxShadow => self.includes_box_shadow(),
            P::BackgroundImage => self.includes_background_image(),
            P::BackgroundRepeat => self.includes_background_repeat(),
            _ => false,
        }
    }

    pub fn includes_background_repeat(self) -> bool {
        self.includes_flag(1048576)
    }

    fn flags(self) -> u32 {
        match self {
            Self::All | Self::None => 0,
            Self::Opacity => 1,
            Self::BackgroundColor => 2,
            Self::Color => 4,
            Self::BorderTopColor => 8,
            Self::BorderBottomColor => 16,
            Self::BorderLeftColor => 32,
            Self::BorderRightColor => 64,
            Self::BorderTopWidth => 2048,
            Self::BorderBottomWidth => 4096,
            Self::BorderLeftWidth => 8192,
            Self::BorderRightWidth => 16384,
            Self::BorderRadius => 128,
            Self::Transform => 256,
            Self::BackgroundPosition => 512,
            Self::BoxShadow => 1024,
            Self::BackgroundImage => 32768,
            Self::BorderTopStyle => 65536,
            Self::BorderBottomStyle => 131072,
            Self::BorderLeftStyle => 262144,
            Self::BorderRightStyle => 524288,
            Self::BackgroundRepeat => 1048576,
            Self::List(flags) => flags,
            Self::OpacityAndBackgroundColor => 3,
            Self::OpacityAndColor => 5,
            Self::BackgroundColorAndColor => 6,
            Self::OpacityAndBackgroundColorAndColor => 7,
        }
    }

    pub(crate) fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::All, _) | (_, Self::All) => Self::All,
            (Self::None, value) | (value, Self::None) => value,
            (left, right) if left == right => left,
            (left, right) => Self::from_flags(left.flags() | right.flags()).unwrap_or(Self::All),
        }
    }
}

/// Timing functions used by the retained single-animation lane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TimingFunction {
    Linear,
    Ease,
    EaseIn,
    EaseOut,
    EaseInOut,
    CubicBezier(f32, f32, f32, f32),
}

impl FromStr for TimingFunction {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("linear") {
            return Ok(Self::Linear);
        }
        if input.eq_ignore_ascii_case("ease") {
            return Ok(Self::Ease);
        }
        if input.eq_ignore_ascii_case("ease-in") {
            return Ok(Self::EaseIn);
        }
        if input.eq_ignore_ascii_case("ease-out") {
            return Ok(Self::EaseOut);
        }
        if input.eq_ignore_ascii_case("ease-in-out") {
            return Ok(Self::EaseInOut);
        }

        let lowered = input.to_ascii_lowercase();
        let arguments = lowered
            .strip_prefix("cubic-bezier(")
            .and_then(|value| value.strip_suffix(')'))
            .ok_or_else(|| ParseError::expected("an easing keyword or cubic-bezier()"))?;
        let values = arguments
            .split(',')
            .map(|value| value.trim().parse::<f32>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| ParseError::expected("four cubic-bezier numbers"))?;
        let [x1, y1, x2, y2] = values.as_slice() else {
            return Err(ParseError::expected("four cubic-bezier numbers"));
        };
        if !values.iter().all(|value| value.is_finite())
            || !(0.0..=1.0).contains(x1)
            || !(0.0..=1.0).contains(x2)
        {
            return Err(ParseError::expected(
                "finite cubic-bezier numbers with x coordinates from zero to one",
            ));
        }
        Ok(Self::CubicBezier(*x1, *y1, *x2, *y2))
    }
}

impl fmt::Display for TimingFunction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Linear => formatter.write_str("linear"),
            Self::Ease => formatter.write_str("ease"),
            Self::EaseIn => formatter.write_str("ease-in"),
            Self::EaseOut => formatter.write_str("ease-out"),
            Self::EaseInOut => formatter.write_str("ease-in-out"),
            Self::CubicBezier(x1, y1, x2, y2) => write!(
                formatter,
                "cubic-bezier({}, {}, {}, {})",
                format_number(x1),
                format_number(y1),
                format_number(x2),
                format_number(y2)
            ),
        }
    }
}

impl TimingFunction {
    pub fn sample(self, progress: f32) -> f32 {
        let progress = progress.clamp(0.0, 1.0);
        match self {
            Self::Linear => progress,
            Self::Ease => cubic_bezier(progress, 0.25, 0.1, 0.25, 1.0),
            Self::EaseIn => cubic_bezier(progress, 0.42, 0.0, 1.0, 1.0),
            Self::EaseOut => cubic_bezier(progress, 0.0, 0.0, 0.58, 1.0),
            Self::EaseInOut => cubic_bezier(progress, 0.42, 0.0, 0.58, 1.0),
            Self::CubicBezier(x1, y1, x2, y2) => cubic_bezier(progress, x1, y1, x2, y2),
        }
    }
}

fn cubic_bezier(progress: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    if progress <= 0.0 {
        return 0.0;
    }
    if progress >= 1.0 {
        return 1.0;
    }
    let mut low = 0.0;
    let mut high = 1.0;
    for _ in 0..16 {
        let middle = (low + high) * 0.5;
        if bezier_axis(middle, x1, x2) < progress {
            low = middle;
        } else {
            high = middle;
        }
    }
    bezier_axis((low + high) * 0.5, y1, y2)
}

fn bezier_axis(t: f32, first: f32, second: f32) -> f32 {
    let inverse = 1.0 - t;
    3.0 * inverse * inverse * t * first + 3.0 * inverse * t * t * second + t * t * t
}

keyword_value! {
    /// CSS border line style.
    pub enum BorderStyle {
        None => "none",
        Hidden => "hidden",
        Dotted => "dotted",
        Dashed => "dashed",
        Solid => "solid",
        Double => "double",
        Groove => "groove",
        Ridge => "ridge",
        Inset => "inset",
        Outset => "outset",
    }
}

impl BorderStyle {
    /// Border styles are discrete in CSS. The bounded retained transition
    /// switches to the target at the midpoint of the clock.
    pub fn interpolate(self, other: Self, progress: f32) -> Self {
        if progress.clamp(0.0, 1.0) < 0.5 {
            self
        } else {
            other
        }
    }
}

keyword_value! {
    /// Display keywords required by the Cambium lane and baseline UA sheet.
    pub enum Display {
        None => "none",
        Contents => "contents",
        Inline => "inline",
        Block => "block",
        FlowRoot => "flow-root",
        ListItem => "list-item",
        InlineBlock => "inline-block",
        Flex => "flex",
        Grid => "grid",
        Table => "table",
        InlineTable => "inline-table",
        TableRowGroup => "table-row-group",
        TableHeaderGroup => "table-header-group",
        TableFooterGroup => "table-footer-group",
        TableRow => "table-row",
        TableCell => "table-cell",
        TableColumnGroup => "table-column-group",
        TableColumn => "table-column",
        TableCaption => "table-caption",
    }
}

keyword_value! {
    /// Physical floating directions lowered into Buckram's block input.
    pub enum Float {
        None => "none",
        Left => "left",
        Right => "right",
    }
}

keyword_value! {
    /// The box-valued `shape-outside` forms admitted by row 12.
    /// Horizontal layout honors linear circular corner radii. Nonlinear radius
    /// math uses the default margin-box float area; basic shapes, images,
    /// elliptical corner pairs, multi-contour curved line retry, and vertical
    /// float-area transforms remain deferred.
    pub enum ShapeOutside {
        None => "none",
        MarginBox => "margin-box",
        BorderBox => "border-box",
        PaddingBox => "padding-box",
        ContentBox => "content-box",
    }
}

keyword_value! {
    /// CSS box sizing mode used by the layout adapter.
    pub enum BoxSizing {
        ContentBox => "content-box",
        BorderBox => "border-box",
    }
}

keyword_value! {
    /// Physical sides which an in-flow block must clear.
    pub enum Clear {
        None => "none",
        Left => "left",
        Right => "right",
        Both => "both",
    }
}

keyword_value! {
    /// `table-layout`: which of CSS 2.1 section 17.5.2's two column-width
    /// algorithms a table uses.
    pub enum TableLayout {
        Auto => "auto",
        Fixed => "fixed",
    }
}

keyword_value! {
    /// CSS 2.1's separate and collapsed table border models.
    pub enum BorderCollapse {
        Separate => "separate",
        Collapse => "collapse",
    }
}

keyword_value! {
    /// Caption placement relative to its table grid.
    pub enum CaptionSide {
        Top => "top",
        Bottom => "bottom",
    }
}

keyword_value! {
    /// Whether an empty table cell retains its background and border.
    pub enum EmptyCells {
        Show => "show",
        Hide => "hide",
    }
}

keyword_value! {
    /// Axes exposed as query-container size bases.
    pub enum ContainerType {
        Normal => "normal",
        Size => "size",
        InlineSize => "inline-size",
    }
}

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

/// CSS `text-transform` as its orthogonal case, width, and kana flags.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TextTransform {
    pub case: TextTransformCase,
    pub full_width: bool,
    pub full_size_kana: bool,
}

impl TextTransform {
    pub const NONE: Self = Self {
        case: TextTransformCase::None,
        full_width: false,
        full_size_kana: false,
    };

    pub const fn is_none(self) -> bool {
        matches!(
            self.case,
            TextTransformCase::None | TextTransformCase::MathAuto
        ) && !self.full_width
            && !self.full_size_kana
    }
}

impl FromStr for TextTransform {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        const EXPECTED: ParseError = ParseError::expected(
            "none | [ capitalize | uppercase | lowercase ] || full-width || full-size-kana | math-auto",
        );
        let tokens = input
            .split_ascii_whitespace()
            .map(str::to_ascii_lowercase)
            .collect::<Vec<_>>();
        if tokens.is_empty() {
            return Err(EXPECTED);
        }
        if tokens.len() == 1
            && let Ok(sole) = tokens[0].parse::<TextTransformCase>()
            && matches!(sole, TextTransformCase::None | TextTransformCase::MathAuto)
        {
            return Ok(Self {
                case: sole,
                ..Self::NONE
            });
        }
        let mut value = Self::NONE;
        let mut seen_case = false;
        for token in &tokens {
            match token.as_str() {
                "capitalize" | "uppercase" | "lowercase" if !seen_case => {
                    seen_case = true;
                    value.case = token.parse().map_err(|_| EXPECTED)?;
                },
                "full-width" if !value.full_width => value.full_width = true,
                "full-size-kana" if !value.full_size_kana => value.full_size_kana = true,
                _ => return Err(EXPECTED),
            }
        }
        Ok(value)
    }
}

impl fmt::Display for TextTransform {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_none() {
            return self.case.fmt(formatter);
        }
        let mut parts = Vec::with_capacity(3);
        if !matches!(self.case, TextTransformCase::None) {
            parts.push(self.case.to_string());
        }
        if self.full_width {
            parts.push("full-width".to_owned());
        }
        if self.full_size_kana {
            parts.push("full-size-kana".to_owned());
        }
        formatter.write_str(&parts.join(" "))
    }
}

/// Split on top-level whitespace while keeping function arguments intact.
fn split_top_level(input: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = None;
    for (index, character) in input.char_indices() {
        match character {
            '(' => {
                depth += 1;
                start.get_or_insert(index);
            },
            ')' => {
                depth = depth.saturating_sub(1);
                start.get_or_insert(index);
            },
            character if character.is_ascii_whitespace() && depth == 0 => {
                if let Some(begin) = start.take() {
                    parts.push(&input[begin..index]);
                }
            },
            _ => {
                start.get_or_insert(index);
            },
        }
    }
    if let Some(begin) = start {
        parts.push(&input[begin..]);
    }
    parts
}

/// CSS `hanging-punctuation`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HangingPunctuation {
    pub first: bool,
    pub force_end: bool,
    pub allow_end: bool,
    pub last: bool,
}

impl HangingPunctuation {
    pub const NONE: Self = Self {
        first: false,
        force_end: false,
        allow_end: false,
        last: false,
    };
}

impl FromStr for HangingPunctuation {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        const EXPECTED: ParseError =
            ParseError::expected("none or first || [ force-end | allow-end ] || last");
        if input.trim().eq_ignore_ascii_case("none") {
            return Ok(Self::NONE);
        }
        let mut value = Self::NONE;
        for token in input.split_ascii_whitespace() {
            match token.to_ascii_lowercase().as_str() {
                "first" if !value.first => value.first = true,
                "force-end" if !value.force_end && !value.allow_end => value.force_end = true,
                "allow-end" if !value.allow_end && !value.force_end => value.allow_end = true,
                "last" if !value.last => value.last = true,
                _ => return Err(EXPECTED),
            }
        }
        if value == Self::NONE {
            return Err(EXPECTED);
        }
        Ok(value)
    }
}

impl fmt::Display for HangingPunctuation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if *self == Self::NONE {
            return formatter.write_str("none");
        }
        let mut parts = Vec::new();
        if self.first {
            parts.push("first");
        }
        if self.force_end {
            parts.push("force-end");
        }
        if self.allow_end {
            parts.push("allow-end");
        }
        if self.last {
            parts.push("last");
        }
        formatter.write_str(&parts.join(" "))
    }
}

/// CSS `text-indent`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextIndent {
    pub length: LengthPercentage,
    pub hanging: bool,
    pub each_line: bool,
}

impl TextIndent {
    pub const ZERO: Self = Self {
        length: LengthPercentage::ZERO,
        hanging: false,
        each_line: false,
    };
}

impl FromStr for TextIndent {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        const EXPECTED: ParseError =
            ParseError::expected("[ <length-percentage> ] && hanging? && each-line?");
        let mut value = Self::ZERO;
        let mut length = None;
        for token in split_top_level(input) {
            match token.to_ascii_lowercase().as_str() {
                "hanging" if !value.hanging => value.hanging = true,
                "each-line" if !value.each_line => value.each_line = true,
                _ if length.is_none() => {
                    length = Some(token.parse::<LengthPercentage>().map_err(|_| EXPECTED)?);
                },
                _ => return Err(EXPECTED),
            }
        }
        value.length = length.ok_or(EXPECTED)?;
        Ok(value)
    }
}

impl fmt::Display for TextIndent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.length.fmt(formatter)?;
        if self.hanging {
            formatter.write_str(" hanging")?;
        }
        if self.each_line {
            formatter.write_str(" each-line")?;
        }
        Ok(())
    }
}

/// CSS `tab-size`: either a space advance multiplier or an absolute length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TabSize {
    Number(f32),
    Length(Length),
}

impl TabSize {
    pub const DEFAULT: Self = Self::Number(8.0);
}

impl FromStr for TabSize {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        const EXPECTED: ParseError = ParseError::expected("a non-negative number or length");
        let input = input.trim();
        if let Ok(number) = input.parse::<f32>() {
            return if number.is_finite() && number >= 0.0 {
                Ok(Self::Number(number))
            } else {
                Err(EXPECTED)
            };
        }
        let length = input.parse::<Length>().map_err(|_| EXPECTED)?;
        if length.value < 0.0 {
            return Err(EXPECTED);
        }
        Ok(Self::Length(length))
    }
}

impl fmt::Display for TabSize {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(number) => formatter.write_str(&format_number(*number)),
            Self::Length(length) => length.fmt(formatter),
        }
    }
}

keyword_value! {
    /// Flex and grid main/cross-axis alignment keywords.
    pub enum Alignment {
        Auto => "auto",
        Start => "start",
        End => "end",
        SelfStart => "self-start",
        SelfEnd => "self-end",
        FlexStart => "flex-start",
        FlexEnd => "flex-end",
        Center => "center",
        Baseline => "baseline",
        Stretch => "stretch",
        SpaceBetween => "space-between",
        SpaceAround => "space-around",
        SpaceEvenly => "space-evenly",
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VerticalAlign {
    Baseline,
    Sub,
    Super,
    TextTop,
    TextBottom,
    Middle,
    /// HTML's legacy `align=middle|center` behavior for replaced content:
    /// align the element's center with the parent's baseline. This differs
    /// from CSS `middle`, which also includes half the parent's x-height.
    MiddleWithBaseline,
    Top,
    Bottom,
    Length(LengthPercentage),
}

impl FromStr for VerticalAlign {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input.trim().to_ascii_lowercase().as_str() {
            "baseline" => Ok(Self::Baseline),
            "sub" => Ok(Self::Sub),
            "super" => Ok(Self::Super),
            "text-top" => Ok(Self::TextTop),
            "text-bottom" => Ok(Self::TextBottom),
            "middle" => Ok(Self::Middle),
            "top" => Ok(Self::Top),
            "bottom" => Ok(Self::Bottom),
            _ => input.parse().map(Self::Length),
        }
    }
}

impl fmt::Display for VerticalAlign {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Baseline => formatter.write_str("baseline"),
            Self::Sub => formatter.write_str("sub"),
            Self::Super => formatter.write_str("super"),
            Self::TextTop => formatter.write_str("text-top"),
            Self::TextBottom => formatter.write_str("text-bottom"),
            Self::Middle => formatter.write_str("middle"),
            Self::MiddleWithBaseline => formatter.write_str("middle"),
            Self::Top => formatter.write_str("top"),
            Self::Bottom => formatter.write_str("bottom"),
            Self::Length(value) => value.fmt(formatter),
        }
    }
}

keyword_value! {
    pub enum FlexDirection {
        Row => "row",
        RowReverse => "row-reverse",
        Column => "column",
        ColumnReverse => "column-reverse",
    }
}

keyword_value! {
    pub enum FlexWrap {
        NoWrap => "nowrap",
        Wrap => "wrap",
        WrapReverse => "wrap-reverse",
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GridAutoFlow {
    Row,
    Column,
    RowDense,
    ColumnDense,
}

impl FromStr for GridAutoFlow {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input.trim().to_ascii_lowercase().as_str() {
            "row" => Ok(Self::Row),
            "column" => Ok(Self::Column),
            "row dense" => Ok(Self::RowDense),
            "column dense" => Ok(Self::ColumnDense),
            _ => Err(ParseError::expected("grid-auto-flow keywords")),
        }
    }
}

impl fmt::Display for GridAutoFlow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Row => "row",
            Self::Column => "column",
            Self::RowDense => "row dense",
            Self::ColumnDense => "column dense",
        })
    }
}

keyword_value! {
    pub enum FontStyle {
        Normal => "normal",
        Italic => "italic",
        Oblique => "oblique",
    }
}

keyword_value! {
    pub enum ListStyleType {
        None => "none",
        Disc => "disc",
        Decimal => "decimal",
    }
}

keyword_value! {
    pub enum Overflow {
        Visible => "visible",
        Hidden => "hidden",
        Clip => "clip",
        Scroll => "scroll",
        Auto => "auto",
    }
}

keyword_value! {
    pub enum Position {
        Static => "static",
        Relative => "relative",
        Absolute => "absolute",
        Sticky => "sticky",
        Fixed => "fixed",
    }
}

keyword_value! {
    pub enum TextWrapMode {
        Wrap => "wrap",
        Nowrap => "nowrap",
    }
}

keyword_value! {
    pub enum WhiteSpaceCollapse {
        Collapse => "collapse",
        Discard => "discard",
        Preserve => "preserve",
        PreserveBreaks => "preserve-breaks",
        PreserveSpaces => "preserve-spaces",
        BreakSpaces => "break-spaces",
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BorderWidth {
    Thin,
    Medium,
    Thick,
    Length(Length),
}

impl BorderWidth {
    /// Interpolate the bounded line-width family used by the border paint
    /// lane. Fixed keyword widths participate in the px family; length
    /// endpoints interpolate only when their units match.
    pub fn interpolate(self, other: Self, progress: f32) -> Self {
        let progress = progress.clamp(0.0, 1.0);
        match (self, other) {
            (Self::Length(from), Self::Length(to)) if from.unit == to.unit => {
                Self::Length(Length {
                    value: from.value + (to.value - from.value) * progress,
                    unit: from.unit,
                })
            },
            (from, to) => match (from.computed_px(), to.computed_px()) {
                (Some(from), Some(to)) => Self::Length(Length::px(from + (to - from) * progress)),
                _ if progress < 0.5 => from,
                _ => to,
            },
        }
    }

    fn computed_px(self) -> Option<f32> {
        match self {
            Self::Thin => Some(1.0),
            Self::Medium => Some(3.0),
            Self::Thick => Some(5.0),
            Self::Length(length) if length.unit == super::LengthUnit::Px => Some(length.value),
            _ => None,
        }
    }
}

impl FromStr for BorderWidth {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input.trim().to_ascii_lowercase().as_str() {
            "thin" => Ok(Self::Thin),
            "medium" => Ok(Self::Medium),
            "thick" => Ok(Self::Thick),
            _ => input.parse::<Length>().map(Self::Length),
        }
    }
}

impl fmt::Display for BorderWidth {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Thin => formatter.write_str("thin"),
            Self::Medium => formatter.write_str("medium"),
            Self::Thick => formatter.write_str("thick"),
            Self::Length(length) => length.fmt(formatter),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FontFamily {
    UserAgentDefault,
    SystemUi,
    Named(Box<str>),
    List(Box<str>),
}

impl FromStr for FontFamily {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("system-ui") {
            return Ok(Self::SystemUi);
        }
        if input.eq_ignore_ascii_case("depends-on-user-agent") {
            return Ok(Self::UserAgentDefault);
        }
        if input.is_empty() {
            return Err(ParseError::expected("a font family list"));
        }
        // Parley accepts a CSS font-family source string and performs ordered
        // lookup itself. Retain a list as CSS source so its commas and quoted
        // multi-word names arrive intact; keep the established compact value
        // for one family.
        if input.contains(',') {
            if split_top_level_commas(input)
                .iter()
                .any(|family| family.trim().is_empty())
            {
                return Err(ParseError::expected("a nonempty font family list"));
            }
            return Ok(Self::List(input.into()));
        }
        let unquoted = input
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .or_else(|| {
                input
                    .strip_prefix('\'')
                    .and_then(|value| value.strip_suffix('\''))
            })
            .unwrap_or(input);
        Ok(Self::Named(unquoted.into()))
    }
}

impl fmt::Display for FontFamily {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UserAgentDefault => formatter.write_str("depends-on-user-agent"),
            Self::SystemUi => formatter.write_str("system-ui"),
            Self::Named(name) if name.contains(char::is_whitespace) => {
                write!(formatter, "\"{name}\"")
            },
            Self::Named(name) => formatter.write_str(name),
            Self::List(source) => formatter.write_str(source),
        }
    }
}

/// One explicit OpenType feature setting retained from CSS.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FontFeatureSetting {
    pub tag: [u8; 4],
    pub value: u16,
}

/// The low-level `font-feature-settings` property. Higher-level font variant
/// properties are kept separate until the shaping boundary applies CSS's
/// precedence order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FontFeatureSettings {
    Normal,
    Settings(Box<[FontFeatureSetting]>),
}

impl FontFeatureSettings {
    pub fn settings(&self) -> &[FontFeatureSetting] {
        match self {
            Self::Normal => &[],
            Self::Settings(settings) => settings,
        }
    }
}

impl FromStr for FontFeatureSettings {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("normal") {
            return Ok(Self::Normal);
        }
        let mut settings = Vec::new();
        for raw in split_top_level_commas(input) {
            let raw = raw.trim();
            let Some(quote) = raw
                .chars()
                .next()
                .filter(|quote| matches!(quote, '\'' | '"'))
            else {
                return Err(ParseError::expected(
                    "a quoted four-byte OpenType feature tag",
                ));
            };
            let after_quote = &raw[quote.len_utf8()..];
            let Some(close) = after_quote.find(quote) else {
                return Err(ParseError::expected("a closed OpenType feature tag"));
            };
            let tag = &after_quote[..close];
            let bytes = tag.as_bytes();
            if bytes.len() != 4
                || !bytes
                    .iter()
                    .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
            {
                return Err(ParseError::expected("a four-byte OpenType feature tag"));
            }
            let value = match after_quote[close + quote.len_utf8()..]
                .trim()
                .to_ascii_lowercase()
                .as_str()
            {
                "" | "on" => 1,
                "off" => 0,
                value => value
                    .parse::<u16>()
                    .map_err(|_| ParseError::expected("on, off, or a feature value"))?,
            };
            settings.push(FontFeatureSetting {
                tag: [bytes[0], bytes[1], bytes[2], bytes[3]],
                value,
            });
        }
        if settings.is_empty() {
            return Err(ParseError::expected("normal or a font feature list"));
        }
        Ok(Self::Settings(settings.into_boxed_slice()))
    }
}

impl fmt::Display for FontFeatureSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Normal => formatter.write_str("normal"),
            Self::Settings(settings) => {
                for (index, setting) in settings.iter().enumerate() {
                    if index != 0 {
                        formatter.write_str(", ")?;
                    }
                    let tag = std::str::from_utf8(&setting.tag).unwrap_or("????");
                    match setting.value {
                        0 => write!(formatter, "\"{tag}\" off")?,
                        1 => write!(formatter, "\"{tag}\" on")?,
                        value => write!(formatter, "\"{tag}\" {value}")?,
                    }
                }
                Ok(())
            },
        }
    }
}

/// Independent overrides represented by `font-variant-ligatures`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FontVariantLigatures {
    common: Option<bool>,
    discretionary: Option<bool>,
    historical: Option<bool>,
    contextual: Option<bool>,
}

impl FontVariantLigatures {
    pub const NORMAL: Self = Self {
        common: None,
        discretionary: None,
        historical: None,
        contextual: None,
    };

    pub const fn common(self) -> Option<bool> {
        self.common
    }

    pub const fn discretionary(self) -> Option<bool> {
        self.discretionary
    }

    pub const fn historical(self) -> Option<bool> {
        self.historical
    }

    pub const fn contextual(self) -> Option<bool> {
        self.contextual
    }
}

impl FromStr for FontVariantLigatures {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("normal") {
            return Ok(Self::NORMAL);
        }
        if input.eq_ignore_ascii_case("none") {
            return Ok(Self {
                common: Some(false),
                discretionary: Some(false),
                historical: Some(false),
                contextual: Some(false),
            });
        }
        let mut result = Self::NORMAL;
        for keyword in input.split_ascii_whitespace() {
            let slot_and_value = match keyword.to_ascii_lowercase().as_str() {
                "common-ligatures" => (&mut result.common, true),
                "no-common-ligatures" => (&mut result.common, false),
                "discretionary-ligatures" => (&mut result.discretionary, true),
                "no-discretionary-ligatures" => (&mut result.discretionary, false),
                "historical-ligatures" => (&mut result.historical, true),
                "no-historical-ligatures" => (&mut result.historical, false),
                "contextual" => (&mut result.contextual, true),
                "no-contextual" => (&mut result.contextual, false),
                _ => return Err(ParseError::expected("a font-variant-ligatures keyword")),
            };
            let (slot, value) = slot_and_value;
            if slot.replace(value).is_some() {
                return Err(ParseError::expected("one keyword per ligature class"));
            }
        }
        if result == Self::NORMAL {
            return Err(ParseError::expected("normal, none, or ligature keywords"));
        }
        Ok(result)
    }
}

impl fmt::Display for FontVariantLigatures {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if *self == Self::NORMAL {
            return formatter.write_str("normal");
        }
        if *self
            == (Self {
                common: Some(false),
                discretionary: Some(false),
                historical: Some(false),
                contextual: Some(false),
            })
        {
            return formatter.write_str("none");
        }
        let mut first = true;
        for (value, enabled, disabled) in [
            (self.common, "common-ligatures", "no-common-ligatures"),
            (
                self.discretionary,
                "discretionary-ligatures",
                "no-discretionary-ligatures",
            ),
            (
                self.historical,
                "historical-ligatures",
                "no-historical-ligatures",
            ),
            (self.contextual, "contextual", "no-contextual"),
        ] {
            let Some(value) = value else {
                continue;
            };
            if !first {
                formatter.write_str(" ")?;
            }
            formatter.write_str(if value { enabled } else { disabled })?;
            first = false;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FontSize {
    XXSmall,
    XSmall,
    Small,
    Medium,
    Large,
    XLarge,
    XXLarge,
    XXXLarge,
    Value(LengthPercentage),
}

impl FontSize {
    /// Resolve CSS's absolute-size keywords against Livery's 16px medium.
    pub const fn absolute_px(self) -> Option<f32> {
        match self {
            Self::XXSmall => Some(9.6),
            Self::XSmall => Some(12.0),
            Self::Small => Some(13.333_333),
            Self::Medium => Some(16.0),
            Self::Large => Some(18.0),
            Self::XLarge => Some(24.0),
            Self::XXLarge => Some(32.0),
            Self::XXXLarge => Some(48.0),
            Self::Value(_) => None,
        }
    }
}
impl FromStr for FontSize {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input.trim().to_ascii_lowercase().as_str() {
            "xx-small" => Ok(Self::XXSmall),
            "x-small" => Ok(Self::XSmall),
            "small" => Ok(Self::Small),
            "medium" => Ok(Self::Medium),
            "large" => Ok(Self::Large),
            "x-large" => Ok(Self::XLarge),
            "xx-large" => Ok(Self::XXLarge),
            "xxx-large" => Ok(Self::XXXLarge),
            _ => input.parse::<LengthPercentage>().map(Self::Value),
        }
    }
}

impl fmt::Display for FontSize {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::XXSmall => formatter.write_str("xx-small"),
            Self::XSmall => formatter.write_str("x-small"),
            Self::Small => formatter.write_str("small"),
            Self::Medium => formatter.write_str("medium"),
            Self::Large => formatter.write_str("large"),
            Self::XLarge => formatter.write_str("x-large"),
            Self::XXLarge => formatter.write_str("xx-large"),
            Self::XXXLarge => formatter.write_str("xxx-large"),
            Self::Value(value) => value.fmt(formatter),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FontWeight {
    Normal,
    Bold,
    Bolder,
    Lighter,
    Number(u16),
}

impl FromStr for FontWeight {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input.trim().to_ascii_lowercase().as_str() {
            "normal" => Ok(Self::Normal),
            "bold" => Ok(Self::Bold),
            "bolder" => Ok(Self::Bolder),
            "lighter" => Ok(Self::Lighter),
            number => number
                .parse::<u16>()
                .ok()
                .filter(|number| (1..=1000).contains(number))
                .map(Self::Number)
                .ok_or_else(|| ParseError::expected("a font weight from 1 through 1000")),
        }
    }
}

impl fmt::Display for FontWeight {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Normal => formatter.write_str("normal"),
            Self::Bold => formatter.write_str("bold"),
            Self::Bolder => formatter.write_str("bolder"),
            Self::Lighter => formatter.write_str("lighter"),
            Self::Number(number) => number.fmt(formatter),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Size {
    Auto,
    None,
    MinContent,
    MaxContent,
    FitContent(LengthPercentage),
    Value(LengthPercentage),
}

impl FromStr for Size {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("auto") {
            return Ok(Self::Auto);
        }
        if input.eq_ignore_ascii_case("none") {
            return Ok(Self::None);
        }
        if input.eq_ignore_ascii_case("min-content") {
            return Ok(Self::MinContent);
        }
        if input.eq_ignore_ascii_case("max-content") {
            return Ok(Self::MaxContent);
        }
        if input.len() > 13
            && input[..12].eq_ignore_ascii_case("fit-content(")
            && input.ends_with(')')
        {
            return input[12..input.len() - 1]
                .parse::<LengthPercentage>()
                .map(Self::FitContent);
        }
        input.parse::<LengthPercentage>().map(Self::Value)
    }
}

impl fmt::Display for Size {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => formatter.write_str("auto"),
            Self::None => formatter.write_str("none"),
            Self::MinContent => formatter.write_str("min-content"),
            Self::MaxContent => formatter.write_str("max-content"),
            Self::FitContent(value) => write!(formatter, "fit-content({value})"),
            Self::Value(value) => value.fmt(formatter),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GridTrack {
    Auto,
    MinContent,
    MaxContent,
    Length(Length),
    Percent(f32),
    Fr(f32),
}

#[derive(Clone, Debug, PartialEq)]
pub enum GridTemplate {
    None,
    Tracks(Vec<GridTrack>),
}

impl FromStr for GridTemplate {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("none") {
            return Ok(Self::None);
        }
        let mut tracks = Vec::new();
        for component in input.split_ascii_whitespace() {
            let track = if component.eq_ignore_ascii_case("auto") {
                GridTrack::Auto
            } else if component.eq_ignore_ascii_case("min-content") {
                GridTrack::MinContent
            } else if component.eq_ignore_ascii_case("max-content") {
                GridTrack::MaxContent
            } else if let Some(value) = component.strip_suffix("fr") {
                GridTrack::Fr(parse_non_negative(value)?)
            } else if let Some(value) = component.strip_suffix('%') {
                GridTrack::Percent(parse_non_negative(value)? / 100.0)
            } else {
                GridTrack::Length(
                    component
                        .parse::<Length>()
                        .map_err(|_| ParseError::expected("grid track sizes"))?,
                )
            };
            tracks.push(track);
        }
        if tracks.is_empty() {
            Err(ParseError::expected("one or more grid tracks"))
        } else {
            Ok(Self::Tracks(tracks))
        }
    }
}

impl fmt::Display for GridTemplate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => formatter.write_str("none"),
            Self::Tracks(tracks) => {
                for (index, track) in tracks.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(" ")?;
                    }
                    track.fmt(formatter)?;
                }
                Ok(())
            },
        }
    }
}

impl fmt::Display for GridTrack {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => formatter.write_str("auto"),
            Self::MinContent => formatter.write_str("min-content"),
            Self::MaxContent => formatter.write_str("max-content"),
            Self::Length(value) => value.fmt(formatter),
            Self::Percent(value) => write!(formatter, "{}%", format_number(*value * 100.0)),
            Self::Fr(value) => write!(formatter, "{}fr", format_number(*value)),
        }
    }
}

fn parse_non_negative(input: &str) -> Result<f32, ParseError> {
    input
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or_else(|| ParseError::expected("a non-negative grid track number"))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GridPlacement {
    Auto,
    Line(i16),
    Span(u16),
}

impl FromStr for GridPlacement {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("auto") {
            return Ok(Self::Auto);
        }
        if let Some(span) = input.strip_prefix("span ") {
            return span
                .parse::<u16>()
                .ok()
                .filter(|value| *value > 0)
                .map(Self::Span)
                .ok_or_else(|| ParseError::expected("a positive grid span"));
        }
        input
            .parse::<i16>()
            .map(Self::Line)
            .map_err(|_| ParseError::expected("auto, span, or a grid line number"))
    }
}

impl fmt::Display for GridPlacement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => formatter.write_str("auto"),
            Self::Line(value) => value.fmt(formatter),
            Self::Span(value) => write!(formatter, "span {value}"),
        }
    }
}

/// CSS `aspect-ratio`, represented as width divided by height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AspectRatio {
    Auto,
    Ratio(f32),
    /// HTML dimension attributes contribute `auto <ratio>`. The operands are
    /// retained separately because HTML permits zero in this mapping even
    /// though an authored CSS `<ratio>` requires two positive numbers.
    AutoRatio {
        width: f32,
        height: f32,
    },
}

impl AspectRatio {
    /// Return the usable preferred ratio. Degenerate HTML ratios remain
    /// serializable computed values but do not enter layout arithmetic.
    pub fn preferred_ratio(self) -> Option<f32> {
        let ratio = match self {
            Self::Auto => return None,
            Self::Ratio(ratio) => ratio,
            Self::AutoRatio { width, height } => width / height,
        };
        ratio
            .is_finite()
            .then_some(ratio)
            .filter(|ratio| *ratio > 0.0)
    }

    pub const fn uses_natural_ratio(self) -> bool {
        matches!(self, Self::Auto | Self::AutoRatio { .. })
    }
}
impl FromStr for AspectRatio {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("auto") {
            return Ok(Self::Auto);
        }
        let lowercase = input.to_ascii_lowercase();
        let (auto, input) =
            if lowercase.starts_with("auto") && input[4..].starts_with(char::is_whitespace) {
                (true, input[4..].trim())
            } else if lowercase.ends_with("auto")
                && input[..input.len() - 4].ends_with(char::is_whitespace)
            {
                (true, input[..input.len() - 4].trim())
            } else {
                (false, input)
            };
        let (width, height) = input
            .split_once('/')
            .map_or((input, "1"), |(width, height)| {
                (width.trim(), height.trim())
            });
        let width = width
            .parse::<f32>()
            .ok()
            .filter(|value| value.is_finite() && *value > 0.0)
            .ok_or_else(|| ParseError::expected("a positive aspect-ratio"))?;
        let height = height
            .parse::<f32>()
            .ok()
            .filter(|value| value.is_finite() && *value > 0.0)
            .ok_or_else(|| ParseError::expected("a positive aspect-ratio"))?;
        if auto {
            Ok(Self::AutoRatio { width, height })
        } else {
            Ok(Self::Ratio(width / height))
        }
    }
}

impl fmt::Display for AspectRatio {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => formatter.write_str("auto"),
            Self::Ratio(value) => formatter.write_str(&format_number(*value)),
            Self::AutoRatio { width, height } => write!(
                formatter,
                "auto {} / {}",
                format_number(*width),
                format_number(*height)
            ),
        }
    }
}

macro_rules! auto_length_percentage {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub enum $name {
            Auto,
            Value(LengthPercentage),
        }

        impl FromStr for $name {
            type Err = ParseError;

            fn from_str(input: &str) -> Result<Self, Self::Err> {
                if input.trim().eq_ignore_ascii_case("auto") {
                    Ok(Self::Auto)
                } else {
                    input.parse::<LengthPercentage>().map(Self::Value)
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self {
                    Self::Auto => formatter.write_str("auto"),
                    Self::Value(value) => value.fmt(formatter),
                }
            }
        }
    };
}

auto_length_percentage!(Inset);
auto_length_percentage!(Margin);

/// A non-negative border corner radius component.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Radius(pub LengthPercentage);

impl Radius {
    pub const ZERO: Self = Self(LengthPercentage::ZERO);

    /// Interpolate the bounded radius forms used by the retained paint lane.
    /// Zero and a concrete length/percentage share the same scalar family;
    /// mixed non-zero units stay discrete until the broader value ratchet.
    pub fn interpolate(self, other: Self, progress: f32) -> Self {
        Self(self.0.interpolate(other.0, progress))
    }
}

impl FromStr for Radius {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let value = input.parse::<LengthPercentage>()?;
        let negative = match value {
            LengthPercentage::Zero => false,
            LengthPercentage::Length(length) => length.value < 0.0,
            LengthPercentage::Percentage(value) => value < 0.0,
            LengthPercentage::Calc(calc) => calc.px < 0.0 || calc.em < 0.0 || calc.rem < 0.0,
            LengthPercentage::Math(_) => false,
        };
        if negative {
            return Err(ParseError::expected("a non-negative border radius"));
        }
        Ok(Self(value))
    }
}

impl fmt::Display for Radius {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// A non-negative flex/grid gap component.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gap(pub LengthPercentage);

impl Gap {
    pub const ZERO: Self = Self(LengthPercentage::ZERO);
}

impl FromStr for Gap {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let value = input.parse::<LengthPercentage>()?;
        let negative = match value {
            LengthPercentage::Zero => false,
            LengthPercentage::Length(length) => length.value < 0.0,
            LengthPercentage::Percentage(value) => value < 0.0,
            LengthPercentage::Calc(calc) => calc.px < 0.0 || calc.em < 0.0 || calc.rem < 0.0,
            LengthPercentage::Math(_) => false,
        };
        if negative {
            return Err(ParseError::expected("a non-negative gap"));
        }
        Ok(Self(value))
    }
}

impl fmt::Display for Gap {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// The horizontal and vertical distances in CSS 2.1's separated-border
/// model. Percentages and negative values are invalid for `border-spacing`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TableBorderSpacing {
    pub horizontal: Length,
    pub vertical: Length,
}

impl TableBorderSpacing {
    pub const ZERO: Self = Self {
        horizontal: Length::ZERO,
        vertical: Length::ZERO,
    };
}

impl FromStr for TableBorderSpacing {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let values = input.split_ascii_whitespace().collect::<Vec<_>>();
        let (horizontal, vertical) = match values.as_slice() {
            [horizontal] => {
                let horizontal = horizontal.parse::<Length>()?;
                (horizontal, horizontal)
            },
            [horizontal, vertical] => (horizontal.parse::<Length>()?, vertical.parse::<Length>()?),
            _ => return Err(ParseError::expected("one or two non-negative lengths")),
        };
        if horizontal.value < 0.0 || vertical.value < 0.0 {
            return Err(ParseError::expected("one or two non-negative lengths"));
        }
        Ok(Self {
            horizontal,
            vertical,
        })
    }
}

impl fmt::Display for TableBorderSpacing {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.horizontal.fmt(formatter)?;
        if self.vertical != self.horizontal {
            write!(formatter, " {}", self.vertical)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlexFactor(f32);

impl FlexFactor {
    pub const ZERO: Self = Self(0.0);
    pub const ONE: Self = Self(1.0);

    pub const fn value(self) -> f32 {
        self.0
    }
}

impl FromStr for FlexFactor {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        input
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(Self)
            .ok_or_else(|| ParseError::expected("a non-negative flex factor"))
    }
}

impl fmt::Display for FlexFactor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&format_number(self.0))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Order(i32);

impl Order {
    pub const ZERO: Self = Self(0);

    pub const fn value(self) -> i32 {
        self.0
    }
}

impl FromStr for Order {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        input
            .trim()
            .parse::<i32>()
            .map(Self)
            .map_err(|_| ParseError::expected("an integer order"))
    }
}

impl fmt::Display for Order {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// A CSS spacing value, with `normal` represented explicitly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Spacing {
    Normal,
    Length(LengthPercentage),
}

impl FromStr for Spacing {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        if input.trim().eq_ignore_ascii_case("normal") {
            Ok(Self::Normal)
        } else {
            input
                .parse::<LengthPercentage>()
                .map(Self::Length)
                .map_err(|_| ParseError::expected("normal or a length"))
        }
    }
}

impl fmt::Display for Spacing {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Normal => formatter.write_str("normal"),
            Self::Length(length) => length.fmt(formatter),
        }
    }
}

pub type TextDecorationColor = super::Color;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineHeight {
    Normal,
    Number(f32),
    Value(LengthPercentage),
}

impl FromStr for LineHeight {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("normal") {
            return Ok(Self::Normal);
        }
        if let Ok(number) = input.parse::<f32>()
            && number.is_finite()
            && number >= 0.0
        {
            return Ok(Self::Number(number));
        }
        input.parse::<LengthPercentage>().map(Self::Value)
    }
}

impl fmt::Display for LineHeight {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Normal => formatter.write_str("normal"),
            Self::Number(number) => formatter.write_str(&format_number(*number)),
            Self::Value(value) => value.fmt(formatter),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Opacity(f32);

impl Opacity {
    pub const ONE: Self = Self(1.0);

    pub const fn from_value(value: f32) -> Self {
        Self(value.clamp(0.0, 1.0))
    }

    pub const fn value(self) -> f32 {
        self.0
    }
}

impl FromStr for Opacity {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        let value = if let Some(percentage) = input.strip_suffix('%') {
            percentage.trim().parse::<f32>().map(|value| value / 100.0)
        } else {
            input.parse::<f32>()
        }
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| ParseError::expected("a finite opacity number or percentage"))?;
        Ok(Self(value.clamp(0.0, 1.0)))
    }
}

impl fmt::Display for Opacity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&format_number(self.0))
    }
}

/// The bounded 2D individual `rotate` property.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rotate {
    None,
    Angle(f32),
    /// An angle expression whose bases are not all constant, retained until
    /// cascade supplies the element's tree context.
    Deferred(MathLengthPercentage),
}

impl Rotate {
    pub const fn radians(self) -> Option<f32> {
        match self {
            Self::None | Self::Deferred(_) => None,
            Self::Angle(value) => Some(value),
        }
    }

    pub(super) fn resolve_math(self, environment: RelativeLengthEnvironment) -> Self {
        match self {
            Self::Deferred(math) => {
                let resolved = math.resolve_relative(environment);
                resolved
                    .resolved_px()
                    .map_or(Self::Deferred(resolved), Self::Angle)
            },
            value => value,
        }
    }
}

impl FromStr for Rotate {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("none") {
            return Ok(Self::None);
        }
        parse_angle(input)
            .or_else(|_| super::calc::parse_angle(input))
            .map(Self::Angle)
            .or_else(|error| {
                super::calc::parse_angle_math(input)
                    .map(Self::Deferred)
                    .map_err(|_| error)
            })
    }
}

impl fmt::Display for Rotate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => formatter.write_str("none"),
            Self::Angle(value) => write!(formatter, "{}rad", format_number(*value)),
            Self::Deferred(math) => math.fmt(formatter),
        }
    }
}

/// The bounded uniform individual `scale` property.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scale {
    None,
    Uniform(f32),
    /// The [`Rotate::Deferred`] twin for number-valued expressions.
    Deferred(MathLengthPercentage),
}

impl Scale {
    pub const fn factor(self) -> Option<f32> {
        match self {
            Self::None | Self::Deferred(_) => None,
            Self::Uniform(value) => Some(value),
        }
    }

    pub(super) fn resolve_math(self, environment: RelativeLengthEnvironment) -> Self {
        match self {
            Self::Deferred(math) => {
                let resolved = math.resolve_relative(environment);
                resolved
                    .resolved_px()
                    .map_or(Self::Deferred(resolved), Self::Uniform)
            },
            value => value,
        }
    }
}

impl FromStr for Scale {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("none") {
            return Ok(Self::None);
        }
        let value = input
            .strip_suffix('%')
            .and_then(|value| value.trim().parse::<f32>().ok())
            .map(|value| value / 100.0)
            .or_else(|| input.parse::<f32>().ok())
            .filter(|value| value.is_finite())
            .map(Ok)
            .unwrap_or_else(|| super::calc::parse_number(input));
        match value {
            Ok(value) => Ok(Self::Uniform(value)),
            Err(error) => super::calc::parse_number_math(input)
                .map(Self::Deferred)
                .map_err(|_| error),
        }
    }
}

impl fmt::Display for Scale {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => formatter.write_str("none"),
            Self::Uniform(value) => formatter.write_str(&format_number(*value)),
            Self::Deferred(math) => math.fmt(formatter),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Transform {
    None,
    Functions(Vec<TransformFunction>),
}

/// A bounded single-layer CSS box shadow.
#[derive(Clone, Debug, PartialEq)]
pub enum BoxShadow {
    None,
    Value(BoxShadowValue),
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoxShadowValue {
    pub inset: bool,
    pub offset_x: Length,
    pub offset_y: Length,
    pub blur_radius: Length,
    pub spread_radius: Length,
    pub color: ComputedColor,
}

impl BoxShadow {
    /// Interpolate the bounded single-shadow form used by the retained paint
    /// lane. Matching length units and inset mode are required; `none`, mixed
    /// units, and mode changes stay discrete until the shadow-list ratchet.
    pub fn interpolate(&self, other: &Self, progress: f32) -> Self {
        let progress = progress.clamp(0.0, 1.0);
        let value = match (self, other) {
            (Self::Value(from), Self::Value(to)) if from.inset == to.inset => {
                interpolate_box_shadow_value(from, to, progress).map(Self::Value)
            },
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

    /// Interpolate a matching shadow after resolving the two color endpoints
    /// under their respective used-value contexts.
    pub fn interpolate_used(
        &self,
        other: &Self,
        from_context: UsedColorContext,
        to_context: UsedColorContext,
        progress: f32,
    ) -> Self {
        let progress = progress.clamp(0.0, 1.0);
        let value = match (self, other) {
            (Self::Value(from), Self::Value(to)) if from.inset == to.inset => {
                interpolate_box_shadow_value(from, to, progress).map(|mut value| {
                    value.color =
                        from.color
                            .interpolate_used(&to.color, from_context, to_context, progress);
                    Self::Value(value)
                })
            },
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

fn interpolate_box_shadow_value(
    from: &BoxShadowValue,
    to: &BoxShadowValue,
    progress: f32,
) -> Option<BoxShadowValue> {
    Some(BoxShadowValue {
        inset: from.inset,
        offset_x: interpolate_length(from.offset_x, to.offset_x, progress)?,
        offset_y: interpolate_length(from.offset_y, to.offset_y, progress)?,
        blur_radius: interpolate_length(from.blur_radius, to.blur_radius, progress)?,
        spread_radius: interpolate_length(from.spread_radius, to.spread_radius, progress)?,
        color: from.color.interpolate(&to.color, progress),
    })
}

impl FromStr for BoxShadow {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("none") {
            return Ok(Self::None);
        }
        let mut inset = false;
        let mut color = None;
        let mut lengths = Vec::new();
        for component in shadow_components(input) {
            if component.eq_ignore_ascii_case("inset") {
                if inset {
                    return Err(ParseError::expected("one inset box-shadow keyword"));
                }
                inset = true;
            } else if let Ok(value) = component.parse::<ComputedColor>() {
                if color.replace(value).is_some() {
                    return Err(ParseError::expected("one box-shadow color"));
                }
            } else if let Ok(value) = component.parse::<Length>() {
                lengths.push(value);
            } else {
                return Err(ParseError::expected("a bounded box-shadow component"));
            }
        }
        if !(2..=4).contains(&lengths.len()) {
            return Err(ParseError::expected("two through four box-shadow lengths"));
        }
        Ok(Self::Value(BoxShadowValue {
            inset,
            offset_x: lengths[0],
            offset_y: lengths[1],
            blur_radius: lengths.get(2).copied().unwrap_or(Length::ZERO),
            spread_radius: lengths.get(3).copied().unwrap_or(Length::ZERO),
            color: color.unwrap_or(ComputedColor::CURRENT_COLOR),
        }))
    }
}

impl fmt::Display for BoxShadow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => formatter.write_str("none"),
            Self::Value(value) => {
                write!(
                    formatter,
                    "{} {} {} {} {}",
                    value.offset_x,
                    value.offset_y,
                    value.blur_radius,
                    value.spread_radius,
                    value.color
                )?;
                if value.inset {
                    formatter.write_str(" inset")?;
                }
                Ok(())
            },
        }
    }
}

fn shadow_components(input: &str) -> Vec<&str> {
    let mut components = Vec::new();
    let mut start = None;
    let mut depth = 0_u32;
    for (index, ch) in input.char_indices() {
        match ch {
            '(' => {
                start.get_or_insert(index);
                depth += 1;
            },
            ')' => depth = depth.saturating_sub(1),
            _ if ch.is_ascii_whitespace() && depth == 0 => {
                if let Some(offset) = start.take() {
                    components.push(&input[offset..index]);
                }
            },
            _ => {
                start.get_or_insert(index);
            },
        }
    }
    if let Some(offset) = start {
        components.push(&input[offset..]);
    }
    components
}

impl Transform {
    pub fn functions(&self) -> Option<&[TransformFunction]> {
        match self {
            Self::None => None,
            Self::Functions(functions) => Some(functions),
        }
    }

    pub const fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    /// Interpolate matching transform primitives directly, then normalize any
    /// mismatched suffix (including `none`) through a decomposed 2D matrix.
    pub fn interpolate(&self, other: &Self, progress: f32) -> Self {
        let progress = progress.clamp(0.0, 1.0);
        if matches!((self, other), (Self::None, Self::None)) {
            return Self::None;
        }

        let from = self.functions().unwrap_or(&[]);
        let to = other.functions().unwrap_or(&[]);
        let mut functions = Vec::new();
        let mut prefix = 0;
        while let (Some(from), Some(to)) = (from.get(prefix), to.get(prefix)) {
            let Some(value) = interpolate_transform_function(*from, *to, progress) else {
                break;
            };
            functions.push(value);
            prefix += 1;
        }
        if prefix == from.len() && prefix == to.len() {
            return Self::Functions(functions);
        }

        let from_matrix = Matrix2D::from_absolute_functions(&from[prefix..], 16.0);
        let to_matrix = Matrix2D::from_absolute_functions(&to[prefix..], 16.0);
        match from_matrix
            .zip(to_matrix)
            .and_then(|(from, to)| from.interpolate(to, progress))
        {
            Some(matrix) => {
                functions.push(TransformFunction::Matrix(matrix));
                Self::Functions(functions)
            },
            None if progress < 0.5 => self.clone(),
            None => other.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TransformFunction {
    Translate(LengthPercentage, LengthPercentage),
    Scale(f32, f32),
    Rotate(f32),
    Skew(f32, f32),
    Matrix(Matrix2D),
}

fn interpolate_transform_function(
    from: TransformFunction,
    to: TransformFunction,
    progress: f32,
) -> Option<TransformFunction> {
    let scalar = |from: f32, to: f32| from + (to - from) * progress;
    Some(match (from, to) {
        (
            TransformFunction::Translate(from_x, from_y),
            TransformFunction::Translate(to_x, to_y),
        ) => TransformFunction::Translate(
            from_x.interpolate(to_x, progress),
            from_y.interpolate(to_y, progress),
        ),
        (TransformFunction::Scale(from_x, from_y), TransformFunction::Scale(to_x, to_y)) => {
            TransformFunction::Scale(scalar(from_x, to_x), scalar(from_y, to_y))
        },
        (TransformFunction::Rotate(from), TransformFunction::Rotate(to)) => {
            TransformFunction::Rotate(scalar(from, to))
        },
        (TransformFunction::Skew(from_x, from_y), TransformFunction::Skew(to_x, to_y)) => {
            TransformFunction::Skew(scalar(from_x, to_x), scalar(from_y, to_y))
        },
        (TransformFunction::Matrix(from), TransformFunction::Matrix(to)) => {
            TransformFunction::Matrix(from.interpolate(to, progress)?)
        },
        _ => return None,
    })
}

fn interpolate_length(from: Length, to: Length, progress: f32) -> Option<Length> {
    (from.unit == to.unit).then_some(Length {
        value: from.value + (to.value - from.value) * progress,
        unit: from.unit,
    })
}

impl FromStr for Transform {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let mut input = input.trim();
        if input.eq_ignore_ascii_case("none") {
            return Ok(Self::None);
        }

        let mut functions = Vec::new();
        while !input.is_empty() {
            let open = input
                .find('(')
                .ok_or_else(|| ParseError::expected("a supported 2D transform function"))?;
            let name = input[..open].trim().to_ascii_lowercase();
            if name.is_empty() || name.split_ascii_whitespace().count() != 1 {
                return Err(ParseError::expected("a supported 2D transform function"));
            }
            let tail = &input[open + 1..];
            let close = tail
                .find(')')
                .ok_or_else(|| ParseError::expected("a closed 2D transform function"))?;
            let arguments = tail[..close]
                .split(|ch: char| ch == ',' || ch.is_ascii_whitespace())
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>();
            functions.push(parse_transform_function(&name, &arguments)?);
            input = tail[close + 1..].trim_start();
        }
        if functions.is_empty() {
            Err(ParseError::expected("none or a 2D transform list"))
        } else {
            Ok(Self::Functions(functions))
        }
    }
}

fn parse_transform_function(
    name: &str,
    arguments: &[&str],
) -> Result<TransformFunction, ParseError> {
    let length_percentage = |value: &str| value.parse::<LengthPercentage>();
    let number = |value: &str| {
        value
            .parse::<f32>()
            .ok()
            .filter(|value| value.is_finite())
            .ok_or_else(|| ParseError::expected("a finite transform number"))
    };
    match (name, arguments) {
        ("translate", [x]) => Ok(TransformFunction::Translate(
            length_percentage(x)?,
            LengthPercentage::ZERO,
        )),
        ("translate", [x, y]) => Ok(TransformFunction::Translate(
            length_percentage(x)?,
            length_percentage(y)?,
        )),
        ("translatex", [x]) => Ok(TransformFunction::Translate(
            length_percentage(x)?,
            LengthPercentage::ZERO,
        )),
        ("translatey", [y]) => Ok(TransformFunction::Translate(
            LengthPercentage::ZERO,
            length_percentage(y)?,
        )),
        ("scale", [both]) => {
            let both = number(both)?;
            Ok(TransformFunction::Scale(both, both))
        },
        ("scale", [x, y]) => Ok(TransformFunction::Scale(number(x)?, number(y)?)),
        ("scalex", [x]) => Ok(TransformFunction::Scale(number(x)?, 1.0)),
        ("scaley", [y]) => Ok(TransformFunction::Scale(1.0, number(y)?)),
        ("rotate", [angle]) => Ok(TransformFunction::Rotate(parse_angle(angle)?)),
        ("skew", [x]) => Ok(TransformFunction::Skew(parse_angle(x)?, 0.0)),
        ("skew", [x, y]) => Ok(TransformFunction::Skew(parse_angle(x)?, parse_angle(y)?)),
        ("skewx", [x]) => Ok(TransformFunction::Skew(parse_angle(x)?, 0.0)),
        ("skewy", [y]) => Ok(TransformFunction::Skew(0.0, parse_angle(y)?)),
        ("matrix", [a, b, c, d, e, f]) => Ok(TransformFunction::Matrix(Matrix2D::new(
            number(a)?,
            number(b)?,
            number(c)?,
            number(d)?,
            number(e)?,
            number(f)?,
        ))),
        _ => Err(ParseError::expected(
            "translate, scale, rotate, skew, or matrix",
        )),
    }
}

fn parse_angle(input: &str) -> Result<f32, ParseError> {
    let lower = input.trim().to_ascii_lowercase();
    let (number, factor) = if let Some(value) = lower.strip_suffix("deg") {
        (value, std::f32::consts::PI / 180.0)
    } else if let Some(value) = lower.strip_suffix("grad") {
        (value, std::f32::consts::PI / 200.0)
    } else if let Some(value) = lower.strip_suffix("rad") {
        (value, 1.0)
    } else if let Some(value) = lower.strip_suffix("turn") {
        (value, std::f32::consts::TAU)
    } else if lower == "0" || lower == "+0" || lower == "-0" {
        ("0", 1.0)
    } else {
        return Err(ParseError::expected("a deg, rad, or turn angle"));
    };
    number
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
        .map(|value| value * factor)
        .ok_or_else(|| ParseError::expected("a finite angle"))
}

impl fmt::Display for Transform {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => formatter.write_str("none"),
            Self::Functions(functions) => {
                for (index, function) in functions.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(" ")?;
                    }
                    function.fmt(formatter)?;
                }
                Ok(())
            },
        }
    }
}

impl fmt::Display for TransformFunction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Translate(x, y) => write!(formatter, "translate({x}, {y})"),
            Self::Scale(x, y) => write!(
                formatter,
                "scale({}, {})",
                format_number(*x),
                format_number(*y)
            ),
            Self::Rotate(radians) => write!(formatter, "rotate({}rad)", format_number(*radians)),
            Self::Skew(x, y) => write!(
                formatter,
                "skew({}rad, {}rad)",
                format_number(*x),
                format_number(*y)
            ),
            Self::Matrix(matrix) => matrix.fmt(formatter),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Padding(pub LengthPercentage);

impl Padding {
    pub const ZERO: Self = Self(LengthPercentage::ZERO);
}

impl FromStr for Padding {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let value = input.parse::<LengthPercentage>()?;
        let negative = match value {
            LengthPercentage::Zero => false,
            LengthPercentage::Length(length) => length.value < 0.0,
            LengthPercentage::Percentage(value) => value < 0.0,
            LengthPercentage::Calc(_) => false,
            LengthPercentage::Math(_) => false,
        };
        if negative {
            return Err(ParseError::expected("a non-negative padding"));
        }
        Ok(Self(value))
    }
}

impl fmt::Display for Padding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextDecorationLine(u8);

impl TextDecorationLine {
    pub const NONE: Self = Self(0);
    const UNDERLINE: u8 = 1 << 0;
    const OVERLINE: u8 = 1 << 1;
    const LINE_THROUGH: u8 = 1 << 2;
    const BLINK: u8 = 1 << 3;

    pub const fn contains_underline(self) -> bool {
        self.0 & Self::UNDERLINE != 0
    }
}

impl FromStr for TextDecorationLine {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("none") {
            return Ok(Self::NONE);
        }
        let mut flags = 0;
        for keyword in input.split_ascii_whitespace() {
            let flag = match keyword.to_ascii_lowercase().as_str() {
                "underline" => Self::UNDERLINE,
                "overline" => Self::OVERLINE,
                "line-through" => Self::LINE_THROUGH,
                "blink" => Self::BLINK,
                _ => return Err(ParseError::expected("text-decoration-line keywords")),
            };
            if flags & flag != 0 {
                return Err(ParseError::expected("unique text-decoration-line keywords"));
            }
            flags |= flag;
        }
        if flags == 0 {
            return Err(ParseError::expected("text-decoration-line keywords"));
        }
        Ok(Self(flags))
    }
}

impl fmt::Display for TextDecorationLine {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if *self == Self::NONE {
            return formatter.write_str("none");
        }
        let mut first = true;
        for (flag, name) in [
            (Self::UNDERLINE, "underline"),
            (Self::OVERLINE, "overline"),
            (Self::LINE_THROUGH, "line-through"),
            (Self::BLINK, "blink"),
        ] {
            if self.0 & flag == 0 {
                continue;
            }
            if !first {
                formatter.write_str(" ")?;
            }
            formatter.write_str(name)?;
            first = false;
        }
        Ok(())
    }
}

// `Eq` is unavailable now that a z-index can retain a float-bearing math
// program; nothing in the tree needs more than `PartialEq`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ZIndex {
    Auto,
    Integer(i32),
    /// The [`Rotate::Deferred`] twin for integer-valued expressions.
    Deferred(MathLengthPercentage),
}

/// Round a resolved number to the integer a `<integer>` property stores.
fn rounded_integer(value: f32) -> Option<i32> {
    let rounded = (value + 0.5).floor();
    (rounded >= i32::MIN as f32 && rounded <= i32::MAX as f32).then_some(rounded as i32)
}

impl ZIndex {
    pub(super) fn resolve_math(self, environment: RelativeLengthEnvironment) -> Self {
        match self {
            Self::Deferred(math) => {
                let resolved = math.resolve_relative(environment);
                resolved
                    .resolved_px()
                    .and_then(rounded_integer)
                    .map_or(Self::Deferred(resolved), Self::Integer)
            },
            value => value,
        }
    }
}

impl FromStr for ZIndex {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        if input.trim().eq_ignore_ascii_case("auto") {
            return Ok(Self::Auto);
        }
        if let Some(integer) = input.trim().parse::<i32>().ok().or_else(|| {
            input
                .contains('(')
                .then(|| super::calc::parse_number(input).ok())
                .flatten()
                .and_then(rounded_integer)
        }) {
            return Ok(Self::Integer(integer));
        }
        input
            .contains('(')
            .then(|| super::calc::parse_number_math(input).ok())
            .flatten()
            .map(Self::Deferred)
            .ok_or_else(|| ParseError::expected("auto or an integer"))
    }
}

impl fmt::Display for ZIndex {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => formatter.write_str("auto"),
            Self::Integer(value) => value.fmt(formatter),
            Self::Deferred(math) => math.fmt(formatter),
        }
    }
}
