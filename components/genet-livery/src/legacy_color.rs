/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! HTML's legacy color parser.
//!
//! Harvested from Stylo's `servo::attr` implementation and reduced to
//! Livery's absolute computed-color representation.

use cssparser::color::parse_named_color;
use livery::values::{Color, ComputedColor};

const HTML_SPACE_CHARACTERS: &[char] = &['\u{0009}', '\u{000a}', '\u{000c}', '\u{000d}', ' '];

/// Parse a color using HTML's legacy attribute algorithm rather than CSS's
/// color grammar.
pub(crate) fn parse_legacy_color(mut input: &str) -> Option<ComputedColor> {
    if input.is_empty() {
        return None;
    }

    input = input.trim_matches(HTML_SPACE_CHARACTERS);
    if input.eq_ignore_ascii_case("transparent") {
        return None;
    }

    if let Ok((red, green, blue)) = parse_named_color(input) {
        return Some(Color::srgb8(red, green, blue, 1.0).into());
    }

    if input.len() == 4 {
        if let (Some(b'#'), Some(red), Some(green), Some(blue)) = (
            input.as_bytes().first().copied(),
            hex(input.as_bytes()[1] as char),
            hex(input.as_bytes()[2] as char),
            hex(input.as_bytes()[3] as char),
        ) {
            return Some(Color::srgb8(red * 17, green * 17, blue * 17, 1.0).into());
        }
    }

    let replaced = input
        .chars()
        .flat_map(|character| {
            if u32::from(character) > 0xffff {
                ['0', '0'].into_iter().collect::<Vec<_>>()
            } else {
                vec![character]
            }
        })
        .take(128)
        .collect::<String>();
    let input = replaced.strip_prefix('#').unwrap_or(&replaced);
    let mut digits = input
        .chars()
        .map(|character| {
            if hex(character).is_some() {
                character as u8
            } else {
                b'0'
            }
        })
        .collect::<Vec<_>>();

    while digits.is_empty() || digits.len() % 3 != 0 {
        digits.push(b'0');
    }

    let mut channel_length = digits.len() / 3;
    let (mut red, mut green, mut blue) = (
        &digits[..channel_length],
        &digits[channel_length..channel_length * 2],
        &digits[channel_length * 2..],
    );

    if channel_length > 8 {
        red = &red[channel_length - 8..];
        green = &green[channel_length - 8..];
        blue = &blue[channel_length - 8..];
        channel_length = 8;
    }

    while channel_length > 2 && red[0] == b'0' && green[0] == b'0' && blue[0] == b'0' {
        red = &red[1..];
        green = &green[1..];
        blue = &blue[1..];
        channel_length -= 1;
    }

    Some(
        Color::srgb8(
            hex_string(red)?,
            hex_string(green)?,
            hex_string(blue)?,
            1.0,
        )
        .into(),
    )
}

fn hex(character: char) -> Option<u8> {
    match character {
        '0'..='9' => Some(character as u8 - b'0'),
        'a'..='f' => Some(character as u8 - b'a' + 10),
        'A'..='F' => Some(character as u8 - b'A' + 10),
        _ => None,
    }
}

fn hex_string(string: &[u8]) -> Option<u8> {
    match string {
        [] => None,
        [digit] => hex(*digit as char),
        [upper, lower, ..] => Some((hex(*upper as char)? << 4) | hex(*lower as char)?),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn srgb8(input: &str) -> Option<(u8, u8, u8, u8)> {
        parse_legacy_color(input).and_then(|color| color.to_srgb8())
    }

    #[test]
    fn accepts_named_shorthand_and_legacy_garbage_forms() {
        assert_eq!(srgb8(" red "), Some((255, 0, 0, 255)));
        assert_eq!(srgb8("#0f8"), Some((0, 255, 136, 255)));
        assert_eq!(srgb8("chucknorris"), Some((192, 0, 0, 255)));
        assert_eq!(srgb8("\u{1f600}"), Some((0, 0, 0, 255)));
    }

    #[test]
    fn rejects_only_the_algorithms_explicit_failures() {
        assert_eq!(srgb8(""), None);
        assert_eq!(srgb8("transparent"), None);
        assert_eq!(srgb8("  "), Some((0, 0, 0, 255)));
    }
}
