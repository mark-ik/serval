//! Text and font values: transform, hanging punctuation, indent, tab
//! size, vertical alignment, the font family and feature vocabulary,
//! size, weight, and line height.

use super::*;

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
pub(crate) fn split_top_level(input: &str) -> Vec<&str> {
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
        Normal => "normal",
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
