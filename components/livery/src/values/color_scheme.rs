// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Element `color-scheme` values and used-scheme selection.

use std::{fmt, str::FromStr};

use super::ParseError;

/// One color scheme the bounded host can prefer or support.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ColorScheme {
    Light,
    Dark,
}

impl fmt::Display for ColorScheme {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Light => "light",
            Self::Dark => "dark",
        })
    }
}

impl FromStr for ColorScheme {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input.trim().to_ascii_lowercase().as_str() {
            "light" => Ok(Self::Light),
            "dark" => Ok(Self::Dark),
            _ => Err(ParseError::expected("light or dark")),
        }
    }
}

/// The inherited `color-scheme` property.
///
/// The list retains author preference order. `only` constrains user-agent
/// adjustment but does not change the selected supported scheme in this
/// bounded host lane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ColorSchemeList {
    Normal,
    Supported {
        schemes: Vec<ColorScheme>,
        only: bool,
    },
}

impl ColorSchemeList {
    pub const NORMAL: Self = Self::Normal;

    /// Select this element's used scheme without changing the host media
    /// preference. A supported preferred scheme wins; otherwise the author's
    /// first supported scheme does.
    pub fn used_scheme(&self, host_preference: ColorScheme) -> ColorScheme {
        match self {
            Self::Normal => host_preference,
            Self::Supported { schemes, .. } => schemes
                .iter()
                .copied()
                .find(|scheme| *scheme == host_preference)
                .unwrap_or_else(|| schemes[0]),
        }
    }

    pub fn only(&self) -> bool {
        matches!(self, Self::Supported { only: true, .. })
    }

    pub fn schemes(&self) -> Option<&[ColorScheme]> {
        match self {
            Self::Normal => None,
            Self::Supported { schemes, .. } => Some(schemes),
        }
    }
}

impl FromStr for ColorSchemeList {
    type Err = ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let words = input.split_ascii_whitespace().collect::<Vec<_>>();
        if words.len() == 1 && words[0].eq_ignore_ascii_case("normal") {
            return Ok(Self::Normal);
        }
        if words.is_empty() || words.iter().any(|word| word.eq_ignore_ascii_case("normal")) {
            return Err(ParseError::expected("normal or supported color schemes"));
        }

        let mut only = false;
        let mut schemes = Vec::with_capacity(words.len());
        for word in words {
            if word.eq_ignore_ascii_case("only") {
                if only {
                    return Err(ParseError::expected("one only keyword"));
                }
                only = true;
                continue;
            }
            let scheme = word.parse::<ColorScheme>()?;
            if schemes.contains(&scheme) {
                return Err(ParseError::expected("distinct supported color schemes"));
            }
            schemes.push(scheme);
        }
        if schemes.is_empty() {
            return Err(ParseError::expected("one or more supported color schemes"));
        }
        Ok(Self::Supported { schemes, only })
    }
}

impl fmt::Display for ColorSchemeList {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Normal => formatter.write_str("normal"),
            Self::Supported { schemes, only } => {
                if *only {
                    formatter.write_str("only ")?;
                }
                for (index, scheme) in schemes.iter().enumerate() {
                    if index != 0 {
                        formatter.write_str(" ")?;
                    }
                    scheme.fmt(formatter)?;
                }
                Ok(())
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_scheme_keeps_author_order_but_prefers_the_host_when_supported() {
        let value = "only light dark".parse::<ColorSchemeList>().unwrap();
        assert_eq!(value.to_string(), "only light dark");
        assert!(value.only());
        assert_eq!(value.used_scheme(ColorScheme::Dark), ColorScheme::Dark);

        let light_only = "light".parse::<ColorSchemeList>().unwrap();
        assert_eq!(
            light_only.used_scheme(ColorScheme::Dark),
            ColorScheme::Light
        );
    }
}
