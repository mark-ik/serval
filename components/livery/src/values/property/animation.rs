//! Animation and transition values: durations and delays, animation
//! names, the transition property list, and timing functions.

use super::*;

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
                crate::values::calc::parse_time(trimmed)
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
            .or_else(|| crate::values::calc::parse_time(trimmed).ok())
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
